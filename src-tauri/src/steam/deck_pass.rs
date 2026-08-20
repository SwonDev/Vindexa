//! Completar la compatibilidad con Steam Deck de la biblioteca.
//!
//! # El hueco que cierra
//!
//! La columna `steam_deck_status` existe desde la primera migración, tiene su
//! índice, viaja en la ficha, se puede filtrar y hasta se puede usar en una
//! regla de colección inteligente. Nunca la escribió nadie: en la instalación
//! real estaba vacía en los 3.877 juegos, así que la ficha decía «Sin datos»
//! en todas y el filtro se ofrecía apagado con una nota diciendo que Steam no
//! publica el dato «mediante una API Web documentada».
//!
//! La nota era cierta y estaba incompleta: no hay API documentada, pero sí un
//! informe público —el mismo que la tienda pinta en la página de cada juego—
//! que no pide clave ni sesión. Es de la misma naturaleza que
//! `featuredcategories`, que ya usa el radar de ofertas, o que
//! `freeGamesPromotions` de Epic.
//!
//! # Por qué de uno en uno
//!
//! El informe se pide por AppID; no admite varios. Igual que el repaso de DRM,
//! esto va poco a poco en segundo plano y con pausa, no a ráfagas.
//!
//! # Lo que no hace
//!
//! No toca los juegos de Epic, GOG ni itch.io: no existen en Steam y su
//! identificador se lo inventó Vindexa.

use std::time::Duration;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::db::Database;
use crate::error::AppResult;
use crate::steam::store_api;

/// Cuántos juegos se preguntan por tanda.
const BATCH_SIZE: u32 = 200;

/// Pausa entre peticiones, la misma que el repaso de DRM.
const REQUEST_INTERVAL: Duration = Duration::from_millis(1_500);

const LAST_AUTO_KEY: &str = "deck.last_auto_pass";

/// Cada cuánto se repasa mientras quede biblioteca por preguntar.
const CATCHUP_INTERVAL_MINUTES: i64 = 10;

/// Qué dejó una pasada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckPassReport {
    /// Juegos preguntados.
    pub checked: u32,
    /// Juegos que recibieron una de las cuatro palabras.
    pub resolved: u32,
    /// La tienda contestó sin informe: retirado, o una categoría que no existía
    /// cuando se escribió esto.
    pub without_report: u32,
    /// No se pudo preguntar.
    pub failed: u32,
    /// Cuántos quedan después de esta tanda.
    pub pending: u32,
}

/// Pregunta por una tanda de juegos sin compatibilidad conocida.
pub async fn run(database: &Database, limit: u32) -> AppResult<DeckPassReport> {
    let limit = if limit == 0 { BATCH_SIZE } else { limit };
    let candidatos = database.steam_deck_pending(limit)?;
    let mut report = DeckPassReport::default();

    for (indice, app_id) in candidatos.iter().enumerate() {
        if indice > 0 {
            sleep(REQUEST_INTERVAL).await;
        }
        report.checked = report.checked.saturating_add(1);
        match store_api::fetch_deck_status(*app_id).await {
            Ok(Some(status)) => match database.save_steam_deck_status(*app_id, status) {
                Ok(()) => report.resolved = report.resolved.saturating_add(1),
                Err(_) => report.failed = report.failed.saturating_add(1),
            },
            // Sin informe: el juego ya no está publicado, o la tienda estrenó
            // una categoría que aquí no se traduce. No se apunta nada —la
            // columna sigue vacía, que es la verdad— y vuelve a la cola.
            Ok(None) => report.without_report = report.without_report.saturating_add(1),
            Err(failure) => {
                report.failed = report.failed.saturating_add(1);
                if let Some(delay) = failure.retry_after {
                    sleep(delay).await;
                }
            }
        }
    }

    report.pending = database.steam_deck_pending_count()?;
    Ok(report)
}

/// Repasa si toca, sin que nadie lo pida.
pub async fn run_if_due(database: &Database) -> AppResult<Option<DeckPassReport>> {
    if database.steam_deck_pending_count()? == 0 {
        return Ok(None);
    }
    if !is_due(
        database,
        chrono::Duration::minutes(CATCHUP_INTERVAL_MINUTES),
    )? {
        return Ok(None);
    }
    let report = run(database, 0).await?;
    mark_done(database)?;
    Ok(Some(report))
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
    let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(&last) else {
        // Una marca ilegible no puede bloquear el repaso para siempre.
        return Ok(true);
    };
    Ok(Utc::now() - parsed.with_timezone(&Utc) >= espera)
}

fn mark_done(database: &Database) -> AppResult<()> {
    let connection = database.open()?;
    connection.execute(
        "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![
            LAST_AUTO_KEY,
            Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::db::migrations;
    use rusqlite::Connection;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        connection
    }

    fn juego(connection: &Connection, app_id: u32, tienda: Option<&str>, deck: Option<&str>) {
        connection
            .execute(
                "INSERT INTO games(app_id, title, external_store, steam_deck_status)
                 VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![app_id, format!("Juego {app_id}"), tienda, deck],
            )
            .expect("insertar juego");
    }

    /// Consulta idéntica a la del módulo, sobre una conexión de prueba.
    fn pendientes(connection: &Connection) -> Vec<u32> {
        let mut statement = connection
            .prepare(
                "SELECT app_id FROM games
                  WHERE (external_store IS NULL OR external_store = '')
                    AND steam_deck_status IS NULL
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
    fn solo_entran_los_de_steam_a_los_que_no_se_ha_preguntado() {
        let connection = database();
        juego(&connection, 1, None, None); // entra
        juego(&connection, 2, None, Some("verified")); // ya se preguntó
        juego(&connection, 3, None, Some("unknown")); // se preguntó y no lo valoran
        juego(&connection, 4, Some("epic"), None); // no existe en Steam
        juego(&connection, 5, Some("itch"), None); // idem

        assert_eq!(pendientes(&connection), [1]);
    }

    /// Contra la tienda de verdad.
    ///
    /// Apagada por defecto. Comprueba lo único que se rompe solo: que el
    /// informe sigue publicándose sin clave y con las mismas categorías.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_la_tienda_de_verdad_el_informe
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_la_tienda_de_verdad_el_informe_sigue_ahi() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        // Portal 2 está verificado desde que existe el Deck.
        let verificado = runtime
            .block_on(crate::steam::store_api::fetch_deck_status(620))
            .expect("preguntar por Portal 2");
        assert_eq!(verificado, Some("verified"));

        // Destiny 2 no es compatible: su anticheat no corre en el Deck.
        let incompatible = runtime
            .block_on(crate::steam::store_api::fetch_deck_status(1_085_660))
            .expect("preguntar por Destiny 2");
        assert_eq!(incompatible, Some("unsupported"));
    }
}
