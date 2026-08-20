//! Con qué modelo habla el agente de casa.
//!
//! # Por qué hace falta guardarlo
//!
//! Sin nada configurado, Vindexa coge el primer servidor local que encuentra y
//! su primer modelo. Eso está bien para empezar a hablar sin decidir nada, y es
//! justo lo que no sirve cuando hay tres modelos servidos a la vez y uno de
//! ellos es el bueno. Aquí se guarda la elección.
//!
//! # Local por defecto, y lo de fuera con nombre y apellidos
//!
//! Lo que se detecta solo siempre está en el bucle local: un modelo que vive en
//! otra máquina ya no es local. Se puede apuntar a un servicio de fuera —hay
//! quien tiene una clave y prefiere usarla— pero **sólo escribiéndolo a mano**,
//! nunca por detección, y sabiendo lo que implica: lo que se le mande incluye
//! los títulos de la biblioteca sobre los que se esté hablando.
//!
//! La clave no se guarda en SQLite. Va al llavero del sistema, que es donde
//! esta aplicación guarda el resto de sus credenciales.

use crate::db::Database;
use crate::error::{AppError, AppResult};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Clave en `app_settings`.
const SETTING_KEY: &str = "vindagent_model";

/// Servicio y cuenta del llavero para la clave de un servicio de fuera.
const KEYCHAIN_SERVICE: &str = "io.vindexa.desktop";
const KEYCHAIN_ACCOUNT: &str = "vindagent-api-key";

/// Elección guardada.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    /// Base del servidor. Vacío significa «el que se detecte».
    #[serde(default)]
    pub base_url: String,
    /// Modelo elegido dentro de ese servidor.
    #[serde(default)]
    pub model: String,
    /// `true` cuando la dirección no es del bucle local y se ha aceptado a
    /// sabiendas. Nunca se pone solo.
    #[serde(default)]
    pub remote_allowed: bool,
    /// Si hay una clave guardada en el llavero. El valor no viaja nunca.
    #[serde(default)]
    pub has_api_key: bool,
}

/// Lo que se recibe al guardar. La clave viaja una vez y va al llavero.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveModelConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub remote_allowed: bool,
    /// Clave nueva. `None` deja la que hubiera; cadena vacía la borra.
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Lee la elección guardada.
pub fn load(database: &Database) -> AppResult<ModelConfig> {
    let connection = database.open()?;
    let raw: Option<String> = connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [SETTING_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let mut config: ModelConfig = raw
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default();
    config.has_api_key = crate::keychain::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).is_ok();
    Ok(config)
}

/// Guarda la elección. Valida antes de escribir.
pub fn save(database: &Database, input: &SaveModelConfig) -> AppResult<ModelConfig> {
    let base = input.base_url.trim();
    if !base.is_empty() {
        let url = url::Url::parse(base)
            .map_err(|_| AppError::validation("La dirección del modelo no es una URL válida."))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(AppError::validation(
                "La dirección del modelo tiene que empezar por http o https.",
            ));
        }
        // Apuntar fuera del ordenador es una decisión, no un descuido: hay que
        // haberla marcado. Sin marcarla, sólo vale el bucle local.
        if !super::is_loopback(&url) && !input.remote_allowed {
            return Err(AppError::validation(
                "Esa dirección no está en este ordenador. Marca que aceptas usar un servicio de fuera si es lo que quieres.",
            ));
        }
    }

    match input.api_key.as_deref().map(str::trim) {
        Some("") => {
            let _ = crate::keychain::delete(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
        }
        Some(key) => {
            crate::keychain::set(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, key).map_err(|error| {
                AppError::new("vindagent", format!("No se pudo guardar la clave: {error}"))
            })?;
        }
        None => {}
    }

    let config = ModelConfig {
        base_url: base.to_owned(),
        model: input.model.trim().to_owned(),
        remote_allowed: input.remote_allowed,
        has_api_key: false,
    };
    let connection = database.open()?;
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![
            SETTING_KEY,
            serde_json::to_string(&config).unwrap_or_default()
        ],
    )?;
    load(database)
}

/// La clave guardada, si la hay. Sólo la lee quien va a hacer la petición.
pub fn api_key() -> Option<String> {
    crate::keychain::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar");
        (directory, database)
    }

    #[test]
    fn sin_nada_guardado_manda_la_deteccion() {
        let (_directory, database) = base();
        let config = load(&database).expect("leer");
        assert_eq!(config.base_url, "");
        assert_eq!(config.model, "");
    }

    #[test]
    fn una_direccion_de_fuera_sin_marcar_se_rechaza() {
        // Es la diferencia entre elegir mandar tus títulos a un servicio ajeno
        // y que ocurra por escribir mal una dirección.
        let (_directory, database) = base();
        let error = save(
            &database,
            &SaveModelConfig {
                base_url: "https://api.ejemplo.com".into(),
                model: "gpt".into(),
                remote_allowed: false,
                api_key: None,
            },
        )
        .expect_err("rechazar");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn marcada_a_sabiendas_si_se_guarda() {
        let (_directory, database) = base();
        let config = save(
            &database,
            &SaveModelConfig {
                base_url: "https://api.ejemplo.com".into(),
                model: "un-modelo".into(),
                remote_allowed: true,
                api_key: None,
            },
        )
        .expect("guardar");
        assert_eq!(config.base_url, "https://api.ejemplo.com");
        assert!(config.remote_allowed);
    }

    #[test]
    fn lo_local_no_necesita_permiso() {
        let (_directory, database) = base();
        let config = save(
            &database,
            &SaveModelConfig {
                base_url: "http://127.0.0.1:8770".into(),
                model: "qwen".into(),
                remote_allowed: false,
                api_key: None,
            },
        )
        .expect("guardar");
        assert_eq!(config.model, "qwen");
    }

    #[test]
    fn una_direccion_que_no_es_url_se_rechaza() {
        let (_directory, database) = base();
        assert_eq!(
            save(
                &database,
                &SaveModelConfig {
                    base_url: "no es una url".into(),
                    ..Default::default()
                },
            )
            .expect_err("rechazar")
            .code,
            "validation"
        );
    }
}
