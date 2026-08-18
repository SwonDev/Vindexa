//! Ámbitos de autorización de un cliente agente.
//!
//! Los ámbitos son un conjunto cerrado. No existe comodín: conceder «todo» a un
//! agente externo obligaría a revisar cada intención nueva, y esa revisión es
//! justo lo que este catálogo evita. Un cliente sin el ámbito exacto de la
//! intención se rechaza **antes** de resolver nombres, tocar la base o escribir
//! nada distinto de la fila de auditoría.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

/// Ámbito requerido por una intención.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentScope {
    /// Consultas de solo lectura sobre la biblioteca y su organización.
    #[serde(rename = "biblioteca:leer")]
    LibraryRead,
    /// Cambios sobre la ficha personal de un juego ya existente.
    #[serde(rename = "biblioteca:escribir")]
    LibraryWrite,
    /// Alta y edición de sesiones de juego.
    #[serde(rename = "sesiones:escribir")]
    SessionsWrite,
    /// Creación de colecciones y cambios de pertenencia.
    #[serde(rename = "colecciones:escribir")]
    CollectionsWrite,
    /// Creación de listas curadas y cambios de pertenencia.
    #[serde(rename = "listas:escribir")]
    ListsWrite,
    /// Colocación de juegos en el planificador.
    #[serde(rename = "planificador:escribir")]
    PlannerWrite,
    /// Programación de avisos.
    #[serde(rename = "avisos:escribir")]
    RemindersWrite,
}

/// Todos los ámbitos, en el orden en que se documentan.
pub const ALL_SCOPES: [AgentScope; 7] = [
    AgentScope::LibraryRead,
    AgentScope::LibraryWrite,
    AgentScope::SessionsWrite,
    AgentScope::CollectionsWrite,
    AgentScope::ListsWrite,
    AgentScope::PlannerWrite,
    AgentScope::RemindersWrite,
];

impl AgentScope {
    /// Cadena que viaja en `agent_clients.scopes_json`.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LibraryRead => "biblioteca:leer",
            Self::LibraryWrite => "biblioteca:escribir",
            Self::SessionsWrite => "sesiones:escribir",
            Self::CollectionsWrite => "colecciones:escribir",
            Self::ListsWrite => "listas:escribir",
            Self::PlannerWrite => "planificador:escribir",
            Self::RemindersWrite => "avisos:escribir",
        }
    }

    /// Reconoce un ámbito escrito por una persona o por el frontend.
    pub fn parse(value: &str) -> AppResult<Self> {
        ALL_SCOPES
            .into_iter()
            .find(|scope| scope.as_str() == value.trim())
            .ok_or_else(|| {
                AppError::validation(format!(
                    "El ámbito «{}» no existe. Ámbitos válidos: {}.",
                    value.trim(),
                    ALL_SCOPES
                        .iter()
                        .map(|scope| scope.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })
    }

    /// Descripción en castellano para la interfaz y la documentación.
    pub const fn description(self) -> &'static str {
        match self {
            Self::LibraryRead => "Leer la biblioteca, la organización y el registro del agente.",
            Self::LibraryWrite => {
                "Cambiar estado, progreso, prioridad, nota, valoración y marcas de un juego."
            }
            Self::SessionsWrite => "Registrar sesiones de juego.",
            Self::CollectionsWrite => "Crear colecciones manuales y cambiar su contenido.",
            Self::ListsWrite => "Crear listas curadas y añadir juegos.",
            Self::PlannerWrite => "Colocar juegos en el planificador.",
            Self::RemindersWrite => "Programar avisos.",
        }
    }
}

/// Conjunto de ámbitos concedidos a un cliente.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeSet {
    granted: Vec<AgentScope>,
}

impl ScopeSet {
    /// Normaliza una lista de ámbitos: valida, elimina duplicados y ordena.
    pub fn from_values<T: AsRef<str>>(values: &[T]) -> AppResult<Self> {
        let mut granted = Vec::with_capacity(values.len());
        for value in values {
            let scope = AgentScope::parse(value.as_ref())?;
            if !granted.contains(&scope) {
                granted.push(scope);
            }
        }
        granted.sort_by_key(|scope| {
            ALL_SCOPES
                .iter()
                .position(|candidate| candidate == scope)
                .unwrap_or_default()
        });
        Ok(Self { granted })
    }

    /// Lee el JSON persistido en `agent_clients.scopes_json`.
    ///
    /// Un JSON ilegible o con un ámbito desconocido deja el conjunto vacío: se
    /// prefiere denegar todo a conceder algo por descuido.
    pub fn from_json(raw: &str) -> Self {
        let Ok(values) = serde_json::from_str::<Vec<String>>(raw) else {
            return Self::default();
        };
        Self::from_values(&values).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        let values = self
            .granted
            .iter()
            .map(|scope| scope.as_str())
            .collect::<Vec<_>>();
        serde_json::to_string(&values).unwrap_or_else(|_| "[]".to_string())
    }

    pub fn contains(&self, scope: AgentScope) -> bool {
        self.granted.contains(&scope)
    }

    pub fn values(&self) -> Vec<String> {
        self.granted
            .iter()
            .map(|scope| scope.as_str().to_string())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.granted.is_empty()
    }

    /// Comprueba el ámbito exigido por una intención.
    pub fn require(&self, scope: AgentScope) -> AppResult<()> {
        if self.contains(scope) {
            return Ok(());
        }
        Err(AppError::new(
            "agent_scope",
            format!(
                "El cliente no tiene el ámbito «{}» necesario para esta acción.",
                scope.as_str()
            ),
        ))
    }
}
