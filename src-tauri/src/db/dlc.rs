//! Persistencia y consultas del contenido adicional (DLC) de la biblioteca.
//!
//! La tabla `game_dlc` (migración 020) guarda una fila por pareja
//! `(app_id, dlc_app_id)`. Las invariantes que sostiene este módulo son:
//!
//! - **Idempotencia**: repetir una importación con los mismos datos no cambia
//!   nada salvo `updated_at`.
//! - **El marcado manual manda**: `hidden` nunca lo toca una importación, y
//!   `owned` solo puede subir de `0` a `1` desde la tienda o desde la evidencia
//!   local. Bajarlo es una decisión explícita de la persona usuaria a través de
//!   [`set_dlc_owned`].
//! - **Nada se inventa**: un DLC cuya ficha no publica Steam se conserva con el
//!   AppID que sí declaró el juego base y `metadata_status = 'unavailable'`; los
//!   agregados de [`dlc_summary`] jamás suman precios desconocidos ni mezclan
//!   monedas distintas.

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Techo de DLC que Vindexa persiste por juego.
pub const MAX_DLC_PER_GAME: usize = 200;
/// Estados admitidos por el `CHECK` de la migración 020.
pub const DLC_METADATA_STATUSES: [&str; 4] = ["pending", "success", "unavailable", "failed"];
/// Ventana de arrendamiento de un candidato reclamado por la cola de refresco.
/// Si el proceso se cierra a mitad, el candidato vuelve a estar disponible
/// pasados estos minutos.
const REFRESH_LEASE_MINUTES: i64 = 5;
const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 1_000;
const MAX_URL_CHARS: usize = 1_000;
const MAX_PRICE_CENTS: u32 = 10_000_000;
const MAX_LIST_ROWS: usize = 500;

/// Un DLC tal y como lo entrega la capa de obtención antes de persistirse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedDlc {
    pub dlc_app_id: u32,
    pub title: String,
    pub capsule_url: Option<String>,
    pub header_url: Option<String>,
    pub short_description: Option<String>,
    pub release_date: Option<String>,
    pub is_free: bool,
    pub price_cents: Option<u32>,
    pub currency: Option<String>,
    pub discount_percent: Option<u8>,
    /// Evidencia de propiedad. Solo promociona: nunca revierte un `owned = 1`
    /// previo, venga de una evidencia anterior o del marcado manual.
    pub owned: bool,
    /// `Some` cuando hay evidencia local concluyente sobre la instalación;
    /// `None` cuando no la hay y el valor guardado debe conservarse.
    pub installed: Option<bool>,
    pub metadata_status: String,
    pub position: u32,
}

impl ImportedDlc {
    /// DLC del que solo se conoce el AppID declarado por el juego base.
    pub fn pending(dlc_app_id: u32, position: u32) -> Self {
        Self {
            dlc_app_id,
            title: String::new(),
            capsule_url: None,
            header_url: None,
            short_description: None,
            release_date: None,
            is_free: false,
            price_cents: None,
            currency: None,
            discount_percent: None,
            owned: false,
            installed: None,
            metadata_status: "pending".to_string(),
            position,
        }
    }

    /// DLC cuya ficha Steam no publica, pero cuyo AppID sí declaró el juego base.
    pub fn unavailable(dlc_app_id: u32, position: u32) -> Self {
        Self {
            metadata_status: "unavailable".to_string(),
            ..Self::pending(dlc_app_id, position)
        }
    }
}

/// Un DLC ya persistido, listo para la interfaz.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDlc {
    pub app_id: u32,
    pub dlc_app_id: u32,
    pub title: String,
    pub capsule_url: Option<String>,
    pub header_url: Option<String>,
    pub short_description: Option<String>,
    pub release_date: Option<String>,
    pub is_free: bool,
    pub price_cents: Option<u32>,
    pub currency: Option<String>,
    pub discount_percent: Option<u8>,
    pub owned: bool,
    pub installed: bool,
    pub hidden: bool,
    pub metadata_status: String,
    pub metadata_fetched_at: Option<String>,
    pub position: u32,
    pub updated_at: String,
}

/// Filtro de listado. Los valores son una allowlist interna: el SQL nunca se
/// construye con texto que venga de la interfaz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DlcFilter {
    /// Todo lo que no esté oculto (vista por defecto).
    #[default]
    Visible,
    /// Todo, ocultos incluidos.
    All,
    Owned,
    NotOwned,
    Installed,
    Hidden,
}

impl DlcFilter {
    /// Traduce el filtro que llega de la interfaz. `None` equivale a `Visible`.
    pub fn parse(value: Option<&str>) -> AppResult<Self> {
        match value.map(str::trim).unwrap_or("") {
            "" | "visible" => Ok(Self::Visible),
            "all" => Ok(Self::All),
            "owned" => Ok(Self::Owned),
            "notOwned" => Ok(Self::NotOwned),
            "installed" => Ok(Self::Installed),
            "hidden" => Ok(Self::Hidden),
            _ => Err(AppError::validation(
                "El filtro de contenido adicional no es válido.",
            )),
        }
    }

    fn predicate(self) -> &'static str {
        match self {
            Self::Visible => "hidden = 0",
            Self::All => "1 = 1",
            Self::Owned => "owned = 1 AND hidden = 0",
            Self::NotOwned => "owned = 0 AND hidden = 0",
            Self::Installed => "installed = 1 AND hidden = 0",
            Self::Hidden => "hidden = 1",
        }
    }
}

/// Recuento de lo que hizo una importación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcImportSummary {
    pub app_id: u32,
    pub received: u32,
    pub inserted: u32,
    pub updated: u32,
    /// Elementos del lote con ficha completa (`metadata_status = 'success'`).
    pub with_metadata: u32,
    /// Elementos del lote sin ficha todavía o sin ficha publicada por Steam.
    pub without_metadata: u32,
    /// Totales de la tabla tras aplicar el lote.
    pub owned: u32,
    pub installed: u32,
}

/// Agregados del contenido adicional de un juego.
///
/// El valor pendiente se expresa siempre en **una sola** moneda: la dominante
/// entre los DLC pendientes con precio conocido. Lo que no se sabe se cuenta
/// aparte y nunca se suma.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcSummary {
    pub app_id: u32,
    pub total: u32,
    pub owned: u32,
    pub installed: u32,
    pub hidden: u32,
    pub free: u32,
    /// DLC ni poseídos ni ocultos.
    pub pending: u32,
    /// Suma de los precios finales de los pendientes en la moneda dominante.
    pub pending_value_cents: Option<u64>,
    pub pending_value_currency: Option<String>,
    /// Cuántos pendientes entran en `pendingValueCents`.
    pub pending_counted: u32,
    /// Pendientes de pago cuyo precio Steam no ha publicado.
    pub pending_unknown_price: u32,
    /// Pendientes con precio en una moneda distinta de la dominante.
    pub pending_other_currency: u32,
}

/// Resultado de una actualización explícita del contenido adicional de un juego.
///
/// Vive junto al resto del modelo de DLC —como `NewsRefreshReport` vive junto al
/// de Discovery— porque combina lo que dijo la tienda con lo que quedó guardado.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcRefreshReport {
    pub app_id: u32,
    /// Cuántos DLC declaró la ficha oficial del juego base.
    pub declared: usize,
    /// `true` si Steam declaró más DLC de los que Vindexa guarda por juego.
    pub truncated: bool,
    /// Fichas individuales obtenidas en esta pasada.
    pub fetched_details: usize,
    /// Fichas que Steam no publica.
    pub unavailable_details: usize,
    /// Fichas que fallaron y se reintentarán más adelante.
    pub failed_details: usize,
    /// Fichas que siguen sin completarse al agotarse el presupuesto de la pasada.
    pub pending_details: usize,
    /// Motivo por el que no se pudo comprobar la propiedad de los DLC en local.
    /// `null` cuando el manifiesto sí se leyó.
    pub ownership_evidence_gap: Option<String>,
    pub ownership_evidence_explanation: Option<String>,
    pub imported: DlcImportSummary,
    pub summary: DlcSummary,
}

/// Candidato de la cola de refresco de fichas de DLC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DlcRefreshCandidate {
    pub app_id: u32,
    pub dlc_app_id: u32,
    pub position: u32,
}

/// Guarda un lote de DLC de un juego de forma transaccional e idempotente.
///
/// Conserva siempre `hidden`, nunca degrada `owned` y solo pisa los campos
/// descriptivos cuando el elemento trae ficha completa; así una segunda pasada
/// del catálogo (que solo conoce AppID y posición) no borra lo ya obtenido.
///
/// No borra filas: un DLC retirado del listado de la tienda puede seguir siendo
/// contenido real que la persona usuaria posee.
pub fn upsert_dlc_batch(
    connection: &mut Connection,
    app_id: u32,
    items: &[ImportedDlc],
) -> AppResult<DlcImportSummary> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    if items.len() > MAX_DLC_PER_GAME {
        return Err(AppError::new(
            "dlc_batch_limit",
            format!("Vindexa guarda como máximo {MAX_DLC_PER_GAME} DLC por juego."),
        ));
    }
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        validate_imported(app_id, item)?;
        if !seen.insert(item.dlc_app_id) {
            return Err(AppError::validation(
                "El lote de contenido adicional contiene un AppID duplicado.",
            ));
        }
    }

    let transaction = connection.transaction()?;
    let base_exists = transaction
        .query_row("SELECT 1 FROM games WHERE app_id = ?1", [app_id], |_| Ok(()))
        .optional()?
        .is_some();
    if !base_exists {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }

    let mut inserted = 0_u32;
    let mut updated = 0_u32;
    let mut with_metadata = 0_u32;
    {
        let mut exists = transaction
            .prepare_cached("SELECT 1 FROM game_dlc WHERE app_id = ?1 AND dlc_app_id = ?2")?;
        let mut upsert = transaction.prepare_cached(
            "INSERT INTO game_dlc(
                 app_id, dlc_app_id, title, capsule_url, header_url, short_description,
                 release_date, is_free, price_cents, currency, discount_percent,
                 owned, installed, hidden, metadata_status, metadata_fetched_at,
                 position, updated_at
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6,
                 ?7, ?8, ?9, ?10, ?11,
                 ?12, COALESCE(?13, 0), 0, ?14,
                 CASE WHEN ?14 = 'pending' THEN NULL
                      ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END,
                 ?15, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )
             ON CONFLICT(app_id, dlc_app_id) DO UPDATE SET
                 title = CASE WHEN ?14 = 'success' THEN ?3 ELSE game_dlc.title END,
                 capsule_url = CASE WHEN ?14 = 'success'
                     THEN COALESCE(?4, game_dlc.capsule_url) ELSE game_dlc.capsule_url END,
                 header_url = CASE WHEN ?14 = 'success'
                     THEN COALESCE(?5, game_dlc.header_url) ELSE game_dlc.header_url END,
                 short_description = CASE WHEN ?14 = 'success'
                     THEN COALESCE(?6, game_dlc.short_description)
                     ELSE game_dlc.short_description END,
                 release_date = CASE WHEN ?14 = 'success'
                     THEN COALESCE(?7, game_dlc.release_date) ELSE game_dlc.release_date END,
                 is_free = CASE WHEN ?14 = 'success' THEN ?8 ELSE game_dlc.is_free END,
                 price_cents = CASE WHEN ?14 = 'success' THEN ?9 ELSE game_dlc.price_cents END,
                 currency = CASE WHEN ?14 = 'success' THEN ?10 ELSE game_dlc.currency END,
                 discount_percent = CASE WHEN ?14 = 'success'
                     THEN ?11 ELSE game_dlc.discount_percent END,
                 owned = MAX(game_dlc.owned, ?12),
                 installed = COALESCE(?13, game_dlc.installed),
                 metadata_status = CASE WHEN ?14 = 'pending'
                     THEN game_dlc.metadata_status ELSE ?14 END,
                 metadata_fetched_at = CASE WHEN ?14 = 'pending'
                     THEN game_dlc.metadata_fetched_at
                     ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now') END,
                 position = ?15,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        for item in items {
            let already_there = exists
                .query_row(params![app_id, item.dlc_app_id], |_| Ok(()))
                .optional()?
                .is_some();
            upsert.execute(params![
                app_id,
                item.dlc_app_id,
                item.title.trim(),
                item.capsule_url,
                item.header_url,
                item.short_description,
                item.release_date,
                i64::from(item.is_free),
                item.price_cents,
                item.currency,
                item.discount_percent,
                i64::from(item.owned),
                item.installed.map(i64::from),
                item.metadata_status,
                item.position,
            ])?;
            if already_there {
                updated += 1;
            } else {
                inserted += 1;
            }
            if item.metadata_status == "success" {
                with_metadata += 1;
            }
        }
    }
    let (owned, installed) = transaction.query_row(
        "SELECT COALESCE(SUM(owned), 0), COALESCE(SUM(installed), 0)
           FROM game_dlc WHERE app_id = ?1",
        [app_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )?;
    transaction.commit()?;

    let received = items.len() as u32;
    Ok(DlcImportSummary {
        app_id,
        received,
        inserted,
        updated,
        with_metadata,
        without_metadata: received - with_metadata,
        owned: count_to_u32(owned, "DLC en propiedad")?,
        installed: count_to_u32(installed, "DLC instalados")?,
    })
}

/// Lista el contenido adicional de un juego según el filtro pedido.
pub fn list_dlc(
    connection: &Connection,
    app_id: u32,
    filter: DlcFilter,
) -> AppResult<Vec<GameDlc>> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    let mut statement = connection.prepare(&format!(
        "SELECT app_id, dlc_app_id, title, capsule_url, header_url, short_description,
                release_date, is_free, price_cents, currency, discount_percent,
                owned, installed, hidden, metadata_status, metadata_fetched_at,
                position, updated_at
           FROM game_dlc
          WHERE app_id = ?1 AND {}
          ORDER BY position ASC,
                   release_date IS NULL ASC,
                   release_date ASC,
                   title COLLATE NOCASE ASC,
                   dlc_app_id ASC
          LIMIT ?2",
        filter.predicate()
    ))?;
    let items = statement
        .query_map(params![app_id, MAX_LIST_ROWS as i64], read_game_dlc)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(items)
}

/// Devuelve un DLC concreto.
pub fn get_dlc(connection: &Connection, app_id: u32, dlc_app_id: u32) -> AppResult<GameDlc> {
    connection
        .query_row(
            "SELECT app_id, dlc_app_id, title, capsule_url, header_url, short_description,
                    release_date, is_free, price_cents, currency, discount_percent,
                    owned, installed, hidden, metadata_status, metadata_fetched_at,
                    position, updated_at
               FROM game_dlc WHERE app_id = ?1 AND dlc_app_id = ?2",
            params![app_id, dlc_app_id],
            read_game_dlc,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("Ese contenido adicional ya no está registrado."))
}

/// Marca manualmente si se posee un DLC. Es la única vía para revertir un
/// `owned = 1`, porque ninguna actualización desde la tienda lo degrada.
pub fn set_dlc_owned(
    connection: &Connection,
    app_id: u32,
    dlc_app_id: u32,
    owned: bool,
) -> AppResult<GameDlc> {
    set_flag(connection, app_id, dlc_app_id, "owned", owned)
}

/// Oculta o vuelve a mostrar un DLC. Una importación nunca toca esta marca.
pub fn set_dlc_hidden(
    connection: &Connection,
    app_id: u32,
    dlc_app_id: u32,
    hidden: bool,
) -> AppResult<GameDlc> {
    set_flag(connection, app_id, dlc_app_id, "hidden", hidden)
}

/// Marca manualmente un DLC como instalado. La evidencia local del manifiesto
/// sobrescribe este valor en la siguiente importación concluyente, porque el
/// manifiesto sí es autoridad sobre lo que hay en disco.
pub fn set_dlc_installed(
    connection: &Connection,
    app_id: u32,
    dlc_app_id: u32,
    installed: bool,
) -> AppResult<GameDlc> {
    set_flag(connection, app_id, dlc_app_id, "installed", installed)
}

fn set_flag(
    connection: &Connection,
    app_id: u32,
    dlc_app_id: u32,
    column: &'static str,
    value: bool,
) -> AppResult<GameDlc> {
    if app_id == 0 || dlc_app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    // `column` procede de una allowlist interna; nunca de la interfaz.
    debug_assert!(matches!(column, "owned" | "hidden" | "installed"));
    let changed = connection.execute(
        &format!(
            "UPDATE game_dlc
                SET {column} = ?3,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE app_id = ?1 AND dlc_app_id = ?2"
        ),
        params![app_id, dlc_app_id, i64::from(value)],
    )?;
    if changed != 1 {
        return Err(AppError::not_found(
            "Ese contenido adicional ya no está registrado.",
        ));
    }
    get_dlc(connection, app_id, dlc_app_id)
}

/// Agregados del contenido adicional de un juego.
pub fn dlc_summary(connection: &Connection, app_id: u32) -> AppResult<DlcSummary> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    let counts = connection.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(owned), 0),
                COALESCE(SUM(installed), 0),
                COALESCE(SUM(hidden), 0),
                COALESCE(SUM(is_free), 0),
                COALESCE(SUM(owned = 0 AND hidden = 0), 0)
           FROM game_dlc WHERE app_id = ?1",
        [app_id],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        },
    )?;

    // Solo entra en el valor pendiente lo que es de pago, no se posee, no está
    // oculto y tiene precio y moneda publicados.
    let priced = connection
        .query_row(
            "SELECT currency, COUNT(*), COALESCE(SUM(price_cents), 0)
               FROM game_dlc
              WHERE app_id = ?1 AND owned = 0 AND hidden = 0 AND is_free = 0
                AND price_cents IS NOT NULL AND currency IS NOT NULL AND currency <> ''
              GROUP BY currency
              ORDER BY COUNT(*) DESC, currency ASC
              LIMIT 1",
            [app_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    let priced_total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM game_dlc
          WHERE app_id = ?1 AND owned = 0 AND hidden = 0 AND is_free = 0
            AND price_cents IS NOT NULL AND currency IS NOT NULL AND currency <> ''",
        [app_id],
        |row| row.get(0),
    )?;
    let unknown_price: i64 = connection.query_row(
        "SELECT COUNT(*) FROM game_dlc
          WHERE app_id = ?1 AND owned = 0 AND hidden = 0 AND is_free = 0
            AND (price_cents IS NULL OR currency IS NULL OR currency = '')",
        [app_id],
        |row| row.get(0),
    )?;

    let (currency, counted, value) = match priced {
        Some((currency, counted, value)) => (Some(currency), counted, Some(value)),
        None => (None, 0, None),
    };
    Ok(DlcSummary {
        app_id,
        total: count_to_u32(counts.0, "DLC registrados")?,
        owned: count_to_u32(counts.1, "DLC en propiedad")?,
        installed: count_to_u32(counts.2, "DLC instalados")?,
        hidden: count_to_u32(counts.3, "DLC ocultos")?,
        free: count_to_u32(counts.4, "DLC gratuitos")?,
        pending: count_to_u32(counts.5, "DLC pendientes")?,
        pending_value_cents: value
            .map(|value| u64::try_from(value).unwrap_or_default())
            .filter(|_| counted > 0),
        pending_value_currency: currency,
        pending_counted: count_to_u32(counted, "DLC pendientes con precio")?,
        pending_unknown_price: count_to_u32(unknown_price, "DLC sin precio conocido")?,
        pending_other_currency: count_to_u32(
            priced_total.saturating_sub(counted),
            "DLC en otra moneda",
        )?,
    })
}

/// Reclama los DLC cuya ficha toca refrescar.
///
/// El vencimiento por estado replica la política de `metadata_queue`: siete días
/// para una ficha correcta, veinticuatro horas para una ausencia y dos horas
/// para un fallo. Reclamar deja la fila en `pending` con `metadata_fetched_at`
/// actual, que actúa de arrendamiento: otro lote no la vuelve a coger hasta
/// pasados [`REFRESH_LEASE_MINUTES`], y un cierre inesperado la libera sola.
/// Los datos ya obtenidos siguen intactos mientras dura el arrendamiento: solo
/// cambia el estado, así que la interfaz nunca se queda sin ficha que mostrar.
///
/// La migración 020 no guarda intentos, así que el aplazamiento es por
/// vencimiento de estado, no exponencial por fila. El backoff exponencial vive
/// en la capa de red (`steam::dlc::retry_delay_seconds`) durante la ráfaga.
pub fn claim_dlc_refresh_candidates(
    connection: &mut Connection,
    limit: usize,
) -> AppResult<Vec<DlcRefreshCandidate>> {
    if limit == 0 || limit > 8 {
        return Err(AppError::validation(
            "El lote de contenido adicional debe contener entre 1 y 8 fichas.",
        ));
    }
    claim_candidates(connection, None, limit)
}

/// Igual que [`claim_dlc_refresh_candidates`], pero limitado a un juego.
///
/// Lo usa la actualización explícita de un juego concreto, que sí puede gastar
/// un presupuesto mayor porque la persona usuaria está esperando el resultado.
pub fn claim_game_dlc_refresh_candidates(
    connection: &mut Connection,
    app_id: u32,
    limit: usize,
) -> AppResult<Vec<DlcRefreshCandidate>> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    if limit == 0 || limit > MAX_DLC_PER_GAME {
        return Err(AppError::validation(
            "El lote de contenido adicional de un juego está fuera de rango.",
        ));
    }
    claim_candidates(connection, Some(app_id), limit)
}

fn claim_candidates(
    connection: &mut Connection,
    app_id: Option<u32>,
    limit: usize,
) -> AppResult<Vec<DlcRefreshCandidate>> {
    // `?2 IS NULL` mantiene el mismo SQL para la cola global y para la de un
    // juego concreto, sin construir el filtro con texto interpolado.
    let transaction = connection.transaction()?;
    let candidates = {
        let mut statement = transaction.prepare(&format!(
            "SELECT app_id, dlc_app_id, position
               FROM game_dlc
              WHERE hidden = 0
                AND (?2 IS NULL OR app_id = ?2)
                AND (
                    (metadata_status = 'pending' AND (
                        metadata_fetched_at IS NULL
                        OR datetime(metadata_fetched_at)
                           <= datetime('now', '-{REFRESH_LEASE_MINUTES} minutes')
                    ))
                    OR (metadata_status = 'success'
                        AND datetime(metadata_fetched_at) < datetime('now', '-7 days'))
                    OR (metadata_status = 'unavailable'
                        AND datetime(metadata_fetched_at) < datetime('now', '-1 day'))
                    OR (metadata_status = 'failed'
                        AND datetime(metadata_fetched_at) < datetime('now', '-2 hours'))
                )
              ORDER BY metadata_fetched_at IS NOT NULL ASC,
                       datetime(metadata_fetched_at) ASC,
                       app_id ASC, position ASC, dlc_app_id ASC
              LIMIT ?1"
        ))?;
        statement
            .query_map(params![limit as i64, app_id], |row| {
                Ok(DlcRefreshCandidate {
                    app_id: row.get(0)?,
                    dlc_app_id: row.get(1)?,
                    position: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for candidate in &candidates {
        transaction.execute(
            "UPDATE game_dlc
                SET metadata_status = 'pending',
                    metadata_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE app_id = ?1 AND dlc_app_id = ?2",
            params![candidate.app_id, candidate.dlc_app_id],
        )?;
    }
    transaction.commit()?;
    Ok(candidates)
}

/// Registra un intento de refresco fallido de una ficha concreta.
pub fn mark_dlc_metadata_failed(
    connection: &Connection,
    app_id: u32,
    dlc_app_id: u32,
) -> AppResult<()> {
    let changed = connection.execute(
        "UPDATE game_dlc
            SET metadata_status = 'failed',
                metadata_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1 AND dlc_app_id = ?2",
        params![app_id, dlc_app_id],
    )?;
    if changed != 1 {
        return Err(AppError::not_found(
            "Ese contenido adicional ya no está registrado.",
        ));
    }
    Ok(())
}

fn read_game_dlc(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameDlc> {
    Ok(GameDlc {
        app_id: row.get(0)?,
        dlc_app_id: row.get(1)?,
        title: row.get(2)?,
        capsule_url: row.get(3)?,
        header_url: row.get(4)?,
        short_description: row.get(5)?,
        release_date: row.get(6)?,
        is_free: row.get::<_, i64>(7)? != 0,
        price_cents: row.get(8)?,
        currency: row.get(9)?,
        discount_percent: row.get(10)?,
        owned: row.get::<_, i64>(11)? != 0,
        installed: row.get::<_, i64>(12)? != 0,
        hidden: row.get::<_, i64>(13)? != 0,
        metadata_status: row.get(14)?,
        metadata_fetched_at: row.get(15)?,
        position: row.get(16)?,
        updated_at: row.get(17)?,
    })
}

fn validate_imported(app_id: u32, item: &ImportedDlc) -> AppResult<()> {
    if item.dlc_app_id == 0 || item.dlc_app_id == app_id {
        return Err(AppError::validation(
            "El AppID del contenido adicional no es válido.",
        ));
    }
    if !DLC_METADATA_STATUSES.contains(&item.metadata_status.as_str()) {
        return Err(AppError::validation(
            "El estado de la ficha de contenido adicional no es válido.",
        ));
    }
    if item.metadata_status == "success" && item.title.trim().is_empty() {
        return Err(AppError::validation(
            "Un contenido adicional con ficha completa necesita un título.",
        ));
    }
    if item.title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::validation(
            "El título del contenido adicional es demasiado largo.",
        ));
    }
    if item
        .short_description
        .as_ref()
        .is_some_and(|value| value.chars().count() > MAX_DESCRIPTION_CHARS)
    {
        return Err(AppError::validation(
            "La descripción del contenido adicional es demasiado larga.",
        ));
    }
    for url in [item.capsule_url.as_ref(), item.header_url.as_ref()]
        .into_iter()
        .flatten()
    {
        if url.chars().count() > MAX_URL_CHARS || !url.starts_with("https://") {
            return Err(AppError::validation(
                "La ilustración del contenido adicional no procede de un origen aceptado.",
            ));
        }
    }
    if item
        .release_date
        .as_ref()
        .is_some_and(|value| value.len() != 10 || value.bytes().filter(|b| *b == b'-').count() != 2)
    {
        return Err(AppError::validation(
            "La fecha de lanzamiento del contenido adicional no es válida.",
        ));
    }
    if item.price_cents.is_some_and(|value| value > MAX_PRICE_CENTS) {
        return Err(AppError::validation(
            "El precio del contenido adicional no es válido.",
        ));
    }
    if item.currency.as_ref().is_some_and(|value| {
        value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_uppercase())
    }) {
        return Err(AppError::validation(
            "La moneda del contenido adicional no es válida.",
        ));
    }
    if item.price_cents.is_some() != item.currency.is_some() {
        return Err(AppError::validation(
            "Un precio de contenido adicional necesita moneda, y viceversa.",
        ));
    }
    if item.discount_percent.is_some_and(|value| value > 100) {
        return Err(AppError::validation(
            "El descuento del contenido adicional no es válido.",
        ));
    }
    if item.position as usize >= MAX_DLC_PER_GAME {
        return Err(AppError::validation(
            "La posición del contenido adicional está fuera de rango.",
        ));
    }
    Ok(())
}

fn count_to_u32(value: i64, label: &str) -> AppResult<u32> {
    u32::try_from(value).map_err(|_| {
        AppError::new(
            "dlc_count",
            format!("El recuento de {label} guardado no es válido."),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use rusqlite::Connection;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("migrar");
        // La cascada de `game_dlc` depende de las claves foráneas, que la
        // aplicación activa en cada conexión.
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (255710, 'Cities: Skylines')",
                [],
            )
            .expect("crear juego base");
        connection
    }

    fn detailed(dlc_app_id: u32, position: u32, price: Option<(u32, &str)>) -> ImportedDlc {
        ImportedDlc {
            dlc_app_id,
            title: format!("DLC {dlc_app_id}"),
            capsule_url: Some(format!(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/{dlc_app_id}/capsule_616x353.jpg"
            )),
            header_url: None,
            short_description: Some("Contenido adicional oficial.".into()),
            release_date: Some("2015-09-24".into()),
            is_free: false,
            price_cents: price.map(|(cents, _)| cents),
            currency: price.map(|(_, currency)| currency.to_string()),
            discount_percent: Some(0),
            owned: false,
            installed: None,
            metadata_status: "success".into(),
            position,
        }
    }

    #[test]
    fn upserting_the_same_batch_twice_changes_nothing_but_the_timestamp() {
        let mut connection = database();
        let batch = vec![
            detailed(346791, 0, Some((649, "EUR"))),
            detailed(359051, 1, Some((1299, "EUR"))),
        ];

        let first = upsert_dlc_batch(&mut connection, 255710, &batch).expect("primera importación");
        assert_eq!(first.inserted, 2);
        assert_eq!(first.updated, 0);
        assert_eq!(first.with_metadata, 2);
        assert_eq!(first.without_metadata, 0);

        let second =
            upsert_dlc_batch(&mut connection, 255710, &batch).expect("segunda importación");
        assert_eq!(second.inserted, 0);
        assert_eq!(second.updated, 2);
        assert_eq!(second.received, 2);

        let rows = list_dlc(&connection, 255710, DlcFilter::All).expect("listar");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].price_cents, Some(649));
        assert_eq!(rows[0].currency.as_deref(), Some("EUR"));
    }

    #[test]
    fn a_refresh_never_undoes_what_the_person_marked_by_hand() {
        let mut connection = database();
        upsert_dlc_batch(&mut connection, 255710, &[detailed(346791, 0, None)])
            .expect("importar DLC");

        set_dlc_owned(&connection, 255710, 346791, true).expect("marcar poseído");
        let hidden = set_dlc_hidden(&connection, 255710, 346791, true).expect("ocultar");
        assert!(hidden.owned);
        assert!(hidden.hidden);

        // La tienda vuelve sin ninguna evidencia de propiedad.
        upsert_dlc_batch(&mut connection, 255710, &[detailed(346791, 0, Some((999, "EUR")))])
            .expect("refrescar desde la tienda");
        let after = get_dlc(&connection, 255710, 346791).expect("releer");
        assert!(after.owned, "el marcado manual de propiedad se conserva");
        assert!(after.hidden, "una importación nunca cambia la ocultación");
        assert_eq!(after.price_cents, Some(999), "el precio sí se actualiza");

        // Y solo la acción explícita puede revertirlo.
        let reverted = set_dlc_owned(&connection, 255710, 346791, false).expect("desmarcar");
        assert!(!reverted.owned);
    }

    #[test]
    fn a_catalog_pass_does_not_erase_a_sheet_already_obtained() {
        let mut connection = database();
        upsert_dlc_batch(&mut connection, 255710, &[detailed(346791, 0, Some((649, "EUR")))])
            .expect("importar ficha completa");

        upsert_dlc_batch(&mut connection, 255710, &[ImportedDlc::pending(346791, 4)])
            .expect("reimportar catálogo");
        let after = get_dlc(&connection, 255710, 346791).expect("releer");
        assert_eq!(after.title, "DLC 346791");
        assert_eq!(after.price_cents, Some(649));
        assert_eq!(after.metadata_status, "success");
        assert_eq!(after.position, 4, "la posición del catálogo sí manda");
    }

    #[test]
    fn a_dlc_without_a_published_sheet_is_kept_without_inventing_data() {
        let mut connection = database();
        let summary = upsert_dlc_batch(
            &mut connection,
            255710,
            &[ImportedDlc::unavailable(346791, 0)],
        )
        .expect("importar ausencia");
        assert_eq!(summary.with_metadata, 0);
        assert_eq!(summary.without_metadata, 1);

        let row = get_dlc(&connection, 255710, 346791).expect("releer");
        assert_eq!(row.title, "");
        assert_eq!(row.metadata_status, "unavailable");
        assert!(row.metadata_fetched_at.is_some());
        assert_eq!(row.price_cents, None);
    }

    #[test]
    fn local_evidence_updates_installation_but_absence_of_evidence_does_not() {
        let mut connection = database();
        let mut installed = detailed(346791, 0, None);
        installed.installed = Some(true);
        installed.owned = true;
        upsert_dlc_batch(&mut connection, 255710, &[installed]).expect("importar con evidencia");
        assert!(get_dlc(&connection, 255710, 346791).expect("releer").installed);

        // Sin evidencia local, la instalación conocida se conserva.
        upsert_dlc_batch(&mut connection, 255710, &[detailed(346791, 0, None)])
            .expect("importar sin evidencia");
        assert!(get_dlc(&connection, 255710, 346791).expect("releer").installed);

        // Con evidencia concluyente en contra, sí se retira.
        let mut removed = detailed(346791, 0, None);
        removed.installed = Some(false);
        upsert_dlc_batch(&mut connection, 255710, &[removed]).expect("importar desinstalado");
        let row = get_dlc(&connection, 255710, 346791).expect("releer");
        assert!(!row.installed);
        assert!(row.owned, "la propiedad demostrada no se retira");
    }

    #[test]
    fn filters_and_orders_by_position_then_release_date_then_title() {
        let mut connection = database();
        let mut batch = vec![
            detailed(300, 0, None),
            detailed(200, 0, None),
            detailed(100, 1, None),
            detailed(400, 2, None),
        ];
        batch[0].release_date = Some("2020-01-02".into());
        batch[1].release_date = Some("2020-01-01".into());
        batch[2].release_date = None;
        batch[3].release_date = Some("2019-01-01".into());
        upsert_dlc_batch(&mut connection, 255710, &batch).expect("importar");

        let visible = list_dlc(&connection, 255710, DlcFilter::Visible).expect("listar");
        assert_eq!(
            visible.iter().map(|row| row.dlc_app_id).collect::<Vec<_>>(),
            vec![200, 300, 100, 400]
        );

        set_dlc_owned(&connection, 255710, 200, true).expect("marcar poseído");
        set_dlc_installed(&connection, 255710, 200, true).expect("marcar instalado");
        set_dlc_hidden(&connection, 255710, 400, true).expect("ocultar");

        let owned = list_dlc(&connection, 255710, DlcFilter::Owned).expect("listar poseídos");
        assert_eq!(
            owned.iter().map(|row| row.dlc_app_id).collect::<Vec<_>>(),
            vec![200]
        );
        let not_owned =
            list_dlc(&connection, 255710, DlcFilter::NotOwned).expect("listar no poseídos");
        assert_eq!(
            not_owned.iter().map(|row| row.dlc_app_id).collect::<Vec<_>>(),
            vec![300, 100]
        );
        let installed =
            list_dlc(&connection, 255710, DlcFilter::Installed).expect("listar instalados");
        assert_eq!(installed.len(), 1);
        let hidden = list_dlc(&connection, 255710, DlcFilter::Hidden).expect("listar ocultos");
        assert_eq!(
            hidden.iter().map(|row| row.dlc_app_id).collect::<Vec<_>>(),
            vec![400]
        );
        assert_eq!(
            list_dlc(&connection, 255710, DlcFilter::All)
                .expect("listar todos")
                .len(),
            4
        );
        assert_eq!(
            list_dlc(&connection, 255710, DlcFilter::Visible)
                .expect("listar visibles")
                .len(),
            3
        );
    }

    #[test]
    fn the_filter_comes_from_an_allowlist_and_rejects_anything_else() {
        assert_eq!(DlcFilter::parse(None).expect("por defecto"), DlcFilter::Visible);
        assert_eq!(
            DlcFilter::parse(Some("notOwned")).expect("no poseídos"),
            DlcFilter::NotOwned
        );
        let error = DlcFilter::parse(Some("owned' OR 1=1 --")).expect_err("rechazar inyección");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn the_pending_value_never_adds_up_what_it_does_not_know() {
        let mut connection = database();
        let mut unknown_price = detailed(500, 3, None);
        unknown_price.discount_percent = None;
        let mut free = detailed(600, 4, None);
        free.is_free = true;
        upsert_dlc_batch(
            &mut connection,
            255710,
            &[
                detailed(100, 0, Some((649, "EUR"))),
                detailed(200, 1, Some((1299, "EUR"))),
                detailed(300, 2, Some((999, "USD"))),
                unknown_price,
                free,
                detailed(700, 5, Some((100, "EUR"))),
            ],
        )
        .expect("importar");
        set_dlc_owned(&connection, 255710, 700, true).expect("marcar poseído");

        let summary = dlc_summary(&connection, 255710).expect("resumen");
        assert_eq!(summary.total, 6);
        assert_eq!(summary.owned, 1);
        assert_eq!(summary.free, 1);
        assert_eq!(summary.pending, 5);
        assert_eq!(summary.pending_value_currency.as_deref(), Some("EUR"));
        assert_eq!(summary.pending_value_cents, Some(649 + 1299));
        assert_eq!(summary.pending_counted, 2);
        assert_eq!(summary.pending_unknown_price, 1, "el DLC sin precio no suma");
        assert_eq!(summary.pending_other_currency, 1, "el DLC en USD no suma");

        // Ocultar y poseer sacan al DLC del valor pendiente.
        set_dlc_hidden(&connection, 255710, 200, true).expect("ocultar");
        let after = dlc_summary(&connection, 255710).expect("resumen tras ocultar");
        assert_eq!(after.hidden, 1);
        assert_eq!(after.pending, 4);
        assert_eq!(after.pending_value_cents, Some(649));
    }

    #[test]
    fn a_game_without_dlc_reports_an_honest_empty_summary() {
        let connection = database();
        let summary = dlc_summary(&connection, 255710).expect("resumen vacío");
        assert_eq!(summary.total, 0);
        assert_eq!(summary.pending_value_cents, None);
        assert_eq!(summary.pending_value_currency, None);
        assert_eq!(summary.pending_counted, 0);
    }

    #[test]
    fn the_refresh_queue_leases_candidates_and_honors_the_status_ttl() {
        let mut connection = database();
        upsert_dlc_batch(
            &mut connection,
            255710,
            &[
                ImportedDlc::pending(100, 0),
                ImportedDlc::pending(200, 1),
                detailed(300, 2, None),
            ],
        )
        .expect("importar");

        let claimed = claim_dlc_refresh_candidates(&mut connection, 8).expect("reclamar");
        assert_eq!(
            claimed,
            vec![
                DlcRefreshCandidate {
                    app_id: 255710,
                    dlc_app_id: 100,
                    position: 0
                },
                DlcRefreshCandidate {
                    app_id: 255710,
                    dlc_app_id: 200,
                    position: 1
                },
            ],
            "una ficha recién obtenida no se vuelve a pedir"
        );

        assert!(
            claim_dlc_refresh_candidates(&mut connection, 8)
                .expect("reclamar de nuevo")
                .is_empty(),
            "el arrendamiento impide reclamar dos veces lo mismo"
        );

        // Un cierre inesperado libera el arrendamiento pasado su plazo.
        connection
            .execute(
                "UPDATE game_dlc SET metadata_fetched_at = datetime('now', '-10 minutes')
                  WHERE dlc_app_id = 100",
                [],
            )
            .expect("simular cierre");
        let recovered = claim_dlc_refresh_candidates(&mut connection, 8).expect("recuperar");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].dlc_app_id, 100);

        // Un fallo se reintenta a las dos horas, no antes.
        mark_dlc_metadata_failed(&connection, 255710, 100).expect("marcar fallo");
        assert!(
            claim_dlc_refresh_candidates(&mut connection, 8)
                .expect("reclamar tras fallo")
                .is_empty()
        );
        connection
            .execute(
                "UPDATE game_dlc SET metadata_fetched_at = datetime('now', '-3 hours')
                  WHERE dlc_app_id = 100",
                [],
            )
            .expect("hacer vencer el fallo");
        assert_eq!(
            claim_dlc_refresh_candidates(&mut connection, 8)
                .expect("reclamar vencido")
                .len(),
            1
        );

        // Una ficha correcta caduca a los siete días.
        connection
            .execute(
                "UPDATE game_dlc SET metadata_fetched_at = datetime('now', '-8 days')
                  WHERE dlc_app_id = 300",
                [],
            )
            .expect("hacer vencer la ficha");
        let stale = claim_dlc_refresh_candidates(&mut connection, 8).expect("reclamar caducado");
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].dlc_app_id, 300);
    }

    #[test]
    fn hidden_dlc_never_consume_the_refresh_budget() {
        let mut connection = database();
        upsert_dlc_batch(&mut connection, 255710, &[ImportedDlc::pending(100, 0)])
            .expect("importar");
        set_dlc_hidden(&connection, 255710, 100, true).expect("ocultar");
        assert!(
            claim_dlc_refresh_candidates(&mut connection, 4)
                .expect("reclamar")
                .is_empty()
        );
    }

    #[test]
    fn a_per_game_refresh_never_steals_candidates_from_another_game() {
        let mut connection = database();
        connection
            .execute("INSERT INTO games(app_id, title) VALUES (620, 'Portal 2')", [])
            .expect("crear segundo juego");
        upsert_dlc_batch(&mut connection, 255710, &[ImportedDlc::pending(100, 0)])
            .expect("importar del primero");
        upsert_dlc_batch(&mut connection, 620, &[ImportedDlc::pending(200, 0)])
            .expect("importar del segundo");

        let claimed = claim_game_dlc_refresh_candidates(&mut connection, 620, MAX_DLC_PER_GAME)
            .expect("reclamar del segundo");
        assert_eq!(
            claimed,
            vec![DlcRefreshCandidate {
                app_id: 620,
                dlc_app_id: 200,
                position: 0
            }]
        );
        // El del primer juego sigue disponible para la cola global.
        let global = claim_dlc_refresh_candidates(&mut connection, 8).expect("reclamar global");
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].app_id, 255710);

        assert_eq!(
            claim_game_dlc_refresh_candidates(&mut connection, 620, 0)
                .expect_err("rechazar presupuesto vacío")
                .code,
            "validation"
        );
        assert_eq!(
            claim_game_dlc_refresh_candidates(&mut connection, 0, 4)
                .expect_err("rechazar AppID cero")
                .code,
            "validation"
        );
    }

    #[test]
    fn deleting_the_base_game_removes_its_dlc() {
        let mut connection = database();
        upsert_dlc_batch(
            &mut connection,
            255710,
            &[detailed(346791, 0, None), detailed(359051, 1, None)],
        )
        .expect("importar");
        assert_eq!(
            list_dlc(&connection, 255710, DlcFilter::All)
                .expect("listar")
                .len(),
            2
        );

        connection
            .execute("DELETE FROM games WHERE app_id = 255710", [])
            .expect("borrar juego base");
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM game_dlc", [], |row| row.get(0))
            .expect("contar DLC huérfanos");
        assert_eq!(remaining, 0, "la cascada de la migración 020 los retira");
    }

    #[test]
    fn a_batch_for_a_game_outside_the_library_is_refused_before_writing() {
        let mut connection = database();
        let error = upsert_dlc_batch(&mut connection, 999_999, &[detailed(1, 0, None)])
            .expect_err("rechazar juego ausente");
        assert_eq!(error.code, "not_found");
    }

    #[test]
    fn the_batch_is_bounded_and_validated_before_touching_sqlite() {
        let mut connection = database();
        let too_many = (1..=(MAX_DLC_PER_GAME as u32 + 1))
            .map(|id| ImportedDlc::pending(id, 0))
            .collect::<Vec<_>>();
        assert_eq!(
            upsert_dlc_batch(&mut connection, 255710, &too_many)
                .expect_err("rechazar lote enorme")
                .code,
            "dlc_batch_limit"
        );

        for invalid in [
            ImportedDlc::pending(0, 0),
            ImportedDlc::pending(255710, 0),
            ImportedDlc {
                metadata_status: "inventado".into(),
                ..ImportedDlc::pending(1, 0)
            },
            ImportedDlc {
                metadata_status: "success".into(),
                ..ImportedDlc::pending(1, 0)
            },
            ImportedDlc {
                capsule_url: Some("http://evil.example/capsule.jpg".into()),
                ..detailed(1, 0, None)
            },
            ImportedDlc {
                price_cents: Some(500),
                currency: None,
                ..detailed(1, 0, None)
            },
            ImportedDlc {
                currency: Some("euro".into()),
                price_cents: Some(500),
                ..detailed(1, 0, None)
            },
            ImportedDlc {
                discount_percent: Some(200),
                ..detailed(1, 0, None)
            },
            ImportedDlc {
                release_date: Some("hace poco".into()),
                ..detailed(1, 0, None)
            },
        ] {
            assert_eq!(
                upsert_dlc_batch(&mut connection, 255710, std::slice::from_ref(&invalid))
                    .expect_err("rechazar entrada no válida")
                    .code,
                "validation",
                "no debió aceptarse: {invalid:?}"
            );
        }

        let duplicated = vec![detailed(1, 0, None), detailed(1, 1, None)];
        assert_eq!(
            upsert_dlc_batch(&mut connection, 255710, &duplicated)
                .expect_err("rechazar duplicado")
                .code,
            "validation"
        );

        let written: i64 = connection
            .query_row("SELECT COUNT(*) FROM game_dlc", [], |row| row.get(0))
            .expect("contar DLC");
        assert_eq!(written, 0, "ningún lote inválido llegó a escribir");
    }

    #[test]
    fn manual_marks_require_an_existing_row_and_a_valid_app_id() {
        let connection = database();
        assert_eq!(
            set_dlc_owned(&connection, 255710, 346791, true)
                .expect_err("rechazar DLC inexistente")
                .code,
            "not_found"
        );
        assert_eq!(
            set_dlc_hidden(&connection, 0, 346791, true)
                .expect_err("rechazar AppID cero")
                .code,
            "validation"
        );
        assert_eq!(
            claim_dlc_refresh_candidates(&mut database(), 0)
                .expect_err("rechazar lote vacío")
                .code,
            "validation"
        );
    }
}
