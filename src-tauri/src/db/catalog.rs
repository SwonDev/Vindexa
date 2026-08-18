//! Catálogo: los juegos que quieres y todavía no tienes (migración 030).
//!
//! # Por qué existe una tabla aparte
//!
//! La biblioteca es `games`, y cada fila de `games` afirma que ese juego es
//! tuyo: su columna `ownership_source` sólo admite `owned`, `family_shared` y
//! `local`. Un juego deseado no es ninguna de las tres cosas, y colarlo ahí con
//! cualquiera de esos valores sería inventar una propiedad que no existe.
//!
//! Tampoco basta con marcarlo y filtrarlo: hay noventa y una consultas
//! repartidas por el proyecto que leen `games` dando por hecho que cada fila es
//! de la biblioteca —la lista, los recuentos, la búsqueda, el planificador, las
//! colecciones inteligentes, la prioridad, el seguimiento, el índice del agente,
//! el emparejamiento con otras tiendas—. Olvidar el filtro en una sola bastaría
//! para enseñar un juego que no tienes en medio de tu biblioteca.
//!
//! Con el catálogo fuera de `games` no hay filtro que olvidar: lo que no está en
//! `games` no lo puede ver ninguna consulta de biblioteca.
//!
//! # Qué sabe el catálogo
//!
//! El AppID, el nombre publicado por la tienda y de dónde salió. Nada más. La
//! portada y la cabecera se **derivan** del AppID con las mismas funciones que
//! usa el escaneo local de Steam, sin red y sin guardar una copia que pueda
//! quedarse desfasada respecto a la fórmula que la genera.

use crate::error::{AppError, AppResult};
use crate::steam::local::{cover_url, header_url};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

/// Procedencias admitidas de una ficha de catálogo; coinciden con el `CHECK` de
/// la migración 030.
pub const CATALOG_SOURCES: [&str; 2] = ["steam_wishlist", "manual"];

/// Longitud máxima del nombre que aceptamos de la tienda.
pub const MAX_CATALOG_TITLE_LENGTH: usize = 200;

/// Un juego que no está en la biblioteca.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogGame {
    pub app_id: u32,
    pub title: String,
    /// Derivada del AppID, nunca guardada ni pedida por red.
    pub cover_url: String,
    /// Derivada del AppID, nunca guardada ni pedida por red.
    pub header_url: String,
    pub source: String,
    pub first_seen_at: String,
    pub updated_at: String,
}

/// Devuelve la ficha de catálogo de un juego, si la tiene.
pub(crate) fn catalog_game(connection: &Connection, app_id: u32) -> AppResult<Option<CatalogGame>> {
    Ok(connection
        .query_row(
            "SELECT app_id, title, source, first_seen_at, updated_at
               FROM catalog_games WHERE app_id = ?1",
            [app_id],
            map_catalog_game,
        )
        .optional()?)
}

/// ¿Está este juego en el catálogo?
pub(crate) fn is_in_catalog(connection: &Connection, app_id: u32) -> AppResult<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM catalog_games WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Da de alta —o refresca el nombre de— una ficha de catálogo.
///
/// Refrescar el nombre es la única concesión: la tienda puede renombrar un
/// juego y el catálogo no tiene otra fuente. El resto de la fila no se toca,
/// empezando por `first_seen_at`, que responde a cuándo lo supimos y no cambia
/// porque volvamos a mirarlo.
///
/// Devuelve `true` si la ficha es nueva.
pub(crate) fn upsert_catalog_game(
    connection: &Connection,
    app_id: u32,
    title: &str,
    source: &str,
) -> AppResult<bool> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID indicado no es válido."));
    }
    let title = validate_catalog_title(title)?;
    if !CATALOG_SOURCES.contains(&source) {
        return Err(AppError::validation(
            "La procedencia de la ficha de catálogo no es válida.",
        ));
    }
    if in_library(connection, app_id)? {
        return Err(AppError::validation(format!(
            "El juego {app_id} ya está en la biblioteca."
        )));
    }
    let existed = is_in_catalog(connection, app_id)?;
    connection.execute(
        "INSERT INTO catalog_games(app_id, title, source)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(app_id) DO UPDATE SET
            title = excluded.title,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![app_id, title, source],
    )?;
    Ok(!existed)
}

/// Retira una ficha de catálogo. El `ON DELETE CASCADE` de la migración 030 se
/// lleva por delante su entrada de deseados.
pub(crate) fn delete_catalog_game(connection: &Connection, app_id: u32) -> AppResult<()> {
    connection.execute("DELETE FROM catalog_games WHERE app_id = ?1", [app_id])?;
    Ok(())
}

/// ¿Tiene este juego ficha en la biblioteca?
pub(crate) fn in_library(connection: &Connection, app_id: u32) -> AppResult<bool> {
    Ok(connection
        .query_row("SELECT 1 FROM games WHERE app_id = ?1", [app_id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn validate_catalog_title(value: &str) -> AppResult<String> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > MAX_CATALOG_TITLE_LENGTH {
        return Err(AppError::validation(format!(
            "El nombre del juego debe tener entre 1 y {MAX_CATALOG_TITLE_LENGTH} caracteres."
        )));
    }
    Ok(value.to_string())
}

fn map_catalog_game(row: &Row<'_>) -> rusqlite::Result<CatalogGame> {
    let app_id: u32 = row.get(0)?;
    Ok(CatalogGame {
        app_id,
        title: row.get(1)?,
        cover_url: cover_url(app_id),
        header_url: header_url(app_id),
        source: row.get(2)?,
        first_seen_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrations, seed_defaults};

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar valores por defecto");
        connection
    }

    fn count(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM catalog_games", [], |row| row.get(0))
            .expect("contar el catálogo")
    }

    #[test]
    fn catalog_art_is_derived_from_the_app_id_without_network() {
        let connection = database();
        upsert_catalog_game(&connection, 570, "Dota 2", "steam_wishlist").expect("dar de alta");
        let game = catalog_game(&connection, 570)
            .expect("consultar")
            .expect("ficha presente");

        assert_eq!(game.title, "Dota 2");
        assert_eq!(game.cover_url, cover_url(570));
        assert_eq!(game.header_url, header_url(570));
        assert_eq!(game.source, "steam_wishlist");
    }

    #[test]
    fn upsert_refreshes_the_title_without_moving_the_first_sighting() {
        let connection = database();
        assert!(upsert_catalog_game(&connection, 10, "Nombre viejo", "steam_wishlist").unwrap());
        let first = catalog_game(&connection, 10).unwrap().unwrap();

        assert!(!upsert_catalog_game(&connection, 10, "Nombre nuevo", "steam_wishlist").unwrap());
        let second = catalog_game(&connection, 10).unwrap().unwrap();

        assert_eq!(second.title, "Nombre nuevo");
        assert_eq!(second.first_seen_at, first.first_seen_at);
        assert_eq!(count(&connection), 1);
    }

    #[test]
    fn a_game_in_the_library_cannot_enter_the_catalog() {
        let connection = database();
        connection
            .execute("INSERT INTO games(app_id, title) VALUES (10, 'Poseído')", [])
            .expect("crear juego de biblioteca");

        let rejected = upsert_catalog_game(&connection, 10, "Poseído", "steam_wishlist")
            .expect_err("rechazar duplicado con la biblioteca");
        assert_eq!(rejected.code, "validation");

        // La misma invariante la defiende el esquema, aunque alguien escriba
        // saltándose esta función.
        let raw = connection.execute(
            "INSERT INTO catalog_games(app_id, title) VALUES (10, 'Poseído')",
            [],
        );
        assert!(raw.is_err());
    }

    #[test]
    fn catalog_rejects_empty_titles_and_unknown_sources() {
        let connection = database();
        assert_eq!(
            upsert_catalog_game(&connection, 10, "   ", "steam_wishlist")
                .expect_err("rechazar título vacío")
                .code,
            "validation"
        );
        assert_eq!(
            upsert_catalog_game(&connection, 10, "Juego", "inventado")
                .expect_err("rechazar procedencia desconocida")
                .code,
            "validation"
        );
        assert_eq!(
            upsert_catalog_game(&connection, 0, "Juego", "manual")
                .expect_err("rechazar AppID cero")
                .code,
            "validation"
        );
    }
}
