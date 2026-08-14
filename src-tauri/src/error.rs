use serde::Serialize;
use std::fmt::{Display, Formatter};

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("validation", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("not_found", message)
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(_error: rusqlite::Error) -> Self {
        Self::new(
            "database",
            "La base de datos local no pudo completar la operación.",
        )
    }
}

impl From<std::io::Error> for AppError {
    fn from(_error: std::io::Error) -> Self {
        Self::new(
            "filesystem",
            "El sistema de archivos no pudo completar la operación.",
        )
    }
}

impl From<reqwest::Error> for AppError {
    fn from(error: reqwest::Error) -> Self {
        let message = if error.is_timeout() {
            "Steam no respondió a tiempo. Vuelve a intentarlo.".to_string()
        } else {
            "No se pudo completar la petición segura a Steam.".to_string()
        };
        Self::new("steam_network", message)
    }
}

impl From<tauri_plugin_opener::Error> for AppError {
    fn from(_error: tauri_plugin_opener::Error) -> Self {
        Self::new("opener", "No se pudo abrir el recurso solicitado.")
    }
}
