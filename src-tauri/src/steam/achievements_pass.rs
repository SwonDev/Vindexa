//! Completar los logros conseguidos de la biblioteca.
//!
//! # El hueco que cierra
//!
//! `achievements_total` estaba puesto en 1.655 juegos de una instalación real
//! y `achievements_unlocked` **en ninguno**, con la clave de Steam configurada
//! desde hacía días. La ficha decía «Logros · Sin datos» en todas y ofrecía un
//! botón para traerlos: uno por juego, mil seiscientas cincuenta y cinco veces.
//! Nadie hace eso, así que la columna se quedó vacía para siempre.
//!
//! Es el mismo patrón que ya apareció en la compatibilidad con Steam Deck y en
//! la prioridad calculada: la función estaba entera y sólo le faltaba correr
//! sola.
//!
//! # Por qué de uno en uno
//!
//! `GetPlayerAchievements` se pide por juego y por cuenta; no admite lotes. Va
//! poco a poco en segundo plano y con pausa, como el repaso de DRM.
//!
//! # Lo que no hace
//!
//! No toca los juegos de Epic, GOG ni itch.io —su identificador se lo inventó
//! Vindexa—, ni pregunta si no hay cuenta vinculada o clave guardada: sin una
//! de las dos, la respuesta sería siempre la misma negativa.

use std::time::Duration;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::db::Database;
use crate::error::AppResult;
use crate::steam::achievements::{self, AchievementOutcome};

/// Cuántos juegos se preguntan por tanda.
///
/// A segundo y medio por petición, doscientos son cinco minutos de trabajo de
/// fondo. Con mil seiscientos juegos con logros, la biblioteca queda cubierta
/// en un par de horas de uso.
const BATCH_SIZE: u32 = 200;

/// Pausa entre peticiones, la misma que el resto de repasos.
const REQUEST_INTERVAL: Duration = Duration::from_millis(1_500);

const LAST_AUTO_KEY: &str = "achievements.last_auto_pass";

/// Cada cuánto se vuelve mientras quede biblioteca por preguntar.
const CATCHUP_INTERVAL_MINUTES: i64 = 10;

/// Qué dejó una pasada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AchievementsPassReport {
    /// Juegos preguntados.
    pub checked: u32,
    /// Juegos que recibieron su recuento.
    pub resolved: u32,
    /// La cuenta no tiene logros ahí, o el perfil no los publica.
    pub unavailable: u32,
    /// No se pudo preguntar.
    pub failed: u32,
    /// Cuántos quedan después de esta tanda.
    pub pending: u32,
}

/// Pregunta por una tanda de juegos sin logros conocidos.
pub async fn run(database: &Database, limit: u32) -> AppResult<AchievementsPassReport> {
    let limit = if limit == 0 { BATCH_SIZE } else { limit };
    let mut report = AchievementsPassReport::default();

    // Sin cuenta no hay a quién preguntarle por «tus» logros.
    let Some(account) = database.get_steam_account()? else {
        return Ok(report);
    };

    let candidatos = database.achievements_pending(limit)?;
    for (indice, app_id) in candidatos.iter().enumerate() {
        if indice > 0 {
            sleep(REQUEST_INTERVAL).await;
        }
        report.checked = report.checked.saturating_add(1);
        match achievements::fetch_saved(&account.steam_id, *app_id).await {
            Ok(AchievementOutcome::Found(summary)) => {
                // Devuelve la ficha entera porque la orden que lo llama la
                // enseña; aquí sólo interesa si se guardó.
                match database.save_achievements(*app_id, summary.unlocked, summary.total) {
                    Ok(_) => report.resolved = report.resolved.saturating_add(1),
                    Err(_) => report.failed = report.failed.saturating_add(1),
                }
            }
            // «No hay logros para esta cuenta en este juego» es una respuesta,
            // no un fallo: se apunta para no volver a preguntarlo mañana.
            Ok(AchievementOutcome::Unavailable) => {
                let _ = database.mark_achievements_attempt(*app_id, "unavailable");
                report.unavailable = report.unavailable.saturating_add(1);
            }
            Err(error) => {
                let _ = database.mark_achievements_attempt(*app_id, "failed");
                report.failed = report.failed.saturating_add(1);
                // Una clave que falta o una cuenta sin permiso no se arreglan
                // insistiendo mil veces: la tanda se corta y se reintenta en la
                // siguiente ronda.
                if matches!(
                    error.code.as_str(),
                    "steam_api_key_missing" | "steam_api_forbidden"
                ) {
                    break;
                }
            }
        }
    }

    report.pending = database.achievements_pending_count()?;
    Ok(report)
}

/// Repasa si toca, sin que nadie lo pida.
pub async fn run_if_due(database: &Database) -> AppResult<Option<AchievementsPassReport>> {
    if database.achievements_pending_count()? == 0 {
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
    use crate::models::{ACHIEVEMENTS_DUE_SQL, ES_DE_STEAM};
    use rusqlite::Connection;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        connection
    }

    /// Un juego con la respuesta que se quiera y la antigüedad que se quiera.
    fn juego(
        connection: &Connection,
        app_id: u32,
        tienda: Option<&str>,
        total: Option<u32>,
        estado: Option<&str>,
        hace_horas: Option<i64>,
    ) {
        connection
            .execute(
                "INSERT INTO games(app_id, title, external_store, achievements_total,
                                   achievements_status, achievements_fetched_at)
                 VALUES (?1, ?2, ?3, ?4, COALESCE(?5, 'pending'),
                         CASE WHEN ?6 IS NULL THEN NULL
                              ELSE datetime('now', '-' || ?6 || ' hours') END)",
                rusqlite::params![
                    app_id,
                    format!("Juego {app_id}"),
                    tienda,
                    total,
                    estado,
                    hace_horas
                ],
            )
            .expect("insertar juego");
    }

    /// La misma cola del módulo, sobre una conexión de prueba.
    fn cola(connection: &Connection) -> Vec<u32> {
        let mut statement = connection
            .prepare(&format!(
                "SELECT g.app_id
                   FROM games g
                  WHERE {ES_DE_STEAM}
                    AND {ACHIEVEMENTS_DUE_SQL}
                  ORDER BY (COALESCE(g.achievements_total, 0) > 0) DESC,
                           COALESCE(g.achievements_fetched_at, '') ASC,
                           g.app_id ASC
                  LIMIT 100"
            ))
            .expect("preparar");
        statement
            .query_map([], |row| row.get::<_, u32>(0))
            .expect("consultar")
            .collect::<Result<Vec<_>, _>>()
            .expect("filas")
    }

    /// Quién entra en la cola, y en qué orden.
    ///
    /// Primero los que tienen logros publicados —donde el recuento va a decir
    /// algo—, y nunca los de otra tienda: su identificador se lo inventó
    /// Vindexa y Steam no sabe nada de él.
    #[test]
    fn la_cola_va_a_los_que_tienen_logros_y_nunca_a_los_de_otra_tienda() {
        let connection = database();
        juego(&connection, 10, None, Some(42), None, None);
        juego(&connection, 20, None, None, None, None);
        juego(&connection, 30, Some("epic"), Some(15), None, None);

        assert_eq!(
            cola(&connection),
            vec![10, 20],
            "primero el que tiene logros, después el que no consta, y el de Epic nunca"
        );
    }

    /// Lo que ya contestó no se repregunta hasta que caduca, con los mismos
    /// plazos que usa la ficha: la pasada de fondo y la pantalla no pueden
    /// discrepar sobre qué está al día.
    #[test]
    fn una_respuesta_reciente_no_vuelve_a_la_cola() {
        let connection = database();
        juego(&connection, 10, None, Some(42), Some("success"), Some(1));
        juego(&connection, 20, None, Some(42), Some("success"), Some(7));
        juego(&connection, 30, None, Some(0), Some("unavailable"), Some(5));
        juego(
            &connection,
            40,
            None,
            Some(0),
            Some("unavailable"),
            Some(30),
        );
        juego(&connection, 50, None, Some(42), Some("failed"), Some(2));

        let pendientes = cola(&connection);
        assert!(!pendientes.contains(&10), "hace una hora que se preguntó");
        assert!(pendientes.contains(&20), "siete horas es más de seis");
        assert!(
            !pendientes.contains(&30),
            "«no hay logros» se respeta un día"
        );
        assert!(pendientes.contains(&40), "treinta horas es más de un día");
        assert!(
            pendientes.contains(&50),
            "un fallo se reintenta a la media hora"
        );
    }
}
