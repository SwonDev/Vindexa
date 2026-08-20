//! Integración nativa con itch.io a través de su API oficial.
//!
//! # Por qué esta tienda sí lleva credenciales y Epic y GOG no
//!
//! El resto de este módulo lee manifiestos que los clientes oficiales dejan en
//! el disco, porque Epic y GOG no publican una API para que una aplicación de
//! terceros consulte la biblioteca de una persona. itch.io sí la publica y
//! documenta: la propia persona genera una clave desde los ajustes de su cuenta
//! y la revoca cuando quiera. Por eso aquí se pide una clave y en las otras
//! tiendas no: es la vía oficial, no un rodeo.
//!
//! Documentación de referencia: <https://itch.io/docs/api/serverside>.
//! La clave se genera en <https://itch.io/user/settings/api-keys>.
//!
//! # Qué comparte con Epic y GOG y qué no
//!
//! El cliente HTTP es el único de las tiendas externas: [`crate::stores::net`].
//! Sigue redirecciones, lo cual es seguro aquí porque `reqwest` retira la
//! cabecera `Authorization` en cuanto una redirección cambia de anfitrión, de
//! puerto o de esquema.
//!
//! Lo que **no** se comparte son los errores ni la custodia del secreto. Ambos
//! están indexados por `ExternalStore`, que sólo tiene `Epic` y `Gog`: un error
//! de `net` diría «Epic» o «GOG» donde hay que decir «itch.io», y `stores::secrets`
//! guarda pares de testigos OAuth, no una clave que la persona pega. Cuando
//! `ExternalStore` admita `Itch`, ambas piezas se pueden unificar.
//!
//! # Dónde acaba la clave
//!
//! En el llavero del sistema, con el mismo patrón que la clave de Steam Web API
//! (ver `crate::steam::secrets`). **Nunca** se escribe en SQLite, ni en un
//! fichero, ni en un registro, ni viaja a la interfaz: los comandos sólo
//! informan de si hay clave guardada o no. Ningún mensaje de error la incluye,
//! ni siquiera recortada.
//!
//! # Qué se importa y qué no
//!
//! `/profile/owned-keys` devuelve **todo** lo que la cuenta posee, y en itch.io
//! eso incluye herramientas, paquetes de recursos, cómics, libros y bandas
//! sonoras además de juegos. Vindexa es una biblioteca de juegos, así que sólo
//! entra lo que su creador clasificó como `game`. Lo demás no se descarta en
//! silencio: el informe dice cuántas entradas quedaron fuera y de qué tipo era
//! cada una.
//!
//! # Nunca se inventa un dato
//!
//! itch.io no publica imagen de cabecera, así que `header_url` queda vacía en
//! vez de fabricarse a partir de un patrón de URL. Las entradas cuya ficha ya no
//! existe (página retirada) se cuentan como omitidas en vez de rellenarse con un
//! título de relleno. El estado de DRM queda en [`DrmState::Unknown`]: las
//! propias preguntas frecuentes de itch.io dicen que **la mayoría** de las
//! publicaciones traen builds sin DRM, no todas, y «la mayoría» no es evidencia
//! suficiente para marcar un juego concreto.

use crate::db::rich_metadata::DrmState;
use crate::error::{AppError, AppResult};
use crate::stores::matching::{MatchCandidate, SteamTitleIndex};
use crate::stores::net;
use crate::stores::{MAX_DISCOVERED_GAMES, sanitize_title};
use reqwest::{StatusCode, header};
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{Instant, sleep_until};
use url::Url;

/// Valor de la columna `store` en `external_store_accounts` y `external_games`.
pub const STORE: &str = "itch";

/// Nombre visible de la tienda. Es una constante del producto.
#[allow(dead_code, reason = "el nombre visible lo pone hoy la interfaz")]
pub const DISPLAY_NAME: &str = "itch.io";

/// Página donde la persona usuaria genera y revoca su clave. Se enseña en
/// Ajustes para no obligar a buscarla.
#[allow(
    dead_code,
    reason = "la interfaz y las capacidades ya declaran esta URL"
)]
pub const API_KEYS_URL: &str = "https://itch.io/user/settings/api-keys";

const PROFILE_ENDPOINT: &str = "https://api.itch.io/profile";
const OWNED_KEYS_ENDPOINT: &str = "https://api.itch.io/profile/owned-keys";

/// Espera mínima entre peticiones a itch.io.
///
/// itch.io **no documenta** ningún límite de peticiones. El cliente oficial en
/// Go (`itchio/go-itchio`, `rate_limiter.go`) comenta que el servidor admite
/// unas 8 peticiones por segundo, y ese es el único número con origen conocido.
/// Importar una biblioteca no es una operación que corra prisa, así que Vindexa
/// se queda muy por debajo en vez de acercarse a un límite que nadie garantiza.
const MIN_REQUEST_INTERVAL: Duration = Duration::from_millis(400);

/// Tope de tamaño de una respuesta. Una página de claves con su ficha completa
/// ronda unos pocos cientos de kilobytes.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Tope de claves que se aceptan de una importación. Es el mismo que aplica el
/// cliente oficial de itch (`itchio/butler`, `fetch_limits.go`).
const MAX_OWNED_KEYS: usize = 5_000;

/// Tope de páginas que se recorren. Protege frente a una respuesta que nunca
/// llegue a vaciarse.
const MAX_PAGES: i64 = 200;

/// Longitudes admitidas para una clave. itch.io **no publica** el formato de
/// sus claves, así que aquí no se valida una forma inventada: sólo se rechaza
/// lo que no puede viajar en una cabecera HTTP.
const MIN_KEY_CHARS: usize = 8;
const MAX_KEY_CHARS: usize = 512;

/// Anfitriones desde los que se acepta una carátula. itch.io sirve las suyas
/// desde su propio CDN; cualquier otra cosa se descarta en vez de guardarse.
const COVER_HOSTS: [&str; 2] = ["itch.zone", "itch.io"];

// ---------------------------------------------------------------------------
// La clave, en el llavero del sistema
// ---------------------------------------------------------------------------

/// Custodia de la clave de itch.io.
///
/// Copia deliberada del patrón de `crate::steam::secrets`: misma cuenta de
/// servicio, mismos errores opacos, misma promesa de que la clave no sale de
/// aquí.
pub mod secrets {
    use super::{MAX_KEY_CHARS, MIN_KEY_CHARS};
    use crate::error::{AppError, AppResult};

    const SERVICE: &str = "io.vindexa.desktop";
    const API_KEY_ACCOUNT: &str = "itch-api-key";

    use crate::keychain;

    pub fn save_api_key(value: &str) -> AppResult<()> {
        let value = value.trim();
        validate_api_key(value)?;
        keychain::set(SERVICE, API_KEY_ACCOUNT, value).map_err(keyring_error)
    }

    pub fn load_api_key() -> AppResult<Option<String>> {
        match keychain::get(SERVICE, API_KEY_ACCOUNT) {
            Ok(value) => {
                validate_api_key(&value)?;
                Ok(Some(value))
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    pub fn has_api_key() -> AppResult<bool> {
        load_api_key().map(|value| value.is_some())
    }

    pub fn delete_api_key() -> AppResult<()> {
        match keychain::delete(SERVICE, API_KEY_ACCOUNT) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }

    /// Comprueba que la clave puede viajar en una cabecera HTTP.
    ///
    /// **No** valida un formato concreto: itch.io no lo publica, e inventarse
    /// uno rechazaría claves buenas el día que cambien de generador. Quien dice
    /// si la clave sirve es itch.io, y lo dice respondiendo a `/profile`.
    pub fn validate_api_key(value: &str) -> AppResult<()> {
        let length = value.chars().count();
        if !(MIN_KEY_CHARS..=MAX_KEY_CHARS).contains(&length) {
            return Err(AppError::validation(
                "La clave de itch.io no tiene una longitud admisible. Cópiala entera desde los ajustes de tu cuenta.",
            ));
        }
        if !value
            .chars()
            .all(|character| character.is_ascii_graphic() && character != '"' && character != '\\')
        {
            return Err(AppError::validation(
                "La clave de itch.io contiene caracteres que no puede tener. Cópiala tal cual desde los ajustes de tu cuenta.",
            ));
        }
        Ok(())
    }

    /// El error del llavero nunca describe qué se intentaba guardar.
    fn keyring_error(_error: keyring::Error) -> AppError {
        AppError::new(
            "secure_storage",
            "No se pudo acceder al almacén seguro del sistema.",
        )
    }

    #[cfg(test)]
    mod tests {
        use super::{keyring_error, validate_api_key};

        #[test]
        fn accepts_any_printable_key_of_reasonable_length() {
            assert!(validate_api_key("AbCd1234EfGh5678").is_ok());
            assert!(validate_api_key("corta").is_err());
            assert!(validate_api_key(&"a".repeat(600)).is_err());
            assert!(validate_api_key("con espacio dentro").is_err());
            assert!(validate_api_key("salto\nde línea").is_err());
        }

        #[test]
        fn secure_storage_errors_do_not_expose_the_key() {
            let error = keyring_error(keyring::Error::Invalid(
                "itch-api-key".to_string(),
                "clave=SUPERSECRETA".to_string(),
            ));

            assert_eq!(error.code, "secure_storage");
            assert!(!error.message.contains("SUPERSECRETA"));
            assert!(!error.message.contains("itch-api-key"));
        }
    }
}

// ---------------------------------------------------------------------------
// Lo que devuelve itch.io
// ---------------------------------------------------------------------------

/// El perfil de la cuenta a la que pertenece la clave.
///
/// Sólo se conserva lo que hace falta para que la persona vea con qué cuenta ha
/// entrado. No se guarda el correo ni ningún otro dato personal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchAccountProfile {
    pub id: i64,
    pub username: String,
    pub display_name: Option<String>,
    pub url: Option<String>,
}

impl ItchAccountProfile {
    /// Cómo se llama esta cuenta en la interfaz.
    pub fn label(&self) -> &str {
        match self.display_name.as_deref() {
            Some(name) if !name.trim().is_empty() => name,
            _ => &self.username,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProfileEnvelope {
    user: Option<WireUser>,
}

#[derive(Debug, Deserialize)]
struct WireUser {
    id: i64,
    username: Option<String>,
    display_name: Option<String>,
    url: Option<String>,
}

/// Una página de `/profile/owned-keys`.
///
/// El formato de cable es `snake_case`, tal y como documenta itch.io. El cliente
/// oficial en Go lo convierte a `camelCase` **después** de recibirlo; aquí se
/// lee tal cual llega.
#[derive(Debug, Deserialize)]
struct WireOwnedKeysPage {
    #[serde(default, deserialize_with = "lista_o_tabla_vacia")]
    owned_keys: Vec<WireOwnedKey>,
}

/// Lee `owned_keys` acepte itch.io la forma que acepte.
///
/// Cuando quedan claves, el campo llega como lista. Cuando no queda ninguna
/// llega como **objeto vacío**:
///
/// ```text
/// {"owned_keys":{},"page":2,"per_page":50}
/// ```
///
/// No es un capricho del servidor: itch.io corre sobre Lua, donde una tabla
/// vacía no distingue entre lista y diccionario, y al serializarla sale `{}`.
/// Comprobado contra la API el 2026-08-19 con una cuenta real: la primera
/// página devolvió sus claves en una lista y la segunda —la que cierra el
/// recorrido— devolvió esa tabla vacía.
///
/// Antes de esto, la página final rompía el análisis y **la importación entera
/// se caía después de haber leído bien todas las anteriores**, así que no
/// llegaba a guardarse ni un juego.
fn lista_o_tabla_vacia<'de, D>(deserializer: D) -> Result<Vec<WireOwnedKey>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitante;

    impl<'de> serde::de::Visitor<'de> for Visitante {
        type Value = Vec<WireOwnedKey>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("una lista de claves de descarga, o una tabla")
        }

        fn visit_seq<A>(self, mut acceso: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut claves = Vec::with_capacity(acceso.size_hint().unwrap_or_default());
            while let Some(clave) = acceso.next_element()? {
                claves.push(clave);
            }
            Ok(claves)
        }

        /// Una tabla de Lua con contenido llega indexada por su posición
        /// (`{"1": …, "2": …}`). El índice no aporta nada que no esté ya en la
        /// clave, así que se descarta y se conserva la entrada.
        fn visit_map<A>(self, mut acceso: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::MapAccess<'de>,
        {
            let mut claves = Vec::new();
            while let Some((_, clave)) =
                acceso.next_entry::<serde::de::IgnoredAny, WireOwnedKey>()?
            {
                claves.push(clave);
            }
            Ok(claves)
        }

        /// `null` es «no hay claves», no un fallo de formato.
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Vec::new())
        }
    }

    deserializer.deserialize_any(Visitante)
}

#[derive(Debug, Deserialize)]
struct WireOwnedKey {
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    game: Option<WireGame>,
}

#[derive(Debug, Deserialize)]
struct WireGame {
    id: i64,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    classification: Option<String>,
    #[serde(default)]
    cover_url: Option<String>,
    #[serde(default)]
    still_cover_url: Option<String>,
}

/// Envoltorio de error de itch.io: `{"errors": ["invalid key"]}`.
#[derive(Debug, Deserialize)]
struct WireErrors {
    #[serde(default)]
    errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Clasificación
// ---------------------------------------------------------------------------

/// Etiqueta en español de una clasificación de itch.io.
///
/// Los valores son los que declara el cliente oficial (`itchio/go-itchio`,
/// `types.go`). Uno desconocido no se traduce: se enseña tal cual llegó, porque
/// inventar una etiqueta oculta que itch.io ha añadido una categoría nueva.
fn classification_label(raw: &str) -> String {
    match raw {
        "game" => "Juegos".to_string(),
        "tool" => "Herramientas y programas".to_string(),
        "assets" => "Recursos gráficos o sonoros".to_string(),
        "game_mod" => "Mods".to_string(),
        "physical_game" => "Juegos físicos o imprimibles".to_string(),
        "soundtrack" => "Bandas sonoras".to_string(),
        "comic" => "Cómics".to_string(),
        "book" => "Libros".to_string(),
        "other" => "Otros contenidos".to_string(),
        "" => "Sin clasificar".to_string(),
        other => other.to_string(),
    }
}

/// Motivos por los que una entrada de la cuenta no llega a la biblioteca.
const SKIP_NO_PAGE: &str = "sin_ficha";
const SKIP_NO_TITLE: &str = "sin_titulo";
const SKIP_REPEATED: &str = "clave_repetida";

fn skip_label(reason: &str) -> String {
    match reason {
        SKIP_NO_PAGE => "Fichas retiradas de itch.io".to_string(),
        SKIP_NO_TITLE => "Entradas sin título utilizable".to_string(),
        SKIP_REPEATED => "Claves repetidas de la misma ficha".to_string(),
        other => classification_label(other),
    }
}

// ---------------------------------------------------------------------------
// Modelos que consume la interfaz
// ---------------------------------------------------------------------------

/// Un juego de itch.io listo para persistir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItchLibraryEntry {
    /// Identificador del juego dentro de itch.io, en decimal.
    pub external_id: String,
    pub title: String,
    /// Carátula publicada por itch.io, ya validada. `None` cuando la ficha no
    /// trae ninguna: no se fabrica.
    pub cover_url: Option<String>,
    /// Fecha en que se obtuvo la clave, tal y como la da itch.io (RFC 3339).
    pub acquired_at: Option<String>,
}

/// Un grupo de entradas omitidas, con su motivo y su recuento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchSkipGroup {
    /// Valor tal y como lo devolvió itch.io, o el motivo interno.
    pub reason: String,
    /// Texto en español para la interfaz.
    pub label: String,
    pub count: usize,
}

/// Resultado de leer la cuenta, antes de tocar la base de datos.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItchFetch {
    pub profile: ItchAccountProfile,
    pub entries: Vec<ItchLibraryEntry>,
    /// Cuántas claves devolvió itch.io en total, antes de filtrar.
    pub owned_keys: usize,
    pub skipped: Vec<ItchSkipGroup>,
    pub pages: i64,
    /// `true` cuando se alcanzó el tope y quedaron claves sin leer.
    pub truncated: bool,
}

/// Informe honesto de una importación.
///
/// No existe un «importado ✓»: cada número dice una cosa distinta y todos
/// viajan a la interfaz.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchImportReport {
    pub store: String,
    /// Con qué cuenta se importó.
    pub account: String,
    /// Cuántas claves tiene la cuenta (todo, no sólo juegos).
    pub owned_keys: usize,
    /// Cuántas de ellas son juegos y han entrado.
    pub imported: usize,
    /// De esos juegos, cuántos no estaban antes en Vindexa.
    pub added: usize,
    /// De esos juegos, cuántos ya estaban y sólo se han actualizado.
    pub already_present: usize,
    /// Cuántos entraron sin carátula porque itch.io no publica ninguna.
    pub without_cover: usize,
    /// Cuántas entradas quedaron fuera.
    pub skipped: usize,
    /// Por qué quedaron fuera, agrupado.
    pub skipped_groups: Vec<ItchSkipGroup>,
    /// Cuántos juegos de itch.io están emparejados con uno de Steam.
    pub matched: usize,
    pub pages: i64,
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// Recorrido de páginas
// ---------------------------------------------------------------------------

/// Por qué se dejó de pedir páginas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StopReason {
    /// itch.io devolvió una página vacía: no queda nada más.
    Complete,
    /// Se alcanzó [`MAX_OWNED_KEYS`].
    Capped,
    /// Se alcanzó [`MAX_PAGES`].
    PageLimit,
}

impl StopReason {
    fn truncated(self) -> bool {
        !matches!(self, Self::Complete)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageStep {
    Fetch(i64),
    Stop(StopReason),
}

/// Estado del recorrido de `/profile/owned-keys`.
///
/// Vive aparte de la petición HTTP para poder comprobar la política de
/// paginación —parar en página vacía, topes— sin tocar la red.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PageCursor {
    next_page: i64,
    collected: usize,
    pages_read: i64,
}

impl PageCursor {
    pub(crate) fn new() -> Self {
        Self {
            next_page: 1,
            collected: 0,
            pages_read: 0,
        }
    }

    /// Primera página que hay que pedir. itch.io numera desde 1.
    pub(crate) fn first(&self) -> i64 {
        self.next_page
    }

    pub(crate) fn pages_read(&self) -> i64 {
        self.pages_read
    }

    /// Registra una página recibida y decide si hay que pedir otra.
    pub(crate) fn accept(&mut self, received: usize) -> PageStep {
        self.pages_read += 1;
        if received == 0 {
            return PageStep::Stop(StopReason::Complete);
        }
        self.collected += received;
        if self.collected >= MAX_OWNED_KEYS {
            return PageStep::Stop(StopReason::Capped);
        }
        if self.pages_read >= MAX_PAGES {
            return PageStep::Stop(StopReason::PageLimit);
        }
        self.next_page += 1;
        PageStep::Fetch(self.next_page)
    }
}

// ---------------------------------------------------------------------------
// Análisis de respuestas
// ---------------------------------------------------------------------------

fn parse_profile(bytes: &[u8]) -> AppResult<ItchAccountProfile> {
    let envelope: ProfileEnvelope =
        serde_json::from_slice(bytes).map_err(|_| unexpected_shape())?;
    let user = envelope.user.ok_or_else(unexpected_shape)?;
    let username = user
        .username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(unexpected_shape)?
        .to_string();
    Ok(ItchAccountProfile {
        id: user.id,
        username,
        display_name: user
            .display_name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        url: user.url.as_deref().and_then(sanitize_profile_url),
    })
}

fn parse_owned_keys(bytes: &[u8]) -> AppResult<Vec<WireOwnedKey>> {
    let page: WireOwnedKeysPage = serde_json::from_slice(bytes).map_err(|_| unexpected_shape())?;
    Ok(page.owned_keys)
}

fn unexpected_shape() -> AppError {
    AppError::new(
        "itch_response",
        "itch.io respondió con un formato que Vindexa no reconoce. Vuelve a intentarlo más tarde.",
    )
}

/// Extrae el mensaje de error de itch.io si el cuerpo lo trae.
///
/// El texto de itch.io está en inglés y no se enseña: sirve para distinguir
/// «clave inválida» de otros fallos y elegir el mensaje propio en español.
fn wire_error_codes(bytes: &[u8]) -> Vec<String> {
    serde_json::from_slice::<WireErrors>(bytes)
        .map(|value| value.errors)
        .unwrap_or_default()
}

/// Acepta una carátula sólo si es una URL `https` servida por itch.io.
fn sanitize_cover_url(value: &str) -> Option<String> {
    let parsed = Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    let allowed = COVER_HOSTS
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")));
    if !allowed {
        return None;
    }
    Some(parsed.to_string())
}

fn sanitize_profile_url(value: &str) -> Option<String> {
    let parsed = Url::parse(value.trim()).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "itch.io" && !host.ends_with(".itch.io") {
        return None;
    }
    Some(parsed.to_string())
}

/// Separa lo que es un juego de lo que no, y explica cada descarte.
///
/// El criterio es la clasificación que eligió quien publicó la página. Es el
/// único dato de itch.io que dice qué es cada cosa, y quien lo escribió es quien
/// mejor lo sabe.
fn select_games(keys: Vec<WireOwnedKey>) -> (Vec<ItchLibraryEntry>, Vec<ItchSkipGroup>) {
    let mut entries: Vec<ItchLibraryEntry> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();

    for key in keys {
        let Some(game) = key.game else {
            *skipped.entry(SKIP_NO_PAGE.to_string()).or_default() += 1;
            continue;
        };
        let classification = game.classification.unwrap_or_default();
        if classification != "game" {
            *skipped.entry(classification).or_default() += 1;
            continue;
        }
        let Some(title) = game.title.as_deref().and_then(sanitize_title) else {
            *skipped.entry(SKIP_NO_TITLE.to_string()).or_default() += 1;
            continue;
        };
        let external_id = game.id.to_string();
        if !is_valid_external_id(&external_id) {
            *skipped.entry(SKIP_NO_TITLE.to_string()).or_default() += 1;
            continue;
        }
        if seen.contains(&external_id) {
            // Dos claves de la misma ficha —una compra y un regalo, por
            // ejemplo— son un solo juego, no dos.
            *skipped.entry(SKIP_REPEATED.to_string()).or_default() += 1;
            continue;
        }
        seen.push(external_id.clone());
        entries.push(ItchLibraryEntry {
            external_id,
            title,
            cover_url: game
                .still_cover_url
                .as_deref()
                .or(game.cover_url.as_deref())
                .and_then(sanitize_cover_url),
            acquired_at: key
                .created_at
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        });
    }

    let groups = skipped
        .into_iter()
        .map(|(reason, count)| ItchSkipGroup {
            label: skip_label(&reason),
            reason,
            count,
        })
        .collect();
    (entries, groups)
}

/// El identificador de itch.io es el número de la ficha. No se acepta otra cosa.
fn is_valid_external_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value != "0"
}

// ---------------------------------------------------------------------------
// Red
// ---------------------------------------------------------------------------

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

/// Pide un recurso de la API y devuelve su cuerpo, con todos los topes puestos.
///
/// La clave viaja sólo en la cabecera `Authorization`. No se pone en la URL, que
/// es lo que acaba en registros y en historiales.
async fn get_bytes(endpoint: &str, key: &str, query: &[(&str, String)]) -> AppResult<Vec<u8>> {
    let client = net::client()?;
    throttle().await;
    let mut response = client
        .get(endpoint)
        .bearer_auth(key)
        .query(query)
        .send()
        .await
        .map_err(classify_request_error)?;

    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Err(rate_limited_error(
            response.headers().get(header::RETRY_AFTER),
        ));
    }
    if !status.is_success() {
        let body = collect_body(&mut response).await.unwrap_or_default();
        return Err(classify_status(status, &wire_error_codes(&body)));
    }

    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    if content_type.as_deref() != Some("application/json") {
        return Err(AppError::new(
            "itch_content_type",
            "itch.io devolvió un tipo de contenido inesperado.",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(too_large_error());
    }
    collect_body(&mut response).await
}

async fn collect_body(response: &mut reqwest::Response) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify_request_error)? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(too_large_error());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn too_large_error() -> AppError {
    AppError::new(
        "itch_too_large",
        "La respuesta de itch.io supera el tamaño máximo permitido.",
    )
}

/// Traduce el estado HTTP a un error propio. Cada motivo tiene su mensaje.
fn classify_status(status: StatusCode, errors: &[String]) -> AppError {
    let says_invalid_key = errors
        .iter()
        .any(|value| value.to_ascii_lowercase().contains("key"));
    match status {
        StatusCode::UNAUTHORIZED => AppError::new(
            "itch_invalid_key",
            "itch.io no aceptó la clave. Genera una nueva en los ajustes de tu cuenta y vuelve a pegarla.",
        ),
        StatusCode::FORBIDDEN if says_invalid_key => AppError::new(
            "itch_invalid_key",
            "itch.io no reconoce esta clave. Puede que la hayas revocado: genera una nueva y vuelve a pegarla.",
        ),
        StatusCode::FORBIDDEN => AppError::new(
            "itch_forbidden",
            "Esta clave de itch.io no tiene permiso para leer tu biblioteca. Genera una desde los ajustes de tu cuenta, sin limitar sus permisos.",
        ),
        StatusCode::NOT_FOUND => AppError::new(
            "itch_endpoint",
            "itch.io ya no ofrece el recurso que Vindexa necesita para leer tu biblioteca.",
        ),
        _ if status.is_server_error() => AppError::new(
            "itch_unavailable",
            "itch.io no está respondiendo ahora mismo. Vuelve a intentarlo más tarde.",
        ),
        other => AppError::new(
            "itch_status",
            format!("itch.io respondió con el estado {other}."),
        ),
    }
}

fn rate_limited_error(retry_after: Option<&header::HeaderValue>) -> AppError {
    let message = match retry_after_seconds(retry_after) {
        Some(seconds) => format!(
            "itch.io ha limitado temporalmente las peticiones. Vuelve a intentarlo dentro de {seconds} segundos."
        ),
        None => {
            "itch.io ha limitado temporalmente las peticiones. Espera unos minutos y vuelve a intentarlo."
                .to_string()
        }
    };
    AppError::new("itch_rate_limited", message)
}

/// Lee `Retry-After` en segundos, acotado para que un valor absurdo no se
/// convierta en una espera imposible de explicar.
fn retry_after_seconds(value: Option<&header::HeaderValue>) -> Option<u64> {
    let seconds = value?.to_str().ok()?.trim().parse::<u64>().ok()?;
    Some(seconds.clamp(1, 3_600))
}

fn classify_request_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        return AppError::new(
            "itch_timeout",
            "itch.io no respondió a tiempo. Vuelve a intentarlo.",
        );
    }
    if error.is_connect() {
        return AppError::new(
            "itch_offline",
            "No se pudo conectar con itch.io. Comprueba tu conexión a internet.",
        );
    }
    AppError::new(
        "itch_request",
        "La comunicación con itch.io falló antes de completarse.",
    )
}

// ---------------------------------------------------------------------------
// Operaciones
// ---------------------------------------------------------------------------

/// Comprueba una clave contra `/profile` y devuelve de quién es.
///
/// Es lo primero que se hace al pegarla: así el error de «clave inválida» sale
/// en el momento de guardarla y no media importación después.
pub async fn verify_key(key: &str) -> AppResult<ItchAccountProfile> {
    secrets::validate_api_key(key)?;
    let bytes = get_bytes(PROFILE_ENDPOINT, key, &[]).await?;
    parse_profile(&bytes)
}

/// Lee la biblioteca completa de la cuenta cuya clave está en el llavero.
///
/// No toca la base de datos: separar la red de la escritura evita retener el
/// bloqueo de SQLite mientras se espera a itch.io.
pub async fn fetch_library() -> AppResult<ItchFetch> {
    let key = secrets::load_api_key()?.ok_or_else(missing_key_error)?;
    let profile = verify_key(&key).await?;

    let mut cursor = PageCursor::new();
    let mut page = cursor.first();
    let mut keys: Vec<WireOwnedKey> = Vec::new();
    let stop = loop {
        let bytes = get_bytes(OWNED_KEYS_ENDPOINT, &key, &[("page", page.to_string())]).await?;
        let received = parse_owned_keys(&bytes)?;
        let count = received.len();
        keys.extend(received);
        match cursor.accept(count) {
            PageStep::Fetch(next) => page = next,
            PageStep::Stop(reason) => break reason,
        }
    };

    keys.truncate(MAX_OWNED_KEYS);
    let owned_keys = keys.len();
    let (entries, skipped) = select_games(keys);
    Ok(ItchFetch {
        profile,
        entries,
        owned_keys,
        skipped,
        pages: cursor.pages_read(),
        truncated: stop.truncated(),
    })
}

fn missing_key_error() -> AppError {
    AppError::new(
        "itch_missing_key",
        "Todavía no has guardado tu clave de itch.io en Vindexa.",
    )
}

// ---------------------------------------------------------------------------
// Persistencia
// ---------------------------------------------------------------------------

/// Guarda la biblioteca leída en `external_store_accounts` y `external_games`.
///
/// Reutiliza las tablas de la migración 025 en vez de crear unas paralelas. La
/// escritura no pasa por `stores::db::persist_scan` porque aquella caduca las
/// instalaciones que ya no aparecen en un manifiesto, y en itch.io no hay
/// instalaciones: son claves de descarga que la cuenta posee para siempre.
///
/// # Idempotencia
///
/// Reimportar no duplica —la clave primaria es `(store, external_id)`— y no pisa
/// una corrección manual de emparejado: la columna `match_source` de la
/// migración 027 la protege, igual que en el escaneo de Epic y GOG. Tampoco se
/// borra ninguna fila que itch.io deje de devolver: destruiría esa corrección
/// manual sin necesidad.
pub fn persist_library(
    connection: &mut Connection,
    fetch: &ItchFetch,
) -> AppResult<ItchImportReport> {
    if fetch.entries.len() > MAX_DISCOVERED_GAMES {
        return Err(AppError::validation(
            "La importación de itch.io supera el límite seguro.",
        ));
    }
    let transaction = connection.transaction()?;
    let report = persist_in_transaction(&transaction, fetch)?;
    transaction.commit()?;
    Ok(report)
}

fn persist_in_transaction(
    transaction: &Transaction<'_>,
    fetch: &ItchFetch,
) -> AppResult<ItchImportReport> {
    let index = steam_index(transaction)?;
    let mut added = 0_usize;
    let mut already_present = 0_usize;
    let mut without_cover = 0_usize;

    {
        let mut exists = transaction
            .prepare_cached("SELECT 1 FROM external_games WHERE store = ?1 AND external_id = ?2")?;
        let mut upsert = transaction.prepare_cached(
            "INSERT INTO external_games(
                store, external_id, title, cover_url, header_url, install_path,
                installed, size_on_disk, launch_target, drm_state,
                matched_app_id, match_confidence, match_source, discovered_at
             ) VALUES (?1, ?2, ?3, ?4, NULL, NULL, 0, NULL, NULL, ?5, ?6, ?7, 'automatic',
                       COALESCE(?8, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))
             ON CONFLICT(store, external_id) DO UPDATE SET
                title = excluded.title,
                cover_url = COALESCE(excluded.cover_url, external_games.cover_url),
                -- Una decisión humana sobrevive a la reimportación.
                matched_app_id = CASE
                    WHEN external_games.match_source = 'manual'
                    THEN external_games.matched_app_id
                    ELSE excluded.matched_app_id END,
                match_confidence = CASE
                    WHEN external_games.match_source = 'manual'
                    THEN external_games.match_confidence
                    ELSE excluded.match_confidence END,
                match_source = external_games.match_source,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;

        for entry in &fetch.entries {
            if !is_valid_external_id(&entry.external_id) || entry.title.trim().is_empty() {
                return Err(AppError::validation(
                    "Una entrada de itch.io llegó sin identificador o sin título utilizable.",
                ));
            }
            if entry.cover_url.is_none() {
                without_cover += 1;
            }
            let known = exists
                .exists(params![STORE, entry.external_id])
                .map_err(AppError::from)?;
            if known {
                already_present += 1;
            } else {
                added += 1;
            }
            let decision = index.best_match(&entry.title);
            upsert.execute(params![
                STORE,
                entry.external_id,
                entry.title,
                entry.cover_url,
                DrmState::Unknown.as_str(),
                decision.map(|value| value.app_id),
                decision.map(|value| value.confidence).unwrap_or(0.0),
                entry.acquired_at,
            ])?;
        }
    }

    // Lo importado entra en la biblioteca como los juegos de Epic y de GOG: con
    // su ficha personal, para poder clasificarse, arrastrarse y planificarse.
    crate::stores::db::link_into_library(transaction)?;
    upsert_account(transaction, Some(&fetch.profile), "success", None)?;

    let matched: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM external_games WHERE store = ?1 AND matched_app_id IS NOT NULL",
        [STORE],
        |row| row.get(0),
    )?;

    Ok(ItchImportReport {
        store: STORE.to_string(),
        account: fetch.profile.label().to_string(),
        owned_keys: fetch.owned_keys,
        imported: fetch.entries.len(),
        added,
        already_present,
        without_cover,
        skipped: fetch.skipped.iter().map(|group| group.count).sum(),
        skipped_groups: fetch.skipped.clone(),
        matched: matched.max(0) as usize,
        pages: fetch.pages,
        truncated: fetch.truncated,
    })
}

/// Índice de títulos de Steam para proponer emparejados.
///
/// Repite la consulta que `stores::db` hace para Epic y GOG porque allí es
/// privada. Cuando el trabajo en paralelo sobre ese módulo termine, conviene
/// exponerla y llamarla desde aquí.
fn steam_index(connection: &Connection) -> AppResult<SteamTitleIndex> {
    // Mismo filtro que en `stores::db`: los juegos de otras tiendas viven ya en
    // `games`, y sin acotar aquí uno de itch.io podría emparejarse consigo
    // mismo o con su copia de Epic.
    let mut statement =
        connection.prepare("SELECT app_id, title FROM games WHERE external_store IS NULL")?;
    let candidates = statement
        .query_map([], |row| {
            Ok(MatchCandidate {
                app_id: row.get(0)?,
                title: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SteamTitleIndex::build(&candidates))
}

/// Escribe el estado de la cuenta de itch.io.
///
/// `detected_root` se queda vacío a propósito: itch.io no se lee de ninguna
/// carpeta de esta máquina, y rellenar ese campo con otra cosa sería mentir
/// sobre lo que significa.
fn upsert_account(
    transaction: &Transaction<'_>,
    profile: Option<&ItchAccountProfile>,
    status: &str,
    error: Option<&AppError>,
) -> AppResult<()> {
    transaction.execute(
        "INSERT INTO external_store_accounts(
            store, display_name, detected_root, linked, last_scan_at, last_scan_status,
            last_scan_error_code, last_scan_error_message, game_count
         ) VALUES (
            ?1, ?2, NULL, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?4, ?5, ?6,
            (SELECT COUNT(*) FROM external_games WHERE store = ?1)
         )
         ON CONFLICT(store) DO UPDATE SET
            display_name = COALESCE(excluded.display_name, external_store_accounts.display_name),
            linked = excluded.linked,
            last_scan_at = excluded.last_scan_at,
            last_scan_status = excluded.last_scan_status,
            last_scan_error_code = excluded.last_scan_error_code,
            last_scan_error_message = excluded.last_scan_error_message,
            game_count = excluded.game_count,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            STORE,
            profile.map(|value| value.label().to_string()),
            profile.is_some(),
            status,
            error.map(|value| value.code.clone()),
            error.map(|value| value.message.clone()),
        ],
    )?;
    Ok(())
}

/// Estado de la sesión tal y como lo enseña Ajustes.
///
/// `has_key` sólo dice **si** hay clave guardada. La clave no aparece en esta
/// estructura, ni recortada ni con su longitud: nada que salga de aquí permite
/// deducirla.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItchSessionState {
    pub has_key: bool,
    /// El llavero no dejó comprobar si hay clave: permiso denegado, llavero
    /// bloqueado o fallo del sistema.
    ///
    /// No es lo mismo que no tener clave, y confundirlos hacía que la tarjeta
    /// pidiera generar una nueva cuando la que había seguía ahí. Con esto la
    /// pantalla dice lo que pasó y ofrece volver a intentarlo, igual que las
    /// tarjetas de Epic y GOG.
    #[serde(default)]
    pub unreadable: bool,
    /// Cuenta con la que se importó por última vez, si consta.
    pub account: Option<String>,
    pub last_import_at: Option<String>,
    pub last_import_status: Option<String>,
    pub last_import_error_message: Option<String>,
    pub game_count: i64,
}

/// Reúne lo que Ajustes necesita saber: si hay clave y qué pasó la última vez.
///
/// Un llavero que no deja leer no tumba la tarjeta: se enseña lo que sí se sabe
/// —lo de la base de datos— y se dice que la clave no se pudo comprobar.
pub fn session_state(connection: &Connection) -> AppResult<ItchSessionState> {
    match secrets::has_api_key() {
        Ok(has_key) => read_session_state(connection, has_key),
        Err(_) => Ok(ItchSessionState {
            unreadable: true,
            ..read_session_state(connection, false)?
        }),
    }
}

/// La parte que sólo depende de la base de datos.
///
/// Se separa para poder comprobarla sin tocar el llavero real de la máquina.
fn read_session_state(connection: &Connection, has_key: bool) -> AppResult<ItchSessionState> {
    let mut statement = connection.prepare(
        "SELECT display_name, last_scan_at, last_scan_status, last_scan_error_message, game_count
           FROM external_store_accounts
          WHERE store = ?1",
    )?;
    let mut rows = statement.query([STORE])?;
    let Some(row) = rows.next()? else {
        // Nunca se ha importado nada: eso no es un error, es un estado.
        return Ok(ItchSessionState {
            has_key,
            unreadable: false,
            account: None,
            last_import_at: None,
            last_import_status: None,
            last_import_error_message: None,
            game_count: 0,
        });
    };
    Ok(ItchSessionState {
        has_key,
        unreadable: false,
        account: row.get(0)?,
        last_import_at: row.get(1)?,
        last_import_status: row.get(2)?,
        last_import_error_message: row.get(3)?,
        game_count: row.get(4)?,
    })
}

/// Deja constancia de una importación fallida, para que Ajustes pueda decir qué
/// pasó en vez de dejar la tarjeta muda.
pub fn record_failure(connection: &mut Connection, error: &AppError) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let profile = current_account_label(&transaction)?;
    transaction.execute(
        "INSERT INTO external_store_accounts(
            store, display_name, detected_root, linked, last_scan_at, last_scan_status,
            last_scan_error_code, last_scan_error_message, game_count
         ) VALUES (
            ?1, ?2, NULL, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), 'failed', ?4, ?5,
            (SELECT COUNT(*) FROM external_games WHERE store = ?1)
         )
         ON CONFLICT(store) DO UPDATE SET
            last_scan_at = excluded.last_scan_at,
            last_scan_status = 'failed',
            last_scan_error_code = excluded.last_scan_error_code,
            last_scan_error_message = excluded.last_scan_error_message,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![
            STORE,
            profile,
            secrets::has_api_key().unwrap_or(false),
            error.code,
            error.message,
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn current_account_label(transaction: &Transaction<'_>) -> AppResult<Option<String>> {
    let label = transaction
        .query_row(
            "SELECT display_name FROM external_store_accounts WHERE store = ?1",
            [STORE],
            |row| row.get::<_, Option<String>>(0),
        )
        .unwrap_or(None);
    Ok(label)
}

/// Cierra la sesión: borra la clave del llavero y marca la cuenta como no
/// vinculada.
///
/// **Conserva los juegos ya importados.** Son la biblioteca de la persona, y
/// borrarlos destruiría de paso los emparejados que hubiera corregido a mano.
/// Para eso está [`forget`].
pub fn sign_out(connection: &mut Connection) -> AppResult<()> {
    secrets::delete_api_key()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE external_store_accounts
            SET linked = 0,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE store = ?1",
        [STORE],
    )?;
    transaction.commit()?;
    Ok(())
}

/// Cierra la sesión y borra además todo lo importado de itch.io.
///
/// Es una operación destructiva y explícita: la interfaz debe pedirla aparte de
/// [`sign_out`], nunca como efecto secundario de cerrar sesión.
pub fn forget(connection: &mut Connection) -> AppResult<usize> {
    secrets::delete_api_key()?;
    let transaction = connection.transaction()?;
    let removed = transaction.execute("DELETE FROM external_games WHERE store = ?1", [STORE])?;
    transaction.execute(
        "DELETE FROM external_store_accounts WHERE store = ?1",
        [STORE],
    )?;
    transaction.commit()?;
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Pruebas
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    /// Esquema mínimo para probar la persistencia.
    ///
    /// El `CHECK` de la columna `store` incluye `'itch'`, que es justo lo que
    /// **todavía no** permite la migración 025 en producción. Estas pruebas
    /// documentan el esquema que la importación necesita; mientras no se amplíe
    /// ese `CHECK`, la escritura real fallará aunque el código sea correcto.
    /// El esquema real, aplicando las migraciones.
    ///
    /// Antes esta prueba llevaba su propia copia del esquema escrita a mano, y
    /// cada migración que tocara `games` o `external_games` la dejaba obsoleta
    /// sin que nadie se enterara hasta que fallaba por «error de base de datos»,
    /// que no dice nada. Un esquema copiado es un esquema que se desincroniza.
    fn connection() -> (tempfile::TempDir, Connection) {
        crate::stores::test_support::migrated_database()
    }

    fn profile() -> ItchAccountProfile {
        ItchAccountProfile {
            id: 29789,
            username: "persona".to_string(),
            display_name: Some("Persona Usuaria".to_string()),
            url: Some("https://persona.itch.io/".to_string()),
        }
    }

    fn fetch_with(entries: Vec<ItchLibraryEntry>) -> ItchFetch {
        ItchFetch {
            profile: profile(),
            owned_keys: entries.len(),
            entries,
            skipped: Vec::new(),
            pages: 1,
            truncated: false,
        }
    }

    fn entry(id: &str, title: &str) -> ItchLibraryEntry {
        ItchLibraryEntry {
            external_id: id.to_string(),
            title: title.to_string(),
            cover_url: Some("https://img.itch.zone/portada.png".to_string()),
            acquired_at: Some("2024-01-05T10:00:00Z".to_string()),
        }
    }

    // -- Respuestas de la API ------------------------------------------------

    #[test]
    fn reads_the_profile_that_itch_returns() {
        let body = br#"{"user":{"id":29789,"username":"persona","display_name":"Persona Usuaria",
                        "url":"https://persona.itch.io","press_user":false,"gamer":true}}"#;
        let parsed = parse_profile(body).expect("leer perfil");
        assert_eq!(parsed.id, 29789);
        assert_eq!(parsed.username, "persona");
        assert_eq!(parsed.label(), "Persona Usuaria");
        assert_eq!(parsed.url.as_deref(), Some("https://persona.itch.io/"));
    }

    #[test]
    fn a_corrupt_response_is_a_named_error_and_not_a_panic() {
        let error = parse_profile(b"{esto no es json").expect_err("debe fallar");
        assert_eq!(error.code, "itch_response");

        let error = parse_profile(br#"{"user":{"id":1}}"#).expect_err("sin nombre de cuenta");
        assert_eq!(error.code, "itch_response");

        let error = parse_owned_keys(b"<html>mantenimiento</html>").expect_err("debe fallar");
        assert_eq!(error.code, "itch_response");
    }

    #[test]
    fn la_pagina_que_cierra_el_recorrido_llega_como_tabla_vacia() {
        // Cuerpo copiado tal cual de la API el 2026-08-19: al agotarse las
        // claves, itch.io serializa `owned_keys` como objeto, no como lista.
        // Interpretarlo como fallo tiraba la importación entera en su último
        // paso, con todas las páginas anteriores ya leídas.
        let keys = parse_owned_keys(br#"{"owned_keys":{},"page":2,"per_page":50}"#)
            .expect("una tabla vacía es una página sin claves");
        assert!(keys.is_empty());

        // `null` tampoco es un fallo de formato.
        let keys = parse_owned_keys(br#"{"owned_keys":null,"page":2}"#).expect("nulo es vacío");
        assert!(keys.is_empty());

        // Una tabla con contenido llega indexada por posición; las claves
        // siguen siendo claves.
        let keys = parse_owned_keys(
            br#"{"owned_keys":{"1":{"game":{"id":7,"title":"Uno","classification":"game"}}}}"#,
        )
        .expect("tabla indexada");
        assert_eq!(keys.len(), 1);

        // Y lo que no es ni lista ni tabla sigue siendo un formato inesperado.
        let error = parse_owned_keys(br#"{"owned_keys":"nada"}"#).expect_err("debe fallar");
        assert_eq!(error.code, "itch_response");
    }

    #[test]
    fn an_empty_library_is_not_an_error() {
        let keys = parse_owned_keys(br#"{"page":1,"per_page":50,"owned_keys":[]}"#)
            .expect("leer página vacía");
        assert!(keys.is_empty());

        let (entries, skipped) = select_games(keys);
        assert!(entries.is_empty());
        assert!(skipped.is_empty());
    }

    #[test]
    fn reads_the_snake_case_that_itch_puts_on_the_wire() {
        // itch.io documenta que sus respuestas son snake_case. El cliente
        // oficial en Go las pasa a camelCase después de recibirlas; aquí se
        // leen tal cual llegan.
        let body = br#"{"page":1,"per_page":50,"owned_keys":[
            {"id":11,"game_id":7,"created_at":"2024-01-05T10:00:00Z","owner_id":29789,
             "game":{"id":7,"title":"Un Juego","classification":"game",
                     "cover_url":"https://img.itch.zone/portada.png",
                     "url":"https://alguien.itch.io/un-juego"}}]}"#;
        let keys = parse_owned_keys(body).expect("leer página");
        let (entries, skipped) = select_games(keys);
        assert!(skipped.is_empty());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].external_id, "7");
        assert_eq!(entries[0].title, "Un Juego");
        assert_eq!(
            entries[0].cover_url.as_deref(),
            Some("https://img.itch.zone/portada.png")
        );
        assert_eq!(
            entries[0].acquired_at.as_deref(),
            Some("2024-01-05T10:00:00Z")
        );
    }

    // -- Qué entra y qué no --------------------------------------------------

    #[test]
    fn only_games_enter_and_everything_else_is_explained() {
        let body = r#"{"owned_keys":[
            {"game":{"id":1,"title":"Juego","classification":"game"}},
            {"game":{"id":2,"title":"Motor de tiles","classification":"tool"}},
            {"game":{"id":3,"title":"Pack de sprites","classification":"assets"}},
            {"game":{"id":4,"title":"Banda sonora","classification":"soundtrack"}},
            {"game":{"id":5,"title":"Novela gráfica","classification":"comic"}},
            {"game":{"id":6,"title":"Manual","classification":"book"}},
            {"game":{"id":7,"title":"Mod","classification":"game_mod"}},
            {"game":{"id":8,"title":"Juego de mesa","classification":"physical_game"}},
            {"game":{"id":9,"title":"Cosa","classification":"other"}},
            {"id":99}
        ]}"#
        .as_bytes();
        let keys = parse_owned_keys(body).expect("leer página");
        let (entries, skipped) = select_games(keys);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Juego");

        let total: usize = skipped.iter().map(|group| group.count).sum();
        assert_eq!(total, 9);

        let motivos: Vec<&str> = skipped.iter().map(|group| group.reason.as_str()).collect();
        for esperado in [
            "tool",
            "assets",
            "soundtrack",
            "comic",
            "book",
            "game_mod",
            "physical_game",
            "other",
            SKIP_NO_PAGE,
        ] {
            assert!(motivos.contains(&esperado), "falta el motivo {esperado}");
        }
        // Cada motivo llega traducido: la interfaz nunca enseña el valor crudo.
        let herramientas = skipped
            .iter()
            .find(|group| group.reason == "tool")
            .expect("grupo de herramientas");
        assert_eq!(herramientas.label, "Herramientas y programas");
    }

    #[test]
    fn an_unknown_classification_is_reported_verbatim() {
        // Si itch.io añade una categoría, se dice cuál es en vez de inventarle
        // una etiqueta que la disfrace.
        let keys = parse_owned_keys(
            br#"{"owned_keys":[{"game":{"id":1,"title":"Algo","classification":"holograma"}}]}"#,
        )
        .expect("leer página");
        let (entries, skipped) = select_games(keys);
        assert!(entries.is_empty());
        assert_eq!(skipped[0].reason, "holograma");
        assert_eq!(skipped[0].label, "holograma");
    }

    #[test]
    fn two_keys_for_the_same_page_are_a_single_game() {
        let keys = parse_owned_keys(
            br#"{"owned_keys":[
                {"id":1,"game":{"id":42,"title":"Juego","classification":"game"}},
                {"id":2,"game":{"id":42,"title":"Juego","classification":"game"}}]}"#,
        )
        .expect("leer página");
        let (entries, skipped) = select_games(keys);
        assert_eq!(entries.len(), 1);
        assert_eq!(skipped[0].reason, SKIP_REPEATED);
        assert_eq!(skipped[0].label, "Claves repetidas de la misma ficha");
    }

    #[test]
    fn a_cover_is_only_kept_when_itch_really_serves_it() {
        assert_eq!(
            sanitize_cover_url("https://img.itch.zone/portada.png").as_deref(),
            Some("https://img.itch.zone/portada.png")
        );
        // Ni http, ni otro anfitrión, ni un sufijo que sólo lo aparente.
        assert!(sanitize_cover_url("http://img.itch.zone/portada.png").is_none());
        assert!(sanitize_cover_url("https://ejemplo.invalid/portada.png").is_none());
        assert!(sanitize_cover_url("https://itch.zone.ejemplo.invalid/x.png").is_none());
        assert!(sanitize_cover_url("no es una url").is_none());
    }

    #[test]
    fn a_game_without_cover_enters_without_inventing_one() {
        let keys = parse_owned_keys(
            br#"{"owned_keys":[{"game":{"id":5,"title":"Sin arte","classification":"game"}}]}"#,
        )
        .expect("leer página");
        let (entries, _) = select_games(keys);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cover_url, None);
    }

    // -- Paginación ----------------------------------------------------------

    #[test]
    fn walks_several_pages_and_stops_on_the_empty_one() {
        let mut cursor = PageCursor::new();
        assert_eq!(cursor.first(), 1);
        assert_eq!(cursor.accept(50), PageStep::Fetch(2));
        assert_eq!(cursor.accept(50), PageStep::Fetch(3));
        assert_eq!(cursor.accept(7), PageStep::Fetch(4));
        assert_eq!(cursor.accept(0), PageStep::Stop(StopReason::Complete));
        assert_eq!(cursor.pages_read(), 4);
        assert!(!StopReason::Complete.truncated());
    }

    #[test]
    fn stops_at_the_key_cap_and_says_that_it_truncated() {
        let mut cursor = PageCursor::new();
        let mut steps = 0;
        loop {
            steps += 1;
            match cursor.accept(1_000) {
                PageStep::Fetch(_) => assert!(steps < 100, "no debería seguir indefinidamente"),
                PageStep::Stop(reason) => {
                    assert_eq!(reason, StopReason::Capped);
                    assert!(reason.truncated());
                    break;
                }
            }
        }
        assert_eq!(steps, 5, "5 páginas de 1000 alcanzan el tope de 5000");
    }

    #[test]
    fn stops_at_the_page_cap_when_the_pages_never_empty() {
        let mut cursor = PageCursor::new();
        let mut reason = None;
        for _ in 0..(MAX_PAGES + 10) {
            if let PageStep::Stop(stop) = cursor.accept(1) {
                reason = Some(stop);
                break;
            }
        }
        assert_eq!(reason, Some(StopReason::PageLimit));
        assert_eq!(cursor.pages_read(), MAX_PAGES);
    }

    // -- Errores -------------------------------------------------------------

    #[test]
    fn every_failure_has_its_own_message_in_spanish() {
        let invalida = classify_status(StatusCode::FORBIDDEN, &["invalid key".to_string()]);
        assert_eq!(invalida.code, "itch_invalid_key");

        let sin_auth = classify_status(StatusCode::UNAUTHORIZED, &[]);
        assert_eq!(sin_auth.code, "itch_invalid_key");

        let sin_permiso = classify_status(StatusCode::FORBIDDEN, &[]);
        assert_eq!(sin_permiso.code, "itch_forbidden");

        let caido = classify_status(StatusCode::BAD_GATEWAY, &[]);
        assert_eq!(caido.code, "itch_unavailable");

        let raro = classify_status(StatusCode::IM_A_TEAPOT, &[]);
        assert_eq!(raro.code, "itch_status");

        // Ninguno repite mensaje: la persona sabe qué le ha pasado.
        let mensajes = [invalida, sin_permiso, caido, raro]
            .iter()
            .map(|error| error.message.clone())
            .collect::<Vec<_>>();
        for (indice, mensaje) in mensajes.iter().enumerate() {
            assert!(!mensaje.is_empty());
            assert!(
                !mensajes[indice + 1..].contains(mensaje),
                "dos errores comparten mensaje"
            );
        }
    }

    #[test]
    fn the_rate_limit_turns_retry_after_into_its_own_error() {
        let con_cabecera = rate_limited_error(Some(&HeaderValue::from_static("90")));
        assert_eq!(con_cabecera.code, "itch_rate_limited");
        assert!(con_cabecera.message.contains("90 segundos"));

        // Un valor absurdo se acota en vez de prometer una espera imposible.
        assert_eq!(
            retry_after_seconds(Some(&HeaderValue::from_static("999999"))),
            Some(3_600)
        );
        assert_eq!(
            retry_after_seconds(Some(&HeaderValue::from_static(
                "Wed, 21 Oct 2026 07:28:00 GMT"
            ))),
            None,
            "la forma de fecha de Retry-After no se adivina: se ignora"
        );

        let sin_cabecera = rate_limited_error(None);
        assert_eq!(sin_cabecera.code, "itch_rate_limited");
        assert!(!sin_cabecera.message.contains("segundos."));
    }

    #[test]
    fn no_error_message_ever_carries_the_key() {
        const CLAVE: &str = "CLAVEQUENODEBESALIRNUNCA";
        let errores = [
            classify_status(StatusCode::FORBIDDEN, &["invalid key".to_string()]),
            classify_status(StatusCode::UNAUTHORIZED, &[]),
            rate_limited_error(Some(&HeaderValue::from_static("30"))),
            missing_key_error(),
            unexpected_shape(),
            too_large_error(),
        ];
        for error in errores {
            assert!(!error.message.contains(CLAVE));
            assert!(!error.message.to_lowercase().contains("bearer"));
        }
    }

    #[test]
    fn the_identifier_is_always_the_number_of_the_itch_page() {
        assert!(is_valid_external_id("42"));
        assert!(!is_valid_external_id(""));
        assert!(!is_valid_external_id("0"));
        assert!(!is_valid_external_id("42a"));
        assert!(!is_valid_external_id("../../etc/passwd"));
        assert!(!is_valid_external_id(&"9".repeat(21)));
    }

    // -- Persistencia --------------------------------------------------------

    #[test]
    fn the_first_import_counts_everything_that_entered() {
        let (_directorio, mut connection) = connection();
        let fetch = ItchFetch {
            profile: profile(),
            entries: vec![entry("1", "Primero"), entry("2", "Segundo")],
            owned_keys: 5,
            skipped: vec![ItchSkipGroup {
                reason: "tool".to_string(),
                label: "Herramientas y programas".to_string(),
                count: 3,
            }],
            pages: 1,
            truncated: false,
        };

        let report = persist_library(&mut connection, &fetch).expect("importar");
        assert_eq!(report.store, "itch");
        assert_eq!(report.account, "Persona Usuaria");
        assert_eq!(report.owned_keys, 5);
        assert_eq!(report.imported, 2);
        assert_eq!(report.added, 2);
        assert_eq!(report.already_present, 0);
        assert_eq!(report.skipped, 3);
        assert_eq!(report.skipped_groups[0].label, "Herramientas y programas");

        let total: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM external_games WHERE store = 'itch'",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(total, 2);

        let (vinculada, cuenta): (bool, Option<String>) = connection
            .query_row(
                "SELECT linked, display_name FROM external_store_accounts WHERE store = 'itch'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer cuenta");
        assert!(vinculada);
        assert_eq!(cuenta.as_deref(), Some("Persona Usuaria"));
    }

    #[test]
    fn reimporting_neither_duplicates_nor_overwrites_a_manual_decision() {
        let (_directorio, mut connection) = connection();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (500, 'Otro Juego')",
                [],
            )
            .expect("sembrar Steam");

        let primera = fetch_with(vec![entry("1", "Primero"), entry("2", "Segundo")]);
        let report = persist_library(&mut connection, &primera).expect("primera importación");
        assert_eq!(report.added, 2);

        // Una persona corrige el emparejado a mano.
        connection
            .execute(
                "UPDATE external_games
                    SET matched_app_id = 500, match_confidence = 1.0, match_source = 'manual'
                  WHERE store = 'itch' AND external_id = '1'",
                [],
            )
            .expect("corregir a mano");

        // Se reimporta: mismo contenido más un juego nuevo.
        let segunda = fetch_with(vec![
            entry("1", "Primero"),
            entry("2", "Segundo"),
            entry("3", "Tercero"),
        ]);
        let report = persist_library(&mut connection, &segunda).expect("segunda importación");
        assert_eq!(report.added, 1, "sólo el juego nuevo cuenta como añadido");
        assert_eq!(report.already_present, 2);

        let total: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM external_games WHERE store = 'itch'",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(total, 3, "reimportar no duplica");

        let (app_id, fuente): (Option<u32>, String) = connection
            .query_row(
                "SELECT matched_app_id, match_source FROM external_games
                  WHERE store = 'itch' AND external_id = '1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer emparejado");
        assert_eq!(app_id, Some(500), "la decisión humana sobrevive");
        assert_eq!(fuente, "manual");
    }

    #[test]
    fn reimporting_keeps_the_cover_when_itch_stops_publishing_one() {
        let (_directorio, mut connection) = connection();
        persist_library(&mut connection, &fetch_with(vec![entry("1", "Primero")]))
            .expect("primera importación");

        let sin_arte = ItchLibraryEntry {
            cover_url: None,
            ..entry("1", "Primero renombrado")
        };
        let report = persist_library(&mut connection, &fetch_with(vec![sin_arte]))
            .expect("segunda importación");
        assert_eq!(report.without_cover, 1);

        let (titulo, portada): (String, Option<String>) = connection
            .query_row(
                "SELECT title, cover_url FROM external_games WHERE store = 'itch' AND external_id = '1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer fila");
        assert_eq!(titulo, "Primero renombrado", "el título sí se actualiza");
        assert_eq!(
            portada.as_deref(),
            Some("https://img.itch.zone/portada.png"),
            "no se borra el arte que ya había"
        );
    }

    #[test]
    fn an_empty_library_leaves_the_account_linked_and_says_zero() {
        let (_directorio, mut connection) = connection();
        let report = persist_library(&mut connection, &fetch_with(Vec::new())).expect("importar");
        assert_eq!(report.imported, 0);
        assert_eq!(report.added, 0);
        assert_eq!(report.matched, 0);

        let (vinculada, estado): (bool, String) = connection
            .query_row(
                "SELECT linked, last_scan_status FROM external_store_accounts WHERE store = 'itch'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer cuenta");
        assert!(vinculada, "una biblioteca vacía no es un fallo de sesión");
        assert_eq!(estado, "success");
    }

    #[test]
    fn a_matching_steam_title_is_proposed_but_never_forced() {
        let (_directorio, mut connection) = connection();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (77, 'Celeste')",
                [],
            )
            .expect("sembrar Steam");

        let fetch = fetch_with(vec![entry("1", "Celeste"), entry("2", "Algo Distinto")]);
        let report = persist_library(&mut connection, &fetch).expect("importar");
        assert_eq!(report.matched, 1);

        let fuente: String = connection
            .query_row(
                "SELECT match_source FROM external_games WHERE store = 'itch' AND external_id = '1'",
                [],
                |row| row.get(0),
            )
            .expect("leer fuente");
        assert_eq!(
            fuente, "automatic",
            "la propuesta nunca se marca como humana"
        );
    }

    #[test]
    fn a_failed_import_is_recorded_without_touching_the_library() {
        let (_directorio, mut connection) = connection();
        persist_library(&mut connection, &fetch_with(vec![entry("1", "Primero")]))
            .expect("importar");

        let error = AppError::new("itch_offline", "No se pudo conectar con itch.io.");
        record_failure(&mut connection, &error).expect("registrar fallo");

        let (estado, codigo): (String, Option<String>) = connection
            .query_row(
                "SELECT last_scan_status, last_scan_error_code
                   FROM external_store_accounts WHERE store = 'itch'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer cuenta");
        assert_eq!(estado, "failed");
        assert_eq!(codigo.as_deref(), Some("itch_offline"));

        let total: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM external_games WHERE store = 'itch'",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(total, 1, "un fallo de red no borra la biblioteca de ayer");
    }

    #[test]
    fn the_session_state_says_what_happened_without_naming_the_key() {
        let (_directorio, mut connection) = connection();

        // Antes de importar nada: no es un error, es «todavía nada».
        let virgen = read_session_state(&connection, false).expect("estado inicial");
        assert_eq!(
            virgen,
            ItchSessionState {
                has_key: false,
                unreadable: false,
                account: None,
                last_import_at: None,
                last_import_status: None,
                last_import_error_message: None,
                game_count: 0,
            }
        );

        persist_library(&mut connection, &fetch_with(vec![entry("1", "Primero")]))
            .expect("importar");
        let despues = read_session_state(&connection, true).expect("estado tras importar");
        assert!(despues.has_key);
        assert_eq!(despues.account.as_deref(), Some("Persona Usuaria"));
        assert_eq!(despues.last_import_status.as_deref(), Some("success"));
        assert_eq!(despues.game_count, 1);
        assert!(despues.last_import_at.is_some());
        // Y «no se pudo comprobar» es un estado aparte de «no hay clave»: la
        // tarjeta pedía generar otra cuando la que había seguía guardada.
        assert!(!despues.unreadable);

        // Lo que viaja a la interfaz no contiene ni rastro de la clave.
        let serializado = serde_json::to_string(&despues).expect("serializar estado");
        assert!(serializado.contains("\"hasKey\":true"));
        assert!(!serializado.to_lowercase().contains("apikey"));
        assert!(!serializado.to_lowercase().contains("token"));
    }

    #[test]
    fn an_entry_with_an_impossible_identifier_stops_the_import() {
        let (_directorio, mut connection) = connection();
        let fetch = fetch_with(vec![ItchLibraryEntry {
            external_id: "../../etc".to_string(),
            ..entry("1", "Primero")
        }]);
        let error = persist_library(&mut connection, &fetch).expect_err("debe rechazarse");
        assert_eq!(error.code, "validation");
    }
}
