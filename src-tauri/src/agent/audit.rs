//! Registro auditable de todo lo que hace un agente.
//!
//! Cada petición deja **exactamente una** fila en `agent_audit_log`, aplicada,
//! rechazada, fallida o pendiente de confirmación. La fila guarda la intención,
//! la frase original, los argumentos ya normalizados, el resultado y los juegos
//! afectados. Sin fila no hay cambio: la escritura del registro va dentro de la
//! misma transacción que el cambio, así que no puede existir una modificación
//! silenciosa.
//!
//! ## Dónde vive el recibo de deshacer
//!
//! La migración 026 no reservó una columna para el recibo. En lugar de esperar
//! a una migración nueva, `affected_json` guarda un objeto con tres claves:
//! `games` (los juegos afectados), `command` (la intención ya resuelta,
//! únicamente mientras la fila está `pending`) y `receipt` (el recibo de
//! deshacer, una vez aplicada). El `CHECK` de la tabla no restringe la forma de
//! esa columna, así que el objeto es válido para el esquema vigente. El informe
//! de integración propone la migración 027 con una columna `receipt_json`
//! dedicada como forma definitiva.

use crate::agent::intent::AgentIntent;
use crate::agent::receipt::UndoReceipt;
use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

/// Filas conservadas en el registro. Por encima de este número se recortan las
/// más antiguas ya cerradas.
pub const MAX_AUDIT_ROWS: usize = 5_000;

/// Estado de una entrada del registro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditResult {
    /// Esperando confirmación humana.
    Pending,
    /// Aplicada.
    Applied,
    /// La persona usuaria la rechazó.
    Rejected,
    /// No se pudo ejecutar: validación, ámbito, ambigüedad o error interno.
    Failed,
    /// Aplicada y después deshecha.
    Undone,
}

impl AuditResult {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
            Self::Undone => "undone",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "applied" => Self::Applied,
            "rejected" => Self::Rejected,
            "failed" => Self::Failed,
            "undone" => Self::Undone,
            _ => Self::Pending,
        }
    }
}

/// Juego tocado por una intención.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AffectedGame {
    pub app_id: u32,
    pub title: String,
}

/// Contenido de `affected_json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuditPayload {
    #[serde(default)]
    pub games: Vec<AffectedGame>,
    /// Intención con todos los identificadores ya resueltos. Solo se conserva
    /// mientras la fila espera confirmación.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<AgentIntent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<UndoReceipt>,
}

impl AuditPayload {
    pub fn with_games(games: Vec<AffectedGame>) -> Self {
        Self {
            games,
            ..Self::default()
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{\"games\":[]}".to_string())
    }

    pub fn from_json(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_default()
    }
}

/// Fila lista para insertarse.
#[derive(Debug, Clone)]
pub struct AuditRecord {
    pub id: String,
    pub client_id: Option<String>,
    pub intent: String,
    pub utterance: String,
    pub arguments_json: String,
    pub result: AuditResult,
    pub payload: AuditPayload,
    pub undo_token: Option<String>,
    pub error_message: Option<String>,
}

impl AuditRecord {
    /// Construye una fila a partir de la intención recibida.
    pub fn new(
        id: impl Into<String>,
        client_id: Option<String>,
        intent: &AgentIntent,
        utterance: &str,
        result: AuditResult,
    ) -> Self {
        Self {
            id: id.into(),
            client_id,
            intent: intent.name().to_string(),
            utterance: truncate(utterance, 2_000),
            arguments_json: serde_json::to_string(intent).unwrap_or_else(|_| "{}".to_string()),
            result,
            payload: AuditPayload::default(),
            undo_token: None,
            error_message: None,
        }
    }
}

/// Entrada tal y como se muestra en la interfaz.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentAuditEntry {
    pub id: String,
    pub client_id: Option<String>,
    pub client_name: Option<String>,
    pub intent: String,
    pub utterance: String,
    pub arguments: serde_json::Value,
    pub result: AuditResult,
    pub affected: Vec<AffectedGame>,
    pub undoable: bool,
    pub error_message: Option<String>,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// Inserta una fila. `connection` puede ser una transacción en curso.
pub fn insert(connection: &Connection, record: &AuditRecord) -> AppResult<()> {
    let completed = !matches!(record.result, AuditResult::Pending);
    connection.execute(
        "INSERT INTO agent_audit_log(
            id, client_id, intent, utterance, arguments_json,
            result, affected_json, undo_token, error_message, completed_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
            CASE WHEN ?10 = 1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE NULL END
         )",
        params![
            record.id,
            record.client_id,
            record.intent,
            record.utterance,
            record.arguments_json,
            record.result.as_str(),
            record.payload.to_json(),
            record.undo_token,
            record.error_message,
            i64::from(completed),
        ],
    )?;
    Ok(())
}

/// Cierra una fila que estaba pendiente.
pub fn complete(
    connection: &Connection,
    audit_id: &str,
    result: AuditResult,
    payload: &AuditPayload,
    undo_token: Option<&str>,
    error_message: Option<&str>,
) -> AppResult<()> {
    let changed = connection.execute(
        "UPDATE agent_audit_log
            SET result = ?2,
                affected_json = ?3,
                undo_token = ?4,
                error_message = ?5,
                completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1 AND result = 'pending'",
        params![
            audit_id,
            result.as_str(),
            payload.to_json(),
            undo_token,
            error_message,
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found(
            "La acción pendiente ya no existe o ya se resolvió.",
        ));
    }
    Ok(())
}

/// Fila pendiente lista para confirmarse o rechazarse.
// Los campos que hoy nadie lee forman parte del modelo que el puente
// devuelve a quien lo integre: recortarlos obligaría a volver a consultar
// la base para reconstruirlos.
#[allow(dead_code, reason = "modelo público del puente de agentes")]
#[derive(Debug, Clone)]
pub struct PendingAction {
    pub id: String,
    pub client_id: Option<String>,
    pub intent: AgentIntent,
    pub utterance: String,
    pub payload: AuditPayload,
}

/// Recupera una acción pendiente.
pub fn find_pending(connection: &Connection, audit_id: &str) -> AppResult<PendingAction> {
    let row: Option<(Option<String>, String, String)> = connection
        .query_row(
            "SELECT client_id, utterance, affected_json
               FROM agent_audit_log
              WHERE id = ?1 AND result = 'pending'",
            [audit_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((client_id, utterance, affected_json)) = row else {
        return Err(AppError::not_found(
            "No hay ninguna acción pendiente con ese identificador.",
        ));
    };
    let payload = AuditPayload::from_json(&affected_json);
    let Some(intent) = payload.command.clone() else {
        return Err(AppError::new(
            "agent_receipt",
            "La acción pendiente no conserva la intención resuelta y no puede confirmarse.",
        ));
    };
    Ok(PendingAction {
        id: audit_id.to_string(),
        client_id,
        intent,
        utterance,
        payload,
    })
}

/// Acción aplicada localizada por su token de deshacer.
// Los campos que hoy nadie lee forman parte del modelo que el puente
// devuelve a quien lo integre: recortarlos obligaría a volver a consultar
// la base para reconstruirlos.
#[allow(dead_code, reason = "modelo público del puente de agentes")]
#[derive(Debug, Clone)]
pub struct UndoableAction {
    pub id: String,
    pub client_id: Option<String>,
    pub payload: AuditPayload,
    pub receipt: UndoReceipt,
}

/// Localiza una acción por su token de deshacer.
/// Testigo de deshacer de una acción propia, buscado por su identificador.
///
/// # Por qué existe
///
/// El testigo son sesenta y cuatro caracteres al azar. Un modelo que tiene que
/// repetirlos de memoria se come alguno —medido: perdió ocho— y el deshacer
/// falla por un motivo que no tiene nada que ver con lo que se pedía. El
/// identificador de la acción sí lo puede manejar: es un UUID que además ya
/// aparece en la auditoría.
///
/// No debilita nada: se exige que la acción sea **de ese cliente** y que siga
/// aplicada, que es exactamente lo que comprueba el deshacer normal.
pub fn undo_token_for_client(
    connection: &Connection,
    audit_id: &str,
    client_id: &str,
) -> AppResult<String> {
    connection
        .query_row(
            "SELECT undo_token
               FROM agent_audit_log
              WHERE id = ?1 AND client_id = ?2 AND result = 'applied'
                AND undo_token IS NOT NULL",
            [audit_id, client_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found("No hay ninguna acción tuya aplicada con ese identificador.")
        })
}

pub fn find_undoable(connection: &Connection, undo_token: &str) -> AppResult<UndoableAction> {
    if undo_token.trim().is_empty() || undo_token.len() > 256 {
        return Err(AppError::validation("El token de deshacer no es válido."));
    }
    let row: Option<(String, Option<String>, String)> = connection
        .query_row(
            "SELECT id, client_id, affected_json
               FROM agent_audit_log
              WHERE undo_token = ?1 AND result = 'applied'",
            [undo_token],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((id, client_id, affected_json)) = row else {
        return Err(AppError::not_found(
            "No hay ninguna acción aplicada con ese token de deshacer.",
        ));
    };
    let payload = AuditPayload::from_json(&affected_json);
    let Some(receipt) = payload.receipt.clone() else {
        return Err(crate::agent::receipt::invalid_receipt());
    };
    Ok(UndoableAction {
        id,
        client_id,
        payload,
        receipt,
    })
}

/// Marca una acción como deshecha e invalida su token, que es de un solo uso.
pub fn mark_undone(connection: &Connection, audit_id: &str) -> AppResult<()> {
    let changed = connection.execute(
        "UPDATE agent_audit_log
            SET result = 'undone',
                undo_token = NULL,
                completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1 AND result = 'applied'",
        [audit_id],
    )?;
    if changed != 1 {
        return Err(crate::agent::receipt::stale_receipt());
    }
    Ok(())
}

/// Últimas entradas del registro, de la más reciente a la más antigua.
///
/// El desempate es por `rowid` y no por `id`: dentro del mismo milisegundo
/// varias filas comparten `created_at` y el identificador es un UUID aleatorio,
/// que daría un orden distinto en cada consulta.
pub fn list(connection: &Connection, limit: u32) -> AppResult<Vec<AgentAuditEntry>> {
    let limit = limit.clamp(1, 200);
    let mut statement = connection.prepare(
        "SELECT a.id, a.client_id, c.name, a.intent, a.utterance, a.arguments_json,
                a.result, a.affected_json, a.undo_token, a.error_message,
                a.created_at, a.completed_at
           FROM agent_audit_log a
           LEFT JOIN agent_clients c ON c.id = a.client_id
          ORDER BY a.created_at DESC, a.rowid DESC
          LIMIT ?1",
    )?;
    let rows = statement.query_map([limit], map_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn map_entry(row: &Row<'_>) -> rusqlite::Result<AgentAuditEntry> {
    let payload = AuditPayload::from_json(&row.get::<_, String>(7)?);
    let arguments =
        serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or(serde_json::Value::Null);
    Ok(AgentAuditEntry {
        id: row.get(0)?,
        client_id: row.get(1)?,
        client_name: row.get(2)?,
        intent: row.get(3)?,
        utterance: row.get(4)?,
        arguments,
        result: AuditResult::parse(&row.get::<_, String>(6)?),
        affected: payload.games,
        undoable: row.get::<_, Option<String>>(8)?.is_some(),
        error_message: row.get(9)?,
        created_at: row.get(10)?,
        completed_at: row.get(11)?,
    })
}

/// Recorta el registro conservando las entradas más recientes.
///
/// Las filas `pending` nunca se recortan: siguen esperando una decisión humana.
pub fn prune(connection: &Connection) -> AppResult<usize> {
    let removed = connection.execute(
        "DELETE FROM agent_audit_log
          WHERE result <> 'pending'
            AND id NOT IN (
                SELECT id FROM agent_audit_log
                 ORDER BY created_at DESC, rowid DESC
                 LIMIT ?1
            )",
        [MAX_AUDIT_ROWS as i64],
    )?;
    Ok(removed)
}

fn truncate(value: &str, maximum: usize) -> String {
    let value = value.trim();
    if value.chars().count() <= maximum {
        return value.to_string();
    }
    value.chars().take(maximum).collect()
}
