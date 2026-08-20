//! Las capturas que se enseñan al pasar el ratón por encima de un juego.
//!
//! # Qué resuelve
//!
//! Una carátula dice cómo se llama un juego; no dice cómo se ve. Al mirar una
//! lista de setenta ofertas, eso es exactamente lo que hace falta para decidir.
//! Steam lo resolvió con un emergente que pasa capturas, y aquí se hace igual.
//!
//! # De dónde salen
//!
//! 1. De `game_media`, si el juego está en la biblioteca y ya se enriqueció.
//! 2. De esta caché, para todo lo demás —los deseados que aún no se poseen, que
//!    son mil trescientos y no están en `games`—.
//! 3. Si no hay ninguna de las dos, se le piden a la tienda **una vez** y se
//!    guardan. La petición filtrada por `screenshots` pesa menos de un kilobyte:
//!    medido sobre Hollow Knight: Silksong, 588 bytes para diez capturas.
//!
//! Cada juego vive en un sitio: si está en `game_media` no se copia aquí.
//!
//! # Un juego sin capturas es un dato, no un hueco
//!
//! `preview_screenshot_checks` recuerda que ya se preguntó. Sin esa marca, un
//! juego retirado de la tienda se preguntaría en cada pasada del ratón.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Cuántas capturas se guardan por juego. La vista rápida enseña cuatro o cinco
/// antes de que nadie siga moviendo el ratón; guardar veinte sería guardar para
/// nadie.
pub const MAX_PREVIEW_SHOTS: usize = 6;

/// Lo que la vista rápida necesita saber de un juego.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePreview {
    pub app_id: u32,
    /// Miniaturas en orden. Vacío significa «no hay», no «aún no se sabe».
    pub screenshots: Vec<String>,
    /// Ya se preguntó a la tienda por este juego.
    pub checked: bool,
}

/// Lo que hay guardado, sin salir a la red.
///
/// Devuelve `checked: false` cuando nunca se ha preguntado, que es la señal para
/// que quien llama decida si merece la pena preguntar ahora.
pub fn stored(connection: &Connection, app_id: u32) -> AppResult<GamePreview> {
    // La biblioteca manda: si el juego se enriqueció, sus capturas ya están y
    // son las mismas que enseña su ficha.
    let mut de_biblioteca = connection.prepare(
        "SELECT COALESCE(thumbnail_url, full_url)
           FROM game_media
          WHERE app_id = ?1 AND kind = 'screenshot'
            AND COALESCE(thumbnail_url, full_url) IS NOT NULL
          ORDER BY position ASC, media_id ASC
          LIMIT ?2",
    )?;
    let biblioteca = de_biblioteca
        .query_map(params![app_id, MAX_PREVIEW_SHOTS as i64], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if !biblioteca.is_empty() {
        return Ok(GamePreview {
            app_id,
            screenshots: biblioteca,
            checked: true,
        });
    }
    drop(de_biblioteca);

    let mut de_cache = connection.prepare(
        "SELECT thumbnail_url FROM preview_screenshots
          WHERE app_id = ?1 ORDER BY position ASC LIMIT ?2",
    )?;
    let cache = de_cache
        .query_map(params![app_id, MAX_PREVIEW_SHOTS as i64], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(de_cache);

    let checked = connection
        .query_row(
            "SELECT 1 FROM preview_screenshot_checks WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    Ok(GamePreview {
        app_id,
        screenshots: cache,
        checked,
    })
}

/// Guarda lo que la tienda ha dicho, incluida la ausencia de capturas.
pub fn save(
    connection: &mut Connection,
    app_id: u32,
    thumbnails: &[String],
    now: DateTime<Utc>,
) -> AppResult<GamePreview> {
    let recortadas: Vec<&String> = thumbnails
        .iter()
        .filter(|url| url.starts_with("https://"))
        .take(MAX_PREVIEW_SHOTS)
        .collect();

    let transaction = connection.transaction()?;
    transaction.execute(
        "DELETE FROM preview_screenshots WHERE app_id = ?1",
        [app_id],
    )?;
    for (position, url) in recortadas.iter().enumerate() {
        transaction.execute(
            "INSERT INTO preview_screenshots(app_id, position, thumbnail_url)
             VALUES (?1, ?2, ?3)",
            params![app_id, position as i64, url],
        )?;
    }
    transaction.execute(
        "INSERT INTO preview_screenshot_checks(app_id, checked_at, found)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(app_id) DO UPDATE SET
             checked_at = excluded.checked_at,
             found = excluded.found",
        params![app_id, now.to_rfc3339(), recortadas.len() as i64],
    )?;
    transaction.commit()?;

    Ok(GamePreview {
        app_id,
        screenshots: recortadas.into_iter().cloned().collect(),
        checked: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use chrono::TimeZone;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        connection
    }

    fn at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 19, 12, 0, 0)
            .single()
            .expect("instante válido")
    }

    fn url(n: u32) -> String {
        format!("https://shared.akamai.steamstatic.com/ss_{n}.600x338.jpg")
    }

    #[test]
    fn un_juego_sin_mirar_lo_dice_en_vez_de_parecer_vacio() {
        // «No hay capturas» y «no lo he mirado» son cosas distintas: sin
        // distinguirlas, la vista rápida preguntaría a la tienda cada vez que el
        // ratón pasa por encima de un juego retirado.
        let connection = database();
        let preview = stored(&connection, 42).expect("leer");
        assert!(preview.screenshots.is_empty());
        assert!(!preview.checked);
    }

    #[test]
    fn un_juego_mirado_y_sin_capturas_no_se_vuelve_a_preguntar() {
        let mut connection = database();
        save(&mut connection, 42, &[], at()).expect("guardar vacío");

        let preview = stored(&connection, 42).expect("leer");
        assert!(preview.screenshots.is_empty());
        assert!(preview.checked, "queda constancia de que se preguntó");
    }

    #[test]
    fn se_guardan_en_orden_y_con_tope() {
        let mut connection = database();
        let muchas: Vec<String> = (0..20).map(url).collect();
        let guardado = save(&mut connection, 7, &muchas, at()).expect("guardar");
        assert_eq!(guardado.screenshots.len(), MAX_PREVIEW_SHOTS);
        assert_eq!(guardado.screenshots[0], url(0));

        let leido = stored(&connection, 7).expect("leer");
        assert_eq!(leido.screenshots, guardado.screenshots);
    }

    #[test]
    fn una_imagen_sin_cifrar_no_entra() {
        let mut connection = database();
        let mezcla = vec!["http://cdn.inseguro/ss_1.jpg".to_string(), url(2)];
        let guardado = save(&mut connection, 8, &mezcla, at()).expect("guardar");
        assert_eq!(guardado.screenshots, vec![url(2)]);
    }

    #[test]
    fn la_biblioteca_manda_sobre_la_cache() {
        // Si el juego está enriquecido, sus capturas ya están en `game_media` y
        // son las mismas que enseña su ficha: copiarlas aquí sería tener el
        // mismo dato en dos sitios que pueden separarse.
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (9, 'Con galería')",
                [],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_media(app_id, media_id, kind, thumbnail_url, position)
                 VALUES (9, 'm1', 'screenshot', ?1, 0)",
                [url(100)],
            )
            .expect("insertar media");
        save(&mut connection, 9, &[url(200)], at()).expect("guardar en caché");

        let preview = stored(&connection, 9).expect("leer");
        assert_eq!(preview.screenshots, vec![url(100)]);
    }

    #[test]
    fn volver_a_guardar_reemplaza_en_vez_de_acumular() {
        let mut connection = database();
        save(&mut connection, 11, &[url(1), url(2), url(3)], at()).expect("primera");
        save(&mut connection, 11, &[url(9)], at()).expect("segunda");

        let preview = stored(&connection, 11).expect("leer");
        assert_eq!(preview.screenshots, vec![url(9)]);
    }
}
