//! Registro de tiendas soportadas por el navegador integrado.
//!
//! Cada tienda declara de forma explícita los hosts a los que puede navegar la
//! ventana. La verificación es estructural sobre el host ya normalizado por
//! `url::Url` (minúsculas, punycode, sin puerto), nunca una comparación por
//! sufijo desnudo: `evilstore.steampowered.com.attacker.tld` no coincide con
//! ninguna regla porque `HostRule::WithSubdomains` exige un punto separador y
//! `HostRule::Exact` exige igualdad completa.

use tauri::Url;

/// Regla de host permitido para la navegación de nivel superior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostRule {
    /// Solo este host, carácter a carácter.
    Exact(&'static str),
    /// Este host y sus subdominios reales (`base` o `algo.base`).
    ///
    /// Se usa únicamente donde el catálogo vive en subdominios por creador y
    /// una lista exacta sería imposible de mantener (itch.io).
    WithSubdomains(&'static str),
}

impl HostRule {
    /// Comprueba un host **ya normalizado** (minúsculas, sin punto final).
    pub fn matches(&self, host: &str) -> bool {
        match self {
            Self::Exact(expected) => host == *expected,
            Self::WithSubdomains(base) => match host.strip_suffix(base) {
                None => false,
                Some("") => true,
                Some(prefix) => prefix.len() > 1 && prefix.ends_with('.'),
            },
        }
    }

    /// Dominio base de la regla, para construir filtros y reglas cosméticas.
    pub fn domain(&self) -> &'static str {
        match self {
            Self::Exact(domain) | Self::WithSubdomains(domain) => domain,
        }
    }
}

/// Perfil completo de una tienda: destinos, inicio, buscador y cosmética.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreProfile {
    /// Identificador estable e interno (etiquetas de ventana, preferencias).
    pub id: &'static str,
    /// Nombre visible en la barra y en el título de la ventana.
    pub name: &'static str,
    /// Página de inicio de la tienda.
    pub home: &'static str,
    /// Hosts permitidos para la navegación de nivel superior.
    pub hosts: &'static [HostRule],
    /// Plantilla de búsqueda dentro de la tienda; `{q}` recibe el término ya
    /// codificado en formato `application/x-www-form-urlencoded`.
    pub search_template: &'static str,
    /// Agente de usuario propio; `None` conserva el del sistema.
    ///
    /// Conservar el del sistema es lo correcto por defecto: un agente inventado
    /// empeora la huella digital y rompe la detección de plataforma de Steam.
    pub user_agent: Option<&'static str>,
    /// UUID del almacén de datos aislado que WebKit usa para esta tienda.
    ///
    /// Es la sesión de la persona usuaria: cambiarlo la cerraría, así que estos
    /// valores son fijos. Uno por tienda para que ninguna vea las cookies de
    /// otra ni las de la ventana principal de la aplicación.
    pub session_store: &'static str,
}

impl StoreProfile {
    /// Etiqueta de la ventana dedicada a esta tienda.
    pub fn window_label(&self) -> String {
        format!("vindexa-store-{}", self.id)
    }

    /// Título de la ventana, en español y sin datos de la sesión.
    pub fn window_title(&self) -> String {
        format!("{} · navegador protegido — Vindexa", self.name)
    }

    /// Identificador binario del almacén de datos aislado de esta tienda.
    ///
    /// Sólo lo consume `WKWebsiteDataStore` de WKWebView, que es la única API de
    /// las tres que identifica el almacén por UUID. En Windows y en Linux el
    /// aislamiento se consigue con un directorio de datos distinto por tienda.
    #[cfg(target_os = "macos")]
    pub fn session_store_id(&self) -> Option<[u8; 16]> {
        uuid::Uuid::parse_str(self.session_store)
            .ok()
            .map(uuid::Uuid::into_bytes)
    }

    /// URL de inicio ya validada contra el propio perfil.
    pub fn home_url(&self) -> Url {
        Url::parse(self.home).expect("la página de inicio de cada tienda es una URL válida")
    }

    /// ¿Puede la ventana de esta tienda navegar a esta URL?
    pub fn allows(&self, url: &Url) -> bool {
        let Some(host) = normalized_host(url) else {
            return false;
        };
        self.hosts.iter().any(|rule| rule.matches(&host))
    }

    /// Construye la URL de búsqueda interna de la tienda para un término libre.
    pub fn search_url(&self, term: &str) -> Option<Url> {
        let encoded: String =
            url::form_urlencoded::byte_serialize(term.trim().as_bytes()).collect();
        if encoded.is_empty() {
            return None;
        }
        let candidate = Url::parse(&self.search_template.replace("{q}", &encoded)).ok()?;
        // Un fallo de plantilla nunca puede escaparse de la allowlist.
        self.allows(&candidate).then_some(candidate)
    }
}

/// Steam. Además de la tienda se permiten los hosts que hacen falta para que
/// el inicio de sesión, el carrito, la comunidad y la ayuda funcionen sin
/// expulsar a la persona usuaria a un navegador externo.
const STEAM_HOSTS: &[HostRule] = &[
    HostRule::Exact("store.steampowered.com"),
    HostRule::Exact("checkout.steampowered.com"),
    HostRule::Exact("login.steampowered.com"),
    HostRule::Exact("help.steampowered.com"),
    HostRule::Exact("steamcommunity.com"),
    HostRule::Exact("www.steamcommunity.com"),
];

// Mismo motivo que en Epic: el inicio de sesión pasa por varios subdominios
// —`login`, `auth`, `embed`— y la lista exacta se quedaba corta en cuanto GOG
// tocaba uno.
const GOG_HOSTS: &[HostRule] = &[
    HostRule::WithSubdomains("gog.com"),
    // Página a la que GOG devuelve tras iniciar sesión, con el código de
    // autorización en la URL. Es el destino que registra su propio cliente, y
    // sin él la navegación se cancela y el inicio de sesión integrado no puede
    // terminar (ver `crate::stores::login_window`).
    HostRule::Exact("embed.gog.com"),
];

// El inicio de sesión de Epic reparte el flujo entre varios subdominios suyos
// —la cuenta, el captcha propio, los servicios de identidad— y enumerarlos uno
// a uno deja el proceso a medias en cuanto Epic cambia uno. Todos pertenecen a
// la misma organización, así que la regla admite sus subdominios; la superficie
// sigue acotada al dominio de la tienda y nada más.
const EPIC_HOSTS: &[HostRule] = &[HostRule::WithSubdomains("epicgames.com")];

// itch.io publica cada página de creador en su propio subdominio, así que aquí
// una lista exacta es inviable. La regla exige el punto separador.
const ITCH_HOSTS: &[HostRule] = &[HostRule::WithSubdomains("itch.io")];

/// Catálogo cerrado de tiendas. Añadir una entrada es la única forma de ampliar
/// la superficie remota del navegador integrado.
pub const STORES: &[StoreProfile] = &[
    StoreProfile {
        id: "steam",
        name: "Steam",
        home: "https://store.steampowered.com/",
        hosts: STEAM_HOSTS,
        search_template: "https://store.steampowered.com/search/?term={q}",
        user_agent: None,
        session_store: "3d871a61-2b84-4050-a08b-145368b80a3f",
    },
    StoreProfile {
        id: "gog",
        name: "GOG.com",
        home: "https://www.gog.com/",
        hosts: GOG_HOSTS,
        search_template: "https://www.gog.com/games?query={q}",
        user_agent: None,
        session_store: "5b880faf-fc81-4a29-b00b-4d785a8f00eb",
    },
    StoreProfile {
        id: "epic",
        name: "Epic Games Store",
        home: "https://store.epicgames.com/",
        hosts: EPIC_HOSTS,
        search_template: "https://store.epicgames.com/browse?q={q}",
        user_agent: None,
        session_store: "f2d7f1ed-e575-4038-ba53-70112b786fa1",
    },
    StoreProfile {
        id: "itch",
        name: "itch.io",
        home: "https://itch.io/",
        hosts: ITCH_HOSTS,
        search_template: "https://itch.io/search?q={q}",
        user_agent: None,
        session_store: "f75501ea-91b2-42d2-8316-3e444b999fff",
    },
];

/// Tienda por defecto: la que abre la ficha de un juego de la biblioteca.
pub const DEFAULT_STORE_ID: &str = "steam";

/// Host normalizado de una URL: minúsculas, sin punto final, sin puerto.
///
/// `url::Url` ya devuelve el host en minúsculas y en punycode, de modo que un
/// dominio homógrafo (`ѕtore.steampowered.com` con `ѕ` cirílica) llega aquí
/// como `xn--...` y nunca coincide con una regla ASCII.
pub fn normalized_host(url: &Url) -> Option<String> {
    let host = url.host_str()?.trim_end_matches('.');
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Perfil cuyo `id` coincide exactamente.
pub fn store_by_id(id: &str) -> Option<&'static StoreProfile> {
    STORES.iter().find(|store| store.id == id)
}

/// Perfil por defecto. Siempre existe.
pub fn default_store() -> &'static StoreProfile {
    store_by_id(DEFAULT_STORE_ID).expect("la tienda por defecto siempre está en el catálogo")
}

/// Primera tienda cuyo perfil admite esta URL, si existe alguna.
pub fn store_for_url(url: &Url) -> Option<&'static StoreProfile> {
    STORES.iter().find(|store| store.allows(url))
}

/// ¿Hay alguna tienda que admita esta URL?
#[allow(dead_code)]
pub fn is_allowed_store_url(url: &Url) -> bool {
    store_for_url(url).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    #[test]
    fn exact_host_rules_reject_suffix_impersonation() {
        let rule = HostRule::Exact("store.steampowered.com");
        assert!(rule.matches("store.steampowered.com"));
        assert!(!rule.matches("store.steampowered.com.attacker.tld"));
        assert!(!rule.matches("evilstore.steampowered.com"));
        assert!(!rule.matches("store.steampowered.com.evil"));
        assert!(!rule.matches("steampowered.com"));
        assert!(!rule.matches("astore.steampowered.com"));
    }

    #[test]
    fn subdomain_rules_demand_a_real_dot_separator() {
        let rule = HostRule::WithSubdomains("itch.io");
        assert!(rule.matches("itch.io"));
        assert!(rule.matches("creador.itch.io"));
        assert!(rule.matches("a.b.itch.io"));
        assert!(!rule.matches("evilitch.io"));
        assert!(!rule.matches("itch.io.attacker.tld"));
        assert!(!rule.matches(".itch.io"));
        assert!(!rule.matches("xitch.io"));
    }

    #[test]
    fn every_store_only_accepts_its_own_hosts() {
        let steam = store_by_id("steam").unwrap();
        let gog = store_by_id("gog").unwrap();
        assert!(steam.allows(&url("https://store.steampowered.com/app/620")));
        assert!(steam.allows(&url("https://checkout.steampowered.com/checkout/")));
        assert!(steam.allows(&url("https://login.steampowered.com/jwt/refresh")));
        assert!(!steam.allows(&url("https://www.gog.com/")));
        assert!(!gog.allows(&url("https://store.steampowered.com/")));
        assert!(!steam.allows(&url("https://store.steampowered.com.attacker.tld/app/620")));
    }

    #[test]
    fn homograph_and_trailing_dot_hosts_never_match() {
        // `ѕ` cirílica: `url` la convierte a punycode antes de comparar.
        let homograph = url("https://ѕtore.steampowered.com/app/620");
        assert!(!is_allowed_store_url(&homograph));
        assert_eq!(
            normalized_host(&url("https://STORE.steampowered.com./app/620")).as_deref(),
            Some("store.steampowered.com")
        );
        assert!(is_allowed_store_url(&url(
            "https://STORE.steampowered.com./app/620"
        )));
    }

    #[test]
    fn store_resolution_is_unambiguous_and_covers_the_catalogue() {
        assert_eq!(
            store_for_url(&url("https://itch.io/games")).unwrap().id,
            "itch"
        );
        assert_eq!(
            store_for_url(&url("https://sombra.itch.io/juego"))
                .unwrap()
                .id,
            "itch"
        );
        assert_eq!(
            store_for_url(&url("https://store.epicgames.com/es-ES/"))
                .unwrap()
                .id,
            "epic"
        );
        assert!(store_for_url(&url("https://steamdb.info/app/620/")).is_none());
        assert!(store_for_url(&url("https://example.com/")).is_none());
        // Ningún host puede pertenecer a dos tiendas a la vez.
        for store in STORES {
            let matches = STORES
                .iter()
                .filter(|other| other.allows(&store.home_url()))
                .count();
            assert_eq!(
                matches, 1,
                "la portada de {} pertenece a una sola tienda",
                store.id
            );
        }
    }

    #[test]
    fn home_and_search_urls_stay_inside_their_own_allowlist() {
        for store in STORES {
            let home = store.home_url();
            assert_eq!(home.scheme(), "https", "{} debe usar HTTPS", store.id);
            assert!(
                store.allows(&home),
                "{} no admite su propia portada",
                store.id
            );

            let search = store
                .search_url("half life 2")
                .unwrap_or_else(|| panic!("{} debe poder buscar", store.id));
            assert!(
                store.allows(&search),
                "{} sale de su allowlist al buscar",
                store.id
            );
            assert!(search.query().unwrap().contains("half+life+2"));
        }
    }

    #[test]
    fn search_terms_cannot_inject_a_different_host() {
        let steam = store_by_id("steam").unwrap();
        let hostile = steam
            .search_url("x&redirect=https://attacker.tld/#@attacker.tld")
            .unwrap();
        assert_eq!(hostile.host_str(), Some("store.steampowered.com"));
        assert!(steam.allows(&hostile));
        assert!(steam.search_url("   ").is_none());
    }

    #[test]
    fn every_store_owns_a_distinct_and_stable_session_store() {
        // El identificador es la sesión de la persona usuaria: si dos tiendas
        // compartieran almacén verían las cookies de la otra, y si alguno
        // cambiase se cerraría la sesión sin avisar.
        let mut vistos: Vec<[u8; 16]> = Vec::new();
        for store in STORES {
            let identificador = store
                .session_store_id()
                .unwrap_or_else(|| panic!("{} necesita un UUID válido", store.id));
            assert!(
                !vistos.contains(&identificador),
                "{} comparte almacén de sesión con otra tienda",
                store.id
            );
            vistos.push(identificador);
        }
        // Valores fijos: cambiarlos cierra la sesión de quien ya la tenía.
        assert_eq!(
            store_by_id("steam").unwrap().session_store,
            "3d871a61-2b84-4050-a08b-145368b80a3f"
        );
    }

    #[test]
    fn store_identifiers_and_labels_are_unique_and_stable() {
        let mut ids: Vec<_> = STORES.iter().map(|store| store.id).collect();
        ids.sort_unstable();
        let unique = ids.len();
        ids.dedup();
        assert_eq!(
            ids.len(),
            unique,
            "los identificadores de tienda son únicos"
        );
        assert_eq!(default_store().id, "steam");
        assert_eq!(default_store().window_label(), "vindexa-store-steam");
    }
}
