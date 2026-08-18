//! API pública del puente: es lo único que deberían llamar los comandos Tauri.
//!
//! ## Reparto de responsabilidades
//!
//! - [`dispatch`] lo llama **el agente**, con su token. Autentica, limita la
//!   frecuencia, valida el esquema, comprueba el ámbito, resuelve nombres y, si
//!   procede, aplica. Todo deja rastro en `agent_audit_log`.
//! - [`confirm`] lo llama **la persona usuaria** desde la interfaz de Vindexa,
//!   sin token de agente. Un agente no puede aprobar sus propias acciones
//!   destructivas: si pudiera, la confirmación no sería una barrera.
//! - [`undo`] lo pueden llamar ambos. El agente solo puede deshacer lo que él
//!   mismo aplicó; la persona usuaria puede deshacer cualquier cosa.
//!
//! ## Por qué no hay socket
//!
//! `SECURITY.md` describe una frontera en la que la ventana principal solo
//! puede invocar los comandos registrados en `lib.rs` y toda la red, el
//! sistema de archivos y SQLite viven en Rust. Abrir un socket TCP —aunque
//! fuera en `127.0.0.1`— añadiría un puerto de escucha permanente, accesible
//! para cualquier proceso local del usuario y para cualquier página web capaz
//! de hacer una petición al bucle local. Sería una frontera de confianza nueva
//! y peor. Este puente es una API de proceso: el transporte lo elige el
//! integrador. La ruta recomendada, y la única sin puertos, es un comando
//! Tauri más un proceso acompañante lanzado por la propia Vindexa. El informe
//! de integración detalla el diff exacto de `lib.rs` y `commands.rs`.

use crate::agent::audit::{
    self, AffectedGame, AuditPayload, AuditRecord, AuditResult, PendingAction,
};
use crate::agent::clients::{self, AuthenticatedClient};
use crate::agent::executor::{self, ApplyOutcome, AuditSlot, Resolution, Resolved};
use crate::agent::intent::{AgentIntent, CONFIRMATION_THRESHOLD, ConfirmationPolicy};
use crate::agent::matching::GameCandidate;
use crate::agent::ratelimit::RateLimiter;
use crate::agent::receipt;
use crate::agent::token;
use crate::error::{AppError, AppResult};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Petición que envía un agente.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRequest {
    /// Token emitido por Vindexa. Nunca se registra ni se devuelve.
    pub token: String,
    /// Frase original de la persona usuaria, tal cual la recibió el agente.
    #[serde(default)]
    pub utterance: String,
    pub intent: AgentIntent,
}

/// Resultado de una petición.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "outcome",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AgentOutcome {
    /// Cambio aplicado.
    Applied {
        audit_id: String,
        undo_token: Option<String>,
        affected: Vec<AffectedGame>,
        summary: String,
    },
    /// Cambio a la espera de que una persona lo confirme en Vindexa.
    PendingConfirmation {
        audit_id: String,
        reason: String,
        affected: Vec<AffectedGame>,
        summary: String,
    },
    /// El nombre del juego no era inequívoco: hay que elegir.
    NeedsGameChoice {
        audit_id: String,
        query: String,
        candidates: Vec<GameCandidate>,
    },
    /// Respuesta a una consulta de solo lectura.
    Answer { audit_id: String, data: Value },
    /// Acción rechazada por una persona.
    Rejected { audit_id: String },
    /// Acción deshecha.
    Undone { audit_id: String, restored: usize },
}

/// Quién pide deshacer una acción.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requester {
    /// La persona usuaria, desde la interfaz de Vindexa.
    Human,
    /// Un agente, identificado por su token.
    Client(String),
}

/// Punto de entrada del agente.
pub fn dispatch(
    connection: &mut Connection,
    limiter: &RateLimiter,
    request: &AgentRequest,
) -> AppResult<AgentOutcome> {
    let audit_id = Uuid::new_v4().to_string();
    let now_millis = Utc::now().timestamp_millis();

    // 1. El token se descompone antes de tocar la base: así el límite de
    //    frecuencia protege también al derivador de claves, que es caro.
    let parsed = token::parse(&request.token);
    let limiter_key = parsed
        .as_ref()
        .map(|parsed| parsed.client_id.clone())
        .unwrap_or_else(|_| "anonymous".to_string());
    if let Err(error) = limiter.check(&limiter_key, now_millis) {
        return Err(fail(connection, &audit_id, None, request, error));
    }
    if let Err(error) = parsed {
        return Err(fail(connection, &audit_id, None, request, error));
    }

    // 2. Autenticación.
    let client = match clients::authenticate(connection, &request.token) {
        Ok(client) => client,
        Err(error) => return Err(fail(connection, &audit_id, None, request, error)),
    };
    let client_id = Some(client.id.clone());
    let _ = clients::touch(connection, &client.id);

    match run(connection, &audit_id, &client, request) {
        Ok(outcome) => {
            let _ = audit::prune(connection);
            Ok(outcome)
        }
        Err(error) => Err(fail(connection, &audit_id, client_id, request, error)),
    }
}

fn run(
    connection: &mut Connection,
    audit_id: &str,
    client: &AuthenticatedClient,
    request: &AgentRequest,
) -> AppResult<AgentOutcome> {
    // 3. Esquema de argumentos.
    request.intent.validate()?;
    // 4. Ámbito, antes de resolver nada y antes de cualquier escritura.
    client.scopes.require(request.intent.required_scope())?;

    // 5. Resolución de nombres.
    let resolution = match executor::resolve(connection, &request.intent)? {
        Resolved::Ready(resolution) => resolution,
        Resolved::NeedsGameChoice { query, candidates } => {
            let mut record = AuditRecord::new(
                audit_id,
                Some(client.id.clone()),
                &request.intent,
                &request.utterance,
                AuditResult::Failed,
            );
            record.error_message = Some(format!(
                "«{query}» coincide con varios juegos; el agente debe confirmar cuál."
            ));
            audit::insert(connection, &record)?;
            return Ok(AgentOutcome::NeedsGameChoice {
                audit_id: audit_id.to_string(),
                query,
                candidates,
            });
        }
    };

    // 6. Solo lectura: se responde y se registra, sin recibo de deshacer.
    if matches!(
        request.intent.confirmation_policy(),
        ConfirmationPolicy::ReadOnly
    ) {
        let data = match &resolution.intent {
            AgentIntent::Query { query } => executor::answer(connection, query)?,
            _ => Value::Null,
        };
        let mut record = AuditRecord::new(
            audit_id,
            Some(client.id.clone()),
            &resolution.intent,
            &request.utterance,
            AuditResult::Applied,
        );
        record.payload = AuditPayload::with_games(resolution.affected.clone());
        audit::insert(connection, &record)?;
        return Ok(AgentOutcome::Answer {
            audit_id: audit_id.to_string(),
            data,
        });
    }

    // 7. ¿Hace falta que una persona lo confirme?
    if let Some(reason) = confirmation_reason(&request.intent, &resolution) {
        let mut record = AuditRecord::new(
            audit_id,
            Some(client.id.clone()),
            &resolution.intent,
            &request.utterance,
            AuditResult::Pending,
        );
        record.payload = AuditPayload {
            games: resolution.affected.clone(),
            command: Some(resolution.intent.clone()),
            receipt: None,
        };
        audit::insert(connection, &record)?;
        return Ok(AgentOutcome::PendingConfirmation {
            audit_id: audit_id.to_string(),
            reason,
            affected: resolution.affected,
            summary: "La acción espera confirmación en Vindexa.".to_string(),
        });
    }

    // 8. Aplicación atómica junto con la fila de auditoría.
    let outcome = executor::apply(
        connection,
        &AuditSlot::Fresh {
            id: audit_id.to_string(),
            client_id: Some(client.id.clone()),
            utterance: request.utterance.clone(),
        },
        &resolution,
    )?;
    Ok(into_applied(outcome))
}

/// Motivo por el que una intención necesita confirmación humana, si lo hay.
fn confirmation_reason(intent: &AgentIntent, resolution: &Resolution) -> Option<String> {
    match intent.confirmation_policy() {
        ConfirmationPolicy::ReadOnly => None,
        ConfirmationPolicy::Always => Some(
            "La acción quita organización que creaste a mano y siempre se confirma.".to_string(),
        ),
        ConfirmationPolicy::Automatic => {
            if resolution.affected.len() > CONFIRMATION_THRESHOLD {
                Some(format!(
                    "La acción afecta a {} juegos, por encima del umbral de {CONFIRMATION_THRESHOLD}.",
                    resolution.affected.len()
                ))
            } else {
                None
            }
        }
    }
}

/// Confirma o rechaza una acción pendiente. Solo la persona usuaria.
pub fn confirm(
    connection: &mut Connection,
    audit_id: &str,
    approve: bool,
) -> AppResult<AgentOutcome> {
    let PendingAction {
        id,
        intent,
        payload,
        ..
    } = audit::find_pending(connection, audit_id)?;

    if !approve {
        audit::complete(
            connection,
            &id,
            AuditResult::Rejected,
            &AuditPayload::with_games(payload.games),
            None,
            None,
        )?;
        return Ok(AgentOutcome::Rejected { audit_id: id });
    }

    // Se vuelve a validar el esquema: la fila pudo quedar pendiente durante
    // días y no se confía en que siga siendo coherente.
    intent.validate()?;
    let resolution = Resolution {
        intent,
        affected: payload.games,
    };
    match executor::apply(
        connection,
        &AuditSlot::Confirm { id: id.clone() },
        &resolution,
    ) {
        Ok(outcome) => Ok(into_applied(outcome)),
        Err(error) => {
            let _ = audit::complete(
                connection,
                &id,
                AuditResult::Failed,
                &AuditPayload::with_games(resolution.affected),
                None,
                Some(&error.message),
            );
            Err(error)
        }
    }
}

/// Deshace una acción aplicada. El token de deshacer es de un solo uso.
pub fn undo(
    connection: &mut Connection,
    undo_token: &str,
    requester: &Requester,
) -> AppResult<AgentOutcome> {
    let action = audit::find_undoable(connection, undo_token)?;
    if let Requester::Client(client_id) = requester
        && action.client_id.as_deref() != Some(client_id.as_str())
    {
        return Err(AppError::new(
            "agent_scope",
            "Un agente solo puede deshacer las acciones que él mismo aplicó.",
        ));
    }

    let restored = receipt::undo(connection, &action.receipt)?;
    audit::mark_undone(connection, &action.id)?;
    Ok(AgentOutcome::Undone {
        audit_id: action.id,
        restored,
    })
}

/// Deshace usando el token de un agente para identificarse.
pub fn undo_as_client(
    connection: &mut Connection,
    limiter: &RateLimiter,
    agent_token: &str,
    undo_token: &str,
) -> AppResult<AgentOutcome> {
    let parsed = token::parse(agent_token)?;
    limiter.check(&parsed.client_id, Utc::now().timestamp_millis())?;
    let client = clients::authenticate(connection, agent_token)?;
    undo(connection, undo_token, &Requester::Client(client.id))
}

fn into_applied(outcome: ApplyOutcome) -> AgentOutcome {
    AgentOutcome::Applied {
        audit_id: outcome.audit_id,
        undo_token: outcome.undo_token,
        affected: outcome.affected,
        summary: outcome.summary,
    }
}

/// Registra el fallo y devuelve el error original sin alterarlo.
fn fail(
    connection: &Connection,
    audit_id: &str,
    client_id: Option<String>,
    request: &AgentRequest,
    error: AppError,
) -> AppError {
    let mut record = AuditRecord::new(
        audit_id,
        client_id,
        &request.intent,
        &request.utterance,
        AuditResult::Failed,
    );
    record.error_message = Some(format!("{}: {}", error.code, error.message));
    // Si el registro no se puede escribir, el error que importa sigue siendo
    // el original: la persona usuaria necesita saber qué falló, no que además
    // la auditoría tuvo un problema.
    let _ = audit::insert(connection, &record);
    error
}
