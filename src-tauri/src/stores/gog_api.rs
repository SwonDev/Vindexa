//! Cliente nativo de GOG: inicio de sesión, refresco, revocación y lectura de
//! la biblioteca completa.
//!
//! # De dónde sale cada dato de este módulo
//!
//! GOG tampoco emite credenciales a terceros. El flujo es el de GOG Galaxy, y
//! está descrito en el código abierto de `heroic-gogdl` (`gogdl/auth.py`, rama
//! `main`, consultado el 18 de agosto de 2026): identificador y secreto del
//! cliente, la URL de redirección `https://embed.gog.com/on_login_success?origin=client`
//! y las dos concesiones (`authorization_code` y `refresh_token`) contra
//! `https://auth.gog.com/token`.
//!
//! El recorrido de la biblioteca sigue a
//! `HeroicGamesLauncher/src/backend/storeManagers/gog/library.ts` de la rama
//! `main`, consultado el mismo día: `galaxy-library.gog.com` pagina lo que se
//! posee con `page_token`/`next_page_token`, y `gamesdb.gog.com` da el título y
//! las imágenes de cada lanzamiento. La forma de esa segunda respuesta se
//! comprobó contra el servicio real, que responde sin autenticación.
//!
//! # El secreto del cliente oficial
//!
//! [`CLIENT_ID`] y [`CLIENT_SECRET`] son los de GOG Galaxy y quedan legibles en
//! el binario de Vindexa, igual que los de Epic. Están publicados en gogdl
//! desde hace años. Lo que sí es secreto es el testigo, y ése vive en el
//! llavero (ver `crate::stores::secrets`).

use crate::error::{AppError, AppResult};
use crate::stores::launch::validate_gog_product_id;
use crate::stores::net;
use crate::stores::secrets::StoredSession;
use crate::stores::{
    DiscoveredGame, ExternalStore, MAX_DISCOVERED_GAMES, ScanSource, sanitize_https_url,
    sanitize_title,
};
use serde::Deserialize;
use std::collections::BTreeMap;

const STORE: ExternalStore = ExternalStore::Gog;

/// Credenciales de GOG Galaxy. Ver el comentario del módulo.
const CLIENT_ID: &str = "46899977096215655";
const CLIENT_SECRET: &str = "9d85c43b1482497dbbce61f6e4aa173a433796eeae2ca8c5f6129f2dc4de46d9";

/// Redirección que espera el servidor de GOG. Tiene que coincidir exactamente
/// con la registrada para ese cliente, así que no es configurable.
const REDIRECT_URI: &str = "https://embed.gog.com/on_login_success?origin=client";

const AUTH_HOST: &str = "https://auth.gog.com";
const LIBRARY_HOST: &str = "https://galaxy-library.gog.com";
const GAMESDB_HOST: &str = "https://gamesdb.gog.com";
const USERS_HOST: &str = "https://users.gog.com";

/// Plataforma cuyos lanzamientos son juegos comprados en GOG.
///
/// `galaxy-library` devuelve también los de las integraciones que la persona
/// usuaria tenga conectadas —Steam, Xbox, Origin— y ésos no son biblioteca de
/// GOG: los muestra su propia tienda.
const GOG_PLATFORM: &str = "gog";

/// Cuántas fichas se piden a la vez a `gamesdb`.
const METADATA_CONCURRENCY: usize = 8;

/// Tope de páginas de la biblioteca. Es una salvaguarda contra un
/// `next_page_token` que no avance, no un límite de biblioteca.
const MAX_LIBRARY_PAGES: usize = 200;

/// Idioma con el que se prefiere el título. `*` es el título neutro que
/// `gamesdb` devuelve siempre.
const LOCALE: &str = "es-ES";
const NEUTRAL_LOCALE: &str = "*";

/// Página que hay que abrir para obtener un código de autorización.
///
/// Tras iniciar sesión, GOG redirige a [`REDIRECT_URI`] con el código en el
/// parámetro `code` de la barra de direcciones.
pub fn login_url() -> String {
    format!(
        "{AUTH_HOST}/auth?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&layout=client2",
        encode_component(REDIRECT_URI)
    )
}

/// Página desde la que se revocan las sesiones de la cuenta.
///
/// GOG no publica un extremo de revocación equivalente al de Epic, así que en
/// vez de fingir que Vindexa puede invalidar el testigo del lado del servidor,
/// se enseña dónde puede hacerlo la persona usuaria.
pub const SESSIONS_URL: &str = "https://www.gog.com/account/settings/security";

// ---------------------------------------------------------------------------
// Respuestas de GOG
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: i64,
    user_id: String,
}

#[derive(Debug, Deserialize)]
struct LibraryPage {
    #[serde(default)]
    items: Vec<LibraryEntry>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LibraryEntry {
    #[serde(default)]
    platform_id: String,
    #[serde(default)]
    external_id: String,
    /// Certificado que `gamesdb` acepta en `X-GOG-Library-Cert`. No es un
    /// secreto de la cuenta: acompaña al lanzamiento, no a la persona.
    #[serde(default)]
    certificate: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GamesDbRelease {
    #[serde(default)]
    title: Option<LocalizedTitle>,
    #[serde(default)]
    game: Option<GamesDbGame>,
}

#[derive(Debug, Deserialize)]
struct GamesDbGame {
    #[serde(default)]
    title: Option<LocalizedTitle>,
    #[serde(default)]
    vertical_cover: Option<ImageFormat>,
    #[serde(default)]
    cover: Option<ImageFormat>,
    #[serde(default)]
    background: Option<ImageFormat>,
    #[serde(default)]
    horizontal_artwork: Option<ImageFormat>,
}

/// `gamesdb` devuelve los títulos como un mapa de idioma a texto, con `*` como
/// valor neutro.
#[derive(Debug, Deserialize)]
struct LocalizedTitle(BTreeMap<String, String>);

impl LocalizedTitle {
    fn resolve(&self) -> Option<&str> {
        self.0
            .get(LOCALE)
            .or_else(|| self.0.get(NEUTRAL_LOCALE))
            .or_else(|| self.0.values().next())
            .map(String::as_str)
    }
}

/// Las imágenes llegan como plantilla, no como URL: `{formatter}` es el recorte
/// y `{ext}` la extensión.
#[derive(Debug, Deserialize)]
struct ImageFormat {
    #[serde(default)]
    url_format: String,
}

impl ImageFormat {
    fn resolve(&self) -> Option<String> {
        if self.url_format.is_empty() {
            return None;
        }
        let resolved = self
            .url_format
            .replace("{formatter}", "")
            .replace("{ext}", "jpg");
        sanitize_https_url(&resolved)
    }
}

#[derive(Debug, Deserialize)]
struct UserProfile {
    #[serde(default)]
    username: Option<String>,
}

// ---------------------------------------------------------------------------
// Códigos de autorización
// ---------------------------------------------------------------------------

/// Saca el código de autorización de lo que la persona usuaria haya pegado.
///
/// GOG deja el código en la barra de direcciones, así que lo natural es pegar
/// la URL entera; pegar sólo el código también vale.
pub fn extract_authorization_code(input: &str) -> AppResult<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(AppError::validation(
            "Pega la dirección a la que te ha llevado GOG, o el código que lleva dentro.",
        ));
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
    let plausible = (16..=256).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if !plausible {
        return Err(invalid_code());
    }
    Ok(code.to_string())
}

fn invalid_code() -> AppError {
    // El mensaje no repite lo pegado: puede contener el código entero.
    AppError::validation(
        "Esa dirección no lleva ningún código de GOG. Copia la barra de direcciones completa de la página en blanco a la que te llevó el inicio de sesión.",
    )
}

// ---------------------------------------------------------------------------
// Sesión
// ---------------------------------------------------------------------------

/// Canjea un código de autorización por una sesión.
pub async fn exchange_code(code: &str) -> AppResult<StoredSession> {
    let code = extract_authorization_code(code)?;
    request_token(&[
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
        ("code", code.as_str()),
    ])
    .await
}

/// Renueva la sesión sin volver a pedir credenciales.
pub async fn refresh(session: &StoredSession) -> AppResult<StoredSession> {
    let mut renewed = request_token(&[
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("grant_type", "refresh_token"),
        ("refresh_token", session.refresh_token.as_str()),
    ])
    .await?;
    // GOG no repite el nombre visible al refrescar. Conservarlo evita que la
    // tarjeta pase de «sesión de Fulanita» a «sesión iniciada» sin motivo.
    if renewed.account_name.is_none() {
        renewed.account_name = session.account_name.clone();
    }
    Ok(renewed)
}

async fn request_token(query: &[(&str, &str)]) -> AppResult<StoredSession> {
    let response = net::client()?
        .get(format!("{AUTH_HOST}/token"))
        .query(query)
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
        // GOG no publica cuándo caduca el testigo de refresco. Queda en `None`
        // en vez de inventarse una fecha.
        refresh_expires_at: None,
        account_id: token.user_id,
        account_name: None,
    };
    session.validate()?;
    Ok(session)
}

/// Pregunta a GOG el nombre visible de la cuenta.
///
/// Se hace en una petición aparte porque el canje del código no lo devuelve.
/// Si falla, la sesión sigue siendo válida y la tarjeta dirá «sesión iniciada»
/// sin nombre: es preferible a inventar uno.
pub async fn fetch_account_name(session: &StoredSession) -> Option<String> {
    let response = net::client()
        .ok()?
        .get(format!("{USERS_HOST}/users/{}", encode_path(&session.account_id)))
        .bearer_auth(&session.access_token)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let profile: UserProfile = net::read_json(STORE, response).await.ok()?;
    profile.username.as_deref().and_then(sanitize_title)
}

// ---------------------------------------------------------------------------
// Biblioteca
// ---------------------------------------------------------------------------

/// Lee la biblioteca completa de la cuenta.
pub async fn fetch_library(session: &StoredSession) -> AppResult<Vec<DiscoveredGame>> {
    let entries = fetch_releases(session).await?;

    let mut games: BTreeMap<String, DiscoveredGame> = BTreeMap::new();
    for batch in entries.chunks(METADATA_CONCURRENCY) {
        let mut tasks = tokio::task::JoinSet::new();
        for entry in batch {
            let token = session.access_token.clone();
            let external_id = entry.external_id.clone();
            let certificate = entry.certificate.clone();
            tasks.spawn(async move {
                let release = fetch_release(&token, &external_id, certificate.as_deref()).await;
                (external_id, release)
            });
        }
        while let Some(joined) = tasks.join_next().await {
            let (external_id, release) = joined.map_err(|_| net::response_error(STORE))?;
            // Un lanzamiento cuya ficha no se pudo leer se omite: mejor un
            // juego de menos que un título inventado.
            let Ok(release) = release else { continue };
            if let Some(game) = into_discovered(external_id, release) {
                games.insert(game.external_id.clone(), game);
            }
        }
    }

    Ok(games.into_values().collect())
}

async fn fetch_releases(session: &StoredSession) -> AppResult<Vec<LibraryEntry>> {
    let mut entries: Vec<LibraryEntry> = Vec::new();
    let mut page_token: Option<String> = None;

    for _ in 0..MAX_LIBRARY_PAGES {
        let mut request = net::client()?
            .get(format!(
                "{LIBRARY_HOST}/users/{}/releases",
                encode_path(&session.account_id)
            ))
            .bearer_auth(&session.access_token);
        if let Some(token) = page_token.as_deref() {
            request = request.query(&[("page_token", token)]);
        }
        let response = request
            .send()
            .await
            .map_err(|_| net::network_error(STORE))?;
        net::check_status(STORE, response.status())?;
        let page: LibraryPage = net::read_json(STORE, response).await?;

        for entry in page.items {
            if entry.platform_id != GOG_PLATFORM {
                continue;
            }
            if validate_gog_product_id(&entry.external_id).is_err() {
                continue;
            }
            entries.push(entry);
            if entries.len() >= MAX_DISCOVERED_GAMES {
                return Ok(entries);
            }
        }

        match page.next_page_token {
            // Un `next_page_token` que repite el anterior no avanza: cortar es
            // preferible a girar en redondo hasta agotar el límite de páginas.
            Some(next) if Some(&next) != page_token.as_ref() => page_token = Some(next),
            _ => break,
        }
    }
    Ok(entries)
}

async fn fetch_release(
    access_token: &str,
    external_id: &str,
    certificate: Option<&str>,
) -> AppResult<GamesDbRelease> {
    let mut request = net::client()?
        .get(format!(
            "{GAMESDB_HOST}/platforms/{GOG_PLATFORM}/external_releases/{}",
            encode_path(external_id)
        ))
        .bearer_auth(access_token);
    if let Some(certificate) = certificate.filter(|value| is_safe_header_value(value)) {
        request = request.header("X-GOG-Library-Cert", certificate);
    }
    let response = request
        .send()
        .await
        .map_err(|_| net::network_error(STORE))?;
    net::check_status(STORE, response.status())?;
    net::read_json(STORE, response).await
}

/// Una cabecera sólo puede llevar caracteres imprimibles ASCII. Un certificado
/// con un salto de línea partiría la petición en dos.
fn is_safe_header_value(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 4_096
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
}

fn into_discovered(external_id: String, release: GamesDbRelease) -> Option<DiscoveredGame> {
    let game = release.game;
    let title = release
        .title
        .as_ref()
        .and_then(LocalizedTitle::resolve)
        .or_else(|| {
            game.as_ref()
                .and_then(|game| game.title.as_ref())
                .and_then(LocalizedTitle::resolve)
        })
        .and_then(sanitize_title)?;

    let cover_url = game.as_ref().and_then(|game| {
        game.vertical_cover
            .as_ref()
            .or(game.cover.as_ref())
            .and_then(ImageFormat::resolve)
    });
    let header_url = game.as_ref().and_then(|game| {
        game.background
            .as_ref()
            .or(game.horizontal_artwork.as_ref())
            .and_then(ImageFormat::resolve)
    });

    Some(DiscoveredGame {
        external_id,
        title,
        cover_url,
        header_url,
        install_path: None,
        // La API dice lo que se posee, no lo que hay descargado.
        installed: false,
        size_on_disk: None,
        launch_target: None,
        // Todo el catálogo de GOG es sin DRM por política publicada de la
        // tienda; la evidencia la aporta `ExternalStore`.
        drm_state: STORE.catalogue_drm_state(),
        source: ScanSource::GogAccountLibrary,
    })
}

// ---------------------------------------------------------------------------
// Escapado
// ---------------------------------------------------------------------------

/// Escapa un valor para incrustarlo en una cadena de consulta.
fn encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Deja en un segmento de ruta sólo lo que no puede cambiar su significado.
/// El identificador de cuenta y el de producto son numéricos, así que en la
/// práctica no hay nada que quitar; la función está para que un cambio de
/// formato no convierta un valor en una ruta con barras.
fn encode_path(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        CLIENT_ID, GamesDbRelease, encode_component, encode_path, extract_authorization_code,
        into_discovered, is_safe_header_value, login_url,
    };
    use crate::db::rich_metadata::DrmState;
    use crate::stores::ScanSource;

    fn release(json: &str) -> GamesDbRelease {
        serde_json::from_str(json).expect("interpretar el lanzamiento")
    }

    #[test]
    fn the_login_page_uses_the_galaxy_client_and_its_exact_redirect() {
        let url = login_url();
        assert!(url.starts_with("https://auth.gog.com/auth?"));
        assert!(url.contains(CLIENT_ID));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("redirect_uri=https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient"));
    }

    #[test]
    fn the_authorization_code_can_be_pasted_as_the_address_or_on_its_own() {
        let code = "abcdefghij0123456789ABCDEF";
        assert_eq!(extract_authorization_code(code).unwrap(), code);
        assert_eq!(
            extract_authorization_code(&format!(
                "https://embed.gog.com/on_login_success?origin=client&code={code}"
            ))
            .unwrap(),
            code
        );
    }

    #[test]
    fn a_rejected_code_is_never_echoed_back_in_the_error() {
        let secret = "https://embed.gog.com/on_login_success?origin=client&token=NO-DEBE-SALIR";
        let error = extract_authorization_code(secret).expect_err("rechazar la dirección");
        assert_eq!(error.code, "validation");
        assert!(!error.message.contains("NO-DEBE-SALIR"));
        assert!(extract_authorization_code("").is_err());
        assert!(extract_authorization_code("corto").is_err());
    }

    #[test]
    fn a_release_becomes_a_drm_free_game_with_its_real_art() {
        // Recorte de la respuesta real de gamesdb para el producto 1207658930.
        let entry = release(
            r#"{
                "title": {"*": "The Witcher 2", "en-US": "The Witcher 2: EE"},
                "game": {
                    "title": {"*": "The Witcher 2"},
                    "vertical_cover": {"url_format": "https://images.gog.com/37fa{formatter}.{ext}?namespace=gamesdb"},
                    "background": {"url_format": "https://images.gog.com/a75b{formatter}.{ext}?namespace=gamesdb"}
                }
            }"#,
        );
        let game = into_discovered("1207658930".to_string(), entry).expect("es un juego");
        assert_eq!(game.title, "The Witcher 2");
        assert_eq!(
            game.cover_url.as_deref(),
            Some("https://images.gog.com/37fa.jpg?namespace=gamesdb")
        );
        assert_eq!(
            game.header_url.as_deref(),
            Some("https://images.gog.com/a75b.jpg?namespace=gamesdb")
        );
        assert_eq!(game.drm_state, DrmState::DrmFree);
        assert!(!game.installed);
        assert_eq!(game.source, ScanSource::GogAccountLibrary);
    }

    #[test]
    fn a_release_without_a_title_is_discarded_rather_than_named() {
        assert!(into_discovered("1207658930".to_string(), release("{}")).is_none());
        assert!(
            into_discovered("1207658930".to_string(), release(r#"{"title":{"*":"  "}}"#)).is_none()
        );
    }

    #[test]
    fn art_is_never_invented_when_gamesdb_does_not_publish_it() {
        let entry = release(r#"{"title": {"*": "Un juego"}, "game": {"title": {"*": "Un juego"}}}"#);
        let game = into_discovered("1207658930".to_string(), entry).expect("es un juego");
        assert_eq!(game.cover_url, None);
        assert_eq!(game.header_url, None);
    }

    #[test]
    fn a_certificate_can_never_split_the_request_in_two() {
        assert!(is_safe_header_value("cert-valido.123"));
        assert!(!is_safe_header_value("cert\r\nX-Inyectada: si"));
        assert!(!is_safe_header_value(""));
        assert!(!is_safe_header_value(&"x".repeat(4_097)));
    }

    #[test]
    fn identifiers_can_never_become_extra_path_segments() {
        assert_eq!(encode_path("1207658930"), "1207658930");
        assert_eq!(encode_path("../../users/otra"), "usersotra");
        assert_eq!(
            encode_component("https://embed.gog.com/on_login_success?origin=client"),
            "https%3A%2F%2Fembed.gog.com%2Fon_login_success%3Forigin%3Dclient"
        );
    }
}
