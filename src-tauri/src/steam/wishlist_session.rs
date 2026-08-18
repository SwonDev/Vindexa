//! Lista de deseados leída desde la sesión abierta en el navegador integrado.
//!
//! # El problema que resuelve
//!
//! [`super::wishlist`] pide la lista a `IWishlistService/GetWishlist/v1` con
//! sólo el SteamID64. Ese servicio respeta la privacidad del perfil: si está en
//! «Sólo amigos» o en privado devuelve `{"response":{}}` y no hay clave, ni
//! cabecera, ni truco que lo cambie desde fuera. Comprobado el 18-08-2026 con
//! `IWishlistService/GetWishlistSortedFiltered/v1` y con
//! `IWishlistService/GetWishlistItemCount/v1`: los tres callan igual.
//!
//! Cuando la persona ya ha iniciado sesión en Steam dentro del navegador de
//! Vindexa, sin embargo, es ella misma mirando su propia lista, y entonces la
//! privacidad del perfil deja de ser un obstáculo: el servidor de Steam le
//! **renderiza** su lista porque reconoce su sesión. Este módulo lee esa página
//! ya renderizada. No pide credenciales, no las guarda y no las mira.
//!
//! # Dónde vive el dato hoy (medido el 18-08-2026)
//!
//! La página de deseados es una aplicación React servida desde el servidor. El
//! HTML **no** trae la lista en una variable suelta —`g_rgWishlistData` ya no
//! existe— y el antiguo `…/wishlistdata/?p=0` responde `302` hacia la portada
//! de la tienda, así que tampoco sirve.
//!
//! Lo que sí trae es la caché de peticiones que la aplicación deja hidratada:
//!
//! ```text
//! window.SSR.renderContext = JSON.parse("…")
//!   └── queryData            (cadena JSON con la caché deshidratada)
//!         └── queries[]
//!               ├── queryKey ["WishlistSortedFiltered", "<steamid64>", 0, {…}, null]
//!               │   state.data { steamid, items: [{ appid, priority, date_added, category_ids }] }
//!               └── queryKey ["wishlistitemcount", "<steamid64>", null]
//!                   state.data <número>
//! ```
//!
//! Dos consecuencias prácticas:
//!
//! - **No hay paginación que recorrer.** `items` llega entero en la primera
//!   carga; lo que se pagina es el adorno (nombre, precio, arte), que Steam
//!   rellena por tandas conforme se hace *scroll*. Vindexa no necesita ese
//!   adorno: los nombres los resuelve aparte y el arte lo deriva del AppID.
//! - **`items` viene filtrado por los filtros guardados de la propia lista.**
//!   En la cuenta de prueba `GetWishlistItemCount` decía 41 y `items` traía 39.
//!   Por eso se lee también el recuento y la diferencia se informa en vez de
//!   dejar que la persona crea que ha importado todo.
//!
//! El HTML de la página distingue además tres situaciones que conviene no
//! confundir, y que este módulo separa con códigos de error propios:
//!
//! | Situación | Señal en la página |
//! | --- | --- |
//! | Sin sesión iniciada | `loaderData[0].steamid == "0"` y no hay consulta de deseados |
//! | Steam limitando peticiones | `loaderData[1].error == "RateLimit"` |
//! | Sesión válida | existe la consulta `WishlistSortedFiltered` con sus `items` |
//!
//! # Qué se le pide a la página y qué no
//!
//! El guion de [`READ_WISHLIST_SCRIPT`] **sólo lee**. No usa `fetch`, ni
//! `XMLHttpRequest`, ni `document.cookie`, ni `localStorage`, ni ninguna forma
//! de sacar algo de la página: devuelve una cadena JSON y nada más. Las cookies
//! de sesión se quedan donde estaban, en el almacén del webview.
//!
//! Antes de evaluarlo se comprueba en Rust que la ventana está en la página que
//! se espera, el propio guion vuelve a comprobarlo desde dentro, y lo que
//! devuelve se valida una tercera vez —incluida la URL que dice haber leído—
//! antes de tocar la base de datos.

// Nada de este módulo tiene todavía quien lo llame: el comando que lo expone y
// el botón de la barra del navegador viven en archivos de otro agente y viajan
// como diff en el informe. Mismo criterio que `store_window::open_store_home`.
#![allow(dead_code)]

use crate::browser::{session, stores};
use crate::db::ImportedWishlistGame;
use crate::error::{AppError, AppResult};
use crate::steam::wishlist::SteamWishlistItem;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, Url, WebviewWindow};

/// Host —y único host— del que se acepta una lista de deseados.
const WISHLIST_HOST: &str = "store.steampowered.com";

/// Página propia de deseados. Estando la sesión iniciada, Steam la resuelve a
/// la lista de quien la pide sin necesidad de conocer su SteamID64.
const WISHLIST_URL: &str = "https://store.steampowered.com/wishlist/";

/// Tope de juegos que se aceptan de una sola lectura.
///
/// La lista de deseados de Steam admite 2000 juegos; el doble deja margen sin
/// abrir la puerta a una respuesta desmedida fabricada por una página hostil.
const MAX_ITEMS: usize = 4_000;

/// Tope de caracteres de la respuesta del guion.
const MAX_PAYLOAD_BYTES: usize = 2 * 1024 * 1024;

/// Margen para que la página de deseados termine de cargarse.
const LOAD_TIMEOUT: Duration = Duration::from_secs(40);

/// Cada cuánto se comprueba si la carga ha terminado.
const LOAD_POLL: Duration = Duration::from_millis(250);

/// Cuánto se espera a que la navegación pedida produzca una carga nueva.
const FRESH_LOAD_GRACE: Duration = Duration::from_secs(5);

/// Margen para que el webview conteste a la evaluación del guion.
const EVAL_TIMEOUT: Duration = Duration::from_secs(20);

/// Fecha de alta más antigua que se acepta de la página.
///
/// Steam abrió en septiembre de 2003; nada anterior a 2003-01-01 puede ser una
/// fecha real de la lista de deseados, así que se descarta en vez de guardarla.
const MIN_ADDED_AT: i64 = 1_041_379_200;

/// Margen por delante del reloj: una marca futura no describe nada.
const MAX_ADDED_AT_SKEW: i64 = 60 * 60 * 24;

/// Lista de deseados tal y como la publica la página ya autenticada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserWishlist {
    /// SteamID64 al que pertenece la lista, según la propia página.
    pub steam_id: String,
    /// Juegos leídos, ya deduplicados y ordenados por antigüedad.
    pub items: Vec<SteamWishlistItem>,
    /// Juegos que la lista de Steam esconde con sus propios filtros.
    ///
    /// Es la diferencia entre el recuento que publica Steam y lo que la página
    /// llegó a mostrar. Se informa en vez de silenciarse: una importación que
    /// deja fuera diez juegos sin decirlo miente por omisión.
    pub hidden_by_filters: usize,
}

/// Resultado completo de importar la lista desde el navegador.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserWishlistImportResult {
    pub report: crate::db::WishlistImportReport,
    /// Cuenta cuya lista se ha leído, según la página.
    pub steam_id: String,
    /// Juegos cuyo nombre no pudo resolverse en la tienda.
    pub titles_unresolved: usize,
    /// Juegos que los filtros de la propia lista de Steam dejaron fuera.
    pub hidden_by_filters: usize,
}

/// Lo que ocurre al añadir a deseados el juego que se está viendo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreWishlistAddition {
    pub app_id: u32,
    pub title: String,
    /// `false` cuando el juego ya estaba en la lista y no se ha tocado.
    pub added: bool,
    /// El juego ya está en la biblioteca: el deseo se guarda igual, pero
    /// conviene decirlo para que nadie crea que se ha añadido un juego nuevo.
    pub in_library: bool,
}

impl StoreWishlistAddition {
    /// Frase para la barra del navegador. Dice qué ha pasado, sin adornos.
    pub fn message(&self) -> String {
        if !self.added {
            return format!("«{}» ya estaba en tus deseados de Vindexa.", self.title);
        }
        if self.in_library {
            return format!(
                "«{}» se ha añadido a tus deseados de Vindexa; ya lo tienes en la biblioteca.",
                self.title
            );
        }
        format!("«{}» se ha añadido a tus deseados de Vindexa.", self.title)
    }
}

/// URL de la página propia de deseados.
pub fn wishlist_page_url() -> Url {
    Url::parse(WISHLIST_URL).expect("la página de deseados es una URL válida")
}

/// ¿Es esta URL una página de lista de deseados de Steam?
///
/// Las tres rutas provienen de la tabla de rutas que la propia aplicación de
/// Steam publica en la página (`/wishlist`, `/wishlist/profiles/<id>` y
/// `/wishlist/id/<vanidad>`). Cualquier otra cosa —incluido un subdominio
/// parecido o un `http:`— se rechaza: es la condición que decide si el guion
/// llega a ejecutarse.
pub fn is_wishlist_page(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if !matches!(url.port(), None | Some(443)) {
        return false;
    }
    if stores::normalized_host(url).as_deref() != Some(WISHLIST_HOST) {
        return false;
    }
    let segments = url
        .path_segments()
        .map(|parts| parts.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    match segments.as_slice() {
        ["wishlist"] => true,
        ["wishlist", "profiles", _] | ["wishlist", "id", _] => true,
        _ => false,
    }
}

/// AppID de una ficha de juego de la tienda de Steam.
///
/// Reconoce `…/app/<id>` y sus variantes con nombre y con idioma
/// (`/app/620/Portal_2/?l=spanish`). Devuelve `None` para cualquier otra
/// página de Steam y para cualquier otra tienda: el catálogo de deseados de
/// Vindexa está indexado por AppID de Steam y no puede guardar otra cosa.
pub fn app_id_from_store_url(url: &Url) -> Option<u32> {
    if url.scheme() != "https" {
        return None;
    }
    if stores::normalized_host(url).as_deref() != Some(WISHLIST_HOST) {
        return None;
    }
    let mut segments = url.path_segments()?.filter(|part| !part.is_empty());
    if segments.next()? != "app" {
        return None;
    }
    let app_id = segments.next()?.parse::<u32>().ok()?;
    (app_id > 0).then_some(app_id)
}

/// Guion que lee la lista de la página ya renderizada.
///
/// Sólo lee y sólo devuelve. No hay una sola llamada de red, ni acceso a
/// cookies, ni a almacenamiento: si alguien añade una, deja de cumplir lo que
/// promete el encabezado de este módulo y la prueba
/// `the_script_only_reads_and_never_reaches_the_network` deja de pasar.
pub const READ_WISHLIST_SCRIPT: &str = r#"
(function () {
  'use strict';
  function fallo(codigo, motivo) {
    return JSON.stringify({ ok: false, error: codigo, reason: motivo || null });
  }

  if (location.protocol !== 'https:') { return fallo('pagina'); }
  var anfitrion = String(location.hostname || '').toLowerCase().replace(/\.+$/, '');
  if (anfitrion !== 'store.steampowered.com') { return fallo('pagina'); }
  if (!/^\/wishlist(\/|$)/.test(location.pathname)) { return fallo('pagina'); }

  var ssr = window.SSR;
  if (!ssr) { return fallo('sin_datos'); }

  var sesion = null;
  var motivo = null;
  try {
    var cargado = ssr.loaderData;
    if (Object.prototype.toString.call(cargado) === '[object Array]') {
      if (typeof cargado[0] === 'string') {
        var raiz = JSON.parse(cargado[0]);
        if (raiz && typeof raiz.steamid === 'string') { sesion = raiz.steamid; }
      }
      if (typeof cargado[1] === 'string') {
        var ruta = JSON.parse(cargado[1]);
        if (ruta && typeof ruta.error === 'string') { motivo = ruta.error; }
      }
    }
  } catch (e) { sesion = null; }

  var contexto = ssr.renderContext;
  if (!contexto || typeof contexto.queryData !== 'string') {
    return fallo(sesion === '0' ? 'sin_sesion' : 'sin_datos', motivo);
  }
  var cache;
  try { cache = JSON.parse(contexto.queryData); } catch (e) { return fallo('sin_datos', motivo); }
  var consultas = cache && cache.queries;
  if (Object.prototype.toString.call(consultas) !== '[object Array]') {
    return fallo('sin_datos', motivo);
  }

  var lista = null;
  var recuento = null;
  for (var i = 0; i < consultas.length; i++) {
    var consulta = consultas[i];
    var clave = consulta && consulta.queryKey;
    if (Object.prototype.toString.call(clave) !== '[object Array]') { continue; }
    var datos = consulta.state && consulta.state.data;
    if (clave[0] === 'WishlistSortedFiltered' && datos &&
        Object.prototype.toString.call(datos.items) === '[object Array]') {
      lista = datos;
    } else if (clave[0] === 'wishlistitemcount' && typeof datos === 'number') {
      recuento = datos;
    }
  }
  if (!lista) {
    return fallo(sesion === '0' ? 'sin_sesion' : 'sin_lista', motivo);
  }

  var juegos = [];
  for (var j = 0; j < lista.items.length; j++) {
    var elemento = lista.items[j];
    if (!elemento || typeof elemento.appid !== 'number') { continue; }
    juegos.push({
      appId: elemento.appid,
      addedAt: typeof elemento.date_added === 'number' ? elemento.date_added : 0
    });
  }

  return JSON.stringify({
    ok: true,
    url: location.href,
    steamId: typeof lista.steamid === 'string' ? lista.steamid : (sesion || ''),
    count: typeof recuento === 'number' ? recuento : null,
    items: juegos
  });
})();
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WishlistPayload {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    steam_id: Option<String>,
    #[serde(default)]
    count: Option<i64>,
    #[serde(default)]
    items: Vec<WishlistPayloadItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WishlistPayloadItem {
    #[serde(default)]
    app_id: i64,
    #[serde(default)]
    added_at: i64,
}

/// Interpreta lo que devolvió el guion como si viniera de un desconocido.
///
/// La barra del navegador vive dentro del documento remoto, de modo que hay que
/// dar por hecho que la página podría fabricar esta respuesta. Por eso se
/// vuelve a comprobar la URL, se validan los tipos y los rangos de cada campo y
/// se descarta todo lo que no encaje, en vez de confiar en que el guion se haya
/// ejecutado sin interferencias.
pub fn parse_wishlist_payload(raw: &str) -> AppResult<BrowserWishlist> {
    if raw.len() > MAX_PAYLOAD_BYTES {
        return Err(AppError::new(
            "wishlist_browser_too_large",
            "La lista de deseados que devolvió la página supera el tamaño máximo que Vindexa admite.",
        ));
    }
    let payload: WishlistPayload = serde_json::from_str(raw).map_err(|_| {
        AppError::new(
            "wishlist_browser_response",
            "La página de Steam devolvió algo que Vindexa no pudo interpretar.",
        )
    })?;

    if !payload.ok {
        return Err(page_error(payload.error.as_deref(), payload.reason.as_deref()));
    }

    // La URL que la página dice haber leído se comprueba con el mismo criterio
    // que se usó antes de evaluar. No se exige que sea idéntica a aquélla:
    // `/wishlist/` puede acabar en `/wishlist/profiles/<id>/` por una redirección
    // legítima de Steam. Lo que no puede es dejar de ser una lista de deseados.
    let claimed = payload.url.as_deref().and_then(|value| Url::parse(value).ok());
    if !claimed.as_ref().is_some_and(is_wishlist_page) {
        return Err(AppError::new(
            "wishlist_browser_page",
            "La ventana ya no está en tu lista de deseados de Steam. Vuelve a abrirla e inténtalo otra vez.",
        ));
    }

    let steam_id = payload
        .steam_id
        .map(|value| value.trim().to_owned())
        .filter(|value| is_steam_id64(value))
        .ok_or_else(|| {
            AppError::new(
                "wishlist_browser_account",
                "La página no dijo de qué cuenta es la lista, así que Vindexa no la ha importado.",
            )
        })?;

    if payload.items.len() > MAX_ITEMS {
        return Err(AppError::new(
            "wishlist_browser_too_many",
            format!("La página devolvió más de {MAX_ITEMS} juegos, que es más de lo que una lista de deseados de Steam admite."),
        ));
    }

    let now = chrono::Utc::now().timestamp();
    let mut items = payload
        .items
        .into_iter()
        .filter_map(|item| {
            let app_id = u32::try_from(item.app_id).ok().filter(|value| *value > 0)?;
            Some(SteamWishlistItem {
                app_id,
                // La página publica la prioridad de Steam, pero Vindexa no la
                // usa: su lista tiene sus propios cubos y su propio orden.
                priority: 0,
                added_at: normalize_added_at(item.added_at, now),
                title: None,
                hidden_in_store: false,
            })
        })
        .collect::<Vec<_>>();

    // Mismo criterio que `super::wishlist::parse_wishlist`: primero lo más
    // antiguo y una sola fila por AppID, para que dos importaciones de la misma
    // lista coloquen los juegos exactamente en el mismo sitio.
    items.sort_by(|left, right| {
        left.added_at
            .cmp(&right.added_at)
            .then(left.app_id.cmp(&right.app_id))
    });
    items.dedup_by_key(|item| item.app_id);

    let hidden_by_filters = payload
        .count
        .and_then(|count| usize::try_from(count).ok())
        .map(|count| count.saturating_sub(items.len()))
        .unwrap_or(0);

    Ok(BrowserWishlist {
        steam_id,
        items,
        hidden_by_filters,
    })
}

/// Convierte a los juegos que espera la importación de la base de datos.
pub fn to_imported_games(items: &[SteamWishlistItem]) -> Vec<ImportedWishlistGame> {
    items
        .iter()
        .map(|item| ImportedWishlistGame {
            app_id: item.app_id,
            title: item.title.clone(),
            added_at: item.added_at.clone(),
        })
        .collect()
}

/// Comprueba que la lista leída es la de la cuenta vinculada en Vindexa.
///
/// El navegador puede estar en la lista de cualquier otra persona: basta con
/// escribir su dirección. Si Vindexa ya sabe qué cuenta es la tuya, importar
/// una ajena sería mezclar dos listas sin decirlo, así que se rechaza. Sin
/// cuenta vinculada no hay con qué comparar y se acepta lo que haya.
pub fn ensure_same_account(linked: Option<&str>, found: &str) -> AppResult<()> {
    let Some(linked) = linked.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    if linked == found {
        return Ok(());
    }
    Err(AppError::new(
        "wishlist_browser_other_account",
        "Esa lista de deseados es de otra cuenta de Steam, no de la que tienes vinculada en Vindexa.",
    ))
}

/// Mensaje de la barra cuando la ficha visible no se puede añadir.
///
/// El catálogo de deseados está indexado por AppID de Steam, así que un juego
/// de GOG, Epic o itch.io no tiene con qué identificarse. Decirlo es preferible
/// a inventarle un identificador que después no encajaría con nada.
pub fn unsupported_page_message(store_name: &str) -> String {
    if store_name == "Steam" {
        return "Abre la ficha del juego en la tienda de Steam para añadirlo a tus deseados."
            .to_string();
    }
    format!(
        "Los deseados de Vindexa se guardan con el AppID de Steam, así que todavía no admiten fichas de {store_name}."
    )
}

/// Fecha de alta válida en RFC 3339, o `None` si la marca no describe nada.
fn normalize_added_at(value: i64, now: i64) -> Option<String> {
    if value < MIN_ADDED_AT || value > now.saturating_add(MAX_ADDED_AT_SKEW) {
        return None;
    }
    chrono::DateTime::<chrono::Utc>::from_timestamp(value, 0).map(|instant| instant.to_rfc3339())
}

/// ¿Tiene la forma de un SteamID64 de una cuenta individual?
fn is_steam_id64(value: &str) -> bool {
    value.len() == 17
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value.starts_with("7656119")
}

/// Traduce el motivo que dio la página a un error con su mensaje en español.
fn page_error(code: Option<&str>, reason: Option<&str>) -> AppError {
    match code {
        Some("sin_sesion") => AppError::new(
            "wishlist_browser_signed_out",
            "Inicia sesión en Steam en la ventana que se ha abierto y vuelve a pulsar «Importar desde el navegador».",
        ),
        Some("sin_lista") if reason == Some("RateLimit") => AppError::new(
            "wishlist_browser_rate_limited",
            "Steam está limitando las peticiones a la lista de deseados. Espera unos minutos y vuelve a intentarlo.",
        ),
        Some("sin_lista") => AppError::new(
            "wishlist_browser_empty",
            "La página de Steam se cargó pero no traía ninguna lista. Recárgala en la ventana del navegador y vuelve a intentarlo.",
        ),
        Some("pagina") => AppError::new(
            "wishlist_browser_page",
            "La ventana no está en tu lista de deseados de Steam. Vuelve a abrirla e inténtalo otra vez.",
        ),
        _ => AppError::new(
            "wishlist_browser_response",
            "La página de Steam no devolvió la lista de deseados. Recárgala y vuelve a intentarlo.",
        ),
    }
}

// ---------------------------------------------------------------------------
// Conexión con la ventana del navegador
// ---------------------------------------------------------------------------

/// Abre —o reutiliza— la ventana de Steam y la lleva a la lista de deseados.
///
/// Se apoya en `store_window::open_store_home` para crearla, de modo que la
/// ventana nace con exactamente el mismo endurecimiento que cualquier otra:
/// almacén aislado, bloqueador nativo, sin descargas, sin ventanas emergentes y
/// sin IPC. Después se navega a la lista con `navigate`, que vuelve a pasar por
/// la política de navegación como cualquier enlace de la página.
async fn open_wishlist_window<R: Runtime>(
    app: &AppHandle<R>,
) -> AppResult<(WebviewWindow<R>, Option<u64>)> {
    let store = stores::store_by_id(stores::DEFAULT_STORE_ID).ok_or_else(|| {
        AppError::new(
            "wishlist_browser_store",
            "El navegador integrado no tiene configurada la tienda de Steam.",
        )
    })?;
    let label = store.window_label();

    if app.get_webview_window(&label).is_none() {
        crate::store_window::open_store_home(app, store.id).await?;
    }
    let window = app.get_webview_window(&label).ok_or_else(|| {
        AppError::new(
            "wishlist_browser_window",
            "No se pudo abrir el navegador integrado de tiendas.",
        )
    })?;
    // Una ventana sin estado registrado quedó a medio abrir y su protección no
    // está confirmada: mismo criterio que usa `store_window` al reutilizarla.
    if !session::is_registered(&label) {
        return Err(AppError::new(
            "wishlist_browser_window",
            "El navegador integrado no está protegido, así que no se ha usado.",
        ));
    }

    let target = wishlist_page_url();
    let previous_generation = load_generation(&label);
    window
        .navigate(target)
        .and_then(|()| window.unminimize())
        .and_then(|()| window.show())
        .and_then(|()| window.set_focus())
        .map_err(|_| {
            AppError::new(
                "wishlist_browser_window",
                "No se pudo llevar el navegador integrado a tu lista de deseados.",
            )
        })?;
    Ok((window, previous_generation))
}

/// Generación de carga de la ventana, o `None` si ya no está registrada.
fn load_generation(label: &str) -> Option<u64> {
    session::with_window(label, |state| state.load_generation)
}

/// Espera a que la ventana termine de cargar la página de deseados.
///
/// Es la primera de las tres comprobaciones de destino: sin una URL de deseados
/// aquí, el guion no llega a evaluarse.
///
/// `previous_generation` es la generación de carga anterior a pedir la
/// navegación, y sirve para no leer el documento equivocado: si la ventana ya
/// estaba en la lista, «cargada y en la URL correcta» se cumpliría de inmediato
/// sobre el documento viejo. Esperar a que la generación cambie garantiza que lo
/// que se lee es la carga que acabamos de provocar. Pasado
/// [`FRESH_LOAD_GRACE`] se deja de exigir: una navegación que el motor decida no
/// repetir no puede bloquear la importación para siempre, y el documento que hay
/// en esa URL sigue siendo una lista de deseados.
async fn wait_for_wishlist_page(label: &str, previous_generation: Option<u64>) -> AppResult<()> {
    let started = tokio::time::Instant::now();
    let deadline = started + LOAD_TIMEOUT;
    loop {
        if let Some(state) = session::snapshot(label)
            && !state.loading
            && let Ok(url) = Url::parse(&state.url)
            && is_wishlist_page(&url)
        {
            let fresh = load_generation(label) != previous_generation;
            if fresh || started.elapsed() >= FRESH_LOAD_GRACE {
                return Ok(());
            }
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(AppError::new(
                "wishlist_browser_timeout",
                "Tu lista de deseados no terminó de cargarse en el navegador integrado. Compruébala en esa ventana y vuelve a intentarlo.",
            ));
        }
        tokio::time::sleep(LOAD_POLL).await;
    }
}

/// Lee la lista de deseados de la sesión abierta en el navegador integrado.
pub async fn read_wishlist<R: Runtime>(app: &AppHandle<R>) -> AppResult<BrowserWishlist> {
    let (window, previous_generation) = open_wishlist_window(app).await?;
    let label = window.label().to_string();
    wait_for_wishlist_page(&label, previous_generation).await?;
    let raw = evaluate_json(&window, READ_WISHLIST_SCRIPT).await?;
    parse_wishlist_payload(&raw)
}

/// Ficha visible en una ventana del navegador, lista para añadir a deseados.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleStorePage {
    pub app_id: u32,
    pub store_name: &'static str,
}

/// Qué ficha se está viendo en la ventana `label`, si es una que se pueda añadir.
pub fn visible_store_page(label: &str) -> Result<VisibleStorePage, String> {
    let Some(state) = session::snapshot(label) else {
        return Err("Esa ventana del navegador ya no está disponible.".to_string());
    };
    let store_name = stores::store_by_id(&state.store_id)
        .map(|store| store.name)
        .unwrap_or("esta tienda");
    let Ok(url) = Url::parse(&state.url) else {
        return Err(unsupported_page_message(store_name));
    };
    match app_id_from_store_url(&url) {
        Some(app_id) => Ok(VisibleStorePage {
            app_id,
            store_name,
        }),
        None => Err(unsupported_page_message(store_name)),
    }
}

/// Nombre publicado por la tienda para un AppID.
///
/// Se pide a la tienda en vez de leerlo de la página: el nombre que acabará en
/// la base de datos no debe depender de lo que un documento remoto diga que se
/// llama el juego. Reutiliza el mismo canal, la misma espera mínima y el mismo
/// tope de cuerpo que la importación por API.
async fn resolve_title(app_id: u32) -> AppResult<String> {
    let mut items = vec![SteamWishlistItem {
        app_id,
        priority: 0,
        added_at: None,
        title: None,
        hidden_in_store: false,
    }];
    crate::steam::wishlist::resolve_store_titles(&mut items).await?;
    items
        .into_iter()
        .next()
        .and_then(|item| item.title)
        .ok_or_else(|| {
            AppError::new(
                "wishlist_browser_title",
                "La tienda de Steam no devolvió el nombre de ese juego, así que Vindexa no lo ha añadido.",
            )
        })
}

/// Añade a los deseados de Vindexa la ficha que se está viendo.
///
/// Devuelve siempre una frase para la barra del navegador, también cuando algo
/// falla: quien pulsa el botón está dentro de la ventana de la tienda y allí no
/// hay más canal que ese aviso, así que un error silencioso se leería como que
/// el botón no hace nada.
///
/// **No ejecuta ningún guion en la página.** El AppID sale de la URL que la
/// política de navegación ya validó, y el nombre lo pide Vindexa a la tienda:
/// lo que acaba en la base de datos no depende de lo que un documento remoto
/// diga que se llama el juego.
pub async fn add_current_page_to_wishlist(
    label: &str,
    database: crate::db::Database,
    maintenance: std::sync::Arc<tokio::sync::RwLock<()>>,
) -> String {
    let page = match visible_store_page(label) {
        Ok(page) => page,
        Err(message) => return message,
    };
    let title = match resolve_title(page.app_id).await {
        Ok(title) => title,
        Err(error) => return error.message,
    };
    match persist_addition(page.app_id, title, database, maintenance).await {
        Ok(addition) => addition.message(),
        Err(error) => error.message,
    }
}

/// Escribe el alta reutilizando la importación de deseados de la base de datos.
///
/// Se apoya en `import_steam_wishlist` con un solo juego en vez de abrir un
/// camino nuevo: esa función ya decide si el juego va a `wishlist_entries` o al
/// catálogo, ya respeta lo que hubiera escrito a mano y ya es idempotente. Un
/// alta manual no necesita otra cosa, y tener dos maneras de meter un deseado
/// sería tener dos maneras de equivocarse.
async fn persist_addition(
    app_id: u32,
    title: String,
    database: crate::db::Database,
    maintenance: std::sync::Arc<tokio::sync::RwLock<()>>,
) -> AppResult<StoreWishlistAddition> {
    let nombre = title.clone();
    let (report, in_library) = tauri::async_runtime::spawn_blocking(move || {
        let _guard = maintenance.blocking_write();
        // `game_detail` falla con «no encontrado» cuando el juego no está en la
        // biblioteca; es la única pregunta pública que responde eso.
        let in_library = database.game_detail(app_id).is_ok();
        let games = [ImportedWishlistGame {
            app_id,
            title: Some(nombre),
            added_at: None,
        }];
        database
            .import_steam_wishlist(&games)
            .map(|report| (report, in_library))
    })
    .await
    .map_err(|_| {
        AppError::new(
            "wishlist_browser_task",
            "Vindexa no pudo terminar de guardar el deseado. Vuelve a intentarlo.",
        )
    })??;

    if report.imported == 0 && report.already_present == 0 {
        // Sólo queda un motivo posible: el límite de la lista de deseados.
        return Err(AppError::validation(
            "Tu lista de deseados está llena, así que ese juego no se ha añadido.",
        ));
    }

    Ok(StoreWishlistAddition {
        app_id,
        title,
        added: report.imported > 0,
        in_library,
    })
}

// ---------------------------------------------------------------------------
// Evaluación en el webview
// ---------------------------------------------------------------------------

/// Evalúa `script` en la ventana y devuelve la cadena que produce.
///
/// La dirección es la única segura: la aplicación pregunta y la página
/// responde. Al revés no existe —la ventana no declara ningún permiso de Tauri
/// y su ACL rechaza cualquier origen remoto—, así que ninguna página puede
/// provocar esta lectura.
#[cfg(target_os = "macos")]
pub(crate) async fn evaluate_json<R: Runtime>(
    window: &WebviewWindow<R>,
    script: &str,
) -> AppResult<String> {
    use block2::RcBlock;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::WKWebView;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    use tokio::time::timeout;

    let (sender, receiver) = oneshot::channel::<Option<String>>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let source = script.to_owned();
    let failed = sender.clone();

    if window
        .with_webview(move |webview| unsafe {
            let view = webview.inner().cast::<WKWebView>();
            if view.is_null() {
                if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
                    let _ = sender.send(None);
                }
                return;
            }
            let source = NSString::from_str(&source);
            let completion = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) else {
                    return;
                };
                if !error.is_null() || value.is_null() {
                    let _ = sender.send(None);
                    return;
                }
                let text = (&*value)
                    .downcast_ref::<NSString>()
                    .map(|value| value.to_string());
                let _ = sender.send(text);
            });
            (&*view).evaluateJavaScript_completionHandler(&source, Some(&completion));
        })
        .is_err()
    {
        if let Some(sender) = failed.lock().ok().and_then(|mut sender| sender.take()) {
            let _ = sender.send(None);
        }
        return Err(evaluation_error());
    }

    match timeout(EVAL_TIMEOUT, receiver).await {
        Ok(Ok(Some(text))) => Ok(text),
        _ => Err(evaluation_error()),
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) async fn evaluate_json<R: Runtime>(
    _window: &WebviewWindow<R>,
    _script: &str,
) -> AppResult<String> {
    // La lectura se apoya en `evaluateJavaScript:completionHandler:` de
    // WKWebView. Fuera de macOS no hay equivalente ya escrito y probado en este
    // proyecto, y devolver una lista vacía se confundiría con «no tienes nada».
    Err(AppError::new(
        "wishlist_browser_unsupported",
        "Importar la lista de deseados desde el navegador integrado sólo está disponible en macOS.",
    ))
}

fn evaluation_error() -> AppError {
    AppError::new(
        "wishlist_browser_eval",
        "El navegador integrado no pudo leer tu lista de deseados. Recarga la página y vuelve a intentarlo.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    /// Carga real de la página de deseados, recortada a tres juegos.
    ///
    /// Reproduce lo que devuelve [`READ_WISHLIST_SCRIPT`] sobre la estructura
    /// medida el 18-08-2026: los `appid` y las marcas de tiempo son los de la
    /// cuenta pública con la que se comprobó el endpoint.
    const CARGA_OK: &str = r#"{
        "ok": true,
        "url": "https://store.steampowered.com/wishlist/",
        "steamId": "76561197960434622",
        "count": 5,
        "items": [
            {"appId": 3669430, "addedAt": 1758122485},
            {"appId": 774171, "addedAt": 1597272213},
            {"appId": 1111840, "addedAt": 1569775680}
        ]
    }"#;

    #[test]
    fn only_the_three_real_wishlist_routes_are_accepted() {
        for aceptada in [
            "https://store.steampowered.com/wishlist/",
            "https://store.steampowered.com/wishlist",
            "https://store.steampowered.com/wishlist/profiles/76561197960434622/",
            "https://store.steampowered.com/wishlist/id/gaben/",
            "https://STORE.steampowered.com./wishlist/?l=spanish",
        ] {
            assert!(is_wishlist_page(&url(aceptada)), "{aceptada} debería valer");
        }

        for rechazada in [
            "http://store.steampowered.com/wishlist/",
            "https://store.steampowered.com:8443/wishlist/",
            "https://store.steampowered.com.attacker.tld/wishlist/",
            "https://evilstore.steampowered.com/wishlist/",
            "https://usuario:clave@store.steampowered.com/wishlist/",
            "https://steamcommunity.com/wishlist/",
            "https://store.steampowered.com/app/620/",
            "https://store.steampowered.com/wishlist/profiles/76561197960434622/wishlistdata/",
            "https://store.steampowered.com/",
        ] {
            assert!(
                !is_wishlist_page(&url(rechazada)),
                "{rechazada} no puede pasar por lista de deseados"
            );
        }
    }

    #[test]
    fn a_valid_payload_becomes_an_ordered_wishlist_without_duplicates() {
        let leida = parse_wishlist_payload(CARGA_OK).expect("leer carga válida");

        assert_eq!(leida.steam_id, "76561197960434622");
        assert_eq!(
            leida.items.iter().map(|item| item.app_id).collect::<Vec<_>>(),
            [1111840, 774171, 3669430]
        );
        assert_eq!(leida.items[0].added_at.as_deref(), Some("2019-09-29T16:48:00+00:00"));
        assert!(leida.items.iter().all(|item| item.title.is_none()));
        // El recuento decía cinco y la página sólo mostró tres.
        assert_eq!(leida.hidden_by_filters, 2);
    }

    #[test]
    fn each_failure_of_the_page_keeps_its_own_code_and_never_invents_a_reason() {
        let casos = [
            (r#"{"ok":false,"error":"sin_sesion"}"#, "wishlist_browser_signed_out"),
            (
                r#"{"ok":false,"error":"sin_lista","reason":"RateLimit"}"#,
                "wishlist_browser_rate_limited",
            ),
            (r#"{"ok":false,"error":"sin_lista"}"#, "wishlist_browser_empty"),
            (r#"{"ok":false,"error":"pagina"}"#, "wishlist_browser_page"),
            (r#"{"ok":false,"error":"sin_datos"}"#, "wishlist_browser_response"),
        ];
        for (carga, codigo) in casos {
            let error = parse_wishlist_payload(carga).expect_err("debe fallar");
            assert_eq!(error.code, codigo, "carga {carga}");
            assert!(error.message.ends_with('.'));
            assert!(!error.message.contains("http"));
        }

        assert_eq!(
            parse_wishlist_payload("no es json").unwrap_err().code,
            "wishlist_browser_response"
        );
    }

    #[test]
    fn a_payload_that_claims_another_page_is_refused() {
        for hostil in [
            r#"{"ok":true,"url":"https://attacker.tld/wishlist/","steamId":"76561197960434622","items":[{"appId":10,"addedAt":1500000000}]}"#,
            r#"{"ok":true,"url":"https://store.steampowered.com/app/620/","steamId":"76561197960434622","items":[]}"#,
            r#"{"ok":true,"steamId":"76561197960434622","items":[]}"#,
        ] {
            let error = parse_wishlist_payload(hostil).expect_err("debe rechazarse");
            assert_eq!(error.code, "wishlist_browser_page");
        }
    }

    #[test]
    fn a_payload_without_a_believable_account_is_refused() {
        for hostil in [
            r#"{"ok":true,"url":"https://store.steampowered.com/wishlist/","steamId":"0","items":[]}"#,
            r#"{"ok":true,"url":"https://store.steampowered.com/wishlist/","steamId":"12345678901234567","items":[]}"#,
            r#"{"ok":true,"url":"https://store.steampowered.com/wishlist/","items":[]}"#,
        ] {
            assert_eq!(
                parse_wishlist_payload(hostil).unwrap_err().code,
                "wishlist_browser_account"
            );
        }
    }

    #[test]
    fn impossible_app_ids_and_dates_are_dropped_instead_of_guessed() {
        let carga = r#"{
            "ok": true,
            "url": "https://store.steampowered.com/wishlist/",
            "steamId": "76561197960434622",
            "items": [
                {"appId": 0, "addedAt": 1500000000},
                {"appId": -7, "addedAt": 1500000000},
                {"appId": 9999999999, "addedAt": 1500000000},
                {"appId": 620, "addedAt": 0},
                {"appId": 440, "addedAt": 99999999999},
                {"appId": 70, "addedAt": 1500000000}
            ]
        }"#;
        let leida = parse_wishlist_payload(carga).expect("leer carga atípica");

        let ids = leida.items.iter().map(|item| item.app_id).collect::<Vec<_>>();
        assert_eq!(ids.len(), 3, "sólo sobreviven los AppID posibles");
        assert!(ids.contains(&620) && ids.contains(&440) && ids.contains(&70));
        // Sin fecha creíble no se inventa ninguna.
        let sin_fecha = leida
            .items
            .iter()
            .filter(|item| item.added_at.is_none())
            .count();
        assert_eq!(sin_fecha, 2);
        assert_eq!(leida.hidden_by_filters, 0);
    }

    #[test]
    fn a_repeated_app_id_collapses_into_its_oldest_entry() {
        let carga = r#"{
            "ok": true,
            "url": "https://store.steampowered.com/wishlist/",
            "steamId": "76561197960434622",
            "items": [
                {"appId": 70, "addedAt": 1500000000},
                {"appId": 70, "addedAt": 1400000000}
            ]
        }"#;
        let leida = parse_wishlist_payload(carga).expect("leer carga repetida");
        assert_eq!(leida.items.len(), 1);
        assert_eq!(
            leida.items[0].added_at.as_deref(),
            Some("2014-05-13T16:53:20+00:00")
        );
    }

    #[test]
    fn an_oversized_payload_is_refused_before_being_parsed() {
        let enorme = "a".repeat(MAX_PAYLOAD_BYTES + 1);
        assert_eq!(
            parse_wishlist_payload(&enorme).unwrap_err().code,
            "wishlist_browser_too_large"
        );

        let mut juegos = String::new();
        for indice in 0..(MAX_ITEMS + 1) {
            if indice > 0 {
                juegos.push(',');
            }
            juegos.push_str(&format!("{{\"appId\":{},\"addedAt\":1500000000}}", indice + 1));
        }
        let carga = format!(
            "{{\"ok\":true,\"url\":\"https://store.steampowered.com/wishlist/\",\"steamId\":\"76561197960434622\",\"items\":[{juegos}]}}"
        );
        assert_eq!(
            parse_wishlist_payload(&carga).unwrap_err().code,
            "wishlist_browser_too_many"
        );
    }

    #[test]
    fn the_script_only_reads_and_never_reaches_the_network() {
        for prohibido in [
            "fetch(",
            "XMLHttpRequest",
            "document.cookie",
            "localStorage",
            "sessionStorage",
            "indexedDB",
            "navigator.sendBeacon",
            "postMessage(",
            "WebSocket",
            "eval(",
            "new Function",
            "access_token",
            "webapi_token",
            "steamLoginSecure",
            "__TAURI__",
            "__TAURI_INTERNALS__",
            "invoke(",
        ] {
            assert!(
                !READ_WISHLIST_SCRIPT.contains(prohibido),
                "el guion no puede usar {prohibido}"
            );
        }
        // Y comprueba por su cuenta dónde se está ejecutando.
        assert!(READ_WISHLIST_SCRIPT.contains("location.protocol !== 'https:'"));
        assert!(READ_WISHLIST_SCRIPT.contains("store.steampowered.com"));
        assert!(READ_WISHLIST_SCRIPT.contains("/^\\/wishlist(\\/|$)/"));
        assert!(READ_WISHLIST_SCRIPT.contains("JSON.stringify"));
    }

    #[test]
    fn the_script_reads_exactly_the_keys_that_steam_publishes_today() {
        // Las claves proceden de la carga real de
        // `store.steampowered.com/wishlist/profiles/<id>/` medida el 18-08-2026.
        for clave in [
            "window.SSR",
            "renderContext",
            "queryData",
            "queries",
            "queryKey",
            "WishlistSortedFiltered",
            "wishlistitemcount",
            "loaderData",
            "steamid",
            "date_added",
            "appid",
        ] {
            assert!(
                READ_WISHLIST_SCRIPT.contains(clave),
                "el guion debe leer «{clave}»"
            );
        }
    }

    #[test]
    fn a_store_page_only_yields_an_app_id_when_it_really_is_one() {
        assert_eq!(
            app_id_from_store_url(&url("https://store.steampowered.com/app/620/Portal_2/?l=spanish")),
            Some(620)
        );
        assert_eq!(
            app_id_from_store_url(&url("https://store.steampowered.com/app/1091500")),
            Some(1091500)
        );
        for ninguno in [
            "https://store.steampowered.com/app/0/",
            "https://store.steampowered.com/app/",
            "https://store.steampowered.com/app/no-es-un-numero/",
            "https://store.steampowered.com/sub/12345/",
            "https://store.steampowered.com/bundle/1234/",
            "https://www.gog.com/game/cyberpunk_2077",
            "https://store.epicgames.com/es-ES/p/fortnite",
            "https://creador.itch.io/juego",
            "http://store.steampowered.com/app/620/",
        ] {
            assert!(
                app_id_from_store_url(&url(ninguno)).is_none(),
                "{ninguno} no es una ficha de Steam"
            );
        }
    }

    #[test]
    fn importing_someone_elses_list_is_refused_only_when_there_is_something_to_compare() {
        assert!(ensure_same_account(None, "76561197960434622").is_ok());
        assert!(ensure_same_account(Some("   "), "76561197960434622").is_ok());
        assert!(ensure_same_account(Some("76561197960434622"), "76561197960434622").is_ok());

        let error = ensure_same_account(Some("76561198000000000"), "76561197960434622")
            .expect_err("cuentas distintas");
        assert_eq!(error.code, "wishlist_browser_other_account");
        assert!(!error.message.contains("7656"), "el mensaje no publica ningún SteamID");
    }

    #[test]
    fn the_addition_says_what_really_happened() {
        let nuevo = StoreWishlistAddition {
            app_id: 620,
            title: "Portal 2".into(),
            added: true,
            in_library: false,
        };
        assert_eq!(nuevo.message(), "«Portal 2» se ha añadido a tus deseados de Vindexa.");

        let repetido = StoreWishlistAddition {
            added: false,
            ..nuevo.clone()
        };
        assert!(repetido.message().contains("ya estaba"));

        let poseido = StoreWishlistAddition {
            in_library: true,
            ..nuevo
        };
        assert!(poseido.message().contains("biblioteca"));
    }

    #[test]
    fn other_stores_are_told_the_real_reason_instead_of_a_vague_no() {
        let gog = unsupported_page_message("GOG.com");
        assert!(gog.contains("AppID de Steam"));
        assert!(gog.contains("GOG.com"));
        assert!(unsupported_page_message("Steam").contains("ficha del juego"));
    }

    #[test]
    fn the_imported_games_carry_over_exactly_what_was_read() {
        let leida = parse_wishlist_payload(CARGA_OK).expect("leer carga válida");
        let juegos = to_imported_games(&leida.items);
        assert_eq!(juegos.len(), leida.items.len());
        assert_eq!(juegos[0].app_id, leida.items[0].app_id);
        assert_eq!(juegos[0].added_at, leida.items[0].added_at);
        assert!(juegos.iter().all(|juego| juego.title.is_none()));
    }
}
