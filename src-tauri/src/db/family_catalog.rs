use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct ImportedFamilyCatalogGame {
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub availability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FamilyCatalogGame {
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub availability: String,
    pub discovered_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FamilyCatalogRequest {
    pub query: Option<String>,
    pub availability: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedFamilyCatalogGames {
    pub items: Vec<FamilyCatalogGame>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

/// Guarda el catálogo en su propia transacción.
///
/// La sincronización general de Steam usa [`save_in_transaction`], porque
/// escribe el catálogo junto al resto de la importación. La sincronización de
/// Familia por sesión no tiene con qué compartir transacción, así que entra por
/// aquí.
pub fn save(
    connection: &mut Connection,
    games: &[ImportedFamilyCatalogGame],
    complete_snapshot: bool,
) -> AppResult<()> {
    let transaction = connection.transaction()?;
    save_in_transaction(&transaction, games, complete_snapshot)?;
    transaction.commit()?;
    Ok(())
}

pub(crate) fn save_in_transaction(
    transaction: &Transaction<'_>,
    games: &[ImportedFamilyCatalogGame],
    complete_snapshot: bool,
) -> AppResult<()> {
    if games.len() > 50_000 {
        return Err(AppError::validation(
            "El catálogo familiar supera el límite seguro de importación.",
        ));
    }
    let mut seen = HashSet::new();
    {
        let mut upsert = transaction.prepare_cached(
            "INSERT INTO family_catalog_games(
                app_id, title, icon_url, cover_url, header_url, availability
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(app_id) DO UPDATE SET
                title = excluded.title,
                icon_url = COALESCE(excluded.icon_url, family_catalog_games.icon_url),
                cover_url = COALESCE(excluded.cover_url, family_catalog_games.cover_url),
                header_url = COALESCE(excluded.header_url, family_catalog_games.header_url),
                availability = excluded.availability,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        for game in games {
            if game.app_id == 0
                || game.title.trim().is_empty()
                || !matches!(game.availability.as_str(), "unknown" | "confirmed")
                || !seen.insert(game.app_id)
            {
                return Err(AppError::validation(
                    "Steam devolvió una entrada familiar no válida o duplicada.",
                ));
            }
            upsert.execute(params![
                game.app_id,
                game.title.trim(),
                game.icon_url,
                game.cover_url,
                game.header_url,
                game.availability,
            ])?;
        }
    }
    // Y a la biblioteca, para que se puedan organizar. `game_personal` cuelga de
    // `games`, así que sin esta fila un juego prestado no tendría dónde guardar
    // su estado, su colección ni su nota. No es una promoción a «tuyo»: entra
    // como `family_shared` y `list_games` lo esconde de la biblioteca propia
    // salvo que se pida el ámbito de Family a propósito.
    {
        let mut upsert_biblioteca = transaction.prepare_cached(
            "INSERT INTO games(
                app_id, title, icon_url, cover_url, header_url,
                playtime_minutes, playtime_recent_minutes,
                ownership_source, family_availability
             ) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, 'family_shared', ?6)
             ON CONFLICT(app_id) DO UPDATE SET
                title = excluded.title,
                icon_url = COALESCE(excluded.icon_url, games.icon_url),
                cover_url = COALESCE(excluded.cover_url, games.cover_url),
                header_url = COALESCE(excluded.header_url, games.header_url),
                -- Un juego que además se posee no pasa a prestado: la propiedad
                -- manda sobre el préstamo.
                family_availability = CASE
                    WHEN games.ownership_source = 'owned' THEN games.family_availability
                    ELSE excluded.family_availability
                END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        let mut ficha_personal = transaction.prepare_cached(
            "INSERT OR IGNORE INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
        )?;
        for game in games {
            upsert_biblioteca.execute(params![
                game.app_id,
                game.title.trim(),
                game.icon_url,
                game.cover_url,
                game.header_url,
                game.availability,
            ])?;
            ficha_personal.execute([game.app_id])?;
        }
    }

    if complete_snapshot {
        // La evidencia de Steam Family es efímera: que un juego estuviese en la
        // caché local en una sincronización anterior no demuestra que siga
        // disponible ahora. Conservamos la organización personal, pero retiramos
        // la afirmación de disponibilidad antes de aplicar la nueva instantánea.
        transaction.execute(
            "UPDATE games
                SET family_availability = 'unknown',
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE ownership_source = 'family_shared'",
            [],
        )?;
        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS vindexa_seen_family_catalog(
                app_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID;
             DELETE FROM vindexa_seen_family_catalog;",
        )?;
        {
            let mut insert = transaction
                .prepare_cached("INSERT INTO vindexa_seen_family_catalog(app_id) VALUES (?1)")?;
            for app_id in seen {
                insert.execute([app_id])?;
            }
        }
        transaction.execute(
            "DELETE FROM family_catalog_games
              WHERE NOT EXISTS (
                SELECT 1 FROM vindexa_seen_family_catalog seen
                 WHERE seen.app_id = family_catalog_games.app_id
              )",
            [],
        )?;
    }
    Ok(())
}

pub fn list(
    connection: &Connection,
    request: &FamilyCatalogRequest,
) -> AppResult<PagedFamilyCatalogGames> {
    let limit = request.limit.unwrap_or(120).clamp(1, 500);
    let offset = request.offset.unwrap_or(0);
    let query = request.query.as_deref().map(str::trim).unwrap_or_default();
    let availability = request.availability.as_deref().unwrap_or_default();
    let sort = request.sort.as_deref().unwrap_or("availability");
    if !availability.is_empty() && !matches!(availability, "unknown" | "confirmed") {
        return Err(AppError::validation(
            "El filtro de disponibilidad familiar no es válido.",
        ));
    }
    let order_by = match sort {
        "availability" => "availability = 'confirmed' DESC, title COLLATE NOCASE ASC, app_id ASC",
        "alphabetical" => "title COLLATE NOCASE ASC, app_id ASC",
        "alphabeticalDesc" => "title COLLATE NOCASE DESC, app_id ASC",
        "updatedDesc" => "datetime(updated_at) DESC, title COLLATE NOCASE ASC, app_id ASC",
        "discoveredDesc" => "datetime(discovered_at) DESC, title COLLATE NOCASE ASC, app_id ASC",
        _ => {
            return Err(AppError::validation(
                "La ordenación del catálogo familiar no es válida.",
            ));
        }
    };
    let pattern = format!("%{query}%");
    let total = connection.query_row(
        "SELECT COUNT(*) FROM family_catalog_games
          WHERE (?1 = '' OR title LIKE ?2 COLLATE NOCASE)
            AND (?3 = '' OR availability = ?3)",
        params![query, pattern, availability],
        |row| row.get(0),
    )?;
    let mut statement = connection.prepare(&format!(
        "SELECT app_id, title, icon_url, cover_url, header_url, availability,
                discovered_at, updated_at
           FROM family_catalog_games
          WHERE (?1 = '' OR title LIKE ?2 COLLATE NOCASE)
            AND (?3 = '' OR availability = ?3)
          ORDER BY {order_by}
          LIMIT ?4 OFFSET ?5"
    ))?;
    let items = statement
        .query_map(
            params![query, pattern, availability, limit, offset],
            |row| {
                Ok(FamilyCatalogGame {
                    app_id: row.get(0)?,
                    title: row.get(1)?,
                    icon_url: row.get(2)?,
                    cover_url: row.get(3)?,
                    header_url: row.get(4)?,
                    availability: row.get(5)?,
                    discovered_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PagedFamilyCatalogGames {
        items,
        total,
        limit,
        offset,
    })
}

pub fn get(connection: &Connection, app_id: u32) -> AppResult<FamilyCatalogGame> {
    connection
        .query_row(
            "SELECT app_id, title, icon_url, cover_url, header_url, availability,
                    discovered_at, updated_at
               FROM family_catalog_games WHERE app_id = ?1",
            [app_id],
            |row| {
                Ok(FamilyCatalogGame {
                    app_id: row.get(0)?,
                    title: row.get(1)?,
                    icon_url: row.get(2)?,
                    cover_url: row.get(3)?,
                    header_url: row.get(4)?,
                    availability: row.get(5)?,
                    discovered_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en el catálogo familiar."))
}

#[cfg(test)]
mod tests {
    use super::{FamilyCatalogRequest, ImportedFamilyCatalogGame, list, save};
    use crate::db::migrations;
    use rusqlite::Connection;

    fn game(app_id: u32, availability: &str) -> ImportedFamilyCatalogGame {
        ImportedFamilyCatalogGame {
            app_id,
            title: format!("Familiar {app_id}"),
            icon_url: None,
            cover_url: None,
            header_url: None,
            availability: availability.to_string(),
        }
    }

    #[test]
    fn replaces_complete_snapshots_and_expires_stale_confirmation() {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("migrar");
        crate::db::seed_defaults(&mut connection).expect("sembrar estados");
        save(
            &mut connection,
            &[game(10, "confirmed"), game(20, "unknown")],
            true,
        )
        .expect("guardar catálogo");
        save(
            &mut connection,
            &[game(10, "unknown"), game(30, "unknown")],
            true,
        )
        .expect("reemplazar catálogo");
        let page = list(&connection, &FamilyCatalogRequest::default()).expect("listar");
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].app_id, 10);
        assert_eq!(page.items[0].availability, "unknown");
        assert_eq!(page.items[1].app_id, 30);
    }

    #[test]
    fn complete_snapshot_expires_confirmation_in_the_personal_index() {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("migrar");
        crate::db::seed_defaults(&mut connection).expect("sembrar estados");
        connection
            .execute(
                "INSERT INTO games(
                    app_id, title, ownership_source, family_availability
                 ) VALUES (10, 'Familiar 10', 'family_shared', 'confirmed')",
                [],
            )
            .expect("crear juego familiar confirmado");

        save(&mut connection, &[game(10, "unknown")], true).expect("guardar nueva instantánea");

        let availability: String = connection
            .query_row(
                "SELECT family_availability FROM games WHERE app_id = 10",
                [],
                |row| row.get(0),
            )
            .expect("leer disponibilidad");
        assert_eq!(availability, "unknown");
    }

    #[test]
    fn filters_and_sorts_the_family_catalog_with_stable_sql_pagination() {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("migrar");
        crate::db::seed_defaults(&mut connection).expect("sembrar estados");
        save(
            &mut connection,
            &[
                game(40, "unknown"),
                game(10, "confirmed"),
                game(30, "confirmed"),
                game(20, "unknown"),
            ],
            true,
        )
        .expect("guardar catálogo ordenable");
        connection
            .execute_batch(
                "UPDATE family_catalog_games SET
                    title = CASE app_id
                        WHEN 10 THEN 'Delta'
                        WHEN 20 THEN 'Alpha'
                        WHEN 30 THEN 'Charlie'
                        ELSE 'Bravo'
                    END,
                    discovered_at = CASE app_id
                        WHEN 10 THEN '2026-08-10T10:00:00.000Z'
                        WHEN 20 THEN '2026-08-11T10:00:00.000Z'
                        WHEN 30 THEN '2026-08-12T10:00:00.000Z'
                        ELSE '2026-08-13T10:00:00.000Z'
                    END,
                    updated_at = CASE app_id
                        WHEN 10 THEN '2026-08-14T13:00:00.000Z'
                        WHEN 20 THEN '2026-08-14T12:00:00.000Z'
                        WHEN 30 THEN '2026-08-14T11:00:00.000Z'
                        ELSE '2026-08-14T10:00:00.000Z'
                    END;",
            )
            .expect("fijar fechas conocidas");

        let page = |availability: Option<&str>, sort: &str, offset| {
            list(
                &connection,
                &FamilyCatalogRequest {
                    query: None,
                    availability: availability.map(str::to_owned),
                    sort: Some(sort.to_owned()),
                    limit: Some(2),
                    offset: Some(offset),
                },
            )
            .expect("listar catálogo")
        };

        assert_eq!(
            page(None, "alphabetical", 0)
                .items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![20, 40]
        );
        assert_eq!(
            page(None, "alphabetical", 2)
                .items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![30, 10]
        );
        assert_eq!(
            page(None, "alphabeticalDesc", 0)
                .items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![10, 30]
        );
        assert_eq!(
            page(None, "availability", 0)
                .items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![30, 10]
        );
        assert_eq!(
            page(None, "updatedDesc", 0)
                .items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
        assert_eq!(
            page(None, "discoveredDesc", 0)
                .items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![40, 30]
        );
        let confirmed = page(Some("confirmed"), "alphabetical", 0);
        assert_eq!(confirmed.total, 2);
        assert!(
            confirmed
                .items
                .iter()
                .all(|game| game.availability == "confirmed")
        );

        let error = list(
            &connection,
            &FamilyCatalogRequest {
                sort: Some("invented".into()),
                ..FamilyCatalogRequest::default()
            },
        )
        .expect_err("rechazar una ordenación no documentada");
        assert_eq!(error.code, "validation");
    }
}
