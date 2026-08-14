use crate::db::StoreMetadataUpdate;
use crate::error::{AppError, AppResult};
use chrono::NaiveDate;
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

// Steamworks GetOwnedGames solo documenta nombre e icono. La descripción y los
// créditos se aíslan aquí porque proceden del endpoint público de la tienda,
// cuyo contrato no forma parte de la Steamworks Web API documentada.
const STORE_DETAILS_ENDPOINT: &str = "https://store.steampowered.com/api/appdetails";
const MAX_STORE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 8_000;
const MAX_METADATA_ITEMS: usize = 64;

#[derive(Debug)]
pub enum StoreMetadataOutcome {
    Found(StoreMetadataUpdate),
    Unavailable,
}

#[derive(Debug)]
pub struct StoreMetadataFailure {
    pub error: AppError,
    pub retry_after: Option<Duration>,
}

impl From<AppError> for StoreMetadataFailure {
    fn from(error: AppError) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct StoreEnvelope {
    success: bool,
    data: Option<StoreData>,
}

#[derive(Debug, Deserialize)]
struct StoreData {
    #[serde(default)]
    is_free: bool,
    short_description: Option<String>,
    about_the_game: Option<String>,
    developers: Option<Vec<String>>,
    publishers: Option<Vec<String>>,
    genres: Option<Vec<StoreLabel>>,
    categories: Option<Vec<StoreLabel>>,
    achievements: Option<StoreAchievements>,
    background: Option<String>,
    background_raw: Option<String>,
    release_date: Option<StoreReleaseDate>,
}

#[derive(Debug, Deserialize)]
struct StoreLabel {
    id: Option<serde_json::Value>,
    description: String,
}

#[derive(Debug, Deserialize)]
struct StoreReleaseDate {
    #[serde(default)]
    coming_soon: bool,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoreAchievements {
    total: Option<u32>,
}

pub async fn fetch_with_retry_hint(
    app_id: u32,
) -> Result<StoreMetadataOutcome, StoreMetadataFailure> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido.").into());
    }
    fetch_from_endpoint(
        store_client().map_err(StoreMetadataFailure::from)?,
        STORE_DETAILS_ENDPOINT,
        app_id,
    )
    .await
}

fn store_client() -> AppResult<&'static Client> {
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
                    "steam_store_http_client",
                    "No se pudo preparar la conexión segura con la tienda de Steam.",
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
) -> Result<StoreMetadataOutcome, StoreMetadataFailure> {
    let mut response = client
        .get(endpoint)
        .query(&[
            ("appids", app_id.to_string()),
            ("l", "spanish".to_string()),
            ("cc", "ES".to_string()),
        ])
        .send()
        .await
        .map_err(|error| StoreMetadataFailure::from(classify_store_error(error)))?;
    let status = response.status();
    if status.is_redirection() {
        return Err(AppError::new(
            "steam_store_redirect",
            "La tienda de Steam redirigió inesperadamente la ficha.",
        )
        .into());
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(StoreMetadataFailure {
            error: AppError::new(
                "steam_store_rate_limited",
                "Steam ha limitado temporalmente la carga de fichas.",
            ),
            retry_after: parse_retry_after(response.headers().get(header::RETRY_AFTER)),
        });
    }
    if !status.is_success() {
        return Err(AppError::new(
            "steam_store_status",
            format!("La tienda de Steam respondió con el estado {status}."),
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
            "steam_store_content_type",
            "La tienda de Steam devolvió un tipo de contenido inesperado.",
        )
        .into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_STORE_RESPONSE_BYTES as u64)
    {
        return Err(AppError::new(
            "steam_store_too_large",
            "La ficha de Steam supera el tamaño máximo permitido.",
        )
        .into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| StoreMetadataFailure::from(classify_store_error(error)))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_STORE_RESPONSE_BYTES {
            return Err(AppError::new(
                "steam_store_too_large",
                "La ficha de Steam supera el tamaño máximo permitido.",
            )
            .into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_store_response(app_id, &bytes).map_err(StoreMetadataFailure::from)
}

fn parse_retry_after(value: Option<&header::HeaderValue>) -> Option<Duration> {
    let seconds = value?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.clamp(1, 3_600)))
}

fn parse_store_response(app_id: u32, bytes: &[u8]) -> AppResult<StoreMetadataOutcome> {
    let mut envelopes: HashMap<String, StoreEnvelope> =
        serde_json::from_slice(bytes).map_err(|_| {
            AppError::new(
                "steam_store_response",
                "Steam devolvió una ficha que Vindexa no pudo interpretar.",
            )
        })?;
    let Some(envelope) = envelopes.remove(&app_id.to_string()) else {
        return Ok(StoreMetadataOutcome::Unavailable);
    };
    if !envelope.success {
        return Ok(StoreMetadataOutcome::Unavailable);
    }
    let Some(data) = envelope.data else {
        return Ok(StoreMetadataOutcome::Unavailable);
    };

    let description = data
        .about_the_game
        .as_deref()
        .map(sanitize_store_text)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            data.short_description
                .as_deref()
                .map(sanitize_store_text)
                .filter(|value| !value.is_empty())
        });
    let is_early_access = data.genres.as_ref().is_some_and(|genres| {
        genres.iter().any(|genre| {
            genre
                .id
                .as_ref()
                .is_some_and(|id| id.as_str() == Some("70") || id.as_u64() == Some(70))
        })
    });
    Ok(StoreMetadataOutcome::Found(StoreMetadataUpdate {
        short_description: description,
        hero_url: data
            .background_raw
            .as_deref()
            .and_then(|value| sanitize_hero_url(value, app_id))
            .or_else(|| {
                data.background
                    .as_deref()
                    .and_then(|value| sanitize_hero_url(value, app_id))
            }),
        developer: join_names(data.developers),
        publisher: join_names(data.publishers),
        genres: unique_labels(data.genres),
        categories: unique_labels(data.categories),
        achievements_total: data.achievements.and_then(|value| value.total),
        release_date: data.release_date.as_ref().and_then(normalize_release_date),
        is_free: data.is_free,
        is_early_access,
    }))
}

fn normalize_release_date(value: &StoreReleaseDate) -> Option<String> {
    if value.coming_soon {
        return None;
    }
    let normalized = value.date.as_deref()?.trim().replace(',', " ");
    let parts = normalized.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 {
        return None;
    }
    let day = parts[0].parse::<u32>().ok()?;
    let month = match parts[1].to_uppercase().as_str() {
        "ENE" | "JAN" => 1,
        "FEB" => 2,
        "MAR" => 3,
        "ABR" | "APR" => 4,
        "MAY" => 5,
        "JUN" => 6,
        "JUL" => 7,
        "AGO" | "AUG" => 8,
        "SEP" => 9,
        "OCT" => 10,
        "NOV" => 11,
        "DIC" | "DEC" => 12,
        _ => return None,
    };
    let year = parts[2].parse::<i32>().ok()?;
    NaiveDate::from_ymd_opt(year, month, day).map(|date| date.format("%Y-%m-%d").to_string())
}

fn join_names(values: Option<Vec<String>>) -> Option<String> {
    let values = values?
        .into_iter()
        .take(16)
        .map(|value| sanitize_store_text(&value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| values.join(", "))
}

fn unique_labels(values: Option<Vec<StoreLabel>>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .unwrap_or_default()
        .into_iter()
        .take(MAX_METADATA_ITEMS)
        .map(|value| sanitize_store_text(&value.description))
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.to_lowercase()))
        .collect()
}

fn sanitize_hero_url(value: &str, app_id: u32) -> Option<String> {
    let parsed = url::Url::parse(value).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let expected_path = format!("/images/storepagebackground/app/{app_id}");
    if parsed.host_str() != Some("store.akamai.steamstatic.com") || parsed.path() != expected_path {
        return None;
    }
    if parsed.query_pairs().any(|(key, value)| {
        key != "t" || value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return None;
    }
    Some(parsed.into())
}

fn sanitize_store_text(value: &str) -> String {
    let mut without_tags = String::with_capacity(value.len().min(MAX_DESCRIPTION_CHARS));
    let mut in_tag = false;
    for character in value.chars() {
        match character {
            '<' => {
                in_tag = true;
                without_tags.push(' ');
            }
            '>' if in_tag => {
                in_tag = false;
                without_tags.push(' ');
            }
            _ if !in_tag => without_tags.push(character),
            _ => {}
        }
    }
    let decoded = decode_basic_entities(&without_tags);
    let normalized = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
    normalized.chars().take(MAX_DESCRIPTION_CHARS).collect()
}

fn decode_basic_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find('&') {
        output.push_str(&remaining[..start]);
        let entity_source = &remaining[start..];
        let Some(end) = entity_source.find(';').filter(|end| *end <= 12) else {
            output.push('&');
            remaining = &remaining[start + 1..];
            continue;
        };
        let entity = &entity_source[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "nbsp" => Some(' '),
            value if value.starts_with("#x") || value.starts_with("#X") => {
                u32::from_str_radix(&value[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            value if value.starts_with('#') => {
                value[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            _ => None,
        };
        if let Some(character) = decoded {
            output.push(character);
        } else {
            output.push_str(&entity_source[..=end]);
        }
        remaining = &entity_source[end + 1..];
    }
    output.push_str(remaining);
    output
}

fn classify_store_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "steam_store_timeout",
            "La tienda de Steam no respondió a tiempo.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "steam_store_connection",
            "No se pudo conectar de forma segura con la tienda de Steam.",
        );
    }
    AppError::new(
        "steam_store_network",
        "No se pudo cargar la ficha desde la tienda de Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        StoreMetadataOutcome, parse_retry_after, parse_store_response, sanitize_hero_url,
        sanitize_store_text,
    };
    use reqwest::header::HeaderValue;
    use std::time::Duration;

    #[test]
    fn honors_bounded_retry_after_seconds_from_rate_limit_responses() {
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("90"))),
            Some(Duration::from_secs(90))
        );
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("999999"))),
            Some(Duration::from_secs(3_600))
        );
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("tomorrow"))),
            None
        );
    }

    #[test]
    fn parses_and_sanitizes_real_store_metadata_shape() {
        let payload = r#"{
          "620": {
            "success": true,
            "data": {
              "is_free": false,
              "short_description": "Descripción corta",
              "about_the_game": "<h2>Portal 2</h2><p>Resuelve &amp; coopera.</p><script>ignorar()</script>",
              "developers": ["Valve"],
              "publishers": ["Valve"],
              "genres": [{"id":"1","description":"Acción"}],
              "categories": [{"id":2,"description":"Un jugador"}],
              "achievements": {"total": 51, "highlighted": []},
              "release_date": {"coming_soon": false, "date": "18 ABR 2011"},
              "background_raw": "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1745363004"
            }
          }
        }"#;
        let outcome = parse_store_response(620, payload.as_bytes()).expect("interpretar ficha");
        let StoreMetadataOutcome::Found(metadata) = outcome else {
            panic!("la ficha debía estar disponible");
        };
        assert_eq!(metadata.developer.as_deref(), Some("Valve"));
        assert_eq!(metadata.genres, vec!["Acción"]);
        assert_eq!(metadata.achievements_total, Some(51));
        assert_eq!(metadata.release_date.as_deref(), Some("2011-04-18"));
        assert!(!metadata.is_free);
        assert!(!metadata.is_early_access);
        assert_eq!(
            metadata.hero_url.as_deref(),
            Some(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1745363004"
            )
        );
        assert_eq!(
            metadata.short_description.as_deref(),
            Some("Portal 2 Resuelve & coopera. ignorar()")
        );
        assert!(!metadata.short_description.unwrap().contains('<'));
    }

    #[test]
    fn detects_free_and_early_access_from_stable_store_fields() {
        let payload = br#"{
          "892970": {
            "success": true,
            "data": {
              "is_free": true,
              "genres": [
                {"id":"23","description":"Indie"},
                {"id":"70","description":"Acceso anticipado"}
              ],
              "release_date": {"coming_soon": false, "date": "2 FEB 2021"}
            }
          }
        }"#;
        let StoreMetadataOutcome::Found(metadata) =
            parse_store_response(892970, payload).expect("interpretar ficha")
        else {
            panic!("la ficha debía estar disponible");
        };
        assert!(metadata.is_free);
        assert!(metadata.is_early_access);
        assert_eq!(metadata.release_date.as_deref(), Some("2021-02-02"));
    }

    #[test]
    fn does_not_invent_a_release_date_for_unreleased_or_ambiguous_values() {
        for date in ["Próximamente", "Q1 2027", "2027"] {
            let payload = format!(
                r#"{{"10":{{"success":true,"data":{{"release_date":{{"coming_soon":false,"date":"{date}"}}}}}}}}"#
            );
            let StoreMetadataOutcome::Found(metadata) =
                parse_store_response(10, payload.as_bytes()).expect("interpretar ficha")
            else {
                panic!("la ficha debía estar disponible");
            };
            assert_eq!(metadata.release_date, None);
        }
    }

    #[test]
    fn returns_an_explicit_fallback_when_store_has_no_app() {
        let payload = r#"{"10":{"success":false}}"#;
        let outcome = parse_store_response(10, payload.as_bytes()).expect("interpretar ausencia");
        assert!(matches!(outcome, StoreMetadataOutcome::Unavailable));
    }

    #[test]
    fn normalizes_entities_and_caps_untrusted_text() {
        let oversized = format!("<b>{}</b>&#33;", "x".repeat(9_000));
        let clean = sanitize_store_text(&oversized);
        assert_eq!(clean.chars().count(), 8_000);
        assert!(!clean.contains('<'));
    }

    #[test]
    fn accepts_only_the_exact_store_hero_host_and_app_path() {
        let valid = "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=123";
        assert_eq!(sanitize_hero_url(valid, 620).as_deref(), Some(valid));
        assert!(
            sanitize_hero_url(
                "https://evil.example/images/storepagebackground/app/620?t=123",
                620
            )
            .is_none()
        );
        assert!(
            sanitize_hero_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/730?t=123",
                620
            )
            .is_none()
        );
        assert!(
            sanitize_hero_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/%2e%2e?t=123",
                620
            )
            .is_none()
        );
        assert!(
            sanitize_hero_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?next=https://evil.example",
                620
            )
            .is_none()
        );
    }
}
