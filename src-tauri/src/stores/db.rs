//! Persistencia de las tiendas externas sobre `external_store_accounts` y
//! `external_games` (migraciones 025 y 027).
//!
//! Sigue el patrón de `crate::db::family_catalog`: funciones libres que reciben
//! la `Connection`, escritura en una única transacción y structs `Serialize` con
//! `rename_all = "camelCase"` para el contrato con la interfaz.
//!
//! # Cómo se conserva una corrección manual
//!
//! La procedencia del emparejado vive en la columna `match_source` que añadió la
//! migración 027:
//!
//! * `manual` ⟺ **la decisión la tomó una persona**, tanto si fue «éste es el
//!   juego» (`matched_app_id` con valor) como «éstos no son el mismo juego»
//!   (`matched_app_id` a `NULL`). Ambas se respetan en todo reescaneo posterior.
//! * `automatic` procede del algoritmo de [`crate::stores::matching`], y se
//!   recalcula libremente.
//!
//! `match_confidence` acompaña a la decisión pero ya no la clasifica: las
//! manuales se guardan con [`MANUAL_CONFIDENCE`] para que la interfaz pueda
//! ordenarlas junto a las demás sin casos especiales.

use crate::db::rich_metadata::{DrmEvidence, DrmState};
use crate::error::{AppError, AppResult};
use crate::models::LOCAL_APP_ID_BASE;
use crate::stores::matching::{MatchCandidate, SteamTitleIndex};
use crate::stores::{
    DiscoveredGame, ExternalStore, MAX_DISCOVERED_GAMES, ScanStatus, StoreOrigin, StoreScan,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

/// Confianza que acompaña a una decisión tomada por una persona. Ver el
/// comentario del módulo.
pub const MANUAL_CONFIDENCE: f64 = 1.0;

/// Valores admitidos por el `CHECK` de `external_games.match_source`.
const MATCH_SOURCE_AUTOMATIC: &str = "automatic";
const MATCH_SOURCE_MANUAL: &str = "manual";

/// Tope de resultados por página.
const MAX_PAGE_LIMIT: u32 = 500;
const DEFAULT_PAGE_LIMIT: u32 = 120;

// ---------------------------------------------------------------------------
// Modelos de lectura
// ---------------------------------------------------------------------------

/// Quién decidió el emparejado que hay guardado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchSource {
    /// Nadie: todavía no hay decisión.
    None,
    /// El algoritmo de emparejado por título.
    Automatic,
    /// Una corrección explícita de la persona usuaria.
    Manual,
}

impl MatchSource {
    /// Traduce la columna `match_source` al estado que ve la interfaz.
    ///
    /// `automatic` sin `matched_app_id` no es «el algoritmo decidió que no»:
    /// es que todavía no hay decisión ninguna, y así se dice.
    fn from_row(matched_app_id: Option<u32>, stored: &str) -> Self {
        match stored {
            MATCH_SOURCE_MANUAL => Self::Manual,
            _ if matched_app_id.is_some() => Self::Automatic,
            _ => Self::None,
        }
    }
}

/// Un juego de tienda externa tal y como lo consume la interfaz.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalGame {
    pub store: String,
    pub external_id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub install_path: Option<String>,
    pub installed: bool,
    pub size_on_disk: Option<i64>,
    pub launch_target: Option<String>,
    pub drm_state: DrmState,
    /// Por qué se afirma ese estado de DRM. Se deriva de la política publicada
    /// de la tienda, no de una inspección del binario.
    ///
    /// **No se dibuja sobre la carátula**: es un dato de ficha, igual que el
    /// equivalente de Steam.
    pub drm_evidence: Vec<DrmEvidence>,
    pub matched_app_id: Option<u32>,
    /// Título del juego de Steam emparejado, para poder enseñarlo sin otra
    /// consulta.
    pub matched_title: Option<String>,
    pub match_confidence: f64,
    pub match_source: MatchSource,
    pub discovered_at: String,
    pub updated_at: String,
}

/// Estado de la vinculación de una tienda.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalStoreAccount {
    pub store: String,
    pub display_name: Option<String>,
    /// Carpeta que se reconoció en esta máquina. Se enseña en Ajustes, como la
    /// ruta de la base de datos.
    pub detected_root: Option<String>,
    pub linked: bool,
    pub last_scan_at: Option<String>,
    pub last_scan_status: Option<String>,
    pub last_scan_error_code: Option<String>,
    pub last_scan_error_message: Option<String>,
    pub game_count: i64,
}

/// Filtro de la lista de juegos externos.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalGameRequest {
    pub store: Option<String>,
    pub query: Option<String>,
    pub installed_only: Option<bool>,
    /// `Some(true)` sólo emparejados, `Some(false)` sólo sin emparejar.
    pub matched: Option<bool>,
    /// `Some(true)` sólo DRM-free.
    pub drm_free_only: Option<bool>,
    pub sort: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedExternalGames {
    pub items: Vec<ExternalGame>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

/// Resumen de un escaneo, para que la interfaz pueda contarlo sin volver a
/// consultar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalStoreScanReport {
    pub store: String,
    pub status: String,
    pub detected_root: Option<String>,
    /// Juegos que el escaneo encontró en los manifiestos.
    pub discovered: usize,
    /// Cuántos de ellos quedaron emparejados con un juego de Steam.
    pub matched: usize,
    /// Entradas descartadas por corruptas, vacías o inseguras.
    pub skipped: u32,
    /// Filas que dejaron de estar instaladas porque el manifiesto ya no está.
    pub expired: usize,
    pub sources: Vec<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Persiste el resultado de un escaneo en una única transacción.
///
/// Si el escaneo no fue `success`, **no se toca ni un solo juego**: sólo se
/// registra el diagnóstico en la cuenta. Un cliente que hoy no responde no es
/// evidencia de que la biblioteca de ayer haya desaparecido.
pub fn persist_scan(
    connection: &mut Connection,
    scan: &StoreScan,
) -> AppResult<ExternalStoreScanReport> {
    if scan.games.len() > MAX_DISCOVERED_GAMES {
        return Err(AppError::validation(
            "El escaneo de la tienda externa supera el límite seguro de importación.",
        ));
    }
    let transaction = connection.transaction()?;
    let report = persist_scan_in_transaction(&transaction, scan)?;
    transaction.commit()?;
    Ok(report)
}

pub(crate) fn persist_scan_in_transaction(
    transaction: &Transaction<'_>,
    scan: &StoreScan,
) -> AppResult<ExternalStoreScanReport> {
    let store = scan.store;
    let mut matched = 0_usize;
    let mut expired = 0_usize;

    if scan.status == ScanStatus::Success {
        let index = build_steam_index(transaction)?;
        let manual = manual_decisions(transaction, store)?;

        {
            // Sólo un escaneo del disco sabe qué hay descargado. Uno de cuenta
            // trae `installed = false` y rutas vacías para **todo**, porque la
            // API no habla de instalaciones: escribir esos valores apagaría el
            // botón de jugar de juegos que siguen instalados. Por eso el origen
            // decide qué columnas se sobrescriben.
            let installation_columns = match scan.origin {
                StoreOrigin::Local => {
                    "install_path = excluded.install_path,
                     installed = excluded.installed,
                     launch_target = excluded.launch_target,"
                }
                StoreOrigin::Account => {
                    "install_path = COALESCE(external_games.install_path, excluded.install_path),
                     installed = external_games.installed,
                     launch_target = COALESCE(external_games.launch_target, excluded.launch_target),"
                }
            };
            let mut upsert = transaction.prepare_cached(&format!(
                "INSERT INTO external_games(
                    store, external_id, title, cover_url, header_url, install_path,
                    installed, size_on_disk, launch_target, drm_state,
                    matched_app_id, match_confidence, match_source
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                 ON CONFLICT(store, external_id) DO UPDATE SET
                    title = excluded.title,
                    cover_url = COALESCE(excluded.cover_url, external_games.cover_url),
                    header_url = COALESCE(excluded.header_url, external_games.header_url),
                    {installation_columns}
                    size_on_disk = COALESCE(excluded.size_on_disk, external_games.size_on_disk),
                    drm_state = excluded.drm_state,
                    -- Una decisión humana sobrevive al reescaneo.
                    matched_app_id = CASE
                        WHEN external_games.match_source = 'manual'
                        THEN external_games.matched_app_id
                        ELSE excluded.matched_app_id END,
                    match_confidence = CASE
                        WHEN external_games.match_source = 'manual'
                        THEN external_games.match_confidence
                        ELSE excluded.match_confidence END,
                    match_source = external_games.match_source,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
            ))?;

            for game in &scan.games {
                validate_discovered_game(store, game)?;
                let decision = match manual.iter().find(|(id, _)| id == &game.external_id) {
                    // Ya hay una decisión humana: el algoritmo no la revisa.
                    Some((_, manual_app_id)) => {
                        (*manual_app_id, MANUAL_CONFIDENCE, MATCH_SOURCE_MANUAL)
                    }
                    None => match index.best_match(&game.title) {
                        Some(decision) => (
                            Some(decision.app_id),
                            decision.confidence,
                            MATCH_SOURCE_AUTOMATIC,
                        ),
                        None => (None, 0.0, MATCH_SOURCE_AUTOMATIC),
                    },
                };
                if decision.0.is_some() {
                    matched += 1;
                }
                upsert.execute(params![
                    store.as_str(),
                    game.external_id,
                    game.title,
                    game.cover_url,
                    game.header_url,
                    game.install_path,
                    game.installed,
                    game.size_on_disk,
                    game.launch_target,
                    game.drm_state.as_str(),
                    decision.0,
                    decision.1,
                    decision.2,
                ])?;
            }
        }

        // Caducar instalaciones es una afirmación sobre el disco, así que sólo
        // la puede hacer quien lo ha mirado. Una sincronización de cuenta que
        // ejecutara esto marcaría como desinstalada la biblioteca entera.
        if scan.origin == StoreOrigin::Local {
            expired = expire_unseen_installations(transaction, scan)?;
        }
    }

    link_into_library(transaction)?;
    upsert_account(transaction, scan)?;

    Ok(ExternalStoreScanReport {
        store: store.as_str().to_string(),
        status: scan.status.as_str().to_string(),
        detected_root: scan.detected_root.clone(),
        discovered: scan.games.len(),
        matched,
        skipped: scan.skipped,
        expired,
        sources: scan
            .sources
            .iter()
            .map(|source| source.as_str().to_string())
            .collect(),
        error_code: scan.error_code.clone(),
        error_message: scan.error_message.clone(),
    })
}

/// Da a cada juego externo su sitio en la biblioteca.
///
/// La migración 037 hizo esto con lo que ya había; esta función lo hace con lo
/// que llega después, que es lo que evita que aquello fuera un apaño de una sola
/// vez. Se llama al final de cada sincronización y de cada importación.
///
/// Tres pasos, en este orden:
///
/// 1. **El que ya está en Steam apunta a su fila.** Tener un juego en Steam y en
///    GOG no son dos juegos, es uno con dos procedencias.
/// 2. **El resto recibe un identificador local** ([`LOCAL_APP_ID_BASE`]),
///    continuando la numeración existente. Se asigna en Rust y no en SQL a
///    propósito: la numeración depende de lo que ya hay en la tabla, y una
///    consulta que lee y escribe a la vez es justo donde se cuelan los errores
///    difíciles de ver.
/// 3. **Entra en `games` y en `game_personal`.** Sin ficha personal no hay
///    estado, ni colección, ni orden manual, ni arrastre: la biblioteca une las
///    dos tablas y sin la segunda el juego no existe para ella.
///
/// Devuelve cuántos juegos se han incorporado.
pub(crate) fn link_into_library(transaction: &Transaction<'_>) -> AppResult<usize> {
    transaction.execute(
        "UPDATE external_games
            SET local_app_id = matched_app_id
          WHERE local_app_id IS NULL
            AND matched_app_id IS NOT NULL
            AND EXISTS (SELECT 1 FROM games g WHERE g.app_id = external_games.matched_app_id)",
        [],
    )?;

    let pendientes: Vec<(String, String)> = {
        let mut statement = transaction.prepare(
            "SELECT store, external_id
               FROM external_games
              WHERE local_app_id IS NULL
              ORDER BY store, external_id",
        )?;
        let filas = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        filas.collect::<Result<_, _>>()?
    };

    if !pendientes.is_empty() {
        let siguiente: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(local_app_id), ?1 - 1) + 1
               FROM external_games
              WHERE local_app_id >= ?1",
            params![i64::from(LOCAL_APP_ID_BASE)],
            |row| row.get(0),
        )?;
        let mut asignar = transaction.prepare(
            "UPDATE external_games SET local_app_id = ?3
              WHERE store = ?1 AND external_id = ?2 AND local_app_id IS NULL",
        )?;
        for (indice, (store, external_id)) in pendientes.iter().enumerate() {
            let asignado = siguiente + indice as i64;
            asignar.execute(params![store, external_id, asignado])?;
        }
    }

    // El juego entra con lo que su tienda publica. Nada se inventa: si no hay
    // carátula, la fila la deja vacía y la interfaz enseña sus iniciales.
    let incorporados = transaction.execute(
        "INSERT INTO games (
            app_id, title, cover_url, header_url,
            playtime_minutes, playtime_recent_minutes,
            ownership_source, family_availability, external_store
         )
         SELECT e.local_app_id, e.title, e.cover_url, e.header_url,
                0, 0, 'owned', 'not_applicable', e.store
           FROM external_games e
          WHERE e.local_app_id IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM games g WHERE g.app_id = e.local_app_id)",
        [],
    )?;

    transaction.execute(
        "INSERT INTO game_personal (app_id, status_id)
         SELECT g.app_id, 'unclassified'
           FROM games g
          WHERE g.external_store IS NOT NULL
            AND NOT EXISTS (SELECT 1 FROM game_personal p WHERE p.app_id = g.app_id)",
        [],
    )?;

    // Comprarlo en otra tienda es tenerlo. Cuando el emparejado cae sobre una
    // fila que venía del préstamo familiar, esa fila deja de ser un préstamo:
    // la biblioteca esconde los préstamos sin confirmar, y sin esto se
    // esconderían con ellos juegos que constan comprados. Pasó de verdad, con
    // 158 juegos de Epic. Que además esté prestado en Steam sigue guardado en
    // `family_catalog_games`; lo que cambia es quién decide si se enseña.
    transaction.execute(
        "UPDATE games
            SET ownership_source = 'owned',
                family_availability = 'not_applicable',
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE ownership_source = 'family_shared'
            AND EXISTS (SELECT 1 FROM external_games e WHERE e.local_app_id = games.app_id)",
        [],
    )?;

    // La carátula puede llegar más tarde que el juego —Epic la publica en otra
    // llamada—, así que se refresca sin pisar nada que ya tuviera valor.
    transaction.execute(
        "UPDATE games
            SET cover_url = COALESCE(games.cover_url, (
                    SELECT e.cover_url FROM external_games e
                     WHERE e.local_app_id = games.app_id AND e.cover_url <> ''
                     LIMIT 1)),
                header_url = COALESCE(games.header_url, (
                    SELECT e.header_url FROM external_games e
                     WHERE e.local_app_id = games.app_id AND e.header_url <> ''
                     LIMIT 1))
          WHERE games.external_store IS NOT NULL",
        [],
    )?;

    Ok(incorporados)
}

/// Marca como no instalados los juegos de esta tienda que ya no aparecen en
/// ningún manifiesto.
///
/// **No se borra la fila.** Igual que `family_catalog` retira la afirmación de
/// disponibilidad sin destruir la organización personal, aquí caduca la
/// evidencia de instalación pero se conserva el juego y, sobre todo, la
/// corrección manual de emparejado que la persona usuaria hubiera hecho.
fn expire_unseen_installations(
    transaction: &Transaction<'_>,
    scan: &StoreScan,
) -> AppResult<usize> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS vindexa_seen_external_games(
            external_id TEXT PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM vindexa_seen_external_games;",
    )?;
    {
        let mut insert = transaction.prepare_cached(
            "INSERT OR IGNORE INTO vindexa_seen_external_games(external_id) VALUES (?1)",
        )?;
        for game in &scan.games {
            insert.execute([&game.external_id])?;
        }
    }
    let expired = transaction.execute(
        "UPDATE external_games
            SET installed = 0,
                install_path = NULL,
                launch_target = NULL,
                size_on_disk = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE store = ?1
            AND (installed = 1 OR install_path IS NOT NULL)
            AND NOT EXISTS (
                SELECT 1 FROM vindexa_seen_external_games seen
                 WHERE seen.external_id = external_games.external_id
            )",
        [scan.store.as_str()],
    )?;
    Ok(expired)
}

fn upsert_account(transaction: &Transaction<'_>, scan: &StoreScan) -> AppResult<()> {
    // `linked` es propiedad exclusiva de `link`/`unlink`: un escaneo nunca
    // vincula ni desvincula por su cuenta.
    transaction.execute(
        "INSERT INTO external_store_accounts(
            store, display_name, detected_root, linked, last_scan_at, last_scan_status,
            last_scan_error_code, last_scan_error_message, game_count
         ) VALUES (
            ?1, ?2, ?3, 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?4, ?5, ?6,
            (SELECT COUNT(*) FROM external_games WHERE store = ?1)
         )
         ON CONFLICT(store) DO UPDATE SET
            display_name = excluded.display_name,
            detected_root = COALESCE(excluded.detected_root, external_store_accounts.detected_root),
            last_scan_at = excluded.last_scan_at,
            last_scan_status = excluded.last_scan_status,
            last_scan_error_code = excluded.last_scan_error_code,
            last_scan_error_message = excluded.last_scan_error_message,
            game_count = excluded.game_count,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            scan.store.as_str(),
            scan.store.display_name(),
            scan.detected_root,
            scan.status.as_str(),
            scan.error_code,
            scan.error_message,
        ],
    )?;
    Ok(())
}

fn validate_discovered_game(store: ExternalStore, game: &DiscoveredGame) -> AppResult<()> {
    crate::stores::launch::validate_external_id(store, &game.external_id)?;
    if game.title.trim().is_empty() {
        return Err(AppError::validation(
            "Un juego de la tienda externa llegó sin título.",
        ));
    }
    if game.size_on_disk.is_some_and(|size| size < 0) {
        return Err(AppError::validation(
            "Un juego de la tienda externa declaró un tamaño imposible.",
        ));
    }
    Ok(())
}

/// Devuelve los emparejados que decidió una persona, para no recalcularlos.
fn manual_decisions(
    connection: &Connection,
    store: ExternalStore,
) -> AppResult<Vec<(String, Option<u32>)>> {
    let mut statement = connection.prepare(
        "SELECT external_id, matched_app_id
           FROM external_games
          WHERE store = ?1 AND match_source = ?2",
    )?;
    let rows = statement
        .query_map(params![store.as_str(), MATCH_SOURCE_MANUAL], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<u32>>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn build_steam_index(connection: &Connection) -> AppResult<SteamTitleIndex> {
    // Sólo juegos de Steam. Desde que los de las demás tiendas también viven en
    // `games`, una consulta sin este filtro dejaría que un juego de GOG se
    // emparejara **consigo mismo** —o con su gemelo de Epic— y el vínculo
    // «también está en Steam» pasaría a significar cualquier cosa.
    let mut statement =
        connection.prepare("SELECT app_id, title FROM games WHERE external_store IS NULL")?;
    let candidates = statement
        .query_map([], |row| {
            Ok(MatchCandidate {
                app_id: row.get(0)?,
                title: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SteamTitleIndex::build(&candidates))
}

/// Vuelve a emparejar los juegos de una tienda que **no** tengan decisión
/// manual. Se usa cuando la biblioteca de Steam cambia sin que haya habido un
/// escaneo nuevo de la tienda externa.
pub fn rematch(connection: &mut Connection, store: ExternalStore) -> AppResult<usize> {
    let transaction = connection.transaction()?;
    let index = build_steam_index(&transaction)?;
    let pending = {
        let mut statement = transaction.prepare(
            "SELECT external_id, title FROM external_games
              WHERE store = ?1 AND match_source <> ?2",
        )?;
        statement
            .query_map(params![store.as_str(), MATCH_SOURCE_MANUAL], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut changed = 0_usize;
    {
        let mut update = transaction.prepare_cached(
            "UPDATE external_games
                SET matched_app_id = ?3,
                    match_confidence = ?4,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE store = ?1 AND external_id = ?2
                AND match_source <> ?5",
        )?;
        for (external_id, title) in pending {
            let (app_id, confidence) = match index.best_match(&title) {
                Some(decision) => (Some(decision.app_id), decision.confidence),
                None => (None, 0.0),
            };
            changed += update.execute(params![
                store.as_str(),
                external_id,
                app_id,
                confidence,
                MATCH_SOURCE_MANUAL,
            ])?;
        }
    }
    transaction.commit()?;
    Ok(changed)
}

/// Registra una corrección manual del emparejado.
///
/// `app_id` a `None` significa «esto **no** es ninguno de mis juegos de Steam»,
/// y también se conserva: es una decisión tan válida como la contraria y evita
/// que el algoritmo vuelva a proponer lo que ya se rechazó.
pub fn set_manual_match(
    connection: &Connection,
    store: ExternalStore,
    external_id: &str,
    app_id: Option<u32>,
) -> AppResult<ExternalGame> {
    let external_id = crate::stores::launch::validate_external_id(store, external_id)?;
    if let Some(app_id) = app_id {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM games WHERE app_id = ?1)",
            [app_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(AppError::not_found(
                "Ese juego ya no está en tu biblioteca de Steam.",
            ));
        }
    }
    let updated = connection.execute(
        "UPDATE external_games
            SET matched_app_id = ?3,
                match_confidence = ?4,
                match_source = ?5,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE store = ?1 AND external_id = ?2",
        params![
            store.as_str(),
            external_id,
            app_id,
            MANUAL_CONFIDENCE,
            MATCH_SOURCE_MANUAL
        ],
    )?;
    if updated == 0 {
        return Err(AppError::not_found(
            "Ese juego ya no está en la biblioteca de la tienda externa.",
        ));
    }
    get(connection, store, external_id)
}

/// Devuelve el emparejado a manos del algoritmo.
///
/// Es el deshacer de [`set_manual_match`]: la decisión humana se retira y el
/// juego vuelve a la propuesta automática con la biblioteca de Steam que hay
/// ahora mismo, en lugar de quedarse congelado en una corrección equivocada.
pub fn clear_manual_match(
    connection: &Connection,
    store: ExternalStore,
    external_id: &str,
) -> AppResult<ExternalGame> {
    let external_id = crate::stores::launch::validate_external_id(store, external_id)?;
    let title: String = connection
        .query_row(
            "SELECT title FROM external_games WHERE store = ?1 AND external_id = ?2",
            params![store.as_str(), external_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("Ese juego ya no está en la biblioteca de la tienda externa.")
        })?;
    let index = build_steam_index(connection)?;
    let (app_id, confidence) = match index.best_match(&title) {
        Some(decision) => (Some(decision.app_id), decision.confidence),
        None => (None, 0.0),
    };
    connection.execute(
        "UPDATE external_games
            SET matched_app_id = ?3,
                match_confidence = ?4,
                match_source = ?5,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE store = ?1 AND external_id = ?2",
        params![
            store.as_str(),
            external_id,
            app_id,
            confidence,
            MATCH_SOURCE_AUTOMATIC
        ],
    )?;
    get(connection, store, external_id)
}

/// Marca la tienda como vinculada. No hay credenciales: «vincular» significa
/// que la persona usuaria acepta que Vindexa lea los manifiestos locales de esa
/// tienda.
pub fn link(connection: &Connection, store: ExternalStore) -> AppResult<ExternalStoreAccount> {
    connection.execute(
        "INSERT INTO external_store_accounts(store, display_name, linked)
         VALUES (?1, ?2, 1)
         ON CONFLICT(store) DO UPDATE SET
            linked = 1,
            display_name = excluded.display_name,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![store.as_str(), store.display_name()],
    )?;
    get_account(connection, store)
}

/// Desvincula la tienda y **olvida todo lo derivado de ella**.
///
/// Igual que desvincular Steam elimina la cuenta y el catálogo familiar
/// derivado, desvincular una tienda externa borra sus juegos descubiertos y su
/// registro. No toca la biblioteca de Steam ni la organización personal.
pub fn unlink(connection: &mut Connection, store: ExternalStore) -> AppResult<()> {
    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM external_games WHERE store = ?1",
        [store.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM external_store_accounts WHERE store = ?1",
        [store.as_str()],
    )?;
    transaction.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

const GAME_COLUMNS: &str = "external_games.store,
     external_games.external_id,
     external_games.title,
     external_games.cover_url,
     external_games.header_url,
     external_games.install_path,
     external_games.installed,
     external_games.size_on_disk,
     external_games.launch_target,
     external_games.drm_state,
     external_games.matched_app_id,
     games.title,
     external_games.match_confidence,
     external_games.discovered_at,
     external_games.updated_at,
     external_games.match_source";

fn read_game(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExternalGame> {
    let store: String = row.get(0)?;
    let drm_text: String = row.get(9)?;
    let drm_state = DrmState::parse(&drm_text).unwrap_or_default();
    let matched_app_id: Option<u32> = row.get(10)?;
    let match_confidence: f64 = row.get(12)?;
    let stored_match_source: String = row.get(15)?;
    // La evidencia no se guarda en la tabla porque es una consecuencia directa
    // de la política de la tienda: se deriva al leer y así nunca puede quedar
    // desincronizada de la marca.
    let drm_evidence = ExternalStore::parse(&store)
        .map(ExternalStore::catalogue_drm_evidence)
        .unwrap_or_default();
    Ok(ExternalGame {
        store,
        external_id: row.get(1)?,
        title: row.get(2)?,
        cover_url: row.get(3)?,
        header_url: row.get(4)?,
        install_path: row.get(5)?,
        installed: row.get(6)?,
        size_on_disk: row.get(7)?,
        launch_target: row.get(8)?,
        drm_state,
        drm_evidence,
        matched_app_id,
        matched_title: row.get(11)?,
        match_confidence,
        match_source: MatchSource::from_row(matched_app_id, &stored_match_source),
        discovered_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

pub fn get(
    connection: &Connection,
    store: ExternalStore,
    external_id: &str,
) -> AppResult<ExternalGame> {
    let external_id = crate::stores::launch::validate_external_id(store, external_id)?;
    connection
        .query_row(
            &format!(
                "SELECT {GAME_COLUMNS}
                   FROM external_games
                   LEFT JOIN games ON games.app_id = external_games.matched_app_id
                  WHERE external_games.store = ?1 AND external_games.external_id = ?2"
            ),
            params![store.as_str(), external_id],
            read_game,
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("Ese juego ya no está en la biblioteca de la tienda externa.")
        })
}

pub fn list(
    connection: &Connection,
    request: &ExternalGameRequest,
) -> AppResult<PagedExternalGames> {
    let limit = request
        .limit
        .unwrap_or(DEFAULT_PAGE_LIMIT)
        .clamp(1, MAX_PAGE_LIMIT);
    let offset = request.offset.unwrap_or(0);
    let store = match request.store.as_deref() {
        Some(value) => Some(ExternalStore::parse(value)?),
        None => None,
    };
    let store_filter = store.map(ExternalStore::as_str).unwrap_or_default();
    let query = request.query.as_deref().map(str::trim).unwrap_or_default();
    let pattern = format!("%{query}%");
    let installed_only = request.installed_only.unwrap_or(false);
    let drm_free_only = request.drm_free_only.unwrap_or(false);
    // -1 = sin filtro, 1 = sólo emparejados, 0 = sólo sin emparejar.
    let matched_filter = match request.matched {
        None => -1_i64,
        Some(true) => 1,
        Some(false) => 0,
    };

    // Igual que en `family_catalog`, la única interpolación de SQL sale de una
    // allowlist interna; el texto de la persona usuaria siempre va como
    // parámetro.
    let order_by = match request.sort.as_deref().unwrap_or("alphabetical") {
        "alphabetical" => {
            "external_games.title COLLATE NOCASE ASC, external_games.store ASC, external_games.external_id ASC"
        }
        "alphabeticalDesc" => {
            "external_games.title COLLATE NOCASE DESC, external_games.store ASC, external_games.external_id ASC"
        }
        "installed" => {
            "external_games.installed DESC, external_games.title COLLATE NOCASE ASC, external_games.external_id ASC"
        }
        "sizeDesc" => {
            "external_games.size_on_disk IS NULL, external_games.size_on_disk DESC, external_games.title COLLATE NOCASE ASC"
        }
        "discoveredDesc" => {
            "datetime(external_games.discovered_at) DESC, external_games.title COLLATE NOCASE ASC"
        }
        "updatedDesc" => {
            "datetime(external_games.updated_at) DESC, external_games.title COLLATE NOCASE ASC"
        }
        _ => {
            return Err(AppError::validation(
                "La ordenación de los juegos externos no es válida.",
            ));
        }
    };

    const WHERE_CLAUSE: &str = "WHERE (?1 = '' OR external_games.store = ?1)
           AND (?2 = '' OR external_games.title LIKE ?3 COLLATE NOCASE)
           AND (?4 = 0 OR external_games.installed = 1)
           AND (?5 = -1
                OR (?5 = 1 AND external_games.matched_app_id IS NOT NULL)
                OR (?5 = 0 AND external_games.matched_app_id IS NULL))
           AND (?6 = 0 OR external_games.drm_state = 'drm_free')";

    let total = connection.query_row(
        &format!("SELECT COUNT(*) FROM external_games {WHERE_CLAUSE}"),
        params![
            store_filter,
            query,
            pattern,
            installed_only,
            matched_filter,
            drm_free_only
        ],
        |row| row.get(0),
    )?;

    let mut statement = connection.prepare(&format!(
        "SELECT {GAME_COLUMNS}
           FROM external_games
           LEFT JOIN games ON games.app_id = external_games.matched_app_id
         {WHERE_CLAUSE}
          ORDER BY {order_by}
          LIMIT ?7 OFFSET ?8"
    ))?;
    let items = statement
        .query_map(
            params![
                store_filter,
                query,
                pattern,
                installed_only,
                matched_filter,
                drm_free_only,
                limit,
                offset
            ],
            read_game,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PagedExternalGames {
        items,
        total,
        limit,
        offset,
    })
}

fn read_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExternalStoreAccount> {
    Ok(ExternalStoreAccount {
        store: row.get(0)?,
        display_name: row.get(1)?,
        detected_root: row.get(2)?,
        linked: row.get(3)?,
        last_scan_at: row.get(4)?,
        last_scan_status: row.get(5)?,
        last_scan_error_code: row.get(6)?,
        last_scan_error_message: row.get(7)?,
        game_count: row.get(8)?,
    })
}

const ACCOUNT_COLUMNS: &str = "store, display_name, detected_root, linked, last_scan_at,
     last_scan_status, last_scan_error_code, last_scan_error_message, game_count";

pub fn get_account(
    connection: &Connection,
    store: ExternalStore,
) -> AppResult<ExternalStoreAccount> {
    connection
        .query_row(
            &format!("SELECT {ACCOUNT_COLUMNS} FROM external_store_accounts WHERE store = ?1"),
            [store.as_str()],
            read_account,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("Esa tienda externa no está registrada."))
}

/// Devuelve siempre una entrada por tienda conocida, aunque nunca se haya
/// escaneado: la interfaz necesita poder ofrecer «Vincular» sin que exista fila.
pub fn list_accounts(connection: &Connection) -> AppResult<Vec<ExternalStoreAccount>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {ACCOUNT_COLUMNS} FROM external_store_accounts ORDER BY store ASC"
    ))?;
    let stored = statement
        .query_map([], read_account)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ExternalStore::ALL
        .iter()
        .map(|store| {
            stored
                .iter()
                .find(|account| account.store == store.as_str())
                .cloned()
                .unwrap_or_else(|| ExternalStoreAccount {
                    store: store.as_str().to_string(),
                    display_name: Some(store.display_name().to_string()),
                    detected_root: None,
                    linked: false,
                    last_scan_at: None,
                    last_scan_status: None,
                    last_scan_error_code: None,
                    last_scan_error_message: None,
                    game_count: 0,
                })
        })
        .collect())
}

/// Datos mínimos para lanzar un juego, sin arrastrar el modelo completo.
pub fn launch_context(
    connection: &Connection,
    store: ExternalStore,
    external_id: &str,
) -> AppResult<(Option<String>, Option<String>)> {
    let external_id = crate::stores::launch::validate_external_id(store, external_id)?;
    connection
        .query_row(
            "SELECT install_path, launch_target FROM external_games
              WHERE store = ?1 AND external_id = ?2",
            params![store.as_str(), external_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("Ese juego ya no está en la biblioteca de la tienda externa.")
        })
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalGameRequest, MANUAL_CONFIDENCE, MatchSource, clear_manual_match, get, link, list,
        list_accounts, persist_scan, rematch, set_manual_match, unlink,
    };
    use crate::db::rich_metadata::DrmState;
    use crate::models::LOCAL_APP_ID_BASE;
    use crate::stores::test_support::{insert_steam_game, migrated_database};
    use crate::stores::{
        DiscoveredGame, ExternalStore, ScanSource, ScanStatus, StoreOrigin, StoreScan,
    };
    use rusqlite::params;

    fn game(external_id: &str, title: &str, installed: bool) -> DiscoveredGame {
        DiscoveredGame {
            external_id: external_id.to_string(),
            title: title.to_string(),
            cover_url: None,
            header_url: None,
            install_path: installed.then(|| "/ruta/instalada".to_string()),
            installed,
            size_on_disk: installed.then_some(1_024),
            launch_target: None,
            drm_state: DrmState::DrmFree,
            source: ScanSource::GogGalaxyDatabase,
        }
    }

    fn successful_scan(store: ExternalStore, games: Vec<DiscoveredGame>) -> StoreScan {
        StoreScan {
            store,
            status: ScanStatus::Success,
            detected_root: Some("/ruta/detectada".to_string()),
            error_code: None,
            error_message: None,
            games,
            sources: vec![ScanSource::GogGalaxyDatabase],
            skipped: 0,
            origin: StoreOrigin::Local,
        }
    }

    /// El mismo escaneo, pero leído de la cuenta: sin nada que decir sobre la
    /// instalación, que es justo lo que no puede sobrescribir.
    fn account_scan(store: ExternalStore, games: Vec<DiscoveredGame>) -> StoreScan {
        StoreScan {
            store,
            status: ScanStatus::Success,
            detected_root: None,
            error_code: None,
            error_message: None,
            games,
            sources: vec![ScanSource::GogAccountLibrary],
            skipped: 0,
            origin: StoreOrigin::Account,
        }
    }

    #[test]
    fn a_scan_persists_games_and_matches_them_against_the_steam_library() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        insert_steam_game(&connection, 377160, "Fallout 4");

        let scan = successful_scan(
            ExternalStore::Gog,
            vec![
                game("1207658924", "Fallout 3: Game of the Year Edition", true),
                game("1207658925", "Un juego que Steam no tiene", false),
            ],
        );
        let report = persist_scan(&mut connection, &scan).expect("persistir escaneo");

        assert_eq!(report.status, "success");
        assert_eq!(report.discovered, 2);
        assert_eq!(report.matched, 1);
        assert_eq!(report.expired, 0);
        assert_eq!(report.detected_root.as_deref(), Some("/ruta/detectada"));

        let matched = get(&connection, ExternalStore::Gog, "1207658924").expect("leer emparejado");
        assert_eq!(matched.matched_app_id, Some(22370));
        assert_eq!(matched.matched_title.as_deref(), Some("Fallout 3"));
        assert_eq!(matched.match_source, MatchSource::Automatic);
        assert!(matched.match_confidence < MANUAL_CONFIDENCE);
        // GOG siempre lleva su evidencia de catálogo.
        assert_eq!(matched.drm_state, DrmState::DrmFree);
        assert_eq!(matched.drm_evidence.len(), 1);
        assert_eq!(matched.drm_evidence[0].matched, "catálogo GOG");

        let unmatched =
            get(&connection, ExternalStore::Gog, "1207658925").expect("leer sin emparejar");
        assert_eq!(unmatched.matched_app_id, None);
        assert_eq!(unmatched.match_source, MatchSource::None);
        assert_eq!(unmatched.match_confidence, 0.0);
    }

    #[test]
    fn a_manual_correction_survives_every_later_rescan() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        insert_steam_game(&connection, 377160, "Fallout 4");

        let scan = successful_scan(
            ExternalStore::Gog,
            vec![game("1207658924", "Fallout 3", true)],
        );
        persist_scan(&mut connection, &scan).expect("primer escaneo");

        // La persona usuaria corrige: en realidad es el 4.
        let corrected =
            set_manual_match(&connection, ExternalStore::Gog, "1207658924", Some(377160))
                .expect("corregir a mano");
        assert_eq!(corrected.matched_app_id, Some(377160));
        assert_eq!(corrected.match_source, MatchSource::Manual);
        assert_eq!(corrected.match_confidence, MANUAL_CONFIDENCE);

        // Un reescaneo vuelve a proponer el 3… y no se le hace caso.
        let report = persist_scan(&mut connection, &scan).expect("reescaneo");
        assert_eq!(report.matched, 1);
        let after = get(&connection, ExternalStore::Gog, "1207658924").expect("releer");
        assert_eq!(after.matched_app_id, Some(377160));
        assert_eq!(after.match_source, MatchSource::Manual);

        // Y `rematch` tampoco la toca.
        let changed = rematch(&mut connection, ExternalStore::Gog).expect("reemparejar");
        assert_eq!(changed, 0);
        assert_eq!(
            get(&connection, ExternalStore::Gog, "1207658924")
                .expect("releer")
                .matched_app_id,
            Some(377160)
        );
    }

    #[test]
    fn the_origin_of_a_match_lives_in_its_own_column() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![
                    game("1207658924", "Fallout 3", true),
                    game("1207658925", "Un juego que Steam no tiene", true),
                ],
            ),
        )
        .expect("escanear");
        set_manual_match(&connection, ExternalStore::Gog, "1207658925", Some(22370))
            .expect("corregir a mano");

        let stored = |external_id: &str| -> String {
            connection
                .query_row(
                    "SELECT match_source FROM external_games WHERE store = 'gog' AND external_id = ?1",
                    [external_id],
                    |row| row.get(0),
                )
                .expect("leer la procedencia")
        };
        assert_eq!(stored("1207658924"), "automatic");
        assert_eq!(stored("1207658925"), "manual");

        // Devolver el emparejado al algoritmo lo recalcula con la biblioteca
        // que hay ahora, en vez de dejarlo congelado en la corrección.
        let cleared =
            clear_manual_match(&connection, ExternalStore::Gog, "1207658925").expect("deshacer");
        assert_eq!(cleared.match_source, MatchSource::None);
        assert_eq!(cleared.matched_app_id, None);
        assert_eq!(stored("1207658925"), "automatic");

        let missing = clear_manual_match(&connection, ExternalStore::Gog, "1207000000")
            .expect_err("rechazar un juego que no existe");
        assert_eq!(missing.code, "not_found");
    }

    #[test]
    fn a_manual_rejection_is_also_remembered() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        let scan = successful_scan(
            ExternalStore::Gog,
            vec![game("1207658924", "Fallout 3", true)],
        );
        persist_scan(&mut connection, &scan).expect("primer escaneo");

        // «Éstos no son el mismo juego».
        let rejected = set_manual_match(&connection, ExternalStore::Gog, "1207658924", None)
            .expect("rechazar a mano");
        assert_eq!(rejected.matched_app_id, None);
        assert_eq!(rejected.match_source, MatchSource::Manual);

        persist_scan(&mut connection, &scan).expect("reescaneo");
        let after = get(&connection, ExternalStore::Gog, "1207658924").expect("releer");
        assert_eq!(after.matched_app_id, None);
        assert_eq!(after.match_source, MatchSource::Manual);
    }

    #[test]
    fn an_uninstalled_game_expires_its_installation_without_losing_its_row() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![
                    game("1207658924", "Fallout 3", true),
                    game("1207658925", "Otro juego", true),
                ],
            ),
        )
        .expect("primer escaneo");
        set_manual_match(&connection, ExternalStore::Gog, "1207658925", Some(22370))
            .expect("corregir a mano");

        // Segundo escaneo: el segundo juego ya no aparece en los manifiestos.
        let report = persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![game("1207658924", "Fallout 3", true)],
            ),
        )
        .expect("segundo escaneo");
        assert_eq!(report.expired, 1);

        let expired = get(&connection, ExternalStore::Gog, "1207658925").expect("la fila sigue");
        assert!(!expired.installed);
        assert_eq!(expired.install_path, None);
        assert_eq!(expired.size_on_disk, None);
        // Y la corrección manual sigue intacta.
        assert_eq!(expired.matched_app_id, Some(22370));
        assert_eq!(expired.match_source, MatchSource::Manual);
    }

    #[test]
    fn a_failed_scan_never_destroys_what_was_already_known() {
        let (_directory, mut connection) = migrated_database();
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Epic,
                vec![game("JuegoEpic", "Un juego de Epic", true)],
            ),
        )
        .expect("primer escaneo");

        let failed = StoreScan {
            store: ExternalStore::Epic,
            status: ScanStatus::Failed,
            detected_root: None,
            error_code: Some("epic_manifests_unreadable".to_string()),
            error_message: Some("No se pudieron leer los manifiestos.".to_string()),
            games: Vec::new(),
            sources: Vec::new(),
            skipped: 3,
            origin: StoreOrigin::Local,
        };
        let report = persist_scan(&mut connection, &failed).expect("persistir fallo");
        assert_eq!(report.status, "failed");
        assert_eq!(report.expired, 0);

        // El juego sigue ahí, intacto.
        let survivor = get(&connection, ExternalStore::Epic, "JuegoEpic").expect("sigue");
        assert!(survivor.installed);

        // Y el diagnóstico quedó registrado en la cuenta.
        let accounts = list_accounts(&connection).expect("listar cuentas");
        let epic = accounts
            .iter()
            .find(|account| account.store == "epic")
            .expect("cuenta de Epic");
        assert_eq!(epic.last_scan_status.as_deref(), Some("failed"));
        assert_eq!(
            epic.last_scan_error_code.as_deref(),
            Some("epic_manifests_unreadable")
        );
        assert_eq!(epic.game_count, 1);
        // Y la raíz detectada del escaneo anterior no se borra por un fallo.
        assert_eq!(epic.detected_root.as_deref(), Some("/ruta/detectada"));
    }

    #[test]
    fn scanning_never_links_or_unlinks_by_itself() {
        let (_directory, mut connection) = migrated_database();
        persist_scan(
            &mut connection,
            &successful_scan(ExternalStore::Gog, vec![game("1207658924", "Juego", true)]),
        )
        .expect("escanear");
        let accounts = list_accounts(&connection).expect("listar");
        assert!(!accounts.iter().any(|account| account.linked));

        link(&connection, ExternalStore::Gog).expect("vincular");
        persist_scan(
            &mut connection,
            &successful_scan(ExternalStore::Gog, vec![game("1207658924", "Juego", true)]),
        )
        .expect("reescanear");
        let accounts = list_accounts(&connection).expect("listar");
        let gog = accounts
            .iter()
            .find(|account| account.store == "gog")
            .expect("cuenta de GOG");
        assert!(gog.linked);
    }

    #[test]
    fn unlinking_forgets_the_store_but_not_the_steam_library() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![game("1207658924", "Fallout 3", true)],
            ),
        )
        .expect("escanear");
        link(&connection, ExternalStore::Gog).expect("vincular");

        unlink(&mut connection, ExternalStore::Gog).expect("desvincular");

        let page = list(&connection, &ExternalGameRequest::default()).expect("listar");
        assert_eq!(page.total, 0);
        let accounts = list_accounts(&connection).expect("listar cuentas");
        let gog = accounts
            .iter()
            .find(|account| account.store == "gog")
            .expect("la tienda sigue ofreciéndose");
        assert!(!gog.linked);
        assert_eq!(gog.game_count, 0);
        assert_eq!(gog.last_scan_at, None);

        // La biblioteca de Steam no se toca.
        let steam_games: i64 = connection
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))
            .expect("contar juegos de Steam");
        assert_eq!(steam_games, 1);
    }

    #[test]
    fn listing_filters_and_sorts_through_an_internal_allowlist() {
        let (_directory, mut connection) = migrated_database();
        insert_steam_game(&connection, 22370, "Fallout 3");
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![
                    game("1207658924", "Fallout 3", true),
                    game("1207658925", "Zelda apócrifa", false),
                ],
            ),
        )
        .expect("escanear GOG");
        persist_scan(
            &mut connection,
            &StoreScan {
                games: vec![DiscoveredGame {
                    drm_state: DrmState::Unknown,
                    ..game("JuegoEpic", "Alfa de Epic", true)
                }],
                ..successful_scan(ExternalStore::Epic, Vec::new())
            },
        )
        .expect("escanear Epic");

        let all = list(&connection, &ExternalGameRequest::default()).expect("listar todo");
        assert_eq!(all.total, 3);
        assert_eq!(all.items[0].title, "Alfa de Epic");

        let only_gog = list(
            &connection,
            &ExternalGameRequest {
                store: Some("gog".into()),
                ..ExternalGameRequest::default()
            },
        )
        .expect("filtrar por tienda");
        assert_eq!(only_gog.total, 2);

        let installed = list(
            &connection,
            &ExternalGameRequest {
                installed_only: Some(true),
                ..ExternalGameRequest::default()
            },
        )
        .expect("filtrar instalados");
        assert_eq!(installed.total, 2);

        let matched = list(
            &connection,
            &ExternalGameRequest {
                matched: Some(true),
                ..ExternalGameRequest::default()
            },
        )
        .expect("filtrar emparejados");
        assert_eq!(matched.total, 1);
        assert_eq!(matched.items[0].external_id, "1207658924");

        let unmatched = list(
            &connection,
            &ExternalGameRequest {
                matched: Some(false),
                ..ExternalGameRequest::default()
            },
        )
        .expect("filtrar sin emparejar");
        assert_eq!(unmatched.total, 2);

        let drm_free = list(
            &connection,
            &ExternalGameRequest {
                drm_free_only: Some(true),
                ..ExternalGameRequest::default()
            },
        )
        .expect("filtrar DRM-free");
        assert_eq!(drm_free.total, 2);
        assert!(drm_free.items.iter().all(|item| item.store == "gog"));

        // La búsqueda de texto va como parámetro, nunca interpolada.
        let searched = list(
            &connection,
            &ExternalGameRequest {
                query: Some("' OR 1=1 --".into()),
                ..ExternalGameRequest::default()
            },
        )
        .expect("buscar texto hostil");
        assert_eq!(searched.total, 0);

        let error = list(
            &connection,
            &ExternalGameRequest {
                sort: Some("title; DROP TABLE external_games".into()),
                ..ExternalGameRequest::default()
            },
        )
        .expect_err("rechazar una ordenación inventada");
        assert_eq!(error.code, "validation");

        let store_error = list(
            &connection,
            &ExternalGameRequest {
                store: Some("steam".into()),
                ..ExternalGameRequest::default()
            },
        )
        .expect_err("rechazar una tienda inventada");
        assert_eq!(store_error.code, "validation");
    }

    #[test]
    fn a_hostile_identifier_never_reaches_the_database() {
        let (_directory, mut connection) = migrated_database();
        let scan = successful_scan(
            ExternalStore::Gog,
            vec![game("1207658924'; DROP TABLE games; --", "Malicioso", true)],
        );
        let error = persist_scan(&mut connection, &scan).expect_err("rechazar el escaneo");
        assert_eq!(error.code, "validation");

        // Y la transacción no dejó nada a medias.
        let rows: i64 = connection
            .query_row("SELECT COUNT(*) FROM external_games", [], |row| row.get(0))
            .expect("contar filas");
        assert_eq!(rows, 0);

        let manual_error =
            set_manual_match(&connection, ExternalStore::Gog, "no-soy-un-id", Some(1))
                .expect_err("rechazar identificador");
        assert_eq!(manual_error.code, "validation");
    }

    #[test]
    fn a_manual_match_to_a_missing_steam_game_is_refused() {
        let (_directory, mut connection) = migrated_database();
        persist_scan(
            &mut connection,
            &successful_scan(ExternalStore::Gog, vec![game("1207658924", "Juego", true)]),
        )
        .expect("escanear");

        let error = set_manual_match(&connection, ExternalStore::Gog, "1207658924", Some(999_999))
            .expect_err("rechazar un AppID inexistente");
        assert_eq!(error.code, "not_found");
    }

    #[test]
    fn un_juego_de_otra_tienda_entra_en_la_biblioteca_y_se_puede_organizar() {
        let (_directory, mut connection) = migrated_database();
        // Uno que Steam no tiene y otro que sí: el primero es un juego nuevo
        // para la biblioteca, el segundo es el mismo juego comprado dos veces.
        insert_steam_game(&connection, 22370, "Fallout 3");
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![
                    game("1207658924", "Fallout 3", false),
                    game("1207658925", "Beneath a Steel Sky", false),
                ],
            ),
        )
        .expect("escanear");

        fn vinculo(connection: &rusqlite::Connection, external_id: &str) -> i64 {
            connection
                .query_row(
                    "SELECT local_app_id FROM external_games WHERE store = 'gog' AND external_id = ?1",
                    params![external_id],
                    |row| row.get(0),
                )
                .expect("leer el vínculo")
        }

        // El que ya estaba en Steam apunta a esa fila: un juego, no dos.
        assert_eq!(vinculo(&connection, "1207658924"), 22370);
        // El que no estaba recibe identificador propio y entra como juego suyo.
        let nuevo = vinculo(&connection, "1207658925");
        assert!(nuevo >= i64::from(LOCAL_APP_ID_BASE), "{nuevo}");

        let (titulo, tienda, propiedad): (String, String, String) = connection
            .query_row(
                "SELECT title, external_store, ownership_source FROM games WHERE app_id = ?1",
                params![nuevo],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("el juego está en la biblioteca");
        assert_eq!(titulo, "Beneath a Steel Sky");
        assert_eq!(tienda, "gog");
        assert_eq!(propiedad, "owned", "un juego comprado es tuyo");

        // Y con ficha personal, que es lo que permite clasificarlo, arrastrarlo
        // y planificarlo. Sin ella la biblioteca ni siquiera lo devolvería.
        let estado: String = connection
            .query_row(
                "SELECT status_id FROM game_personal WHERE app_id = ?1",
                params![nuevo],
                |row| row.get(0),
            )
            .expect("tiene ficha personal");
        assert_eq!(estado, "unclassified");

        // Un juego prestado en Steam que además consta comprado en otra tienda
        // deja de esconderse: la compra manda sobre el préstamo.
        connection
            .execute(
                "INSERT INTO games(app_id, title, ownership_source, family_availability)
                 VALUES (900001, 'Prestado y comprado', 'family_shared', 'unknown')",
                [],
            )
            .expect("insertar prestado");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (900001, 'unclassified')",
                [],
            )
            .expect("ficha del prestado");
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Epic,
                vec![game("epic-1", "Prestado y comprado", false)],
            ),
        )
        .expect("escanear Epic");
        let (propiedad, disponibilidad): (String, String) = connection
            .query_row(
                "SELECT ownership_source, family_availability FROM games WHERE app_id = 900001",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer el prestado");
        assert_eq!(propiedad, "owned", "comprarlo en otra tienda es tenerlo");
        assert_eq!(disponibilidad, "not_applicable");

        // Reescanear no duplica ni reasigna: el identificador es estable.
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![
                    game("1207658924", "Fallout 3", false),
                    game("1207658925", "Beneath a Steel Sky", false),
                ],
            ),
        )
        .expect("reescanear");
        assert_eq!(vinculo(&connection, "1207658925"), nuevo);
        let total: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM games WHERE external_store IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(total, 1, "reescanear no crea un juego nuevo");
    }

    #[test]
    fn rematching_picks_up_a_steam_library_that_grew_later() {
        let (_directory, mut connection) = migrated_database();
        persist_scan(
            &mut connection,
            &successful_scan(
                ExternalStore::Gog,
                vec![game("1207658924", "Fallout 3", true)],
            ),
        )
        .expect("escanear antes de tener Steam");
        assert_eq!(
            get(&connection, ExternalStore::Gog, "1207658924")
                .expect("leer")
                .matched_app_id,
            None
        );

        insert_steam_game(&connection, 22370, "Fallout 3");
        let changed = rematch(&mut connection, ExternalStore::Gog).expect("reemparejar");
        assert_eq!(changed, 1);
        assert_eq!(
            get(&connection, ExternalStore::Gog, "1207658924")
                .expect("leer")
                .matched_app_id,
            Some(22370)
        );
    }

    #[test]
    fn syncing_the_account_never_uninstalls_what_the_disc_found() {
        let (_directory, mut connection) = migrated_database();

        // El disco encontró el juego instalado, con su ruta y su ejecutable.
        let mut installed = game("1207658924", "Fallout 3", true);
        installed.install_path = Some("/Users/Shared/GOG/Fallout 3".to_string());
        installed.launch_target = Some("/Users/Shared/GOG/Fallout 3/juego".to_string());
        installed.size_on_disk = Some(4_096);
        persist_scan(
            &mut connection,
            &successful_scan(ExternalStore::Gog, vec![installed]),
        )
        .expect("escaneo local");

        // La cuenta sabe qué se posee y **nada** de la instalación: trae el
        // mismo juego marcado como no instalado y sin rutas, más uno que sólo
        // aparece en la biblioteca de la tienda.
        let report = persist_scan(
            &mut connection,
            &account_scan(
                ExternalStore::Gog,
                vec![
                    game("1207658924", "Fallout 3", false),
                    game("1207658930", "The Witcher 2", false),
                ],
            ),
        )
        .expect("sincronización de cuenta");

        let survivor = get(&connection, ExternalStore::Gog, "1207658924").expect("sigue");
        assert!(
            survivor.installed,
            "una sincronización de cuenta no puede desinstalar nada"
        );
        assert_eq!(
            survivor.install_path.as_deref(),
            Some("/Users/Shared/GOG/Fallout 3")
        );
        assert_eq!(
            survivor.launch_target.as_deref(),
            Some("/Users/Shared/GOG/Fallout 3/juego")
        );
        assert_eq!(survivor.size_on_disk, Some(4_096));

        // Y el juego que sólo existe en la cuenta entra igual, sin instalación.
        let owned = get(&connection, ExternalStore::Gog, "1207658930").expect("nuevo");
        assert!(!owned.installed);
        assert_eq!(owned.install_path, None);

        // Caducar instalaciones es cosa del disco: la cuenta no lo hace.
        assert_eq!(report.expired, 0);
        assert_eq!(report.discovered, 2);
    }

    #[test]
    fn a_local_scan_still_expires_what_is_no_longer_installed() {
        let (_directory, mut connection) = migrated_database();

        let mut installed = game("1207658924", "Fallout 3", true);
        installed.install_path = Some("/Users/Shared/GOG/Fallout 3".to_string());
        persist_scan(
            &mut connection,
            &successful_scan(ExternalStore::Gog, vec![installed]),
        )
        .expect("escaneo local");

        // El mismo escáner local ya no lo ve: eso sí es evidencia de que se
        // desinstaló, y la fila se conserva pero deja de estar instalada.
        let report = persist_scan(
            &mut connection,
            &successful_scan(ExternalStore::Gog, Vec::new()),
        )
        .expect("segundo escaneo local");
        assert_eq!(report.expired, 1);
        assert!(
            !get(&connection, ExternalStore::Gog, "1207658924")
                .expect("la fila sobrevive")
                .installed
        );
    }

    #[test]
    fn the_origin_of_a_scan_defaults_to_the_disc() {
        // `StoreScan::empty` es lo que usan todos los escáneres locales. Si el
        // valor por defecto cambiara, empezarían a comportarse como la cuenta
        // sin que nadie lo hubiera pedido.
        let scan = StoreScan::empty(ExternalStore::Epic, ScanStatus::Success);
        assert_eq!(scan.origin, StoreOrigin::Local);
    }
}
