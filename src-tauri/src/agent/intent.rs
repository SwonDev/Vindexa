//! Catálogo cerrado de intenciones y sus esquemas de argumentos.
//!
//! El catálogo es cerrado a propósito: un agente externo solo puede pedir lo
//! que está aquí escrito. No hay intención de borrado de colecciones, listas,
//! juegos ni estados, y tampoco de ejecución, instalación o desinstalación:
//! esas acciones siguen siendo exclusivamente humanas desde la interfaz.
//!
//! Cada variante declara su ámbito ([`AgentScope`]) y su nivel de riesgo
//! ([`ConfirmationPolicy`]). La validación de esta capa es puramente sintáctica
//! —longitudes, rangos, formatos de fecha, exclusividad de campos—; la
//! existencia real de juegos, estados y colecciones la comprueba el ejecutor
//! dentro de la transacción.

use crate::agent::scope::AgentScope;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};

/// Duración máxima admitida para una sesión: 24 horas.
pub const MAX_SESSION_MINUTES: u32 = 1_440;
/// Longitud máxima de una nota de sesión, alineada con el disparador SQL.
pub const MAX_SESSION_NOTE: usize = 2_000;
/// Longitud máxima de la nota personal de un juego.
pub const MAX_PERSONAL_NOTE: usize = 4_000;
/// Longitud máxima de la próxima acción y del punto de guardado.
pub const MAX_SHORT_TEXT: usize = 280;
/// Longitud máxima del nombre de una colección o de una lista curada.
pub const MAX_NAME: usize = 80;
/// Longitud máxima de una descripción.
pub const MAX_DESCRIPTION: usize = 1_000;
/// Juegos máximos que admite una sola intención.
pub const MAX_GAMES_PER_INTENT: usize = 200;
/// A partir de este número de juegos afectados hace falta confirmación humana.
pub const CONFIRMATION_THRESHOLD: usize = 5;

/// Cómo se elige el juego sobre el que actúa una intención.
///
/// Hay que indicar exactamente uno de los dos campos. `appId` es la vía
/// inequívoca; `name` activa la resolución tolerante de
/// [`crate::agent::matching`], que puede acabar pidiendo confirmación.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameSelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl GameSelector {
    pub fn validate(&self) -> AppResult<()> {
        match (self.app_id, self.name.as_deref().map(str::trim)) {
            (Some(app_id), None) if app_id > 0 => Ok(()),
            (Some(_), None) => Err(AppError::validation(
                "El AppID del juego debe ser un entero positivo.",
            )),
            (None, Some(name)) if !name.is_empty() && name.chars().count() <= 200 => Ok(()),
            (None, Some(_)) => Err(AppError::validation(
                "El nombre del juego debe tener entre 1 y 200 caracteres.",
            )),
            (Some(_), Some(_)) => Err(AppError::validation(
                "Indica el juego por «appId» o por «name», no por los dos a la vez.",
            )),
            (None, None) => Err(AppError::validation(
                "Falta el juego: indica «appId» o «name».",
            )),
        }
    }
}

/// Cómo se elige una colección o una lista curada: por identificador o por
/// nombre exacto (sin distinguir mayúsculas ni acentos al comparar).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EntitySelector {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl EntitySelector {
    pub fn validate(&self, label: &str) -> AppResult<()> {
        match (
            self.id.as_deref().map(str::trim),
            self.name.as_deref().map(str::trim),
        ) {
            (Some(id), None) if !id.is_empty() && id.chars().count() <= 128 => Ok(()),
            (None, Some(name)) if !name.is_empty() && name.chars().count() <= MAX_NAME => Ok(()),
            (Some(_), Some(_)) => Err(AppError::validation(format!(
                "Indica {label} por «id» o por «name», no por los dos a la vez."
            ))),
            _ => Err(AppError::validation(format!(
                "Falta {label}: indica «id» o «name»."
            ))),
        }
    }
}

/// Consultas de solo lectura.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentQuery {
    /// Busca juegos por texto y devuelve su ficha resumida.
    #[serde(rename = "biblioteca")]
    Library {
        #[serde(default)]
        text: String,
        #[serde(default)]
        status_id: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Ficha completa de un juego concreto.
    #[serde(rename = "juego")]
    Game { game: GameSelector },
    /// Estados disponibles con su recuento.
    #[serde(rename = "estados")]
    Statuses,
    /// Colecciones con su número de juegos.
    #[serde(rename = "colecciones")]
    Collections,
    /// Listas curadas con su número de juegos.
    #[serde(rename = "listas")]
    CuratedLists,
    /// Columnas del planificador con lo que contienen.
    #[serde(rename = "planificador")]
    Planner,
    /// Avisos pendientes.
    #[serde(rename = "avisos")]
    Reminders,
    /// Últimas sesiones registradas de un juego.
    #[serde(rename = "sesiones")]
    Sessions {
        game: GameSelector,
        #[serde(default)]
        limit: Option<u32>,
    },
    /// Últimas entradas del registro de auditoría del propio agente.
    #[serde(rename = "auditoria")]
    Audit {
        #[serde(default)]
        limit: Option<u32>,
    },
}

/// Catálogo cerrado de intenciones.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "intent",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentIntent {
    /// «Acabo de estar 2 horas jugando a X y voy por el 40 %.»
    #[serde(rename = "registrar_sesion")]
    RegisterSession {
        game: GameSelector,
        minutes: u32,
        #[serde(default)]
        started_at: Option<String>,
        #[serde(default)]
        progress: Option<u8>,
        #[serde(default)]
        note: String,
    },
    /// «Ya me lo he pasado pero seguiré jugando: bájale la prioridad.»
    #[serde(rename = "marcar_terminado")]
    MarkFinished {
        game: GameSelector,
        #[serde(default)]
        completed_on: Option<String>,
        /// `true` conserva el estado actual para que el juego siga siendo
        /// jugable; `false` lo mueve al estado «Completado».
        #[serde(default)]
        keep_playable: bool,
        #[serde(default)]
        priority: Option<u8>,
    },
    #[serde(rename = "cambiar_estado")]
    ChangeStatus {
        game: GameSelector,
        status_id: String,
    },
    /// `priority` fija un valor absoluto; `delta` lo mueve. Exactamente uno.
    #[serde(rename = "ajustar_prioridad")]
    AdjustPriority {
        game: GameSelector,
        #[serde(default)]
        priority: Option<u8>,
        #[serde(default)]
        delta: Option<i8>,
    },
    #[serde(rename = "fijar")]
    Pin { game: GameSelector, pinned: bool },
    #[serde(rename = "seguir")]
    Track { game: GameSelector, tracking: bool },
    /// `rating` nulo borra la valoración.
    #[serde(rename = "valorar")]
    Rate {
        game: GameSelector,
        #[serde(default)]
        rating: Option<u8>,
    },
    #[serde(rename = "anotar")]
    Annotate {
        game: GameSelector,
        note: String,
        /// `true` añade la nota al final de la existente en lugar de sustituirla.
        #[serde(default)]
        append: bool,
    },
    #[serde(rename = "fijar_proxima_accion")]
    SetNextAction {
        game: GameSelector,
        #[serde(default)]
        action: Option<String>,
    },
    #[serde(rename = "fijar_checkpoint")]
    SetCheckpoint {
        game: GameSelector,
        #[serde(default)]
        checkpoint: Option<String>,
    },
    /// Solo colecciones manuales: un agente no crea reglas inteligentes.
    #[serde(rename = "crear_coleccion")]
    CreateCollection {
        name: String,
        #[serde(default)]
        description: String,
        #[serde(default = "default_color")]
        color: String,
        #[serde(default = "default_icon")]
        icon: String,
        #[serde(default)]
        games: Vec<GameSelector>,
    },
    #[serde(rename = "añadir_a_coleccion")]
    AddToCollection {
        collection: EntitySelector,
        games: Vec<GameSelector>,
    },
    #[serde(rename = "quitar_de_coleccion")]
    RemoveFromCollection {
        collection: EntitySelector,
        games: Vec<GameSelector>,
    },
    #[serde(rename = "crear_lista_curada")]
    CreateCuratedList {
        name: String,
        #[serde(default)]
        description: String,
        #[serde(default = "default_curated_kind")]
        kind: String,
        #[serde(default = "default_accent")]
        accent: String,
        #[serde(default = "default_list_icon")]
        icon: String,
        #[serde(default)]
        pinned: bool,
    },
    #[serde(rename = "añadir_a_lista")]
    AddToCuratedList {
        list: EntitySelector,
        games: Vec<GameSelector>,
        #[serde(default)]
        note: String,
        #[serde(default)]
        highlight: bool,
    },
    #[serde(rename = "planificar")]
    Plan {
        game: GameSelector,
        column_id: String,
        #[serde(default)]
        target_date: Option<String>,
        #[serde(default)]
        estimated_minutes: Option<u32>,
    },
    #[serde(rename = "programar_aviso")]
    ScheduleReminder {
        game: GameSelector,
        due_at: String,
        #[serde(default)]
        note: String,
    },
    #[serde(rename = "consultar")]
    Query { query: AgentQuery },
}

fn default_color() -> String {
    "#5CAAC1".to_string()
}

fn default_icon() -> String {
    "folder".to_string()
}

fn default_list_icon() -> String {
    "list".to_string()
}

fn default_curated_kind() -> String {
    "manual".to_string()
}

fn default_accent() -> String {
    "cyan".to_string()
}

/// Nivel de confirmación humana exigido antes de aplicar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationPolicy {
    /// Se aplica directamente salvo que afecte a demasiados juegos.
    Automatic,
    /// Siempre pasa por `pending`, sin importar cuántos juegos toque.
    Always,
    /// Solo lectura: nunca modifica nada.
    ReadOnly,
}

impl AgentIntent {
    /// Nombre público de la intención, tal y como viaja por IPC.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::RegisterSession { .. } => "registrar_sesion",
            Self::MarkFinished { .. } => "marcar_terminado",
            Self::ChangeStatus { .. } => "cambiar_estado",
            Self::AdjustPriority { .. } => "ajustar_prioridad",
            Self::Pin { .. } => "fijar",
            Self::Track { .. } => "seguir",
            Self::Rate { .. } => "valorar",
            Self::Annotate { .. } => "anotar",
            Self::SetNextAction { .. } => "fijar_proxima_accion",
            Self::SetCheckpoint { .. } => "fijar_checkpoint",
            Self::CreateCollection { .. } => "crear_coleccion",
            Self::AddToCollection { .. } => "añadir_a_coleccion",
            Self::RemoveFromCollection { .. } => "quitar_de_coleccion",
            Self::CreateCuratedList { .. } => "crear_lista_curada",
            Self::AddToCuratedList { .. } => "añadir_a_lista",
            Self::Plan { .. } => "planificar",
            Self::ScheduleReminder { .. } => "programar_aviso",
            Self::Query { .. } => "consultar",
        }
    }

    /// Ámbito exigido antes de ejecutar nada.
    pub const fn required_scope(&self) -> AgentScope {
        match self {
            Self::RegisterSession { .. } => AgentScope::SessionsWrite,
            Self::MarkFinished { .. }
            | Self::ChangeStatus { .. }
            | Self::AdjustPriority { .. }
            | Self::Pin { .. }
            | Self::Track { .. }
            | Self::Rate { .. }
            | Self::Annotate { .. }
            | Self::SetNextAction { .. }
            | Self::SetCheckpoint { .. } => AgentScope::LibraryWrite,
            Self::CreateCollection { .. }
            | Self::AddToCollection { .. }
            | Self::RemoveFromCollection { .. } => AgentScope::CollectionsWrite,
            Self::CreateCuratedList { .. } | Self::AddToCuratedList { .. } => {
                AgentScope::ListsWrite
            }
            Self::Plan { .. } => AgentScope::PlannerWrite,
            Self::ScheduleReminder { .. } => AgentScope::RemindersWrite,
            Self::Query { .. } => AgentScope::LibraryRead,
        }
    }

    /// Política de confirmación de la intención.
    pub const fn confirmation_policy(&self) -> ConfirmationPolicy {
        match self {
            // Quitar juegos de una colección es la única acción sustractiva del
            // catálogo: pierde organización que la persona creó a mano.
            Self::RemoveFromCollection { .. } => ConfirmationPolicy::Always,
            Self::Query { .. } => ConfirmationPolicy::ReadOnly,
            _ => ConfirmationPolicy::Automatic,
        }
    }

    /// Comprueba el esquema de argumentos sin tocar la base de datos.
    pub fn validate(&self) -> AppResult<()> {
        match self {
            Self::RegisterSession {
                game,
                minutes,
                started_at,
                progress,
                note,
            } => {
                game.validate()?;
                if *minutes == 0 || *minutes > MAX_SESSION_MINUTES {
                    return Err(AppError::validation(format!(
                        "La sesión debe durar entre 1 y {MAX_SESSION_MINUTES} minutos."
                    )));
                }
                if let Some(started_at) = started_at {
                    parse_timestamp(started_at, "el inicio de la sesión")?;
                }
                validate_progress(*progress)?;
                validate_length(note, MAX_SESSION_NOTE, "la nota de la sesión")?;
                Ok(())
            }
            Self::MarkFinished {
                game,
                completed_on,
                priority,
                ..
            } => {
                game.validate()?;
                if let Some(completed_on) = completed_on {
                    parse_date(completed_on, "de finalización")?;
                }
                validate_priority(*priority)?;
                Ok(())
            }
            Self::ChangeStatus { game, status_id } => {
                game.validate()?;
                validate_identifier(status_id, "el estado")?;
                Ok(())
            }
            Self::AdjustPriority {
                game,
                priority,
                delta,
            } => {
                game.validate()?;
                match (priority, delta) {
                    (Some(value), None) => validate_priority(Some(*value)),
                    (None, Some(delta)) => {
                        if *delta == 0 || delta.unsigned_abs() > 5 {
                            return Err(AppError::validation(
                                "El ajuste de prioridad debe estar entre -5 y 5 y no puede ser cero.",
                            ));
                        }
                        Ok(())
                    }
                    (Some(_), Some(_)) => Err(AppError::validation(
                        "Indica «priority» o «delta», no los dos a la vez.",
                    )),
                    (None, None) => Err(AppError::validation(
                        "Falta la prioridad: indica «priority» o «delta».",
                    )),
                }
            }
            Self::Pin { game, .. } | Self::Track { game, .. } => game.validate(),
            Self::Rate { game, rating } => {
                game.validate()?;
                if rating.is_some_and(|value| !(1..=10).contains(&value)) {
                    return Err(AppError::validation(
                        "La valoración debe estar entre 1 y 10, o ser nula para borrarla.",
                    ));
                }
                Ok(())
            }
            Self::Annotate { game, note, .. } => {
                game.validate()?;
                validate_length(note, MAX_PERSONAL_NOTE, "la nota")?;
                Ok(())
            }
            Self::SetNextAction { game, action } => {
                game.validate()?;
                if let Some(action) = action {
                    validate_length(action, MAX_SHORT_TEXT, "la próxima acción")?;
                }
                Ok(())
            }
            Self::SetCheckpoint { game, checkpoint } => {
                game.validate()?;
                if let Some(checkpoint) = checkpoint {
                    validate_length(checkpoint, MAX_SHORT_TEXT, "el punto de guardado")?;
                }
                Ok(())
            }
            Self::CreateCollection {
                name,
                description,
                color,
                icon,
                games,
            } => {
                validate_name(name, "de la colección")?;
                validate_length(description, MAX_DESCRIPTION, "la descripción")?;
                validate_hex_color(color)?;
                validate_name(icon, "del icono")?;
                validate_games(games)
            }
            Self::AddToCollection { collection, games }
            | Self::RemoveFromCollection { collection, games } => {
                collection.validate("la colección")?;
                if games.is_empty() {
                    return Err(AppError::validation("Indica al menos un juego."));
                }
                validate_games(games)
            }
            Self::CreateCuratedList {
                name,
                description,
                kind,
                accent,
                icon,
                ..
            } => {
                validate_name(name, "de la lista curada")?;
                validate_length(description, MAX_DESCRIPTION, "la descripción")?;
                crate::db::curated::validate_choice(
                    kind,
                    &crate::db::curated::CURATED_KINDS,
                    "tipo de lista",
                )?;
                crate::db::curated::validate_choice(
                    accent,
                    &crate::db::curated::CURATED_ACCENTS,
                    "acento de la lista",
                )?;
                validate_name(icon, "del icono")?;
                Ok(())
            }
            Self::AddToCuratedList {
                list, games, note, ..
            } => {
                list.validate("la lista curada")?;
                if games.is_empty() {
                    return Err(AppError::validation("Indica al menos un juego."));
                }
                validate_length(note, crate::db::curated::MAX_NOTE_LENGTH, "la nota")?;
                validate_games(games)
            }
            Self::Plan {
                game,
                column_id,
                target_date,
                estimated_minutes,
            } => {
                game.validate()?;
                validate_identifier(column_id, "la columna del planificador")?;
                if let Some(target_date) = target_date {
                    parse_date(target_date, "objetivo")?;
                }
                if estimated_minutes.is_some_and(|value| value > 100_000) {
                    return Err(AppError::validation(
                        "La estimación no puede superar 100.000 minutos.",
                    ));
                }
                Ok(())
            }
            Self::ScheduleReminder { game, due_at, note } => {
                game.validate()?;
                parse_timestamp(due_at, "el vencimiento del aviso")?;
                validate_length(note, MAX_SHORT_TEXT, "la nota del aviso")?;
                Ok(())
            }
            Self::Query { query } => query.validate(),
        }
    }
}

impl AgentQuery {
    pub fn validate(&self) -> AppResult<()> {
        match self {
            Self::Library {
                text,
                status_id,
                limit,
            } => {
                validate_length(text, 200, "el texto de búsqueda")?;
                if let Some(status_id) = status_id {
                    validate_identifier(status_id, "el estado")?;
                }
                validate_limit(*limit)
            }
            Self::Game { game } => game.validate(),
            Self::Sessions { game, limit } => {
                game.validate()?;
                validate_limit(*limit)
            }
            Self::Audit { limit } => validate_limit(*limit),
            Self::Statuses
            | Self::Collections
            | Self::CuratedLists
            | Self::Planner
            | Self::Reminders => Ok(()),
        }
    }
}

/// Normaliza una marca de tiempo ISO-8601 al formato que guarda SQLite.
pub fn parse_timestamp(value: &str, label: &str) -> AppResult<String> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|value| {
            value
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Secs, true)
        })
        .map_err(|_| {
            AppError::validation(format!(
                "La fecha de {label} debe seguir el formato ISO-8601, por ejemplo 2026-08-18T21:30:00Z."
            ))
        })
}

/// Valida una fecha `AAAA-MM-DD` y la devuelve normalizada.
pub fn parse_date(value: &str, label: &str) -> AppResult<String> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d")
        .map(|value| value.format("%Y-%m-%d").to_string())
        .map_err(|_| {
            AppError::validation(format!("La fecha {label} debe usar el formato AAAA-MM-DD."))
        })
}

fn validate_progress(progress: Option<u8>) -> AppResult<()> {
    if progress.is_some_and(|value| value > 100) {
        return Err(AppError::validation(
            "El progreso debe estar entre 0 y 100.",
        ));
    }
    Ok(())
}

fn validate_priority(priority: Option<u8>) -> AppResult<()> {
    if priority.is_some_and(|value| value > 5) {
        return Err(AppError::validation("La prioridad debe estar entre 0 y 5."));
    }
    Ok(())
}

fn validate_length(value: &str, maximum: usize, label: &str) -> AppResult<()> {
    if value.chars().count() > maximum {
        return Err(AppError::validation(format!(
            "{} no puede superar {maximum} caracteres.",
            capitalize(label)
        )));
    }
    Ok(())
}

fn validate_name(value: &str, label: &str) -> AppResult<()> {
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length == 0 || length > MAX_NAME {
        return Err(AppError::validation(format!(
            "El nombre {label} debe tener entre 1 y {MAX_NAME} caracteres."
        )));
    }
    Ok(())
}

fn validate_identifier(value: &str, label: &str) -> AppResult<()> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 128 {
        return Err(AppError::validation(format!(
            "El identificador de {label} no es válido."
        )));
    }
    Ok(())
}

fn validate_hex_color(value: &str) -> AppResult<()> {
    let trimmed = value.trim();
    let valid = trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit());
    if valid {
        return Ok(());
    }
    Err(AppError::validation(
        "El color debe ser hexadecimal con la forma #RRGGBB.",
    ))
}

fn validate_limit(limit: Option<u32>) -> AppResult<()> {
    if limit.is_some_and(|value| value == 0 || value > 200) {
        return Err(AppError::validation(
            "El límite de resultados debe estar entre 1 y 200.",
        ));
    }
    Ok(())
}

fn validate_games(games: &[GameSelector]) -> AppResult<()> {
    if games.len() > MAX_GAMES_PER_INTENT {
        return Err(AppError::validation(format!(
            "Una intención no puede afectar a más de {MAX_GAMES_PER_INTENT} juegos."
        )));
    }
    for game in games {
        game.validate()?;
    }
    Ok(())
}

fn capitalize(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}
