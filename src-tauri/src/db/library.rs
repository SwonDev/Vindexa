use crate::error::{AppError, AppResult};
use crate::models::RichGameMetadata;
use crate::models::{
    ActivityItem, BulkUpdateStatusInput, GameDetail, GameListRequest, GameSession, GameSummary,
    LibraryFilterChoice, LibraryFilterOptions, LibraryStats, PagedGames, UpdateGameInput,
    archive_scope_clause, is_valid_archive_scope, is_valid_game_sort,
};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Row, ToSql, Transaction, params, params_from_iter};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ImportedInstallation {
    pub library_path: String,
    pub install_path: String,
    pub size_on_disk: Option<u64>,
    pub build_id: Option<u64>,
    pub last_updated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ImportedGame {
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub playtime_minutes: u64,
    pub playtime_recent_minutes: u64,
    pub last_played_at: Option<String>,
    pub ownership_source: String,
    pub family_availability: String,
    pub installation: Option<ImportedInstallation>,
}

#[derive(Debug, Clone)]
pub struct StoreMetadataUpdate {
    pub short_description: Option<String>,
    pub hero_url: Option<String>,
    pub header_url: Option<String>,
    pub capsule_url: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub achievements_total: Option<u32>,
    pub release_date: Option<String>,
    pub is_free: bool,
    pub is_early_access: bool,
    /// Sistemas en los que la tienda ofrece el juego. `None` es «no se sabe»,
    /// que no es lo mismo que «no compatible»: sin el dato, la interfaz no
    /// puede desaconsejar una instalación.
    pub platform_windows: Option<bool>,
    pub platform_mac: Option<bool>,
    pub platform_linux: Option<bool>,
}

struct GameDetailMetadata {
    hero_url: Option<String>,
    short_description: Option<String>,
    developer: Option<String>,
    publisher: Option<String>,
    genres_json: String,
    categories_json: String,
    status: String,
    fetched_at: Option<String>,
    achievements_status: String,
    achievements_fetched_at: Option<String>,
}

const GAME_SELECT: &str = "
    SELECT g.app_id, g.title, g.icon_url, g.cover_url, g.header_url,
           g.playtime_minutes, g.playtime_recent_minutes, g.last_played_at,
           g.release_date, g.is_early_access, g.steam_deck_status,
           g.achievements_unlocked, g.achievements_total,
           p.installed, i.install_path, i.size_on_disk,
           p.status_id, s.name, s.color, p.progress, p.priority,
           p.pinned, p.tracking, p.rating, p.estimated_minutes,
           p.target_date, p.next_action, p.checkpoint, p.notes, p.manual_position,
           g.is_free, g.ownership_source, g.family_availability,
           (SELECT GROUP_CONCAT(cg.collection_id, CHAR(31))
              FROM collection_games cg
             WHERE cg.app_id = g.app_id) AS collection_ids,
           g.genres_json, g.developer,
           g.platform_windows, g.platform_mac, g.platform_linux
      FROM games g
      JOIN game_personal p ON p.app_id = g.app_id
      JOIN statuses s ON s.id = p.status_id
 LEFT JOIN game_installations i ON i.rowid = (
           SELECT installation.rowid
             FROM game_installations installation
            WHERE installation.app_id = g.app_id AND installation.is_primary = 1
            ORDER BY installation.library_path COLLATE NOCASE ASC
            LIMIT 1
      )";

pub fn library_stats(connection: &Connection) -> AppResult<LibraryStats> {
    // Los juegos de tiendas externas se cuentan aparte: son otra tabla y otra
    // procedencia, y meterlos en la consulta principal obligaría a un `LEFT
    // JOIN` que no aporta nada al resto de las cifras.
    let mut external_store_games = std::collections::BTreeMap::new();
    {
        let mut statement =
            connection.prepare("SELECT store, COUNT(*) FROM external_games GROUP BY store")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            external_store_games.insert(row.get::<_, String>(0)?, row.get::<_, i64>(1)?);
        }
    }

    connection
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(CASE WHEN p.installed = 1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN p.status_id = 'playing' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN p.status_id = 'backlog' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN p.tracking = 1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(g.playtime_minutes), 0),
                    (SELECT COUNT(*) FROM game_archive),
                    -- Lo mismo que enseña el ámbito de Steam Family: los juegos
                    -- prestados que están en la biblioteca. Un recuento que no
                    -- coincide con lo que hay dentro es un fallo.
                    (SELECT COUNT(*) FROM games f
                      WHERE f.ownership_source = 'family_shared')
             FROM games g JOIN game_personal p ON p.app_id = g.app_id
            WHERE NOT (
                g.ownership_source = 'family_shared'
                AND g.family_availability <> 'confirmed'
            )
              AND NOT EXISTS (SELECT 1 FROM game_archive a WHERE a.app_id = g.app_id)",
            [],
            |row| {
                Ok(LibraryStats {
                    total_games: row.get(0)?,
                    installed_games: row.get(1)?,
                    playing_games: row.get(2)?,
                    backlog_games: row.get(3)?,
                    tracked_games: row.get(4)?,
                    total_playtime_minutes: row.get(5)?,
                    archived_games: row.get(6)?,
                    family_catalog_games: row.get(7)?,
                    external_store_games: external_store_games.clone(),
                })
            },
        )
        .map_err(Into::into)
}

pub fn filter_options(connection: &Connection) -> AppResult<LibraryFilterOptions> {
    let (total_games, metadata_games, achievement_games, steam_deck_games) = connection.query_row(
        "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE metadata_status = 'success'),
                COUNT(*) FILTER (WHERE achievements_status = 'success'),
                COUNT(*) FILTER (WHERE steam_deck_status IS NOT NULL)
           FROM games
          WHERE NOT (
              ownership_source = 'family_shared'
              AND family_availability <> 'confirmed'
          )",
        [],
        |row| {
            Ok((
                get_u64(row, 0)?,
                get_u64(row, 1)?,
                get_u64(row, 2)?,
                get_u64(row, 3)?,
            ))
        },
    )?;
    let genres = distinct_metadata_values(connection, "genres_json")?;
    let categories = distinct_metadata_values(connection, "categories_json")?;
    let tags = {
        let mut statement =
            connection.prepare("SELECT id, name FROM tags ORDER BY name COLLATE NOCASE")?;
        statement
            .query_map([], |row| {
                Ok(LibraryFilterChoice {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(LibraryFilterOptions {
        genres,
        categories,
        tags,
        total_games,
        metadata_games,
        achievement_games,
        steam_deck_games,
    })
}

fn distinct_metadata_values(connection: &Connection, column: &str) -> AppResult<Vec<String>> {
    debug_assert!(matches!(column, "genres_json" | "categories_json"));
    let sql = format!(
        "SELECT DISTINCT CAST(metadata.value AS TEXT)
           FROM games g,
                json_each(CASE WHEN json_valid(g.{column}) THEN g.{column} ELSE '[]' END) metadata
          WHERE g.metadata_status = 'success'
            AND NOT (
                g.ownership_source = 'family_shared'
                AND g.family_availability <> 'confirmed'
            )
            AND metadata.type = 'text'
            AND trim(CAST(metadata.value AS TEXT)) <> ''
          ORDER BY CAST(metadata.value AS TEXT) COLLATE NOCASE"
    );
    let mut statement = connection.prepare(&sql)?;
    Ok(statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn validate_filter_request(request: &GameListRequest) -> AppResult<()> {
    validate_range(
        request.min_playtime_minutes,
        request.max_playtime_minutes,
        "horas jugadas",
    )?;
    validate_bounded_range(request.min_progress, request.max_progress, 100, "progreso")?;
    validate_bounded_range(request.min_rating, request.max_rating, 10, "valoración")?;
    validate_bounded_range(
        request.min_achievement_percent,
        request.max_achievement_percent,
        100,
        "logros",
    )?;
    validate_range(
        request.min_session_minutes,
        request.max_session_minutes,
        "tamaño de sesión",
    )?;
    validate_date_range(
        request.release_from.as_deref(),
        request.release_to.as_deref(),
        "fecha de lanzamiento",
    )?;
    validate_date_range(
        request.last_played_from.as_deref(),
        request.last_played_to.as_deref(),
        "última vez jugado",
    )?;
    validate_date_range(
        request.target_date_from.as_deref(),
        request.target_date_to.as_deref(),
        "fecha objetivo",
    )?;
    for (value, label) in [
        (request.genre.as_deref(), "género"),
        (request.category.as_deref(), "categoría"),
        (request.tag_id.as_deref(), "etiqueta"),
    ] {
        if let Some(value) = value
            && (value.trim().is_empty() || value.chars().count() > 120)
        {
            return Err(AppError::validation(format!(
                "El filtro de {label} no es válido."
            )));
        }
    }
    if let Some(status) = request.steam_deck_status.as_deref()
        && !matches!(status, "verified" | "playable" | "unsupported" | "unknown")
    {
        return Err(AppError::validation(
            "El filtro de compatibilidad con Steam Deck no es válido.",
        ));
    }
    Ok(())
}

fn validate_range<T: PartialOrd + Copy>(
    min: Option<T>,
    max: Option<T>,
    label: &str,
) -> AppResult<()> {
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(AppError::validation(format!(
            "El rango de {label} tiene el mínimo por encima del máximo."
        )));
    }
    Ok(())
}

fn validate_bounded_range<T>(min: Option<T>, max: Option<T>, limit: T, label: &str) -> AppResult<()>
where
    T: PartialOrd + Copy,
{
    if min.is_some_and(|value| value > limit) || max.is_some_and(|value| value > limit) {
        return Err(AppError::validation(format!(
            "El filtro de {label} supera el límite permitido."
        )));
    }
    validate_range(min, max, label)
}

fn validate_date_range(from: Option<&str>, to: Option<&str>, label: &str) -> AppResult<()> {
    let parse = |value: &str| {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
            AppError::validation(format!("El filtro de {label} debe usar una fecha válida."))
        })
    };
    let from = from.map(parse).transpose()?;
    let to = to.map(parse).transpose()?;
    validate_range(from, to, label)
}

fn push_numeric_filter<T>(
    clauses: &mut Vec<String>,
    values: &mut Vec<Box<dyn ToSql>>,
    sql: &str,
    value: Option<T>,
) where
    T: ToSql + Copy + 'static,
{
    if let Some(value) = value {
        clauses.push(sql.to_string());
        values.push(Box::new(value));
    }
}

fn push_u64_filter(
    clauses: &mut Vec<String>,
    values: &mut Vec<Box<dyn ToSql>>,
    sql: &str,
    value: Option<u64>,
) -> AppResult<()> {
    if let Some(value) = value {
        let value = i64::try_from(value).map_err(|_| {
            AppError::validation("El rango de horas jugadas supera el límite permitido.")
        })?;
        clauses.push(sql.to_string());
        values.push(Box::new(value));
    }
    Ok(())
}

fn push_date_filter(
    clauses: &mut Vec<String>,
    values: &mut Vec<Box<dyn ToSql>>,
    sql: &str,
    value: Option<&str>,
    timestamp_boundary: Option<bool>,
) {
    if let Some(value) = value {
        clauses.push(sql.to_string());
        let value = match timestamp_boundary {
            Some(false) => format!("{value}T00:00:00Z"),
            Some(true) => format!("{value}T23:59:59.999Z"),
            None => value.to_string(),
        };
        values.push(Box::new(value));
    }
}

pub fn list_games(
    connection: &Connection,
    request: &GameListRequest,
    smart_collection_ids: Option<&HashSet<u32>>,
) -> AppResult<PagedGames> {
    validate_filter_request(request)?;
    let limit = request.limit.unwrap_or(120).clamp(1, 500);
    let offset = request.offset.unwrap_or(0);
    // Los juegos prestados sin confirmar se esconden de la biblioteca: tenerlos
    // a la vista no es tenerlos, y mezclarlos con los propios diría que sí. La
    // excepción es pedirlos a propósito —el ámbito de Steam Family—, donde son
    // justo lo que se busca.
    let familia_pedida = request.ownership_source.as_deref() == Some("family_shared");
    let mut clauses: Vec<String> = if familia_pedida {
        Vec::new()
    } else {
        vec![
            "NOT (g.ownership_source = 'family_shared' AND g.family_availability <> 'confirmed')"
                .to_string(),
        ]
    };
    // Lo archivado se esconde salvo que se pida verlo: es el sentido de
    // archivar. Un ámbito desconocido es un error, no un `active` silencioso.
    let archive_scope = request.archive_scope.as_deref().unwrap_or("active");
    if !is_valid_archive_scope(archive_scope) {
        return Err(AppError::validation(
            "El ámbito de archivado de la biblioteca no es válido.",
        ));
    }
    if let Some(clause) = archive_scope_clause(archive_scope) {
        clauses.push(clause.to_string());
    }
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();

    if let Some(query) = request
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
    {
        let pattern = format!("%{query}%");
        clauses.push(
            "(g.title LIKE ? COLLATE NOCASE OR COALESCE(p.notes, '') LIKE ? COLLATE NOCASE OR COALESCE(p.checkpoint, '') LIKE ? COLLATE NOCASE OR COALESCE(p.next_action, '') LIKE ? COLLATE NOCASE)".to_string(),
        );
        for _ in 0..4 {
            values.push(Box::new(pattern.clone()));
        }
    }
    if let Some(status_id) = request.status_id.as_ref() {
        clauses.push("p.status_id = ?".to_string());
        values.push(Box::new(status_id.clone()));
    }
    if let Some(installed) = request.installed {
        clauses.push("p.installed = ?".to_string());
        values.push(Box::new(if installed { 1_i64 } else { 0_i64 }));
    }
    if let Some(tracking) = request.tracking {
        clauses.push("p.tracking = ?".to_string());
        values.push(Box::new(if tracking { 1_i64 } else { 0_i64 }));
    }
    if let Some(early_access) = request.early_access {
        clauses.push("g.metadata_status = 'success' AND g.is_early_access = ?".to_string());
        values.push(Box::new(if early_access { 1_i64 } else { 0_i64 }));
    }
    if let Some(is_free) = request.is_free {
        clauses.push("g.is_free = ?".to_string());
        values.push(Box::new(if is_free { 1_i64 } else { 0_i64 }));
    }
    if let Some(ownership_source) = request.ownership_source.as_deref() {
        if !matches!(ownership_source, "owned" | "family_shared" | "local") {
            return Err(AppError::validation(
                "El origen de propiedad de la biblioteca no es válido.",
            ));
        }
        clauses.push("g.ownership_source = ?".to_string());
        values.push(Box::new(ownership_source.to_string()));
    }
    if let Some(never_played) = request.never_played {
        clauses.push(if never_played {
            "g.playtime_minutes = 0".to_string()
        } else {
            "g.playtime_minutes > 0".to_string()
        });
    }
    push_u64_filter(
        &mut clauses,
        &mut values,
        "g.playtime_minutes >= ?",
        request.min_playtime_minutes,
    )?;
    push_u64_filter(
        &mut clauses,
        &mut values,
        "g.playtime_minutes <= ?",
        request.max_playtime_minutes,
    )?;
    push_numeric_filter(
        &mut clauses,
        &mut values,
        "p.progress >= ?",
        request.min_progress,
    );
    push_numeric_filter(
        &mut clauses,
        &mut values,
        "p.progress <= ?",
        request.max_progress,
    );
    push_numeric_filter(
        &mut clauses,
        &mut values,
        "p.rating >= ?",
        request.min_rating,
    );
    push_numeric_filter(
        &mut clauses,
        &mut values,
        "p.rating <= ?",
        request.max_rating,
    );
    for (value, column) in [
        (request.genre.as_deref(), "g.genres_json"),
        (request.category.as_deref(), "g.categories_json"),
    ] {
        if let Some(value) = value {
            clauses.push(format!(
                "g.metadata_status = 'success' AND EXISTS (
                    SELECT 1
                      FROM json_each(CASE WHEN json_valid({column}) THEN {column} ELSE '[]' END) metadata
                     WHERE CAST(metadata.value AS TEXT) = ? COLLATE NOCASE
                )"
            ));
            values.push(Box::new(value.trim().to_string()));
        }
    }
    if let Some(tag_id) = request.tag_id.as_deref() {
        clauses.push(
            "EXISTS (
                SELECT 1 FROM game_tags gt
                 WHERE gt.app_id = g.app_id AND gt.tag_id = ?
            )"
            .to_string(),
        );
        values.push(Box::new(tag_id.trim().to_string()));
    }
    push_date_filter(
        &mut clauses,
        &mut values,
        "g.metadata_status = 'success' AND g.release_date >= ?",
        request.release_from.as_deref(),
        None,
    );
    push_date_filter(
        &mut clauses,
        &mut values,
        "g.metadata_status = 'success' AND g.release_date <= ?",
        request.release_to.as_deref(),
        None,
    );
    push_date_filter(
        &mut clauses,
        &mut values,
        "g.last_played_at >= ?",
        request.last_played_from.as_deref(),
        Some(false),
    );
    push_date_filter(
        &mut clauses,
        &mut values,
        "g.last_played_at <= ?",
        request.last_played_to.as_deref(),
        Some(true),
    );
    if request.min_achievement_percent.is_some() || request.max_achievement_percent.is_some() {
        clauses.push("g.achievements_status = 'success' AND g.achievements_total > 0".to_string());
    }
    push_numeric_filter(
        &mut clauses,
        &mut values,
        "(100.0 * g.achievements_unlocked / g.achievements_total) >= ?",
        request.min_achievement_percent,
    );
    push_numeric_filter(
        &mut clauses,
        &mut values,
        "(100.0 * g.achievements_unlocked / g.achievements_total) <= ?",
        request.max_achievement_percent,
    );
    if let Some(status) = request.steam_deck_status.as_deref() {
        clauses.push("g.steam_deck_status = ?".to_string());
        values.push(Box::new(status.to_string()));
    }
    push_date_filter(
        &mut clauses,
        &mut values,
        "p.target_date >= ?",
        request.target_date_from.as_deref(),
        None,
    );
    push_date_filter(
        &mut clauses,
        &mut values,
        "p.target_date <= ?",
        request.target_date_to.as_deref(),
        None,
    );
    let average_session_sql = "(
        SELECT AVG((julianday(session.ended_at) - julianday(session.started_at)) * 1440.0)
          FROM game_sessions session
         WHERE session.app_id = g.app_id AND session.ended_at IS NOT NULL
    )";
    if let Some(minutes) = request.min_session_minutes {
        clauses.push(format!("{average_session_sql} >= ?"));
        values.push(Box::new(i64::from(minutes)));
    }
    if let Some(minutes) = request.max_session_minutes {
        clauses.push(format!("{average_session_sql} <= ?"));
        values.push(Box::new(i64::from(minutes)));
    }
    let manual_collection_id = request
        .collection_id
        .as_deref()
        .filter(|_| smart_collection_ids.is_none());
    if let Some(collection_id) = manual_collection_id {
        clauses.push(
            "EXISTS (SELECT 1 FROM collection_games cg WHERE cg.app_id = g.app_id AND cg.collection_id = ?)"
                .to_string(),
        );
        values.push(Box::new(collection_id.to_string()));
    }
    if let Some(allowed) = smart_collection_ids {
        if allowed.is_empty() {
            clauses.push("0 = 1".to_string());
        } else {
            connection.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS vindexa_allowed_game_ids(
                    app_id INTEGER PRIMARY KEY
                 ) WITHOUT ROWID;
                 DELETE FROM vindexa_allowed_game_ids;",
            )?;
            let mut insert = connection
                .prepare_cached("INSERT INTO vindexa_allowed_game_ids(app_id) VALUES (?1)")?;
            for app_id in allowed {
                insert.execute([app_id])?;
            }
            drop(insert);
            clauses.push(
                "EXISTS (
                    SELECT 1 FROM vindexa_allowed_game_ids allowed
                     WHERE allowed.app_id = g.app_id
                )"
                .to_string(),
            );
        }
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    let sort = request.sort.as_deref().unwrap_or("manual");
    if !is_valid_game_sort(sort) {
        return Err(AppError::validation(
            "El criterio de ordenación de la biblioteca no es válido.",
        ));
    }
    let random_order = format!(
        "((g.app_id * 1103515245 + {}) & 2147483647) ASC, g.app_id ASC",
        request.sort_seed.unwrap_or(0)
    );
    let order_sql = match sort {
        "alphabetical" => "g.title COLLATE NOCASE ASC, g.app_id ASC".to_string(),
        "alphabeticalDesc" => "g.title COLLATE NOCASE DESC, g.app_id ASC".to_string(),
        "playtime" => {
            "g.playtime_minutes DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "playtimeAsc" => {
            "g.playtime_minutes ASC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "lastPlayed" => {
            "g.last_played_at IS NULL ASC, g.last_played_at DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "releaseDate" => {
            "g.release_date IS NULL ASC, g.release_date DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "releaseDateAsc" => {
            "g.release_date IS NULL ASC, g.release_date ASC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "progress" => "p.progress DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string(),
        "rating" => "p.rating IS NULL ASC, p.rating DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string(),
        "targetDate" => {
            "p.target_date IS NULL ASC, p.target_date ASC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "recentlyAdded" => {
            "g.imported_at DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "installedFirst" => {
            "p.installed DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "uninstalledFirst" => {
            "p.installed ASC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "sizeDesc" => {
            "i.size_on_disk IS NULL ASC, i.size_on_disk DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "sizeAsc" => {
            "i.size_on_disk IS NULL ASC, i.size_on_disk ASC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string()
        }
        "random" => random_order,
        _ if manual_collection_id.is_some() => {
            "(SELECT cg.position FROM collection_games cg WHERE cg.collection_id = ? AND cg.app_id = g.app_id) ASC, g.app_id ASC".to_string()
        }
        _ => "p.manual_position ASC, p.pinned DESC, p.priority DESC, g.title COLLATE NOCASE ASC, g.app_id ASC".to_string(),
    };

    let total: i64 = connection.query_row(
        &format!(
            "SELECT COUNT(*) FROM games g
             JOIN game_personal p ON p.app_id = g.app_id
             JOIN statuses s ON s.id = p.status_id
        LEFT JOIN game_installations i ON i.rowid = (
                  SELECT installation.rowid
                    FROM game_installations installation
                   WHERE installation.app_id = g.app_id AND installation.is_primary = 1
                   ORDER BY installation.library_path COLLATE NOCASE ASC
                   LIMIT 1
             )
             {where_sql}"
        ),
        params_from_iter(values.iter().map(|value| value.as_ref())),
        |row| row.get(0),
    )?;

    if sort == "manual"
        && let Some(collection_id) = manual_collection_id
    {
        values.push(Box::new(collection_id.to_string()));
    }
    values.push(Box::new(i64::from(limit)));
    values.push(Box::new(i64::from(offset)));
    let mut statement = connection.prepare(&format!(
        "{GAME_SELECT}{where_sql} ORDER BY {order_sql} LIMIT ? OFFSET ?"
    ))?;
    let rows = statement.query_map(
        params_from_iter(values.iter().map(|value| value.as_ref())),
        map_game_summary,
    )?;
    let items = rows.collect::<Result<Vec<_>, _>>()?;
    Ok(PagedGames {
        items,
        total,
        limit,
        offset,
    })
}

pub fn game_summary(connection: &Connection, app_id: u32) -> AppResult<GameSummary> {
    connection
        .query_row(
            &format!("{GAME_SELECT} WHERE g.app_id = ?1"),
            [app_id],
            map_game_summary,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))
}

fn map_game_summary(row: &Row<'_>) -> rusqlite::Result<GameSummary> {
    Ok(GameSummary {
        app_id: row.get(0)?,
        title: row.get(1)?,
        icon_url: row.get(2)?,
        cover_url: row.get(3)?,
        header_url: row.get(4)?,
        playtime_minutes: get_u64(row, 5)?,
        playtime_recent_minutes: get_u64(row, 6)?,
        last_played_at: row.get(7)?,
        release_date: row.get(8)?,
        is_early_access: row.get::<_, i64>(9)? != 0,
        steam_deck_status: row.get(10)?,
        achievements_unlocked: row.get(11)?,
        achievements_total: row.get(12)?,
        installed: row.get::<_, i64>(13)? != 0,
        install_path: row.get(14)?,
        size_on_disk: get_optional_u64(row, 15)?,
        status_id: row.get(16)?,
        status_name: row.get(17)?,
        status_color: row.get(18)?,
        progress: row.get(19)?,
        priority: row.get(20)?,
        pinned: row.get::<_, i64>(21)? != 0,
        tracking: row.get::<_, i64>(22)? != 0,
        rating: row.get(23)?,
        estimated_minutes: row.get(24)?,
        target_date: row.get(25)?,
        next_action: row.get(26)?,
        checkpoint: row.get(27)?,
        notes: row.get(28)?,
        manual_position: row.get(29)?,
        is_free: row.get::<_, i64>(30)? != 0,
        ownership_source: row.get(31)?,
        family_availability: row.get(32)?,
        collection_ids: row
            .get::<_, Option<String>>(33)?
            .map(|joined| {
                joined
                    .split('\u{1f}')
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        genres: row
            .get::<_, Option<String>>(34)?
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
        developer: row.get(35)?,
        platform_windows: row.get::<_, Option<i64>>(36)?.map(|value| value != 0),
        platform_mac: row.get::<_, Option<i64>>(37)?.map(|value| value != 0),
        platform_linux: row.get::<_, Option<i64>>(38)?.map(|value| value != 0),
    })
}

fn get_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
}

fn get_optional_u64(row: &Row<'_>, index: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(index)?
        .map(|value| {
            u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
        })
        .transpose()
}

pub fn get_game_detail(connection: &Connection, app_id: u32) -> AppResult<GameDetail> {
    let summary = game_summary(connection, app_id)?;
    let metadata = connection.query_row(
        "SELECT hero_url, short_description, developer, publisher, genres_json, categories_json,
                metadata_status, metadata_fetched_at,
                achievements_status, achievements_fetched_at
           FROM games WHERE app_id = ?1",
        [app_id],
        |row| {
            Ok(GameDetailMetadata {
                hero_url: row.get(0)?,
                short_description: row.get(1)?,
                developer: row.get(2)?,
                publisher: row.get(3)?,
                genres_json: row.get(4)?,
                categories_json: row.get(5)?,
                status: row.get(6)?,
                fetched_at: row.get(7)?,
                achievements_status: row.get(8)?,
                achievements_fetched_at: row.get(9)?,
            })
        },
    )?;

    let collection_ids = query_strings(
        connection,
        "SELECT collection_id FROM collection_games WHERE app_id = ?1 ORDER BY position",
        app_id,
    )?;
    let tags = query_strings(
        connection,
        "SELECT t.name FROM tags t JOIN game_tags gt ON gt.tag_id = t.id WHERE gt.app_id = ?1 ORDER BY t.name",
        app_id,
    )?;
    let tag_ids = query_strings(
        connection,
        "SELECT tag_id FROM game_tags WHERE app_id = ?1 ORDER BY tag_id",
        app_id,
    )?;
    let (started_at, completed_at, abandoned_at) = connection.query_row(
        "SELECT started_at, completed_at, abandoned_at FROM game_personal WHERE app_id = ?1",
        [app_id],
        |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let sessions = {
        let mut statement = connection.prepare(
            "SELECT id, started_at, ended_at, progress_before, progress_after, note
             FROM game_sessions WHERE app_id = ?1 ORDER BY started_at DESC, id DESC LIMIT 50",
        )?;
        statement
            .query_map([app_id], |row| {
                Ok(GameSession {
                    id: row.get(0)?,
                    started_at: row.get(1)?,
                    ended_at: row.get(2)?,
                    progress_before: row.get(3)?,
                    progress_after: row.get(4)?,
                    note: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    let sessions_total = connection.query_row(
        "SELECT COUNT(*) FROM game_sessions WHERE app_id = ?1",
        [app_id],
        |row| get_u64(row, 0),
    )?;
    let activity = {
        let mut statement = connection.prepare(
            "SELECT id, kind, message, created_at FROM activity
             WHERE app_id = ?1 ORDER BY created_at DESC LIMIT 50",
        )?;
        statement
            .query_map([app_id], |row| {
                Ok(ActivityItem {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    message: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(GameDetail {
        summary,
        hero_url: metadata.hero_url,
        short_description: metadata.short_description,
        developer: metadata.developer,
        publisher: metadata.publisher,
        genres: serde_json::from_str(&metadata.genres_json).unwrap_or_default(),
        categories: serde_json::from_str(&metadata.categories_json).unwrap_or_default(),
        metadata_status: metadata.status,
        metadata_fetched_at: metadata.fetched_at,
        achievements_status: metadata.achievements_status,
        achievements_fetched_at: metadata.achievements_fetched_at,
        collection_ids,
        tags,
        tag_ids,
        sessions,
        sessions_total,
        activity,
        started_at,
        completed_at,
        abandoned_at,
        // La parte enriquecida la compone `Database::detail_with_rich`, que sí
        // puede alcanzar `db::rich_metadata`.
        rich: RichGameMetadata::default(),
    })
}

pub fn store_metadata_refresh_due(connection: &Connection, app_id: u32) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT CASE
                WHEN metadata_fetched_at IS NULL THEN 1
                WHEN metadata_status = 'success'
                    THEN datetime(metadata_fetched_at) < datetime('now', '-7 days')
                WHEN metadata_status = 'unavailable'
                    THEN datetime(metadata_fetched_at) < datetime('now', '-1 day')
                ELSE datetime(metadata_fetched_at) < datetime('now', '-2 hours')
             END
             FROM games WHERE app_id = ?1",
            [app_id],
            |row| row.get::<_, i64>(0).map(|value| value != 0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))
}

pub fn save_store_metadata(
    connection: &Connection,
    app_id: u32,
    metadata: &StoreMetadataUpdate,
) -> AppResult<()> {
    let genres_json = serde_json::to_string(&metadata.genres).map_err(|_| {
        AppError::new(
            "store_metadata_encode",
            "No se pudieron preparar los géneros recibidos de Steam.",
        )
    })?;
    let categories_json = serde_json::to_string(&metadata.categories).map_err(|_| {
        AppError::new(
            "store_metadata_encode",
            "No se pudieron preparar las categorías recibidas de Steam.",
        )
    })?;
    let changed = connection.execute(
        "UPDATE games SET
            short_description = ?2,
            hero_url = COALESCE(?3, hero_url),
            developer = COALESCE(?4, developer),
            publisher = COALESCE(?5, publisher),
            genres_json = ?6,
            categories_json = ?7,
            achievements_total = CASE
                WHEN achievements_status = 'success' THEN achievements_total
                ELSE COALESCE(?8, achievements_total)
            END,
            release_date = COALESCE(?9, release_date),
            is_free = ?10,
            is_early_access = ?11,
            header_url = COALESCE(?12, header_url),
            capsule_url = COALESCE(?13, capsule_url),
            platform_windows = COALESCE(?14, platform_windows),
            platform_mac = COALESCE(?15, platform_mac),
            platform_linux = COALESCE(?16, platform_linux),
            metadata_status = 'success',
            metadata_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE app_id = ?1",
        params![
            app_id,
            metadata.short_description,
            metadata.hero_url,
            metadata.developer,
            metadata.publisher,
            genres_json,
            categories_json,
            metadata.achievements_total,
            metadata.release_date,
            if metadata.is_free { 1_i64 } else { 0_i64 },
            if metadata.is_early_access {
                1_i64
            } else {
                0_i64
            },
            metadata.header_url,
            metadata.capsule_url,
            metadata.platform_windows.map(i64::from),
            metadata.platform_mac.map(i64::from),
            metadata.platform_linux.map(i64::from),
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    Ok(())
}

pub fn achievements_refresh_due(connection: &Connection, app_id: u32) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT CASE
                WHEN achievements_fetched_at IS NULL THEN 1
                WHEN achievements_status = 'success'
                    THEN datetime(achievements_fetched_at) < datetime('now', '-6 hours')
                WHEN achievements_status = 'unavailable'
                    THEN datetime(achievements_fetched_at) < datetime('now', '-1 day')
                ELSE datetime(achievements_fetched_at) < datetime('now', '-30 minutes')
             END
             FROM games WHERE app_id = ?1",
            [app_id],
            |row| row.get::<_, i64>(0).map(|value| value != 0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))
}

pub fn save_achievements(
    connection: &Connection,
    app_id: u32,
    unlocked: u32,
    total: u32,
) -> AppResult<()> {
    if unlocked > total {
        return Err(AppError::validation(
            "Steam devolvió un recuento de logros incoherente.",
        ));
    }
    let changed = connection.execute(
        "UPDATE games SET
            achievements_unlocked = ?2,
            achievements_total = ?3,
            achievements_status = 'success',
            achievements_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE app_id = ?1",
        params![app_id, unlocked, total],
    )?;
    if changed != 1 {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    Ok(())
}

pub fn mark_achievements_attempt(
    connection: &Connection,
    app_id: u32,
    status: &str,
) -> AppResult<()> {
    if !matches!(status, "unavailable" | "failed") {
        return Err(AppError::validation(
            "El estado de los logros no es válido.",
        ));
    }
    let changed = connection.execute(
        "UPDATE games SET
            achievements_status = ?2,
            achievements_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE app_id = ?1",
        params![app_id, status],
    )?;
    if changed != 1 {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    Ok(())
}

pub fn mark_store_metadata_attempt(
    connection: &Connection,
    app_id: u32,
    status: &str,
) -> AppResult<()> {
    if !matches!(status, "unavailable" | "failed") {
        return Err(AppError::validation("El estado de metadatos no es válido."));
    }
    let changed = connection.execute(
        "UPDATE games SET
            metadata_status = ?2,
            metadata_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE app_id = ?1",
        params![app_id, status],
    )?;
    if changed != 1 {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    Ok(())
}

fn query_strings(connection: &Connection, sql: &str, app_id: u32) -> AppResult<Vec<String>> {
    let mut statement = connection.prepare(sql)?;
    let values = statement
        .query_map([app_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

pub fn update_game(connection: &mut Connection, input: &UpdateGameInput) -> AppResult<()> {
    validate_update_game_input(input)?;
    let transaction = connection.transaction()?;
    let status_exists = transaction
        .query_row(
            "SELECT 1 FROM statuses WHERE id = ?1",
            [&input.status_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !status_exists {
        return Err(AppError::validation("El estado seleccionado ya no existe."));
    }
    let changed = transaction.execute(
        "UPDATE game_personal SET
            status_id = ?2, progress = ?3, priority = ?4, pinned = ?5, tracking = ?6,
            rating = ?7, estimated_minutes = ?8, target_date = ?9,
            next_action = ?10, checkpoint = ?11, notes = ?12,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE app_id = ?1",
        params![
            input.app_id,
            input.status_id,
            input.progress,
            input.priority,
            input.pinned,
            input.tracking,
            input.rating,
            input.estimated_minutes,
            clean_optional(&input.target_date),
            clean_optional(&input.next_action),
            clean_optional(&input.checkpoint),
            clean_optional(&input.notes),
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found("No se pudo actualizar el juego."));
    }
    transaction.execute(
        "INSERT INTO activity(id, kind, app_id, message) VALUES (?1, 'personal_update', ?2, ?3)",
        params![
            Uuid::new_v4().to_string(),
            input.app_id,
            "Se actualizó la organización personal."
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn validate_update_game_input(input: &UpdateGameInput) -> AppResult<()> {
    if input.app_id == 0 || input.progress > 100 || input.priority > 5 {
        return Err(AppError::validation(
            "Los valores del juego no son válidos.",
        ));
    }
    if input
        .rating
        .is_some_and(|rating| !(1..=10).contains(&rating))
    {
        return Err(AppError::validation(
            "La valoración debe estar entre 1 y 10.",
        ));
    }

    if input.estimated_minutes == Some(0) {
        return Err(AppError::validation(
            "La duración estimada debe ser mayor que cero.",
        ));
    }

    if let Some(value) = input
        .target_date
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| {
            AppError::validation("La fecha objetivo debe usar el formato AAAA-MM-DD y ser válida.")
        })?;
        if date.format("%Y-%m-%d").to_string() != value {
            return Err(AppError::validation(
                "La fecha objetivo debe usar el formato AAAA-MM-DD y ser válida.",
            ));
        }
    }

    validate_optional_text(
        &input.next_action,
        500,
        "La siguiente acción no puede superar 500 caracteres.",
    )?;
    validate_optional_text(
        &input.checkpoint,
        2_000,
        "El checkpoint no puede superar 2000 caracteres.",
    )?;
    validate_optional_text(
        &input.notes,
        20_000,
        "Las notas no pueden superar 20000 caracteres.",
    )?;
    Ok(())
}

fn validate_optional_text(value: &Option<String>, limit: usize, message: &str) -> AppResult<()> {
    if value
        .as_deref()
        .is_some_and(|value| value.chars().count() > limit)
    {
        return Err(AppError::validation(message));
    }
    Ok(())
}

pub fn bulk_update_status(
    connection: &mut Connection,
    input: &BulkUpdateStatusInput,
) -> AppResult<usize> {
    const MAX_BULK_GAMES: usize = 10_000;

    if input.app_ids.is_empty() {
        return Err(AppError::validation(
            "Selecciona al menos un juego para cambiar su estado.",
        ));
    }
    if input.app_ids.len() > MAX_BULK_GAMES {
        return Err(AppError::validation(
            "La selección es demasiado grande para una sola operación.",
        ));
    }
    let status_id = input.status_id.trim();
    if status_id.is_empty() || status_id.len() > 128 {
        return Err(AppError::validation("El estado seleccionado no es válido."));
    }

    let unique_app_ids = input.app_ids.iter().copied().collect::<HashSet<_>>();
    if unique_app_ids.len() != input.app_ids.len() || unique_app_ids.contains(&0) {
        return Err(AppError::validation(
            "La selección contiene juegos no válidos o duplicados.",
        ));
    }

    let transaction = connection.transaction()?;
    let status_exists = transaction
        .query_row("SELECT 1 FROM statuses WHERE id = ?1", [status_id], |_| {
            Ok(())
        })
        .optional()?
        .is_some();
    if !status_exists {
        return Err(AppError::validation("El estado seleccionado ya no existe."));
    }

    let placeholders = std::iter::repeat_n("?", input.app_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let requested_count = i64::try_from(input.app_ids.len())
        .map_err(|_| AppError::validation("La selección es demasiado grande."))?;
    let existing_count: i64 = transaction.query_row(
        &format!("SELECT COUNT(*) FROM game_personal WHERE app_id IN ({placeholders})"),
        params_from_iter(input.app_ids.iter()),
        |row| row.get(0),
    )?;
    if existing_count != requested_count {
        return Err(AppError::not_found(
            "Uno o más juegos ya no están en la biblioteca.",
        ));
    }

    let mut update = transaction.prepare_cached(
        "UPDATE game_personal
            SET status_id = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
    )?;
    let mut activity = transaction.prepare_cached(
        "INSERT INTO activity(id, kind, app_id, message)
         VALUES (?1, 'personal_update', ?2, ?3)",
    )?;
    for app_id in &input.app_ids {
        let changed = update.execute(params![app_id, status_id])?;
        if changed != 1 {
            return Err(AppError::not_found(
                "No se pudo actualizar toda la selección.",
            ));
        }
        activity.execute(params![
            Uuid::new_v4().to_string(),
            app_id,
            "Se actualizó el estado mediante una operación masiva."
        ])?;
    }
    drop(update);
    drop(activity);
    transaction.commit()?;
    Ok(input.app_ids.len())
}

fn clean_optional(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub fn upsert_imported_games(
    connection: &mut Connection,
    games: &[ImportedGame],
    reset_installed: bool,
) -> AppResult<(usize, usize)> {
    let transaction = connection.transaction()?;
    let result = upsert_imported_games_in_transaction(&transaction, games, reset_installed)?;
    transaction.commit()?;
    Ok(result)
}

pub(crate) fn upsert_imported_games_in_transaction(
    transaction: &Transaction<'_>,
    games: &[ImportedGame],
    reset_installed: bool,
) -> AppResult<(usize, usize)> {
    if reset_installed {
        transaction.execute("UPDATE game_personal SET installed = 0", [])?;
        transaction.execute("DELETE FROM game_installations", [])?;
    }
    let mut imported = 0;
    let mut updated = 0;
    let next_position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(manual_position), -1) + 1 FROM game_personal",
        [],
        |row| row.get(0),
    )?;

    for (index, game) in games.iter().enumerate() {
        if !matches!(
            game.ownership_source.as_str(),
            "owned" | "family_shared" | "local"
        ) || !matches!(
            game.family_availability.as_str(),
            "not_applicable" | "unknown" | "confirmed"
        ) || (game.ownership_source != "family_shared"
            && game.family_availability != "not_applicable")
        {
            return Err(AppError::validation(
                "El origen de un juego importado no es válido.",
            ));
        }
        let playtime_minutes = sqlite_integer(game.playtime_minutes, "tiempo total")?;
        let playtime_recent_minutes =
            sqlite_integer(game.playtime_recent_minutes, "tiempo reciente")?;
        let existed = transaction
            .query_row(
                "SELECT 1 FROM games WHERE app_id = ?1",
                [game.app_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        transaction.execute(
            "INSERT INTO games(
                app_id, title, icon_url, cover_url, header_url, playtime_minutes,
                playtime_recent_minutes, last_played_at, ownership_source, family_availability
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(app_id) DO UPDATE SET
                title = excluded.title,
                icon_url = COALESCE(excluded.icon_url, games.icon_url),
                -- El arte que trae la importación es la URL derivada por
                -- convención: una conjetura a partir del AppID que devuelve 404
                -- para buena parte del catálogo moderno. Cuando la fila ya tiene
                -- una URL —la que `steam::store_assets` resolvió contra el
                -- índice oficial— se conserva, en vez de sustituir un nombre de
                -- archivo comprobado por uno inventado.
                cover_url = COALESCE(games.cover_url, excluded.cover_url),
                header_url = COALESCE(games.header_url, excluded.header_url),
                playtime_minutes = MAX(excluded.playtime_minutes, games.playtime_minutes),
                playtime_recent_minutes = excluded.playtime_recent_minutes,
                last_played_at = COALESCE(excluded.last_played_at, games.last_played_at),
                ownership_source = CASE
                    WHEN games.ownership_source = 'owned' OR excluded.ownership_source = 'owned'
                        THEN 'owned'
                    WHEN games.ownership_source = 'family_shared'
                      OR excluded.ownership_source = 'family_shared'
                        THEN 'family_shared'
                    ELSE 'local'
                END,
                family_availability = CASE
                    WHEN games.ownership_source = 'owned' OR excluded.ownership_source = 'owned'
                        THEN 'not_applicable'
                    WHEN games.ownership_source = 'family_shared'
                      OR excluded.ownership_source = 'family_shared'
                    THEN CASE
                        WHEN games.family_availability = 'confirmed'
                          OR excluded.family_availability = 'confirmed'
                          OR games.ownership_source = 'local'
                          OR excluded.ownership_source = 'local'
                            THEN 'confirmed'
                        ELSE 'unknown'
                    END
                    ELSE 'not_applicable'
                END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                game.app_id,
                game.title,
                game.icon_url,
                game.cover_url,
                game.header_url,
                playtime_minutes,
                playtime_recent_minutes,
                game.last_played_at,
                game.ownership_source,
                game.family_availability,
            ],
        )?;
        transaction.execute(
            "INSERT OR IGNORE INTO game_personal(app_id, status_id, manual_position)
             VALUES (?1, 'unclassified', ?2)",
            params![game.app_id, next_position + index as i64],
        )?;
        if let Some(installation) = &game.installation {
            let size_on_disk = installation
                .size_on_disk
                .map(|value| sqlite_integer(value, "tamaño de instalación"))
                .transpose()?;
            let build_id = installation
                .build_id
                .map(|value| sqlite_integer(value, "identificador de build"))
                .transpose()?;
            transaction.execute(
                "UPDATE game_personal SET installed = 1 WHERE app_id = ?1",
                [game.app_id],
            )?;
            transaction.execute(
                "INSERT INTO game_installations(
                    app_id, library_path, install_path, size_on_disk, build_id, last_updated_at, is_primary
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
                 ON CONFLICT(app_id, library_path) DO UPDATE SET
                    install_path = excluded.install_path,
                    size_on_disk = excluded.size_on_disk,
                    build_id = excluded.build_id,
                    last_updated_at = excluded.last_updated_at",
                params![
                    game.app_id,
                    installation.library_path,
                    installation.install_path,
                    size_on_disk,
                    build_id,
                    installation.last_updated_at,
                ],
            )?;
        }
        if existed {
            updated += 1;
        } else {
            imported += 1;
        }
    }
    Ok((imported, updated))
}

fn sqlite_integer(value: u64, field: &str) -> AppResult<i64> {
    i64::try_from(value).map_err(|_| {
        AppError::validation(format!(
            "Steam devolvió un {field} que excede el límite compatible con SQLite."
        ))
    })
}
