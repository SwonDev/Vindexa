//! Cliente nativo de Epic Games Store: inicio de sesión, refresco, revocación y
//! lectura de la biblioteca completa.
//!
//! # De dónde sale cada dato de este módulo
//!
//! Epic no publica una API para terceros ni emite credenciales de cliente. El
//! flujo que hay aquí es el que usa su propio lanzador, y está descrito en el
//! código abierto de Legendary (`legendary/api/egs.py`, rama `master`,
//! consultado el 18 de agosto de 2026):
//!
//! * Servidor de OAuth, servicio de lanzador, catálogo y biblioteca: las
//!   constantes `_oauth_host`, `_launcher_host`, `_catalog_host` y
//!   `_library_host` de esa clase.
//! * Tipos de concesión (`authorization_code`, `refresh_token`) y el
//!   `token_type=eg1`: el método `start_session`.
//! * Recorrido de la biblioteca: `get_game_assets` para lo que se posee y
//!   `get_game_info` para la ficha de cada juego.
//! * La página desde la que se obtiene el código: la redirección 302 de
//!   `https://legendary.gl/epiclogin`, comprobada el mismo día.
//!
//! Los criterios de filtrado —qué es un complemento, qué es un recurso de
//! Unreal Engine y qué imagen es la carátula— siguen a
//! `HeroicGamesLauncher/src/backend/storeManagers/legendary/library.ts` de la
//! rama `main`, consultado también el 18 de agosto de 2026.
//!
//! # El secreto del cliente oficial
//!
//! [`CLIENT_ID`] y [`CLIENT_SECRET`] son las credenciales del Epic Games
//! Launcher. Van compiladas como literales en el binario de Vindexa y
//! **cualquiera puede recuperarlas** con `strings`; no son un secreto de esta
//! aplicación ni se protegen como tal. Están publicadas desde hace años en
//! Legendary. Lo que sí es secreto es el testigo que se obtiene con ellas, y ése
//! vive en el llavero (ver `crate::stores::secrets`).

use crate::db::rich_metadata::DrmState;
use crate::error::{AppError, AppResult};
use crate::stores::launch::validate_epic_app_name;
use crate::stores::net;
use crate::stores::secrets::StoredSession;
use crate::stores::{
    DiscoveredGame, ExternalStore, MAX_DISCOVERED_GAMES, ScanSource, sanitize_https_url,
    sanitize_title,
};
use serde::Deserialize;
use std::collections::BTreeMap;

const STORE: ExternalStore = ExternalStore::Epic;

/// Credenciales del Epic Games Launcher. Ver el comentario del módulo: quedan
/// legibles en el binario y no se tratan como material sensible.
const CLIENT_ID: &str = "34a02cf8f4414e29b15921876da36f9a";
const CLIENT_SECRET: &str = "daafbccc737745039dffe53d94fc76cf";

const OAUTH_HOST: &str = "account-public-service-prod03.ol.epicgames.com";
const LAUNCHER_HOST: &str = "launcher-public-service-prod06.ol.epicgames.com";
const CATALOG_HOST: &str = "catalog-public-service-prod06.ol.epicgames.com";

/// El lanzador se identifica así ante los servicios de Epic. Enviar el mismo
/// agente evita que la petición se distinga de la del cliente oficial por un
/// detalle de cabecera.
const USER_AGENT: &str =
    "UELauncher/11.0.1-14907503+++Portal+Release-Live Windows/10.0.19041.1.256.64bit";

/// Etiqueta y plataforma con las que se pide el catálogo.
///
/// `Windows` no es un descuido en un programa de macOS: es la plataforma en la
/// que Epic publica su catálogo completo, y aquí se está listando **lo que se
/// posee**, no lo que se puede ejecutar en este equipo. Pedir `Mac` devolvería
/// un puñado de títulos y daría la impresión de una biblioteca incompleta.
const LABEL: &str = "Live";
const PLATFORM: &str = "Windows";

/// País y idioma con los que se pide la ficha del catálogo. Determinan el
/// idioma del título que se guarda.
const COUNTRY: &str = "ES";
const LOCALE: &str = "es-ES";

/// Cuántas fichas de catálogo se piden a la vez.
///
/// Epic obliga a una petición por juego para conocer su título, así que este
/// número decide lo que tarda una sincronización. Legendary usa un grupo de
/// dieciséis hilos; aquí se baja a ocho porque no hay motivo para acercarse al
/// límite de un servicio que no nos ha invitado.
const METADATA_CONCURRENCY: usize = 8;

/// Categoría con la que Epic marca los recursos que no son juegos.
const MODS_CATEGORY: &str = "mods";

/// Espacio de nombres del mercado de Unreal Engine. Sus recursos aparecen en la
/// lista de lo que se posee pero no son juegos de la biblioteca.
const UNREAL_NAMESPACE: &str = "ue";

/// Página que hay que abrir para obtener un código de autorización.
///
/// Es la que resuelve `https://legendary.gl/epiclogin`. Tras iniciar sesión,
/// Epic redirige a una página que muestra un JSON con el campo
/// `authorizationCode`.
pub fn login_url() -> String {
    format!(
        "https://www.epicgames.com/id/login?redirectUrl=https%3A//www.epicgames.com/id/api/redirect%3FclientId%3D{CLIENT_ID}%26responseType%3Dcode"
    )
}

// ---------------------------------------------------------------------------
// Respuestas de Epic
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    #[serde(default)]
    refresh_expires: Option<i64>,
    account_id: String,
    #[serde(rename = "displayName", default)]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AssetEntry {
    #[serde(rename = "appName", default)]
    app_name: String,
    #[serde(default)]
    namespace: String,
    #[serde(rename = "catalogItemId", default)]
    catalog_item_id: String,
}

#[derive(Debug, Deserialize)]
struct CatalogItem {
    #[serde(default)]
    title: String,
    #[serde(rename = "keyImages", default)]
    key_images: Vec<KeyImage>,
    #[serde(default)]
    categories: Vec<Category>,
    /// Presente sólo en los complementos: apunta al juego base. Es el mismo
    /// criterio que usa Legendary para decidir que algo es un DLC.
    #[serde(rename = "mainGameItem", default)]
    main_game_item: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct KeyImage {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct Category {
    #[serde(default)]
    path: String,
}

// ---------------------------------------------------------------------------
// Códigos de autorización
// ---------------------------------------------------------------------------

/// Saca el código de autorización de lo que la persona usuaria haya pegado.
///
/// La página de Epic muestra un JSON, así que pegar el bloque entero es lo
/// natural; pegar sólo el código también, y pegar la URL de la barra de
/// direcciones también. Se aceptan las tres formas en vez de exigir una y
/// devolver un error de formato a quien hizo lo razonable.
pub fn extract_authorization_code(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(missing_code());
    }

    if trimmed.starts_with('{') {
        let parsed: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|_| invalid_code())?;
        let code = parsed
            .get("authorizationCode")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(invalid_code)?;
        return validate_code(code);
    }

    if trimmed.starts_with("https://") {
        let parsed = url::Url::parse(trimmed).map_err(|_| invalid_code())?;
        let code = parsed
            .query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.into_owned())
            .ok_or_else(invalid_code)?;
        return validate_code(&code);
    }

    validate_code(trimmed.trim_matches(['"', '\'']))
}

fn validate_code(code: &str) -> AppResult<String> {
    let code = code.trim();
    // Epic emite códigos hexadecimales de treinta y dos caracteres, pero el
    // rango se deja holgado a propósito: rechazar un código válido porque su
    // longitud cambió sería peor que dejar que Epic conteste que no vale.
    let plausible =
        (16..=128).contains(&code.len()) && code.bytes().all(|byte| byte.is_ascii_alphanumeric());
    if !plausible {
        return Err(invalid_code());
    }
    Ok(code.to_string())
}

fn missing_code() -> AppError {
    AppError::validation("Pega el código de autorización que te ha dado Epic.")
}

fn invalid_code() -> AppError {
    // El mensaje no repite lo que se pegó: puede ser el código entero.
    AppError::validation(
        "Eso no parece un código de autorización de Epic. Copia el valor de «authorizationCode» de la página que se ha abierto.",
    )
}

// ---------------------------------------------------------------------------
// Sesión
// ---------------------------------------------------------------------------

/// Canjea un código de autorización por una sesión.
pub async fn exchange_code(code: &str) -> AppResult<StoredSession> {
    let code = extract_authorization_code(code)?;
    request_token(&[
        ("grant_type", "authorization_code"),
        ("code", code.as_str()),
        ("token_type", "eg1"),
    ])
    .await
}

/// Renueva la sesión sin volver a pedir credenciales.
pub async fn refresh(session: &StoredSession) -> AppResult<StoredSession> {
    request_token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", session.refresh_token.as_str()),
        ("token_type", "eg1"),
    ])
    .await
}

async fn request_token(form: &[(&str, &str)]) -> AppResult<StoredSession> {
    let response = net::client()?
        .post(format!("https://{OAUTH_HOST}/account/api/oauth/token"))
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
        .form(form)
        .send()
        .await
        .map_err(|_| net::network_error(STORE))?;

    net::check_status(STORE, response.status())?;
    let token: TokenResponse = net::read_json(STORE, response).await?;
    let now = chrono::Utc::now().timestamp();

    let session = StoredSession {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: now.saturating_add(token.expires_in.max(0)),
        refresh_expires_at: token
            .refresh_expires
            .filter(|seconds| *seconds > 0)
            .map(|seconds| now.saturating_add(seconds)),
        account_id: token.account_id,
        account_name: token.display_name.and_then(|name| sanitize_title(&name)),
    };
    session.validate()?;
    Ok(session)
}

/// Invalida el testigo en los servidores de Epic.
///
/// Se llama al cerrar sesión, antes de borrar el llavero. Que falle no impide
/// cerrar sesión: el testigo local se borra igual, y quien quiera asegurarse de
/// que no queda nada del lado de Epic tiene el enlace a la gestión de sesiones
/// de su cuenta.
pub async fn revoke(session: &StoredSession) -> AppResult<()> {
    let response = net::client()?
        .delete(format!(
            "https://{OAUTH_HOST}/account/api/oauth/sessions/kill/{}",
            urlencoding_path(&session.access_token)
        ))
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .bearer_auth(&session.access_token)
        .send()
        .await
        .map_err(|_| net::network_error(STORE))?;
    net::check_status(STORE, response.status())
}

/// Escapa el testigo para poder incrustarlo en una ruta.
///
/// Los `eg1` son hexadecimales, así que en la práctica nunca hay nada que
/// escapar; la función existe para que un cambio de formato en Epic no
/// convierta el testigo en una ruta con barras.
fn urlencoding_path(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Biblioteca
// ---------------------------------------------------------------------------

/// Lee la biblioteca completa de la cuenta.
///
/// Primero pide lo que se posee y después una ficha por juego, que es lo único
/// que devuelve el título. Ése es el coste real de la operación: una petición
/// por juego de la biblioteca.
pub async fn fetch_library(session: &StoredSession) -> AppResult<Vec<DiscoveredGame>> {
    let assets = fetch_assets(session).await?;
    let mut pending: Vec<AssetEntry> = Vec::new();
    for asset in assets {
        if asset.namespace == UNREAL_NAMESPACE {
            continue;
        }
        if validate_epic_app_name(&asset.app_name).is_err()
            || asset.catalog_item_id.is_empty()
            || asset.namespace.is_empty()
        {
            continue;
        }
        pending.push(asset);
        if pending.len() >= MAX_DISCOVERED_GAMES {
            break;
        }
    }

    let mut games: BTreeMap<String, DiscoveredGame> = BTreeMap::new();
    for batch in pending.chunks(METADATA_CONCURRENCY) {
        let mut tasks = tokio::task::JoinSet::new();
        for asset in batch {
            let token = session.access_token.clone();
            let app_name = asset.app_name.clone();
            let namespace = asset.namespace.clone();
            let catalog_item_id = asset.catalog_item_id.clone();
            tasks.spawn(async move {
                let item = fetch_catalog_item(&token, &namespace, &catalog_item_id).await;
                (app_name, item)
            });
        }
        while let Some(joined) = tasks.join_next().await {
            let (app_name, item) = joined.map_err(|_| net::response_error(STORE))?;
            // Una ficha que no se pudo leer se omite: es preferible una
            // biblioteca con un juego de menos que una con un título inventado.
            let Ok(Some(item)) = item else { continue };
            if let Some(game) = into_discovered(app_name, item) {
                games.insert(game.external_id.clone(), game);
            }
        }
    }

    Ok(games.into_values().collect())
}

async fn fetch_assets(session: &StoredSession) -> AppResult<Vec<AssetEntry>> {
    let response = net::client()?
        .get(format!(
            "https://{LAUNCHER_HOST}/launcher/api/public/assets/{PLATFORM}"
        ))
        .query(&[("label", LABEL)])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .bearer_auth(&session.access_token)
        .send()
        .await
        .map_err(|_| net::network_error(STORE))?;
    net::check_status(STORE, response.status())?;
    net::read_json(STORE, response).await
}

async fn fetch_catalog_item(
    access_token: &str,
    namespace: &str,
    catalog_item_id: &str,
) -> AppResult<Option<CatalogItem>> {
    let response = net::client()?
        .get(format!(
            "https://{CATALOG_HOST}/catalog/api/shared/namespace/{namespace}/bulk/items"
        ))
        .query(&[
            ("id", catalog_item_id),
            ("includeDLCDetails", "true"),
            ("includeMainGameDetails", "true"),
            ("country", COUNTRY),
            ("locale", LOCALE),
        ])
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|_| net::network_error(STORE))?;
    net::check_status(STORE, response.status())?;
    // La respuesta llega indexada por identificador de catálogo, no como objeto
    // suelto.
    let mut items: BTreeMap<String, CatalogItem> = net::read_json(STORE, response).await?;
    Ok(items.remove(catalog_item_id))
}

/// Convierte una ficha del catálogo en un juego de la biblioteca, o descarta la
/// entrada cuando no lo es.
fn into_discovered(app_name: String, item: CatalogItem) -> Option<DiscoveredGame> {
    if item.main_game_item.is_some() {
        return None;
    }
    if item
        .categories
        .iter()
        .any(|category| category.path == MODS_CATEGORY)
    {
        return None;
    }
    let title = sanitize_title(&item.title)?;

    Some(DiscoveredGame {
        external_id: app_name,
        title,
        cover_url: pick_image(&item.key_images, &["DieselGameBoxTall", "OfferImageTall"]),
        header_url: pick_image(&item.key_images, &["DieselGameBox", "OfferImageWide"]),
        install_path: None,
        // La API dice lo que se posee, nunca lo que hay descargado. Marcar
        // «instalado» aquí sobrescribiría lo que sí sabe el escáner local.
        installed: false,
        size_on_disk: None,
        launch_target: None,
        // Epic no publica ninguna política de catálogo sin DRM.
        drm_state: DrmState::Unknown,
        source: ScanSource::EpicAccountLibrary,
    })
}

/// Elige la primera imagen cuyo tipo esté en la lista de preferencias.
///
/// Si ninguno coincide, no hay carátula: no se compone una URL a partir de un
/// patrón, porque Epic no garantiza ninguno.
fn pick_image(images: &[KeyImage], preferred: &[&str]) -> Option<String> {
    preferred.iter().find_map(|wanted| {
        images
            .iter()
            .find(|image| image.kind == *wanted)
            .and_then(|image| sanitize_https_url(&image.url))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CLIENT_ID, CatalogItem, KeyImage, extract_authorization_code, into_discovered, login_url,
        pick_image, urlencoding_path,
    };
    use crate::stores::ScanSource;

    fn catalog_item(json: &str) -> CatalogItem {
        serde_json::from_str(json).expect("interpretar la ficha de catálogo")
    }

    #[test]
    fn the_login_page_asks_for_a_code_for_the_launcher_client() {
        let url = login_url();
        assert!(url.starts_with("https://www.epicgames.com/id/login?"));
        assert!(url.contains(CLIENT_ID));
        assert!(url.contains("responseType%3Dcode"));
    }

    #[test]
    fn the_authorization_code_can_be_pasted_in_any_of_the_three_natural_shapes() {
        let code = "0123456789abcdef0123456789abcdef";

        assert_eq!(extract_authorization_code(code).unwrap(), code);
        assert_eq!(
            extract_authorization_code(&format!("  {code}  ")).unwrap(),
            code
        );
        assert_eq!(
            extract_authorization_code(&format!("\"{code}\"")).unwrap(),
            code
        );
        assert_eq!(
            extract_authorization_code(&format!(
                "{{\"redirectUrl\":\"https://localhost\",\"authorizationCode\":\"{code}\",\"sid\":null}}"
            ))
            .unwrap(),
            code
        );
        assert_eq!(
            extract_authorization_code(&format!(
                "https://www.epicgames.com/id/api/redirect?code={code}"
            ))
            .unwrap(),
            code
        );
    }

    #[test]
    fn a_rejected_code_is_never_echoed_back_in_the_error() {
        let secret = "-----no-deberia-aparecer-nunca-----";
        let error = extract_authorization_code(secret).expect_err("rechazar el código");
        assert_eq!(error.code, "validation");
        assert!(!error.message.contains(secret));

        assert!(extract_authorization_code("").is_err());
        assert!(extract_authorization_code("corto").is_err());
        assert!(extract_authorization_code("{\"sid\":null}").is_err());
    }

    #[test]
    fn a_token_can_never_turn_into_a_path_segment() {
        assert_eq!(urlencoding_path("abc123"), "abc123");
        assert_eq!(
            urlencoding_path("../../account/api/oauth"),
            "....accountapioauth"
        );
        assert_eq!(urlencoding_path("a/b?c=d#e"), "abcde");
    }

    #[test]
    fn a_dlc_is_not_a_game_of_the_library() {
        let item = catalog_item(
            r#"{"title":"Complemento","keyImages":[],"categories":[],"mainGameItem":{"id":"base"}}"#,
        );
        assert!(into_discovered("Complemento_App".to_string(), item).is_none());
    }

    #[test]
    fn a_mod_entry_is_not_a_game_of_the_library() {
        let item =
            catalog_item(r#"{"title":"Recurso","keyImages":[],"categories":[{"path":"mods"}]}"#);
        assert!(into_discovered("Recurso_App".to_string(), item).is_none());
    }

    #[test]
    fn a_catalogue_entry_without_a_usable_title_is_discarded_rather_than_named() {
        let item = catalog_item(r#"{"title":"   ","keyImages":[],"categories":[]}"#);
        assert!(into_discovered("Sin_Titulo".to_string(), item).is_none());
    }

    #[test]
    fn a_real_game_keeps_its_title_and_its_two_images() {
        let item = catalog_item(
            r#"{
                "title": "  Un\tJuego  ",
                "categories": [{"path": "games"}, {"path": "applications"}],
                "keyImages": [
                    {"type": "DieselGameBox", "url": "https://cdn1.epicgames.com/ancho.jpg"},
                    {"type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/alto.jpg"},
                    {"type": "Thumbnail", "url": "https://cdn1.epicgames.com/mini.jpg"}
                ]
            }"#,
        );
        let game = into_discovered("Juego_App".to_string(), item).expect("es un juego");
        assert_eq!(game.external_id, "Juego_App");
        assert_eq!(game.title, "Un Juego");
        assert_eq!(
            game.cover_url.as_deref(),
            Some("https://cdn1.epicgames.com/alto.jpg")
        );
        assert_eq!(
            game.header_url.as_deref(),
            Some("https://cdn1.epicgames.com/ancho.jpg")
        );
        // La API sólo dice lo que se posee: la instalación la conoce el escáner
        // local y no se puede fingir desde aquí.
        assert!(!game.installed);
        assert_eq!(game.install_path, None);
        assert_eq!(game.source, ScanSource::EpicAccountLibrary);
    }

    #[test]
    fn art_is_never_invented_when_epic_does_not_publish_it() {
        let images = vec![KeyImage {
            kind: "Thumbnail".to_string(),
            url: "https://cdn1.epicgames.com/mini.jpg".to_string(),
        }];
        assert_eq!(pick_image(&images, &["DieselGameBoxTall"]), None);

        // Un esquema que no sea https tampoco se acepta, venga de donde venga.
        let hostile = vec![KeyImage {
            kind: "DieselGameBoxTall".to_string(),
            url: "javascript:alert(1)".to_string(),
        }];
        assert_eq!(pick_image(&hostile, &["DieselGameBoxTall"]), None);
    }
}
