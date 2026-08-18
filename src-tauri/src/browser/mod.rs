//! Navegador integrado de Vindexa.
//!
//! Este módulo contiene toda la lógica pura del navegador de tiendas: qué
//! destinos existen, qué navegaciones se permiten, qué se bloquea, cómo se
//! recuerda el historial y qué barra se dibuja. La conexión con Tauri (crear la
//! ventana, instalar el filtro nativo, ejecutar las órdenes) vive en
//! `crate::store_window`, que se apoya en estas piezas.
//!
//! ## Fronteras
//!
//! | Frontera | Dónde se aplica |
//! | --- | --- |
//! | Destinos permitidos | [`stores`] y [`policy::evaluate`] |
//! | Esquemas peligrosos | [`policy::DANGEROUS_SCHEMES`] |
//! | Publicidad y rastreo | [`blocker`] |
//! | Sesión aislada por tienda | [`stores::StoreProfile::session_store`] |
//! | Interfaz de la barra | [`chrome`] — **no** es una frontera de seguridad |

pub mod blocker;
pub mod chrome;
pub mod policy;
pub mod prefs;
pub mod session;
pub mod stores;

#[cfg(test)]
mod tests {
    use super::{blocker, policy, stores};
    use tauri::Url;

    #[test]
    fn every_allowed_store_host_is_also_protected_from_the_blocklist() {
        // Si un host de navegación pudiera bloquearse, la tienda se rompería:
        // la lista de permitidos del bloqueador debe cubrir todos los destinos.
        for store in stores::STORES {
            for rule in store.hosts {
                assert!(
                    blocker::is_allowed_asset_host(rule.domain()),
                    "{} es un destino de {} y debe estar en la lista de permitidos",
                    rule.domain(),
                    store.id
                );
            }
            let home = store.home_url();
            assert!(
                !blocker::is_blocked_request(&home),
                "la portada de {} nunca puede bloquearse",
                store.id
            );
        }
    }

    #[test]
    fn no_blocked_domain_can_ever_serve_a_store_document() {
        for blocked in blocker::BLOCKED_DOMAINS {
            let candidate = Url::parse(&format!("https://{}/", blocked.domain))
                .expect("dominio bloqueado válido");
            assert!(
                stores::store_for_url(&candidate).is_none(),
                "{} está bloqueado y no puede ser además un destino de navegación",
                blocked.domain
            );
        }
    }

    #[test]
    fn the_control_scheme_is_not_a_navigable_destination() {
        let control = Url::parse(&format!("{}://token/back", policy::CONTROL_SCHEME))
            .expect("URL de control válida");
        assert!(stores::store_for_url(&control).is_none());
        assert!(!blocker::is_blocked_request(&control));
        assert!(
            !policy::DANGEROUS_SCHEMES.contains(&policy::CONTROL_SCHEME),
            "el esquema de control se trata aparte, no como esquema peligroso"
        );
    }
}
