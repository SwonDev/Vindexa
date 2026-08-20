//! Completar el DRM de la biblioteca sin volver a bajarla entera.
//!
//! # El hueco que cierra
//!
//! La clasificación DRM llegó con la migración 039 y viaja dentro del
//! enriquecimiento de fichas. Todo lo que se había enriquecido **antes** se
//! quedó en `unknown` con la evidencia vacía, y el enriquecimiento sólo vuelve a
//! pasar por un juego una semana después. Medido en la instalación real: 1.788
//! juegos con la ficha completa y sin veredicto de DRM, de 3.877.
//!
//! Volver a enriquecer esos 1.788 significa bajar capturas, vídeos y
//! valoraciones que ya se tienen. Esta pasada pide `filters=basic,categories`,
//! que es donde viajan los tres avisos y lo único que la clasificación mira:
//! medido sobre Red Dead Redemption 2, 8.402 bytes en vez de 17.118.
//!
//! (`drm_notice` **no** es una sección válida del parámetro `filters`: pedirla
//! sola devuelve `data: []`. Comprobado contra la tienda.)
//!
//! # Por qué de uno en uno
//!
//! Comprobado contra la tienda: `appdetails` sólo admite varios AppID cuando el
//! filtro es `price_overview`. Con cualquier otro filtro, pedir dos devuelve
//! `null`. Así que aquí se va de uno en uno, con pausa, y se hace poco a poco en
//! segundo plano en vez de a ráfagas.
//!
//! # Lo que no hace
//!
//! No toca los juegos de Epic, GOG ni itch.io: no existen en Steam y preguntar
//! por ellos allí devolvería vacío. El DRM de GOG ya viene de su propia tienda,
//! que vende sin DRM por definición.

use std::time::Duration;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::db::Database;
use crate::error::AppResult;
use crate::steam::store_api;

/// Cuántos juegos se repasan por tanda. Con pausa de segundo y medio, una tanda
/// son unos cinco minutos: suficiente para avanzar, poco para molestar.
const BATCH_SIZE: u32 = 200;

/// Pausa entre peticiones. La tienda tolera unas doscientas cada cinco minutos;
/// segundo y medio deja margen de sobra por si algo más está pidiendo a la vez.
const REQUEST_INTERVAL: Duration = Duration::from_millis(1_500);

const LAST_AUTO_KEY: &str = "drm.last_auto_pass";

/// Cada cuánto se repasa **mientras quede biblioteca por mirar**.
///
/// La primera vez hay miles de juegos sin veredicto —los que se enriquecieron
/// antes de que existiera la clasificación— y una tanda son doscientos. A doce
/// horas por tanda, completar una biblioteca de cuatro mil juegos llevaría más
/// de una semana de aplicación abierta; a diez minutos, unas tres horas.
const CATCHUP_INTERVAL_MINUTES: i64 = 10;

// No hay ritmo «en reposo»: cuando no queda nada por mirar, esta pasada se
// calla del todo. Un juego preguntado y sin veredicto no vuelve por aquí, pero
// tampoco se queda olvidado: el enriquecimiento de fichas vuelve a pasar por él
// a los siete días y escribe el DRM igual que esta pasada.

/// Qué dejó una pasada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrmPassReport {
    /// Juegos preguntados.
    pub checked: u32,
    /// Juegos que salieron de `unknown` con un veredicto.
    pub classified: u32,
    /// Preguntados y seguían sin señal suficiente: la tienda no declara nada
    /// que permita afirmar ni negar. `unknown` es una respuesta, no un fallo.
    pub still_unknown: u32,
    /// No se pudo preguntar: la tienda falló o no reconoció el juego.
    pub failed: u32,
    /// Cuántos quedan por repasar después de esta tanda.
    pub pending: u32,
}

/// Repasa una tanda de juegos sin veredicto de DRM.
pub async fn run(database: &Database, limit: u32) -> AppResult<DrmPassReport> {
    let limit = if limit == 0 { BATCH_SIZE } else { limit };
    let candidatos = pending_app_ids(database, limit)?;
    let mut report = DrmPassReport::default();

    for (indice, app_id) in candidatos.iter().enumerate() {
        if indice > 0 {
            sleep(REQUEST_INTERVAL).await;
        }
        report.checked = report.checked.saturating_add(1);
        match store_api::fetch_drm_signals(*app_id).await {
            Ok(Some(assessment)) => {
                let clasificado = assessment.state != crate::db::rich_metadata::DrmState::Unknown;
                match database.save_drm_assessment(*app_id, &assessment) {
                    Ok(()) if clasificado => {
                        report.classified = report.classified.saturating_add(1);
                    }
                    Ok(()) => report.still_unknown = report.still_unknown.saturating_add(1),
                    Err(_) => report.failed = report.failed.saturating_add(1),
                }
            }
            // La tienda contestó sin datos: el juego ya no está publicado.
            //
            // No es un fallo de red y no tiene sentido reintentarlo, así que se
            // deja constancia de **que se preguntó**: el estado sigue siendo
            // «no se sabe» —porque no se sabe—, pero la fecha de comprobación
            // lo saca de la cola.
            //
            // Sin esto se quedaba dentro para siempre. Y como la cola va por
            // AppID ascendente y los juegos viejos son justo los que más se
            // retiran, cada tanda gastaba su cabecera preguntando una y otra
            // vez por los mismos juegos que ya no existen.
            Ok(None) => {
                let sin_señal = crate::db::rich_metadata::DrmAssessment {
                    state: crate::db::rich_metadata::DrmState::Unknown,
                    evidence: Vec::new(),
                };
                match database.save_drm_assessment(*app_id, &sin_señal) {
                    Ok(()) => report.still_unknown = report.still_unknown.saturating_add(1),
                    Err(_) => report.failed = report.failed.saturating_add(1),
                }
            }
            Err(failure) => {
                report.failed = report.failed.saturating_add(1);
                if let Some(delay) = failure.retry_after {
                    sleep(delay).await;
                }
            }
        }
    }

    report.pending = pending_count(database)?;
    Ok(report)
}

/// Repasa si toca, sin que nadie lo pida.
///
/// El ritmo depende de lo que quede: con biblioteca por mirar va cada diez
/// minutos hasta terminar, y luego se calla.
pub async fn run_if_due(database: &Database) -> AppResult<Option<DrmPassReport>> {
    let pendientes = pending_count(database)?;
    if pendientes == 0 {
        return Ok(None);
    }
    let espera = chrono::Duration::minutes(CATCHUP_INTERVAL_MINUTES);
    if !is_due(database, espera)? {
        return Ok(None);
    }
    let report = run(database, 0).await?;
    mark_done(database)?;
    Ok(Some(report))
}

/// Juegos de Steam a los que nunca se les miró el DRM.
///
/// «Nunca se miró» no es lo mismo que «se miró y no se supo». Lo segundo tiene
/// fecha en `drm_checked_at` y **no vuelve a esta cola**: no hay plazo de
/// caducidad, y decir que lo hay sería describir algo que no ocurre. Si la
/// ficha de ese juego se vuelve a enriquecer, el veredicto se recalcula por ese
/// camino, que es el que sí revisita.
fn pending_app_ids(database: &Database, limit: u32) -> AppResult<Vec<u32>> {
    let connection = database.open()?;
    let mut statement = connection.prepare(
        "SELECT app_id
           FROM games
          WHERE (external_store IS NULL OR external_store = '')
            AND drm_state = 'unknown'
            AND drm_checked_at IS NULL
          ORDER BY app_id ASC
          LIMIT ?1",
    )?;
    let ids = statement
        .query_map([limit], |row| row.get::<_, u32>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

fn pending_count(database: &Database) -> AppResult<u32> {
    let connection = database.open()?;
    let total: i64 = connection.query_row(
        "SELECT COUNT(*)
           FROM games
          WHERE (external_store IS NULL OR external_store = '')
            AND drm_state = 'unknown'
            AND drm_checked_at IS NULL",
        [],
        |row| row.get(0),
    )?;
    Ok(total.max(0) as u32)
}

fn is_due(database: &Database, espera: chrono::Duration) -> AppResult<bool> {
    let connection = database.open()?;
    let last: Option<String> = connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [LAST_AUTO_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let Some(last) = last else {
        return Ok(true);
    };
    let Ok(moment) = chrono::DateTime::parse_from_rfc3339(&last) else {
        return Ok(true);
    };
    Ok(Utc::now().signed_duration_since(moment.with_timezone(&Utc)) >= espera)
}

fn mark_done(database: &Database) -> AppResult<()> {
    let connection = database.open()?;
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![LAST_AUTO_KEY, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use rusqlite::Connection;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("claves foráneas");
        // Sin sembrar: esta prueba sólo mira la consulta de pendientes, que no
        // depende de estados ni colecciones por defecto.
        migrations::migrate(&mut connection).expect("migrar");
        connection
    }

    fn juego(connection: &Connection, app_id: u32, tienda: Option<&str>, estado: &str, mirado: bool) {
        connection
            .execute(
                "INSERT INTO games(app_id, title, external_store, drm_state, drm_checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    app_id,
                    format!("Juego {app_id}"),
                    tienda,
                    estado,
                    mirado.then(|| "2026-08-01T00:00:00Z".to_string())
                ],
            )
            .expect("insertar juego");
    }

    /// Consulta idéntica a la del módulo, sobre una conexión de prueba.
    fn pendientes(connection: &Connection) -> Vec<u32> {
        let mut statement = connection
            .prepare(
                "SELECT app_id FROM games
                  WHERE (external_store IS NULL OR external_store = '')
                    AND drm_state = 'unknown'
                    AND drm_checked_at IS NULL
                  ORDER BY app_id ASC",
            )
            .expect("preparar");
        statement
            .query_map([], |row| row.get::<_, u32>(0))
            .expect("consultar")
            .collect::<Result<Vec<_>, _>>()
            .expect("recoger")
    }

    #[test]
    fn solo_entran_los_de_steam_que_nunca_se_miraron() {
        let connection = database();
        juego(&connection, 1, None, "unknown", false); // entra
        juego(&connection, 2, None, "drm_free", false); // ya tiene veredicto
        juego(&connection, 3, None, "unknown", true); // ya se miró y no se supo
        juego(&connection, 4, Some("epic"), "unknown", false); // no está en Steam
        juego(&connection, 5, Some("gog"), "unknown", false); // idem

        assert_eq!(pendientes(&connection), [1]);
    }

    /// Contra la tienda de verdad.
    ///
    /// Apagada por defecto. Comprueba lo que de verdad se rompe solo: que el
    /// filtro sigue trayendo los avisos y que la clasificación los entiende.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_la_tienda
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_la_tienda_de_verdad_los_avisos_siguen_llegando() {
        use crate::db::rich_metadata::DrmState;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Red Dead Redemption 2 exige cuenta de Rockstar: es el caso que separa
        // «sin DRM» de «DRM de terceros», y si el filtro dejara de traer el
        // aviso, este juego se clasificaría como libre.
        let rdr2 = runtime
            .block_on(store_api::fetch_drm_signals(1174180))
            .expect("preguntar")
            .expect("la tienda conoce el juego");
        assert_eq!(rdr2.state, DrmState::ThirdPartyDrm, "{rdr2:?}");
        assert!(
            rdr2.evidence.iter().any(|e| e.matched.to_lowercase().contains("rockstar")),
            "la evidencia cita el aviso literal: {:?}",
            rdr2.evidence
        );

        // Portal 2 no declara ningún aviso: con la respuesta completa, eso es
        // evidencia de que no hay DRM de terceros.
        let portal2 = runtime
            .block_on(store_api::fetch_drm_signals(620))
            .expect("preguntar")
            .expect("la tienda conoce el juego");
        assert_eq!(portal2.state, DrmState::DrmFree, "{portal2:?}");
    }

    #[test]
    fn un_informe_vacio_no_afirma_nada() {
        let report = DrmPassReport::default();
        assert_eq!(report.checked, 0);
        assert_eq!(report.classified, 0);
        assert_eq!(report.pending, 0);
    }


    /// Un juego que ya no está en la tienda tiene que salir de la cola.
    ///
    /// La cola va por AppID ascendente y los juegos viejos son justo los que
    /// más se retiran: sin marcar que se preguntó, cada tanda gastaba su
    /// cabecera volviendo a preguntar por los mismos juegos que ya no existen,
    /// cada diez minutos, para siempre.
    #[test]
    fn preguntar_por_un_juego_retirado_cuenta_como_preguntado() {
        let mut connection = database();
        juego(&connection, 2430, None, "unknown", false);
        assert_eq!(
            pendientes(&connection),
            vec![2430],
            "empieza en la cola"
        );

        // Es lo que hace la pasada cuando la tienda contesta sin datos: guarda
        // «no se sabe» —porque no se sabe— y con ello la fecha de comprobación.
        crate::db::rich_metadata::save(
            &mut connection,
            2430,
            &crate::db::rich_metadata::RichMetadataUpdate {
                drm: Some(crate::db::rich_metadata::DrmAssessment {
                    state: crate::db::rich_metadata::DrmState::Unknown,
                    evidence: Vec::new(),
                }),
                ..crate::db::rich_metadata::RichMetadataUpdate::default()
            },
        )
        .expect("guardar");

        assert!(
            pendientes(&connection).is_empty(),
            "preguntado y sin respuesta sale de la cola"
        );
        let estado: String = connection
            .query_row("SELECT drm_state FROM games WHERE app_id = 2430", [], |row| {
                row.get(0)
            })
            .expect("leer estado");
        assert_eq!(estado, "unknown", "sigue sin saberse, que es la verdad");
    }
}
