//! Lectura de la lista de deseados de Steam.
//!
//! # Qué endpoint se usa y por qué
//!
//! Steam expone la lista de deseados en `IWishlistService/GetWishlist/v1`.
//! Frente al resto de la integración de Vindexa tiene tres particularidades que
//! condicionan todo este módulo:
//!
//! 1. **No pide clave Web API ni sesión iniciada.** Basta el SteamID64 en la
//!    URL. Por eso [`fetch`] no recibe ninguna credencial: importar deseados
//!    funciona aunque la persona todavía no haya guardado su clave en Ajustes.
//! 2. **No devuelve el nombre del juego.** Cada elemento trae únicamente
//!    `appid`, `priority` y `date_added`. Como Vindexa no puede inventar un
//!    título, los nombres se resuelven aparte contra
//!    `IStoreBrowseService/GetItems/v1`, que acepta lotes y tampoco pide clave.
//! 3. **Una lista vacía, una lista oculta y un SteamID inexistente devuelven
//!    exactamente la misma respuesta**: `{"response":{}}` con estado 200. Ese
//!    empate no se puede deshacer desde este endpoint, así que no se resuelve
//!    adivinando: ver [`fetch`] y [`SteamWishlistSnapshot::visibility_unknown`].
//!
//! El endpoint antiguo `store.steampowered.com/wishlist/profiles/<id>/wishlistdata/`
//! ya no sirve: hoy redirige a la portada de la tienda y responde HTML, no JSON.
//! No se contempla como alternativa.
//!
//! # Límite de peticiones
//!
//! Se replica la política que ya usan [`super::dlc`] y [`super::store_api`]:
//! cliente HTTP único, espera mínima entre peticiones, tope de cuerpo por
//! respuesta y traducción de `429` a un error propio con su `Retry-After`. La
//! lista de deseados más larga que se ha observado ronda los dos megabytes, de
//! ahí el tope de este módulo.

use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};

const WISHLIST_ENDPOINT: &str = "https://api.steampowered.com/IWishlistService/GetWishlist/v1/";
pub(super) const STORE_ITEMS_ENDPOINT: &str =
    "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/";

/// Tope de cuerpo por respuesta. Una lista de deseados de varios miles de
/// juegos ronda los dos megabytes; el doble deja margen sin abrir la puerta a
/// una respuesta desmedida.
const MAX_WISHLIST_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
/// Espera mínima entre peticiones de este módulo.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(900);
/// AppIDs por petición al resolver nombres. Doscientos entran de sobra en una
/// sola respuesta y reducen a cinco las peticiones de una lista de mil juegos.
const STORE_ITEMS_BATCH: usize = 200;
/// `communityvisibilitystate` de un perfil público en `GetPlayerSummaries`.
const PUBLIC_VISIBILITY: u8 = 3;

/// Un juego de la lista de deseados de Steam, ya con su nombre resuelto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamWishlistItem {
    pub app_id: u32,
    /// Prioridad **de Steam** dentro de su propia lista. `0` significa «sin
    /// ordenar». No es la prioridad de Vindexa y no se traduce a ella.
    pub priority: u32,
    /// Momento en el que se añadió a la lista de Steam, en RFC 3339.
    pub added_at: Option<String>,
    /// Nombre publicado por la tienda. `None` cuando la tienda no lo devuelve;
    /// nunca se sustituye por un marcador.
    pub title: Option<String>,
    /// La tienda declara el juego como no visible (retirado, restringido por
    /// país o sin ficha pública).
    pub hidden_in_store: bool,
}

/// Lo que devuelve una lectura completa de la lista de deseados.
#[derive(Debug, Clone)]
pub struct SteamWishlistSnapshot {
    pub steam_id: String,
    pub items: Vec<SteamWishlistItem>,
    /// Juegos cuyo nombre sí pudo resolverse en la tienda.
    pub titles_resolved: usize,
    /// Juegos que se quedaron sin nombre. Se cuentan aparte porque sin título
    /// no se puede crear una ficha honesta.
    pub titles_unresolved: usize,
    /// La lista llegó vacía y no hay forma de distinguir si está vacía de
    /// verdad o si Steam la está ocultando. Quien presente el resultado debe
    /// decir las dos posibilidades, no elegir una.
    pub visibility_unknown: bool,
}

#[derive(Debug, Deserialize)]
struct WishlistEnvelope {
    response: WishlistResponse,
}

#[derive(Debug, Default, Deserialize)]
struct WishlistResponse {
    #[serde(default)]
    items: Option<Vec<WishlistItemPayload>>,
}

#[derive(Debug, Deserialize)]
struct WishlistItemPayload {
    appid: u32,
    #[serde(default)]
    priority: u32,
    #[serde(default)]
    date_added: i64,
}

#[derive(Debug, Deserialize)]
struct StoreItemsEnvelope {
    response: StoreItemsResponse,
}

#[derive(Debug, Default, Deserialize)]
struct StoreItemsResponse {
    #[serde(default)]
    store_items: Vec<StoreItemPayload>,
}

#[derive(Debug, Deserialize)]
struct StoreItemPayload {
    #[serde(default)]
    appid: Option<u32>,
    #[serde(default)]
    id: Option<u32>,
    #[serde(default)]
    success: u32,
    #[serde(default)]
    visible: bool,
    #[serde(default)]
    name: Option<String>,
}

/// Lee la lista de deseados de un SteamID64.
///
/// `known_visibility` es el `communityvisibilitystate` que la sincronización de
/// biblioteca ya guardó para esa cuenta, si existe. Sólo se usa para desempatar
/// la respuesta vacía: con un perfil demostradamente no público, la lista vacía
/// se convierte en el error `steam_wishlist_private`; sin esa prueba se informa
/// una lista de cero juegos con [`SteamWishlistSnapshot::visibility_unknown`] a
/// `true`, porque afirmar que el perfil es privado sería inventarlo.
pub async fn fetch(
    steam_id: &str,
    known_visibility: Option<u8>,
) -> AppResult<SteamWishlistSnapshot> {
    super::web_api::validate_steam_id(steam_id)?;
    let client = wishlist_client()?;

    let bytes = get_bytes(client, WISHLIST_ENDPOINT, &[("steamid", steam_id.to_owned())]).await?;
    let mut items = parse_wishlist(&bytes)?;

    if items.is_empty() {
        if known_visibility.is_some_and(|visibility| visibility != PUBLIC_VISIBILITY) {
            return Err(private_wishlist_error());
        }
        // La visibilidad guardada procede de `GetPlayerSummaries`, que consulta
        // con la clave de la propia cuenta y por eso puede informar «público»
        // de un perfil que para un tercero no lo es. El perfil publica su
        // estado real en XML y sin credenciales, así que se pregunta ahí antes
        // de rendirse a la ambigüedad.
        if let Some(estado) = profile_privacy_state(client, steam_id).await
            && estado != PUBLIC_PRIVACY_STATE
        {
            return Err(private_wishlist_error());
        }
        return Ok(SteamWishlistSnapshot {
            steam_id: steam_id.to_owned(),
            items,
            titles_resolved: 0,
            titles_unresolved: 0,
            visibility_unknown: true,
        });
    }

    resolve_titles(client, &mut items).await?;
    let titles_resolved = items.iter().filter(|item| item.title.is_some()).count();

    Ok(SteamWishlistSnapshot {
        steam_id: steam_id.to_owned(),
        titles_unresolved: items.len() - titles_resolved,
        titles_resolved,
        items,
        visibility_unknown: false,
    })
}

/// Resuelve el nombre de una tanda de juegos contra la tienda.
///
/// Existe para que la importación desde el navegador ([`super::wishlist_session`])
/// no abra un segundo canal contra el mismo servicio: comparte cliente, espera
/// mínima entre peticiones y tope de cuerpo con el resto de este módulo, que es
/// lo que hace que la política de peticiones a Steam siga siendo una sola.
pub async fn resolve_store_titles(items: &mut [SteamWishlistItem]) -> AppResult<()> {
    resolve_titles(wishlist_client()?, items).await
}

/// Rellena el nombre de cada juego consultando la tienda por lotes.
///
/// Un lote que falle no cancela la importación: los juegos de ese lote se
/// quedan sin título y se contabilizan como tales. Sí se propaga el límite de
/// peticiones, porque seguir pidiendo sólo empeora la situación.
async fn resolve_titles(client: &Client, items: &mut [SteamWishlistItem]) -> AppResult<()> {
    for chunk in items.chunks_mut(STORE_ITEMS_BATCH) {
        let app_ids = chunk.iter().map(|item| item.app_id).collect::<Vec<_>>();
        let bytes = match get_bytes(
            client,
            STORE_ITEMS_ENDPOINT,
            &[("input_json", store_items_input_json(&app_ids))],
        )
        .await
        {
            Ok(bytes) => bytes,
            Err(error) if error.code == "steam_wishlist_rate_limited" => return Err(error),
            Err(_) => continue,
        };
        let Ok(resolved) = parse_store_items(&bytes) else {
            continue;
        };
        for item in chunk.iter_mut() {
            let Some(found) = resolved
                .iter()
                .find(|candidate| candidate.app_id == item.app_id)
            else {
                continue;
            };
            item.title.clone_from(&found.title);
            item.hidden_in_store = found.hidden_in_store;
        }
    }
    Ok(())
}

/// Petición de la tienda en el formato `input_json` que espera el servicio.
///
/// Se pide el mínimo: sin `include_assets` ni `include_release`, la respuesta
/// ya trae `name` y `visible`, que es lo único que aquí hace falta. El arte se
/// deriva del AppID con [`super::local::cover_url`] y no cuesta red.
fn store_items_input_json(app_ids: &[u32]) -> String {
    let ids = app_ids
        .iter()
        .map(|app_id| format!("{{\"appid\":{app_id}}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"ids\":[{ids}],\"context\":{{\"language\":\"spanish\",\"country_code\":\"ES\"}},\"data_request\":{{}}}}"
    )
}

fn parse_wishlist(bytes: &[u8]) -> AppResult<Vec<SteamWishlistItem>> {
    let envelope: WishlistEnvelope = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_wishlist_response",
            "Steam devolvió una lista de deseados que Vindexa no pudo interpretar.",
        )
    })?;
    let mut items = envelope
        .response
        .items
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item.appid > 0)
        .map(|item| SteamWishlistItem {
            app_id: item.appid,
            priority: item.priority,
            added_at: unix_to_rfc3339(item.date_added),
            title: None,
            hidden_in_store: false,
        })
        .collect::<Vec<_>>();
    // Steam repite el mismo AppID en listas antiguas migradas entre cuentas.
    // Ordenar por antigüedad y quedarse con la primera aparición mantiene la
    // cronología real y hace la importación reproducible.
    items.sort_by(|left, right| {
        left.added_at
            .cmp(&right.added_at)
            .then(left.app_id.cmp(&right.app_id))
    });
    items.dedup_by_key(|item| item.app_id);
    Ok(items)
}

#[derive(Debug)]
struct ResolvedStoreItem {
    app_id: u32,
    title: Option<String>,
    hidden_in_store: bool,
}

fn parse_store_items(bytes: &[u8]) -> AppResult<Vec<ResolvedStoreItem>> {
    let envelope: StoreItemsEnvelope = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_wishlist_store_response",
            "La tienda de Steam devolvió una respuesta que Vindexa no pudo interpretar.",
        )
    })?;
    Ok(envelope
        .response
        .store_items
        .into_iter()
        .filter_map(|item| {
            let app_id = item.appid.or(item.id).filter(|app_id| *app_id > 0)?;
            // `success` distinto de 1 significa que la tienda no resolvió la
            // ficha; el `name` que acompaña a ese caso viene vacío.
            let title = if item.success == 1 {
                item.name
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
            } else {
                None
            };
            Some(ResolvedStoreItem {
                app_id,
                hidden_in_store: !item.visible,
                title,
            })
        })
        .collect())
}

fn unix_to_rfc3339(value: i64) -> Option<String> {
    if value <= 0 {
        return None;
    }
    DateTime::<Utc>::from_timestamp(value, 0).map(|instant| instant.to_rfc3339())
}

/// Valor de `privacyState` que publica un perfil abierto a cualquiera.
const PUBLIC_PRIVACY_STATE: &str = "public";

/// Estado de privacidad que el propio perfil publica en XML.
///
/// Es la única fuente que distingue «lista vacía» de «lista escondida» sin
/// credenciales: el servicio de deseados devuelve lo mismo en ambos casos. Ante
/// cualquier problema —red, formato inesperado, perfil sin XML— devuelve `None`
/// y quien llama conserva la ambigüedad en vez de afirmar algo que no sabe.
async fn profile_privacy_state(client: &Client, steam_id: &str) -> Option<String> {
    let url = format!("https://steamcommunity.com/profiles/{steam_id}/?xml=1");
    let response = client.get(&url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body = response.text().await.ok()?;
    let inicio = body.find("<privacyState>")? + "<privacyState>".len();
    let fin = body[inicio..].find("</privacyState>")? + inicio;
    Some(body[inicio..fin].trim().to_ascii_lowercase())
}

fn private_wishlist_error() -> AppError {
    AppError::new(
        "steam_wishlist_private",
        "Tu perfil de Steam no es público, así que Steam no deja leer la lista de deseados. Ponlo en público —y también «Detalles del juego»— y vuelve a intentarlo.",
    )
}

pub(super) fn wishlist_client() -> AppResult<&'static Client> {
    static CLIENT: OnceLock<Result<Client, AppError>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .redirect(Policy::none())
            .user_agent("Vindexa/0.1 (+https://vindexa.app)")
            .build()
            .map_err(|_| {
                AppError::new(
                    "steam_wishlist_http_client",
                    "No se pudo preparar la conexión segura con Steam.",
                )
            })
    }) {
        Ok(client) => Ok(client),
        Err(error) => Err(error.clone()),
    }
}

/// Serializa las peticiones de este módulo y garantiza la espera mínima.
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

pub(super) async fn get_bytes(
    client: &Client,
    endpoint: &str,
    query: &[(&str, String)],
) -> AppResult<Vec<u8>> {
    throttle().await;
    let mut response = client
        .get(endpoint)
        .query(query)
        .send()
        .await
        .map_err(classify_request_error)?;
    let status = response.status();
    if status.is_redirection() {
        return Err(AppError::new(
            "steam_wishlist_redirect",
            "Steam redirigió inesperadamente la solicitud de la lista de deseados.",
        ));
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(rate_limited_error(parse_retry_after(
            response.headers().get(header::RETRY_AFTER),
        )));
    }
    if status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND {
        return Err(AppError::validation(
            "Steam no reconoce ese SteamID64. Revísalo en Ajustes y vuelve a intentarlo.",
        ));
    }
    if status.is_server_error() {
        return Err(AppError::new(
            "steam_wishlist_unavailable",
            "Steam no está disponible temporalmente. Vuelve a intentarlo más tarde.",
        ));
    }
    if !status.is_success() {
        return Err(AppError::new(
            "steam_wishlist_status",
            format!("Steam respondió con el estado {status}."),
        ));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_owned);
    if !content_type.is_some_and(|value| value.eq_ignore_ascii_case("application/json")) {
        return Err(AppError::new(
            "steam_wishlist_content_type",
            "Steam devolvió un tipo de contenido inesperado al pedir la lista de deseados.",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_WISHLIST_RESPONSE_BYTES as u64)
    {
        return Err(too_large_error());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify_request_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_WISHLIST_RESPONSE_BYTES {
            return Err(too_large_error());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn too_large_error() -> AppError {
    AppError::new(
        "steam_wishlist_too_large",
        "La lista de deseados de Steam supera el tamaño máximo que Vindexa admite.",
    )
}

fn rate_limited_error(retry_after: Option<Duration>) -> AppError {
    let message = match retry_after {
        Some(delay) => format!(
            "Steam ha limitado temporalmente las peticiones. Vuelve a intentarlo dentro de {} segundos.",
            delay.as_secs()
        ),
        None => "Steam ha limitado temporalmente las peticiones. Espera unos minutos antes de volver a importar la lista de deseados.".to_string(),
    };
    AppError::new("steam_wishlist_rate_limited", message)
}

fn parse_retry_after(value: Option<&header::HeaderValue>) -> Option<Duration> {
    let seconds = value?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(Duration::from_secs(seconds.clamp(1, 3_600)))
}

fn classify_request_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "steam_wishlist_timeout",
            "Steam no respondió a tiempo al pedir la lista de deseados. Vuelve a intentarlo.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "steam_wishlist_connection",
            "No se pudo conectar con Steam. Comprueba la conexión a Internet y vuelve a intentarlo.",
        );
    }
    AppError::new(
        "steam_wishlist_network",
        "No se pudo leer la lista de deseados de Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        PUBLIC_VISIBILITY, parse_retry_after, parse_store_items, parse_wishlist, private_wishlist_error,
        rate_limited_error, store_items_input_json, unix_to_rfc3339,
    };
    use reqwest::header::HeaderValue;
    use std::time::Duration;

    /// Respuesta real del endpoint, recortada a tres juegos.
    const WISHLIST_OK: &str = r#"{"response":{"items":[
        {"appid":226620,"priority":0,"date_added":1708728972},
        {"appid":202350,"priority":3,"date_added":1403384867},
        {"appid":264040,"priority":4,"date_added":1387215270}
    ]}}"#;

    /// Lo que Steam devuelve para una lista vacía, una lista oculta y un
    /// SteamID que no existe: las tres cosas son indistinguibles.
    const WISHLIST_EMPTY: &str = r#"{"response":{}}"#;

    const STORE_ITEMS_OK: &str = r#"{"response":{"store_items":[
        {"item_type":0,"id":226620,"success":1,"visible":true,"name":"Deadlight","appid":226620},
        {"item_type":0,"id":202350,"success":1,"visible":true,"name":"Steam Mobile Access","appid":202350},
        {"item_type":0,"id":264040,"success":15,"visible":false,"name":"","appid":264040}
    ]}}"#;

    #[test]
    fn reads_a_valid_wishlist_ordered_by_date_added() {
        let items = parse_wishlist(WISHLIST_OK.as_bytes()).expect("leer lista");

        assert_eq!(
            items.iter().map(|item| item.app_id).collect::<Vec<_>>(),
            [264040, 202350, 226620]
        );
        assert_eq!(items[0].priority, 4);
        assert_eq!(
            items[0].added_at.as_deref(),
            Some("2013-12-16T17:34:30+00:00")
        );
        assert!(items.iter().all(|item| item.title.is_none()));
    }

    #[test]
    fn an_empty_response_yields_no_items_and_never_an_invented_reason() {
        let items = parse_wishlist(WISHLIST_EMPTY.as_bytes()).expect("leer lista vacía");
        assert!(items.is_empty());

        // El desempate sólo existe cuando hay una visibilidad guardada que lo
        // demuestre; el propio endpoint no distingue vacía de oculta.
        assert_ne!(PUBLIC_VISIBILITY, 1);
        assert_eq!(private_wishlist_error().code, "steam_wishlist_private");
    }

    #[test]
    fn a_corrupt_response_fails_with_its_own_code() {
        let error =
            parse_wishlist(b"{\"response\":{\"items\":[{\"appid\":").expect_err("rechazar JSON roto");
        assert_eq!(error.code, "steam_wishlist_response");

        let store = parse_store_items(b"no es json").expect_err("rechazar tienda ilegible");
        assert_eq!(store.code, "steam_wishlist_store_response");
    }

    #[test]
    fn store_items_resolve_titles_and_flag_hidden_games() {
        let resolved = parse_store_items(STORE_ITEMS_OK.as_bytes()).expect("leer tienda");

        let deadlight = resolved
            .iter()
            .find(|item| item.app_id == 226620)
            .expect("juego resuelto");
        assert_eq!(deadlight.title.as_deref(), Some("Deadlight"));
        assert!(!deadlight.hidden_in_store);

        let hidden = resolved
            .iter()
            .find(|item| item.app_id == 264040)
            .expect("juego oculto");
        assert!(hidden.title.is_none());
        assert!(hidden.hidden_in_store);
    }

    #[test]
    fn duplicated_app_ids_collapse_into_the_oldest_entry() {
        let repeated = r#"{"response":{"items":[
            {"appid":70,"priority":2,"date_added":1500000000},
            {"appid":70,"priority":9,"date_added":1400000000}
        ]}}"#;
        let items = parse_wishlist(repeated.as_bytes()).expect("leer lista repetida");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].priority, 9);
    }

    #[test]
    fn items_without_a_usable_app_id_or_date_are_dropped_or_left_empty() {
        let odd = r#"{"response":{"items":[
            {"appid":0,"priority":1,"date_added":1400000000},
            {"appid":70,"priority":1,"date_added":0}
        ]}}"#;
        let items = parse_wishlist(odd.as_bytes()).expect("leer lista atípica");

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].app_id, 70);
        assert!(items[0].added_at.is_none());
        assert!(unix_to_rfc3339(-1).is_none());
    }

    #[test]
    fn the_store_request_asks_only_for_what_it_needs() {
        let body = store_items_input_json(&[10, 20]);

        assert!(body.contains("{\"appid\":10}"));
        assert!(body.contains("{\"appid\":20}"));
        assert!(!body.contains("include_assets"));
        assert!(!body.contains("key"));
    }

    #[test]
    fn the_rate_limit_message_uses_the_hint_when_steam_sends_one() {
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("45"))),
            Some(Duration::from_secs(45))
        );
        // Un `0` no autoriza a reintentar de inmediato: se eleva al mínimo.
        assert_eq!(
            parse_retry_after(Some(&HeaderValue::from_static("0"))),
            Some(Duration::from_secs(1))
        );
        assert_eq!(parse_retry_after(Some(&HeaderValue::from_static("ya"))), None);

        let hinted = rate_limited_error(Some(Duration::from_secs(45)));
        assert_eq!(hinted.code, "steam_wishlist_rate_limited");
        assert!(hinted.message.contains("45 segundos"));
        assert!(
            rate_limited_error(None)
                .message
                .contains("unos minutos")
        );
    }
}
