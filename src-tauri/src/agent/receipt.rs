//! Recibos de deshacer para las acciones del agente.
//!
//! Reutiliza el enfoque de [`crate::db::library_dnd`]: al aplicar se guarda el
//! estado anterior **y** el estado aplicado; al deshacer se comprueba primero
//! que lo que hay en la base sigue siendo exactamente lo que se aplicó. Si algo
//! cambió entre medias —la persona editó el juego, otra automatización pasó por
//! encima— el deshacer se rechaza como caducado en lugar de sobrescribir
//! trabajo posterior.
//!
//! La diferencia con `library_dnd` es el alcance: allí el recibo cubre un
//! reordenamiento; aquí cubre además la ficha personal completa, la creación de
//! contenedores y la pertenencia a colecciones y listas.

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

/// Subconjunto mutable de `game_personal` que el agente puede tocar.
///
/// Se guarda entero, no campo a campo: comparar la fila completa hace que
/// cualquier edición ajena invalide el deshacer, que es el lado seguro.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonalSnapshot {
    pub app_id: u32,
    pub status_id: String,
    pub progress: u8,
    pub priority: u8,
    pub pinned: bool,
    pub tracking: bool,
    pub rating: Option<u8>,
    pub next_action: Option<String>,
    pub checkpoint: Option<String>,
    pub notes: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub abandoned_at: Option<String>,
}

/// Colocación de un juego en el planificador.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlannerSnapshot {
    pub column_id: String,
    pub position: i64,
    pub target_date: Option<String>,
    pub estimated_minutes: Option<u32>,
}

/// Recibo de una acción aplicada.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum UndoReceipt {
    /// Cambios sobre la ficha personal de uno o varios juegos.
    PersonalFields {
        operation_id: String,
        previous: Vec<PersonalSnapshot>,
        applied: Vec<PersonalSnapshot>,
    },
    /// Sesión registrada, con el progreso que llevaba el juego antes.
    ///
    /// Las instantáneas van en `Box` para que esta variante no infle el tamaño
    /// de todo el enumerado; `serde` las serializa igual que si fueran planas.
    Session {
        operation_id: String,
        session_id: String,
        activity_id: String,
        previous: Option<Box<PersonalSnapshot>>,
        applied: Option<Box<PersonalSnapshot>>,
    },
    /// Contenido completo de una colección manual.
    CollectionMembers {
        operation_id: String,
        collection_id: String,
        previous_order: Vec<u32>,
        applied_order: Vec<u32>,
    },
    /// Colección creada por el agente.
    CollectionCreated {
        operation_id: String,
        collection_id: String,
        members: Vec<u32>,
    },
    /// Lista curada creada por el agente.
    CuratedListCreated {
        operation_id: String,
        list_id: String,
    },
    /// Juegos añadidos a una lista curada.
    CuratedMembers {
        operation_id: String,
        list_id: String,
        added_app_ids: Vec<u32>,
    },
    /// Colocación en el planificador.
    PlannerPlacement {
        operation_id: String,
        app_id: u32,
        previous: Option<PlannerSnapshot>,
        applied: PlannerSnapshot,
    },
    /// Aviso programado.
    Reminder {
        operation_id: String,
        reminder_id: String,
    },
}

/// Lee la ficha personal de un juego.
pub fn read_personal(connection: &Connection, app_id: u32) -> AppResult<PersonalSnapshot> {
    connection
        .query_row(
            "SELECT app_id, status_id, progress, priority, pinned, tracking, rating,
                    next_action, checkpoint, notes, started_at, completed_at, abandoned_at
               FROM game_personal
              WHERE app_id = ?1",
            [app_id],
            map_personal,
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(format!(
                "El juego {app_id} todavía no tiene ficha personal en la biblioteca."
            ))
        })
}

fn map_personal(row: &Row<'_>) -> rusqlite::Result<PersonalSnapshot> {
    Ok(PersonalSnapshot {
        app_id: row.get(0)?,
        status_id: row.get(1)?,
        progress: row.get::<_, i64>(2)? as u8,
        priority: row.get::<_, i64>(3)? as u8,
        pinned: row.get::<_, i64>(4)? == 1,
        tracking: row.get::<_, i64>(5)? == 1,
        rating: row.get::<_, Option<i64>>(6)?.map(|value| value as u8),
        next_action: row.get(7)?,
        checkpoint: row.get(8)?,
        notes: row.get(9)?,
        started_at: row.get(10)?,
        completed_at: row.get(11)?,
        abandoned_at: row.get(12)?,
    })
}

/// Escribe una ficha personal completa.
pub fn write_personal(transaction: &Transaction<'_>, snapshot: &PersonalSnapshot) -> AppResult<()> {
    let changed = transaction.execute(
        "UPDATE game_personal
            SET status_id = ?2,
                progress = ?3,
                priority = ?4,
                pinned = ?5,
                tracking = ?6,
                rating = ?7,
                next_action = ?8,
                checkpoint = ?9,
                notes = ?10,
                started_at = ?11,
                completed_at = ?12,
                abandoned_at = ?13,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
        params![
            snapshot.app_id,
            snapshot.status_id,
            i64::from(snapshot.progress),
            i64::from(snapshot.priority),
            i64::from(snapshot.pinned),
            i64::from(snapshot.tracking),
            snapshot.rating.map(i64::from),
            snapshot.next_action,
            snapshot.checkpoint,
            snapshot.notes,
            snapshot.started_at,
            snapshot.completed_at,
            snapshot.abandoned_at,
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found(
            "El juego ya no está en la biblioteca personal.",
        ));
    }
    Ok(())
}

/// Orden actual de una colección manual.
pub fn read_collection_order(connection: &Connection, collection_id: &str) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM collection_games
          WHERE collection_id = ?1
          ORDER BY position ASC, app_id ASC",
    )?;
    let rows = statement.query_map([collection_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Reescribe el contenido de una colección manual.
pub fn write_collection_order(
    transaction: &Transaction<'_>,
    collection_id: &str,
    order: &[u32],
) -> AppResult<()> {
    transaction.execute(
        "DELETE FROM collection_games WHERE collection_id = ?1",
        [collection_id],
    )?;
    let mut insert = transaction.prepare_cached(
        "INSERT INTO collection_games(collection_id, app_id, position) VALUES (?1, ?2, ?3)",
    )?;
    for (position, app_id) in order.iter().enumerate() {
        insert.execute(params![collection_id, app_id, position as i64])?;
    }
    Ok(())
}

/// Colocación actual de un juego en el planificador.
pub fn read_planner(connection: &Connection, app_id: u32) -> AppResult<Option<PlannerSnapshot>> {
    let snapshot = connection
        .query_row(
            "SELECT column_id, position, target_date, estimated_minutes
               FROM planner_items
              WHERE app_id = ?1",
            [app_id],
            |row| {
                Ok(PlannerSnapshot {
                    column_id: row.get(0)?,
                    position: row.get(1)?,
                    target_date: row.get(2)?,
                    estimated_minutes: row.get::<_, Option<i64>>(3)?.map(|value| value as u32),
                })
            },
        )
        .optional()?;
    Ok(snapshot)
}

/// Aplica el recibo al revés. Devuelve el número de elementos restaurados.
pub fn undo(connection: &mut Connection, receipt: &UndoReceipt) -> AppResult<usize> {
    match receipt {
        UndoReceipt::PersonalFields {
            previous, applied, ..
        } => {
            if previous.len() != applied.len() || previous.is_empty() {
                return Err(invalid_receipt());
            }
            let transaction = connection.transaction()?;
            for (before, after) in previous.iter().zip(applied) {
                if before.app_id != after.app_id {
                    return Err(invalid_receipt());
                }
                let current = read_personal(&transaction, after.app_id)?;
                if current != *after {
                    return Err(stale_receipt());
                }
            }
            for before in previous {
                write_personal(&transaction, before)?;
            }
            transaction.commit()?;
            Ok(previous.len())
        }
        UndoReceipt::Session {
            session_id,
            activity_id,
            previous,
            applied,
            ..
        } => {
            let transaction = connection.transaction()?;
            if let (Some(before), Some(after)) = (previous, applied) {
                let current = read_personal(&transaction, after.app_id)?;
                if current != **after {
                    return Err(stale_receipt());
                }
                write_personal(&transaction, before)?;
            }
            let removed =
                transaction.execute("DELETE FROM game_sessions WHERE id = ?1", [session_id])?;
            if removed != 1 {
                return Err(stale_receipt());
            }
            transaction.execute("DELETE FROM activity WHERE id = ?1", [activity_id])?;
            transaction.commit()?;
            Ok(1)
        }
        UndoReceipt::CollectionMembers {
            collection_id,
            previous_order,
            applied_order,
            ..
        } => {
            let transaction = connection.transaction()?;
            if read_collection_order(&transaction, collection_id)? != *applied_order {
                return Err(stale_receipt());
            }
            write_collection_order(&transaction, collection_id, previous_order)?;
            transaction.commit()?;
            Ok(previous_order.len())
        }
        UndoReceipt::CollectionCreated {
            collection_id,
            members,
            ..
        } => {
            let transaction = connection.transaction()?;
            if read_collection_order(&transaction, collection_id)? != *members {
                return Err(stale_receipt());
            }
            let removed =
                transaction.execute("DELETE FROM collections WHERE id = ?1", [collection_id])?;
            if removed != 1 {
                return Err(stale_receipt());
            }
            transaction.commit()?;
            Ok(1)
        }
        UndoReceipt::CuratedListCreated { list_id, .. } => {
            let transaction = connection.transaction()?;
            let items: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM curated_list_items WHERE list_id = ?1",
                [list_id],
                |row| row.get(0),
            )?;
            if items != 0 {
                return Err(stale_receipt());
            }
            let removed =
                transaction.execute("DELETE FROM curated_lists WHERE id = ?1", [list_id])?;
            if removed != 1 {
                return Err(stale_receipt());
            }
            transaction.commit()?;
            Ok(1)
        }
        UndoReceipt::CuratedMembers {
            list_id,
            added_app_ids,
            ..
        } => {
            let transaction = connection.transaction()?;
            let mut removed = 0usize;
            for app_id in added_app_ids {
                let affected = transaction.execute(
                    "DELETE FROM curated_list_items WHERE list_id = ?1 AND app_id = ?2",
                    params![list_id, app_id],
                )?;
                if affected != 1 {
                    return Err(stale_receipt());
                }
                removed += 1;
            }
            normalize_curated_positions(&transaction, list_id)?;
            transaction.commit()?;
            Ok(removed)
        }
        UndoReceipt::PlannerPlacement {
            app_id,
            previous,
            applied,
            ..
        } => {
            let transaction = connection.transaction()?;
            let current = read_planner(&transaction, *app_id)?;
            if current.as_ref() != Some(applied) {
                return Err(stale_receipt());
            }
            transaction.execute("DELETE FROM planner_items WHERE app_id = ?1", [app_id])?;
            if let Some(before) = previous {
                transaction.execute(
                    "INSERT INTO planner_items(
                        column_id, app_id, position, target_date, estimated_minutes
                     ) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        before.column_id,
                        app_id,
                        before.position,
                        before.target_date,
                        before.estimated_minutes.map(i64::from),
                    ],
                )?;
            }
            transaction.commit()?;
            Ok(1)
        }
        UndoReceipt::Reminder { reminder_id, .. } => {
            let removed = connection.execute(
                "DELETE FROM game_reminders WHERE id = ?1 AND completed_at IS NULL",
                [reminder_id],
            )?;
            if removed != 1 {
                return Err(stale_receipt());
            }
            Ok(1)
        }
    }
}

/// Renumera las posiciones de una lista curada tras un borrado.
pub fn normalize_curated_positions(transaction: &Transaction<'_>, list_id: &str) -> AppResult<()> {
    let order = {
        let mut statement = transaction.prepare(
            "SELECT app_id FROM curated_list_items
              WHERE list_id = ?1
              ORDER BY position ASC, app_id ASC",
        )?;
        let rows = statement.query_map([list_id], |row| row.get::<_, u32>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut update = transaction.prepare_cached(
        "UPDATE curated_list_items SET position = ?3 WHERE list_id = ?1 AND app_id = ?2",
    )?;
    for (position, app_id) in order.iter().enumerate() {
        update.execute(params![list_id, app_id, position as i64])?;
    }
    Ok(())
}

pub(crate) fn stale_receipt() -> AppError {
    AppError::new(
        "agent_stale",
        "La acción ya no se puede deshacer porque la biblioteca cambió después de aplicarla.",
    )
}

pub(crate) fn invalid_receipt() -> AppError {
    AppError::new(
        "agent_receipt",
        "El recibo de deshacer no es coherente con la acción registrada.",
    )
}
