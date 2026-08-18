//! Estado de navegación de cada ventana de tienda.
//!
//! El historial vive **solo en memoria** y desaparece al cerrar la ventana: la
//! ventana es privada y no debe dejar rastro de qué se ha mirado. Lo único que
//! se persiste es el zoom por tienda, que no revela contenido navegado.

use crate::browser::stores::{self, StoreProfile};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::Url;

/// Pasos de zoom disponibles, del más pequeño al más grande.
pub const ZOOM_STEPS: &[f64] = &[0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0];

/// Zoom por defecto de una tienda sin preferencia guardada.
pub const DEFAULT_ZOOM: f64 = 1.0;

/// Número máximo de entradas de historial por ventana.
const MAX_HISTORY: usize = 200;

/// Historial lineal con cursor, equivalente al de una pestaña de navegador.
#[derive(Debug, Clone)]
pub struct History {
    entries: Vec<Url>,
    cursor: usize,
}

impl History {
    /// Historial que arranca en `initial`.
    pub fn new(initial: Url) -> Self {
        Self {
            entries: vec![initial],
            cursor: 0,
        }
    }

    /// Entrada actual.
    pub fn current(&self) -> &Url {
        &self.entries[self.cursor]
    }

    /// Añade una entrada nueva, descartando las que hubiera por delante.
    pub fn push(&mut self, url: Url) {
        if *self.current() == url {
            return;
        }
        self.entries.truncate(self.cursor + 1);
        self.entries.push(url);
        if self.entries.len() > MAX_HISTORY {
            let excess = self.entries.len() - MAX_HISTORY;
            self.entries.drain(0..excess);
        }
        self.cursor = self.entries.len() - 1;
    }

    /// ¿Se puede retroceder?
    pub fn can_go_back(&self) -> bool {
        self.cursor > 0
    }

    /// ¿Se puede avanzar?
    pub fn can_go_forward(&self) -> bool {
        self.cursor + 1 < self.entries.len()
    }

    /// Retrocede una entrada y devuelve el destino.
    pub fn back(&mut self) -> Option<Url> {
        if !self.can_go_back() {
            return None;
        }
        self.cursor -= 1;
        Some(self.current().clone())
    }

    /// Avanza una entrada y devuelve el destino.
    pub fn forward(&mut self) -> Option<Url> {
        if !self.can_go_forward() {
            return None;
        }
        self.cursor += 1;
        Some(self.current().clone())
    }

    /// Número de entradas conservadas.
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Siempre hay al menos la entrada inicial.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        false
    }
}

/// Zoom inmediatamente superior al actual.
pub fn zoom_in(current: f64) -> f64 {
    ZOOM_STEPS
        .iter()
        .copied()
        .find(|step| *step > current + f64::EPSILON)
        .unwrap_or_else(|| *ZOOM_STEPS.last().expect("hay pasos de zoom"))
}

/// Zoom inmediatamente inferior al actual.
pub fn zoom_out(current: f64) -> f64 {
    ZOOM_STEPS
        .iter()
        .copied()
        .rev()
        .find(|step| *step < current - f64::EPSILON)
        .unwrap_or_else(|| *ZOOM_STEPS.first().expect("hay pasos de zoom"))
}

/// Ajusta un zoom arbitrario al paso válido más cercano.
pub fn clamp_zoom(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_ZOOM;
    }
    ZOOM_STEPS
        .iter()
        .copied()
        .min_by(|a, b| {
            (a - value)
                .abs()
                .partial_cmp(&(b - value).abs())
                .expect("los pasos de zoom son finitos")
        })
        .unwrap_or(DEFAULT_ZOOM)
}

/// Estado vivo de una ventana de tienda.
#[derive(Debug)]
pub struct WindowState {
    /// Perfil de tienda al que pertenece la ventana.
    pub store: &'static StoreProfile,
    /// Token del canal de control de esta ventana.
    pub token: String,
    /// Historial en memoria.
    pub history: History,
    /// Zoom aplicado.
    pub zoom: f64,
    /// ¿Se instaló el bloqueador nativo?
    pub blocker_active: bool,
    /// ¿Hay una carga en curso?
    pub loading: bool,
    /// Destino de un salto de historial en curso.
    ///
    /// Cuando la navegación que llega coincide con este destino no se crea una
    /// entrada nueva: la ha originado el propio historial. Guardar la URL en
    /// vez de un booleano evita que la marca se quede pegada si el salto no
    /// llega a producirse.
    pub pending_traversal: Option<Url>,
    /// Generación de carga, para que un vigilante de tiempo de espera sepa si
    /// sigue observando la misma navegación.
    pub load_generation: u64,
    /// Aviso en español para la barra (error de red, destino bloqueado…).
    pub notice: Option<String>,
}

impl WindowState {
    /// Estado inicial de una ventana recién creada.
    pub fn new(store: &'static StoreProfile, token: String, start: Url, zoom: f64) -> Self {
        Self {
            store,
            token,
            history: History::new(start),
            zoom: clamp_zoom(zoom),
            blocker_active: false,
            loading: true,
            pending_traversal: None,
            load_generation: 0,
            notice: None,
        }
    }

    /// Registra una navegación aceptada y devuelve la generación de carga.
    ///
    /// Un salto de historial no crea entrada nueva; cualquier otra navegación
    /// sí. Devolver la generación permite vigilar el tiempo de espera de esta
    /// carga concreta sin confundirla con la siguiente.
    pub fn note_navigation(&mut self, url: &Url) -> u64 {
        match self.pending_traversal.take() {
            Some(expected) if expected == *url => {}
            // Si no había salto, o esperábamos otro destino, el salto se
            // perdió: se descarta y esta navegación crea entrada propia.
            _ => self.history.push(url.clone()),
        }
        self.loading = true;
        self.notice = None;
        self.load_generation = self.load_generation.wrapping_add(1);
        self.load_generation
    }

    /// Prepara un salto de historial hacia atrás.
    pub fn go_back(&mut self) -> Option<Url> {
        let target = self.history.back()?;
        self.pending_traversal = Some(target.clone());
        Some(target)
    }

    /// Prepara un salto de historial hacia delante.
    pub fn go_forward(&mut self) -> Option<Url> {
        let target = self.history.forward()?;
        self.pending_traversal = Some(target.clone());
        Some(target)
    }

    /// Fotografía serializable para la barra del navegador.
    pub fn snapshot(&self) -> ChromeState {
        let current = self.history.current();
        ChromeState {
            store_id: self.store.id.to_string(),
            store_name: self.store.name.to_string(),
            url: current.as_str().to_string(),
            host: stores::normalized_host(current).unwrap_or_default(),
            path: display_path(current),
            secure: current.scheme() == "https",
            can_go_back: self.history.can_go_back(),
            can_go_forward: self.history.can_go_forward(),
            loading: self.loading,
            zoom: self.zoom,
            zoom_percent: (self.zoom * 100.0).round() as u32,
            blocker_active: self.blocker_active,
            notice: self.notice.clone(),
            stores: stores::STORES
                .iter()
                .map(|store| ChromeStore {
                    id: store.id.to_string(),
                    name: store.name.to_string(),
                    active: store.id == self.store.id,
                })
                .collect(),
        }
    }
}

/// Entrada del selector de tiendas de la barra.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeStore {
    /// Identificador interno de la tienda.
    pub id: String,
    /// Nombre visible.
    pub name: String,
    /// ¿Es la tienda de esta ventana?
    pub active: bool,
}

/// Estado que consume la barra del navegador.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeState {
    /// Identificador de la tienda activa.
    pub store_id: String,
    /// Nombre visible de la tienda activa.
    pub store_name: String,
    /// URL completa actual.
    pub url: String,
    /// Host normalizado, para el indicador de origen.
    pub host: String,
    /// Ruta y consulta, para el resto de la barra de direcciones.
    pub path: String,
    /// ¿El origen actual es HTTPS?
    pub secure: bool,
    /// ¿Hay historial hacia atrás?
    pub can_go_back: bool,
    /// ¿Hay historial hacia delante?
    pub can_go_forward: bool,
    /// ¿Se está cargando algo?
    pub loading: bool,
    /// Zoom aplicado, como factor.
    pub zoom: f64,
    /// Zoom aplicado, en porcentaje redondeado.
    pub zoom_percent: u32,
    /// ¿Está activo el bloqueador nativo?
    pub blocker_active: bool,
    /// Aviso puntual en español.
    pub notice: Option<String>,
    /// Tiendas disponibles para el selector.
    pub stores: Vec<ChromeStore>,
}

/// Ruta visible de una URL: camino y consulta, sin el origen.
fn display_path(url: &Url) -> String {
    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    path
}

type Registry = Mutex<HashMap<String, WindowState>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registra el estado de una ventana recién abierta.
pub fn register(label: &str, state: WindowState) {
    if let Ok(mut windows) = registry().lock() {
        windows.insert(label.to_string(), state);
    }
}

/// Olvida el estado de una ventana cerrada. El historial se pierde a propósito.
pub fn forget(label: &str) {
    if let Ok(mut windows) = registry().lock() {
        windows.remove(label);
    }
}

/// ¿Hay estado registrado para esta etiqueta?
pub fn is_registered(label: &str) -> bool {
    registry()
        .lock()
        .map(|windows| windows.contains_key(label))
        .unwrap_or(false)
}

/// Ejecuta `action` sobre el estado de una ventana, si existe.
pub fn with_window<T>(label: &str, action: impl FnOnce(&mut WindowState) -> T) -> Option<T> {
    let mut windows = registry().lock().ok()?;
    windows.get_mut(label).map(action)
}

/// Fotografía del estado de una ventana.
pub fn snapshot(label: &str) -> Option<ChromeState> {
    with_window(label, |state| state.snapshot())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    fn steam() -> &'static StoreProfile {
        stores::store_by_id("steam").unwrap()
    }

    #[test]
    fn history_behaves_like_a_browser_tab() {
        let mut history = History::new(url("https://store.steampowered.com/"));
        assert!(!history.can_go_back());
        assert!(!history.can_go_forward());
        assert_eq!(history.back(), None);

        history.push(url("https://store.steampowered.com/app/620"));
        history.push(url("https://store.steampowered.com/app/440"));
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        assert_eq!(
            history.back().unwrap().as_str(),
            "https://store.steampowered.com/app/620"
        );
        assert!(history.can_go_forward());
        assert_eq!(
            history.forward().unwrap().as_str(),
            "https://store.steampowered.com/app/440"
        );
        assert_eq!(history.forward(), None);
    }

    #[test]
    fn navigating_after_going_back_drops_the_forward_branch() {
        let mut history = History::new(url("https://store.steampowered.com/"));
        history.push(url("https://store.steampowered.com/app/620"));
        history.push(url("https://store.steampowered.com/app/440"));
        history.back();
        history.push(url("https://steamcommunity.com/market/"));
        assert!(!history.can_go_forward());
        assert_eq!(history.current().as_str(), "https://steamcommunity.com/market/");
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn repeated_navigations_to_the_same_url_do_not_grow_the_history() {
        let mut history = History::new(url("https://store.steampowered.com/"));
        history.push(url("https://store.steampowered.com/"));
        history.push(url("https://store.steampowered.com/"));
        assert_eq!(history.len(), 1);
        assert!(!history.can_go_back());
    }

    #[test]
    fn history_is_bounded_and_keeps_the_newest_entries() {
        let mut history = History::new(url("https://store.steampowered.com/app/0"));
        for index in 1..(MAX_HISTORY + 25) {
            history.push(url(&format!("https://store.steampowered.com/app/{index}")));
        }
        assert_eq!(history.len(), MAX_HISTORY);
        assert_eq!(
            history.current().as_str(),
            format!("https://store.steampowered.com/app/{}", MAX_HISTORY + 24)
        );
        assert!(history.can_go_back());
    }

    #[test]
    fn zoom_moves_step_by_step_and_saturates() {
        assert_eq!(zoom_in(1.0), 1.1);
        assert_eq!(zoom_out(1.0), 0.9);
        assert_eq!(zoom_in(2.0), 2.0);
        assert_eq!(zoom_out(0.5), 0.5);
        assert_eq!(clamp_zoom(1.03), 1.0);
        assert_eq!(clamp_zoom(9.0), 2.0);
        assert_eq!(clamp_zoom(f64::NAN), DEFAULT_ZOOM);
        assert!(ZOOM_STEPS.contains(&DEFAULT_ZOOM));
    }

    #[test]
    fn the_snapshot_describes_the_bar_without_leaking_anything_local() {
        let mut state = WindowState::new(
            steam(),
            "abc123".into(),
            url("https://store.steampowered.com/app/620/Portal_2/?l=spanish"),
            1.25,
        );
        state.blocker_active = true;
        state.loading = false;
        let snapshot = state.snapshot();

        assert_eq!(snapshot.store_id, "steam");
        assert_eq!(snapshot.host, "store.steampowered.com");
        assert_eq!(snapshot.path, "/app/620/Portal_2/?l=spanish");
        assert!(snapshot.secure);
        assert!(!snapshot.can_go_back);
        assert_eq!(snapshot.zoom_percent, 125);
        assert!(snapshot.blocker_active);
        assert_eq!(snapshot.stores.len(), stores::STORES.len());
        assert_eq!(snapshot.stores.iter().filter(|s| s.active).count(), 1);

        let json = serde_json::to_string(&snapshot).unwrap();
        assert!(json.contains("storeId"));
        assert!(!json.contains("Users/"));
        assert!(!json.contains("token"));
    }

    #[test]
    fn history_jumps_do_not_create_duplicate_entries() {
        let mut state = WindowState::new(
            steam(),
            "tok".into(),
            url("https://store.steampowered.com/"),
            DEFAULT_ZOOM,
        );
        state.note_navigation(&url("https://store.steampowered.com/app/620"));
        state.note_navigation(&url("https://store.steampowered.com/app/440"));
        assert_eq!(state.history.len(), 3);

        let target = state.go_back().unwrap();
        assert_eq!(target.as_str(), "https://store.steampowered.com/app/620");
        state.note_navigation(&target);
        assert_eq!(state.history.len(), 3, "el salto no crea entrada nueva");
        assert!(state.history.can_go_forward());

        // Un salto que nunca llega no deja la marca pegada: la siguiente
        // navegación normal sí crea entrada.
        state.go_back();
        state.note_navigation(&url("https://steamcommunity.com/market/"));
        assert!(!state.history.can_go_forward());
        assert_eq!(
            state.history.current().as_str(),
            "https://steamcommunity.com/market/"
        );
        assert!(state.pending_traversal.is_none());
    }

    #[test]
    fn each_navigation_gets_its_own_load_generation() {
        let mut state = WindowState::new(
            steam(),
            "tok".into(),
            url("https://store.steampowered.com/"),
            DEFAULT_ZOOM,
        );
        state.notice = Some("aviso anterior".into());
        let first = state.note_navigation(&url("https://store.steampowered.com/app/620"));
        let second = state.note_navigation(&url("https://store.steampowered.com/app/440"));
        assert_ne!(first, second);
        assert!(state.loading);
        assert!(state.notice.is_none(), "cada carga limpia el aviso previo");
    }

    #[test]
    fn the_registry_isolates_windows_and_forgets_them_on_close() {
        let label = "vindexa-store-prueba-aislamiento";
        assert!(!is_registered(label));
        register(
            label,
            WindowState::new(
                steam(),
                "tok".into(),
                url("https://store.steampowered.com/"),
                DEFAULT_ZOOM,
            ),
        );
        assert!(is_registered(label));

        with_window(label, |state| {
            state.history.push(url("https://store.steampowered.com/app/620"));
            state.zoom = 1.5;
        });
        let taken = snapshot(label).unwrap();
        assert!(taken.can_go_back);
        assert_eq!(taken.zoom_percent, 150);

        // Otra ventana no ve nada de la primera.
        assert!(snapshot("vindexa-store-otra-ventana").is_none());

        forget(label);
        assert!(!is_registered(label));
        assert!(snapshot(label).is_none());
    }
}
