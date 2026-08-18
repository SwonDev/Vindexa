//! Inicio de sesión de Epic y GOG **dentro** de Vindexa.
//!
//! # Por qué existe
//!
//! El primer intento abría la página de Epic en el navegador del sistema y le
//! pedía a la persona que copiase el `authorizationCode` de un JSON crudo. Eso
//! no es un inicio de sesión, es una pantalla de depuración: nadie que no sepa
//! qué es un código de autorización puede terminarlo.
//!
//! Aquí el recorrido es el que se espera: se pulsa «Iniciar sesión», se abre la
//! página de la tienda en la ventana del navegador integrado, se escribe usuario
//! y contraseña **en la página de la tienda**, y cuando la navegación llega a la
//! página de retorno Vindexa recoge el código por su cuenta. No hay JSON, no hay
//! nada que copiar.
//!
//! # De dónde sale el patrón
//!
//! De [`crate::steam::wishlist_session`], que ya hace exactamente esto para la
//! lista de deseados: abre la ventana registrada, espera a que cargue, evalúa un
//! guion acotado y valida lo que vuelve. Se reutiliza su evaluador en vez de
//! copiar su puente con WKWebView.
//!
//! # Las tres comprobaciones
//!
//! El documento remoto podría fabricar la respuesta del guion, así que el
//! destino se comprueba tres veces y de forma independiente:
//!
//! 1. **En Rust, antes de leer nada**: el anfitrión y la ruta de la URL que
//!    registró la ventana tienen que ser los de la página de retorno.
//! 2. **Dentro del guion**: vuelve a mirar `location` antes de tocar el
//!    documento, por si la página cambió entre el sondeo y la evaluación.
//! 3. **En Rust, sobre lo devuelto**: el código pasa por el mismo validador de
//!    forma que usa el flujo manual de cada tienda.
//!
//! # Lo que **no** se lee
//!
//! Sólo el código de autorización. Ni cookies, ni `localStorage`, ni el testigo
//! de sesión que Epic o GOG tengan en esa ventana. El código **no se registra ni
//! se muestra**, ni siquiera truncado: es de un solo uso y da acceso a la cuenta.

use crate::browser::{session, stores as browser_stores};
use crate::error::{AppError, AppResult};
use crate::stores::{ExternalStore, epic_api, gog_api};
use serde::Deserialize;
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, Url};

/// Cuánto se espera a que la persona termine de identificarse.
///
/// Cinco minutos porque aquí no espera una máquina: hay que escribir el correo,
/// la contraseña, resolver un captcha y, muy a menudo, ir a por el segundo
/// factor al teléfono. Un minuto convertiría el doble factor en un fallo.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);

/// Cada cuánto se mira en qué página está la ventana.
const POLL: Duration = Duration::from_millis(400);

/// Tope de la respuesta del guion. El código son decenas de caracteres; este
/// margen sobra y evita interpretar un documento entero si algo va mal.
const MAX_PAYLOAD_BYTES: usize = 16 * 1024;

/// Página a la que la tienda devuelve a la persona con el código.
struct Landing {
    host: &'static str,
    path: &'static str,
}

/// Dónde termina el recorrido de cada tienda.
///
/// Epic deja el código **en el cuerpo** de `/id/api/redirect`, en un JSON. GOG
/// lo deja **en la URL** de `embed.gog.com/on_login_success`, así que en su caso
/// no hace falta evaluar nada dentro de la página.
fn landing(store: ExternalStore) -> Landing {
    match store {
        ExternalStore::Epic => Landing {
            host: "www.epicgames.com",
            path: "/id/api/redirect",
        },
        ExternalStore::Gog => Landing {
            host: "embed.gog.com",
            path: "/on_login_success",
        },
    }
}

/// Identificador de la tienda dentro del navegador integrado.
fn browser_store_id(store: ExternalStore) -> &'static str {
    match store {
        ExternalStore::Epic => "epic",
        ExternalStore::Gog => "gog",
    }
}

/// ¿Puede esta tienda iniciar sesión dentro de Vindexa?
///
/// La respuesta no es una constante: depende de que la allowlist del navegador
/// integrado admita **su página de retorno**. Si no la admite, la navegación se
/// cancela y el código no llegaría nunca, así que es preferible no ofrecer el
/// camino que dejar que falle a mitad.
///
/// Se calcula en vez de escribirse a mano para que ampliar la allowlist baste
/// para habilitar la tienda, sin tener que acordarse de tocar también esto.
pub fn supports_in_app(store: ExternalStore) -> bool {
    let Some(profile) = browser_stores::store_by_id(browser_store_id(store)) else {
        return false;
    };
    let expected = landing(store);
    let Ok(target) = Url::parse(&format!("https://{}{}", expected.host, expected.path)) else {
        return false;
    };
    profile.allows(&target) && login_url(store).is_ok_and(|url| profile.allows(&url))
}

fn unsupported_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_login_unsupported",
        format!(
            "Vindexa todavía no puede completar el inicio de sesión de {} en su propia ventana.",
            store.display_name()
        ),
    )
}

fn login_url(store: ExternalStore) -> AppResult<Url> {
    let raw = match store {
        ExternalStore::Epic => epic_api::login_url(),
        ExternalStore::Gog => gog_api::login_url(),
    };
    Url::parse(&raw).map_err(|_| {
        AppError::new(
            "external_store_login",
            "No se pudo preparar la página de inicio de sesión de la tienda.",
        )
    })
}

/// ¿Es ésta la página de retorno de la tienda?
///
/// Compara el anfitrión ya normalizado y exige la ruta exacta. Un `startsWith`
/// aceptaría `/id/api/redirect.malicioso`, así que se compara entera.
fn is_landing(store: ExternalStore, url: &Url) -> bool {
    let expected = landing(store);
    let Some(host) = browser_stores::normalized_host(url) else {
        return false;
    };
    url.scheme() == "https"
        && host == expected.host
        && url.path().trim_end_matches('/') == expected.path.trim_end_matches('/')
}

/// Guion que lee el código del cuerpo de la página de retorno de Epic.
///
/// Vuelve a comprobar `location` antes de tocar el documento y **sólo** extrae
/// `authorizationCode`. No mira cookies ni almacenamiento.
const READ_EPIC_CODE_SCRIPT: &str = r#"
(function () {
  'use strict';
  function fallo(codigo) { return JSON.stringify({ ok: false, error: codigo }); }

  if (location.protocol !== 'https:') { return fallo('pagina'); }
  var anfitrion = String(location.hostname || '').toLowerCase().replace(/\.+$/, '');
  if (anfitrion !== 'www.epicgames.com') { return fallo('pagina'); }
  if (location.pathname.replace(/\/+$/, '') !== '/id/api/redirect') { return fallo('pagina'); }

  var texto = '';
  if (document.body && typeof document.body.innerText === 'string') {
    texto = document.body.innerText;
  } else if (document.documentElement) {
    texto = document.documentElement.textContent || '';
  }
  texto = texto.trim();
  if (!texto) { return fallo('vacia'); }
  if (texto.length > 16000) { return fallo('tamano'); }

  var datos;
  try { datos = JSON.parse(texto); } catch (e) { return fallo('formato'); }
  if (!datos || typeof datos !== 'object') { return fallo('formato'); }

  var codigo = datos.authorizationCode;
  if (typeof codigo !== 'string' || !codigo) { return fallo('sin_codigo'); }
  return JSON.stringify({ ok: true, code: codigo });
})()
"#;

#[derive(Debug, Deserialize)]
struct CodePayload {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// Interpreta lo que devolvió el guion como si viniera de un desconocido.
fn parse_code_payload(store: ExternalStore, raw: &str) -> AppResult<String> {
    if raw.len() > MAX_PAYLOAD_BYTES {
        return Err(page_error(store, None));
    }
    let payload: CodePayload = serde_json::from_str(raw).map_err(|_| page_error(store, None))?;
    if !payload.ok {
        return Err(page_error(store, payload.error.as_deref()));
    }
    let code = payload.code.ok_or_else(|| page_error(store, None))?;
    // Tercera comprobación: el mismo validador de forma que el flujo manual.
    validate_code(store, &code)
}

fn validate_code(store: ExternalStore, code: &str) -> AppResult<String> {
    match store {
        ExternalStore::Epic => epic_api::extract_authorization_code(code),
        ExternalStore::Gog => gog_api::extract_authorization_code(code),
    }
}

/// Motivo por el que la página de retorno no sirvió.
///
/// El código interno del guion **nunca** se concatena al mensaje: se traduce a
/// una frase fija. Así ninguna cadena de la página puede acabar en la interfaz.
fn page_error(store: ExternalStore, code: Option<&str>) -> AppError {
    let detail = match code {
        Some("sin_codigo") => "La página de retorno no traía ningún código.",
        Some("pagina") => "La ventana dejó de estar en la página de retorno.",
        Some("formato") | Some("vacia") | Some("tamano") => {
            "La página de retorno no tenía el formato esperado."
        }
        _ => "No se pudo leer la respuesta de la página de retorno.",
    };
    AppError::new(
        "external_store_login_page",
        format!(
            "{detail} Vuelve a iniciar sesión en {}.",
            store.display_name()
        ),
    )
}

fn window_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_login_window",
        format!(
            "No se pudo abrir el navegador integrado para iniciar sesión en {}.",
            store.display_name()
        ),
    )
}

fn cancelled_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_login_cancelled",
        format!(
            "Se cerró la ventana antes de terminar de iniciar sesión en {}.",
            store.display_name()
        ),
    )
}

fn timeout_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_login_timeout",
        format!(
            "Se agotó el tiempo de espera para iniciar sesión en {}. Vuelve a intentarlo.",
            store.display_name()
        ),
    )
}

/// Abre la página de inicio de sesión de la tienda en el navegador integrado y
/// espera a recoger el código de autorización.
///
/// Devuelve el código **sin registrarlo en ningún sitio**. Quien lo recibe lo
/// canjea inmediatamente y lo descarta.
pub async fn capture_authorization_code<R: Runtime>(
    app: &AppHandle<R>,
    store: ExternalStore,
) -> AppResult<String> {
    if !supports_in_app(store) {
        return Err(unsupported_error(store));
    }
    let profile = browser_stores::store_by_id(browser_store_id(store))
        .ok_or_else(|| window_error(store))?;
    let label = profile.window_label();

    if app.get_webview_window(&label).is_none() {
        crate::store_window::open_store_home(app, profile.id).await?;
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| window_error(store))?;
    // Una ventana sin estado registrado quedó a medio abrir y su protección no
    // está confirmada: mismo criterio que `wishlist_session`.
    if !session::is_registered(&label) {
        return Err(AppError::new(
            "external_store_login_window",
            "El navegador integrado no está protegido, así que no se ha usado.",
        ));
    }

    window
        .navigate(login_url(store)?)
        .and_then(|()| window.unminimize())
        .and_then(|()| window.show())
        .and_then(|()| window.set_focus())
        .map_err(|_| window_error(store))?;

    let code = wait_for_code(app, store, &label, &window).await;

    // Pase lo que pase, la ventana no se queda enseñando la página de retorno.
    // En el caso de Epic esa página es el JSON con el código dentro.
    let _ = window.navigate(profile.home_url());
    if code.is_ok() {
        let _ = window.hide();
    }
    code
}

async fn wait_for_code<R: Runtime>(
    app: &AppHandle<R>,
    store: ExternalStore,
    label: &str,
    window: &tauri::WebviewWindow<R>,
) -> AppResult<String> {
    let deadline = tokio::time::Instant::now() + SIGN_IN_TIMEOUT;
    loop {
        // Cerrar la ventana es la forma natural de desistir, y se entiende así
        // en vez de dejar la operación colgada cinco minutos.
        if app.get_webview_window(label).is_none() || !session::is_registered(label) {
            return Err(cancelled_error(store));
        }

        if let Some(state) = session::snapshot(label)
            && let Ok(url) = Url::parse(&state.url)
            && is_landing(store, &url)
        {
            match store {
                // GOG deja el código en la propia URL: no hace falta mirar
                // dentro del documento, así que no se mira.
                ExternalStore::Gog => {
                    if let Some(code) = url
                        .query_pairs()
                        .find(|(key, _)| key == "code")
                        .map(|(_, value)| value.into_owned())
                    {
                        return validate_code(store, &code);
                    }
                    return Err(page_error(store, Some("sin_codigo")));
                }
                // Epic lo deja en el cuerpo, así que hay que esperar a que el
                // documento esté cargado antes de leerlo.
                ExternalStore::Epic => {
                    if !state.loading {
                        let raw = crate::steam::wishlist_session::evaluate_json(
                            window,
                            READ_EPIC_CODE_SCRIPT,
                        )
                        .await
                        .map_err(|_| page_error(store, None))?;
                        return parse_code_payload(store, &raw);
                    }
                }
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(timeout_error(store));
        }
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        READ_EPIC_CODE_SCRIPT, browser_store_id, is_landing, landing, login_url, page_error,
        parse_code_payload,
    };
    use crate::browser::stores as browser_stores;
    use crate::stores::ExternalStore;
    use tauri::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    #[test]
    fn each_store_lands_on_its_own_page_and_only_on_that_one() {
        assert!(is_landing(
            ExternalStore::Epic,
            &url("https://www.epicgames.com/id/api/redirect?clientId=abc&responseType=code")
        ));
        assert!(is_landing(
            ExternalStore::Gog,
            &url("https://embed.gog.com/on_login_success?origin=client&code=abc")
        ));

        // Una tienda nunca acepta la página de retorno de la otra.
        assert!(!is_landing(
            ExternalStore::Epic,
            &url("https://embed.gog.com/on_login_success?code=abc")
        ));
        assert!(!is_landing(
            ExternalStore::Gog,
            &url("https://www.epicgames.com/id/api/redirect")
        ));
    }

    #[test]
    fn a_lookalike_host_or_path_is_never_mistaken_for_the_landing_page() {
        for impostor in [
            // Sufijo: el anfitrión se compara entero, no por final.
            "https://www.epicgames.com.malo.example/id/api/redirect",
            "https://malo-www.epicgames.com/id/api/redirect",
            // Subdominio no declarado.
            "https://cuentas.epicgames.com/id/api/redirect",
            // Ruta que sólo empieza igual.
            "https://www.epicgames.com/id/api/redirect-falso",
            "https://www.epicgames.com/id/api/redirect/otra",
            // Sin TLS.
            "http://www.epicgames.com/id/api/redirect",
        ] {
            assert!(
                !is_landing(ExternalStore::Epic, &url(impostor)),
                "{impostor} no puede pasar por la página de retorno"
            );
        }

        for impostor in [
            "https://embed.gog.com.malo.example/on_login_success",
            "https://embed.gog.com/on_login_success/otra",
            "https://www.gog.com/on_login_success",
        ] {
            assert!(
                !is_landing(ExternalStore::Gog, &url(impostor)),
                "{impostor} no puede pasar por la página de retorno"
            );
        }
    }

    #[test]
    fn a_store_only_offers_in_app_login_if_it_can_reach_its_own_landing_page() {
        // Si la allowlist del navegador no admitiera la página de retorno, la
        // navegación se cancelaría y el código no llegaría nunca. En vez de
        // ofrecer un camino que falla a mitad, la capacidad se deriva de la
        // propia allowlist: ampliarla habilita la tienda sola.
        for store in ExternalStore::ALL {
            let profile = browser_stores::store_by_id(browser_store_id(store))
                .expect("la tienda existe en el navegador integrado");
            let expected = landing(store);
            let target = url(&format!("https://{}{}", expected.host, expected.path));
            assert_eq!(
                super::supports_in_app(store),
                profile.allows(&target),
                "{} debe ofrecer el inicio de sesión integrado exactamente cuando puede llegar a su página de retorno",
                store.as_str()
            );
        }
    }

    #[test]
    fn epic_can_sign_in_inside_vindexa_today() {
        // Es el caso que motivó todo esto: su página de retorno ya está en la
        // allowlist, así que nadie tiene que copiar un JSON nunca más.
        assert!(super::supports_in_app(ExternalStore::Epic));
    }

    #[test]
    fn an_unsupported_store_is_refused_before_opening_any_window() {
        // El mensaje no promete lo que no puede cumplir ni deja la ventana
        // abierta esperando un código que no va a llegar.
        let error = super::unsupported_error(ExternalStore::Gog);
        assert_eq!(error.code, "external_store_login_unsupported");
        assert!(error.message.contains("GOG"));
    }

    #[test]
    fn the_login_page_of_each_store_is_reachable_by_its_browser_window() {
        for store in ExternalStore::ALL {
            let profile = browser_stores::store_by_id(browser_store_id(store))
                .expect("la tienda existe en el navegador integrado");
            let target = login_url(store).expect("URL de inicio de sesión válida");
            assert!(
                profile.allows(&target),
                "{} no puede navegar a su propia página de inicio de sesión",
                store.as_str()
            );
        }
    }

    #[test]
    fn a_valid_payload_yields_the_code_and_nothing_else() {
        let code = "0123456789abcdef0123456789abcdef";
        let raw = format!(r#"{{"ok":true,"code":"{code}"}}"#);
        assert_eq!(
            parse_code_payload(ExternalStore::Epic, &raw).expect("código válido"),
            code
        );
    }

    #[test]
    fn a_payload_that_the_page_could_have_faked_is_still_validated() {
        // `ok: true` no basta: la forma del código se comprueba igual.
        for hostile in [
            r#"{"ok":true,"code":"../../etc/passwd"}"#,
            r#"{"ok":true,"code":"corto"}"#,
            r#"{"ok":true,"code":""}"#,
            r#"{"ok":true}"#,
            r#"{"ok":false,"error":"pagina"}"#,
            "no es json",
        ] {
            assert!(
                parse_code_payload(ExternalStore::Epic, hostile).is_err(),
                "{hostile} no puede aceptarse"
            );
        }
    }

    #[test]
    fn an_oversized_payload_is_refused_before_being_parsed() {
        let huge = format!(r#"{{"ok":true,"code":"{}"}}"#, "a".repeat(64 * 1024));
        assert!(parse_code_payload(ExternalStore::Epic, &huge).is_err());
    }

    #[test]
    fn no_string_from_the_page_can_reach_the_message() {
        // El guion sólo devuelve códigos internos conocidos, pero aunque
        // devolviera otra cosa, el mensaje es una frase fija.
        let error = page_error(ExternalStore::Epic, Some("<script>alert(1)</script>"));
        assert!(!error.message.contains("script"));
        assert!(error.message.contains("Epic Games Store"));
        assert_eq!(error.code, "external_store_login_page");
    }

    #[test]
    fn the_script_checks_the_page_again_and_reads_only_the_code() {
        // La primera comprobación la hace Rust, pero la página puede cambiar
        // entre el sondeo y la evaluación: el guion vuelve a mirar.
        assert!(READ_EPIC_CODE_SCRIPT.contains("location.hostname"));
        assert!(READ_EPIC_CODE_SCRIPT.contains("location.pathname"));
        assert!(READ_EPIC_CODE_SCRIPT.contains("authorizationCode"));
        // Y no toca nada más de la sesión de la tienda.
        for prohibido in ["document.cookie", "localStorage", "sessionStorage", "fetch("] {
            assert!(
                !READ_EPIC_CODE_SCRIPT.contains(prohibido),
                "el guion no puede tocar {prohibido}"
            );
        }
    }
}
