use crate::error::{AppError, AppResult};
use crate::models::{
    AppPreferences, CollectionSummary, MovePlannerItemInput, PlannerColumn, PlannerItem,
    PlannerOverview, PlannerSettings, SaveCollectionInput, SavePlannerItemInput, SmartRule,
    StatusDefinition,
};
use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

const MAX_NAME_LENGTH: usize = 80;
const MAX_DESCRIPTION_LENGTH: usize = 1_000;

#[derive(Debug)]
struct GameFacts {
    app_id: u32,
    title: String,
    developer: Option<String>,
    publisher: Option<String>,
    genres: Vec<String>,
    categories: Vec<String>,
    status_id: String,
    installed: bool,
    tracking: bool,
    early_access: bool,
    is_free: bool,
    progress: f64,
    priority: f64,
    rating: Option<f64>,
    playtime_minutes: f64,
    estimated_minutes: Option<f64>,
    target_date: Option<String>,
    release_date: Option<String>,
    last_played_at: Option<String>,
    imported_at: String,
    updated_at: String,
    steam_deck_status: Option<String>,
    achievement_percent: Option<f64>,
    tags: Vec<String>,
}

pub fn list_statuses(connection: &Connection) -> AppResult<Vec<StatusDefinition>> {
    let mut statement = connection.prepare(
        // Las mismas dos exclusiones que aplica `library::list_games` por
        // defecto: los juegos prestados sin confirmar y los archivados no están
        // en la biblioteca, así que tampoco pueden estar en sus recuentos.
        // Cuando el catálogo de Family pasó a tener ficha personal, esta
        // consulta empezó a contarlo y la barra ofrecía mil setecientos juegos
        // que el listado escondía.
        "SELECT s.id, s.name, s.color, s.position, s.built_in,
                COUNT(CASE WHEN g.app_id IS NOT NULL THEN 1 END)
           FROM statuses s
      LEFT JOIN game_personal p ON p.status_id = s.id
      LEFT JOIN games g ON g.app_id = p.app_id
                       AND NOT (g.ownership_source = 'family_shared'
                                AND g.family_availability <> 'confirmed')
                       AND NOT EXISTS (SELECT 1 FROM game_archive a WHERE a.app_id = g.app_id)
       GROUP BY s.id
       ORDER BY s.position ASC, s.name COLLATE NOCASE ASC",
    )?;
    let values = statement
        .query_map([], |row| {
            Ok(StatusDefinition {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                position: row.get(3)?,
                built_in: row.get::<_, i64>(4)? != 0,
                game_count: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(values)
}

pub fn save_status(
    connection: &mut Connection,
    id: Option<&str>,
    name: &str,
    color: &str,
) -> AppResult<StatusDefinition> {
    let name = validate_name(name, "estado")?;
    let color = validate_color(color)?;
    let transaction = connection.transaction()?;

    let status_id = if let Some(id) = id {
        ensure_status_exists(&transaction, id)?;
        id.to_string()
    } else {
        format!("custom-{}", Uuid::new_v4())
    };
    ensure_unique_name(&transaction, "statuses", &name, Some(&status_id))?;

    if id.is_some() {
        transaction.execute(
            "UPDATE statuses
                SET name = ?2, color = ?3,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE id = ?1",
            params![status_id, name, color],
        )?;
    } else {
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM statuses",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO statuses(id, name, color, position, built_in)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![status_id, name, color, position],
        )?;
    }
    transaction.commit()?;
    find_status(connection, &status_id)
}

pub fn delete_status(
    connection: &mut Connection,
    status_id: &str,
    replacement_status_id: &str,
) -> AppResult<()> {
    if status_id == replacement_status_id {
        return Err(AppError::validation(
            "El estado de reemplazo debe ser distinto al que se elimina.",
        ));
    }
    let transaction = connection.transaction()?;
    let built_in = transaction
        .query_row(
            "SELECT built_in FROM statuses WHERE id = ?1",
            [status_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El estado ya no existe."))?
        != 0;
    if built_in {
        return Err(AppError::validation(
            "Los estados integrados de Vindexa no se pueden eliminar.",
        ));
    }
    ensure_status_exists(&transaction, replacement_status_id)?;
    transaction.execute(
        "UPDATE game_personal SET status_id = ?2 WHERE status_id = ?1",
        params![status_id, replacement_status_id],
    )?;
    transaction.execute("DELETE FROM statuses WHERE id = ?1", [status_id])?;
    normalize_positions(&transaction, "statuses")?;
    transaction.commit()?;
    Ok(())
}

pub fn reorder_statuses(connection: &mut Connection, ordered_ids: &[String]) -> AppResult<()> {
    let transaction = connection.transaction()?;
    ensure_exact_id_set(&transaction, "statuses", ordered_ids)?;
    write_positions(&transaction, "statuses", ordered_ids)?;
    transaction.commit()?;
    Ok(())
}

pub fn list_collections(connection: &Connection) -> AppResult<Vec<CollectionSummary>> {
    let mut statement = connection.prepare(
        // Mismo criterio que en los estados: la cifra tiene que coincidir con lo
        // que se encuentra al pulsarla.
        "SELECT c.id, c.name, c.description, c.color, c.icon, c.kind, c.match_mode,
                c.position, COUNT(CASE WHEN g.app_id IS NOT NULL THEN 1 END)
           FROM collections c
      LEFT JOIN collection_games cg ON cg.collection_id = c.id
      LEFT JOIN games g ON g.app_id = cg.app_id
                       AND NOT (g.ownership_source = 'family_shared'
                                AND g.family_availability <> 'confirmed')
                       AND NOT EXISTS (SELECT 1 FROM game_archive a WHERE a.app_id = g.app_id)
       GROUP BY c.id
       ORDER BY c.position ASC, c.name COLLATE NOCASE ASC",
    )?;
    let mut collections = statement
        .query_map([], |row| {
            Ok(CollectionSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                color: row.get(3)?,
                icon: row.get(4)?,
                kind: row.get(5)?,
                match_mode: row.get(6)?,
                position: row.get(7)?,
                game_count: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if collections
        .iter()
        .any(|collection| collection.kind == "smart")
    {
        let facts = load_game_facts(connection)?;
        for collection in &mut collections {
            if collection.kind == "smart" {
                let rules = list_smart_rules(connection, &collection.id)?;
                collection.game_count =
                    evaluate_rules(&facts, &rules, &collection.match_mode).len() as i64;
            }
        }
    }
    Ok(collections)
}

pub fn list_smart_rules(connection: &Connection, collection_id: &str) -> AppResult<Vec<SmartRule>> {
    let mut statement = connection.prepare(
        "SELECT id, group_id, field, operator, value_json, position
           FROM smart_rules
          WHERE collection_id = ?1
          ORDER BY group_id ASC, position ASC, id ASC",
    )?;
    let rows = statement.query_map([collection_id], |row| {
        let raw_value: String = row.get(4)?;
        let value = serde_json::from_str(&raw_value).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                raw_value.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        Ok(SmartRule {
            id: Some(row.get(0)?),
            group_id: row.get(1)?,
            field: row.get(2)?,
            operator: row.get(3)?,
            value,
            position: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn save_collection(
    connection: &mut Connection,
    input: &SaveCollectionInput,
) -> AppResult<CollectionSummary> {
    let name = validate_name(&input.name, "colección")?;
    if input.description.chars().count() > MAX_DESCRIPTION_LENGTH {
        return Err(AppError::validation(format!(
            "La descripción no puede superar {MAX_DESCRIPTION_LENGTH} caracteres."
        )));
    }
    let description = input.description.trim().to_string();
    let color = validate_color(&input.color)?;
    let icon = input.icon.trim();
    if icon.is_empty() || icon.chars().count() > 64 {
        return Err(AppError::validation(
            "El icono de la colección no es válido.",
        ));
    }
    if !matches!(input.kind.as_str(), "manual" | "smart") {
        return Err(AppError::validation(
            "El tipo de colección debe ser manual o inteligente.",
        ));
    }
    if !matches!(input.match_mode.as_str(), "all" | "any") {
        return Err(AppError::validation(
            "El modo de coincidencia debe ser «todas» o «cualquiera».",
        ));
    }
    if input.kind == "manual" && !input.rules.is_empty() {
        return Err(AppError::validation(
            "Una colección manual no puede contener reglas inteligentes.",
        ));
    }
    if input.kind == "smart" {
        if input.rules.is_empty() {
            return Err(AppError::validation(
                "Añade al menos una regla a la colección inteligente.",
            ));
        }
        for rule in &input.rules {
            validate_rule(rule)?;
        }
    }

    let transaction = connection.transaction()?;
    let collection_id = input
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let existing_kind = transaction
        .query_row(
            "SELECT kind FROM collections WHERE id = ?1",
            [&collection_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if input.id.is_some() && existing_kind.is_none() {
        return Err(AppError::not_found("La colección ya no existe."));
    }
    if existing_kind
        .as_deref()
        .is_some_and(|kind| kind != input.kind)
    {
        return Err(AppError::validation(
            "No se puede cambiar el tipo de una colección existente porque podría perder su organización.",
        ));
    }
    ensure_unique_name(&transaction, "collections", &name, Some(&collection_id))?;

    if existing_kind.is_some() {
        transaction.execute(
            "UPDATE collections
                SET name = ?2, description = ?3, color = ?4, icon = ?5,
                    match_mode = ?6,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE id = ?1",
            params![
                collection_id,
                name,
                description,
                color,
                icon,
                input.match_mode
            ],
        )?;
    } else {
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM collections",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO collections(
                id, name, description, color, icon, kind, match_mode, position
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                collection_id,
                name,
                description,
                color,
                icon,
                input.kind,
                input.match_mode,
                position
            ],
        )?;
    }

    if input.kind == "smart" {
        replace_smart_rules(&transaction, &collection_id, &input.rules)?;
    }
    transaction.commit()?;
    find_collection(connection, &collection_id)
}

pub fn delete_collection(connection: &mut Connection, collection_id: &str) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let deleted = transaction.execute("DELETE FROM collections WHERE id = ?1", [collection_id])?;
    if deleted != 1 {
        return Err(AppError::not_found("La colección ya no existe."));
    }
    normalize_positions(&transaction, "collections")?;
    transaction.commit()?;
    Ok(())
}

pub fn reorder_collections(connection: &mut Connection, ordered_ids: &[String]) -> AppResult<()> {
    let transaction = connection.transaction()?;
    ensure_exact_id_set(&transaction, "collections", ordered_ids)?;
    write_positions(&transaction, "collections", ordered_ids)?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
pub fn collection_game_ids(
    connection: &Connection,
    collection_id: &str,
) -> AppResult<HashSet<u32>> {
    if let Some(ids) = smart_collection_game_ids(connection, collection_id)? {
        return Ok(ids);
    }
    let mut statement = connection.prepare(
        "SELECT app_id FROM collection_games WHERE collection_id = ?1 ORDER BY position",
    )?;
    let rows = statement.query_map([collection_id], |row| row.get(0))?;
    Ok(rows.collect::<Result<HashSet<_>, _>>()?)
}

pub fn smart_collection_game_ids(
    connection: &Connection,
    collection_id: &str,
) -> AppResult<Option<HashSet<u32>>> {
    let (kind, match_mode) = connection
        .query_row(
            "SELECT kind, match_mode FROM collections WHERE id = ?1",
            [collection_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La colección ya no existe."))?;
    if kind == "manual" {
        return Ok(None);
    }
    let rules = list_smart_rules(connection, collection_id)?;
    Ok(Some(evaluate_rules(
        &load_game_facts(connection)?,
        &rules,
        &match_mode,
    )))
}

pub fn preview_smart_rules(
    connection: &Connection,
    rules: &[SmartRule],
    match_mode: &str,
) -> AppResult<HashSet<u32>> {
    if !matches!(match_mode, "all" | "any") {
        return Err(AppError::validation(
            "El modo de coincidencia debe ser «todas» o «cualquiera».",
        ));
    }
    if rules.is_empty() {
        return Ok(HashSet::new());
    }
    for rule in rules {
        validate_rule(rule)?;
    }
    Ok(evaluate_rules(
        &load_game_facts(connection)?,
        rules,
        match_mode,
    ))
}

pub fn preview_smart_collection(
    connection: &Connection,
    input: &SaveCollectionInput,
) -> AppResult<HashSet<u32>> {
    if input.kind != "smart" {
        return Err(AppError::validation(
            "La vista previa solo está disponible para colecciones inteligentes.",
        ));
    }
    if input.rules.is_empty() {
        return Err(AppError::validation(
            "Añade al menos una regla para generar la vista previa.",
        ));
    }
    preview_smart_rules(connection, &input.rules, &input.match_mode)
}

// La interfaz cambia la pertenencia juego a juego, nunca de golpe, así que en
// producción no hay llamador. Las pruebas de este módulo sí la usan para
// montar colecciones, y es la única vía transaccional para fijar el conjunto
// completo de una vez.
#[allow(dead_code, reason = "sin llamador en producción; la usan las pruebas")]
pub fn set_collection_games(
    connection: &mut Connection,
    collection_id: &str,
    app_ids: &[u32],
) -> AppResult<()> {
    let ordered_ids = deduplicate_app_ids(app_ids)?;
    let transaction = connection.transaction()?;
    ensure_manual_collection(&transaction, collection_id)?;
    ensure_games_exist(&transaction, &ordered_ids)?;

    transaction.execute(
        "DELETE FROM collection_games WHERE collection_id = ?1",
        [collection_id],
    )?;
    for (position, app_id) in ordered_ids.iter().enumerate() {
        transaction.execute(
            "INSERT INTO collection_games(collection_id, app_id, position)
             VALUES (?1, ?2, ?3)",
            params![collection_id, app_id, position as i64],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn set_game_collections(
    connection: &mut Connection,
    app_id: u32,
    collection_ids: &[String],
) -> AppResult<()> {
    let unique_ids = deduplicate_strings(collection_ids)?;
    let transaction = connection.transaction()?;
    ensure_games_exist(&transaction, &[app_id])?;
    for collection_id in &unique_ids {
        ensure_manual_collection(&transaction, collection_id)?;
    }

    let previous_collection_ids = {
        let mut statement = transaction.prepare(
            "SELECT collection_id FROM collection_games WHERE app_id = ?1 ORDER BY collection_id",
        )?;
        statement
            .query_map([app_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let desired = unique_ids.iter().cloned().collect::<HashSet<_>>();
    let previous = previous_collection_ids
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    for previous_id in &previous_collection_ids {
        if !desired.contains(previous_id) {
            transaction.execute(
                "DELETE FROM collection_games WHERE app_id = ?1 AND collection_id = ?2",
                params![app_id, previous_id],
            )?;
            normalize_collection_games(&transaction, previous_id)?;
        }
    }
    for collection_id in unique_ids {
        if !previous.contains(&collection_id) {
            let position: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(position), -1) + 1
                   FROM collection_games WHERE collection_id = ?1",
                [&collection_id],
                |row| row.get(0),
            )?;
            transaction.execute(
                "INSERT INTO collection_games(collection_id, app_id, position)
                 VALUES (?1, ?2, ?3)",
                params![collection_id, app_id, position],
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn list_planner(connection: &Connection) -> AppResult<Vec<PlannerColumn>> {
    let mut column_statement = connection.prepare(
        "SELECT id, name, color, position, wip_limit
           FROM planner_columns
          ORDER BY position ASC, name COLLATE NOCASE ASC",
    )?;
    let columns = column_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<u32>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut result = Vec::with_capacity(columns.len());
    let mut item_statement = connection.prepare(
        "SELECT pi.app_id, g.title, g.cover_url, p.progress, pi.position,
                COALESCE(pi.target_date, p.target_date),
                COALESCE(pi.estimated_minutes, p.estimated_minutes),
                COALESCE(pi.queue_position, pi.position), pi.planned_for,
                NULLIF(pi.objective, '')
           FROM planner_items pi
           JOIN games g ON g.app_id = pi.app_id
           JOIN game_personal p ON p.app_id = pi.app_id
          WHERE pi.column_id = ?1
          ORDER BY pi.position ASC, g.title COLLATE NOCASE ASC",
    )?;
    for (id, name, color, position, wip_limit) in columns {
        let items = item_statement
            .query_map([&id], |row| {
                Ok(PlannerItem {
                    app_id: row.get(0)?,
                    title: row.get(1)?,
                    cover_url: row.get(2)?,
                    progress: row.get(3)?,
                    position: row.get(4)?,
                    target_date: row.get(5)?,
                    estimated_minutes: row.get(6)?,
                    queue_position: row.get(7)?,
                    planned_for: row.get(8)?,
                    objective: row.get(9)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        result.push(PlannerColumn {
            id,
            name,
            color,
            position,
            wip_limit,
            items,
        });
    }
    Ok(result)
}

pub fn planner_overview(connection: &Connection) -> AppResult<PlannerOverview> {
    let columns = list_planner(connection)?;
    let mut queue = columns
        .iter()
        .flat_map(|column| column.items.iter().cloned())
        .collect::<Vec<_>>();
    queue.sort_by_key(|item| (item.queue_position, item.app_id));
    let settings = connection.query_row(
        "SELECT weekly_capacity_minutes, monthly_capacity_minutes
           FROM planner_settings WHERE id = 1",
        [],
        |row| {
            Ok(PlannerSettings {
                weekly_capacity_minutes: row.get(0)?,
                monthly_capacity_minutes: row.get(1)?,
            })
        },
    )?;
    Ok(PlannerOverview {
        columns,
        queue,
        settings,
    })
}

pub fn move_planner_item(
    connection: &mut Connection,
    input: &MovePlannerItemInput,
) -> AppResult<()> {
    if input.app_id == 0 || input.position < 0 {
        return Err(AppError::validation(
            "La posición del juego en el planner no es válida.",
        ));
    }
    let transaction = connection.transaction()?;
    ensure_games_exist(&transaction, &[input.app_id])?;
    let wip_limit = transaction
        .query_row(
            "SELECT wip_limit FROM planner_columns WHERE id = ?1",
            [&input.column_id],
            |row| row.get::<_, Option<u32>>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La columna del planner ya no existe."))?;
    let existing_plan = transaction
        .query_row(
            "SELECT column_id, target_date, estimated_minutes, queue_position,
                    planned_for, objective
               FROM planner_items WHERE app_id = ?1",
            [input.app_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<u32>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .optional()?;
    let source_column = existing_plan.as_ref().map(|value| value.0.clone());
    if source_column.as_deref() != Some(input.column_id.as_str()) {
        let destination_count: u32 = transaction.query_row(
            "SELECT COUNT(*) FROM planner_items WHERE column_id = ?1",
            [&input.column_id],
            |row| row.get(0),
        )?;
        if wip_limit.is_some_and(|limit| destination_count >= limit) {
            return Err(AppError::validation(
                "La columna ha alcanzado su límite de trabajo en curso.",
            ));
        }
    }

    transaction.execute(
        "DELETE FROM planner_items WHERE app_id = ?1",
        [input.app_id],
    )?;
    if let Some(source) = source_column.as_deref()
        && source != input.column_id
    {
        normalize_planner_column(&transaction, source)?;
    }

    let (target_date, estimated_minutes, queue_position, planned_for, objective) =
        if let Some((_, target, estimate, queue, planned, objective)) = existing_plan {
            (target, estimate, queue.unwrap_or(0), planned, objective)
        } else {
            let (target, estimate) = transaction.query_row(
                "SELECT target_date, estimated_minutes FROM game_personal WHERE app_id = ?1",
                [input.app_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<u32>>(1)?,
                    ))
                },
            )?;
            let queue: i64 = transaction.query_row(
                "SELECT COALESCE(MAX(queue_position), -1) + 1 FROM planner_items",
                [],
                |row| row.get(0),
            )?;
            (target, estimate, queue, None, String::new())
        };
    let mut destination_ids = planner_app_ids(&transaction, &input.column_id)?;
    let insert_at = (input.position as usize).min(destination_ids.len());
    destination_ids.insert(insert_at, input.app_id);
    transaction.execute(
        "INSERT INTO planner_items(
            column_id, app_id, position, target_date, estimated_minutes,
            queue_position, planned_for, objective
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            input.column_id,
            input.app_id,
            insert_at as i64,
            target_date,
            estimated_minutes,
            queue_position,
            planned_for,
            objective
        ],
    )?;
    write_planner_positions(&transaction, &input.column_id, &destination_ids)?;
    transaction.commit()?;
    Ok(())
}

pub fn remove_planner_item(connection: &mut Connection, app_id: u32) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let source = transaction
        .query_row(
            "SELECT column_id FROM planner_items WHERE app_id = ?1",
            [app_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego no está en el planner."))?;
    transaction.execute("DELETE FROM planner_items WHERE app_id = ?1", [app_id])?;
    normalize_planner_column(&transaction, &source)?;
    normalize_planner_queue(&transaction)?;
    transaction.commit()?;
    Ok(())
}

pub fn save_planner_item(
    connection: &mut Connection,
    input: &SavePlannerItemInput,
) -> AppResult<()> {
    if input.app_id == 0 {
        return Err(AppError::validation("El juego del plan no es válido."));
    }
    let objective = input.objective.as_deref().unwrap_or_default().trim();
    if objective.chars().count() > 160 {
        return Err(AppError::validation(
            "El objetivo no puede superar 160 caracteres.",
        ));
    }
    if input
        .estimated_minutes
        .is_some_and(|minutes| minutes > 600_000)
    {
        return Err(AppError::validation(
            "La estimación del juego supera el máximo permitido.",
        ));
    }
    let planned = validate_planner_date(input.planned_for.as_deref(), "planificada")?;
    let target = validate_planner_date(input.target_date.as_deref(), "objetivo")?;
    if planned
        .zip(target)
        .is_some_and(|(planned, target)| target < planned)
    {
        return Err(AppError::validation(
            "La fecha objetivo no puede ser anterior a la fecha planificada.",
        ));
    }

    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE planner_items
            SET objective = ?2, planned_for = ?3, target_date = ?4,
                estimated_minutes = ?5
          WHERE app_id = ?1",
        params![
            input.app_id,
            objective,
            input.planned_for,
            input.target_date,
            input.estimated_minutes
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found(
            "El juego ya no está en el planificador.",
        ));
    }
    transaction.execute(
        "UPDATE game_personal
            SET target_date = ?2, estimated_minutes = ?3,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
        params![input.app_id, input.target_date, input.estimated_minutes],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn move_planner_queue_item(
    connection: &mut Connection,
    app_id: u32,
    position: i64,
) -> AppResult<()> {
    if app_id == 0 || position < 0 {
        return Err(AppError::validation("La posición de la cola no es válida."));
    }
    let transaction = connection.transaction()?;
    let mut ids = planner_queue_app_ids(&transaction)?;
    let previous = ids
        .iter()
        .position(|candidate| *candidate == app_id)
        .ok_or_else(|| AppError::not_found("El juego ya no está en la cola."))?;
    ids.remove(previous);
    let insert_at = (position as usize).min(ids.len());
    ids.insert(insert_at, app_id);
    write_planner_queue_positions(&transaction, &ids)?;
    transaction.commit()?;
    Ok(())
}

pub fn save_planner_settings(
    connection: &mut Connection,
    settings: &PlannerSettings,
) -> AppResult<PlannerSettings> {
    if settings.weekly_capacity_minutes < 60
        || settings.monthly_capacity_minutes < 60
        || settings.weekly_capacity_minutes > 600_000
        || settings.monthly_capacity_minutes > 2_400_000
    {
        return Err(AppError::validation(
            "La capacidad debe estar entre una y 40.000 horas.",
        ));
    }
    if settings.monthly_capacity_minutes < settings.weekly_capacity_minutes {
        return Err(AppError::validation(
            "La capacidad mensual debe ser igual o mayor que la semanal.",
        ));
    }
    connection.execute(
        "UPDATE planner_settings
            SET weekly_capacity_minutes = ?1, monthly_capacity_minutes = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = 1",
        params![
            settings.weekly_capacity_minutes,
            settings.monthly_capacity_minutes
        ],
    )?;
    Ok(settings.clone())
}

pub fn save_planner_column(
    connection: &mut Connection,
    id: Option<&str>,
    name: &str,
    color: &str,
    wip_limit: Option<u32>,
) -> AppResult<PlannerColumn> {
    let name = validate_name(name, "columna")?;
    let color = validate_color(color)?;
    if wip_limit == Some(0) {
        return Err(AppError::validation(
            "El límite de trabajo en curso debe ser mayor que cero.",
        ));
    }
    let transaction = connection.transaction()?;
    let column_id = id
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let exists = transaction
        .query_row(
            "SELECT 1 FROM planner_columns WHERE id = ?1",
            [&column_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if id.is_some() && !exists {
        return Err(AppError::not_found("La columna del planner ya no existe."));
    }
    let duplicate = transaction
        .query_row(
            "SELECT 1 FROM planner_columns
              WHERE name = ?1 COLLATE NOCASE AND id <> ?2",
            params![name, column_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if duplicate {
        return Err(AppError::validation(
            "Ya existe una columna con ese nombre.",
        ));
    }
    if let Some(limit) = wip_limit {
        let count: u32 = transaction.query_row(
            "SELECT COUNT(*) FROM planner_items WHERE column_id = ?1",
            [&column_id],
            |row| row.get(0),
        )?;
        if count > limit {
            return Err(AppError::validation(format!(
                "La columna ya contiene {count} juegos; mueve alguno antes de fijar el límite en {limit}."
            )));
        }
    }
    if exists {
        transaction.execute(
            "UPDATE planner_columns SET name = ?2, color = ?3, wip_limit = ?4 WHERE id = ?1",
            params![column_id, name, color, wip_limit],
        )?;
    } else {
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM planner_columns",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO planner_columns(id, name, color, position, wip_limit)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![column_id, name, color, position, wip_limit],
        )?;
    }
    transaction.commit()?;
    list_planner(connection)?
        .into_iter()
        .find(|column| column.id == column_id)
        .ok_or_else(|| AppError::not_found("La columna del planner ya no existe."))
}

pub fn delete_planner_column(
    connection: &mut Connection,
    column_id: &str,
    replacement_column_id: Option<&str>,
) -> AppResult<()> {
    if replacement_column_id == Some(column_id) {
        return Err(AppError::validation(
            "La columna de reemplazo debe ser distinta.",
        ));
    }
    let transaction = connection.transaction()?;
    let exists = transaction
        .query_row(
            "SELECT 1 FROM planner_columns WHERE id = ?1",
            [column_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::not_found("La columna del planner ya no existe."));
    }
    let source_ids = planner_app_ids(&transaction, column_id)?;
    if !source_ids.is_empty() && replacement_column_id.is_none() {
        return Err(AppError::validation(
            "La columna contiene juegos. Selecciona otra columna para conservarlos.",
        ));
    }
    if let Some(replacement) = replacement_column_id {
        let limit = transaction
            .query_row(
                "SELECT wip_limit FROM planner_columns WHERE id = ?1",
                [replacement],
                |row| row.get::<_, Option<u32>>(0),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("La columna de reemplazo ya no existe."))?;
        let mut destination_ids = planner_app_ids(&transaction, replacement)?;
        if limit.is_some_and(|limit| destination_ids.len() + source_ids.len() > limit as usize) {
            return Err(AppError::validation(
                "La columna de reemplazo no tiene capacidad para conservar todos los juegos.",
            ));
        }
        for app_id in &source_ids {
            transaction.execute(
                "UPDATE planner_items SET column_id = ?2 WHERE column_id = ?1 AND app_id = ?3",
                params![column_id, replacement, app_id],
            )?;
            destination_ids.push(*app_id);
        }
        write_planner_positions(&transaction, replacement, &destination_ids)?;
    }
    transaction.execute("DELETE FROM planner_columns WHERE id = ?1", [column_id])?;
    normalize_planner_columns(&transaction)?;
    transaction.commit()?;
    Ok(())
}

pub fn reorder_planner_columns(
    connection: &mut Connection,
    ordered_ids: &[String],
) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let unique: HashSet<&String> = ordered_ids.iter().collect();
    let count_raw: i64 =
        transaction.query_row("SELECT COUNT(*) FROM planner_columns", [], |row| row.get(0))?;
    let count = usize::try_from(count_raw).map_err(|_| {
        AppError::new(
            "database_data",
            "El recuento de columnas guardado no es válido.",
        )
    })?;
    if unique.len() != ordered_ids.len() || unique.len() != count {
        return Err(AppError::validation(
            "La lista de columnas no coincide con el planner guardado.",
        ));
    }
    for (position, id) in ordered_ids.iter().enumerate() {
        let changed = transaction.execute(
            "UPDATE planner_columns SET position = ?2 WHERE id = ?1",
            params![id, position as i64],
        )?;
        if changed != 1 {
            return Err(AppError::validation(
                "La lista contiene una columna desconocida.",
            ));
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn save_preferences(
    connection: &mut Connection,
    preferences: &AppPreferences,
) -> AppResult<()> {
    if !matches!(
        preferences.density.as_str(),
        "ultraCompact" | "compact" | "comfortable"
    ) {
        return Err(AppError::validation(
            "La densidad debe ser ultracompacta, compacta o cómoda.",
        ));
    }
    if preferences.periodic_sync_minutes != 0 && preferences.periodic_sync_minutes < 15 {
        return Err(AppError::validation(
            "La sincronización periódica debe estar desactivada o usar un intervalo mínimo de 15 minutos.",
        ));
    }
    if preferences.periodic_sync_minutes > 10_080 {
        return Err(AppError::validation(
            "El intervalo de sincronización no puede superar una semana.",
        ));
    }
    if !crate::models::is_valid_game_sort(&preferences.library_sort) {
        return Err(AppError::validation(
            "El criterio de ordenación de la biblioteca no es válido.",
        ));
    }
    if preferences.art_cache_mib < crate::models::MIN_ART_CACHE_MIB
        || preferences.art_cache_mib > crate::models::MAX_ART_CACHE_MIB
    {
        return Err(AppError::validation(
            "El presupuesto de la caché de arte debe estar entre 128 MiB y 8 GiB.",
        ));
    }
    validate_shortcuts(&preferences.shortcuts)?;
    let shortcuts = serde_json::to_string(&preferences.shortcuts).map_err(|_| {
        AppError::validation("Los atajos no se pudieron serializar de forma segura.")
    })?;
    let transaction = connection.transaction()?;
    for (key, value) in [
        ("density", preferences.density.clone()),
        (
            "periodic_sync_minutes",
            preferences.periodic_sync_minutes.to_string(),
        ),
        (
            "confirm_uninstall",
            preferences.confirm_uninstall.to_string(),
        ),
        ("library_sort", preferences.library_sort.clone()),
        ("art_cache_mib", preferences.art_cache_mib.to_string()),
        ("shortcuts", shortcuts),
    ] {
        transaction.execute(
            "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![key, value],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

pub fn load_preferences(connection: &Connection) -> AppResult<AppPreferences> {
    let value = |key: &str, fallback: &str| -> AppResult<String> {
        Ok(connection
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or_else(|| fallback.to_string()))
    };
    let density = value("density", "compact")?;
    let periodic_sync_minutes = value("periodic_sync_minutes", "0")?
        .parse()
        .map_err(|_| AppError::new("database_data", "El intervalo guardado no es válido."))?;
    let confirm_uninstall = match value("confirm_uninstall", "true")?.as_str() {
        "true" => true,
        "false" => false,
        _ => {
            return Err(AppError::new(
                "database_data",
                "La preferencia de confirmación guardada no es válida.",
            ));
        }
    };
    let art_cache_mib = value("art_cache_mib", "512")?.parse().map_err(|_| {
        AppError::new(
            "database_data",
            "El presupuesto guardado de la caché de arte no es válido.",
        )
    })?;
    let library_sort = value("library_sort", "manual")?;
    if !crate::models::is_valid_game_sort(&library_sort) {
        return Err(AppError::new(
            "database_data",
            "El criterio de ordenación guardado no es válido.",
        ));
    }
    let shortcuts = serde_json::from_str(&value(
        "shortcuts",
        &serde_json::to_string(&crate::models::ShortcutBindings::default())
            .expect("los atajos predeterminados son serializables"),
    )?)
    .map_err(|_| AppError::new("database_data", "Los atajos guardados no son válidos."))?;
    validate_shortcuts(&shortcuts)
        .map_err(|_| AppError::new("database_data", "Los atajos guardados no son válidos."))?;
    Ok(AppPreferences {
        density,
        periodic_sync_minutes,
        confirm_uninstall,
        library_sort,
        art_cache_mib,
        shortcuts,
    })
}

fn validate_shortcuts(shortcuts: &crate::models::ShortcutBindings) -> AppResult<()> {
    use std::collections::HashSet;

    let bindings = [
        &shortcuts.library,
        &shortcuts.planner,
        &shortcuts.wishlist,
        &shortcuts.collections,
        &shortcuts.tracking,
        &shortcuts.search,
        &shortcuts.sync,
        &shortcuts.close_panel,
    ];
    let mut unique = HashSet::new();
    for binding in bindings {
        if binding == "Mod+Comma" {
            return Err(AppError::validation(
                "El atajo Mod+Coma está reservado para abrir Ajustes.",
            ));
        }
        if !is_valid_shortcut(binding) {
            return Err(AppError::validation(
                "Cada atajo debe usar una combinación válida con Mod, Alt o Mayús.",
            ));
        }
        if !unique.insert(binding) {
            return Err(AppError::validation(
                "Dos acciones no pueden compartir el mismo atajo.",
            ));
        }
    }
    Ok(())
}

fn is_valid_shortcut(binding: &str) -> bool {
    if binding.is_empty() || binding.len() > 64 {
        return false;
    }
    let parts = binding.split('+').collect::<Vec<_>>();
    let Some(key) = parts.last().copied() else {
        return false;
    };
    let modifiers = &parts[..parts.len().saturating_sub(1)];
    let modifiers_valid = modifiers
        .iter()
        .all(|part| matches!(*part, "Mod" | "Alt" | "Shift"))
        && modifiers.iter().collect::<HashSet<_>>().len() == modifiers.len();
    let key_valid = key.len() == 1
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
        || matches!(
            key,
            "Escape"
                | "Enter"
                | "Backspace"
                | "Delete"
                | "Insert"
                | "Home"
                | "End"
                | "Space"
                | "Comma"
                | "Slash"
                | "ArrowUp"
                | "ArrowDown"
                | "ArrowLeft"
                | "ArrowRight"
                | "F1"
                | "F2"
                | "F3"
                | "F4"
                | "F5"
                | "F6"
                | "F7"
                | "F8"
                | "F9"
                | "F10"
                | "F11"
                | "F12"
        );
    modifiers_valid
        && key_valid
        // `Intro`, `Espacio`, las flechas, `Inicio`, `Fin`, `Insert` y `Supr`
        // valen por sí solas: la interfaz sólo las dispara dentro de la rejilla
        // de la biblioteca, nunca sobre el control que tenga el foco.
        && (!modifiers.is_empty()
            || matches!(
                key,
                "Escape"
                    | "Enter"
                    | "Space"
                    | "Delete"
                    | "Insert"
                    | "Home"
                    | "End"
                    | "ArrowUp"
                    | "ArrowDown"
                    | "ArrowLeft"
                    | "ArrowRight"
            )
            || key.starts_with('F'))
}

fn find_status(connection: &Connection, status_id: &str) -> AppResult<StatusDefinition> {
    list_statuses(connection)?
        .into_iter()
        .find(|status| status.id == status_id)
        .ok_or_else(|| AppError::not_found("El estado ya no existe."))
}

fn find_collection(connection: &Connection, collection_id: &str) -> AppResult<CollectionSummary> {
    list_collections(connection)?
        .into_iter()
        .find(|collection| collection.id == collection_id)
        .ok_or_else(|| AppError::not_found("La colección ya no existe."))
}

fn validate_name(value: &str, entity: &str) -> AppResult<String> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > MAX_NAME_LENGTH {
        return Err(AppError::validation(format!(
            "El nombre del {entity} debe tener entre 1 y {MAX_NAME_LENGTH} caracteres."
        )));
    }
    Ok(value.to_string())
}

fn validate_color(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::validation(
            "El color debe usar el formato hexadecimal #RRGGBB.",
        ));
    }
    Ok(value.to_ascii_uppercase())
}

fn ensure_status_exists(connection: &Connection, status_id: &str) -> AppResult<()> {
    if connection
        .query_row("SELECT 1 FROM statuses WHERE id = ?1", [status_id], |_| {
            Ok(())
        })
        .optional()?
        .is_none()
    {
        return Err(AppError::not_found("El estado ya no existe."));
    }
    Ok(())
}

fn ensure_unique_name(
    connection: &Connection,
    table: &str,
    name: &str,
    excluding_id: Option<&str>,
) -> AppResult<()> {
    debug_assert!(matches!(table, "statuses" | "collections"));
    let sql = format!(
        "SELECT 1 FROM {table} WHERE name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id <> ?2)"
    );
    if connection
        .query_row(&sql, params![name, excluding_id], |_| Ok(()))
        .optional()?
        .is_some()
    {
        return Err(AppError::validation(
            "Ya existe un elemento con ese nombre.",
        ));
    }
    Ok(())
}

fn ensure_exact_id_set(
    connection: &Connection,
    table: &str,
    ordered_ids: &[String],
) -> AppResult<()> {
    debug_assert!(matches!(table, "statuses" | "collections"));
    let unique: HashSet<&String> = ordered_ids.iter().collect();
    let count_raw: i64 =
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?;
    let count = usize::try_from(count_raw).map_err(|_| {
        AppError::new(
            "database_data",
            "El recuento de elementos guardado no es válido.",
        )
    })?;
    if unique.len() != ordered_ids.len() || unique.len() != count {
        return Err(AppError::validation(
            "La lista de ordenación no coincide con los elementos guardados.",
        ));
    }
    for id in ordered_ids {
        let exists = connection
            .query_row(
                &format!("SELECT 1 FROM {table} WHERE id = ?1"),
                [id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::validation(
                "La lista de ordenación contiene un elemento desconocido.",
            ));
        }
    }
    Ok(())
}

fn write_positions(connection: &Connection, table: &str, ordered_ids: &[String]) -> AppResult<()> {
    debug_assert!(matches!(table, "statuses" | "collections"));
    let mut statement = connection.prepare(&format!(
        "UPDATE {table} SET position = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1"
    ))?;
    for (position, id) in ordered_ids.iter().enumerate() {
        statement.execute(params![id, position as i64])?;
    }
    Ok(())
}

fn normalize_positions(connection: &Connection, table: &str) -> AppResult<()> {
    debug_assert!(matches!(table, "statuses" | "collections"));
    let mut statement = connection.prepare(&format!(
        "SELECT id FROM {table} ORDER BY position ASC, name COLLATE NOCASE ASC"
    ))?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    write_positions(connection, table, &ids)
}

fn replace_smart_rules(
    transaction: &Transaction<'_>,
    collection_id: &str,
    rules: &[SmartRule],
) -> AppResult<()> {
    transaction.execute(
        "DELETE FROM smart_rules WHERE collection_id = ?1",
        [collection_id],
    )?;
    let mut seen_ids = HashSet::new();
    for (fallback_position, rule) in rules.iter().enumerate() {
        let id = rule
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        if !seen_ids.insert(id.clone()) {
            return Err(AppError::validation(
                "Dos reglas inteligentes no pueden compartir identificador.",
            ));
        }
        let id_in_use = transaction
            .query_row("SELECT 1 FROM smart_rules WHERE id = ?1", [&id], |_| Ok(()))
            .optional()?
            .is_some();
        if id_in_use {
            return Err(AppError::validation(
                "Una regla inteligente pertenece a otra colección.",
            ));
        }
        transaction.execute(
            "INSERT INTO smart_rules(
                id, collection_id, group_id, field, operator, value_json, position
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                collection_id,
                rule.group_id,
                rule.field,
                rule.operator,
                serde_json::to_string(&rule.value).map_err(|error| AppError::new(
                    "serialization",
                    format!("No se pudo guardar la regla: {error}")
                ))?,
                if rule.position >= 0 {
                    rule.position
                } else {
                    fallback_position as i64
                }
            ],
        )?;
    }
    Ok(())
}

fn validate_rule(rule: &SmartRule) -> AppResult<()> {
    if rule.group_id < 0 {
        return Err(AppError::validation(
            "El grupo de una regla no puede ser negativo.",
        ));
    }
    let allowed = match rule.field.as_str() {
        "installed" | "tracking" | "earlyAccess" | "isFree" => {
            &["equals", "notEquals", "isTrue", "isFalse"][..]
        }
        "progress" | "priority" | "rating" | "playtimeMinutes" | "estimatedMinutes"
        | "achievementPercent" => &[
            "equals",
            "notEquals",
            "greaterThan",
            "greaterOrEqual",
            "lessThan",
            "lessOrEqual",
            "isSet",
            "isNotSet",
        ][..],
        "targetDate" | "releaseDate" | "lastPlayedAt" | "importedAt" | "updatedAt" => &[
            "equals",
            "notEquals",
            "before",
            "after",
            "isSet",
            "isNotSet",
        ][..],
        "title" | "developer" | "publisher" | "statusId" | "steamDeckStatus" => &[
            "equals",
            "notEquals",
            "contains",
            "notContains",
            "in",
            "isSet",
            "isNotSet",
        ][..],
        "genre" | "category" | "tag" => {
            &["equals", "notEquals", "contains", "notContains", "in"][..]
        }
        _ => {
            return Err(AppError::validation(format!(
                "El campo de regla «{}» no está disponible.",
                rule.field
            )));
        }
    };
    if !allowed.contains(&rule.operator.as_str()) {
        return Err(AppError::validation(format!(
            "El operador «{}» no es válido para «{}».",
            rule.operator, rule.field
        )));
    }
    if matches!(
        rule.operator.as_str(),
        "isSet" | "isNotSet" | "isTrue" | "isFalse"
    ) {
        return Ok(());
    }
    match rule.field.as_str() {
        "installed" | "tracking" | "earlyAccess" | "isFree" => {
            if rule.value.is_boolean() {
                Ok(())
            } else {
                Err(AppError::validation("La regla necesita un valor booleano."))
            }
        }
        "progress" | "priority" | "rating" | "playtimeMinutes" | "estimatedMinutes"
        | "achievementPercent" => {
            if rule.value.as_f64().is_some() {
                Ok(())
            } else {
                Err(AppError::validation("La regla necesita un valor numérico."))
            }
        }
        _ => {
            if json_strings(&rule.value).is_empty() {
                Err(AppError::validation(format!(
                    "La regla «{}» necesita al menos un valor de comparación.",
                    rule.field
                )))
            } else {
                Ok(())
            }
        }
    }
}

fn load_game_facts(connection: &Connection) -> AppResult<Vec<GameFacts>> {
    let mut tags: HashMap<u32, Vec<String>> = HashMap::new();
    let mut tag_statement = connection.prepare(
        "SELECT gt.app_id, t.name
           FROM game_tags gt JOIN tags t ON t.id = gt.tag_id
          ORDER BY t.name COLLATE NOCASE",
    )?;
    let tag_rows = tag_statement.query_map([], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in tag_rows {
        let (app_id, tag) = row?;
        tags.entry(app_id).or_default().push(tag);
    }

    let mut statement = connection.prepare(
        "SELECT g.app_id, g.title, g.developer, g.publisher, g.genres_json,
                g.categories_json, g.is_early_access, g.playtime_minutes,
                g.release_date, p.status_id, p.installed, p.tracking, p.progress,
                p.priority, p.rating, p.estimated_minutes, p.target_date,
                g.is_free, g.last_played_at, g.imported_at, g.updated_at,
                g.steam_deck_status, g.achievements_unlocked, g.achievements_total
           FROM games g JOIN game_personal p ON p.app_id = g.app_id
          WHERE NOT EXISTS (
                SELECT 1 FROM game_archive a WHERE a.app_id = g.app_id
          )",
    )?;
    let rows = statement.query_map([], |row| {
        let genres_json: String = row.get(4)?;
        let categories_json: String = row.get(5)?;
        let genres = parse_string_array(&genres_json)?;
        let categories = parse_string_array(&categories_json)?;
        let app_id = row.get::<_, u32>(0)?;
        Ok(GameFacts {
            app_id,
            title: row.get(1)?,
            developer: row.get(2)?,
            publisher: row.get(3)?,
            genres,
            categories,
            early_access: row.get::<_, i64>(6)? != 0,
            playtime_minutes: row.get::<_, f64>(7)?,
            release_date: row.get(8)?,
            status_id: row.get(9)?,
            installed: row.get::<_, i64>(10)? != 0,
            tracking: row.get::<_, i64>(11)? != 0,
            progress: row.get(12)?,
            priority: row.get(13)?,
            rating: row.get(14)?,
            estimated_minutes: row.get(15)?,
            target_date: row.get(16)?,
            is_free: row.get::<_, i64>(17)? != 0,
            last_played_at: row.get(18)?,
            imported_at: row.get(19)?,
            updated_at: row.get(20)?,
            steam_deck_status: row.get(21)?,
            achievement_percent: {
                let unlocked = row.get::<_, Option<f64>>(22)?;
                let total = row.get::<_, Option<f64>>(23)?;
                match (unlocked, total) {
                    (Some(unlocked), Some(total)) if total > 0.0 => {
                        Some((unlocked / total) * 100.0)
                    }
                    _ => None,
                }
            },
            tags: tags.remove(&app_id).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn parse_string_array(raw: &str) -> rusqlite::Result<Vec<String>> {
    serde_json::from_str(raw).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            raw.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn evaluate_rules(facts: &[GameFacts], rules: &[SmartRule], match_mode: &str) -> HashSet<u32> {
    if rules.is_empty() {
        return HashSet::new();
    }
    let mut groups: BTreeMap<i64, Vec<&SmartRule>> = BTreeMap::new();
    for rule in rules {
        groups.entry(rule.group_id).or_default().push(rule);
    }
    facts
        .iter()
        .filter(|facts| {
            let group_matches = groups
                .values()
                .map(|group| group.iter().all(|rule| rule_matches(facts, rule)));
            if match_mode == "any" {
                group_matches.into_iter().any(|matches| matches)
            } else {
                group_matches.into_iter().all(|matches| matches)
            }
        })
        .map(|facts| facts.app_id)
        .collect()
}

fn rule_matches(facts: &GameFacts, rule: &SmartRule) -> bool {
    match rule.field.as_str() {
        "installed" => compare_bool(facts.installed, rule),
        "tracking" => compare_bool(facts.tracking, rule),
        "earlyAccess" => compare_bool(facts.early_access, rule),
        "isFree" => compare_bool(facts.is_free, rule),
        "progress" => compare_number(Some(facts.progress), rule),
        "priority" => compare_number(Some(facts.priority), rule),
        "rating" => compare_number(facts.rating, rule),
        "playtimeMinutes" => compare_number(Some(facts.playtime_minutes), rule),
        "estimatedMinutes" => compare_number(facts.estimated_minutes, rule),
        "achievementPercent" => compare_number(facts.achievement_percent, rule),
        "targetDate" => compare_date(facts.target_date.as_deref(), rule),
        "releaseDate" => compare_date(facts.release_date.as_deref(), rule),
        "lastPlayedAt" => compare_date(facts.last_played_at.as_deref(), rule),
        "importedAt" => compare_date(Some(&facts.imported_at), rule),
        "updatedAt" => compare_date(Some(&facts.updated_at), rule),
        "title" => compare_string(Some(&facts.title), rule),
        "developer" => compare_string(facts.developer.as_deref(), rule),
        "publisher" => compare_string(facts.publisher.as_deref(), rule),
        "statusId" => compare_string(Some(&facts.status_id), rule),
        "steamDeckStatus" => compare_string(facts.steam_deck_status.as_deref(), rule),
        "genre" => compare_string_list(&facts.genres, rule),
        "category" => compare_string_list(&facts.categories, rule),
        "tag" => compare_string_list(&facts.tags, rule),
        _ => false,
    }
}

fn compare_bool(actual: bool, rule: &SmartRule) -> bool {
    match rule.operator.as_str() {
        "isTrue" => actual,
        "isFalse" => !actual,
        "equals" => rule.value.as_bool() == Some(actual),
        "notEquals" => rule
            .value
            .as_bool()
            .is_some_and(|expected| actual != expected),
        _ => false,
    }
}

fn compare_number(actual: Option<f64>, rule: &SmartRule) -> bool {
    match rule.operator.as_str() {
        "isSet" => actual.is_some(),
        "isNotSet" => actual.is_none(),
        _ => {
            let (Some(actual), Some(expected)) = (actual, rule.value.as_f64()) else {
                return false;
            };
            match rule.operator.as_str() {
                "equals" => (actual - expected).abs() < f64::EPSILON,
                "notEquals" => (actual - expected).abs() >= f64::EPSILON,
                "greaterThan" => actual > expected,
                "greaterOrEqual" => actual >= expected,
                "lessThan" => actual < expected,
                "lessOrEqual" => actual <= expected,
                _ => false,
            }
        }
    }
}

fn compare_date(actual: Option<&str>, rule: &SmartRule) -> bool {
    match rule.operator.as_str() {
        "isSet" => actual.is_some_and(|value| !value.is_empty()),
        "isNotSet" => actual.is_none_or(str::is_empty),
        _ => {
            let Some(actual) = actual else { return false };
            let Some(expected) = rule.value.as_str() else {
                return false;
            };
            match rule.operator.as_str() {
                "equals" => actual == expected,
                "notEquals" => actual != expected,
                "before" => actual < expected,
                "after" => actual > expected,
                _ => false,
            }
        }
    }
}

fn compare_string(actual: Option<&str>, rule: &SmartRule) -> bool {
    match rule.operator.as_str() {
        "isSet" => actual.is_some_and(|value| !value.trim().is_empty()),
        "isNotSet" => actual.is_none_or(|value| value.trim().is_empty()),
        _ => {
            let Some(actual) = actual else {
                return rule.operator == "notEquals" || rule.operator == "notContains";
            };
            compare_text(actual, rule)
        }
    }
}

fn compare_string_list(actual: &[String], rule: &SmartRule) -> bool {
    let expected = json_strings(&rule.value);
    match rule.operator.as_str() {
        "equals" => expected
            .first()
            .is_some_and(|value| actual.iter().any(|item| eq_ignore_case(item, value))),
        "notEquals" => expected
            .first()
            .is_some_and(|value| actual.iter().all(|item| !eq_ignore_case(item, value))),
        "contains" => expected
            .first()
            .is_some_and(|value| actual.iter().any(|item| contains_ignore_case(item, value))),
        "notContains" => expected
            .first()
            .is_some_and(|value| actual.iter().all(|item| !contains_ignore_case(item, value))),
        "in" => actual
            .iter()
            .any(|item| expected.iter().any(|value| eq_ignore_case(item, value))),
        _ => false,
    }
}

fn compare_text(actual: &str, rule: &SmartRule) -> bool {
    let expected = json_strings(&rule.value);
    match rule.operator.as_str() {
        "equals" => expected
            .first()
            .is_some_and(|value| eq_ignore_case(actual, value)),
        "notEquals" => expected
            .first()
            .is_some_and(|value| !eq_ignore_case(actual, value)),
        "contains" => expected
            .first()
            .is_some_and(|value| contains_ignore_case(actual, value)),
        "notContains" => expected
            .first()
            .is_some_and(|value| !contains_ignore_case(actual, value)),
        "in" => expected.iter().any(|value| eq_ignore_case(actual, value)),
        _ => false,
    }
}

fn json_strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => vec![value.clone()],
        Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

fn eq_ignore_case(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    haystack
        .to_lowercase()
        .contains(&needle.trim().to_lowercase())
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
            "Las colecciones inteligentes se actualizan mediante sus reglas.",
        ));
    }
    Ok(())
}

fn ensure_games_exist(connection: &Connection, app_ids: &[u32]) -> AppResult<()> {
    for app_id in app_ids {
        if *app_id == 0
            || connection
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

#[allow(dead_code, reason = "auxiliar de set_collection_games")]
fn deduplicate_app_ids(app_ids: &[u32]) -> AppResult<Vec<u32>> {
    let mut seen = HashSet::new();
    let result = app_ids
        .iter()
        .copied()
        .filter(|app_id| seen.insert(*app_id))
        .collect::<Vec<_>>();
    if result.contains(&0) {
        return Err(AppError::validation("Steam no utiliza el AppID 0."));
    }
    Ok(result)
}

fn deduplicate_strings(values: &[String]) -> AppResult<Vec<String>> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for value in values {
        if value.trim().is_empty() {
            return Err(AppError::validation(
                "La lista contiene una colección sin identificador.",
            ));
        }
        if seen.insert(value.clone()) {
            result.push(value.clone());
        }
    }
    Ok(result)
}

fn planner_app_ids(connection: &Connection, column_id: &str) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM planner_items WHERE column_id = ?1 ORDER BY position ASC, app_id ASC",
    )?;
    Ok(statement
        .query_map([column_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn planner_queue_app_ids(connection: &Connection) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM planner_items
          ORDER BY queue_position ASC, app_id ASC",
    )?;
    Ok(statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn write_planner_queue_positions(connection: &Connection, app_ids: &[u32]) -> AppResult<()> {
    let offset = i64::try_from(app_ids.len())
        .map_err(|_| AppError::validation("La cola contiene demasiados juegos."))?
        + 1;
    connection.execute(
        "UPDATE planner_items SET queue_position = queue_position + ?1
          WHERE queue_position IS NOT NULL",
        [offset],
    )?;
    let mut statement =
        connection.prepare("UPDATE planner_items SET queue_position = ?2 WHERE app_id = ?1")?;
    for (position, app_id) in app_ids.iter().enumerate() {
        statement.execute(params![app_id, position as i64])?;
    }
    Ok(())
}

fn normalize_planner_queue(connection: &Connection) -> AppResult<()> {
    let ids = planner_queue_app_ids(connection)?;
    write_planner_queue_positions(connection, &ids)
}

fn validate_planner_date(value: Option<&str>, label: &str) -> AppResult<Option<NaiveDate>> {
    value
        .map(|date| {
            NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| {
                AppError::validation(format!("La fecha {label} debe usar el formato AAAA-MM-DD."))
            })
        })
        .transpose()
}

fn write_planner_positions(
    connection: &Connection,
    column_id: &str,
    app_ids: &[u32],
) -> AppResult<()> {
    let mut statement = connection
        .prepare("UPDATE planner_items SET position = ?3 WHERE column_id = ?1 AND app_id = ?2")?;
    for (position, app_id) in app_ids.iter().enumerate() {
        statement.execute(params![column_id, app_id, position as i64])?;
    }
    Ok(())
}

fn normalize_planner_column(connection: &Connection, column_id: &str) -> AppResult<()> {
    let ids = planner_app_ids(connection, column_id)?;
    write_planner_positions(connection, column_id, &ids)
}

fn normalize_collection_games(connection: &Connection, collection_id: &str) -> AppResult<()> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM collection_games
          WHERE collection_id = ?1 ORDER BY position ASC, app_id ASC",
    )?;
    let app_ids = statement
        .query_map([collection_id], |row| row.get::<_, u32>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut update = connection.prepare(
        "UPDATE collection_games SET position = ?3
          WHERE collection_id = ?1 AND app_id = ?2",
    )?;
    for (position, app_id) in app_ids.iter().enumerate() {
        update.execute(params![collection_id, app_id, position as i64])?;
    }
    Ok(())
}

fn normalize_planner_columns(connection: &Connection) -> AppResult<()> {
    let mut statement = connection
        .prepare("SELECT id FROM planner_columns ORDER BY position ASC, name COLLATE NOCASE ASC")?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    let mut update =
        connection.prepare("UPDATE planner_columns SET position = ?2 WHERE id = ?1")?;
    for (position, id) in ids.iter().enumerate() {
        update.execute(params![id, position as i64])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::library::{ImportedGame, upsert_imported_games};
    use crate::db::{migrations, seed_defaults};

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("aplicar migraciones");
        seed_defaults(&mut connection).expect("crear valores iniciales");
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
                "INSERT INTO game_personal(app_id, status_id, manual_position)
                 VALUES (?1, 'unclassified', ?1)",
                [app_id],
            )
            .expect("insertar datos personales");
    }

    fn manual_collection(name: &str) -> SaveCollectionInput {
        SaveCollectionInput {
            id: None,
            name: name.to_string(),
            description: String::new(),
            color: "#5CAAC1".to_string(),
            icon: "folder".to_string(),
            kind: "manual".to_string(),
            match_mode: "all".to_string(),
            rules: Vec::new(),
        }
    }

    #[test]
    fn custom_status_deletion_reassigns_games_without_losing_personal_data() {
        let mut connection = database();
        insert_game(&connection, 10, "Juego persistente");
        let status = save_status(&mut connection, None, "Favorito", "#abcdef")
            .expect("crear estado personalizado");
        connection
            .execute(
                "UPDATE game_personal
                    SET status_id = ?2, notes = 'No perder', progress = 73
                  WHERE app_id = ?1",
                params![10, status.id],
            )
            .expect("personalizar juego");

        delete_status(&mut connection, &status.id, "backlog").expect("eliminar estado");
        let personal: (String, String, u8) = connection
            .query_row(
                "SELECT status_id, notes, progress FROM game_personal WHERE app_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("leer datos personales");
        assert_eq!(personal, ("backlog".into(), "No perder".into(), 73));
    }

    #[test]
    fn built_in_status_cannot_be_deleted() {
        let mut connection = database();
        let error = delete_status(&mut connection, "playing", "backlog")
            .expect_err("proteger estado integrado");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn editing_manual_collection_preserves_membership_order() {
        let mut connection = database();
        insert_game(&connection, 10, "Primero");
        insert_game(&connection, 20, "Segundo");
        let created = save_collection(&mut connection, &manual_collection("Mi orden"))
            .expect("crear colección");
        set_collection_games(&mut connection, &created.id, &[20, 10]).expect("asignar juegos");

        let mut edited = manual_collection("Mi orden definitivo");
        edited.id = Some(created.id.clone());
        edited.description = "Se conserva".to_string();
        save_collection(&mut connection, &edited).expect("editar colección");

        let ids = planner_like_collection_ids(&connection, &created.id);
        assert_eq!(ids, vec![20, 10]);
    }

    #[test]
    fn invalid_manual_replacement_is_atomic() {
        let mut connection = database();
        insert_game(&connection, 10, "Primero");
        let created = save_collection(&mut connection, &manual_collection("Segura"))
            .expect("crear colección");
        set_collection_games(&mut connection, &created.id, &[10]).expect("asignar juego");

        let error = set_collection_games(&mut connection, &created.id, &[999])
            .expect_err("rechazar juego inexistente");
        assert_eq!(error.code, "not_found");
        assert_eq!(
            planner_like_collection_ids(&connection, &created.id),
            vec![10]
        );
    }

    #[test]
    fn editing_game_memberships_keeps_existing_manual_order() {
        let mut connection = database();
        insert_game(&connection, 10, "Primero");
        insert_game(&connection, 20, "Segundo");
        let first = save_collection(&mut connection, &manual_collection("Primera"))
            .expect("crear primera colección");
        let second = save_collection(&mut connection, &manual_collection("Segunda"))
            .expect("crear segunda colección");
        set_collection_games(&mut connection, &first.id, &[10, 20]).expect("fijar orden");

        set_game_collections(&mut connection, 10, &[first.id.clone(), second.id.clone()])
            .expect("actualizar pertenencias del juego");

        assert_eq!(
            planner_like_collection_ids(&connection, &first.id),
            vec![10, 20]
        );
        assert_eq!(
            planner_like_collection_ids(&connection, &second.id),
            vec![10]
        );
    }

    #[test]
    fn smart_collection_matches_grouped_rules_and_reports_real_count() {
        let mut connection = database();
        insert_game(&connection, 10, "Aventura instalada");
        insert_game(&connection, 20, "Aventura futura");
        insert_game(&connection, 30, "Juego rastreado");
        connection
            .execute(
                "UPDATE game_personal SET installed = 1, progress = 40 WHERE app_id = 10",
                [],
            )
            .expect("preparar instalado");
        connection
            .execute(
                "UPDATE game_personal SET installed = 1, progress = 5 WHERE app_id = 20",
                [],
            )
            .expect("preparar futuro");
        connection
            .execute(
                "UPDATE game_personal SET tracking = 1 WHERE app_id = 30",
                [],
            )
            .expect("preparar seguimiento");

        let smart = SaveCollectionInput {
            id: None,
            name: "En marcha".into(),
            description: String::new(),
            color: "#4B8FB5".into(),
            icon: "sparkles".into(),
            kind: "smart".into(),
            match_mode: "any".into(),
            rules: vec![
                SmartRule {
                    id: None,
                    group_id: 0,
                    field: "installed".into(),
                    operator: "isTrue".into(),
                    value: Value::Null,
                    position: 0,
                },
                SmartRule {
                    id: None,
                    group_id: 0,
                    field: "progress".into(),
                    operator: "greaterOrEqual".into(),
                    value: Value::from(25),
                    position: 1,
                },
                SmartRule {
                    id: None,
                    group_id: 1,
                    field: "tracking".into(),
                    operator: "isTrue".into(),
                    value: Value::Null,
                    position: 0,
                },
            ],
        };
        let collection = save_collection(&mut connection, &smart).expect("crear inteligente");

        let ids = collection_game_ids(&connection, &collection.id).expect("evaluar reglas");
        assert_eq!(ids, HashSet::from([10, 30]));
        assert_eq!(
            list_collections(&connection)
                .expect("listar colecciones")
                .into_iter()
                .find(|item| item.id == collection.id)
                .expect("encontrar colección")
                .game_count,
            2
        );
    }

    #[test]
    fn smart_preview_is_exact_and_never_mutates_collections() {
        let connection = database();
        insert_game(&connection, 10, "Instalado");
        insert_game(&connection, 20, "Sin instalar");
        connection
            .execute(
                "UPDATE game_personal SET installed = 1 WHERE app_id = 10",
                [],
            )
            .expect("marcar instalación");
        let input = SaveCollectionInput {
            id: None,
            name: "Previsualización".into(),
            description: String::new(),
            color: "#5CAAC1".into(),
            icon: "eye".into(),
            kind: "smart".into(),
            match_mode: "all".into(),
            rules: vec![SmartRule {
                id: None,
                group_id: 0,
                field: "installed".into(),
                operator: "isTrue".into(),
                value: Value::Null,
                position: 0,
            }],
        };
        let before: i64 = connection
            .query_row("SELECT COUNT(*) FROM collections", [], |row| row.get(0))
            .expect("contar antes");
        let matches = preview_smart_collection(&connection, &input).expect("previsualizar");
        let after: i64 = connection
            .query_row("SELECT COUNT(*) FROM collections", [], |row| row.get(0))
            .expect("contar después");
        assert_eq!(matches, HashSet::from([10]));
        assert_eq!(before, after);
    }

    #[test]
    fn planner_move_is_dense_and_enforces_wip_limit() {
        let mut connection = database();
        for app_id in 1..=4 {
            insert_game(&connection, app_id, &format!("Juego {app_id}"));
        }
        for (position, app_id) in [1_u32, 2, 3].iter().enumerate() {
            move_planner_item(
                &mut connection,
                &MovePlannerItemInput {
                    app_id: *app_id,
                    column_id: "playing".into(),
                    position: position as i64,
                },
            )
            .expect("llenar jugando");
        }
        let error = move_planner_item(
            &mut connection,
            &MovePlannerItemInput {
                app_id: 4,
                column_id: "playing".into(),
                position: 0,
            },
        )
        .expect_err("respetar límite WIP");
        assert_eq!(error.code, "validation");

        move_planner_item(
            &mut connection,
            &MovePlannerItemInput {
                app_id: 3,
                column_id: "next".into(),
                position: 0,
            },
        )
        .expect("mover entre columnas");
        let planner = list_planner(&connection).expect("listar planner");
        let playing = planner
            .iter()
            .find(|column| column.id == "playing")
            .unwrap();
        assert_eq!(
            playing
                .items
                .iter()
                .map(|item| (item.app_id, item.position))
                .collect::<Vec<_>>(),
            vec![(1, 0), (2, 1)]
        );
    }

    #[test]
    fn deleting_planner_column_rehomes_items_in_order() {
        let mut connection = database();
        insert_game(&connection, 10, "Primero");
        insert_game(&connection, 20, "Segundo");
        let custom = save_planner_column(
            &mut connection,
            None,
            "Semana siguiente",
            "#5CAAC1",
            Some(2),
        )
        .expect("crear columna");
        for (position, app_id) in [10_u32, 20].into_iter().enumerate() {
            move_planner_item(
                &mut connection,
                &MovePlannerItemInput {
                    app_id,
                    column_id: custom.id.clone(),
                    position: position as i64,
                },
            )
            .expect("añadir al planner");
        }

        delete_planner_column(&mut connection, &custom.id, Some("later"))
            .expect("conservar juegos al borrar");
        let planner = list_planner(&connection).expect("listar planner");
        assert!(planner.iter().all(|column| column.id != custom.id));
        let later = planner.iter().find(|column| column.id == "later").unwrap();
        assert_eq!(
            later
                .items
                .iter()
                .map(|item| item.app_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );
    }

    #[test]
    fn planner_queue_and_schedule_survive_column_moves() {
        let mut connection = database();
        insert_game(&connection, 10, "Aventura");
        insert_game(&connection, 20, "Estrategia");
        move_planner_item(
            &mut connection,
            &MovePlannerItemInput {
                app_id: 10,
                column_id: "playing".into(),
                position: 0,
            },
        )
        .expect("añadir primer juego");
        move_planner_item(
            &mut connection,
            &MovePlannerItemInput {
                app_id: 20,
                column_id: "next".into(),
                position: 0,
            },
        )
        .expect("añadir segundo juego");
        save_planner_item(
            &mut connection,
            &SavePlannerItemInput {
                app_id: 10,
                objective: Some("Llegar al epílogo".into()),
                planned_for: Some("2026-08-14".into()),
                target_date: Some("2026-08-20".into()),
                estimated_minutes: Some(600),
            },
        )
        .expect("guardar planificación");
        move_planner_queue_item(&mut connection, 20, 0).expect("reordenar cola");

        move_planner_item(
            &mut connection,
            &MovePlannerItemInput {
                app_id: 10,
                column_id: "later".into(),
                position: 0,
            },
        )
        .expect("mover conservando metadatos");

        let overview = planner_overview(&connection).expect("leer overview");
        assert_eq!(
            overview
                .queue
                .iter()
                .map(|item| item.app_id)
                .collect::<Vec<_>>(),
            vec![20, 10]
        );
        let planned = overview
            .queue
            .iter()
            .find(|item| item.app_id == 10)
            .expect("juego planificado");
        assert_eq!(planned.queue_position, 1);
        assert_eq!(planned.planned_for.as_deref(), Some("2026-08-14"));
        assert_eq!(planned.target_date.as_deref(), Some("2026-08-20"));
        assert_eq!(planned.estimated_minutes, Some(600));
        assert_eq!(planned.objective.as_deref(), Some("Llegar al epílogo"));
    }

    #[test]
    fn planner_capacity_is_persistent_and_rejects_an_impossible_month() {
        let mut connection = database();
        let saved = save_planner_settings(
            &mut connection,
            &PlannerSettings {
                weekly_capacity_minutes: 720,
                monthly_capacity_minutes: 2_880,
            },
        )
        .expect("guardar capacidad");
        assert_eq!(saved.weekly_capacity_minutes, 720);
        assert_eq!(
            planner_overview(&connection)
                .expect("leer capacidad")
                .settings
                .monthly_capacity_minutes,
            2_880
        );

        let error = save_planner_settings(
            &mut connection,
            &PlannerSettings {
                weekly_capacity_minutes: 900,
                monthly_capacity_minutes: 600,
            },
        )
        .expect_err("rechazar mes menor que semana");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn status_reorder_requires_complete_set_and_is_atomic() {
        let mut connection = database();
        let original = list_statuses(&connection)
            .expect("listar estados")
            .into_iter()
            .map(|status| status.id)
            .collect::<Vec<_>>();
        let mut reversed = original.clone();
        reversed.reverse();
        reorder_statuses(&mut connection, &reversed).expect("reordenar estados");
        assert_eq!(
            list_statuses(&connection)
                .expect("listar reordenados")
                .into_iter()
                .map(|status| status.id)
                .collect::<Vec<_>>(),
            reversed
        );

        let error = reorder_statuses(&mut connection, &["playing".into()])
            .expect_err("rechazar lista parcial");
        assert_eq!(error.code, "validation");
        assert_eq!(
            list_statuses(&connection)
                .expect("comprobar atomicidad")
                .into_iter()
                .map(|status| status.id)
                .collect::<Vec<_>>(),
            reversed
        );
    }

    #[test]
    fn preferences_round_trip_and_reject_unsafe_frequency() {
        let mut connection = database();
        let preferences = AppPreferences {
            density: "comfortable".into(),
            periodic_sync_minutes: 60,
            confirm_uninstall: false,
            library_sort: "lastPlayed".into(),
            art_cache_mib: crate::models::DEFAULT_ART_CACHE_MIB,
            shortcuts: Default::default(),
        };
        save_preferences(&mut connection, &preferences).expect("guardar preferencias");
        let loaded = load_preferences(&connection).expect("leer preferencias");
        assert_eq!(loaded.density, "comfortable");
        assert_eq!(loaded.periodic_sync_minutes, 60);
        assert!(!loaded.confirm_uninstall);
        assert_eq!(loaded.library_sort, "lastPlayed");

        let error = save_preferences(
            &mut connection,
            &AppPreferences {
                periodic_sync_minutes: 5,
                ..preferences
            },
        )
        .expect_err("rechazar frecuencia excesiva");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn preferences_reject_colliding_or_unmodified_shortcuts() {
        let mut connection = database();
        let mut preferences = AppPreferences {
            density: "compact".into(),
            periodic_sync_minutes: 0,
            confirm_uninstall: true,
            library_sort: "manual".into(),
            art_cache_mib: crate::models::DEFAULT_ART_CACHE_MIB,
            shortcuts: Default::default(),
        };
        preferences.shortcuts.planner = preferences.shortcuts.library.clone();
        let collision = save_preferences(&mut connection, &preferences)
            .expect_err("rechazar dos acciones con la misma combinación");
        assert_eq!(collision.code, "validation");

        preferences.shortcuts.planner = "P".into();
        let unmodified = save_preferences(&mut connection, &preferences)
            .expect_err("rechazar una letra sin modificadores");
        assert_eq!(unmodified.code, "validation");

        preferences.shortcuts.planner = "Mod+Comma".into();
        let reserved = save_preferences(&mut connection, &preferences)
            .expect_err("rechazar el atajo reservado para Ajustes");
        assert_eq!(reserved.code, "validation");
    }

    #[test]
    fn steam_resync_updates_metadata_without_overwriting_personal_organization() {
        let mut connection = database();
        let initial = ImportedGame {
            app_id: 730,
            title: "Título inicial".into(),
            icon_url: Some("https://example.invalid/icon-old.jpg".into()),
            cover_url: None,
            header_url: None,
            playtime_minutes: 120,
            playtime_recent_minutes: 10,
            last_played_at: Some("2026-08-01T10:00:00Z".into()),
            ownership_source: "owned".into(),
            family_availability: "not_applicable".into(),
            installation: None,
        };
        upsert_imported_games(&mut connection, &[initial], false).expect("primera importación");
        let collection = save_collection(&mut connection, &manual_collection("Intocables"))
            .expect("crear colección personal");
        set_collection_games(&mut connection, &collection.id, &[730]).expect("organizar juego");
        connection
            .execute(
                "UPDATE game_personal
                    SET status_id = 'playing', progress = 64, rating = 9,
                        notes = 'Decisión exclusivamente personal'
                  WHERE app_id = 730",
                [],
            )
            .expect("personalizar juego");

        let refreshed = ImportedGame {
            app_id: 730,
            title: "Título actualizado desde Steam".into(),
            icon_url: Some("https://example.invalid/icon-new.jpg".into()),
            cover_url: Some("https://example.invalid/cover.jpg".into()),
            header_url: None,
            playtime_minutes: 300,
            playtime_recent_minutes: 45,
            last_played_at: Some("2026-08-14T08:00:00Z".into()),
            ownership_source: "owned".into(),
            family_availability: "not_applicable".into(),
            installation: None,
        };
        let counts = upsert_imported_games(&mut connection, &[refreshed], false)
            .expect("resincronizar metadatos");
        assert_eq!(counts, (0, 1));

        let metadata: (String, i64) = connection
            .query_row(
                "SELECT title, playtime_minutes FROM games WHERE app_id = 730",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer metadatos actualizados");
        assert_eq!(metadata, ("Título actualizado desde Steam".into(), 300));
        let personal: (String, u8, u8, String) = connection
            .query_row(
                "SELECT status_id, progress, rating, notes
                   FROM game_personal WHERE app_id = 730",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("leer organización preservada");
        assert_eq!(
            personal,
            (
                "playing".into(),
                64,
                9,
                "Decisión exclusivamente personal".into()
            )
        );
        assert_eq!(
            planner_like_collection_ids(&connection, &collection.id),
            vec![730]
        );
    }

    fn planner_like_collection_ids(connection: &Connection, collection_id: &str) -> Vec<u32> {
        let mut statement = connection
            .prepare(
                "SELECT app_id FROM collection_games
                  WHERE collection_id = ?1 ORDER BY position ASC",
            )
            .expect("preparar lectura");
        statement
            .query_map([collection_id], |row| row.get(0))
            .expect("leer pertenencias")
            .collect::<Result<Vec<_>, _>>()
            .expect("materializar pertenencias")
    }
}
