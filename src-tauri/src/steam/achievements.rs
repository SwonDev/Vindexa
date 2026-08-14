use super::secrets;
use super::web_api::validate_steam_id;
use crate::error::{AppError, AppResult};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;

// Método público documentado por Steamworks:
// https://partner.steamgames.com/doc/webapi/ISteamUserStats#GetPlayerAchievements
const PLAYER_ACHIEVEMENTS_ENDPOINT: &str =
    "https://api.steampowered.com/ISteamUserStats/GetPlayerAchievements/v0001/";
const MAX_ACHIEVEMENTS_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AchievementSummary {
    pub unlocked: u32,
    pub total: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AchievementOutcome {
    Found(AchievementSummary),
    Unavailable,
}

#[derive(Debug, Deserialize)]
struct AchievementEnvelope {
    playerstats: PlayerStats,
}

#[derive(Debug, Deserialize)]
struct PlayerStats {
    #[serde(default)]
    success: bool,
    achievements: Option<Vec<PlayerAchievement>>,
}

#[derive(Debug, Deserialize)]
struct PlayerAchievement {
    #[serde(default)]
    achieved: u8,
}

/// Lee la clave del llavero sólo cuando el usuario solicita expresamente
/// actualizar los logros de una ficha. No se invoca durante `bootstrap` ni al
/// abrir la biblioteca.
pub async fn fetch_saved(steam_id: &str, app_id: u32) -> AppResult<AchievementOutcome> {
    let api_key = secrets::load_api_key()?.ok_or_else(|| {
        AppError::new(
            "steam_api_key_missing",
            "Añade tu clave de Steam Web API en Ajustes antes de actualizar los logros.",
        )
    })?;
    fetch(&api_key, steam_id, app_id).await
}

pub async fn fetch(api_key: &str, steam_id: &str, app_id: u32) -> AppResult<AchievementOutcome> {
    secrets::validate_api_key(api_key)?;
    validate_steam_id(steam_id)?;
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido."));
    }
    fetch_from_endpoint(
        achievements_client()?,
        PLAYER_ACHIEVEMENTS_ENDPOINT,
        api_key,
        steam_id,
        app_id,
    )
    .await
}

fn achievements_client() -> AppResult<&'static Client> {
    static CLIENT: OnceLock<Result<Client, AppError>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(12))
            .redirect(Policy::none())
            .user_agent("Vindexa/0.1 (+https://vindexa.app)")
            .build()
            .map_err(|_| {
                AppError::new(
                    "steam_achievements_http_client",
                    "No se pudo preparar la conexión segura con los logros de Steam.",
                )
            })
    }) {
        Ok(client) => Ok(client),
        Err(error) => Err(error.clone()),
    }
}

async fn fetch_from_endpoint(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    steam_id: &str,
    app_id: u32,
) -> AppResult<AchievementOutcome> {
    let mut response = client
        .get(endpoint)
        .query(&[
            ("key", api_key.to_owned()),
            ("steamid", steam_id.to_owned()),
            ("appid", app_id.to_string()),
            ("l", "spanish".to_owned()),
            ("format", "json".to_owned()),
        ])
        .send()
        .await
        .map_err(classify_request_error)?;
    let status = response.status();
    if status.is_redirection() {
        return Err(AppError::new(
            "steam_achievements_redirect",
            "Steam redirigió inesperadamente la consulta de logros.",
        ));
    }
    if status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND {
        return Ok(AchievementOutcome::Unavailable);
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
            "Steam ha limitado temporalmente la consulta de logros.",
        ));
    }
    if !status.is_success() {
        return Err(AppError::new(
            "steam_achievements_status",
            format!("Steam respondió con el estado {status} al consultar los logros."),
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
            "steam_achievements_content_type",
            "Steam devolvió un tipo de contenido inesperado al consultar los logros.",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ACHIEVEMENTS_RESPONSE_BYTES as u64)
    {
        return Err(AppError::new(
            "steam_achievements_too_large",
            "La respuesta de logros supera el tamaño máximo permitido.",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify_request_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_ACHIEVEMENTS_RESPONSE_BYTES {
            return Err(AppError::new(
                "steam_achievements_too_large",
                "La respuesta de logros supera el tamaño máximo permitido.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_response(&bytes)
}

fn parse_response(bytes: &[u8]) -> AppResult<AchievementOutcome> {
    let envelope: AchievementEnvelope = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_achievements_response",
            "Steam devolvió unos logros que Vindexa no pudo interpretar.",
        )
    })?;
    if !envelope.playerstats.success {
        return Ok(AchievementOutcome::Unavailable);
    }
    let achievements = envelope.playerstats.achievements.unwrap_or_default();
    let total = u32::try_from(achievements.len()).map_err(|_| {
        AppError::new(
            "steam_achievements_response",
            "Steam devolvió demasiados logros para procesarlos de forma segura.",
        )
    })?;
    let unlocked = u32::try_from(
        achievements
            .iter()
            .filter(|achievement| achievement.achieved != 0)
            .count(),
    )
    .map_err(|_| {
        AppError::new(
            "steam_achievements_response",
            "Steam devolvió demasiados logros para procesarlos de forma segura.",
        )
    })?;
    Ok(AchievementOutcome::Found(AchievementSummary {
        unlocked,
        total,
    }))
}

fn classify_request_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "steam_achievements_timeout",
            "Steam no respondió a tiempo al consultar los logros.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "steam_achievements_connection",
            "No se pudo conectar de forma segura con los logros de Steam.",
        );
    }
    AppError::new(
        "steam_achievements_network",
        "La consulta de logros de Steam no pudo completarse de forma segura.",
    )
}

#[cfg(test)]
mod tests {
    use super::{AchievementOutcome, fetch_from_endpoint, parse_response};
    use reqwest::{Client, redirect::Policy};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: &'static str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reservar puerto local");
        let address = listener.local_addr().expect("leer puerto local");
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("aceptar petición HTTP local");
            let mut request = [0_u8; 4096];
            let read = stream.read(&mut request).expect("leer petición HTTP local");
            let request = String::from_utf8_lossy(&request[..read]);
            assert!(request.contains("appid=620"));
            assert!(request.contains("steamid=76561198000000000"));
            stream
                .write_all(response.as_bytes())
                .expect("responder petición HTTP local");
        });
        (format!("http://{address}/"), task)
    }

    #[test]
    fn counts_only_unlocked_achievements_from_the_documented_shape() {
        let payload = br#"{
          "playerstats": {
            "steamID": "76561198000000000",
            "gameName": "Portal 2",
            "success": true,
            "achievements": [
              {"apiname":"A","achieved":1,"unlocktime":1700000000},
              {"apiname":"B","achieved":0,"unlocktime":0},
              {"apiname":"C","achieved":1,"unlocktime":1700000001}
            ]
          }
        }"#;
        let outcome = parse_response(payload).expect("interpretar logros");
        let AchievementOutcome::Found(summary) = outcome else {
            panic!("los logros debían estar disponibles");
        };
        assert_eq!(summary.unlocked, 2);
        assert_eq!(summary.total, 3);
    }

    #[test]
    fn distinguishes_private_or_unsupported_achievements() {
        let payload = br#"{"playerstats":{"success":false,"error":"Profile is not public"}}"#;
        assert_eq!(
            parse_response(payload).expect("interpretar indisponibilidad"),
            AchievementOutcome::Unavailable
        );
    }

    #[test]
    fn rejects_redirects_without_forwarding_the_key() {
        let (endpoint, server) = serve_once(
            "HTTP/1.1 302 Found\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n",
        );
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .expect("crear cliente HTTP");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime");
        let error = runtime
            .block_on(fetch_from_endpoint(
                &client,
                &endpoint,
                "0123456789ABCDEF0123456789ABCDEF",
                "76561198000000000",
                620,
            ))
            .expect_err("rechazar redirección");
        server.join().expect("cerrar servidor");
        assert_eq!(error.code, "steam_achievements_redirect");
        assert!(!error.message.contains("0123456789ABCDEF"));
    }

    #[test]
    fn treats_an_app_without_stats_as_unavailable() {
        let (endpoint, server) = serve_once(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
        );
        let client = Client::builder().build().expect("crear cliente HTTP");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime");
        let outcome = runtime
            .block_on(fetch_from_endpoint(
                &client,
                &endpoint,
                "0123456789ABCDEF0123456789ABCDEF",
                "76561198000000000",
                620,
            ))
            .expect("interpretar ausencia");
        server.join().expect("cerrar servidor");
        assert_eq!(outcome, AchievementOutcome::Unavailable);
    }
}
