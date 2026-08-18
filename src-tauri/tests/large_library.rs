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
use rusqlite::Connection;
use std::time::{Duration, Instant};

const LARGE_LIBRARY_SIZE: usize = 5_000;

#[path = "support/schema.rs"]
mod schema;
fn large_library_database() -> (Connection, Duration) {
    let mut connection = schema::base_en_memoria();
    connection
        .execute(
            "INSERT INTO statuses(id, name, color, position, built_in)
             VALUES ('unclassified', 'Sin clasificar', '#6F7B8A', 0, 1)",
            [],
        )
        .expect("insertar estado requerido");

    let fixtures = (1..=LARGE_LIBRARY_SIZE)
        .map(|index| ImportedGame {
            app_id: index as u32,
            title: format!("Juego {index:04}"),
            icon_url: None,
            cover_url: Some(format!(
                "https://cdn.cloudflare.steamstatic.com/steam/apps/{index}/library_600x900.jpg"
            )),
            header_url: None,
            playtime_minutes: (index as u64) * 7,
            playtime_recent_minutes: (index % 180) as u64,
            last_played_at: (index % 3 == 0).then(|| "2026-08-14T12:00:00Z".to_string()),
            ownership_source: "owned".into(),
            family_availability: "not_applicable".into(),
            installation: (index % 4 == 0).then(|| ImportedInstallation {
                library_path: "/fixture/SteamLibrary".to_string(),
                install_path: format!("/fixture/SteamLibrary/steamapps/common/Juego {index:04}"),
                size_on_disk: Some((index as u64) * 1_048_576),
                build_id: Some(10_000 + index as u64),
                last_updated_at: Some("2026-08-14T12:00:00Z".to_string()),
            }),
        })
        .collect::<Vec<_>>();

    let started = Instant::now();
    let (imported, updated) =
        library::upsert_imported_games(&mut connection, &fixtures, true).expect("importar 5000");
    let import_elapsed = started.elapsed();
    assert_eq!((imported, updated), (LARGE_LIBRARY_SIZE, 0));
    connection
        .execute(
            "UPDATE games
             SET metadata_status = 'success',
                 genres_json = '[\"Acción\"]',
                 categories_json = '[\"Cooperativo\"]'
             WHERE app_id % 10 = 0",
            [],
        )
        .expect("preparar metadatos para el gate de filtros JSON");
    (connection, import_elapsed)
}

#[test]
fn five_thousand_games_remain_exact_under_filters_and_pagination() {
    let (connection, import_elapsed) = large_library_database();
    assert!(
        import_elapsed < Duration::from_secs(10),
        "la importación de {LARGE_LIBRARY_SIZE} juegos tardó {import_elapsed:?}, por encima del presupuesto de 10 s"
    );

    let query_started = Instant::now();
    let first_page = library::list_games(
        &connection,
        &GameListRequest {
            sort: Some("alphabetical".to_string()),
            limit: Some(120),
            offset: Some(0),
            ..GameListRequest::default()
        },
        None,
    )
    .expect("listar primera página");
    let second_page = library::list_games(
        &connection,
        &GameListRequest {
            sort: Some("alphabetical".to_string()),
            limit: Some(120),
            offset: Some(120),
            ..GameListRequest::default()
        },
        None,
    )
    .expect("listar segunda página");
    let installed = library::list_games(
        &connection,
        &GameListRequest {
            installed: Some(true),
            sort: Some("playtime".to_string()),
            limit: Some(500),
            offset: Some(0),
            ..GameListRequest::default()
        },
        None,
    )
    .expect("filtrar instalados");
    let filtered = library::list_games(
        &connection,
        &GameListRequest {
            query: Some("Juego 4321".to_string()),
            limit: Some(20),
            ..GameListRequest::default()
        },
        None,
    )
    .expect("buscar título concreto");
    let advanced = library::list_games(
        &connection,
        &GameListRequest {
            installed: Some(true),
            never_played: Some(false),
            tracking: Some(false),
            min_playtime_minutes: Some(10_000),
            max_progress: Some(0),
            genre: Some("Acción".into()),
            category: Some("Cooperativo".into()),
            sort: Some("alphabetical".into()),
            limit: Some(120),
            ..GameListRequest::default()
        },
        None,
    )
    .expect("combinar filtros avanzados sobre 5000 juegos");
    let query_elapsed = query_started.elapsed();
    eprintln!(
        "gate 5000 juegos: importación {import_elapsed:?}; paginación + filtros + búsqueda {query_elapsed:?}"
    );

    assert_eq!(first_page.total, LARGE_LIBRARY_SIZE as i64);
    assert_eq!(first_page.items.len(), 120);
    assert_eq!(first_page.items.first().map(|game| game.app_id), Some(1));
    assert_eq!(first_page.items.last().map(|game| game.app_id), Some(120));
    assert_eq!(second_page.total, LARGE_LIBRARY_SIZE as i64);
    assert_eq!(second_page.items.len(), 120);
    assert_eq!(second_page.items.first().map(|game| game.app_id), Some(121));
    assert!(
        first_page.items.iter().all(|game| !second_page
            .items
            .iter()
            .any(|next| next.app_id == game.app_id)),
        "las páginas no deben solaparse"
    );
    assert_eq!(installed.total, (LARGE_LIBRARY_SIZE / 4) as i64);
    assert_eq!(installed.items.len(), 500);
    assert!(installed.items.iter().all(|game| game.installed));
    assert_eq!(filtered.total, 1);
    assert_eq!(filtered.items[0].app_id, 4_321);
    assert!(advanced.total > 0);
    assert_eq!(advanced.items.len(), 120);
    assert!(advanced.items.iter().all(|game| {
        game.installed && game.playtime_minutes >= 10_000 && game.progress == 0 && !game.tracking
    }));
    assert!(
        query_elapsed < Duration::from_secs(3),
        "paginación, filtros y búsqueda sobre {LARGE_LIBRARY_SIZE} juegos tardaron {query_elapsed:?}, por encima del presupuesto de 3 s"
    );
}
