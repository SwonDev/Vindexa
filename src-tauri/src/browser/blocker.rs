//! Reglas nativas de bloqueo de anuncios y rastreo del navegador integrado.
//!
//! Las reglas son **datos versionados** y se compilan a la sintaxis de listas
//! de contenido de WebKit (`WKContentRuleList` en macOS, filtros de contenido
//! de WebKitGTK en Linux). Ambas plataformas comparten el mismo formato JSON.
//!
//! ## Por qué cambia el formato respecto de la versión 1
//!
//! En WebKit, `if-domain` **no** filtra por el dominio del recurso, sino por el
//! del documento principal. Una regla `{"url-filter": ".*", "if-domain":
//! ["*doubleclick.net"]}` solo actuaría si la propia página fuese
//! `doubleclick.net`, es decir, nunca. El bloqueo de red se expresa por tanto
//! con `url-filter` sobre la URL del recurso, e `if-domain` queda reservado a
//! las reglas cosméticas, donde sí se evalúa contra el documento.
//!
//! ## Orden de las reglas (crítico)
//!
//! 1. Bloqueo de red.
//! 2. `ignore-previous-rules` para los CDN de las tiendas.
//! 3. Reglas cosméticas.
//!
//! WebKit acumula acciones en orden e `ignore-previous-rules` descarta todo lo
//! anterior para esa carga. Poner el paso 2 al final anularía la cosmética; el
//! paso 2 antes de la cosmética garantiza a la vez que **ningún** dominio de
//! las tiendas pueda quedar bloqueado por una regla previa.

use crate::browser::stores::normalized_host;
use tauri::Url;

/// Dominio de red bloqueado, con el motivo documentado.
// El campo documenta la regla y lo consumen las pruebas y el informe de
// seguridad; no lo lee el código de producción.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct BlockedDomain {
    /// Dominio registrable o subdominio concreto; incluye sus subdominios.
    pub domain: &'static str,
    /// Motivo, para el informe de seguridad y para revisiones futuras.
    pub reason: &'static str,
}

/// Dominio que jamás puede bloquearse, con la función que cumple.
// El campo documenta la regla y lo consumen las pruebas y el informe de
// seguridad; no lo lee el código de producción.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct AllowedDomain {
    /// Dominio registrable o subdominio concreto; incluye sus subdominios.
    pub domain: &'static str,
    /// Qué se rompería si se bloquease.
    pub purpose: &'static str,
}

/// Redes de publicidad, analítica y rastreo bloqueadas.
///
/// Solo contiene terceros. La telemetría de primera parte de cada tienda no se
/// bloquea a propósito: la regla crítica es que la tienda nunca se rompa, y una
/// tienda puede acoplar su carrito o su sesión a su propio dominio.
pub const BLOCKED_DOMAINS: &[BlockedDomain] = &[
    // Google / DoubleClick
    BlockedDomain {
        domain: "doubleclick.net",
        reason: "red publicitaria de Google",
    },
    BlockedDomain {
        domain: "2mdn.net",
        reason: "servidor de creatividades de DoubleClick",
    },
    BlockedDomain {
        domain: "googlesyndication.com",
        reason: "entrega de anuncios AdSense",
    },
    BlockedDomain {
        domain: "googleadservices.com",
        reason: "conversiones de Google Ads",
    },
    BlockedDomain {
        domain: "googletagservices.com",
        reason: "etiquetas publicitarias de Google",
    },
    BlockedDomain {
        domain: "googletagmanager.com",
        reason: "contenedor de etiquetas de rastreo",
    },
    BlockedDomain {
        domain: "google-analytics.com",
        reason: "analítica de Google",
    },
    BlockedDomain {
        domain: "analytics.google.com",
        reason: "analítica de Google (GA4)",
    },
    BlockedDomain {
        domain: "adservice.google.com",
        reason: "servicio de anuncios de Google",
    },
    BlockedDomain {
        domain: "ad-delivery.net",
        reason: "entrega de anuncios de Google",
    },
    BlockedDomain {
        domain: "googleoptimize.com",
        reason: "experimentos con seguimiento",
    },
    BlockedDomain {
        domain: "app-measurement.com",
        reason: "medición de Firebase",
    },
    // Medición de audiencia
    BlockedDomain {
        domain: "scorecardresearch.com",
        reason: "panel de audiencia de Comscore",
    },
    BlockedDomain {
        domain: "imrworldwide.com",
        reason: "medición de Nielsen",
    },
    BlockedDomain {
        domain: "quantserve.com",
        reason: "medición de Quantcast",
    },
    BlockedDomain {
        domain: "quantcount.com",
        reason: "medición de Quantcast",
    },
    BlockedDomain {
        domain: "chartbeat.com",
        reason: "analítica de audiencia",
    },
    BlockedDomain {
        domain: "chartbeat.net",
        reason: "analítica de audiencia",
    },
    // Redes sociales usadas como píxel
    BlockedDomain {
        domain: "facebook.net",
        reason: "píxel y SDK de Meta",
    },
    BlockedDomain {
        domain: "ads-twitter.com",
        reason: "píxel publicitario de X",
    },
    BlockedDomain {
        domain: "analytics.twitter.com",
        reason: "analítica de X",
    },
    BlockedDomain {
        domain: "t.co",
        reason: "redirector de medición de X",
    },
    BlockedDomain {
        domain: "analytics.tiktok.com",
        reason: "píxel de TikTok",
    },
    BlockedDomain {
        domain: "sc-static.net",
        reason: "píxel de Snap",
    },
    BlockedDomain {
        domain: "tr.snapchat.com",
        reason: "conversiones de Snap",
    },
    BlockedDomain {
        domain: "ct.pinterest.com",
        reason: "píxel de Pinterest",
    },
    BlockedDomain {
        domain: "alb.reddit.com",
        reason: "píxel de Reddit",
    },
    // Intercambios publicitarios
    BlockedDomain {
        domain: "adnxs.com",
        reason: "intercambio publicitario Xandr",
    },
    BlockedDomain {
        domain: "rubiconproject.com",
        reason: "intercambio publicitario Magnite",
    },
    BlockedDomain {
        domain: "pubmatic.com",
        reason: "intercambio publicitario",
    },
    BlockedDomain {
        domain: "openx.net",
        reason: "intercambio publicitario",
    },
    BlockedDomain {
        domain: "casalemedia.com",
        reason: "intercambio publicitario Index",
    },
    BlockedDomain {
        domain: "33across.com",
        reason: "intercambio publicitario",
    },
    BlockedDomain {
        domain: "adsrvr.org",
        reason: "plataforma de compra The Trade Desk",
    },
    BlockedDomain {
        domain: "criteo.com",
        reason: "reorientación publicitaria",
    },
    BlockedDomain {
        domain: "criteo.net",
        reason: "reorientación publicitaria",
    },
    BlockedDomain {
        domain: "taboola.com",
        reason: "recomendación publicitaria",
    },
    BlockedDomain {
        domain: "outbrain.com",
        reason: "recomendación publicitaria",
    },
    BlockedDomain {
        domain: "amazon-adsystem.com",
        reason: "red publicitaria de Amazon",
    },
    BlockedDomain {
        domain: "smartadserver.com",
        reason: "servidor de anuncios",
    },
    BlockedDomain {
        domain: "adform.net",
        reason: "plataforma publicitaria",
    },
    BlockedDomain {
        domain: "mathtag.com",
        reason: "sincronización de identificadores",
    },
    BlockedDomain {
        domain: "bidswitch.net",
        reason: "enrutado de pujas",
    },
    BlockedDomain {
        domain: "sharethrough.com",
        reason: "publicidad nativa",
    },
    BlockedDomain {
        domain: "teads.tv",
        reason: "publicidad en vídeo",
    },
    BlockedDomain {
        domain: "yieldmo.com",
        reason: "publicidad",
    },
    BlockedDomain {
        domain: "serving-sys.com",
        reason: "servidor de creatividades",
    },
    BlockedDomain {
        domain: "moatads.com",
        reason: "verificación publicitaria",
    },
    BlockedDomain {
        domain: "doubleverify.com",
        reason: "verificación publicitaria",
    },
    BlockedDomain {
        domain: "adsafeprotected.com",
        reason: "verificación publicitaria",
    },
    BlockedDomain {
        domain: "adroll.com",
        reason: "reorientación publicitaria",
    },
    BlockedDomain {
        domain: "media.net",
        reason: "red publicitaria",
    },
    // Gestión de datos e identidad
    BlockedDomain {
        domain: "demdex.net",
        reason: "gestión de audiencias de Adobe",
    },
    BlockedDomain {
        domain: "omtrdc.net",
        reason: "recogida de Adobe Analytics",
    },
    BlockedDomain {
        domain: "everesttech.net",
        reason: "identidad publicitaria de Adobe",
    },
    BlockedDomain {
        domain: "2o7.net",
        reason: "recogida heredada de Adobe",
    },
    BlockedDomain {
        domain: "bluekai.com",
        reason: "plataforma de datos de Oracle",
    },
    BlockedDomain {
        domain: "krxd.net",
        reason: "plataforma de datos Krux",
    },
    BlockedDomain {
        domain: "rlcdn.com",
        reason: "identidad de LiveRamp",
    },
    BlockedDomain {
        domain: "agkn.com",
        reason: "identidad de Neustar",
    },
    // Producto, sesión y atribución
    BlockedDomain {
        domain: "hotjar.com",
        reason: "grabación de sesión",
    },
    BlockedDomain {
        domain: "hotjar.io",
        reason: "grabación de sesión",
    },
    BlockedDomain {
        domain: "mouseflow.com",
        reason: "grabación de sesión",
    },
    BlockedDomain {
        domain: "fullstory.com",
        reason: "grabación de sesión",
    },
    BlockedDomain {
        domain: "logrocket.com",
        reason: "grabación de sesión",
    },
    BlockedDomain {
        domain: "crazyegg.com",
        reason: "mapas de calor",
    },
    BlockedDomain {
        domain: "clarity.ms",
        reason: "grabación de sesión de Microsoft",
    },
    BlockedDomain {
        domain: "bat.bing.com",
        reason: "píxel de Microsoft Ads",
    },
    BlockedDomain {
        domain: "mc.yandex.ru",
        reason: "analítica de Yandex",
    },
    BlockedDomain {
        domain: "mixpanel.com",
        reason: "analítica de producto",
    },
    BlockedDomain {
        domain: "amplitude.com",
        reason: "analítica de producto",
    },
    BlockedDomain {
        domain: "segment.com",
        reason: "enrutado de eventos",
    },
    BlockedDomain {
        domain: "segment.io",
        reason: "enrutado de eventos",
    },
    BlockedDomain {
        domain: "heapanalytics.com",
        reason: "analítica de producto",
    },
    BlockedDomain {
        domain: "nr-data.net",
        reason: "baliza de New Relic",
    },
    BlockedDomain {
        domain: "cloudflareinsights.com",
        reason: "baliza de medición",
    },
    BlockedDomain {
        domain: "branch.io",
        reason: "atribución de campañas",
    },
    BlockedDomain {
        domain: "appsflyer.com",
        reason: "atribución de campañas",
    },
    BlockedDomain {
        domain: "adjust.com",
        reason: "atribución de campañas",
    },
    BlockedDomain {
        domain: "kochava.com",
        reason: "atribución de campañas",
    },
];

/// Dominios que ninguna regla puede bloquear.
///
/// Es la garantía explícita de que la tienda sigue funcionando: imágenes,
/// capsulas, capturas, tráilers, hojas de estilo, inicio de sesión y carrito.
pub const ALLOWED_DOMAINS: &[AllowedDomain] = &[
    // --- Steam ---
    AllowedDomain {
        domain: "steampowered.com",
        purpose: "tienda, API pública, inicio de sesión, carrito, ayuda y medios",
    },
    AllowedDomain {
        domain: "steamstatic.com",
        purpose: "CDN de Steam: cdn/shared/store/community/avatars/video/clan en Akamai, Cloudflare y Fastly",
    },
    AllowedDomain {
        domain: "steamcommunity.com",
        purpose: "comunidad, mercado y capturas",
    },
    AllowedDomain {
        domain: "steamgames.com",
        purpose: "recursos estáticos heredados de Steam",
    },
    AllowedDomain {
        domain: "steam-chat.com",
        purpose: "vista web del chat de Steam",
    },
    AllowedDomain {
        domain: "steamcdn-a.akamaihd.net",
        purpose: "CDN heredada de imágenes y vídeos de Steam",
    },
    AllowedDomain {
        domain: "steamstore-a.akamaihd.net",
        purpose: "CDN heredada de la tienda",
    },
    AllowedDomain {
        domain: "steamuserimages-a.akamaihd.net",
        purpose: "capturas subidas por la comunidad",
    },
    AllowedDomain {
        domain: "steamcommunity-a.akamaihd.net",
        purpose: "CDN heredada de la comunidad",
    },
    AllowedDomain {
        domain: "steambroadcast.akamaized.net",
        purpose: "retransmisiones de Steam",
    },
    // --- GOG ---
    AllowedDomain {
        domain: "gog.com",
        purpose: "tienda, sesión y API de GOG",
    },
    AllowedDomain {
        domain: "gog-statics.com",
        purpose: "imágenes y recursos estáticos de GOG",
    },
    AllowedDomain {
        domain: "gogcdn.net",
        purpose: "CDN de GOG",
    },
    // --- Epic ---
    AllowedDomain {
        domain: "epicgames.com",
        purpose: "tienda, sesión, GraphQL y verificación de Epic",
    },
    AllowedDomain {
        domain: "unrealengine.com",
        purpose: "CDN de imágenes y vídeos de Epic",
    },
    AllowedDomain {
        domain: "epicgames-download1.akamaized.net",
        purpose: "CDN de medios de Epic",
    },
    // --- itch.io ---
    AllowedDomain {
        domain: "itch.io",
        purpose: "tienda y páginas de creadores",
    },
    AllowedDomain {
        domain: "itch.zone",
        purpose: "imágenes y juegos HTML5 de itch.io",
    },
    AllowedDomain {
        domain: "hwcdn.net",
        purpose: "CDN donde itch.io sirve los juegos web",
    },
    // --- Verificación humana compartida ---
    AllowedDomain {
        domain: "recaptcha.net",
        purpose: "reCAPTCHA en el inicio de sesión",
    },
    AllowedDomain {
        domain: "gstatic.com",
        purpose: "recursos de reCAPTCHA y tipografías",
    },
    AllowedDomain {
        domain: "hcaptcha.com",
        purpose: "hCaptcha en el inicio de sesión",
    },
];

/// ¿Coincide `host` con `domain` o con uno de sus subdominios?
#[allow(dead_code)]
fn domain_matches(host: &str, domain: &str) -> bool {
    match host.strip_suffix(domain) {
        None => false,
        Some("") => true,
        Some(prefix) => prefix.len() > 1 && prefix.ends_with('.'),
    }
}

/// ¿Está este dominio en la lista de permitidos?
#[allow(dead_code)]
pub fn is_allowed_asset_host(host: &str) -> bool {
    ALLOWED_DOMAINS
        .iter()
        .any(|allowed| domain_matches(host, allowed.domain))
}

/// Espejo en Rust de la decisión que tomará WebKit para un subrecurso.
///
/// Existe para poder probar la lista con un corpus de URLs sin arrancar un
/// webview. Refleja el mismo orden: bloqueo, después lista de permitidos.
#[allow(dead_code)]
pub fn is_blocked_request(url: &Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    let Some(host) = normalized_host(url) else {
        return false;
    };
    if is_allowed_asset_host(&host) {
        return false;
    }
    BLOCKED_DOMAINS
        .iter()
        .any(|blocked| domain_matches(&host, blocked.domain))
}

#[cfg(test)]
mod tests {
    use super::*;
    // La lista de contenido vive en su propio módulo y sólo existe en las
    // plataformas que saben instalarla; sus pruebas llevan el mismo `cfg`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use super::lista_de_contenido::{
        content_rule_list_json, escaped_domain, host_url_filter, rule_count, rules,
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    use serde_json::Value;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    /// Corpus de recursos que **deben** bloquearse.
    const BLOCKED_CORPUS: &[&str] = &[
        "https://stats.g.doubleclick.net/j/collect?v=1",
        "https://securepubads.g.doubleclick.net/tag/js/gpt.js",
        "https://www.google-analytics.com/analytics.js",
        "https://www.googletagmanager.com/gtag/js?id=G-XXXX",
        "https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js",
        "https://sb.scorecardresearch.com/b?c1=2&c2=6036284",
        "https://connect.facebook.net/en_US/fbevents.js",
        "https://static.criteo.net/js/ld/ld.js",
        "https://ib.adnxs.com/ttj?id=1234",
        "https://c.amazon-adsystem.com/aax2/apstag.js",
        "https://script.hotjar.com/modules.js",
        "https://cdn.mouseflow.com/projects/x.js",
        "https://www.clarity.ms/tag/abcdef",
        "https://bat.bing.com/bat.js",
        "https://mc.yandex.ru/metrika/tag.js",
        "https://api.mixpanel.com/track",
        "https://cdn.segment.com/analytics.js/v1/x/analytics.min.js",
        "https://bam.nr-data.net/1/abc",
        "https://static.cloudflareinsights.com/beacon.min.js",
        "https://analytics.tiktok.com/i18n/pixel/events.js",
        "https://t.co/i/adsct",
        "https://dpm.demdex.net/id?d_visid_ver=5",
        "https://cdn.branch.io/branch-latest.min.js",
    ];

    /// Corpus de recursos que **jamás** pueden bloquearse.
    const ALLOWED_CORPUS: &[&str] = &[
        // Steam: imágenes, cápsulas, capturas
        "https://cdn.akamai.steamstatic.com/steam/apps/620/header.jpg",
        "https://cdn.cloudflare.steamstatic.com/steam/apps/620/capsule_616x353.jpg",
        "https://cdn.fastly.steamstatic.com/steam/apps/620/library_600x900.jpg",
        "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/ss_1.jpg",
        // Carátulas del carrusel «Más como esto», servidas por el nodo Fastly:
        // aparecen a medio cargar en cualquier captura y es fácil confundirlas
        // con un bloqueo, así que quedan fijadas como caso de regresión.
        "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/1462040/header.jpg?t=1773895755",
        "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/39210/header_2x.jpg?t=1782870674",
        "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/2993780/ss_790f3c235.600x338.jpg?t=1749900924",
        "https://community.fastly.steamstatic.com/public/css/skin_1/header.css",
        "https://cdn.fastly.steamstatic.com/store/some-asset.js",
        "https://clan.fastly.steamstatic.com/images/1234/evento.png",
        // El intercambio de sesión de Steam entre sus dominios.
        "https://login.steampowered.com/jwt/ajaxrefresh",
        "https://steamcommunity.com/login/settoken",
        "https://api.steampowered.com/IAuthenticationService/BeginAuthSessionViaCredentials/v1",
        "https://store.akamai.steamstatic.com/public/shared/javascript/main.js",
        "https://community.akamai.steamstatic.com/public/css/skin_1/header.css",
        "https://avatars.fastly.steamstatic.com/abcdef_full.jpg",
        "https://clan.akamai.steamstatic.com/images/1234/evento.png",
        // Steam: vídeo y tráilers
        "https://video.akamai.steamstatic.com/store_trailers/256660296/movie480_vp9.webm",
        "https://video.cloudflare.steamstatic.com/store_trailers/256660296/movie_max.mp4",
        "https://video.fastly.steamstatic.com/store_trailers/256660296/movie480.webm",
        "https://steamcdn-a.akamaihd.net/steam/apps/256660296/movie480.webm",
        "https://steamuserimages-a.akamaihd.net/ugc/123/AAAA/",
        "https://steambroadcast.akamaized.net/broadcast/x.m3u8",
        // Steam: tienda, sesión y carrito
        "https://store.steampowered.com/app/620/Portal_2/",
        "https://store.steampowered.com/api/appdetails?appids=620",
        "https://login.steampowered.com/jwt/refresh",
        "https://checkout.steampowered.com/checkout/",
        "https://help.steampowered.com/es/",
        "https://steamcommunity.com/market/",
        "https://media.steampowered.com/apps/620/x.mp3",
        // Verificación humana
        "https://www.recaptcha.net/recaptcha/api.js",
        "https://www.gstatic.com/recaptcha/releases/x/recaptcha__es.js",
        "https://newassets.hcaptcha.com/captcha/v1/x/static/hcaptcha.html",
        // GOG
        "https://www.gog.com/es/game/cyberpunk_2077",
        "https://images.gog-statics.com/abc_product_card.png",
        "https://static.gog.com/js/app.js",
        "https://login.gog.com/auth",
        // Epic
        "https://store.epicgames.com/es-ES/p/fortnite",
        "https://cdn1.epicgames.com/offer/x/hero.jpg",
        "https://cdn2.unrealengine.com/x/video.mp4",
        "https://graphql.epicgames.com/graphql",
        // itch.io
        "https://itch.io/games",
        "https://img.itch.zone/abc/original/x.png",
        "https://html-classic.itch.zone/html/1234/index.html",
        "https://v6p9d9t4ssl.hwcdn.net/html/1234/index.html",
    ];

    #[test]
    fn the_blocked_corpus_is_blocked() {
        for raw in BLOCKED_CORPUS {
            assert!(is_blocked_request(&url(raw)), "{raw} debería bloquearse");
        }
    }

    #[test]
    fn the_store_corpus_is_never_blocked() {
        for raw in ALLOWED_CORPUS {
            assert!(
                !is_blocked_request(&url(raw)),
                "{raw} nunca puede bloquearse: rompería la tienda"
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_allowlist_wins_over_the_blocklist() {
        // Aunque alguien añadiese un dominio de Steam a la lista de bloqueo, la
        // regla `ignore-previous-rules` posterior lo rescataría.
        for allowed in ALLOWED_DOMAINS {
            let candidate = url(&format!("https://cdn.{}/recurso.js", allowed.domain));
            assert!(
                !is_blocked_request(&candidate),
                "{} está en la lista de permitidos",
                allowed.domain
            );
        }
        let rules = rules();
        let first_ignore = rules
            .iter()
            .position(|rule| rule["action"]["type"] == "ignore-previous-rules")
            .expect("hay reglas de permiso");
        let last_block = rules
            .iter()
            .rposition(|rule| rule["action"]["type"] == "block")
            .expect("hay reglas de bloqueo");
        assert!(
            last_block < first_ignore,
            "los permisos deben evaluarse después de los bloqueos"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn cosmetic_rules_are_last_so_nothing_cancels_them() {
        let rules = rules();
        let first_cosmetic = rules
            .iter()
            .position(|rule| rule["action"]["type"] == "css-display-none")
            .expect("hay reglas cosméticas");
        let last_ignore = rules
            .iter()
            .rposition(|rule| rule["action"]["type"] == "ignore-previous-rules")
            .expect("hay reglas de permiso");
        assert!(
            last_ignore < first_cosmetic,
            "`ignore-previous-rules` antes de la cosmética; si no, la anularía"
        );
    }

    #[test]
    fn domain_matching_is_boundary_safe() {
        assert!(domain_matches("doubleclick.net", "doubleclick.net"));
        assert!(domain_matches("stats.g.doubleclick.net", "doubleclick.net"));
        assert!(!domain_matches("evildoubleclick.net", "doubleclick.net"));
        assert!(!domain_matches(
            "doubleclick.net.attacker.tld",
            "doubleclick.net"
        ));
        assert!(!domain_matches(".doubleclick.net", "doubleclick.net"));
        // Un host que suplanta un CDN de Steam no hereda su permiso.
        assert!(!is_allowed_asset_host("steamstatic.com.attacker.tld"));
        assert!(!is_allowed_asset_host("evilsteamstatic.com"));
        assert!(is_allowed_asset_host("video.akamai.steamstatic.com"));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn url_filters_are_anchored_and_escaped() {
        let filter = host_url_filter("doubleclick.net");
        assert_eq!(
            filter,
            r"^https?://([a-z0-9_.-]+\.)?doubleclick\.net([/:?&#].*)?$"
        );
        assert!(filter.starts_with("^https?://"));
        assert!(filter.ends_with('$'));
        assert_eq!(escaped_domain("adform.net"), r"adform\.net");
        for blocked in BLOCKED_DOMAINS {
            let filter = host_url_filter(blocked.domain);
            assert!(
                filter.contains(&escaped_domain(blocked.domain)),
                "{} debe aparecer escapado en su filtro",
                blocked.domain
            );
            assert!(
                !filter.contains('{') && !filter.contains('}'),
                "WebKit no admite cuantificadores de repeticion en url-filter"
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_compiled_list_is_valid_json_with_the_expected_shape() {
        let compiled = content_rule_list_json();
        let parsed: Value = serde_json::from_str(&compiled).expect("JSON válido");
        let array = parsed.as_array().expect("la lista es un array");
        assert_eq!(array.len(), rule_count());
        assert!(array.len() < 50_000, "WebKit limita el tamaño de la lista");
        for rule in array {
            let trigger = rule.get("trigger").expect("toda regla lleva trigger");
            assert!(
                trigger.get("url-filter").is_some(),
                "WebKit exige url-filter en cada trigger"
            );
            let action = rule.get("action").expect("toda regla lleva action");
            let kind = action["type"].as_str().unwrap();
            assert!(
                matches!(kind, "block" | "ignore-previous-rules" | "css-display-none"),
                "acción inesperada: {kind}"
            );
            if kind == "css-display-none" {
                assert!(action["selector"].as_str().is_some_and(|s| !s.is_empty()));
                let domains = trigger["if-domain"]
                    .as_array()
                    .expect("cosmética por dominio");
                assert!(domains.iter().all(|d| d.as_str().unwrap().starts_with('*')));
            }
        }
    }

    #[test]
    fn the_blocklist_never_shadows_a_store_domain() {
        for blocked in BLOCKED_DOMAINS {
            assert!(
                !is_allowed_asset_host(blocked.domain),
                "{} no puede estar bloqueado y permitido a la vez",
                blocked.domain
            );
            assert!(
                !blocked.reason.is_empty(),
                "{} necesita un motivo documentado",
                blocked.domain
            );
        }
        for allowed in ALLOWED_DOMAINS {
            assert!(
                !allowed.purpose.is_empty(),
                "{} necesita justificar su permiso",
                allowed.domain
            );
        }
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_identifier_is_versioned() {
        assert_eq!(BLOCKER_ID, "io.vindexa.browser-protection.v2");
        assert!(BLOCKER_ID.starts_with("io.vindexa."));
    }

    #[test]
    fn non_web_schemes_are_not_the_blockers_business() {
        assert!(!is_blocked_request(&url("about:blank")));
        assert!(!is_blocked_request(&url("vindexa-browser://abc/back")));
    }
}

// ---------------------------------------------------------------------------
// Lista de contenido nativa
// ---------------------------------------------------------------------------

/// Reglas en el formato de lista de contenido de WebKit.
///
/// Vive en un módulo aparte porque **sólo se compila donde hay un motor web que
/// sepa instalarla**: WKWebView en macOS y WebKitGTK en Linux. WebView2 no
/// ofrece nada equivalente todavía, así que en Windows estas reglas no tendrían
/// quién las aplicase.
///
/// Están todas juntas a propósito. Cuando estaban repartidas por el módulo, cada
/// intento de excluirlas de Windows dejaba fuera una constante que sólo usaba
/// otra de ellas, y el siguiente aviso aparecía en la compilación siguiente. Un
/// solo `cfg` sobre el conjunto cierra esa cadena.
#[cfg(any(target_os = "macos", target_os = "linux"))]
mod lista_de_contenido {
    use super::{ALLOWED_DOMAINS, BLOCKED_DOMAINS};
    use serde_json::{Value, json};

    /// Identificador versionado de la lista compilada.
    ///
    /// Cambiarlo obliga a WebKit a recompilar la lista en lugar de reutilizar una
    /// versión anterior almacenada en caché.
    // El bloqueador nativo sólo existe donde el motor web sabe instalar una
    // lista de contenido: WKWebView en macOS y WebKitGTK en Linux. WebView2 no
    // ofrece nada equivalente todavía, así que en Windows estas reglas no
    // tendrían quién las aplicase y el módulo no las compila.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub const BLOCKER_ID: &str = "io.vindexa.browser-protection.v2";

    /// Regla cosmética: oculta elementos en los documentos de ciertos dominios.
    // El campo documenta la regla y lo consumen las pruebas y el informe de
    // seguridad; no lo lee el código de producción.
    #[allow(dead_code)]
    #[derive(Debug, Clone, Copy)]
    pub struct CosmeticRule {
        /// Dominios de documento a los que aplica (se les añade `*` para subdominios).
        pub domains: &'static [&'static str],
        /// Selector CSS, en una sola cadena separada por comas.
        pub selector: &'static str,
        /// Qué oculta, para poder auditarla.
        pub purpose: &'static str,
    }

    /// Dominios de documento de cada tienda para las reglas cosméticas.
    const STEAM_DOCUMENTS: &[&str] = &[
        "store.steampowered.com",
        "steamcommunity.com",
        "checkout.steampowered.com",
        "help.steampowered.com",
    ];
    const GOG_DOCUMENTS: &[&str] = &["gog.com"];
    const EPIC_DOCUMENTS: &[&str] = &["epicgames.com"];
    const ITCH_DOCUMENTS: &[&str] = &["itch.io"];
    const ALL_STORE_DOCUMENTS: &[&str] = &[
        "store.steampowered.com",
        "steamcommunity.com",
        "checkout.steampowered.com",
        "help.steampowered.com",
        "gog.com",
        "epicgames.com",
        "itch.io",
    ];

    /// Reglas cosméticas.
    ///
    /// Deliberadamente **no** se oculta la verja de edad de Steam: Steam sirve un
    /// documento distinto para el contenido adulto, así que ocultarla con CSS
    /// dejaría una página en blanco en lugar de la ficha. Eso incumpliría la regla
    /// de no romper funcionalidad legítima.
    pub const COSMETIC_RULES: &[CosmeticRule] = &[
        CosmeticRule {
            domains: ALL_STORE_DOCUMENTS,
            selector: "#onetrust-consent-sdk, #onetrust-banner-sdk, .onetrust-pc-dark-filter, #CybotCookiebotDialog, #CybotCookiebotDialogBodyUnderlay, #didomi-host, .qc-cmp2-container, #qc-cmp2-container, #truste-consent-track, .truste_overlay, .osano-cm-window, #usercentrics-root",
            purpose: "plataformas de consentimiento de terceros (OneTrust, Cookiebot, Didomi, Quantcast, TrustArc, Osano, Usercentrics)",
        },
        CosmeticRule {
            domains: STEAM_DOCUMENTS,
            selector: "#cookiePrefPopup, .cookiepreferences_popup, .cookiepreferences_popup_overlay, .responsive_optin_banner, #responsive_optin_banner",
            purpose: "aviso de cookies de Steam y banner que empuja a la app móvil",
        },
        CosmeticRule {
            domains: GOG_DOCUMENTS,
            selector: ".cookie-banner, .newsletter-popup, .promo-banner--sticky",
            purpose: "aviso de cookies, superposición de suscripción y franja promocional fija de GOG",
        },
        CosmeticRule {
            domains: EPIC_DOCUMENTS,
            selector: "[data-testid='cookie-banner'], .cookie-consent-banner, [class*='NewsletterSignup']",
            purpose: "aviso de cookies y superposición de boletín de Epic",
        },
        CosmeticRule {
            domains: ITCH_DOCUMENTS,
            selector: ".cookie_banner, #cookie_banner, .newsletter_signup_widget",
            purpose: "aviso de cookies y captación de boletín de itch.io",
        },
    ];

    /// Escapa un dominio para insertarlo en un `url-filter`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(super) fn escaped_domain(domain: &str) -> String {
        domain.replace('.', r"\.")
    }

    /// Patrón `url-filter` que cubre un dominio y todos sus subdominios.
    ///
    /// Anclado al principio de la URL y cerrado por un delimitador, de modo que
    /// `doubleclick\.net` no coincide con `evildoubleclick.net` ni con
    /// `doubleclick.net.attacker.tld`.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn host_url_filter(domain: &str) -> String {
        format!(
            r"^https?://([a-z0-9_.-]+\.)?{}([/:?&#].*)?$",
            escaped_domain(domain)
        )
    }

    /// Construye la lista de reglas en el orden exigido por WebKit.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub(super) fn rules() -> Vec<Value> {
        let mut rules = Vec::with_capacity(
            BLOCKED_DOMAINS.len() + ALLOWED_DOMAINS.len() + COSMETIC_RULES.len(),
        );

        for blocked in BLOCKED_DOMAINS {
            rules.push(json!({
                "trigger": {
                    "url-filter": host_url_filter(blocked.domain),
                    "url-filter-is-case-sensitive": false
                },
                "action": { "type": "block" }
            }));
        }

        for allowed in ALLOWED_DOMAINS {
            rules.push(json!({
                "trigger": {
                    "url-filter": host_url_filter(allowed.domain),
                    "url-filter-is-case-sensitive": false
                },
                "action": { "type": "ignore-previous-rules" }
            }));
        }

        for cosmetic in COSMETIC_RULES {
            let domains: Vec<String> = cosmetic
                .domains
                .iter()
                .map(|domain| format!("*{domain}"))
                .collect();
            rules.push(json!({
                "trigger": {
                    "url-filter": ".*",
                    "if-domain": domains
                },
                "action": {
                    "type": "css-display-none",
                    "selector": cosmetic.selector
                }
            }));
        }

        rules
    }

    /// Lista de contenido compilable, en JSON.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn content_rule_list_json() -> String {
        serde_json::to_string(&Value::Array(rules()))
            .expect("las reglas del bloqueador siempre serializan")
    }

    /// Número total de reglas generadas. Útil para vigilar el tamaño de la lista.
    #[allow(dead_code)]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn rule_count() -> usize {
        BLOCKED_DOMAINS.len() + ALLOWED_DOMAINS.len() + COSMETIC_RULES.len()
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub use lista_de_contenido::{BLOCKER_ID, content_rule_list_json};
