//! Preferencias persistentes del navegador integrado.
//!
//! Lo único que se guarda es el zoom por tienda. No se persiste historial, ni
//! cookies, ni sesión: la ventana es privada y debe seguir siéndolo entre
//! arranques. El archivo vive en el directorio de configuración de la
//! aplicación y nunca se expone al frontend ni a la ventana remota.

use crate::browser::session::{DEFAULT_ZOOM, clamp_zoom};
use crate::browser::stores;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Nombre del archivo de preferencias dentro del directorio de configuración.
const FILE_NAME: &str = "browser-zoom.json";

/// Zoom guardado por identificador de tienda.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ZoomPreferences {
    entries: BTreeMap<String, f64>,
}

impl ZoomPreferences {
    /// Zoom guardado para una tienda, o el valor por defecto.
    pub fn get(&self, store_id: &str) -> f64 {
        self.entries
            .get(store_id)
            .copied()
            .map(clamp_zoom)
            .unwrap_or(DEFAULT_ZOOM)
    }

    /// Guarda el zoom de una tienda. Solo se aceptan tiendas del catálogo y
    /// pasos válidos, así que un archivo manipulado no puede inyectar valores
    /// arbitrarios en la interfaz.
    pub fn set(&mut self, store_id: &str, zoom: f64) {
        if stores::store_by_id(store_id).is_none() {
            return;
        }
        self.entries.insert(store_id.to_string(), clamp_zoom(zoom));
    }

    /// Reconstruye las preferencias desde JSON, descartando lo que no encaje.
    pub fn from_json(raw: &str) -> Self {
        let mut prefs = Self::default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
            return prefs;
        };
        let Some(object) = value.as_object() else {
            return prefs;
        };
        for (store_id, zoom) in object {
            if let Some(zoom) = zoom.as_f64() {
                prefs.set(store_id, zoom);
            }
        }
        prefs
    }

    /// Serializa las preferencias a JSON estable.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.entries).unwrap_or_else(|_| "{}".to_string())
    }

    /// ¿Hay alguna preferencia guardada?
    ///
    /// Sin llamador en producción: el zoom por tienda se lee siempre con su
    /// valor por defecto, así que da igual si el mapa está vacío. Las pruebas la
    /// usan para comprobar que una preferencia borrada no deja rastro.
    #[allow(dead_code, reason = "sin llamador en producción; la usan las pruebas")]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Ruta del archivo de preferencias dentro de un directorio de configuración.
pub fn preferences_path(config_dir: &Path) -> PathBuf {
    config_dir.join(FILE_NAME)
}

/// Lee las preferencias. Cualquier fallo devuelve preferencias vacías: el zoom
/// nunca puede impedir que se abra la tienda.
pub fn load(config_dir: &Path) -> ZoomPreferences {
    std::fs::read_to_string(preferences_path(config_dir))
        .map(|raw| ZoomPreferences::from_json(&raw))
        .unwrap_or_default()
}

/// Escribe las preferencias. Los errores se ignoran a propósito y jamás se
/// propagan a la interfaz con una ruta local dentro.
pub fn save(config_dir: &Path, prefs: &ZoomPreferences) {
    if std::fs::create_dir_all(config_dir).is_err() {
        return;
    }
    let _ = std::fs::write(preferences_path(config_dir), prefs.to_json());
}

/// Actualiza el zoom de una tienda y lo persiste en un solo paso.
pub fn remember_zoom(config_dir: &Path, store_id: &str, zoom: f64) {
    let mut prefs = load(config_dir);
    prefs.set(store_id, zoom);
    save(config_dir, &prefs);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::session::ZOOM_STEPS;
    use tempfile::tempdir;

    #[test]
    fn unknown_stores_and_absurd_values_are_discarded() {
        let mut prefs = ZoomPreferences::default();
        prefs.set("steam", 1.25);
        prefs.set("tienda-inventada", 1.5);
        prefs.set("gog", 42.0);
        prefs.set("epic", f64::NAN);

        assert_eq!(prefs.get("steam"), 1.25);
        assert_eq!(prefs.get("tienda-inventada"), DEFAULT_ZOOM);
        assert_eq!(prefs.get("gog"), *ZOOM_STEPS.last().unwrap());
        assert_eq!(prefs.get("epic"), DEFAULT_ZOOM);
    }

    #[test]
    fn a_tampered_file_cannot_inject_values() {
        let prefs = ZoomPreferences::from_json(
            r#"{"steam": 9999, "../../etc/passwd": 1.5, "gog": "1.5", "itch": 1.5}"#,
        );
        assert_eq!(prefs.get("steam"), *ZOOM_STEPS.last().unwrap());
        assert_eq!(prefs.get("../../etc/passwd"), DEFAULT_ZOOM);
        assert_eq!(prefs.get("gog"), DEFAULT_ZOOM);
        assert_eq!(prefs.get("itch"), 1.5);

        assert!(ZoomPreferences::from_json("no es json").is_empty());
        assert!(ZoomPreferences::from_json("[1,2,3]").is_empty());
    }

    #[test]
    fn preferences_round_trip_through_disk() {
        let dir = tempdir().expect("directorio temporal");
        assert!(load(dir.path()).is_empty());

        remember_zoom(dir.path(), "steam", 1.5);
        remember_zoom(dir.path(), "gog", 0.9);
        let reloaded = load(dir.path());
        assert_eq!(reloaded.get("steam"), 1.5);
        assert_eq!(reloaded.get("gog"), 0.9);
        assert_eq!(reloaded.get("epic"), DEFAULT_ZOOM);

        let stored = std::fs::read_to_string(preferences_path(dir.path())).unwrap();
        assert!(stored.contains("steam"));
        // Nada de historial ni de URLs visitadas en disco.
        assert!(!stored.contains("http"));
    }

    #[test]
    fn an_unwritable_directory_never_breaks_the_browser() {
        let missing = Path::new("/vindexa-ruta-que-no-existe/imposible");
        assert!(load(missing).is_empty());
        // No debe entrar en pánico ni propagar el error.
        remember_zoom(missing, "steam", 1.25);
    }
}
