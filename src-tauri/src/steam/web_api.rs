use super::local::{cover_url, header_url};
use super::secrets;
use crate::db::{Database, ImportedFamilyCatalogGame, ImportedGame};
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

const OWNED_GAMES_ENDPOINT: &str =
    "https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/";
const PLAYER_SUMMARIES_ENDPOINT: &str =
    "https://api.steampowered.com/ISteamUser/GetPlayerSummaries/v0002/";
const MAX_STEAM_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug)]
pub struct SteamLibrarySnapshot {
    pub steam_id: String,
    pub games: Vec<ImportedGame>,
    pub profile: Option<SteamProfile>,
    pub private_library_suspected: bool,
    pub family_members_detected: usize,
    pub family_members_inaccessible: usize,
    pub family_games_imported: usize,
    pub family_catalog: Vec<ImportedFamilyCatalogGame>,
    pub family_catalog_complete: bool,
}

#[derive(Debug, Clone)]
pub struct SteamProfile {
    pub persona_name: String,
    pub avatar_url: Option<String>,
    pub profile_url: Option<String>,
    pub visibility: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct OwnedGamesEnvelope {
    response: OwnedGamesResponse,
}

#[derive(Debug, Deserialize)]
struct OwnedGamesResponse {
    #[serde(default)]
    game_count: Option<u32>,
    games: Option<Vec<OwnedGame>>,
}

#[derive(Debug, Deserialize)]
struct OwnedGame {
    appid: u32,
    name: String,
    #[serde(default)]
    playtime_forever: u64,
    #[serde(default)]
    playtime_2weeks: u64,
    img_icon_url: Option<String>,
    #[serde(default)]
    rtime_last_played: i64,
}

#[derive(Debug, Deserialize)]
struct PlayerSummariesEnvelope {
    response: PlayerSummariesResponse,
}

#[derive(Debug, Deserialize)]
struct PlayerSummariesResponse {
    #[serde(default)]
    players: Vec<PlayerSummary>,
}

#[derive(Debug, Deserialize)]
struct PlayerSummary {
    steamid: String,
    personaname: String,
    profileurl: Option<String>,
    avatarfull: Option<String>,
    communityvisibilitystate: Option<u8>,
}

pub async fn fetch_saved_account(steam_id: &str) -> AppResult<SteamLibrarySnapshot> {
    let api_key = secrets::load_api_key()?.ok_or_else(|| {
        AppError::new(
            "steam_api_key_missing",
            "Añade tu clave de Steam Web API en Ajustes antes de sincronizar la biblioteca.",
        )
    })?;
    fetch(&api_key, steam_id).await
}

pub async fn fetch(api_key: &str, steam_id: &str) -> AppResult<SteamLibrarySnapshot> {
    secrets::validate_api_key(api_key)?;
    validate_steam_id(steam_id)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirect(Policy::none())
        .user_agent("Vindexa/0.1 (+https://vindexa.app)")
        .build()
        .map_err(|_| {
            AppError::new(
                "steam_http_client",
                "No se pudo preparar la conexión segura con Steam.",
            )
        })?;

    let owned_params = [
        ("key", api_key),
        ("steamid", steam_id),
        ("include_appinfo", "1"),
        ("include_played_free_games", "1"),
        ("format", "json"),
    ];
    let profile_params = [("key", api_key), ("steamids", steam_id), ("format", "json")];

    let owned =
        get_json::<OwnedGamesEnvelope>(&client, OWNED_GAMES_ENDPOINT, &owned_params).await?;
    let summaries =
        get_json::<PlayerSummariesEnvelope>(&client, PLAYER_SUMMARIES_ENDPOINT, &profile_params)
            .await?;

    let private_library_suspected = library_access_unavailable(&owned.response);
    let own_games = owned
        .response
        .games
        .unwrap_or_default()
        .into_iter()
        .filter_map(|game| map_owned_game(game, "owned", "not_applicable"))
        .collect::<Vec<_>>();
    let profile = summaries
        .response
        .players
        .into_iter()
        .find(|profile| profile.steamid == steam_id)
        .map(|profile| SteamProfile {
            persona_name: profile.personaname,
            avatar_url: sanitize_profile_url(profile.avatarfull),
            profile_url: sanitize_profile_url(profile.profileurl),
            visibility: profile.communityvisibilitystate,
        });

    let family = merge_family_games(&client, api_key, steam_id, own_games).await;

    Ok(SteamLibrarySnapshot {
        steam_id: steam_id.to_owned(),
        games: family.games,
        profile,
        private_library_suspected,
        family_members_detected: family.members_detected,
        family_members_inaccessible: family.members_inaccessible,
        family_games_imported: family.confirmed_games,
        family_catalog: family.catalog,
        family_catalog_complete: family.complete,
    })
}

struct FamilyMerge {
    games: Vec<ImportedGame>,
    catalog: Vec<ImportedFamilyCatalogGame>,
    members_detected: usize,
    members_inaccessible: usize,
    confirmed_games: usize,
    complete: bool,
}

async fn merge_family_games(
    client: &Client,
    api_key: &str,
    steam_id: &str,
    own_games: Vec<ImportedGame>,
) -> FamilyMerge {
    let mut games = own_games
        .into_iter()
        .map(|game| (game.app_id, game))
        .collect::<BTreeMap<_, _>>();
    let Some(evidence) = super::family::detect_current(steam_id).ok().flatten() else {
        return FamilyMerge {
            games: games.into_values().collect(),
            catalog: Vec::new(),
            members_detected: 0,
            members_inaccessible: 0,
            confirmed_games: 0,
            complete: false,
        };
    };
    let detected = evidence.group.member_steam_ids.len();
    let mut inaccessible = 0;
    let mut confirmed_games = 0;
    let mut catalog = BTreeMap::new();

    for member_steam_id in evidence.group.member_steam_ids {
        let params = [
            ("key", api_key),
            ("steamid", member_steam_id.as_str()),
            ("include_appinfo", "1"),
            ("include_played_free_games", "1"),
            ("format", "json"),
        ];
        let response =
            match get_json::<OwnedGamesEnvelope>(client, OWNED_GAMES_ENDPOINT, &params).await {
                Ok(response) => response.response,
                Err(_) => {
                    inaccessible += 1;
                    continue;
                }
            };
        if library_access_unavailable(&response) {
            inaccessible += 1;
            continue;
        }
        for owned in response.games.unwrap_or_default() {
            if games.contains_key(&owned.appid) {
                continue;
            }
            let availability = if evidence.cached_library_app_ids.contains(&owned.appid) {
                "confirmed"
            } else {
                "unknown"
            };
            let Some(game) = map_owned_game(owned, "family_shared", availability) else {
                continue;
            };
            catalog
                .entry(game.app_id)
                .and_modify(|existing: &mut ImportedFamilyCatalogGame| {
                    if availability == "confirmed" {
                        existing.availability = "confirmed".to_string();
                    }
                })
                .or_insert_with(|| ImportedFamilyCatalogGame {
                    app_id: game.app_id,
                    title: game.title.clone(),
                    icon_url: game.icon_url.clone(),
                    cover_url: game.cover_url.clone(),
                    header_url: game.header_url.clone(),
                    availability: availability.to_string(),
                });
            if availability == "confirmed" {
                games.insert(game.app_id, game);
                confirmed_games += 1;
            }
        }
    }

    FamilyMerge {
        games: games.into_values().collect(),
        catalog: catalog.into_values().collect(),
        members_detected: detected,
        members_inaccessible: inaccessible,
        confirmed_games,
        complete: inaccessible == 0,
    }
}

pub fn mark_sync_failed(database: &Database, steam_id: &str, error: &AppError) -> AppResult<()> {
    validate_steam_id(steam_id)?;
    let persisted_message = safe_persisted_error_message(error);
    database.open()?.execute(
        "UPDATE steam_accounts SET
            last_sync_status = 'failed',
            last_sync_error_code = ?2,
            last_sync_error_message = ?3,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE steam_id = ?1",
        rusqlite::params![steam_id, error.code, persisted_message],
    )?;
    Ok(())
}

async fn get_json<T: for<'de> Deserialize<'de>>(
    client: &Client,
    endpoint: &str,
    query: &[(&str, &str)],
) -> AppResult<T> {
    let mut response = client
        .get(endpoint)
        .query(query)
        .send()
        .await
        .map_err(classify_request_error)?;
    let status = response.status();
    if status.is_redirection() {
        return Err(AppError::new(
            "steam_api_redirect",
            "Steam redirigió inesperadamente la solicitud. La sincronización se detuvo para no enviar la clave a otro destino.",
        ));
    }
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(AppError::new(
            "steam_api_unauthorized",
            "Steam rechazó la clave Web API. Revísala en Ajustes.",
        ));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(AppError::new(
            "steam_rate_limited",
            "Steam ha limitado temporalmente las solicitudes. Espera unos minutos antes de sincronizar de nuevo.",
        ));
    }
    if status.is_server_error() {
        return Err(AppError::new(
            "steam_api_unavailable",
            "Steam Web API no está disponible temporalmente. Vuelve a intentarlo más tarde.",
        ));
    }
    if !status.is_success() {
        return Err(AppError::new(
            "steam_api_status",
            format!("Steam Web API respondió con el estado {status}."),
        ));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !content_type.is_some_and(|value| value.eq_ignore_ascii_case("application/json")) {
        return Err(AppError::new(
            "steam_api_content_type",
            "Steam devolvió un tipo de contenido inesperado.",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_STEAM_RESPONSE_BYTES as u64)
    {
        return Err(AppError::new(
            "steam_api_too_large",
            "La respuesta de Steam supera el tamaño máximo permitido.",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify_request_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_STEAM_RESPONSE_BYTES {
            return Err(AppError::new(
                "steam_api_too_large",
                "La respuesta de Steam supera el tamaño máximo permitido.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        AppError::new(
            "steam_api_response",
            "Steam devolvió una respuesta que Vindexa no pudo interpretar.",
        )
    })
}

fn map_owned_game(
    game: OwnedGame,
    ownership_source: &str,
    family_availability: &str,
) -> Option<ImportedGame> {
    if game.appid == 0 || game.name.trim().is_empty() {
        return None;
    }
    let icon_url = game.img_icon_url.as_deref().and_then(|hash| {
        let valid = hash.len() == 40 && hash.bytes().all(|byte| byte.is_ascii_hexdigit());
        valid.then(|| {
            format!(
                "https://media.steampowered.com/steamcommunity/public/images/apps/{}/{hash}.jpg",
                game.appid
            )
        })
    });
    let is_owned = ownership_source == "owned";
    Some(ImportedGame {
        app_id: game.appid,
        title: game.name.trim().to_owned(),
        icon_url,
        cover_url: Some(cover_url(game.appid)),
        header_url: Some(header_url(game.appid)),
        playtime_minutes: if is_owned { game.playtime_forever } else { 0 },
        playtime_recent_minutes: if is_owned { game.playtime_2weeks } else { 0 },
        last_played_at: is_owned
            .then(|| unix_time(game.rtime_last_played))
            .flatten(),
        ownership_source: ownership_source.to_string(),
        family_availability: family_availability.to_string(),
        installation: None,
    })
}

fn library_access_unavailable(response: &OwnedGamesResponse) -> bool {
    response.games.is_none() && response.game_count.is_none()
}

fn unix_time(seconds: i64) -> Option<String> {
    (seconds > 0)
        .then(|| DateTime::<Utc>::from_timestamp(seconds, 0))
        .flatten()
        .map(|value| value.to_rfc3339())
}

fn sanitize_profile_url(value: Option<String>) -> Option<String> {
    let value = value?;
    let parsed = url::Url::parse(&value).ok()?;
    let allowed_host = parsed.host_str().is_some_and(|host| {
        host == "steamcommunity.com"
            || host == "avatars.steamstatic.com"
            || host == "avatars.fastly.steamstatic.com"
            || host == "cdn.akamai.steamstatic.com"
    });
    (parsed.scheme() == "https" && allowed_host).then_some(value)
}

pub(crate) fn validate_steam_id(value: &str) -> AppResult<u64> {
    if !(16..=20).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::validation(
            "El SteamID64 no tiene un formato válido.",
        ));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| AppError::validation("El SteamID64 no tiene un formato válido."))?;
    if parsed == 0 {
        return Err(AppError::validation(
            "El SteamID64 no tiene un formato válido.",
        ));
    }
    Ok(parsed)
}

fn classify_request_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "steam_timeout",
            "Steam no respondió a tiempo. Vuelve a intentarlo.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "steam_connection",
            "No se pudo establecer una conexión segura con Steam. Comprueba la conexión a Internet y vuelve a intentarlo.",
        );
    }
    AppError::new(
        "steam_network",
        "La solicitud a Steam no pudo completarse de forma segura. Vuelve a intentarlo.",
    )
}

fn safe_persisted_error_message(error: &AppError) -> String {
    let contains_sensitive_context = error.message.contains("http://")
        || error.message.contains("https://")
        || error.message.to_ascii_lowercase().contains("key=");
    if contains_sensitive_context {
        return "La sincronización no pudo completarse; los detalles sensibles se omitieron."
            .to_string();
    }
    error.message.chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        OwnedGamesEnvelope, get_json, library_access_unavailable, map_owned_game,
        sanitize_profile_url, validate_steam_id,
    };
    use reqwest::{Client, redirect::Policy};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;
    use tempfile::TempDir;

    use crate::db::Database;
    use crate::error::AppError;

    fn serve_once(response: &'static str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reservar puerto local");
        let address = listener.local_addr().expect("leer puerto local");
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("aceptar petición HTTP local");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("leer petición HTTP local");
            stream
                .write_all(response.as_bytes())
                .expect("responder petición HTTP local");
        });
        (format!("http://{address}/"), task)
    }

    #[test]
    fn parses_owned_games_without_mocking_the_product_flow() {
        let payload = r#"{
            "response": {
                "game_count": 1,
                "games": [{
                    "appid": 570,
                    "name": "Dota 2",
                    "playtime_forever": 321,
                    "playtime_2weeks": 12,
                    "img_icon_url": "0123456789abcdef0123456789abcdef01234567",
                    "rtime_last_played": 1700000000
                }]
            }
        }"#;
        let parsed: OwnedGamesEnvelope = serde_json::from_str(payload).unwrap();
        let game = map_owned_game(
            parsed.response.games.unwrap().remove(0),
            "owned",
            "not_applicable",
        )
        .unwrap();
        assert_eq!(game.app_id, 570);
        assert_eq!(game.playtime_minutes, 321);
        assert!(
            game.icon_url
                .unwrap()
                .starts_with("https://media.steampowered.com/")
        );
    }

    #[test]
    fn never_attributes_a_family_members_playtime_to_the_linked_account() {
        let parsed: OwnedGamesEnvelope = serde_json::from_str(
            r#"{"response":{"games":[{
                "appid":620,
                "name":"Portal 2",
                "playtime_forever":9999,
                "playtime_2weeks":42,
                "rtime_last_played":1700000000
            }]}}"#,
        )
        .expect("interpretar fixture");
        let game = map_owned_game(
            parsed.response.games.unwrap().remove(0),
            "family_shared",
            "confirmed",
        )
        .expect("mapear juego familiar");

        assert_eq!(game.playtime_minutes, 0);
        assert_eq!(game.playtime_recent_minutes, 0);
        assert_eq!(game.last_played_at, None);
        assert_eq!(game.ownership_source, "family_shared");
        assert_eq!(game.family_availability, "confirmed");
    }

    #[test]
    fn distinguishes_an_empty_public_library_from_an_unavailable_library() {
        let public_empty: OwnedGamesEnvelope =
            serde_json::from_str(r#"{"response":{"game_count":0}}"#).unwrap();
        assert!(!library_access_unavailable(&public_empty.response));

        let unavailable: OwnedGamesEnvelope = serde_json::from_str(r#"{"response":{}}"#).unwrap();
        assert!(library_access_unavailable(&unavailable.response));
    }

    #[test]
    fn validates_steam_ids_and_profile_hosts() {
        assert!(validate_steam_id("76561198000000000").is_ok());
        assert!(validate_steam_id("7656not-a-steamid").is_err());
        assert!(
            sanitize_profile_url(Some("https://steamcommunity.com/id/example/".into())).is_some()
        );
        assert!(
            sanitize_profile_url(Some(
                "https://avatars.fastly.steamstatic.com/example_full.jpg".into()
            ))
            .is_some()
        );
        assert!(
            include_str!("../../tauri.conf.json")
                .contains("https://avatars.fastly.steamstatic.com")
        );
        assert!(
            sanitize_profile_url(Some(
                "https://avatars.unlisted.steamstatic.com/example_full.jpg".into()
            ))
            .is_none()
        );
        assert!(sanitize_profile_url(Some("https://example.com/steal".into())).is_none());
    }

    #[test]
    fn reports_redirects_as_a_specific_api_error_without_following_them() {
        let (endpoint, server) = serve_once(
            "HTTP/1.1 302 Found\r\nLocation: https://example.com/?key=fixture-secret\r\nContent-Length: 0\r\n\r\n",
        );
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .expect("crear cliente HTTP");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime de prueba");
        let error = runtime
            .block_on(get_json::<OwnedGamesEnvelope>(
                &client,
                &endpoint,
                &[("key", "fixture-secret")],
            ))
            .expect_err("rechazar redirección");
        server.join().expect("cerrar servidor local");

        assert_eq!(error.code, "steam_api_redirect");
        assert!(!error.message.contains("example.com"));
        assert!(!error.message.contains("fixture-secret"));
    }

    #[test]
    fn rejects_non_json_and_declared_oversized_success_responses() {
        let client = Client::builder().build().expect("crear cliente HTTP");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime de prueba");

        let (html_endpoint, html_server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\n{}");
        let html_error = runtime
            .block_on(get_json::<OwnedGamesEnvelope>(&client, &html_endpoint, &[]))
            .expect_err("rechazar respuesta no JSON");
        html_server.join().expect("cerrar servidor local");
        assert_eq!(html_error.code, "steam_api_content_type");

        let (large_endpoint, large_server) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 33554433\r\n\r\n",
        );
        let large_error = runtime
            .block_on(get_json::<OwnedGamesEnvelope>(
                &client,
                &large_endpoint,
                &[],
            ))
            .expect_err("rechazar respuesta sobredimensionada");
        large_server.join().expect("cerrar servidor local");
        assert_eq!(large_error.code, "steam_api_too_large");
    }

    #[test]
    fn persists_the_exact_sync_error_and_clears_it_after_success() {
        let directory = TempDir::new().expect("crear temporal");
        let database = Database::new(directory.path().join("sync.sqlite3"));
        database.initialize().expect("inicializar base");
        let steam_id = "76561198000000000";
        database
            .save_steam_identity(steam_id)
            .expect("guardar cuenta");

        let failure = AppError::new(
            "steam_api_unauthorized",
            "Steam rechazó la clave Web API. Revísala en Ajustes.",
        );
        super::mark_sync_failed(&database, steam_id, &failure).expect("persistir fallo");
        let failed = database
            .get_steam_account()
            .expect("leer cuenta")
            .expect("cuenta guardada");
        assert_eq!(failed.last_sync_status.as_deref(), Some("failed"));
        assert_eq!(
            failed.last_sync_error_code.as_deref(),
            Some("steam_api_unauthorized")
        );
        assert_eq!(
            failed.last_sync_error_message.as_deref(),
            Some("Steam rechazó la clave Web API. Revísala en Ajustes.")
        );

        database
            .persist_steam_sync(database.generation(), steam_id, None, &[], &[], false)
            .expect("persistir éxito sin perfil");
        let succeeded = database
            .get_steam_account()
            .expect("leer cuenta")
            .expect("cuenta guardada");
        assert_eq!(succeeded.last_sync_status.as_deref(), Some("success"));
        assert!(succeeded.last_sync_at.is_some());
        assert!(succeeded.last_sync_error_code.is_none());
        assert!(succeeded.last_sync_error_message.is_none());
    }

    #[test]
    fn persisted_sync_errors_never_store_urls_or_query_secrets() {
        let directory = TempDir::new().expect("crear temporal");
        let database = Database::new(directory.path().join("redaction.sqlite3"));
        database.initialize().expect("inicializar base");
        let steam_id = "76561198000000000";
        database
            .save_steam_identity(steam_id)
            .expect("guardar cuenta");
        let unsafe_error = AppError::new(
            "steam_network",
            "falló https://api.example.test/?key=fixture-secret",
        );

        super::mark_sync_failed(&database, steam_id, &unsafe_error).expect("persistir fallo");

        let persisted = database
            .get_steam_account()
            .expect("leer cuenta")
            .expect("cuenta guardada")
            .last_sync_error_message
            .expect("mensaje persistido");
        assert!(!persisted.contains("http"));
        assert!(!persisted.contains("key="));
        assert!(!persisted.contains("fixture-secret"));
    }
}
