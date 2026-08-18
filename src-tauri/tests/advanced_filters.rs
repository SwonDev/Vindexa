#[allow(dead_code)]
#[path = "../src/error.rs"]
mod error;
#[allow(dead_code)]
#[path = "../src/db/library.rs"]
mod library;
#[allow(dead_code)]
#[path = "../src/models.rs"]
mod models;

#[path = "support/schema.rs"]
mod schema;
use models::GameListRequest;
use rusqlite::{Connection, params};

fn database() -> Connection {
    let connection = schema::base_en_memoria();
    connection
        .execute(
            "INSERT INTO statuses(id, name, color, position, built_in)
             VALUES ('unclassified', 'Sin clasificar', '#6F7B8A', 0, 1)",
            [],
        )
        .expect("insertar estado");
    connection
        .execute(
            "INSERT INTO collections(id, name, color, icon, kind)
             VALUES ('coop', 'Cooperativos', '#5CAAC1', 'users', 'manual')",
            [],
        )
        .expect("insertar colección");
    connection
        .execute(
            "INSERT INTO tags(id, name, color) VALUES ('relajante', 'Relajante', '#7EA64B')",
            [],
        )
        .expect("insertar etiqueta");

    for (app_id, title, playtime, last_played) in [
        (1, "Alpha", 0, None),
        (2, "Beta", 600, Some("2026-08-10T10:00:00Z")),
        (3, "Gamma", 120, Some("2025-01-10T10:00:00Z")),
    ] {
        connection
            .execute(
                "INSERT INTO games(app_id, title, playtime_minutes, last_played_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![app_id, title, playtime, last_played],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar organización personal");
    }

    connection
        .execute_batch(
            "UPDATE games SET
                metadata_status = 'success',
                release_date = '2025-02-01',
                is_early_access = 1,
                genres_json = '[\"Acción\",\"Aventura\"]',
                categories_json = '[\"Cooperativo en línea\"]',
                steam_deck_status = 'verified',
                achievements_unlocked = 5,
                achievements_total = 10,
                achievements_status = 'success'
              WHERE app_id = 1;
             UPDATE games SET
                metadata_status = 'success',
                release_date = '2020-01-01',
                genres_json = '[\"Estrategia\"]',
                categories_json = '[\"Un jugador\"]',
                achievements_unlocked = 10,
                achievements_total = 10,
                achievements_status = 'success'
              WHERE app_id = 2;
             UPDATE game_personal SET
                installed = 0, tracking = 0, progress = 20, rating = 8,
                target_date = '2026-09-15'
              WHERE app_id = 1;
             UPDATE game_personal SET
                installed = 1, tracking = 1, progress = 80, rating = 10
              WHERE app_id = 2;
             INSERT INTO collection_games(collection_id, app_id, position) VALUES ('coop', 1, 0);
             INSERT INTO game_tags(app_id, tag_id) VALUES (1, 'relajante');
             INSERT INTO game_sessions(id, app_id, started_at, ended_at, note)
             VALUES ('session-alpha', 1, '2026-08-01T10:00:00Z', '2026-08-01T11:00:00Z', '');
             INSERT INTO game_sessions(id, app_id, started_at, ended_at, note)
             VALUES ('session-beta', 2, '2026-08-01T10:00:00Z', '2026-08-01T12:00:00Z', '');",
        )
        .expect("preparar hechos filtrables");
    connection
}

fn ids(connection: &Connection, request: GameListRequest) -> Vec<u32> {
    library::list_games(connection, &request, None)
        .expect("filtrar biblioteca")
        .items
        .into_iter()
        .map(|game| game.app_id)
        .collect()
}

#[test]
fn combines_every_available_filter_before_pagination() {
    let connection = database();
    let request = GameListRequest {
        status_id: Some("unclassified".into()),
        collection_id: Some("coop".into()),
        installed: Some(false),
        tracking: Some(false),
        early_access: Some(true),
        never_played: Some(true),
        min_playtime_minutes: Some(0),
        max_playtime_minutes: Some(0),
        min_progress: Some(10),
        max_progress: Some(30),
        min_rating: Some(7),
        max_rating: Some(9),
        genre: Some("Acción".into()),
        category: Some("Cooperativo en línea".into()),
        tag_id: Some("relajante".into()),
        release_from: Some("2024-01-01".into()),
        release_to: Some("2026-01-01".into()),
        min_achievement_percent: Some(40),
        max_achievement_percent: Some(60),
        steam_deck_status: Some("verified".into()),
        target_date_from: Some("2026-09-01".into()),
        target_date_to: Some("2026-09-30".into()),
        min_session_minutes: Some(45),
        max_session_minutes: Some(75),
        limit: Some(1),
        ..GameListRequest::default()
    };

    assert_eq!(ids(&connection, request), [1]);
}

#[test]
fn false_filters_and_null_semantics_are_explicit() {
    let connection = database();
    assert_eq!(
        ids(
            &connection,
            GameListRequest {
                never_played: Some(false),
                installed: Some(false),
                ..GameListRequest::default()
            }
        ),
        [3]
    );
    assert_eq!(
        ids(
            &connection,
            GameListRequest {
                min_rating: Some(1),
                ..GameListRequest::default()
            }
        ),
        [1, 2]
    );
}

#[test]
fn last_played_window_includes_the_whole_end_day_and_excludes_nulls() {
    let connection = database();
    assert_eq!(
        ids(
            &connection,
            GameListRequest {
                last_played_from: Some("2026-08-10".into()),
                last_played_to: Some("2026-08-10".into()),
                ..GameListRequest::default()
            }
        ),
        [2]
    );
}

#[test]
fn exposes_only_real_filter_options_and_coverage() {
    let connection = database();
    let options = library::filter_options(&connection).expect("cargar opciones");

    assert_eq!(options.total_games, 3);
    assert_eq!(options.metadata_games, 2);
    assert_eq!(options.achievement_games, 2);
    assert_eq!(options.steam_deck_games, 1);
    assert_eq!(options.genres, ["Acción", "Aventura", "Estrategia"]);
    assert_eq!(options.categories, ["Cooperativo en línea", "Un jugador"]);
    assert_eq!(options.tags[0].id, "relajante");
}

#[test]
fn rejects_inverted_ranges_and_invalid_dates() {
    let connection = database();
    for request in [
        GameListRequest {
            min_progress: Some(80),
            max_progress: Some(20),
            ..GameListRequest::default()
        },
        GameListRequest {
            release_from: Some("01/01/2024".into()),
            ..GameListRequest::default()
        },
    ] {
        let error =
            library::list_games(&connection, &request, None).expect_err("rechazar filtro ambiguo");
        assert_eq!(error.code, "validation");
    }
}
