use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

const MAX_BATCH_GAMES: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDropInput {
    pub app_ids: Vec<u32>,
    pub target: LibraryDropTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LibraryDropTarget {
    Status {
        id: String,
    },
    Collection {
        id: String,
        before_app_id: Option<u32>,
    },
    Manual {
        before_app_id: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StatusPlacement {
    pub app_id: u32,
    pub status_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LibraryDropReceipt {
    Status {
        operation_id: String,
        target_id: String,
        app_ids: Vec<u32>,
        previous: Vec<StatusPlacement>,
        activity_ids: Vec<String>,
    },
    Collection {
        operation_id: String,
        target_id: String,
        app_ids: Vec<u32>,
        before_app_id: Option<u32>,
        previous_order: Vec<u32>,
        applied_order: Vec<u32>,
    },
    Manual {
        operation_id: String,
        app_ids: Vec<u32>,
        before_app_id: u32,
        previous_order: Vec<u32>,
        applied_order: Vec<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryDropResult {
    pub moved: usize,
    pub receipt: LibraryDropReceipt,
}

pub fn apply_drop(
    connection: &mut Connection,
    input: &LibraryDropInput,
) -> AppResult<LibraryDropResult> {
    validate_app_ids(&input.app_ids)?;
    match &input.target {
        LibraryDropTarget::Status { .. } => {
            apply_status(connection, &input.app_ids, target_id(&input.target)?)
        }
        LibraryDropTarget::Collection { before_app_id, .. } => apply_collection(
            connection,
            &input.app_ids,
            target_id(&input.target)?,
            *before_app_id,
        ),
        LibraryDropTarget::Manual { before_app_id } => {
            apply_manual(connection, &input.app_ids, *before_app_id)
        }
    }
}

pub fn undo_drop(connection: &mut Connection, receipt: &LibraryDropReceipt) -> AppResult<usize> {
    match receipt {
        LibraryDropReceipt::Status {
            operation_id,
            target_id,
            app_ids,
            previous,
            activity_ids,
        } => undo_status(
            connection,
            operation_id,
            target_id,
            app_ids,
            previous,
            activity_ids,
        ),
        LibraryDropReceipt::Collection {
            operation_id,
            target_id,
            app_ids,
            before_app_id,
            previous_order,
            applied_order,
        } => undo_collection(
            connection,
            operation_id,
            target_id,
            app_ids,
            *before_app_id,
            previous_order,
            applied_order,
        ),
        LibraryDropReceipt::Manual {
            operation_id,
            app_ids,
            before_app_id,
            previous_order,
            applied_order,
        } => undo_manual(
            connection,
            operation_id,
            app_ids,
            *before_app_id,
            previous_order,
            applied_order,
        ),
    }
}

fn apply_status(
    connection: &mut Connection,
    app_ids: &[u32],
    target_id: &str,
) -> AppResult<LibraryDropResult> {
    let transaction = connection.transaction()?;
    ensure_status_exists(&transaction, target_id)?;
    let previous = status_placements(&transaction, app_ids)?;
    let operation_id = Uuid::new_v4().to_string();
    let activity_ids = app_ids
        .iter()
        .map(|_| Uuid::new_v4().to_string())
        .collect::<Vec<_>>();

    let mut update = transaction.prepare_cached(
        "UPDATE game_personal
            SET status_id = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
    )?;
    let mut activity = transaction.prepare_cached(
        "INSERT INTO activity(id, kind, app_id, message)
         VALUES (?1, 'personal_update', ?2, ?3)",
    )?;
    for (app_id, activity_id) in app_ids.iter().zip(&activity_ids) {
        if update.execute(params![app_id, target_id])? != 1 {
            return Err(AppError::not_found(
                "No se pudo mover toda la selección al estado.",
            ));
        }
        activity.execute(params![
            activity_id,
            app_id,
            format!("Se movió el juego mediante el lote {operation_id}.")
        ])?;
    }
    drop(update);
    drop(activity);
    transaction.commit()?;

    Ok(LibraryDropResult {
        moved: app_ids.len(),
        receipt: LibraryDropReceipt::Status {
            operation_id,
            target_id: target_id.to_string(),
            app_ids: app_ids.to_vec(),
            previous,
            activity_ids,
        },
    })
}

fn apply_collection(
    connection: &mut Connection,
    app_ids: &[u32],
    target_id: &str,
    before_app_id: Option<u32>,
) -> AppResult<LibraryDropResult> {
    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, target_id)?;
    ensure_games_exist(&transaction, app_ids)?;
    let previous_order = collection_order(&transaction, target_id)?;
    if before_app_id.is_some_and(|anchor| app_ids.contains(&anchor)) {
        return Err(AppError::validation(
            "Suelta la selección antes de otro juego de la colección.",
        ));
    }
    let applied_order = build_collection_order(&previous_order, app_ids, before_app_id)?;
    write_collection_order(&transaction, target_id, &applied_order)?;
    transaction.commit()?;

    Ok(LibraryDropResult {
        moved: app_ids.len(),
        receipt: LibraryDropReceipt::Collection {
            operation_id: Uuid::new_v4().to_string(),
            target_id: target_id.to_string(),
            app_ids: app_ids.to_vec(),
            before_app_id,
            previous_order,
            applied_order,
        },
    })
}

fn apply_manual(
    connection: &mut Connection,
    app_ids: &[u32],
    before_app_id: u32,
) -> AppResult<LibraryDropResult> {
    let transaction = connection.transaction()?;
    ensure_games_exist(&transaction, app_ids)?;
    let previous_order = manual_order(&transaction)?;
    let applied_order = build_collection_order(&previous_order, app_ids, Some(before_app_id))?;
    write_manual_order(&transaction, &applied_order)?;
    transaction.commit()?;
    Ok(LibraryDropResult {
        moved: app_ids.len(),
        receipt: LibraryDropReceipt::Manual {
            operation_id: Uuid::new_v4().to_string(),
            app_ids: app_ids.to_vec(),
            before_app_id,
            previous_order,
            applied_order,
        },
    })
}

fn undo_status(
    connection: &mut Connection,
    operation_id: &str,
    target_id: &str,
    app_ids: &[u32],
    previous: &[StatusPlacement],
    activity_ids: &[String],
) -> AppResult<usize> {
    validate_operation_id(operation_id)?;
    validate_app_ids(app_ids)?;
    if previous.len() != app_ids.len() || activity_ids.len() != app_ids.len() {
        return Err(invalid_receipt());
    }
    let previous_ids = previous.iter().map(|item| item.app_id).collect::<Vec<_>>();
    if previous_ids != app_ids || activity_ids.iter().any(|id| Uuid::parse_str(id).is_err()) {
        return Err(invalid_receipt());
    }

    let transaction = connection.transaction()?;
    ensure_status_exists(&transaction, target_id)?;
    for placement in previous {
        ensure_status_exists(&transaction, &placement.status_id)?;
        let current_status: String = transaction
            .query_row(
                "SELECT status_id FROM game_personal WHERE app_id = ?1",
                [placement.app_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(stale_receipt)?;
        if current_status != target_id {
            return Err(stale_receipt());
        }
    }
    for (app_id, activity_id) in app_ids.iter().zip(activity_ids) {
        let latest: Option<String> = transaction
            .query_row(
                "SELECT id FROM activity WHERE app_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                [app_id],
                |row| row.get(0),
            )
            .optional()?;
        if latest.as_deref() != Some(activity_id) {
            return Err(stale_receipt());
        }
    }

    let mut update = transaction.prepare_cached(
        "UPDATE game_personal
            SET status_id = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
    )?;
    let mut activity = transaction.prepare_cached(
        "INSERT INTO activity(id, kind, app_id, message)
         VALUES (?1, 'personal_update', ?2, ?3)",
    )?;
    for placement in previous {
        update.execute(params![placement.app_id, placement.status_id])?;
        activity.execute(params![
            Uuid::new_v4().to_string(),
            placement.app_id,
            format!("Se deshizo el lote {operation_id}.")
        ])?;
    }
    drop(update);
    drop(activity);
    transaction.commit()?;
    Ok(app_ids.len())
}

fn undo_collection(
    connection: &mut Connection,
    operation_id: &str,
    target_id: &str,
    app_ids: &[u32],
    before_app_id: Option<u32>,
    previous_order: &[u32],
    applied_order: &[u32],
) -> AppResult<usize> {
    validate_operation_id(operation_id)?;
    validate_app_ids(app_ids)?;
    validate_order(previous_order)?;
    validate_order(applied_order)?;
    let expected = build_collection_order(previous_order, app_ids, before_app_id)?;
    if expected != applied_order {
        return Err(invalid_receipt());
    }

    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, target_id)?;
    if collection_order(&transaction, target_id)? != applied_order {
        return Err(stale_receipt());
    }
    ensure_games_exist(&transaction, previous_order)?;
    write_collection_order(&transaction, target_id, previous_order)?;
    transaction.commit()?;
    Ok(app_ids.len())
}

fn undo_manual(
    connection: &mut Connection,
    operation_id: &str,
    app_ids: &[u32],
    before_app_id: u32,
    previous_order: &[u32],
    applied_order: &[u32],
) -> AppResult<usize> {
    validate_operation_id(operation_id)?;
    validate_app_ids(app_ids)?;
    validate_order(previous_order)?;
    validate_order(applied_order)?;
    if build_collection_order(previous_order, app_ids, Some(before_app_id))? != applied_order {
        return Err(invalid_receipt());
    }
    let transaction = connection.transaction()?;
    if manual_order(&transaction)? != applied_order {
        return Err(stale_receipt());
    }
    write_manual_order(&transaction, previous_order)?;
    transaction.commit()?;
    Ok(app_ids.len())
}

fn build_collection_order(
    previous_order: &[u32],
    app_ids: &[u32],
    before_app_id: Option<u32>,
) -> AppResult<Vec<u32>> {
    if before_app_id.is_some_and(|anchor| app_ids.contains(&anchor)) {
        return Err(invalid_receipt());
    }
    let selected = app_ids.iter().copied().collect::<HashSet<_>>();
    let mut next = previous_order
        .iter()
        .copied()
        .filter(|app_id| !selected.contains(app_id))
        .collect::<Vec<_>>();
    let insert_at = if let Some(anchor) = before_app_id {
        next.iter()
            .position(|app_id| *app_id == anchor)
            .ok_or_else(|| {
                AppError::not_found("El juego de referencia ya no está en la colección.")
            })?
    } else {
        next.len()
    };
    next.splice(insert_at..insert_at, app_ids.iter().copied());
    Ok(next)
}

fn status_placements(connection: &Connection, app_ids: &[u32]) -> AppResult<Vec<StatusPlacement>> {
    let mut statement =
        connection.prepare_cached("SELECT status_id FROM game_personal WHERE app_id = ?1")?;
    app_ids
        .iter()
        .map(|app_id| {
            statement
                .query_row([app_id], |row| row.get::<_, String>(0))
                .optional()?
                .map(|status_id| StatusPlacement {
                    app_id: *app_id,
                    status_id,
                })
                .ok_or_else(|| {
                    AppError::not_found("Uno o más juegos ya no están en la biblioteca.")
                })
        })
        .collect()
}

fn collection_order(connection: &Connection, collection_id: &str) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM collection_games
          WHERE collection_id = ?1
          ORDER BY position ASC, app_id ASC",
    )?;
    Ok(statement
        .query_map([collection_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn manual_order(connection: &Connection) -> AppResult<Vec<u32>> {
    let mut statement = connection
        .prepare("SELECT app_id FROM game_personal ORDER BY manual_position ASC, app_id ASC")?;
    Ok(statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn write_manual_order(connection: &Connection, app_ids: &[u32]) -> AppResult<()> {
    let mut update = connection.prepare_cached(
        "UPDATE game_personal
            SET manual_position = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
    )?;
    for (position, app_id) in app_ids.iter().enumerate() {
        if update.execute(params![app_id, position as i64])? != 1 {
            return Err(AppError::not_found(
                "Uno o más juegos ya no están en la biblioteca.",
            ));
        }
    }
    Ok(())
}

fn write_collection_order(
    connection: &Connection,
    collection_id: &str,
    app_ids: &[u32],
) -> AppResult<()> {
    connection.execute(
        "DELETE FROM collection_games WHERE collection_id = ?1",
        [collection_id],
    )?;
    let mut insert = connection.prepare_cached(
        "INSERT INTO collection_games(collection_id, app_id, position)
         VALUES (?1, ?2, ?3)",
    )?;
    for (position, app_id) in app_ids.iter().enumerate() {
        insert.execute(params![collection_id, app_id, position as i64])?;
    }
    Ok(())
}

fn ensure_status_exists(connection: &Connection, status_id: &str) -> AppResult<()> {
    if status_id.trim().is_empty() || status_id.len() > 128 {
        return Err(AppError::validation("El estado seleccionado no es válido."));
    }
    connection
        .query_row("SELECT 1 FROM statuses WHERE id = ?1", [status_id], |_| {
            Ok(())
        })
        .optional()?
        .ok_or_else(|| AppError::not_found("El estado seleccionado ya no existe."))
}

fn ensure_manual_collection(connection: &Connection, collection_id: &str) -> AppResult<()> {
    let kind = connection
        .query_row(
            "SELECT kind FROM collections WHERE id = ?1",
            [collection_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La colección ya no existe."))?;
    if kind != "manual" {
        return Err(AppError::validation(
            "No puedes soltar juegos en una colección inteligente; cambia sus reglas.",
        ));
    }
    Ok(())
}

fn ensure_games_exist(connection: &Connection, app_ids: &[u32]) -> AppResult<()> {
    for app_id in app_ids {
        if connection
            .query_row(
                "SELECT 1 FROM games WHERE app_id = ?1",
                [app_id],
                |_| Ok(()),
            )
            .optional()?
            .is_none()
        {
            return Err(AppError::not_found(format!(
                "El juego {app_id} ya no está en la biblioteca."
            )));
        }
    }
    Ok(())
}

fn validate_app_ids(app_ids: &[u32]) -> AppResult<()> {
    if app_ids.is_empty() {
        return Err(AppError::validation(
            "Selecciona al menos un juego para mover.",
        ));
    }
    if app_ids.len() > MAX_BATCH_GAMES {
        return Err(AppError::validation("La selección es demasiado grande."));
    }
    let unique = app_ids.iter().copied().collect::<HashSet<_>>();
    if unique.len() != app_ids.len() || unique.contains(&0) {
        return Err(AppError::validation(
            "La selección contiene juegos no válidos o duplicados.",
        ));
    }
    Ok(())
}

fn validate_order(app_ids: &[u32]) -> AppResult<()> {
    if app_ids.len() > MAX_BATCH_GAMES {
        return Err(invalid_receipt());
    }
    let unique = app_ids.iter().copied().collect::<HashSet<_>>();
    if unique.len() != app_ids.len() || unique.contains(&0) {
        return Err(invalid_receipt());
    }
    Ok(())
}

fn target_id(target: &LibraryDropTarget) -> AppResult<&str> {
    let id = match target {
        LibraryDropTarget::Status { id } | LibraryDropTarget::Collection { id, .. } => id.trim(),
        LibraryDropTarget::Manual { .. } => "",
    };
    if id.is_empty() || id.len() > 128 {
        return Err(AppError::validation(
            "El destino seleccionado no es válido.",
        ));
    }
    Ok(id)
}

fn validate_operation_id(operation_id: &str) -> AppResult<()> {
    Uuid::parse_str(operation_id)
        .map(|_| ())
        .map_err(|_| invalid_receipt())
}

fn invalid_receipt() -> AppError {
    AppError::validation("La operación que intentas deshacer no es válida.")
}

fn stale_receipt() -> AppError {
    AppError::new(
        "stale_undo",
        "La organización cambió después del lote; no se ha deshecho nada.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{library, migrations, seed_defaults};
    use crate::models::GameListRequest;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir memoria");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar");
        connection
    }

    fn insert_game(connection: &Connection, app_id: u32, title: &str) {
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                params![app_id, title],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar datos personales");
    }

    fn insert_collection(connection: &Connection, id: &str, kind: &str) {
        connection
            .execute(
                "INSERT INTO collections(id, name, color, icon, kind, position)
                 VALUES (?1, ?1, '#66c0f4', 'folder', ?2, 0)",
                params![id, kind],
            )
            .expect("insertar colección");
    }

    fn status(connection: &Connection, app_id: u32) -> String {
        connection
            .query_row(
                "SELECT status_id FROM game_personal WHERE app_id = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("leer estado")
    }

    #[test]
    fn status_batch_is_atomic_and_undo_restores_each_previous_status() {
        let mut connection = database();
        insert_game(&connection, 10, "Uno");
        insert_game(&connection, 20, "Dos");
        connection
            .execute(
                "UPDATE game_personal SET status_id = 'backlog' WHERE app_id = 20",
                [],
            )
            .expect("preparar estado");

        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![10, 20],
                target: LibraryDropTarget::Status {
                    id: "playing".to_string(),
                },
            },
        )
        .expect("mover lote");
        assert_eq!(status(&connection, 10), "playing");
        assert_eq!(status(&connection, 20), "playing");

        assert_eq!(
            undo_drop(&mut connection, &result.receipt).expect("deshacer"),
            2
        );
        assert_eq!(status(&connection, 10), "unclassified");
        assert_eq!(status(&connection, 20), "backlog");
    }

    #[test]
    fn collection_batch_preserves_order_and_undo_restores_exact_snapshot() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        insert_collection(&connection, "manual", "manual");
        write_collection_order(&connection, "manual", &[20]).expect("orden inicial");

        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![10, 30],
                target: LibraryDropTarget::Collection {
                    id: "manual".to_string(),
                    before_app_id: None,
                },
            },
        )
        .expect("añadir lote");
        assert_eq!(
            collection_order(&connection, "manual").unwrap(),
            vec![20, 10, 30]
        );

        undo_drop(&mut connection, &result.receipt).expect("deshacer");
        assert_eq!(collection_order(&connection, "manual").unwrap(), vec![20]);
    }

    #[test]
    fn collection_batch_can_insert_or_reorder_before_a_stable_game_and_undo_exactly() {
        let mut connection = database();
        for app_id in [10, 20, 30, 40] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        insert_collection(&connection, "manual", "manual");
        write_collection_order(&connection, "manual", &[10, 20, 30]).expect("orden inicial");

        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![30, 40],
                target: LibraryDropTarget::Collection {
                    id: "manual".to_string(),
                    before_app_id: Some(20),
                },
            },
        )
        .expect("insertar antes del ancla");

        assert_eq!(
            collection_order(&connection, "manual").unwrap(),
            vec![10, 30, 40, 20]
        );
        undo_drop(&mut connection, &result.receipt).expect("deshacer orden");
        assert_eq!(
            collection_order(&connection, "manual").unwrap(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn smart_collection_is_rejected_before_any_write() {
        let mut connection = database();
        insert_game(&connection, 10, "Uno");
        insert_collection(&connection, "smart", "smart");

        let error = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![10],
                target: LibraryDropTarget::Collection {
                    id: "smart".to_string(),
                    before_app_id: None,
                },
            },
        )
        .expect_err("rechazar inteligente");

        assert_eq!(error.code, "validation");
        assert!(collection_order(&connection, "smart").unwrap().is_empty());
    }

    #[test]
    fn stale_status_receipt_does_not_overwrite_a_later_change() {
        let mut connection = database();
        insert_game(&connection, 10, "Uno");
        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![10],
                target: LibraryDropTarget::Status {
                    id: "playing".to_string(),
                },
            },
        )
        .expect("mover");
        connection
            .execute(
                "UPDATE game_personal SET status_id = 'backlog' WHERE app_id = 10",
                [],
            )
            .expect("cambio posterior");

        let error = undo_drop(&mut connection, &result.receipt).expect_err("rechazar stale");
        assert_eq!(error.code, "stale_undo");
        assert_eq!(status(&connection, 10), "backlog");
    }

    #[test]
    fn stale_collection_receipt_does_not_overwrite_a_later_reorder() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        insert_collection(&connection, "manual", "manual");
        write_collection_order(&connection, "manual", &[10, 20]).expect("orden inicial");
        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![30],
                target: LibraryDropTarget::Collection {
                    id: "manual".to_string(),
                    before_app_id: Some(20),
                },
            },
        )
        .expect("mover");
        write_collection_order(&connection, "manual", &[30, 10, 20])
            .expect("reordenación posterior");

        let error = undo_drop(&mut connection, &result.receipt).expect_err("rechazar stale");
        assert_eq!(error.code, "stale_undo");
        assert_eq!(
            collection_order(&connection, "manual").unwrap(),
            vec![30, 10, 20]
        );
    }

    #[test]
    fn manual_collection_query_uses_its_persisted_game_order() {
        let connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        insert_collection(&connection, "manual", "manual");
        write_collection_order(&connection, "manual", &[30, 10, 20]).expect("ordenar");

        let page = library::list_games(
            &connection,
            &GameListRequest {
                collection_id: Some("manual".to_string()),
                sort: Some("manual".to_string()),
                limit: Some(20),
                ..GameListRequest::default()
            },
            None,
        )
        .expect("listar colección");

        assert_eq!(
            page.items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![30, 10, 20]
        );
    }

    #[test]
    fn global_manual_query_keeps_the_drag_order_even_when_games_are_pinned_or_prioritized() {
        let connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        write_manual_order(&connection, &[10, 20, 30]).expect("ordenar biblioteca");
        connection
            .execute(
                "UPDATE game_personal SET pinned = 1, priority = 5 WHERE app_id = 30",
                [],
            )
            .expect("destacar juego posterior");
        connection
            .execute(
                "UPDATE game_personal SET priority = 4 WHERE app_id = 20",
                [],
            )
            .expect("priorizar juego intermedio");

        let page = library::list_games(
            &connection,
            &GameListRequest {
                sort: Some("manual".to_string()),
                limit: Some(20),
                ..GameListRequest::default()
            },
            None,
        )
        .expect("listar biblioteca en orden manual");

        assert_eq!(
            page.items
                .iter()
                .map(|game| game.app_id)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn global_manual_order_moves_a_batch_before_an_anchor_and_undoes_exactly() {
        let mut connection = database();
        for app_id in [10, 20, 30, 40] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        write_manual_order(&connection, &[10, 20, 30, 40]).expect("orden inicial");

        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![30, 40],
                target: LibraryDropTarget::Manual { before_app_id: 20 },
            },
        )
        .expect("reordenar biblioteca");
        assert_eq!(manual_order(&connection).unwrap(), vec![10, 30, 40, 20]);

        undo_drop(&mut connection, &result.receipt).expect("deshacer");
        assert_eq!(manual_order(&connection).unwrap(), vec![10, 20, 30, 40]);
    }

    #[test]
    fn global_manual_order_rejects_a_stale_undo_without_partial_writes() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        write_manual_order(&connection, &[10, 20, 30]).expect("orden inicial");
        let result = apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![30],
                target: LibraryDropTarget::Manual { before_app_id: 20 },
            },
        )
        .expect("reordenar");
        write_manual_order(&connection, &[30, 10, 20]).expect("cambio posterior");

        let error = undo_drop(&mut connection, &result.receipt).expect_err("rechazar stale");
        assert_eq!(error.code, "stale_undo");
        assert_eq!(manual_order(&connection).unwrap(), vec![30, 10, 20]);
    }

    #[test]
    fn global_manual_order_uses_the_full_library_when_the_anchor_is_on_a_later_page() {
        let mut connection = database();
        let initial = (1..=300).collect::<Vec<_>>();
        for app_id in &initial {
            insert_game(&connection, *app_id, &format!("Juego {app_id}"));
        }
        write_manual_order(&connection, &initial).expect("orden inicial");

        apply_drop(
            &mut connection,
            &LibraryDropInput {
                app_ids: vec![1, 2],
                target: LibraryDropTarget::Manual { before_app_id: 250 },
            },
        )
        .expect("reordenar mediante un ancla de otra página");

        let applied = manual_order(&connection).expect("leer orden completo");
        assert_eq!(applied.len(), 300);
        assert_eq!(&applied[246..251], &[249, 1, 2, 250, 251]);
        assert_eq!(applied.iter().copied().collect::<HashSet<_>>().len(), 300);
    }
}
