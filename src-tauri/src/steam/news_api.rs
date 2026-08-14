use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;

const NEWS_ENDPOINT: &str = "https://api.steampowered.com/ISteamNews/GetNewsForApp/v2/";
const OFFICIAL_FEED: &str = "steam_community_announcements";
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_NEWS_ITEMS: usize = 8;
const MAX_TITLE_CHARS: usize = 240;
const MAX_PREVIEW_CHARS: usize = 360;
const CONTRACT_RETRY_SECONDS: u64 = 6 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamNewsPublication {
    pub gid: String,
    pub app_id: u32,
    pub title: String,
    pub content_preview: String,
    pub published_at: String,
    pub feed_label: String,
    pub feed_name: String,
}

#[derive(Debug)]
pub struct SteamNewsFailure {
    pub error: AppError,
    pub retry_after_seconds: Option<u64>,
}

impl From<AppError> for SteamNewsFailure {
    fn from(error: AppError) -> Self {
        Self {
            error,
            retry_after_seconds: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct NewsEnvelope {
    appnews: RawAppNews,
}

#[derive(Debug, Deserialize)]
struct RawAppNews {
    appid: u32,
    #[serde(default)]
    newsitems: Vec<RawNewsItem>,
}

#[derive(Debug, Deserialize)]
struct RawNewsItem {
    gid: Option<String>,
    title: Option<String>,
    contents: Option<String>,
    date: Option<i64>,
    feedlabel: Option<String>,
    feedname: Option<String>,
    appid: Option<u32>,
}

pub async fn fetch(app_id: u32) -> Result<Vec<SteamNewsPublication>, SteamNewsFailure> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido.").into());
    }
    fetch_from_endpoint(
        news_client().map_err(SteamNewsFailure::from)?,
        NEWS_ENDPOINT,
        app_id,
    )
    .await
}

fn news_client() -> AppResult<&'static Client> {
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
                    "steam_news_http_client",
                    "No se pudo preparar la conexión segura con las noticias de Steam.",
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
    app_id: u32,
) -> Result<Vec<SteamNewsPublication>, SteamNewsFailure> {
    let mut response = client
        .get(endpoint)
        .query(&[
            ("appid", app_id.to_string()),
            ("count", MAX_NEWS_ITEMS.to_string()),
            ("maxlength", MAX_PREVIEW_CHARS.to_string()),
            ("feeds", OFFICIAL_FEED.to_string()),
            ("format", "json".to_string()),
        ])
        .send()
        .await
        .map_err(|error| SteamNewsFailure::from(classify_request_error(error)))?;

    let status = response.status();
    if status.is_redirection() {
        return Err(AppError::new(
            "steam_news_redirect",
            "Steam redirigió inesperadamente la consulta de publicaciones.",
        )
        .into());
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(SteamNewsFailure {
            error: AppError::new(
                "steam_news_rate_limited",
                "Steam ha limitado temporalmente la consulta de publicaciones.",
            ),
            retry_after_seconds: parse_retry_after(response.headers().get(header::RETRY_AFTER)),
        });
    }
    if status.is_server_error() {
        return Err(AppError::new(
            "steam_news_unavailable",
            "El feed de Steam no está disponible temporalmente.",
        )
        .into());
    }
    if !status.is_success() {
        return Err(AppError::new(
            "steam_news_status",
            format!("El feed de Steam respondió con el estado {status}."),
        )
        .into());
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim);
    if !content_type.is_some_and(|value| value.eq_ignore_ascii_case("application/json")) {
        return Err(AppError::new(
            "steam_news_content_type",
            "Steam devolvió un tipo de contenido inesperado para sus publicaciones.",
        )
        .into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(AppError::new(
            "steam_news_too_large",
            "El feed de Steam supera el tamaño máximo permitido.",
        )
        .into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| SteamNewsFailure::from(classify_request_error(error)))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(AppError::new(
                "steam_news_too_large",
                "El feed de Steam supera el tamaño máximo permitido.",
            )
            .into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_news_response(app_id, &bytes).map_err(Into::into)
}

fn parse_news_response(app_id: u32, bytes: &[u8]) -> AppResult<Vec<SteamNewsPublication>> {
    let envelope: NewsEnvelope = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_news_response",
            "Steam devolvió publicaciones que Vindexa no pudo interpretar.",
        )
    })?;
    if envelope.appnews.appid != app_id {
        return Err(AppError::new(
            "steam_news_response",
            "Steam devolvió publicaciones para un juego distinto del solicitado.",
        ));
    }
    Ok(envelope
        .appnews
        .newsitems
        .into_iter()
        .filter_map(|item| map_news_item(app_id, item))
        .take(MAX_NEWS_ITEMS)
        .collect())
}

fn map_news_item(app_id: u32, item: RawNewsItem) -> Option<SteamNewsPublication> {
    if item.appid != Some(app_id) || item.feedname.as_deref() != Some(OFFICIAL_FEED) {
        return None;
    }
    let gid = item.gid?.trim().to_string();
    if gid.is_empty() || gid.len() > 40 || !gid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let title = sanitize_text(&item.title?, MAX_TITLE_CHARS);
    if title.is_empty() {
        return None;
    }
    let published_at = DateTime::<Utc>::from_timestamp(item.date?, 0)?.to_rfc3339();
    let feed_label = sanitize_text(item.feedlabel.as_deref().unwrap_or("Steam"), 80);
    Some(SteamNewsPublication {
        gid,
        app_id,
        title,
        content_preview: sanitize_text(
            item.contents.as_deref().unwrap_or_default(),
            MAX_PREVIEW_CHARS,
        ),
        published_at,
        feed_label: if feed_label.is_empty() {
            "Steam".to_string()
        } else {
            feed_label
        },
        feed_name: OFFICIAL_FEED.to_string(),
    })
}

fn sanitize_text(value: &str, limit: usize) -> String {
    let mut plain = String::with_capacity(value.len().min(limit));
    let mut html_tag = false;
    let mut bbcode_tag = false;
    for character in value.chars() {
        match character {
            '<' if !bbcode_tag => {
                html_tag = true;
                plain.push(' ');
            }
            '>' if html_tag => {
                html_tag = false;
                plain.push(' ');
            }
            '[' if !html_tag => {
                bbcode_tag = true;
                plain.push(' ');
            }
            ']' if bbcode_tag => {
                bbcode_tag = false;
                plain.push(' ');
            }
            _ if !html_tag && !bbcode_tag => plain.push(character),
            _ => {}
        }
    }
    decode_basic_entities(&plain)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn decode_basic_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn classify_request_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "steam_news_timeout",
            "El feed de Steam no respondió a tiempo.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "steam_news_connection",
            "No se pudo conectar de forma segura con el feed de Steam.",
        );
    }
    AppError::new(
        "steam_news_network",
        "No se pudieron consultar las publicaciones de Steam.",
    )
}

fn parse_retry_after(value: Option<&header::HeaderValue>) -> Option<u64> {
    value?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|seconds| seconds.clamp(60, CONTRACT_RETRY_SECONDS))
}

pub fn retry_delay_seconds(error_code: &str, attempts: u32, retry_after: Option<u64>) -> u64 {
    if error_code == "steam_news_rate_limited" {
        return retry_after
            .unwrap_or(15 * 60)
            .clamp(60, CONTRACT_RETRY_SECONDS);
    }
    if matches!(
        error_code,
        "steam_news_timeout"
            | "steam_news_connection"
            | "steam_news_network"
            | "steam_news_unavailable"
    ) {
        let exponent = attempts.saturating_sub(1).min(5);
        return (15 * 60_u64)
            .saturating_mul(1_u64 << exponent)
            .clamp(60, CONTRACT_RETRY_SECONDS);
    }
    CONTRACT_RETRY_SECONDS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    fn serve_once(response: &'static str, delay: Duration) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("reservar puerto local");
        let address = listener.local_addr().expect("leer puerto local");
        let task = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("aceptar petición HTTP local");
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).expect("leer petición HTTP local");
            thread::sleep(delay);
            let _ = stream.write_all(response.as_bytes());
        });
        (format!("http://{address}/"), task)
    }

    fn test_client(timeout: Duration) -> Client {
        Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(1))
            .timeout(timeout)
            .build()
            .expect("crear cliente HTTP")
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime")
    }

    #[test]
    fn parses_only_the_requested_official_feed_and_sanitizes_plain_text() {
        let payload = r#"{
          "appnews": {
            "appid": 570,
            "count": 2,
            "newsitems": [
              {
                "gid": "1840944183772671",
                "title": " Gameplay  patch ",
                "contents": "[img]asset[/img] <b>Notas</b>   reales",
                "date": 1785455895,
                "feedlabel": "Community Announcements",
                "feedname": "steam_community_announcements",
                "appid": 570
              },
              {
                "gid": "otro-feed",
                "title": "Artículo externo",
                "contents": "No debe entrar",
                "date": 1785455895,
                "feedlabel": "Gaming News",
                "feedname": "gaming_news",
                "appid": 570
              }
            ]
          }
        }"#;

        let items = parse_news_response(570, payload.as_bytes()).expect("interpretar feed");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].gid, "1840944183772671");
        assert_eq!(items[0].title, "Gameplay patch");
        assert_eq!(items[0].content_preview, "asset Notas reales");
        assert_eq!(items[0].feed_name, "steam_community_announcements");
        assert_eq!(items[0].published_at, "2026-07-30T23:58:15+00:00");
    }

    #[test]
    fn rejects_an_envelope_for_a_different_app_instead_of_misattributing_news() {
        let payload = br#"{"appnews":{"appid":730,"count":0,"newsitems":[]}}"#;
        let error = parse_news_response(570, payload).expect_err("rechazar AppID distinto");
        assert_eq!(error.code, "steam_news_response");
    }

    #[test]
    fn retry_policy_backs_off_transient_failures_but_not_contract_errors() {
        assert_eq!(
            retry_delay_seconds("steam_news_rate_limited", 1, Some(900)),
            900
        );
        assert_eq!(retry_delay_seconds("steam_news_unavailable", 3, None), 3600);
        assert_eq!(retry_delay_seconds("steam_news_response", 1, None), 21_600);
    }

    #[test]
    fn rejects_redirects_wrong_mime_and_declared_oversized_bodies() {
        for (response, expected_code) in [
            (
                "HTTP/1.1 302 Found\r\nLocation: https://example.com/\r\nContent-Length: 0\r\n\r\n",
                "steam_news_redirect",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\n\r\n{}",
                "steam_news_content_type",
            ),
            (
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 524289\r\n\r\n",
                "steam_news_too_large",
            ),
        ] {
            let (endpoint, server) = serve_once(response, Duration::ZERO);
            let failure = runtime()
                .block_on(fetch_from_endpoint(
                    &test_client(Duration::from_secs(1)),
                    &endpoint,
                    570,
                ))
                .expect_err("rechazar respuesta fuera de contrato");
            server.join().expect("cerrar servidor");
            assert_eq!(failure.error.code, expected_code);
        }
    }

    #[test]
    fn classifies_timeout_and_honors_bounded_retry_after() {
        let (slow_endpoint, slow_server) = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
            Duration::from_millis(80),
        );
        let timeout = runtime()
            .block_on(fetch_from_endpoint(
                &test_client(Duration::from_millis(20)),
                &slow_endpoint,
                570,
            ))
            .expect_err("agotar timeout");
        slow_server.join().expect("cerrar servidor lento");
        assert_eq!(timeout.error.code, "steam_news_timeout");

        let (rate_endpoint, rate_server) = serve_once(
            "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 999999\r\nContent-Length: 0\r\n\r\n",
            Duration::ZERO,
        );
        let rate_limit = runtime()
            .block_on(fetch_from_endpoint(
                &test_client(Duration::from_secs(1)),
                &rate_endpoint,
                570,
            ))
            .expect_err("interpretar rate limit");
        rate_server.join().expect("cerrar servidor rate limit");
        assert_eq!(rate_limit.error.code, "steam_news_rate_limited");
        assert_eq!(rate_limit.retry_after_seconds, Some(CONTRACT_RETRY_SECONDS));
    }
}
