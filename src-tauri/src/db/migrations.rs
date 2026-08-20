use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, params};

/// `VNDX` en ASCII. Permite rechazar cualquier SQLite que no haya sido creado
/// por Vindexa antes de una restauración.
pub const APPLICATION_ID: i64 = 0x564E4458;
pub const CURRENT_VERSION: i64 = 49;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../../migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "indexes_and_search",
        sql: include_str!("../../migrations/002_indexes.sql"),
    },
    Migration {
        version: 3,
        name: "steam_sync_diagnostics",
        sql: include_str!("../../migrations/003_steam_sync_diagnostics.sql"),
    },
    Migration {
        version: 4,
        name: "library_sorting",
        sql: include_str!("../../migrations/004_library_sorting.sql"),
    },
    Migration {
        version: 5,
        name: "store_metadata",
        sql: include_str!("../../migrations/005_store_metadata.sql"),
    },
    Migration {
        version: 6,
        name: "game_hero",
        sql: include_str!("../../migrations/006_game_hero.sql"),
    },
    Migration {
        version: 7,
        name: "steam_metadata_complete",
        sql: include_str!("../../migrations/007_steam_metadata_complete.sql"),
    },
    Migration {
        version: 8,
        name: "planner_advanced",
        sql: include_str!("../../migrations/008_planner_advanced.sql"),
    },
    Migration {
        version: 9,
        name: "tracking_discovery",
        sql: include_str!("../../migrations/009_tracking_discovery.sql"),
    },
    Migration {
        version: 10,
        name: "library_filters",
        sql: include_str!("../../migrations/010_library_filters.sql"),
    },
    Migration {
        version: 11,
        name: "family_catalog",
        sql: include_str!("../../migrations/011_family_catalog.sql"),
    },
    Migration {
        version: 12,
        name: "personal_sessions_tags",
        sql: include_str!("../../migrations/012_personal_sessions_tags.sql"),
    },
    Migration {
        version: 13,
        name: "legacy_ownership_provenance",
        sql: include_str!("../../migrations/013_legacy_ownership_provenance.sql"),
    },
    Migration {
        version: 14,
        name: "metadata_enrichment_queue",
        sql: include_str!("../../migrations/014_metadata_enrichment_queue.sql"),
    },
    Migration {
        version: 15,
        name: "discovery_signals",
        sql: include_str!("../../migrations/015_discovery_signals.sql"),
    },
    Migration {
        version: 16,
        name: "manual_position_index",
        sql: include_str!("../../migrations/016_manual_position_index.sql"),
    },
    Migration {
        version: 17,
        name: "game_capsule_url",
        sql: include_str!("../../migrations/017_game_capsule_url.sql"),
    },
    Migration {
        version: 18,
        name: "drm_free_detection",
        sql: include_str!("../../migrations/018_drm_free_detection.sql"),
    },
    Migration {
        version: 19,
        name: "rich_game_metadata",
        sql: include_str!("../../migrations/019_rich_game_metadata.sql"),
    },
    Migration {
        version: 20,
        name: "game_dlc",
        sql: include_str!("../../migrations/020_game_dlc.sql"),
    },
    Migration {
        version: 21,
        name: "curated_lists",
        sql: include_str!("../../migrations/021_curated_lists.sql"),
    },
    Migration {
        version: 22,
        name: "wishlist_and_videos",
        sql: include_str!("../../migrations/022_wishlist_and_videos.sql"),
    },
    Migration {
        version: 23,
        name: "notifications",
        sql: include_str!("../../migrations/023_notifications.sql"),
    },
    Migration {
        version: 24,
        name: "priority_engine",
        sql: include_str!("../../migrations/024_priority_engine.sql"),
    },
    Migration {
        version: 25,
        name: "external_stores",
        sql: include_str!("../../migrations/025_external_stores.sql"),
    },
    Migration {
        version: 26,
        name: "agent_bridge",
        sql: include_str!("../../migrations/026_agent_bridge.sql"),
    },
    Migration {
        version: 27,
        name: "agent_receipt",
        sql: include_str!("../../migrations/027_agent_receipt.sql"),
    },
    Migration {
        version: 28,
        name: "saved_views",
        sql: include_str!("../../migrations/028_saved_views.sql"),
    },
    Migration {
        version: 29,
        name: "pricing_and_archive",
        sql: include_str!("../../migrations/029_pricing_and_archive.sql"),
    },
    Migration {
        version: 30,
        name: "wishlist_catalog",
        sql: include_str!("../../migrations/030_wishlist_catalog.sql"),
    },
    Migration {
        version: 31,
        name: "game_platforms",
        sql: include_str!("../../migrations/031_game_platforms.sql"),
    },
    Migration {
        version: 32,
        name: "itch_store",
        sql: include_str!("../../migrations/032_itch_store.sql"),
    },
    Migration {
        version: 33,
        name: "catalog_prices",
        sql: include_str!("../../migrations/033_catalog_prices.sql"),
    },
    Migration {
        version: 34,
        name: "sharp_covers",
        sql: include_str!("../../migrations/034_sharp_covers.sql"),
    },
    Migration {
        version: 35,
        name: "family_sharp_covers",
        sql: include_str!("../../migrations/035_family_sharp_covers.sql"),
    },
    Migration {
        version: 36,
        name: "family_games_are_organizable",
        sql: include_str!("../../migrations/036_family_games_are_organizable.sql"),
    },
    Migration {
        version: 37,
        name: "external_games_join_the_library",
        sql: include_str!("../../migrations/037_external_games_join_the_library.sql"),
    },
    Migration {
        version: 38,
        name: "bought_elsewhere_is_owned",
        sql: include_str!("../../migrations/038_bought_elsewhere_is_owned.sql"),
    },
    Migration {
        version: 39,
        name: "store_drm_reaches_the_library",
        sql: include_str!("../../migrations/039_store_drm_reaches_the_library.sql"),
    },
    Migration {
        version: 40,
        name: "upcoming_checks",
        sql: include_str!("../../migrations/040_upcoming_checks.sql"),
    },
    Migration {
        version: 41,
        name: "art_cache_fits_the_library",
        sql: include_str!("../../migrations/041_art_cache_fits_the_library.sql"),
    },
    Migration {
        version: 42,
        name: "agent_tasks",
        sql: include_str!("../../migrations/042_agent_tasks.sql"),
    },
    Migration {
        version: 43,
        name: "epic_free_games",
        sql: include_str!("../../migrations/043_epic_free_games.sql"),
    },
    Migration {
        version: 44,
        name: "preview_screenshots",
        sql: include_str!("../../migrations/044_preview_screenshots.sql"),
    },
    Migration {
        version: 45,
        name: "store_deals",
        sql: include_str!("../../migrations/045_store_deals.sql"),
    },
    Migration {
        version: 46,
        name: "deals_by_store",
        sql: include_str!("../../migrations/046_deals_by_store.sql"),
    },
    Migration {
        version: 47,
        name: "price_checks",
        sql: include_str!("../../migrations/047_price_checks.sql"),
    },
    Migration {
        version: 48,
        name: "drm_evidence_in_words",
        sql: include_str!("../../migrations/048_drm_evidence_in_words.sql"),
    },
    Migration {
        version: 49,
        name: "library_hero_for_everyone",
        sql: include_str!("../../migrations/049_library_hero_for_everyone.sql"),
    },
];

pub fn migrate(connection: &mut Connection) -> AppResult<()> {
    validate_or_adopt_identity(connection)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );",
    )?;

    for migration in MIGRATIONS {
        let already_applied = connection
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |_| Ok(()),
            )
            .optional()?
            .is_some();

        if already_applied {
            continue;
        }

        let transaction = connection.transaction()?;
        transaction.execute_batch(migration.sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        )?;
        transaction.pragma_update(None, "user_version", migration.version)?;
        transaction.commit()?;
    }

    connection.pragma_update(None, "application_id", APPLICATION_ID)?;

    Ok(())
}

pub fn validate_history(connection: &Connection) -> AppResult<()> {
    let application_id: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id != APPLICATION_ID {
        return Err(AppError::new(
            "backup_identity",
            "El archivo no contiene la identidad de una copia de Vindexa.",
        ));
    }
    let user_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if user_version != CURRENT_VERSION {
        return Err(AppError::new(
            "backup_version",
            format!(
                "La copia usa la versión de esquema {user_version}; Vindexa necesita la versión {CURRENT_VERSION}."
            ),
        ));
    }

    let mut statement =
        connection.prepare("SELECT version, name FROM schema_migrations ORDER BY version ASC")?;
    let applied = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let expected = MIGRATIONS
        .iter()
        .map(|migration| (migration.version, migration.name.to_string()))
        .collect::<Vec<_>>();
    if applied != expected {
        return Err(AppError::new(
            "backup_migrations",
            "El historial de migraciones de la copia no coincide con esta versión de Vindexa.",
        ));
    }
    Ok(())
}

fn validate_or_adopt_identity(connection: &Connection) -> AppResult<()> {
    let application_id: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    if application_id == APPLICATION_ID {
        return Ok(());
    }
    if application_id != 0 {
        return Err(AppError::new(
            "database_identity",
            "La ruta de datos contiene una base perteneciente a otra aplicación.",
        ));
    }

    let user_table_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master
          WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    if user_table_count == 0 {
        return Ok(());
    }
    let has_migration_table = connection
        .query_row(
            "SELECT 1 FROM sqlite_master
              WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !has_migration_table {
        return Err(AppError::new(
            "database_identity",
            "La ruta de datos contiene un SQLite sin identidad ni historial de Vindexa.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{APPLICATION_ID, CURRENT_VERSION, MIGRATIONS, migrate, validate_history};
    use rusqlite::Connection;

    #[test]
    fn migrations_are_idempotent_and_enable_fts() {
        let mut connection = Connection::open_in_memory().expect("open database");
        migrate(&mut connection).expect("first migration pass");
        migrate(&mut connection).expect("second migration pass");

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("schema version");
        assert_eq!(version, CURRENT_VERSION);
        let application_id: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .expect("application id");
        assert_eq!(application_id, APPLICATION_ID);
        validate_history(&connection).expect("validate migration identity");

        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Juego de prueba')",
                [],
            )
            .expect("insert game");
        let matches: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM game_search WHERE game_search MATCH 'Juego'",
                [],
                |row| row.get(0),
            )
            .expect("search game");
        assert_eq!(matches, 1);
    }

    #[test]
    fn applied_metadata_migration_remains_byte_stable_at_its_original_default() {
        let metadata = include_str!("../../migrations/007_steam_metadata_complete.sql");
        assert!(metadata.contains("ownership_source TEXT NOT NULL DEFAULT 'owned'"));
        assert!(!metadata.contains("ownership_source TEXT NOT NULL DEFAULT 'local'"));
    }

    #[test]
    fn refuses_to_adopt_an_unrelated_populated_database() {
        let mut connection = Connection::open_in_memory().expect("open database");
        connection
            .execute("CREATE TABLE foreign_product(id INTEGER PRIMARY KEY)", [])
            .expect("create unrelated schema");

        let error = migrate(&mut connection).expect_err("reject unrelated database");
        assert_eq!(error.code, "database_identity");
        let application_id: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .expect("application id");
        assert_eq!(application_id, 0);
    }

    #[test]
    fn upgrades_v2_sync_state_without_losing_the_linked_account() {
        let mut connection = Connection::open_in_memory().expect("open database");
        for migration in MIGRATIONS.iter().take(2) {
            connection
                .execute_batch(migration.sql)
                .expect("apply historical migration");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name) VALUES (?1, ?2)",
                    rusqlite::params![migration.version, migration.name],
                )
                .expect("record historical migration");
            connection
                .pragma_update(None, "user_version", migration.version)
                .expect("record historical schema version");
        }
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .expect("record application identity");
        connection
            .execute(
                "INSERT INTO steam_accounts(steam_id, last_sync_status)
                 VALUES ('76561198000000000', 'failed')",
                [],
            )
            .expect("store linked account");

        migrate(&mut connection).expect("upgrade v2 database");

        let account = connection
            .query_row(
                "SELECT steam_id, last_sync_status, last_sync_error_code,
                        last_sync_error_message
                 FROM steam_accounts",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .expect("read migrated account");
        assert_eq!(account.0, "76561198000000000");
        assert_eq!(account.1, "failed");
        assert!(account.2.is_none());
        assert!(account.3.is_none());
        validate_history(&connection).expect("validate upgraded history");
    }

    #[test]
    fn upgrades_v12_history_through_the_provenance_repair() {
        let mut connection = Connection::open_in_memory().expect("open database");
        for migration in MIGRATIONS.iter().take(12) {
            connection
                .execute_batch(migration.sql)
                .expect("apply historical migration");
            connection
                .execute(
                    "INSERT INTO schema_migrations(version, name, applied_at)
                     VALUES (?1, ?2, '2026-02-01T00:00:00.000Z')",
                    rusqlite::params![migration.version, migration.name],
                )
                .expect("record historical migration");
            connection
                .pragma_update(None, "user_version", migration.version)
                .expect("record historical schema version");
        }
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .expect("record application identity");
        connection
            .execute(
                "INSERT INTO games(
                    app_id, title, imported_at, ownership_source, family_availability
                 ) VALUES (
                    10, 'Manifiesto anterior', '2026-01-01T00:00:00.000Z',
                    'owned', 'not_applicable'
                 )",
                [],
            )
            .expect("store legacy row");

        migrate(&mut connection).expect("upgrade v12 database");

        let source: String = connection
            .query_row(
                "SELECT ownership_source FROM games WHERE app_id = 10",
                [],
                |row| row.get(0),
            )
            .expect("read repaired source");
        assert_eq!(source, "local");
        validate_history(&connection).expect("validate upgraded history");
    }
}
