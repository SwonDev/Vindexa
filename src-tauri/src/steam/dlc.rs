//! Obtención de los DLC de un juego de la biblioteca.
//!
//! Este módulo es deliberadamente autónomo: construye su propio cliente HTTP y
//! su propio analizador de respuestas para no acoplarse a `store_api.rs`. La
//! disciplina de red (timeouts, `User-Agent`, sin redirecciones, tope de bytes,
//! `429` con `Retry-After` y espera mínima entre peticiones) se replica aquí de
//! forma explícita.
//!
//! ## Procedencia de los datos
//!
//! - La lista de DLC de un juego procede de `data.dlc[]` de la ficha pública
//!   `store.steampowered.com/api/appdetails` del **juego base**.
//! - Cada DLC se describe consultando su propia ficha. Solo se persiste si el
//!   vínculo con el juego base es verificable: o bien venía en la lista `dlc[]`
//!   del juego base (procedencia por construcción), o bien su `fullgame.appid`
//!   coincide con el juego base. Si `fullgame.appid` nombra un juego distinto,
//!   la ficha se descarta: Steam se estaría contradiciendo y Vindexa no inventa
//!   relaciones.
//!
//! ## Propiedad (`owned`) de un DLC
//!
//! Steam **no** publica los DLC poseídos: `IPlayerService/GetOwnedGames` solo
//! devuelve juegos. La única evidencia local verificable es el manifiesto
//! `appmanifest_<appid>.acf` del juego base, cuya sección `InstalledDepots`
//! marca con la clave `dlcappid` los depots que pertenecen a un DLC concreto.
//! Que un depot de DLC esté instalado demuestra que Steam concedió la licencia,
//! así que se acepta como prueba de propiedad *y* de instalación.
//!
//! La ausencia de esa evidencia **no** demuestra lo contrario: hay DLC sin
//! depots propios (contenido desbloqueable, cosméticos incluidos en el juego
//! base, licencias no descargadas). Por eso, cuando no hay evidencia, `owned`
//! se deja en `0` y el motivo queda registrado en [`LocalDlcEvidence`]; nunca se
//! adivina. El marcado manual de la persona usuaria siempre prevalece y jamás lo
//! revierte una actualización desde la tienda.

use crate::db::dlc::{ImportedDlc, MAX_DLC_PER_GAME};
use crate::error::{AppError, AppResult};
use chrono::NaiveDate;
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

const STORE_DETAILS_ENDPOINT: &str = "https://store.steampowered.com/api/appdetails";
/// Tope de cuerpo por respuesta. La ficha de un juego con muchos DLC es la
/// respuesta más grande que se espera de este endpoint.
const MAX_DLC_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// Espera mínima entre peticiones a la tienda para no martillearla.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(900);
const MAX_TITLE_CHARS: usize = 200;
const MAX_DESCRIPTION_CHARS: usize = 1_000;
/// 100 000,00 en la unidad menor de la moneda. Un precio mayor se considera
/// una respuesta corrupta y se descarta en vez de guardarse.
const MAX_PRICE_CENTS: i64 = 10_000_000;

/// Catálogo de DLC declarado por la ficha del juego base.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DlcCatalog {
    pub app_id: u32,
    /// Cuántos AppID de DLC declaró Steam antes de aplicar el techo local.
    pub declared: usize,
    /// `true` si Steam declaró más DLC que [`MAX_DLC_PER_GAME`].
    pub truncated: bool,
    pub items: Vec<ImportedDlc>,
}

/// Resultado de consultar la ficha individual de un DLC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DlcDetailOutcome {
    Found(Box<ImportedDlc>),
    /// Steam respondió correctamente pero no publica ficha para ese AppID.
    Unavailable,
}

/// Error de red o de contrato con la pista de reintento que Steam haya dado.
#[derive(Debug)]
pub struct DlcFetchFailure {
    pub error: AppError,
    pub retry_after: Option<Duration>,
}

impl From<AppError> for DlcFetchFailure {
    fn from(error: AppError) -> Self {
        Self {
            error,
            retry_after: None,
        }
    }
}

/// Motivo por el que no hay evidencia local sobre los DLC de un juego.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalDlcEvidenceGap {
    SteamNotInstalled,
    LibrariesUnreadable,
    GameNotInstalled,
    InvalidAppId,
}

impl LocalDlcEvidenceGap {
    /// Código estable, apto para telemetría local y para la interfaz.
    pub fn code(self) -> &'static str {
        match self {
            Self::SteamNotInstalled => "dlc_evidence_steam_not_installed",
            Self::LibrariesUnreadable => "dlc_evidence_libraries_unreadable",
            Self::GameNotInstalled => "dlc_evidence_game_not_installed",
            Self::InvalidAppId => "dlc_evidence_invalid_app_id",
        }
    }

    /// Explicación en español, sin rutas locales ni datos personales.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::SteamNotInstalled => {
                "No se encontró una instalación local de Steam, así que Vindexa no puede \
                 comprobar qué DLC posees."
            }
            Self::LibrariesUnreadable => {
                "Steam está instalado, pero no se pudo leer ninguna de sus bibliotecas para \
                 comprobar los DLC instalados."
            }
            Self::GameNotInstalled => {
                "El juego no está instalado en este equipo, así que no hay manifiesto local que \
                 demuestre qué DLC posees."
            }
            Self::InvalidAppId => "El AppID de Steam no es válido.",
        }
    }
}

/// Evidencia local sobre los DLC de un juego.
///
/// `Manifest` solo se emite cuando se ha leído de verdad el manifiesto del juego
/// base. Un conjunto vacío significa «leído y sin depots de DLC instalados», que
/// es distinto de `Unavailable` («no se pudo comprobar»).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalDlcEvidence {
    Manifest {
        installed_dlc_app_ids: BTreeSet<u32>,
    },
    Unavailable {
        gap: LocalDlcEvidenceGap,
    },
}

impl LocalDlcEvidence {
    #[allow(dead_code, reason = "en producción se pregunta por gap_code")]
    pub fn is_conclusive(&self) -> bool {
        matches!(self, Self::Manifest { .. })
    }

    /// Código del motivo cuando no hay evidencia; `None` si sí la hay.
    pub fn gap_code(&self) -> Option<&'static str> {
        match self {
            Self::Manifest { .. } => None,
            Self::Unavailable { gap } => Some(gap.code()),
        }
    }

    /// Explicación del motivo cuando no hay evidencia; `None` si sí la hay.
    pub fn gap_explanation(&self) -> Option<&'static str> {
        match self {
            Self::Manifest { .. } => None,
            Self::Unavailable { gap } => Some(gap.explanation()),
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
    name: Option<String>,
    #[serde(default)]
    is_free: bool,
    short_description: Option<String>,
    header_image: Option<String>,
    capsule_image: Option<String>,
    release_date: Option<StoreReleaseDate>,
    price_overview: Option<StorePriceOverview>,
    fullgame: Option<StoreFullGame>,
    #[serde(default)]
    dlc: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct StoreReleaseDate {
    #[serde(default)]
    coming_soon: bool,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorePriceOverview {
    currency: Option<String>,
    #[serde(rename = "final")]
    final_price: Option<i64>,
    discount_percent: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct StoreFullGame {
    appid: Option<serde_json::Value>,
}

/// Lee la lista de DLC declarada por la ficha del juego base.
///
/// Los elementos vuelven con `metadata_status = "pending"`: en esta pasada solo
/// se conocen el AppID y su posición dentro del catálogo oficial. La descripción
/// de cada DLC llega después con [`fetch_detail`], normalmente a través de la
/// cola de refresco.
pub async fn fetch_catalog(app_id: u32) -> Result<DlcCatalog, DlcFetchFailure> {
    if app_id == 0 {
        return Err(AppError::validation("El AppID de Steam no es válido.").into());
    }
    let bytes = request_app_details(app_id).await?;
    parse_catalog(app_id, &bytes).map_err(DlcFetchFailure::from)
}

/// Lee la ficha individual de un DLC ya vinculado al juego base.
pub async fn fetch_detail(
    app_id: u32,
    dlc_app_id: u32,
    position: u32,
) -> Result<DlcDetailOutcome, DlcFetchFailure> {
    if app_id == 0 || dlc_app_id == 0 || app_id == dlc_app_id {
        return Err(AppError::validation("El AppID del DLC no es válido.").into());
    }
    let bytes = request_app_details(dlc_app_id).await?;
    parse_detail(app_id, dlc_app_id, position, &bytes).map_err(DlcFetchFailure::from)
}

/// Comprueba en el manifiesto local del juego base qué DLC están instalados.
///
/// Es una operación de sistema de archivos síncrona: llámala desde un contexto
/// bloqueante, no desde el hilo del runtime asíncrono.
pub fn scan_installed_dlc(app_id: u32) -> LocalDlcEvidence {
    if app_id == 0 {
        return LocalDlcEvidence::Unavailable {
            gap: LocalDlcEvidenceGap::InvalidAppId,
        };
    }
    let Ok(steam) = steamlocate::locate() else {
        return LocalDlcEvidence::Unavailable {
            gap: LocalDlcEvidenceGap::SteamNotInstalled,
        };
    };
    match steam.find_app(app_id) {
        Ok(Some((app, _library))) => LocalDlcEvidence::Manifest {
            installed_dlc_app_ids: installed_dlc_from_depots(
                app.installed_depots.values().map(|depot| depot.dlc_app_id),
            ),
        },
        Ok(None) => LocalDlcEvidence::Unavailable {
            gap: LocalDlcEvidenceGap::GameNotInstalled,
        },
        Err(_) => LocalDlcEvidence::Unavailable {
            gap: LocalDlcEvidenceGap::LibrariesUnreadable,
        },
    }
}

/// Aplica la evidencia local a un lote antes de persistirlo.
///
/// - Con evidencia (`Manifest`): un DLC con depot instalado se marca poseído e
///   instalado; el resto queda con `installed = Some(false)` porque el
///   manifiesto sí es concluyente sobre la instalación.
/// - Sin evidencia (`Unavailable`): no se afirma nada. `owned` queda en `false`
///   e `installed` en `None`, que la capa de persistencia interpreta como «no
///   toques lo que ya había».
pub fn apply_local_evidence(items: &mut [ImportedDlc], evidence: &LocalDlcEvidence) {
    match evidence {
        LocalDlcEvidence::Manifest {
            installed_dlc_app_ids,
        } => {
            for item in items {
                let installed = installed_dlc_app_ids.contains(&item.dlc_app_id);
                item.installed = Some(installed);
                item.owned = item.owned || installed;
            }
        }
        LocalDlcEvidence::Unavailable { .. } => {
            for item in items {
                item.installed = None;
            }
        }
    }
}

/// Política de reintento para la cola de DLC.
///
/// Solo se reintentan los fallos transitorios. Un error de contrato (respuesta
/// ilegible, vínculo incoherente) no mejora repitiéndolo.
pub fn retry_delay_seconds(
    error_code: &str,
    attempts: u32,
    retry_after: Option<Duration>,
) -> Option<u64> {
    let (base_seconds, max_attempts) = match error_code {
        "steam_dlc_rate_limited" => (60_u64, 6_u32),
        "steam_dlc_timeout" | "steam_dlc_connection" | "steam_dlc_network" => (15_u64, 4_u32),
        _ => return None,
    };
    if attempts >= max_attempts {
        return None;
    }
    let exponential = base_seconds.saturating_mul(2_u64.saturating_pow(attempts.saturating_sub(1)));
    let hinted = retry_after.map(|value| value.as_secs()).unwrap_or(0);
    Some(exponential.max(hinted).clamp(1, 3_600))
}

fn dlc_client() -> AppResult<&'static Client> {
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
                    "steam_dlc_http_client",
                    "No se pudo preparar la conexión segura con la tienda de Steam.",
                )
            })
    }) {
        Ok(client) => Ok(client),
        Err(error) => Err(error.clone()),
    }
}

/// Serializa las peticiones de DLC y garantiza la espera mínima entre ellas.
async fn throttle() {
    static NEXT_REQUEST_AT: Mutex<Option<Instant>> = Mutex::const_new(None);
    let mut next_request_at = NEXT_REQUEST_AT.lock().await;
    if let Some(deadline) = *next_request_at
        && deadline > Instant::now()
    {
        sleep_until(deadline).await;
    }
    *next_request_at = Some(Instant::now() + MIN_REQUEST_INTERVAL);
}

async fn request_app_details(app_id: u32) -> Result<Vec<u8>, DlcFetchFailure> {
    let client = dlc_client().map_err(DlcFetchFailure::from)?;
    throttle().await;
    let mut response = client
        .get(STORE_DETAILS_ENDPOINT)
        .query(&[
            ("appids", app_id.to_string()),
            ("l", "spanish".to_string()),
            ("cc", "ES".to_string()),
        ])
        .send()
        .await
        .map_err(|error| DlcFetchFailure::from(classify_dlc_error(error)))?;
    let status = response.status();
    if status.is_redirection() {
        return Err(AppError::new(
            "steam_dlc_redirect",
            "La tienda de Steam redirigió inesperadamente la ficha del contenido adicional.",
        )
        .into());
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(DlcFetchFailure {
            error: AppError::new(
                "steam_dlc_rate_limited",
                "Steam ha limitado temporalmente la carga de contenido adicional.",
            ),
            retry_after: parse_retry_after(response.headers().get(header::RETRY_AFTER)),
        });
    }
    if !status.is_success() {
        return Err(AppError::new(
            "steam_dlc_status",
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
            "steam_dlc_content_type",
            "La tienda de Steam devolvió un tipo de contenido inesperado.",
        )
        .into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DLC_RESPONSE_BYTES as u64)
    {
        return Err(too_large_error().into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| DlcFetchFailure::from(classify_dlc_error(error)))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_DLC_RESPONSE_BYTES {
            return Err(too_large_error().into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn too_large_error() -> AppError {
    AppError::new(
        "steam_dlc_too_large",
        "La ficha de Steam supera el tamaño máximo permitido.",
    )
}

fn parse_retry_after(value: Option<&header::HeaderValue>) -> Option<Duration> {
    let seconds = value?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.clamp(1, 3_600)))
}

fn envelope_for(app_id: u32, bytes: &[u8]) -> AppResult<Option<StoreData>> {
    let mut envelopes: HashMap<String, StoreEnvelope> =
        serde_json::from_slice(bytes).map_err(|_| {
            AppError::new(
                "steam_dlc_response",
                "Steam devolvió una ficha que Vindexa no pudo interpretar.",
            )
        })?;
    let Some(envelope) = envelopes.remove(&app_id.to_string()) else {
        return Ok(None);
    };
    if !envelope.success {
        return Ok(None);
    }
    Ok(envelope.data)
}

fn parse_catalog(app_id: u32, bytes: &[u8]) -> AppResult<DlcCatalog> {
    let Some(data) = envelope_for(app_id, bytes)? else {
        return Ok(DlcCatalog {
            app_id,
            declared: 0,
            truncated: false,
            items: Vec::new(),
        });
    };
    let mut seen = BTreeSet::new();
    let declared_ids = data
        .dlc
        .iter()
        .filter_map(json_app_id)
        .filter(|dlc_app_id| *dlc_app_id != app_id)
        .filter(|dlc_app_id| seen.insert(*dlc_app_id))
        .collect::<Vec<_>>();
    let declared = declared_ids.len();
    let truncated = declared > MAX_DLC_PER_GAME;
    let items = declared_ids
        .into_iter()
        .take(MAX_DLC_PER_GAME)
        .enumerate()
        .map(|(index, dlc_app_id)| ImportedDlc::pending(dlc_app_id, index as u32))
        .collect();
    Ok(DlcCatalog {
        app_id,
        declared,
        truncated,
        items,
    })
}

fn parse_detail(
    app_id: u32,
    dlc_app_id: u32,
    position: u32,
    bytes: &[u8],
) -> AppResult<DlcDetailOutcome> {
    let Some(data) = envelope_for(dlc_app_id, bytes)? else {
        return Ok(DlcDetailOutcome::Unavailable);
    };
    // El vínculo ya está probado por procedencia (el AppID salió de `dlc[]` del
    // juego base). Si además Steam publica `fullgame`, debe coincidir; si nombra
    // otro juego, la ficha se rechaza en vez de inventar la relación.
    if let Some(full_game) = data
        .fullgame
        .as_ref()
        .and_then(|value| value.appid.as_ref())
        .and_then(json_app_id)
        && full_game != app_id
    {
        return Err(AppError::new(
            "steam_dlc_link_mismatch",
            "Steam vinculó ese contenido adicional con otro juego, así que Vindexa no lo guarda.",
        ));
    }

    let title = data
        .name
        .as_deref()
        .map(|value| sanitize_text(value, MAX_TITLE_CHARS))
        .filter(|value| !value.is_empty());
    // Sin nombre no hay nada que mostrar: se trata como ficha ausente en lugar
    // de inventar un título a partir del AppID.
    let Some(title) = title else {
        return Ok(DlcDetailOutcome::Unavailable);
    };

    let price = data.price_overview.as_ref().and_then(normalize_price);
    Ok(DlcDetailOutcome::Found(Box::new(ImportedDlc {
        dlc_app_id,
        title,
        capsule_url: data
            .capsule_image
            .as_deref()
            .and_then(|value| sanitize_store_image_url(value, dlc_app_id)),
        header_url: data
            .header_image
            .as_deref()
            .and_then(|value| sanitize_store_image_url(value, dlc_app_id)),
        short_description: data
            .short_description
            .as_deref()
            .map(|value| sanitize_text(value, MAX_DESCRIPTION_CHARS))
            .filter(|value| !value.is_empty()),
        release_date: data.release_date.as_ref().and_then(normalize_release_date),
        is_free: data.is_free,
        price_cents: price.as_ref().map(|price| price.cents),
        currency: price.as_ref().map(|price| price.currency.clone()),
        discount_percent: price.as_ref().and_then(|price| price.discount_percent),
        owned: false,
        installed: None,
        metadata_status: "success".to_string(),
        position,
    })))
}

struct NormalizedPrice {
    cents: u32,
    currency: String,
    discount_percent: Option<u8>,
}

fn normalize_price(value: &StorePriceOverview) -> Option<NormalizedPrice> {
    let currency = value.currency.as_deref()?.trim().to_uppercase();
    if currency.len() != 3 || !currency.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    let cents = value.final_price?;
    if !(0..=MAX_PRICE_CENTS).contains(&cents) {
        return None;
    }
    let discount_percent = value
        .discount_percent
        .filter(|value| (0..=100).contains(value))
        .map(|value| value as u8);
    Some(NormalizedPrice {
        cents: cents as u32,
        currency,
        discount_percent,
    })
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

fn json_app_id(value: &serde_json::Value) -> Option<u32> {
    match value {
        serde_json::Value::Number(number) => {
            number.as_u64().and_then(|value| u32::try_from(value).ok())
        }
        serde_json::Value::String(text) => text.trim().parse::<u32>().ok(),
        _ => None,
    }
    .filter(|app_id| *app_id > 0)
}

pub(crate) fn installed_dlc_from_depots(
    depots: impl Iterator<Item = Option<u64>>,
) -> BTreeSet<u32> {
    depots
        .flatten()
        .filter_map(|value| u32::try_from(value).ok())
        .filter(|dlc_app_id| *dlc_app_id > 0)
        .collect()
}

/// Cápsulas y cabeceras oficiales: HTTPS, host de CDN permitido y ruta que
/// contiene el AppID del propio DLC, con `?t=<dígitos>` opcional.
fn sanitize_store_image_url(value: &str, app_id: u32) -> Option<String> {
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

fn sanitize_text(value: &str, max_chars: usize) -> String {
    let mut without_tags = String::with_capacity(value.len().min(max_chars * 2));
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
    normalized.chars().take(max_chars).collect()
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

fn classify_dlc_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "steam_dlc_timeout",
            "La tienda de Steam no respondió a tiempo.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "steam_dlc_connection",
            "No se pudo conectar de forma segura con la tienda de Steam.",
        );
    }
    AppError::new(
        "steam_dlc_network",
        "No se pudo cargar el contenido adicional desde la tienda de Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        DlcDetailOutcome, LocalDlcEvidence, LocalDlcEvidenceGap, apply_local_evidence,
        installed_dlc_from_depots, parse_catalog, parse_detail, parse_retry_after,
        retry_delay_seconds, sanitize_store_image_url, sanitize_text,
    };
    use crate::db::dlc::{ImportedDlc, MAX_DLC_PER_GAME};
    use reqwest::header::HeaderValue;
    use std::collections::BTreeSet;
    use std::time::Duration;

    #[test]
    fn reads_the_official_dlc_list_of_the_base_game_without_duplicates() {
        let payload = br#"{
          "255710": {
            "success": true,
            "data": {
              "name": "Cities: Skylines",
              "dlc": [346791, "346791", 359051, 0, 255710, "no-es-un-id"]
            }
          }
        }"#;
        let catalog = parse_catalog(255710, payload).expect("interpretar catálogo");
        assert_eq!(catalog.app_id, 255710);
        assert_eq!(catalog.declared, 2);
        assert!(!catalog.truncated);
        assert_eq!(
            catalog
                .items
                .iter()
                .map(|item| (item.dlc_app_id, item.position))
                .collect::<Vec<_>>(),
            vec![(346791, 0), (359051, 1)]
        );
        assert!(
            catalog
                .items
                .iter()
                .all(|item| item.metadata_status == "pending")
        );
    }

    #[test]
    fn caps_the_catalog_and_reports_the_truncation_instead_of_hiding_it() {
        let ids = (1_000..1_000 + MAX_DLC_PER_GAME as u32 + 5)
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let payload = format!(r#"{{"10":{{"success":true,"data":{{"dlc":[{ids}]}}}}}}"#);
        let catalog = parse_catalog(10, payload.as_bytes()).expect("interpretar catálogo");
        assert_eq!(catalog.declared, MAX_DLC_PER_GAME + 5);
        assert!(catalog.truncated);
        assert_eq!(catalog.items.len(), MAX_DLC_PER_GAME);
    }

    #[test]
    fn a_game_without_dlc_is_an_empty_catalog_not_an_error() {
        let payload = br#"{"10":{"success":true,"data":{"name":"Sin DLC"}}}"#;
        let catalog = parse_catalog(10, payload).expect("interpretar catálogo");
        assert_eq!(catalog.declared, 0);
        assert!(catalog.items.is_empty());

        let missing = parse_catalog(10, br#"{"10":{"success":false}}"#).expect("ausencia");
        assert!(missing.items.is_empty());
    }

    #[test]
    fn parses_a_real_dlc_sheet_with_price_and_official_art() {
        let payload = r#"{
          "346791": {
            "success": true,
            "data": {
              "type": "dlc",
              "name": "Cities: Skylines - After Dark",
              "is_free": false,
              "fullgame": {"appid": "255710", "name": "Cities: Skylines"},
              "short_description": "<b>Amplía</b> tu ciudad &amp; la vida nocturna.",
              "header_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/346791/header.jpg?t=1745363004",
              "capsule_image": "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/346791/capsule_616x353.jpg",
              "release_date": {"coming_soon": false, "date": "24 SEP 2015"},
              "price_overview": {"currency": "EUR", "initial": 1299, "final": 649, "discount_percent": 50}
            }
          }
        }"#;
        let DlcDetailOutcome::Found(dlc) =
            parse_detail(255710, 346791, 3, payload.as_bytes()).expect("interpretar ficha")
        else {
            panic!("la ficha debía estar disponible");
        };
        assert_eq!(dlc.dlc_app_id, 346791);
        assert_eq!(dlc.title, "Cities: Skylines - After Dark");
        assert_eq!(dlc.position, 3);
        assert_eq!(dlc.metadata_status, "success");
        assert_eq!(dlc.price_cents, Some(649));
        assert_eq!(dlc.currency.as_deref(), Some("EUR"));
        assert_eq!(dlc.discount_percent, Some(50));
        assert_eq!(dlc.release_date.as_deref(), Some("2015-09-24"));
        assert_eq!(
            dlc.short_description.as_deref(),
            Some("Amplía tu ciudad & la vida nocturna.")
        );
        assert!(dlc.header_url.is_some());
        assert!(dlc.capsule_url.is_some());
        // La ficha de la tienda nunca demuestra propiedad por sí sola.
        assert!(!dlc.owned);
        assert_eq!(dlc.installed, None);
    }

    #[test]
    fn refuses_to_invent_a_link_when_steam_names_a_different_base_game() {
        let payload = br#"{
          "346791": {
            "success": true,
            "data": {"name": "DLC ajeno", "fullgame": {"appid": "730"}}
          }
        }"#;
        let error = parse_detail(255710, 346791, 0, payload).expect_err("rechazar vínculo ajeno");
        assert_eq!(error.code, "steam_dlc_link_mismatch");
        assert!(!error.message.contains("730"));
    }

    #[test]
    fn a_sheet_without_a_name_or_without_success_is_reported_as_unavailable() {
        for payload in [
            br#"{"346791":{"success":false}}"#.as_slice(),
            br#"{"346791":{"success":true,"data":{"fullgame":{"appid":255710}}}}"#.as_slice(),
            br#"{"346791":{"success":true,"data":{"name":"   "}}}"#.as_slice(),
        ] {
            assert_eq!(
                parse_detail(255710, 346791, 0, payload).expect("interpretar ausencia"),
                DlcDetailOutcome::Unavailable
            );
        }
    }

    #[test]
    fn discards_prices_that_do_not_look_like_a_real_store_price() {
        for price in [
            r#"{"currency":"€","final":649}"#,
            r#"{"currency":"EUR","final":-1}"#,
            r#"{"currency":"EUR","final":99999999}"#,
            r#"{"currency":"EUR"}"#,
        ] {
            let payload = format!(
                r#"{{"1":{{"success":true,"data":{{"name":"DLC","price_overview":{price}}}}}}}"#
            );
            let DlcDetailOutcome::Found(dlc) =
                parse_detail(2, 1, 0, payload.as_bytes()).expect("interpretar ficha")
            else {
                panic!("la ficha debía estar disponible");
            };
            assert_eq!(dlc.price_cents, None, "precio no verificable: {price}");
            assert_eq!(dlc.currency, None);
        }
    }

    #[test]
    fn accepts_only_official_art_hosts_scoped_to_the_dlc_app_id() {
        let valid =
            "https://shared.steamstatic.com/store_item_assets/steam/apps/346791/header.jpg?t=12";
        assert_eq!(
            sanitize_store_image_url(valid, 346791).as_deref(),
            Some(valid)
        );
        assert!(
            sanitize_store_image_url(
                "https://evil.example/store_item_assets/steam/apps/346791/header.jpg",
                346791
            )
            .is_none()
        );
        assert!(
            sanitize_store_image_url(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/730/header.jpg",
                346791
            )
            .is_none()
        );
        assert!(
            sanitize_store_image_url(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/346791/header.jpg?next=https://evil.example",
                346791
            )
            .is_none()
        );
    }

    #[test]
    fn local_manifest_depots_are_the_only_proof_of_dlc_ownership() {
        let depots = [Some(346791_u64), None, Some(359051), Some(0), None];
        let installed = installed_dlc_from_depots(depots.into_iter());
        assert_eq!(installed, BTreeSet::from([346791, 359051]));

        let mut items = vec![
            ImportedDlc::pending(346791, 0),
            ImportedDlc::pending(359051, 1),
            ImportedDlc::pending(400000, 2),
        ];
        apply_local_evidence(
            &mut items,
            &LocalDlcEvidence::Manifest {
                installed_dlc_app_ids: installed,
            },
        );
        assert_eq!(
            items
                .iter()
                .map(|item| (item.owned, item.installed))
                .collect::<Vec<_>>(),
            vec![(true, Some(true)), (true, Some(true)), (false, Some(false))]
        );
    }

    #[test]
    fn without_local_evidence_nothing_is_claimed_and_the_reason_is_explicit() {
        let mut items = vec![ImportedDlc::pending(346791, 0)];
        let evidence = LocalDlcEvidence::Unavailable {
            gap: LocalDlcEvidenceGap::GameNotInstalled,
        };
        apply_local_evidence(&mut items, &evidence);
        assert!(!items[0].owned);
        assert_eq!(items[0].installed, None);
        assert!(!evidence.is_conclusive());
        assert_eq!(evidence.gap_code(), Some("dlc_evidence_game_not_installed"));
        assert!(
            evidence
                .gap_explanation()
                .is_some_and(|value| value.contains("no está instalado"))
        );

        let conclusive = LocalDlcEvidence::Manifest {
            installed_dlc_app_ids: BTreeSet::new(),
        };
        assert!(conclusive.is_conclusive());
        assert_eq!(conclusive.gap_code(), None);
    }

    #[test]
    fn honors_bounded_retry_after_and_only_retries_transient_failures() {
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("90"))),
            Some(Duration::from_secs(90))
        );
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("999999"))),
            Some(Duration::from_secs(3_600))
        );
        assert_eq!(parse_retry_after(None), None);

        assert_eq!(
            retry_delay_seconds("steam_dlc_rate_limited", 1, Some(Duration::from_secs(90))),
            Some(90)
        );
        assert_eq!(retry_delay_seconds("steam_dlc_rate_limited", 6, None), None);
        assert_eq!(retry_delay_seconds("steam_dlc_timeout", 3, None), Some(60));
        assert_eq!(
            retry_delay_seconds("steam_dlc_link_mismatch", 1, None),
            None
        );
    }

    #[test]
    fn untrusted_store_text_is_stripped_and_capped() {
        let oversized = format!("<b>{}</b>&#33;", "x".repeat(9_000));
        let clean = sanitize_text(&oversized, 200);
        assert_eq!(clean.chars().count(), 200);
        assert!(!clean.contains('<'));
        assert_eq!(sanitize_text("<script>robar()</script>", 200), "robar()");
    }
}
