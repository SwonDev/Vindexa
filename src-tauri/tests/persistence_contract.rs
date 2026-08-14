use rusqlite::backup::Backup;
use rusqlite::{Connection, ErrorCode, params};

const INITIAL_SCHEMA: &str = include_str!("../migrations/001_initial.sql");
const INDEXES_AND_SEARCH: &str = include_str!("../migrations/002_indexes.sql");

fn migrated_database() -> Connection {
    let connection = Connection::open_in_memory().expect("abrir base de datos temporal");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("activar claves foráneas");
    connection
        .execute_batch(INITIAL_SCHEMA)
        .expect("aplicar esquema inicial");
    connection
        .execute_batch(INDEXES_AND_SEARCH)
        .expect("aplicar índices y búsqueda");
    connection
}

fn seed_status_and_game(connection: &Connection, app_id: i64, title: &str) {
    connection
        .execute(
            "INSERT OR IGNORE INTO statuses(id, name, color, position, built_in)
             VALUES ('backlog', 'Pendiente', '#5CAAC1', 0, 1)",
            [],
        )
        .expect("insertar estado base");
    connection
        .execute(
            "INSERT INTO games(app_id, title, playtime_minutes) VALUES (?1, ?2, 120)",
            params![app_id, title],
        )
        .expect("insertar juego");
    connection
        .execute(
            "INSERT INTO game_personal(app_id, status_id, notes, checkpoint, next_action)
             VALUES (?1, 'backlog', 'Una nota personal', 'Capítulo 2', 'Explorar el bosque')",
            [app_id],
        )
        .expect("insertar datos personales");
}

#[test]
fn schema_rejects_invalid_progress_and_orphan_personal_data() {
    let connection = migrated_database();
    seed_status_and_game(&connection, 10, "Portal");

    let invalid_progress = connection
        .execute(
            "UPDATE game_personal SET progress = 101 WHERE app_id = 10",
            [],
        )
        .expect_err("el progreso fuera de rango debe rechazarse");
    assert_eq!(
        invalid_progress.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation)
    );

    let orphan = connection
        .execute(
            "INSERT INTO game_personal(app_id, status_id) VALUES (99999, 'backlog')",
            [],
        )
        .expect_err("los datos personales huérfanos deben rechazarse");
    assert_eq!(
        orphan.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation)
    );
}

#[test]
fn deleting_remote_game_cascades_owned_relations_without_leaving_personal_data() {
    let connection = migrated_database();
    seed_status_and_game(&connection, 20, "Half-Life 2");
    connection
        .execute(
            "INSERT INTO collections(id, name, color, icon, kind)
             VALUES ('favorites', 'Favoritos', '#A4D007', 'star', 'manual')",
            [],
        )
        .expect("insertar colección");
    connection
        .execute(
            "INSERT INTO collection_games(collection_id, app_id) VALUES ('favorites', 20)",
            [],
        )
        .expect("relacionar colección");
    connection
        .execute(
            "INSERT INTO game_installations(app_id, library_path, install_path)
             VALUES (20, '/SteamLibrary', '/SteamLibrary/Half-Life 2')",
            [],
        )
        .expect("insertar instalación");

    connection
        .execute("DELETE FROM games WHERE app_id = 20", [])
        .expect("eliminar juego");

    for table in ["game_personal", "collection_games", "game_installations"] {
        let sql = format!("SELECT COUNT(*) FROM {table}");
        let remaining: i64 = connection
            .query_row(&sql, [], |row| row.get(0))
            .expect("contar relaciones");
        assert_eq!(remaining, 0, "{table} debe respetar ON DELETE CASCADE");
    }
}

#[test]
fn full_text_search_tracks_title_and_personal_notes_updates() {
    let connection = migrated_database();
    seed_status_and_game(&connection, 30, "Celeste");

    connection
        .execute(
            "UPDATE game_personal
             SET notes = 'Retomar la montaña nevada', next_action = 'Buscar la fresa'
             WHERE app_id = 30",
            [],
        )
        .expect("actualizar índice personal");

    let by_title: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM game_search WHERE game_search MATCH 'Celeste'",
            [],
            |row| row.get(0),
        )
        .expect("buscar título");
    let by_note: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM game_search WHERE game_search MATCH 'montaña'",
            [],
            |row| row.get(0),
        )
        .expect("buscar nota");

    assert_eq!(by_title, 1);
    assert_eq!(by_note, 1);

    connection
        .execute(
            "UPDATE games SET title = 'Celeste Farewell' WHERE app_id = 30",
            [],
        )
        .expect("actualizar título");
    let renamed: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM game_search WHERE game_search MATCH 'Farewell'",
            [],
            |row| row.get(0),
        )
        .expect("buscar título actualizado");
    assert_eq!(renamed, 1);
}

#[test]
fn backup_roundtrip_preserves_remote_and_personal_fields() {
    let source = migrated_database();
    seed_status_and_game(&source, 40, "Disco Elysium");
    source
        .execute(
            "UPDATE game_personal
             SET progress = 47, rating = 10, pinned = 1, tracking = 1,
                 notes = 'No sobrescribir durante una resincronización'
             WHERE app_id = 40",
            [],
        )
        .expect("personalizar juego");

    let mut restored = Connection::open_in_memory().expect("abrir destino");
    {
        let backup = Backup::new(&source, &mut restored).expect("iniciar copia");
        backup.step(-1).expect("completar copia");
    }

    let restored_record: (String, i64, i64, i64, String) = restored
        .query_row(
            "SELECT g.title, p.progress, p.rating, p.tracking, p.notes
             FROM games g JOIN game_personal p USING(app_id) WHERE g.app_id = 40",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .expect("leer copia restaurada");

    assert_eq!(
        restored_record,
        (
            "Disco Elysium".to_string(),
            47,
            10,
            1,
            "No sobrescribir durante una resincronización".to_string(),
        )
    );
    let integrity: String = restored
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("comprobar integridad");
    assert_eq!(integrity, "ok");
}
