#[allow(dead_code)]
#[path = "../src/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../src/db/library.rs"]
mod library;
#[allow(dead_code)]
#[path = "../src/models.rs"]
mod models;

use library::{ImportedGame, ImportedInstallation};
use models::GameListRequest;
use rusqlite::{Connection, params};
use std::collections::HashSet;

const INITIAL_SCHEMA: &str = include_str!("../migrations/001_initial.sql");
const INDEXES_AND_SEARCH: &str = include_str!("../migrations/002_indexes.sql");
const SORT_INDEXES: &str = include_str!("../migrations/004_library_sorting.sql");
const COMPLETE_METADATA: &str = include_str!("../migrations/007_steam_metadata_complete.sql");
const MANUAL_POSITION_INDEX: &str = include_str!("../migrations/016_manual_position_index.sql");

fn database() -> Connection {
    let mut connection = Connection::open_in_memory().expect("abrir SQLite temporal");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("activar claves foráneas");
    connection
        .execute_batch(INITIAL_SCHEMA)
        .expect("aplicar esquema");
    connection
        .execute_batch(INDEXES_AND_SEARCH)
        .expect("aplicar índices base");
    connection
        .execute_batch(SORT_INDEXES)
        .expect("aplicar índices de ordenación");
    connection
        .execute_batch(COMPLETE_METADATA)
        .expect("aplicar origen y metadata completa");
    connection
        .execute_batch(MANUAL_POSITION_INDEX)
        .expect("alinear índice con el orden manual");
    connection
        .execute(
            "INSERT INTO statuses(id, name, color, position, built_in)
             VALUES ('unclassified', 'Sin clasificar', '#6F7B8A', 0, 1)",
            [],
        )
        .expect("crear estado");

    let titles = ["Alpha", "Beta", "Gamma", "Delta", "Alpha", "Omega"];
    let games = titles
        .into_iter()
        .enumerate()
        .map(|(index, title)| {
            let app_id = u32::try_from(index + 1).expect("app id");
            let installation = match app_id {
                1 => Some(ImportedInstallation {
                    library_path: "/Steam/A".into(),
                    install_path: "/Steam/A/Alpha".into(),
                    size_on_disk: Some(500),
                    build_id: Some(1),
                    last_updated_at: None,
                }),
                3 => Some(ImportedInstallation {
                    library_path: "/Steam/B".into(),
                    install_path: "/Steam/B/Gamma".into(),
                    size_on_disk: Some(100),
                    build_id: Some(1),
                    last_updated_at: None,
                }),
                5 => Some(ImportedInstallation {
                    library_path: "/Steam/C".into(),
                    install_path: "/Steam/C/Alpha".into(),
                    size_on_disk: None,
                    build_id: Some(1),
                    last_updated_at: None,
                }),
                _ => None,
            };
            ImportedGame {
                app_id,
                title: title.into(),
                icon_url: None,
                cover_url: None,
                header_url: None,
                playtime_minutes: [100, 20, 100, 0, 50, 0][index],
                playtime_recent_minutes: 0,
                last_played_at: [
                    Some("2026-07-01T10:00:00Z".into()),
                    None,
                    Some("2026-08-01T10:00:00Z".into()),
                    Some("2026-06-01T10:00:00Z".into()),
                    None,
                    None,
                ][index]
                    .clone(),
                ownership_source: "owned".into(),
                family_availability: "not_applicable".into(),
                installation,
            }
        })
        .collect::<Vec<_>>();
    library::upsert_imported_games(&mut connection, &games, true).expect("importar juegos");

    for (app_id, release_date, imported_at) in [
        (1, Some("2020-01-01"), "2026-01-01T00:00:00Z"),
        (2, None, "2026-08-06T00:00:00Z"),
        (3, Some("2024-01-01"), "2026-08-05T00:00:00Z"),
        (4, Some("2010-01-01"), "2026-08-04T00:00:00Z"),
        (5, None, "2026-08-03T00:00:00Z"),
        (6, None, "2026-08-02T00:00:00Z"),
    ] {
        connection
            .execute(
                "UPDATE games SET release_date = ?2, imported_at = ?3 WHERE app_id = ?1",
                params![app_id, release_date, imported_at],
            )
            .expect("preparar fechas");
    }
    for (app_id, pinned, priority, position) in [
        (1, 0, 1, 4),
        (2, 0, 5, 2),
        (3, 0, 5, 1),
        (4, 0, 0, 3),
        (5, 1, 0, 5),
        (6, 0, 0, 0),
    ] {
        connection
            .execute(
                "UPDATE game_personal
                    SET pinned = ?2, priority = ?3, manual_position = ?4
                  WHERE app_id = ?1",
                params![app_id, pinned, priority, position],
            )
            .expect("preparar orden personal");
    }
    connection
}

fn sorted_ids(connection: &Connection, sort: &str) -> Vec<u32> {
    library::list_games(
        connection,
        &GameListRequest {
            sort: Some(sort.into()),
            limit: Some(500),
            ..GameListRequest::default()
        },
        None,
    )
    .expect("ordenar juegos")
    .items
    .into_iter()
    .map(|game| game.app_id)
    .collect()
}

#[test]
fn steam_like_sorts_are_deterministic_and_keep_nulls_last() {
    let connection = database();

    assert_eq!(sorted_ids(&connection, "alphabetical"), [1, 5, 2, 4, 3, 6]);
    assert_eq!(
        sorted_ids(&connection, "alphabeticalDesc"),
        [6, 3, 4, 2, 1, 5]
    );
    assert_eq!(sorted_ids(&connection, "lastPlayed"), [3, 1, 4, 5, 2, 6]);
    assert_eq!(sorted_ids(&connection, "recentlyAdded"), [2, 3, 4, 5, 6, 1]);
    assert_eq!(sorted_ids(&connection, "releaseDate"), [3, 1, 4, 5, 2, 6]);
    assert_eq!(
        sorted_ids(&connection, "releaseDateAsc"),
        [4, 1, 3, 5, 2, 6]
    );
    assert_eq!(sorted_ids(&connection, "playtime"), [1, 3, 5, 2, 4, 6]);
    assert_eq!(sorted_ids(&connection, "playtimeAsc"), [4, 6, 2, 5, 1, 3]);
    assert_eq!(
        sorted_ids(&connection, "installedFirst"),
        [1, 5, 3, 2, 4, 6]
    );
    assert_eq!(
        sorted_ids(&connection, "uninstalledFirst"),
        [2, 4, 6, 1, 5, 3]
    );
    assert_eq!(sorted_ids(&connection, "sizeDesc"), [1, 3, 5, 2, 4, 6]);
    assert_eq!(sorted_ids(&connection, "sizeAsc"), [3, 1, 5, 2, 4, 6]);
    assert_eq!(sorted_ids(&connection, "manual"), [6, 3, 2, 4, 1, 5]);
}

#[test]
fn manual_sort_index_starts_with_the_persisted_drag_position() {
    let connection = database();
    let columns = connection
        .prepare("PRAGMA index_info('idx_personal_manual_sort')")
        .expect("consultar índice")
        .query_map([], |row| row.get::<_, String>(2))
        .expect("leer columnas")
        .collect::<Result<Vec<_>, _>>()
        .expect("materializar columnas");

    assert_eq!(columns, ["manual_position", "pinned", "priority", "app_id"]);
}

#[test]
fn pagination_and_smart_collection_filter_are_applied_inside_sql() {
    let connection = database();
    let allowed = HashSet::from([2, 3, 5]);
    let page = library::list_games(
        &connection,
        &GameListRequest {
            sort: Some("alphabetical".into()),
            limit: Some(2),
            offset: Some(0),
            ..GameListRequest::default()
        },
        Some(&allowed),
    )
    .expect("paginar colección");
    assert_eq!(page.total, 3);
    assert_eq!(
        page.items
            .into_iter()
            .map(|game| game.app_id)
            .collect::<Vec<_>>(),
        [5, 2]
    );

    let next = library::list_games(
        &connection,
        &GameListRequest {
            sort: Some("alphabetical".into()),
            limit: Some(2),
            offset: Some(2),
            ..GameListRequest::default()
        },
        Some(&allowed),
    )
    .expect("paginar segunda página");
    assert_eq!(next.total, 3);
    assert_eq!(next.items[0].app_id, 3);
}

#[test]
fn rejects_unknown_sort_instead_of_silently_changing_the_order() {
    let connection = database();
    let error = library::list_games(
        &connection,
        &GameListRequest {
            sort: Some("orden-inventado".into()),
            ..GameListRequest::default()
        },
        None,
    )
    .expect_err("rechazar orden desconocido");
    assert_eq!(error.code, "validation");
}

#[test]
fn random_order_uses_a_stable_seed_across_pages() {
    let connection = database();
    let page = |offset| {
        library::list_games(
            &connection,
            &GameListRequest {
                sort: Some("random".into()),
                sort_seed: Some(42),
                limit: Some(3),
                offset: Some(offset),
                ..GameListRequest::default()
            },
            None,
        )
        .expect("orden aleatorio estable")
        .items
        .into_iter()
        .map(|game| game.app_id)
        .collect::<Vec<_>>()
    };

    let first = page(0);
    let repeated = page(0);
    let second = page(3);
    assert_eq!(first, repeated);
    assert!(first.iter().all(|app_id| !second.contains(app_id)));
    assert_eq!(first.len() + second.len(), 6);
}
