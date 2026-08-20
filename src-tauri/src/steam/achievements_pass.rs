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

/// Errores ante los que la tanda se corta en vez de seguir preguntando.
///
/// Una clave que falta, una que Steam rechaza, un límite de peticiones o una
/// conexión que no se pudo preparar dan la misma respuesta para los mil
/// seiscientos juegos siguientes. Los cuatro los emite [`achievements`]; una
/// prueba de aquí abajo comprueba que siguen existiendo, porque un código
/// inventado se lee igual de bien y no corta nada.
const NEGATIVAS_QUE_NO_SE_ARREGLAN_INSISTIENDO: [&str; 4] = [
    "steam_api_key_missing",
    "steam_api_unauthorized",
    "steam_rate_limited",
    "steam_achievements_http_client",
];

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
    /// No se preguntó nada porque la clave todavía no está cargada.
    ///
    /// No es un fallo ni un «no había trabajo»: es una pasada que se apaga sola
    /// y tiene que poder decirse, en vez de parecer que no hay nada que hacer.
    pub waiting_for_key: bool,
}

/// Pregunta por una tanda de juegos sin logros conocidos.
pub async fn run(database: &Database, limit: u32) -> AppResult<AchievementsPassReport> {
    let limit = if limit == 0 { BATCH_SIZE } else { limit };
    let mut report = AchievementsPassReport::default();

    // Sin cuenta no hay a quién preguntarle por «tus» logros.
    let Some(account) = database.get_steam_account()? else {
        report.pending = database.achievements_pending_count()?;
        return Ok(report);
    };

    // Y sin la clave **ya cargada**, esto no se hace.
    //
    // Leerla del llavero puede abrir un diálogo de contraseña del sistema, y
    // una tarea de fondo no tiene derecho a eso: aparece sola, en mitad de otra
    // cosa, y si no hay nadie delante se queda esperando o se deniega. Pasó
    // exactamente así. Se espera a que un acto explícito —verificar la clave,
    // sincronizar— la cargue; entonces la siguiente ronda la encuentra
    // recordada y sigue sin preguntar nada a nadie.
    let Some(api_key) = crate::steam::secrets::cached_api_key() else {
        report.waiting_for_key = true;
        // Y con el recuento de verdad: «quedan 0 por preguntar» al lado de
        // «esperando la clave» se lee como que ya no hay nada que hacer, que es
        // lo contrario de lo que pasa.
        report.pending = database.achievements_pending_count()?;
        return Ok(report);
    };

    let candidatos = database.achievements_pending(limit)?;
    for (indice, app_id) in candidatos.iter().enumerate() {
        if indice > 0 {
            sleep(REQUEST_INTERVAL).await;
        }
        report.checked = report.checked.saturating_add(1);
        // Con la clave en la mano: `fetch_saved` volvería al llavero en cada
        // juego, y eso es justo lo que no puede pasar aquí.
        match achievements::fetch(&api_key, &account.steam_id, *app_id).await {
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
                // Hay tres negativas que no se arreglan insistiendo, y una
                // biblioteca entera insistiendo son mil seiscientas peticiones
                // inútiles: la tanda se corta y se reintenta en la siguiente
                // ronda. Los códigos son los que emite `achievements.rs`; no se
                // inventan aquí.
                if NEGATIVAS_QUE_NO_SE_ARREGLAN_INSISTIENDO.contains(&error.code.as_str()) {
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
    // Una pasada que no llegó a preguntar nada no gasta el turno: si la clave
    // aparece dentro de un minuto, la siguiente ronda empieza de verdad.
    if !report.waiting_for_key {
        mark_done(database)?;
    }
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

    /// Ninguna salida temprana puede decir «quedan 0».
    ///
    /// La primera versión salía sin rellenar el recuento, así que el aviso se
    /// leía «los logros esperan a que se cargue la clave; quedan 0 juegos por
    /// preguntar» con tres mil quinientos esperando. Un recuento que no
    /// coincide con lo que pasa es peor que no darlo.
    #[test]
    fn ninguna_salida_temprana_deja_el_recuento_a_cero() {
        let completo = include_str!("achievements_pass.rs");
        let cuerpo = completo
            .split("pub async fn run(")
            .nth(1)
            .expect("la función existe")
            .split("/// Repasa si toca")
            .next()
            .expect("hasta la siguiente");
        for (indice, trozo) in cuerpo.split("return Ok(report);").enumerate() {
            // El último trozo es lo que va después del último `return`.
            if indice + 1 == cuerpo.matches("return Ok(report);").count() + 1 {
                break;
            }
            assert!(
                trozo.contains("report.pending = database.achievements_pending_count()?"),
                "una salida de `run` no dice cuántos quedan"
            );
        }
    }

    /// Una tarea de fondo no abre diálogos de contraseña.
    ///
    /// Leer la clave del llavero puede abrir uno del sistema. Quien pulsa
    /// «verificar» está delante y puede contestarlo; una pasada que arranca
    /// sola a los seis minutos, no: aparece en mitad de otra cosa y, si no hay
    /// nadie, se queda esperando o se deniega. Pasó tal cual.
    ///
    /// Esta prueba lee el módulo para comprobar que la pasada usa la clave ya
    /// recordada y **no** la que iría al llavero.
    #[test]
    fn la_pasada_no_va_al_llavero_ella_sola() {
        // Sólo el módulo, sin sus pruebas: aquí abajo se nombra a las dos
        // funciones a propósito y la comprobación se encontraría a sí misma.
        let completo = include_str!("achievements_pass.rs");
        let fuente = completo
            .split("#[cfg(test)]")
            .next()
            .expect("el módulo antes de sus pruebas");
        assert!(
            fuente.contains("secrets::cached_api_key()"),
            "la pasada tiene que usar la clave ya recordada"
        );
        // Se mira el código, no los comentarios: aquí abajo se nombra a
        // `fetch_saved` a propósito para explicar por qué no se usa.
        let llamadas: Vec<&str> = fuente
            .lines()
            .map(str::trim)
            .filter(|linea| !linea.starts_with("//") && !linea.starts_with("///"))
            .filter(|linea| linea.contains("fetch_saved("))
            .collect();
        assert!(
            llamadas.is_empty(),
            "`fetch_saved` vuelve al llavero en cada juego: {llamadas:?}"
        );
        // Y `cached_api_key` no puede acabar consultando el llavero por su
        // cuenta: si un día lo hiciera, esta prueba deja de tener sentido.
        let secretos = include_str!("secrets.rs");
        let cuerpo = secretos
            .split("pub fn cached_api_key")
            .nth(1)
            .expect("la función existe")
            .split("pub fn load_api_key")
            .next()
            .expect("hasta la siguiente función");
        assert!(
            !cuerpo.contains("keychain::"),
            "`cached_api_key` no puede tocar el llavero"
        );
    }

    /// Sonda contra una copia de la base real, a mano.
    ///
    /// No toca la base de nadie: se le pasa la ruta de una copia por
    /// `VINDEXA_BASE_DE_PRUEBA`, y sin esa variable no hace nada.
    #[test]
    #[ignore = "necesita una copia de una base real en VINDEXA_BASE_DE_PRUEBA"]
    fn cuantos_quedan_en_una_base_de_verdad() {
        let Ok(ruta) = std::env::var("VINDEXA_BASE_DE_PRUEBA") else {
            return;
        };
        let database = crate::db::Database::new(std::path::PathBuf::from(ruta));
        let pendientes = database
            .achievements_pending_count()
            .expect("contar pendientes");
        let cola = database.achievements_pending(5).expect("cola");
        let cuenta = database.get_steam_account().expect("cuenta");
        eprintln!(
            "pendientes={pendientes} · cola={cola:?} · cuenta={}",
            cuenta.map_or("(ninguna)".to_string(), |c| format!(
                "vinculada, id de {} caracteres",
                c.steam_id.len()
            ))
        );
    }

    /// Los códigos con los que se corta la tanda tienen que existir.
    ///
    /// El primer intento de esto usaba `steam_api_forbidden`, que no lo emite
    /// nadie: compilaba, se leía bien y no cortaba nada. Un identificador
    /// escrito de memoria es una suposición, y las suposiciones se comprueban.
    #[test]
    fn los_codigos_que_cortan_la_tanda_los_emite_de_verdad_el_modulo_de_logros() {
        let fuente = include_str!("achievements.rs");
        for codigo in super::NEGATIVAS_QUE_NO_SE_ARREGLAN_INSISTIENDO {
            assert!(
                fuente.contains(&format!("\"{codigo}\"")),
                "«{codigo}» no lo emite achievements.rs: la tanda no se cortaría nunca por él"
            );
        }
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
