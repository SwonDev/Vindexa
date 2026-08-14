#[allow(dead_code)]
#[path = "../src/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../src/db/library.rs"]
mod library;
#[allow(dead_code)]
#[path = "../src/models.rs"]
mod models;

use library::ImportedGame;
use rusqlite::{Connection, params};

const MIGRATIONS_BEFORE_OWNERSHIP: &[(&str, &str)] = &[
    ("initial", include_str!("../migrations/001_initial.sql")),
    (
        "indexes_and_search",
        include_str!("../migrations/002_indexes.sql"),
    ),
    (
        "steam_sync_diagnostics",
        include_str!("../migrations/003_steam_sync_diagnostics.sql"),
    ),
    (
        "library_sorting",
        include_str!("../migrations/004_library_sorting.sql"),
    ),
    (
        "store_metadata",
        include_str!("../migrations/005_store_metadata.sql"),
    ),
    ("game_hero", include_str!("../migrations/006_game_hero.sql")),
];
const OWNERSHIP_V7: &str = include_str!("../migrations/007_steam_metadata_complete.sql");
const REPAIR_V13: &str = include_str!("../migrations/013_legacy_ownership_provenance.sql");

fn ownership_source(connection: &Connection, app_id: u32) -> String {
    connection
        .query_row(
            "SELECT ownership_source FROM games WHERE app_id = ?1",
            [app_id],
            |row| row.get(0),
        )
        .expect("leer procedencia")
}

#[test]
fn upgrades_legacy_local_rows_without_overriding_web_evidence_and_resync_promotes_owned() {
    let mut connection = Connection::open_in_memory().expect("abrir SQLite temporal");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("activar claves foráneas");
    for (index, (name, sql)) in MIGRATIONS_BEFORE_OWNERSHIP.iter().enumerate() {
        connection.execute_batch(sql).expect("aplicar migración");
        connection
            .execute(
                "INSERT INTO schema_migrations(version, name, applied_at)
                 VALUES (?1, ?2, '2025-01-01T00:00:00.000Z')",
                params![index as i64 + 1, name],
            )
            .expect("registrar migración");
    }
    connection
        .execute(
            "INSERT INTO statuses(id, name, color, position, built_in)
             VALUES ('unclassified', 'Sin clasificar', '#6F7B8A', 0, 1)",
            [],
        )
        .expect("crear estado");
    connection
        .execute_batch(
            "INSERT INTO games(app_id, title, imported_at)
             VALUES (10, 'Sólo manifiesto', '2026-01-01T00:00:00.000Z');
             INSERT INTO games(app_id, title, playtime_minutes, imported_at)
             VALUES (20, 'Con tiempo Web', 12, '2026-01-01T00:00:00.000Z');
             INSERT INTO games(app_id, title, icon_url, imported_at)
             VALUES (30, 'Con icono Web', 'https://media.steampowered.com/icon.jpg',
                     '2026-01-01T00:00:00.000Z');",
        )
        .expect("crear cohorte anterior a 007");
    connection
        .execute_batch(OWNERSHIP_V7)
        .expect("aplicar migración 007 inmutable");
    connection
        .execute(
            "INSERT INTO schema_migrations(version, name, applied_at)
             VALUES (7, 'steam_metadata_complete', '2026-02-01T00:00:00.000Z')",
            [],
        )
        .expect("registrar migración 007");
    connection
        .execute(
            "INSERT INTO games(
                app_id, title, imported_at, ownership_source, family_availability
             ) VALUES (
                40, 'Web posterior sin actividad', '2026-03-01T00:00:00.000Z',
                'owned', 'not_applicable'
             )",
            [],
        )
        .expect("crear juego Web posterior a 007");

    connection
        .execute_batch(REPAIR_V13)
        .expect("reparar procedencia legacy");

    assert_eq!(ownership_source(&connection, 10), "local");
    assert_eq!(ownership_source(&connection, 20), "owned");
    assert_eq!(ownership_source(&connection, 30), "owned");
    assert_eq!(ownership_source(&connection, 40), "owned");

    library::upsert_imported_games(
        &mut connection,
        &[ImportedGame {
            app_id: 10,
            title: "Sólo manifiesto".into(),
            icon_url: None,
            cover_url: None,
            header_url: None,
            playtime_minutes: 0,
            playtime_recent_minutes: 0,
            last_played_at: None,
            ownership_source: "owned".into(),
            family_availability: "not_applicable".into(),
            installation: None,
        }],
        false,
    )
    .expect("resincronizar mediante GetOwnedGames");
    assert_eq!(ownership_source(&connection, 10), "owned");
}
