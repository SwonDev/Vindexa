//! Registro de clientes agente: alta, rotación, revocación y autenticación.
//!
//! Un cliente es una identidad local con un token propio y un conjunto cerrado
//! de ámbitos. Vindexa nunca guarda el token: solo el resumen derivado descrito
//! en [`crate::agent::token`]. El token en claro se devuelve **una única vez**,
//! en el momento de la emisión, y a partir de ahí solo puede rotarse.

use crate::agent::scope::ScopeSet;
use crate::agent::token::{self, MintedToken, TokenPolicy};
use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Tipos de cliente admitidos por el `CHECK` de la migración 026.
pub const CLIENT_KINDS: [&str; 2] = ["hermes", "generic"];
/// Número máximo de clientes simultáneos. Un puente local no necesita más y el
/// tope acota el coste de una autenticación fallida.
pub const MAX_CLIENTS: usize = 16;
const MAX_NAME_LENGTH: usize = 64;

/// Alta de un cliente nuevo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAgentClient {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub scopes: Vec<String>,
}

fn default_kind() -> String {
    "hermes".to_string()
}

/// Cliente tal y como se muestra en la interfaz. Nunca incluye `token_hash`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentClientSummary {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub scopes: Vec<String>,
    pub enabled: bool,
    pub last_seen_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Resultado de emitir o rotar un token.
///
/// `token` es la única copia en claro que existirá jamás.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedAgentClient {
    pub client: AgentClientSummary,
    pub token: String,
}

/// Cliente autenticado, listo para autorizar una intención.
// Los campos que hoy nadie lee forman parte del modelo que el puente
// devuelve a quien lo integre: recortarlos obligaría a volver a consultar
// la base para reconstruirlos.
#[allow(dead_code, reason = "modelo público del puente de agentes")]
#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    pub id: String,
    pub name: String,
    pub scopes: ScopeSet,
}

/// Da de alta un cliente y emite su token.
pub fn issue(
    connection: &mut Connection,
    input: &NewAgentClient,
    policy: TokenPolicy,
) -> AppResult<IssuedAgentClient> {
    let name = validate_name(&input.name)?;
    let kind = validate_kind(&input.kind)?;
    let scopes = ScopeSet::from_values(&input.scopes)?;
    if scopes.is_empty() {
        return Err(AppError::validation(
            "Concede al menos un ámbito al cliente agente.",
        ));
    }

    let transaction = connection.transaction()?;
    let total: i64 =
        transaction.query_row("SELECT COUNT(*) FROM agent_clients", [], |row| row.get(0))?;
    if total as usize >= MAX_CLIENTS {
        return Err(AppError::validation(format!(
            "No se admiten más de {MAX_CLIENTS} clientes agente."
        )));
    }
    let duplicated: Option<String> = transaction
        .query_row(
            "SELECT id FROM agent_clients WHERE name = ?1 COLLATE NOCASE",
            [&name],
            |row| row.get(0),
        )
        .optional()?;
    if duplicated.is_some() {
        return Err(AppError::validation(
            "Ya existe un cliente agente con ese nombre.",
        ));
    }

    let MintedToken {
        client_id,
        plaintext,
        hash,
    } = token::mint(policy);
    transaction.execute(
        "INSERT INTO agent_clients(id, name, kind, token_hash, scopes_json, enabled)
         VALUES (?1, ?2, ?3, ?4, ?5, 1)",
        params![&client_id, &name, &kind, &hash, scopes.to_json()],
    )?;
    let client = read_summary(&transaction, &client_id)?;
    transaction.commit()?;

    Ok(IssuedAgentClient {
        client,
        token: plaintext,
    })
}

/// Rota el token de un cliente existente e invalida el anterior.
/// Retira el cliente que ocupa un nombre, si lo hay.
///
/// # Por qué existe
///
/// El enlace automático da de alta un cliente por agente detectado, con el
/// nombre del agente. Si el registro local del enlace se pierde —una copia
/// restaurada, un borrado del archivo de estado— el nombre sigue ocupado y el
/// alta falla con «ya existe un cliente con ese nombre». Ese error abortaba la
/// reconciliación entera y la aplicación se quedaba **sin agente para siempre**,
/// repitiendo el mismo fallo en cada arranque.
///
/// Retirar el anterior es correcto porque el testigo viejo ya no lo tiene
/// nadie: si se hubiera conservado, el enlace estaría vigente y no se estaría
/// rehaciendo. Devuelve el identificador retirado, para poder decirlo.
pub fn revoke_by_name(connection: &mut Connection, name: &str) -> AppResult<Option<String>> {
    let existing: Option<String> = connection
        .query_row(
            "SELECT id FROM agent_clients WHERE name = ?1 COLLATE NOCASE",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    let Some(client_id) = existing else {
        return Ok(None);
    };
    revoke(connection, &client_id)?;
    Ok(Some(client_id))
}

pub fn rotate(
    connection: &mut Connection,
    client_id: &str,
    policy: TokenPolicy,
) -> AppResult<IssuedAgentClient> {
    let transaction = connection.transaction()?;
    ensure_exists(&transaction, client_id)?;
    let MintedToken {
        plaintext, hash, ..
    } = token::mint_for_client(client_id, policy);
    transaction.execute(
        "UPDATE agent_clients
            SET token_hash = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        params![client_id, &hash],
    )?;
    let client = read_summary(&transaction, client_id)?;
    transaction.commit()?;
    Ok(IssuedAgentClient {
        client,
        token: plaintext,
    })
}

/// Cambia los ámbitos concedidos a un cliente.
pub fn set_scopes(
    connection: &mut Connection,
    client_id: &str,
    scopes: &[String],
) -> AppResult<AgentClientSummary> {
    let scopes = ScopeSet::from_values(scopes)?;
    let transaction = connection.transaction()?;
    ensure_exists(&transaction, client_id)?;
    transaction.execute(
        "UPDATE agent_clients
            SET scopes_json = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        params![client_id, scopes.to_json()],
    )?;
    let client = read_summary(&transaction, client_id)?;
    transaction.commit()?;
    Ok(client)
}

/// Activa o desactiva un cliente sin borrar su historial de auditoría.
pub fn set_enabled(
    connection: &mut Connection,
    client_id: &str,
    enabled: bool,
) -> AppResult<AgentClientSummary> {
    let transaction = connection.transaction()?;
    ensure_exists(&transaction, client_id)?;
    transaction.execute(
        "UPDATE agent_clients
            SET enabled = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        params![client_id, i64::from(enabled)],
    )?;
    let client = read_summary(&transaction, client_id)?;
    transaction.commit()?;
    Ok(client)
}

/// Revoca un cliente. La auditoría sobrevive porque la clave foránea es
/// `ON DELETE SET NULL`.
pub fn revoke(connection: &mut Connection, client_id: &str) -> AppResult<()> {
    let removed = connection.execute("DELETE FROM agent_clients WHERE id = ?1", [client_id])?;
    if removed == 0 {
        return Err(AppError::not_found("El cliente agente ya no existe."));
    }
    Ok(())
}

/// Lista los clientes registrados sin exponer resúmenes de tokens.
pub fn list(connection: &Connection) -> AppResult<Vec<AgentClientSummary>> {
    let mut statement = connection.prepare(
        "SELECT id, name, kind, scopes_json, enabled, last_seen_at, created_at, updated_at
           FROM agent_clients
          ORDER BY name COLLATE NOCASE ASC, id ASC",
    )?;
    let rows = statement.query_map([], map_summary)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Autentica un token y devuelve el cliente con sus ámbitos.
///
/// Rechaza por el mismo motivo un token con formato inválido, un cliente
/// inexistente, un cliente desactivado y un secreto equivocado: el agente no
/// debe poder distinguir esos casos.
pub fn authenticate(connection: &Connection, presented: &str) -> AppResult<AuthenticatedClient> {
    let parsed = token::parse(presented)?;
    let row: Option<(String, String, String, i64)> = connection
        .query_row(
            "SELECT name, token_hash, scopes_json, enabled FROM agent_clients WHERE id = ?1",
            [&parsed.client_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((name, stored_hash, scopes_json, enabled)) = row else {
        return Err(token::invalid_token());
    };
    if !token::verify(&parsed.secret_hex, &stored_hash) || enabled == 0 {
        return Err(token::invalid_token());
    }
    Ok(AuthenticatedClient {
        id: parsed.client_id,
        name,
        scopes: ScopeSet::from_json(&scopes_json),
    })
}

/// Marca la última actividad de un cliente.
pub fn touch(connection: &Connection, client_id: &str) -> AppResult<()> {
    connection.execute(
        "UPDATE agent_clients
            SET last_seen_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        [client_id],
    )?;
    Ok(())
}

fn ensure_exists(connection: &Connection, client_id: &str) -> AppResult<()> {
    connection
        .query_row(
            "SELECT 1 FROM agent_clients WHERE id = ?1",
            [client_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El cliente agente ya no existe."))
}

fn read_summary(connection: &Connection, client_id: &str) -> AppResult<AgentClientSummary> {
    connection
        .query_row(
            "SELECT id, name, kind, scopes_json, enabled, last_seen_at, created_at, updated_at
               FROM agent_clients
              WHERE id = ?1",
            [client_id],
            map_summary,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El cliente agente ya no existe."))
}

fn map_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentClientSummary> {
    Ok(AgentClientSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        scopes: ScopeSet::from_json(&row.get::<_, String>(3)?).values(),
        enabled: row.get::<_, i64>(4)? == 1,
        last_seen_at: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn validate_name(value: &str) -> AppResult<String> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > MAX_NAME_LENGTH {
        return Err(AppError::validation(format!(
            "El nombre del cliente agente debe tener entre 1 y {MAX_NAME_LENGTH} caracteres."
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(AppError::validation(
            "El nombre del cliente agente no admite caracteres de control.",
        ));
    }
    Ok(value.to_string())
}

fn validate_kind(value: &str) -> AppResult<String> {
    let value = value.trim();
    if CLIENT_KINDS.contains(&value) {
        return Ok(value.to_string());
    }
    Err(AppError::validation(format!(
        "El tipo de cliente debe ser uno de: {}.",
        CLIENT_KINDS.join(", ")
    )))
}
