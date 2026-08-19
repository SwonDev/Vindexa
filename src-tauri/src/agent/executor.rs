//! Resolución y ejecución de las intenciones del agente.
//!
//! El flujo tiene dos mitades bien separadas:
//!
//! 1. [`resolve`] convierte una intención con nombres en lenguaje natural en
//!    una intención con identificadores concretos. Si un nombre no es
//!    inequívoco devuelve candidatos y **no** toca nada.
//! 2. [`apply`] ejecuta la intención ya resuelta dentro de una única
//!    transacción que incluye la fila de auditoría. O se escriben el cambio y
//!    su registro, o no se escribe ninguno de los dos.
//!
//! Las escrituras son SQL propio en lugar de llamadas a `db::organization` o
//! `db::curated` porque esas funciones abren su propia transacción sobre
//! `&mut Connection` y no se pueden anidar dentro de la del agente. Renunciar a
//! la atomicidad para reutilizarlas dejaría una ventana en la que un cambio
//! existe sin su fila de auditoría, que es justo lo que este puente evita. Las
//! reglas de validación sí se comparten: se reutilizan las constantes y los
//! comprobadores `pub(crate)` de `db::curated`.

use crate::agent::audit::{self, AffectedGame, AuditPayload, AuditRecord, AuditResult};
use crate::agent::intent::{
    AgentIntent, AgentQuery, EntitySelector, GameSelector, parse_date, parse_timestamp,
};
use crate::agent::matching::{self, GameCandidate, GameIndexEntry, GameMatch};
use crate::agent::receipt::{
    PersonalSnapshot, PlannerSnapshot, UndoReceipt, read_collection_order, read_personal,
    read_planner, write_collection_order, write_personal,
};
use crate::agent::token;
use crate::error::{AppError, AppResult};
use chrono::{Duration, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// Intención con todos los identificadores resueltos.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolution {
    pub intent: AgentIntent,
    pub affected: Vec<AffectedGame>,
}

/// Resultado de resolver una intención.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    /// Lista para aplicarse.
    Ready(Resolution),
    /// Hay que preguntar a qué juego se refiere.
    NeedsGameChoice {
        query: String,
        candidates: Vec<GameCandidate>,
    },
}

/// Resultado de aplicar una intención.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub audit_id: String,
    pub undo_token: Option<String>,
    pub affected: Vec<AffectedGame>,
    pub summary: String,
}

/// Dónde escribir la fila de auditoría al aplicar.
#[derive(Debug, Clone)]
pub enum AuditSlot {
    /// Fila nueva.
    Fresh {
        id: String,
        client_id: Option<String>,
        utterance: String,
    },
    /// Fila que ya existía en estado `pending` y hay que cerrar.
    Confirm { id: String },
}

impl AuditSlot {
    fn id(&self) -> &str {
        match self {
            Self::Fresh { id, .. } | Self::Confirm { id } => id,
        }
    }
}

// ---------------------------------------------------------------------------
// Resolución
// ---------------------------------------------------------------------------

/// Convierte nombres en identificadores. No modifica nada.
pub fn resolve(connection: &Connection, intent: &AgentIntent) -> AppResult<Resolved> {
    let mut resolved = intent.clone();
    let mut affected = Vec::new();

    match &mut resolved {
        AgentIntent::RegisterSession { game, .. }
        | AgentIntent::MarkFinished { game, .. }
        | AgentIntent::ChangeStatus { game, .. }
        | AgentIntent::AdjustPriority { game, .. }
        | AgentIntent::Pin { game, .. }
        | AgentIntent::Track { game, .. }
        | AgentIntent::Rate { game, .. }
        | AgentIntent::Annotate { game, .. }
        | AgentIntent::SetNextAction { game, .. }
        | AgentIntent::SetCheckpoint { game, .. }
        | AgentIntent::Plan { game, .. }
        | AgentIntent::ScheduleReminder { game, .. } => match resolve_game(connection, game)? {
            GameResolution::Ready(entry) => {
                *game = GameSelector {
                    app_id: Some(entry.app_id),
                    name: None,
                };
                affected.push(entry);
            }
            GameResolution::Choice { query, candidates } => {
                return Ok(Resolved::NeedsGameChoice { query, candidates });
            }
        },
        AgentIntent::CreateCollection { games, .. } => {
            if let Some(choice) = resolve_games(connection, games, &mut affected)? {
                return Ok(choice);
            }
        }
        AgentIntent::AddToCollection { collection, games }
        | AgentIntent::RemoveFromCollection { collection, games } => {
            *collection = resolve_collection(connection, collection)?;
            if let Some(choice) = resolve_games(connection, games, &mut affected)? {
                return Ok(choice);
            }
        }
        AgentIntent::AddToCuratedList { list, games, .. } => {
            *list = resolve_curated_list(connection, list)?;
            if let Some(choice) = resolve_games(connection, games, &mut affected)? {
                return Ok(choice);
            }
        }
        AgentIntent::CreateCuratedList { .. } => {}
        AgentIntent::Query { query } => {
            if let AgentQuery::Game { game } | AgentQuery::Sessions { game, .. } = query {
                match resolve_game(connection, game)? {
                    GameResolution::Ready(entry) => {
                        *game = GameSelector {
                            app_id: Some(entry.app_id),
                            name: None,
                        };
                        affected.push(entry);
                    }
                    GameResolution::Choice { query, candidates } => {
                        return Ok(Resolved::NeedsGameChoice { query, candidates });
                    }
                }
            }
        }
    }

    Ok(Resolved::Ready(Resolution {
        intent: resolved,
        affected,
    }))
}

enum GameResolution {
    Ready(AffectedGame),
    Choice {
        query: String,
        candidates: Vec<GameCandidate>,
    },
}

fn resolve_games(
    connection: &Connection,
    games: &mut [GameSelector],
    affected: &mut Vec<AffectedGame>,
) -> AppResult<Option<Resolved>> {
    for selector in games.iter_mut() {
        match resolve_game(connection, selector)? {
            GameResolution::Ready(entry) => {
                *selector = GameSelector {
                    app_id: Some(entry.app_id),
                    name: None,
                };
                affected.push(entry);
            }
            GameResolution::Choice { query, candidates } => {
                return Ok(Some(Resolved::NeedsGameChoice { query, candidates }));
            }
        }
    }
    Ok(None)
}

fn resolve_game(connection: &Connection, selector: &GameSelector) -> AppResult<GameResolution> {
    selector.validate()?;
    if let Some(app_id) = selector.app_id {
        let title = game_title(connection, app_id)?;
        // Si además venía el nombre, se comprueba que hablen del mismo juego.
        // Un agente que manda los dos casi siempre acierta; cuando no, es que
        // se ha equivocado de identificador, y aplicar el cambio al juego
        // equivocado sería el peor final posible.
        if let Some(name) = selector.name.as_deref().map(str::trim)
            && !name.is_empty()
            && !matching::same_game(name, &title)
        {
            return Err(AppError::validation(format!(
                "El AppID {app_id} es de «{title}», no de «{name}». Indica sólo uno de los dos."
            )));
        }
        return Ok(GameResolution::Ready(AffectedGame { app_id, title }));
    }
    let name = selector
        .name
        .as_deref()
        .unwrap_or_default()
        .trim()
        .to_string();
    let catalog = load_catalog(connection)?;
    match matching::resolve(&name, &catalog) {
        GameMatch::Resolved(candidate) => Ok(GameResolution::Ready(AffectedGame {
            app_id: candidate.app_id,
            title: candidate.title,
        })),
        GameMatch::Ambiguous(candidates) => Ok(GameResolution::Choice {
            query: name,
            candidates,
        }),
        GameMatch::NotFound => Err(AppError::not_found(format!(
            "No hay ningún juego de la biblioteca que se parezca a «{name}»."
        ))),
    }
}

fn load_catalog(connection: &Connection) -> AppResult<Vec<GameIndexEntry>> {
    let mut statement = connection.prepare("SELECT app_id, title FROM games")?;
    let rows = statement.query_map([], |row| {
        Ok(GameIndexEntry {
            app_id: row.get(0)?,
            title: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn game_title(connection: &Connection, app_id: u32) -> AppResult<String> {
    connection
        .query_row(
            "SELECT title FROM games WHERE app_id = ?1",
            [app_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("El juego {app_id} no está en la biblioteca.")))
}

fn resolve_collection(
    connection: &Connection,
    selector: &EntitySelector,
) -> AppResult<EntitySelector> {
    selector.validate("la colección")?;
    if let Some(id) = selector.id.as_deref() {
        ensure_manual_collection(connection, id.trim())?;
        return Ok(EntitySelector {
            id: Some(id.trim().to_string()),
            name: None,
        });
    }
    let wanted = matching::normalize(selector.name.as_deref().unwrap_or_default());
    let mut statement =
        connection.prepare("SELECT id, name FROM collections WHERE kind = 'manual'")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut matches = Vec::new();
    for row in rows {
        let (id, name) = row?;
        if matching::normalize(&name) == wanted {
            matches.push(id);
        }
    }
    match matches.len() {
        1 => Ok(EntitySelector {
            id: Some(matches.remove(0)),
            name: None,
        }),
        0 => Err(AppError::not_found(format!(
            "No existe ninguna colección manual llamada «{}».",
            selector.name.as_deref().unwrap_or_default().trim()
        ))),
        _ => Err(AppError::validation(
            "Hay varias colecciones con ese nombre. Indica la colección por «id».",
        )),
    }
}

fn resolve_curated_list(
    connection: &Connection,
    selector: &EntitySelector,
) -> AppResult<EntitySelector> {
    selector.validate("la lista curada")?;
    if let Some(id) = selector.id.as_deref() {
        ensure_curated_list(connection, id.trim())?;
        return Ok(EntitySelector {
            id: Some(id.trim().to_string()),
            name: None,
        });
    }
    let wanted = matching::normalize(selector.name.as_deref().unwrap_or_default());
    let mut statement = connection.prepare("SELECT id, name FROM curated_lists")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut matches = Vec::new();
    for row in rows {
        let (id, name) = row?;
        if matching::normalize(&name) == wanted {
            matches.push(id);
        }
    }
    match matches.len() {
        1 => Ok(EntitySelector {
            id: Some(matches.remove(0)),
            name: None,
        }),
        0 => Err(AppError::not_found(format!(
            "No existe ninguna lista curada llamada «{}».",
            selector.name.as_deref().unwrap_or_default().trim()
        ))),
        _ => Err(AppError::validation(
            "Hay varias listas con ese nombre. Indica la lista por «id».",
        )),
    }
}

// ---------------------------------------------------------------------------
// Aplicación
// ---------------------------------------------------------------------------

/// Aplica una intención ya resuelta y deja su rastro en la auditoría.
pub fn apply(
    connection: &mut Connection,
    slot: &AuditSlot,
    resolution: &Resolution,
) -> AppResult<ApplyOutcome> {
    let operation_id = Uuid::new_v4().to_string();
    let transaction = connection.transaction()?;
    let (receipt, summary) = execute(&transaction, &operation_id, &resolution.intent)?;
    let undo_token = receipt.as_ref().map(|_| token::mint_undo_token());

    let payload = AuditPayload {
        games: resolution.affected.clone(),
        command: None,
        receipt: receipt.clone(),
    };
    match slot {
        AuditSlot::Fresh {
            id,
            client_id,
            utterance,
        } => {
            let mut record = AuditRecord::new(
                id.clone(),
                client_id.clone(),
                &resolution.intent,
                utterance,
                AuditResult::Applied,
            );
            record.payload = payload;
            record.undo_token = undo_token.clone();
            audit::insert(&transaction, &record)?;
        }
        AuditSlot::Confirm { id } => {
            audit::complete(
                &transaction,
                id,
                AuditResult::Applied,
                &payload,
                undo_token.as_deref(),
                None,
            )?;
        }
    }
    transaction.commit()?;

    Ok(ApplyOutcome {
        audit_id: slot.id().to_string(),
        undo_token,
        affected: resolution.affected.clone(),
        summary,
    })
}

/// Ejecuta el cambio. Devuelve el recibo de deshacer y un resumen legible.
fn execute(
    transaction: &Transaction<'_>,
    operation_id: &str,
    intent: &AgentIntent,
) -> AppResult<(Option<UndoReceipt>, String)> {
    match intent {
        AgentIntent::RegisterSession {
            game,
            minutes,
            started_at,
            progress,
            note,
        } => register_session(
            transaction,
            operation_id,
            required_app_id(game)?,
            *minutes,
            started_at.as_deref(),
            *progress,
            note,
        ),
        AgentIntent::MarkFinished {
            game,
            completed_on,
            keep_playable,
            priority,
        } => mark_finished(
            transaction,
            operation_id,
            required_app_id(game)?,
            completed_on.as_deref(),
            *keep_playable,
            *priority,
        ),
        AgentIntent::ChangeStatus { game, status_id } => {
            let app_id = required_app_id(game)?;
            let status_id = status_id.trim();
            ensure_status(transaction, status_id)?;
            patch_personal(transaction, operation_id, app_id, |snapshot| {
                snapshot.status_id = status_id.to_string();
                Ok(())
            })
            .map(|receipt| {
                (
                    receipt,
                    format!("Estado cambiado a «{status_id}» para el juego {app_id}."),
                )
            })
        }
        AgentIntent::AdjustPriority {
            game,
            priority,
            delta,
        } => {
            let app_id = required_app_id(game)?;
            let (absolute, delta) = (*priority, *delta);
            let mut applied = 0u8;
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                let next = match (absolute, delta) {
                    (Some(value), _) => value,
                    (None, Some(delta)) => {
                        (i16::from(snapshot.priority) + i16::from(delta)).clamp(0, 5) as u8
                    }
                    (None, None) => snapshot.priority,
                };
                snapshot.priority = next;
                applied = next;
                Ok(())
            })?;
            Ok((receipt, format!("Prioridad fijada en {applied}.")))
        }
        AgentIntent::Pin { game, pinned } => {
            let app_id = required_app_id(game)?;
            let pinned = *pinned;
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                snapshot.pinned = pinned;
                Ok(())
            })?;
            Ok((
                receipt,
                if pinned {
                    "Juego fijado.".to_string()
                } else {
                    "Juego desfijado.".to_string()
                },
            ))
        }
        AgentIntent::Track { game, tracking } => {
            let app_id = required_app_id(game)?;
            let tracking = *tracking;
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                snapshot.tracking = tracking;
                Ok(())
            })?;
            Ok((
                receipt,
                if tracking {
                    "Seguimiento activado.".to_string()
                } else {
                    "Seguimiento desactivado.".to_string()
                },
            ))
        }
        AgentIntent::Rate { game, rating } => {
            let app_id = required_app_id(game)?;
            let rating = *rating;
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                snapshot.rating = rating;
                Ok(())
            })?;
            Ok((
                receipt,
                match rating {
                    Some(value) => format!("Valoración fijada en {value} sobre 10."),
                    None => "Valoración borrada.".to_string(),
                },
            ))
        }
        AgentIntent::Annotate { game, note, append } => {
            let app_id = required_app_id(game)?;
            let note = note.trim().to_string();
            let append = *append;
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                let combined = match (append, snapshot.notes.as_deref()) {
                    (true, Some(existing)) if !existing.trim().is_empty() => {
                        format!("{}\n{note}", existing.trim_end())
                    }
                    _ => note.clone(),
                };
                if combined.chars().count() > crate::agent::intent::MAX_PERSONAL_NOTE {
                    return Err(AppError::validation(format!(
                        "La nota resultante superaría {} caracteres.",
                        crate::agent::intent::MAX_PERSONAL_NOTE
                    )));
                }
                snapshot.notes = optional_text(&combined);
                Ok(())
            })?;
            Ok((receipt, "Nota guardada.".to_string()))
        }
        AgentIntent::SetNextAction { game, action } => {
            let app_id = required_app_id(game)?;
            let action = action.clone();
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                snapshot.next_action = action.as_deref().and_then(optional_text);
                Ok(())
            })?;
            Ok((receipt, "Próxima acción guardada.".to_string()))
        }
        AgentIntent::SetCheckpoint { game, checkpoint } => {
            let app_id = required_app_id(game)?;
            let checkpoint = checkpoint.clone();
            let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
                snapshot.checkpoint = checkpoint.as_deref().and_then(optional_text);
                Ok(())
            })?;
            Ok((receipt, "Punto de guardado registrado.".to_string()))
        }
        AgentIntent::CreateCollection {
            name,
            description,
            color,
            icon,
            games,
        } => create_collection(
            transaction,
            operation_id,
            name,
            description,
            color,
            icon,
            games,
        ),
        AgentIntent::AddToCollection { collection, games } => {
            change_collection(transaction, operation_id, collection, games, true)
        }
        AgentIntent::RemoveFromCollection { collection, games } => {
            change_collection(transaction, operation_id, collection, games, false)
        }
        AgentIntent::CreateCuratedList {
            name,
            description,
            kind,
            accent,
            icon,
            pinned,
        } => create_curated_list(
            transaction,
            operation_id,
            name,
            description,
            kind,
            accent,
            icon,
            *pinned,
        ),
        AgentIntent::AddToCuratedList {
            list,
            games,
            note,
            highlight,
        } => add_to_curated_list(transaction, operation_id, list, games, note, *highlight),
        AgentIntent::Plan {
            game,
            column_id,
            target_date,
            estimated_minutes,
        } => plan(
            transaction,
            operation_id,
            required_app_id(game)?,
            column_id.trim(),
            target_date.as_deref(),
            *estimated_minutes,
        ),
        AgentIntent::ScheduleReminder { game, due_at, note } => schedule_reminder(
            transaction,
            operation_id,
            required_app_id(game)?,
            due_at,
            note,
        ),
        AgentIntent::Query { .. } => Err(AppError::validation(
            "«consultar» no modifica nada y no debe aplicarse.",
        )),
    }
}

fn register_session(
    transaction: &Transaction<'_>,
    operation_id: &str,
    app_id: u32,
    minutes: u32,
    started_at: Option<&str>,
    progress: Option<u8>,
    note: &str,
) -> AppResult<(Option<UndoReceipt>, String)> {
    let duration = Duration::minutes(i64::from(minutes));
    let started = match started_at {
        Some(value) => chrono::DateTime::parse_from_rfc3339(
            parse_timestamp(value, "el inicio de la sesión")?.as_str(),
        )
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| AppError::validation("La fecha de inicio de la sesión no es válida."))?,
        None => Utc::now() - duration,
    };
    let ended = started + duration;
    let started_text = started.to_rfc3339_opts(SecondsFormat::Secs, true);
    let ended_text = ended.to_rfc3339_opts(SecondsFormat::Secs, true);
    let session_day = started.format("%Y-%m-%d").to_string();

    let previous = read_personal(transaction, app_id)?;
    let session_id = Uuid::new_v4().to_string();
    let activity_id = Uuid::new_v4().to_string();

    transaction.execute(
        "INSERT INTO game_sessions(
            id, app_id, started_at, ended_at, progress_before, progress_after, note
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session_id,
            app_id,
            started_text,
            ended_text,
            i64::from(previous.progress),
            progress.map(i64::from),
            note.trim(),
        ],
    )?;
    transaction.execute(
        "INSERT INTO activity(id, kind, app_id, message) VALUES (?1, 'session_create', ?2, ?3)",
        params![
            activity_id,
            app_id,
            format!("Un agente registró una sesión de {minutes} minutos.")
        ],
    )?;

    let mut applied = previous.clone();
    if let Some(progress) = progress {
        applied.progress = progress;
    }
    if applied.started_at.is_none() {
        applied.started_at = Some(session_day);
    }
    ensure_dates_consistent(&applied)?;
    write_personal(transaction, &applied)?;

    let summary = match progress {
        Some(progress) => {
            format!(
                "Sesión de {minutes} minutos registrada y progreso actualizado al {progress} %."
            )
        }
        None => format!("Sesión de {minutes} minutos registrada."),
    };
    Ok((
        Some(UndoReceipt::Session {
            operation_id: operation_id.to_string(),
            session_id,
            activity_id,
            previous: Some(Box::new(previous)),
            applied: Some(Box::new(applied)),
        }),
        summary,
    ))
}

fn mark_finished(
    transaction: &Transaction<'_>,
    operation_id: &str,
    app_id: u32,
    completed_on: Option<&str>,
    keep_playable: bool,
    priority: Option<u8>,
) -> AppResult<(Option<UndoReceipt>, String)> {
    let completed = match completed_on {
        Some(value) => parse_date(value, "de finalización")?,
        None => Utc::now().format("%Y-%m-%d").to_string(),
    };
    if !keep_playable {
        ensure_status(transaction, "completed")?;
    }
    let receipt = patch_personal(transaction, operation_id, app_id, |snapshot| {
        snapshot.progress = 100;
        snapshot.completed_at = Some(completed.clone());
        // Un juego terminado no puede estar además abandonado: el disparador
        // `personal_dates_validate_update` lo prohíbe y el significado tampoco
        // lo admite.
        snapshot.abandoned_at = None;
        if !keep_playable {
            snapshot.status_id = "completed".to_string();
        }
        if let Some(priority) = priority {
            snapshot.priority = priority;
        }
        ensure_dates_consistent(snapshot)
    })?;
    let summary = if keep_playable {
        format!("Marcado como terminado el {completed}; sigue en su estado actual.")
    } else {
        format!("Marcado como completado el {completed}.")
    };
    Ok((receipt, summary))
}

/// Lee la ficha personal, aplica un cambio y devuelve el recibo.
fn patch_personal(
    transaction: &Transaction<'_>,
    operation_id: &str,
    app_id: u32,
    patch: impl FnOnce(&mut PersonalSnapshot) -> AppResult<()>,
) -> AppResult<Option<UndoReceipt>> {
    let previous = read_personal(transaction, app_id)?;
    let mut applied = previous.clone();
    patch(&mut applied)?;
    ensure_dates_consistent(&applied)?;
    write_personal(transaction, &applied)?;
    Ok(Some(UndoReceipt::PersonalFields {
        operation_id: operation_id.to_string(),
        previous: vec![previous],
        applied: vec![applied],
    }))
}

/// Comprueba las mismas reglas que el disparador SQL, con un mensaje útil.
fn ensure_dates_consistent(snapshot: &PersonalSnapshot) -> AppResult<()> {
    if snapshot.completed_at.is_some() && snapshot.abandoned_at.is_some() {
        return Err(AppError::validation(
            "Un juego no puede estar terminado y abandonado a la vez.",
        ));
    }
    if let (Some(started), Some(completed)) = (&snapshot.started_at, &snapshot.completed_at)
        && completed.as_str() < started.as_str()
    {
        return Err(AppError::validation(format!(
            "La fecha de finalización ({completed}) es anterior al inicio registrado ({started})."
        )));
    }
    if let (Some(started), Some(abandoned)) = (&snapshot.started_at, &snapshot.abandoned_at)
        && abandoned.as_str() < started.as_str()
    {
        return Err(AppError::validation(
            "La fecha de abandono es anterior al inicio registrado.",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn create_collection(
    transaction: &Transaction<'_>,
    operation_id: &str,
    name: &str,
    description: &str,
    color: &str,
    icon: &str,
    games: &[GameSelector],
) -> AppResult<(Option<UndoReceipt>, String)> {
    let name = name.trim();
    let duplicated: Option<String> = transaction
        .query_row(
            "SELECT id FROM collections WHERE name = ?1 COLLATE NOCASE",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    if duplicated.is_some() {
        return Err(AppError::validation(
            "Ya existe una colección con ese nombre.",
        ));
    }
    let collection_id = Uuid::new_v4().to_string();
    let position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM collections",
        [],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO collections(id, name, description, color, icon, kind, match_mode, position)
         VALUES (?1, ?2, ?3, ?4, ?5, 'manual', 'all', ?6)",
        params![
            collection_id,
            name,
            description.trim(),
            color.trim(),
            icon.trim(),
            position
        ],
    )?;

    let members = distinct_app_ids(games)?;
    for app_id in &members {
        ensure_game_exists(transaction, *app_id)?;
    }
    write_collection_order(transaction, &collection_id, &members)?;

    Ok((
        Some(UndoReceipt::CollectionCreated {
            operation_id: operation_id.to_string(),
            collection_id,
            members: members.clone(),
        }),
        format!("Colección «{name}» creada con {} juego(s).", members.len()),
    ))
}

fn change_collection(
    transaction: &Transaction<'_>,
    operation_id: &str,
    collection: &EntitySelector,
    games: &[GameSelector],
    add: bool,
) -> AppResult<(Option<UndoReceipt>, String)> {
    let collection_id = collection
        .id
        .as_deref()
        .ok_or_else(|| AppError::validation("La colección no está resuelta."))?;
    ensure_manual_collection(transaction, collection_id)?;
    let requested = distinct_app_ids(games)?;
    let previous_order = read_collection_order(transaction, collection_id)?;

    let applied_order = if add {
        let mut next = previous_order.clone();
        for app_id in &requested {
            ensure_game_exists(transaction, *app_id)?;
            if !next.contains(app_id) {
                next.push(*app_id);
            }
        }
        next
    } else {
        previous_order
            .iter()
            .copied()
            .filter(|app_id| !requested.contains(app_id))
            .collect()
    };

    if applied_order == previous_order {
        return Err(AppError::validation(if add {
            "Todos los juegos indicados ya estaban en la colección."
        } else {
            "Ninguno de los juegos indicados estaba en la colección."
        }));
    }
    write_collection_order(transaction, collection_id, &applied_order)?;

    let difference = applied_order.len().abs_diff(previous_order.len());
    let summary = if add {
        format!("Añadido(s) {difference} juego(s) a la colección.")
    } else {
        format!("Quitado(s) {difference} juego(s) de la colección.")
    };
    Ok((
        Some(UndoReceipt::CollectionMembers {
            operation_id: operation_id.to_string(),
            collection_id: collection_id.to_string(),
            previous_order,
            applied_order,
        }),
        summary,
    ))
}

#[allow(clippy::too_many_arguments)]
fn create_curated_list(
    transaction: &Transaction<'_>,
    operation_id: &str,
    name: &str,
    description: &str,
    kind: &str,
    accent: &str,
    icon: &str,
    pinned: bool,
) -> AppResult<(Option<UndoReceipt>, String)> {
    let name = name.trim();
    let total: i64 =
        transaction.query_row("SELECT COUNT(*) FROM curated_lists", [], |row| row.get(0))?;
    if total as usize >= crate::db::curated::MAX_LISTS {
        return Err(AppError::validation(format!(
            "No se admiten más de {} listas curadas.",
            crate::db::curated::MAX_LISTS
        )));
    }
    let duplicated: Option<String> = transaction
        .query_row(
            "SELECT id FROM curated_lists WHERE name = ?1 COLLATE NOCASE",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    if duplicated.is_some() {
        return Err(AppError::validation(
            "Ya existe una lista curada con ese nombre.",
        ));
    }
    let list_id = Uuid::new_v4().to_string();
    let position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM curated_lists",
        [],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO curated_lists(
            id, name, description, kind, accent, icon, pinned, position
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            list_id,
            name,
            description.trim(),
            kind.trim(),
            accent.trim(),
            icon.trim(),
            i64::from(pinned),
            position
        ],
    )?;
    Ok((
        Some(UndoReceipt::CuratedListCreated {
            operation_id: operation_id.to_string(),
            list_id,
        }),
        format!("Lista curada «{name}» creada."),
    ))
}

fn add_to_curated_list(
    transaction: &Transaction<'_>,
    operation_id: &str,
    list: &EntitySelector,
    games: &[GameSelector],
    note: &str,
    highlight: bool,
) -> AppResult<(Option<UndoReceipt>, String)> {
    let list_id = list
        .id
        .as_deref()
        .ok_or_else(|| AppError::validation("La lista curada no está resuelta."))?;
    ensure_curated_list(transaction, list_id)?;
    let note = crate::db::curated::validate_note(note)?;
    let requested = distinct_app_ids(games)?;

    let existing: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM curated_list_items WHERE list_id = ?1",
        [list_id],
        |row| row.get(0),
    )?;
    let mut position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM curated_list_items WHERE list_id = ?1",
        [list_id],
        |row| row.get(0),
    )?;
    if existing as usize + requested.len() > crate::db::curated::MAX_ITEMS_PER_LIST {
        return Err(AppError::validation(format!(
            "Una lista curada no admite más de {} juegos.",
            crate::db::curated::MAX_ITEMS_PER_LIST
        )));
    }

    let mut added = Vec::new();
    for app_id in &requested {
        crate::db::curated::ensure_game_is_listable(transaction, *app_id)?;
        let already: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM curated_list_items WHERE list_id = ?1 AND app_id = ?2",
                params![list_id, app_id],
                |row| row.get(0),
            )
            .optional()?;
        if already.is_some() {
            continue;
        }
        transaction.execute(
            "INSERT INTO curated_list_items(list_id, app_id, position, note, highlight)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![list_id, app_id, position, note, i64::from(highlight)],
        )?;
        position += 1;
        added.push(*app_id);
    }
    if added.is_empty() {
        return Err(AppError::validation(
            "Todos los juegos indicados ya estaban en la lista.",
        ));
    }
    transaction.execute(
        "UPDATE curated_lists
            SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        [list_id],
    )?;
    let total = added.len();
    Ok((
        Some(UndoReceipt::CuratedMembers {
            operation_id: operation_id.to_string(),
            list_id: list_id.to_string(),
            added_app_ids: added,
        }),
        format!("Añadido(s) {total} juego(s) a la lista curada."),
    ))
}

fn plan(
    transaction: &Transaction<'_>,
    operation_id: &str,
    app_id: u32,
    column_id: &str,
    target_date: Option<&str>,
    estimated_minutes: Option<u32>,
) -> AppResult<(Option<UndoReceipt>, String)> {
    ensure_game_exists(transaction, app_id)?;
    let column_name: String = transaction
        .query_row(
            "SELECT name FROM planner_columns WHERE id = ?1",
            [column_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La columna del planificador ya no existe."))?;
    let target_date = target_date
        .map(|value| parse_date(value, "objetivo"))
        .transpose()?;

    let previous = read_planner(transaction, app_id)?;
    transaction.execute("DELETE FROM planner_items WHERE app_id = ?1", [app_id])?;
    let position: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM planner_items WHERE column_id = ?1",
        [column_id],
        |row| row.get(0),
    )?;
    transaction.execute(
        "INSERT INTO planner_items(column_id, app_id, position, target_date, estimated_minutes)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            column_id,
            app_id,
            position,
            target_date,
            estimated_minutes.map(i64::from)
        ],
    )?;
    let applied = PlannerSnapshot {
        column_id: column_id.to_string(),
        position,
        target_date,
        estimated_minutes,
    };
    Ok((
        Some(UndoReceipt::PlannerPlacement {
            operation_id: operation_id.to_string(),
            app_id,
            previous,
            applied,
        }),
        format!("Juego colocado en la columna «{column_name}»."),
    ))
}

fn schedule_reminder(
    transaction: &Transaction<'_>,
    operation_id: &str,
    app_id: u32,
    due_at: &str,
    note: &str,
) -> AppResult<(Option<UndoReceipt>, String)> {
    ensure_game_exists(transaction, app_id)?;
    let due_at = parse_timestamp(due_at, "el vencimiento del aviso")?;
    let reminder_id = Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO game_reminders(id, app_id, due_at, note) VALUES (?1, ?2, ?3, ?4)",
        params![reminder_id, app_id, due_at, note.trim()],
    )?;
    Ok((
        Some(UndoReceipt::Reminder {
            operation_id: operation_id.to_string(),
            reminder_id,
        }),
        format!("Aviso programado para el {due_at}."),
    ))
}

// ---------------------------------------------------------------------------
// Consultas de solo lectura
// ---------------------------------------------------------------------------

/// Resuelve una consulta. Nunca escribe.
pub fn answer(connection: &Connection, query: &AgentQuery) -> AppResult<Value> {
    match query {
        AgentQuery::Library {
            text,
            status_id,
            limit,
        } => {
            let limit = limit.unwrap_or(20).clamp(1, 200);
            let catalog = load_catalog(connection)?;
            let needle = matching::normalize(text);
            let mut rows = Vec::new();
            for entry in catalog {
                let score = if needle.is_empty() {
                    1.0
                } else {
                    matching::score(&needle, &matching::normalize(&entry.title))
                };
                if !needle.is_empty() && score < matching::CANDIDATE_THRESHOLD {
                    continue;
                }
                let Some(summary) = game_json(connection, entry.app_id)? else {
                    continue;
                };
                if let Some(status_id) = status_id
                    && summary.get("statusId").and_then(Value::as_str) != Some(status_id.trim())
                {
                    continue;
                }
                rows.push((score, summary));
            }
            rows.sort_by(|left, right| {
                right
                    .0
                    .partial_cmp(&left.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            rows.truncate(limit as usize);
            Ok(json!({
                "games": rows.into_iter().map(|(_, row)| row).collect::<Vec<_>>()
            }))
        }
        AgentQuery::Game { game } => {
            let app_id = required_app_id(game)?;
            let summary = game_json(connection, app_id)?
                .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))?;
            Ok(json!({ "game": summary }))
        }
        AgentQuery::Statuses => {
            let mut statement = connection.prepare(
                "SELECT s.id, s.name, COUNT(p.app_id)
                   FROM statuses s
                   LEFT JOIN game_personal p ON p.status_id = s.id
                  GROUP BY s.id, s.name, s.position
                  ORDER BY s.position ASC, s.id ASC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "games": row.get::<_, i64>(2)?,
                }))
            })?;
            Ok(json!({ "statuses": rows.collect::<Result<Vec<_>, _>>()? }))
        }
        AgentQuery::Collections => {
            let mut statement = connection.prepare(
                "SELECT c.id, c.name, c.kind,
                        (SELECT COUNT(*) FROM collection_games g WHERE g.collection_id = c.id)
                   FROM collections c
                  ORDER BY c.position ASC, c.id ASC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "kind": row.get::<_, String>(2)?,
                    "games": row.get::<_, i64>(3)?,
                }))
            })?;
            Ok(json!({ "collections": rows.collect::<Result<Vec<_>, _>>()? }))
        }
        AgentQuery::CuratedLists => {
            let mut statement = connection.prepare(
                "SELECT l.id, l.name, l.kind,
                        (SELECT COUNT(*) FROM curated_list_items i WHERE i.list_id = l.id)
                   FROM curated_lists l
                  ORDER BY l.pinned DESC, l.position ASC, l.id ASC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "kind": row.get::<_, String>(2)?,
                    "games": row.get::<_, i64>(3)?,
                }))
            })?;
            Ok(json!({ "lists": rows.collect::<Result<Vec<_>, _>>()? }))
        }
        AgentQuery::Planner => {
            let mut statement = connection.prepare(
                "SELECT c.id, c.name, i.app_id, g.title
                   FROM planner_columns c
                   LEFT JOIN planner_items i ON i.column_id = c.id
                   LEFT JOIN games g ON g.app_id = i.app_id
                  ORDER BY c.position ASC, i.position ASC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<u32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?;
            let mut columns: Vec<Value> = Vec::new();
            for row in rows {
                let (id, name, app_id, title) = row?;
                if columns
                    .last()
                    .and_then(|column| column.get("id"))
                    .and_then(Value::as_str)
                    != Some(id.as_str())
                {
                    columns.push(json!({ "id": id, "name": name, "games": [] }));
                }
                if let (Some(app_id), Some(title)) = (app_id, title)
                    && let Some(games) = columns
                        .last_mut()
                        .and_then(|column| column.get_mut("games"))
                        .and_then(Value::as_array_mut)
                {
                    games.push(json!({ "appId": app_id, "title": title }));
                }
            }
            Ok(json!({ "columns": columns }))
        }
        AgentQuery::Reminders => {
            let mut statement = connection.prepare(
                "SELECT r.id, r.app_id, g.title, r.due_at, r.note
                   FROM game_reminders r
                   JOIN games g ON g.app_id = r.app_id
                  WHERE r.completed_at IS NULL
                  ORDER BY r.due_at ASC, r.id ASC
                  LIMIT 100",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "appId": row.get::<_, u32>(1)?,
                    "title": row.get::<_, String>(2)?,
                    "dueAt": row.get::<_, String>(3)?,
                    "note": row.get::<_, String>(4)?,
                }))
            })?;
            Ok(json!({ "reminders": rows.collect::<Result<Vec<_>, _>>()? }))
        }
        AgentQuery::Sessions { game, limit } => {
            let app_id = required_app_id(game)?;
            let limit = limit.unwrap_or(10).clamp(1, 200);
            let mut statement = connection.prepare(
                "SELECT id, started_at, ended_at, progress_before, progress_after, note
                   FROM game_sessions
                  WHERE app_id = ?1
                  ORDER BY started_at DESC, id DESC
                  LIMIT ?2",
            )?;
            let rows = statement.query_map(params![app_id, limit], |row| {
                Ok(json!({
                    "id": row.get::<_, String>(0)?,
                    "startedAt": row.get::<_, String>(1)?,
                    "endedAt": row.get::<_, Option<String>>(2)?,
                    "progressBefore": row.get::<_, Option<i64>>(3)?,
                    "progressAfter": row.get::<_, Option<i64>>(4)?,
                    "note": row.get::<_, String>(5)?,
                }))
            })?;
            Ok(json!({ "sessions": rows.collect::<Result<Vec<_>, _>>()? }))
        }
        AgentQuery::Audit { limit } => {
            let entries = audit::list(connection, limit.unwrap_or(20))?;
            Ok(json!({ "entries": entries }))
        }
    }
}

fn game_json(connection: &Connection, app_id: u32) -> AppResult<Option<Value>> {
    let row = connection
        .query_row(
            "SELECT g.app_id, g.title, g.playtime_minutes, g.last_played_at,
                    p.status_id, p.progress, p.priority, p.pinned, p.tracking,
                    p.rating, p.next_action, p.checkpoint, p.notes,
                    p.started_at, p.completed_at
               FROM games g
               LEFT JOIN game_personal p ON p.app_id = g.app_id
              WHERE g.app_id = ?1",
            [app_id],
            |row| {
                Ok(json!({
                    "appId": row.get::<_, u32>(0)?,
                    "title": row.get::<_, String>(1)?,
                    "playtimeMinutes": row.get::<_, i64>(2)?,
                    "lastPlayedAt": row.get::<_, Option<String>>(3)?,
                    "statusId": row.get::<_, Option<String>>(4)?,
                    "progress": row.get::<_, Option<i64>>(5)?,
                    "priority": row.get::<_, Option<i64>>(6)?,
                    "pinned": row.get::<_, Option<i64>>(7)?.map(|value| value == 1),
                    "tracking": row.get::<_, Option<i64>>(8)?.map(|value| value == 1),
                    "rating": row.get::<_, Option<i64>>(9)?,
                    "nextAction": row.get::<_, Option<String>>(10)?,
                    "checkpoint": row.get::<_, Option<String>>(11)?,
                    "notes": row.get::<_, Option<String>>(12)?,
                    "startedAt": row.get::<_, Option<String>>(13)?,
                    "completedAt": row.get::<_, Option<String>>(14)?,
                }))
            },
        )
        .optional()?;
    Ok(row)
}

// ---------------------------------------------------------------------------
// Comprobaciones compartidas
// ---------------------------------------------------------------------------

fn required_app_id(selector: &GameSelector) -> AppResult<u32> {
    selector.app_id.ok_or_else(|| {
        AppError::validation("El juego no está resuelto: falta el AppID definitivo.")
    })
}

fn distinct_app_ids(games: &[GameSelector]) -> AppResult<Vec<u32>> {
    let mut ordered = Vec::with_capacity(games.len());
    for selector in games {
        let app_id = required_app_id(selector)?;
        if !ordered.contains(&app_id) {
            ordered.push(app_id);
        }
    }
    Ok(ordered)
}

fn ensure_game_exists(connection: &Connection, app_id: u32) -> AppResult<()> {
    connection
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("El juego {app_id} no está en la biblioteca.")))
}

fn ensure_status(connection: &Connection, status_id: &str) -> AppResult<()> {
    connection
        .query_row("SELECT 1 FROM statuses WHERE id = ?1", [status_id], |_| {
            Ok(())
        })
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(format!(
                "El estado «{status_id}» no existe en la biblioteca."
            ))
        })
}

fn ensure_manual_collection(connection: &Connection, collection_id: &str) -> AppResult<()> {
    let kind: Option<String> = connection
        .query_row(
            "SELECT kind FROM collections WHERE id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .optional()?;
    match kind.as_deref() {
        Some("manual") => Ok(()),
        Some(_) => Err(AppError::validation(
            "Una colección inteligente calcula su contenido con reglas y no admite cambios manuales.",
        )),
        None => Err(AppError::not_found("La colección ya no existe.")),
    }
}

fn ensure_curated_list(connection: &Connection, list_id: &str) -> AppResult<()> {
    connection
        .query_row(
            "SELECT 1 FROM curated_lists WHERE id = ?1",
            [list_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La lista curada ya no existe."))
}

fn optional_text(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
