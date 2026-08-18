//! Listas curadas (migración 021).
//!
//! Una lista curada es una selección editorial personal: la persona usuaria
//! decide qué juegos entran, en qué orden aparecen, qué nota acompaña a cada
//! uno y cuáles quedan destacados. No es lo mismo que una colección
//! (`db::organization`): las colecciones organizan la biblioteca completa y
//! pueden ser inteligentes; una lista curada es siempre manual y existe para
//! ser mostrada, no para clasificar.
//!
//! ## Fronteras con el resto de `db`
//!
//! - El mapeo de [`GameSummary`] pertenece a `db::library`. Aquí se invoca
//!   [`crate::db::library::game_summary`]; este módulo nunca reconstruye ese
//!   modelo ni duplica su SQL.
//! - La reordenación replica el enfoque de `db::library_dnd`: se lee el orden
//!   actual, se calcula el orden nuevo en memoria con [`place_before`] y se
//!   reescriben las posiciones dentro de una única transacción. `library_dnd`
//!   mantiene su propia copia porque además emite recibos de deshacer; ver el
//!   informe de integración para la consolidación propuesta.
//! - Las cascadas las garantiza el esquema (`ON DELETE CASCADE` sobre
//!   `curated_lists` y sobre `games`). Este módulo se limita a normalizar las
//!   posiciones después de cada borrado que sí controla.

use crate::error::{AppError, AppResult};
use crate::models::GameSummary;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

/// Longitud máxima del nombre de una lista curada.
pub const MAX_NAME_LENGTH: usize = 80;
/// Longitud máxima de la descripción de una lista curada.
pub const MAX_DESCRIPTION_LENGTH: usize = 1_000;
/// Longitud máxima de la nota que acompaña a un juego dentro de una lista.
pub const MAX_NOTE_LENGTH: usize = 500;
/// Longitud máxima del identificador de icono que resuelve la interfaz.
pub const MAX_ICON_LENGTH: usize = 64;
/// Número máximo de listas curadas que aceptamos crear.
pub const MAX_LISTS: usize = 200;
/// Número máximo de juegos que aceptamos dentro de una lista curada.
pub const MAX_ITEMS_PER_LIST: usize = 5_000;

/// Tipos válidos de lista curada; coinciden con el `CHECK` de la migración 021.
pub const CURATED_KINDS: [&str; 4] = ["manual", "wishlist", "backlog", "showcase"];

/// Acentos válidos. Son nombres semánticos: la interfaz los traduce a los
/// tokens de `DESIGN.md`. Se validan aquí para que ningún valor arbitrario
/// llegue a una clase CSS o a un `style` construido en el frontend.
pub const CURATED_ACCENTS: [&str; 8] = [
    "cyan", "blue", "teal", "lime", "amber", "rose", "violet", "slate",
];

const DEFAULT_PAGE_LIMIT: u32 = 60;
const MAX_PAGE_LIMIT: u32 = 200;

const CURATED_LIST_SELECT: &str = "
    SELECT l.id, l.name, l.description, l.kind, l.accent, l.icon,
           l.cover_app_id, l.pinned, l.position, l.created_at, l.updated_at,
           (SELECT COUNT(*) FROM curated_list_items ci WHERE ci.list_id = l.id),
           COALESCE(
             (SELECT COALESCE(cover.cover_url, cover.capsule_url, cover.header_url)
                FROM games cover
               WHERE cover.app_id = l.cover_app_id),
             (SELECT COALESCE(g.cover_url, g.capsule_url, g.header_url)
                FROM curated_list_items ci
                JOIN games g ON g.app_id = ci.app_id
               WHERE ci.list_id = l.id
                 AND COALESCE(g.cover_url, g.capsule_url, g.header_url) IS NOT NULL
               ORDER BY ci.position ASC, ci.app_id ASC
               LIMIT 1)
           )
      FROM curated_lists l";

// ---------------------------------------------------------------------------
// Modelos
// ---------------------------------------------------------------------------

/// Una lista curada con su recuento de juegos y su portada ya resuelta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedList {
    pub id: String,
    pub name: String,
    pub description: String,
    pub kind: String,
    pub accent: String,
    pub icon: String,
    /// Juego elegido explícitamente como portada, si lo hay.
    pub cover_app_id: Option<u32>,
    /// Portada resuelta: la del `cover_app_id` si existe y tiene arte; si no,
    /// la del primer juego de la lista que tenga arte.
    pub cover_url: Option<String>,
    pub pinned: bool,
    pub position: i64,
    pub game_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Alta o edición de una lista curada.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveCuratedListInput {
    /// Ausente al crear; presente al editar una lista existente.
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: String,
    pub accent: String,
    pub icon: String,
    #[serde(default)]
    pub cover_app_id: Option<u32>,
    #[serde(default)]
    pub pinned: bool,
}

/// Un juego dentro de una lista curada, con su nota y su marca de destacado.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedListEntry {
    pub game: GameSummary,
    pub note: String,
    pub highlight: bool,
    pub position: i64,
    pub added_at: String,
}

/// Página de una lista curada.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CuratedListDetail {
    pub list: CuratedList,
    pub items: Vec<CuratedListEntry>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

/// Añadir un juego a una lista curada.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AddCuratedGameInput {
    pub list_id: String,
    pub app_id: u32,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub highlight: bool,
    /// Juego ante el cual insertar. `None` coloca el juego al final.
    #[serde(default)]
    pub before_app_id: Option<u32>,
}

/// Editar la nota y el destacado de un juego ya presente en la lista.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCuratedItemInput {
    pub list_id: String,
    pub app_id: u32,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub highlight: bool,
}

// ---------------------------------------------------------------------------
// Listas
// ---------------------------------------------------------------------------

/// Devuelve todas las listas curadas con su recuento y su portada resuelta.
pub fn list_curated_lists(connection: &Connection) -> AppResult<Vec<CuratedList>> {
    let mut statement = connection.prepare(&format!(
        "{CURATED_LIST_SELECT} ORDER BY l.pinned DESC, l.position ASC, l.id ASC"
    ))?;
    let lists = statement
        .query_map([], map_curated_list)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(lists)
}

/// Devuelve una lista curada concreta.
pub fn curated_list(connection: &Connection, list_id: &str) -> AppResult<CuratedList> {
    let list_id = validate_identifier(list_id)?;
    connection
        .query_row(
            &format!("{CURATED_LIST_SELECT} WHERE l.id = ?1"),
            [list_id],
            map_curated_list,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La lista curada ya no existe."))
}

/// Crea o actualiza una lista curada dentro de una transacción.
pub fn save_curated_list(
    connection: &mut Connection,
    input: &SaveCuratedListInput,
) -> AppResult<CuratedList> {
    let name = validate_name(&input.name)?;
    let description = validate_description(&input.description)?;
    let kind = validate_choice(&input.kind, &CURATED_KINDS, "tipo de lista curada")?;
    let accent = validate_choice(&input.accent, &CURATED_ACCENTS, "acento de la lista curada")?;
    let icon = validate_icon(&input.icon)?;

    let transaction = connection.transaction()?;
    if let Some(cover_app_id) = input.cover_app_id {
        ensure_game_exists(&transaction, cover_app_id)?;
    }

    let list_id = match input.id.as_deref() {
        Some(existing) => validate_identifier(existing)?.to_string(),
        None => Uuid::new_v4().to_string(),
    };
    let exists = transaction
        .query_row(
            "SELECT 1 FROM curated_lists WHERE id = ?1",
            [&list_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if input.id.is_some() && !exists {
        return Err(AppError::not_found("La lista curada ya no existe."));
    }
    ensure_unique_name(&transaction, &name, &list_id)?;

    if exists {
        transaction.execute(
            "UPDATE curated_lists
                SET name = ?2, description = ?3, kind = ?4, accent = ?5, icon = ?6,
                    cover_app_id = ?7, pinned = ?8,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE id = ?1",
            params![
                list_id,
                name,
                description,
                kind,
                accent,
                icon,
                input.cover_app_id,
                i64::from(input.pinned),
            ],
        )?;
    } else {
        let total: i64 =
            transaction.query_row("SELECT COUNT(*) FROM curated_lists", [], |row| row.get(0))?;
        if usize::try_from(total).unwrap_or(usize::MAX) >= MAX_LISTS {
            return Err(AppError::validation(format!(
                "No puedes tener más de {MAX_LISTS} listas curadas. Borra alguna antes de crear otra."
            )));
        }
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM curated_lists",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO curated_lists(
                id, name, description, kind, accent, icon, cover_app_id, pinned, position
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                list_id,
                name,
                description,
                kind,
                accent,
                icon,
                input.cover_app_id,
                i64::from(input.pinned),
                position,
            ],
        )?;
    }
    transaction.commit()?;
    curated_list(connection, &list_id)
}

/// Borra una lista curada. Sus juegos caen por `ON DELETE CASCADE`.
pub fn delete_curated_list(connection: &mut Connection, list_id: &str) -> AppResult<()> {
    let list_id = validate_identifier(list_id)?;
    let transaction = connection.transaction()?;
    let deleted = transaction.execute("DELETE FROM curated_lists WHERE id = ?1", [list_id])?;
    if deleted != 1 {
        return Err(AppError::not_found("La lista curada ya no existe."));
    }
    normalize_list_positions(&transaction)?;
    transaction.commit()?;
    Ok(())
}

/// Reordena todas las listas curadas. `ordered_ids` debe contener exactamente
/// las listas guardadas, sin repeticiones ni ausencias.
///
/// El orden de lectura sigue siendo `pinned DESC, position ASC`: fijar una
/// lista la sube al principio sin perder la posición relativa que se guarda
/// aquí.
pub fn reorder_curated_lists(connection: &mut Connection, ordered_ids: &[String]) -> AppResult<()> {
    let transaction = connection.transaction()?;
    ensure_exact_list_set(&transaction, ordered_ids)?;
    write_list_positions(&transaction, ordered_ids)?;
    transaction.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Juegos dentro de una lista
// ---------------------------------------------------------------------------

/// Devuelve una página de la lista curada con el [`GameSummary`] de cada juego.
///
/// Solo se cuentan y se devuelven juegos que sigan teniendo registro personal:
/// es lo que necesita `db::library::game_summary` y así el total y la página
/// nunca se contradicen.
pub fn curated_list_detail(
    connection: &Connection,
    list_id: &str,
    limit: Option<u32>,
    offset: Option<u32>,
) -> AppResult<CuratedListDetail> {
    let list = curated_list(connection, list_id)?;
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let offset = offset.unwrap_or(0);

    let total: i64 = connection.query_row(
        "SELECT COUNT(*) FROM curated_list_items ci
          WHERE ci.list_id = ?1
            AND EXISTS (SELECT 1 FROM game_personal p WHERE p.app_id = ci.app_id)",
        [&list.id],
        |row| row.get(0),
    )?;

    let mut statement = connection.prepare(
        "SELECT ci.app_id, ci.note, ci.highlight, ci.position, ci.added_at
           FROM curated_list_items ci
          WHERE ci.list_id = ?1
            AND EXISTS (SELECT 1 FROM game_personal p WHERE p.app_id = ci.app_id)
          ORDER BY ci.position ASC, ci.app_id ASC
          LIMIT ?2 OFFSET ?3",
    )?;
    let rows = statement
        .query_map(params![list.id, limit, offset], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)? != 0,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let mut items = Vec::with_capacity(rows.len());
    for (app_id, note, highlight, position, added_at) in rows {
        items.push(CuratedListEntry {
            game: crate::db::library::game_summary(connection, app_id)?,
            note,
            highlight,
            position,
            added_at,
        });
    }

    Ok(CuratedListDetail {
        list,
        items,
        total,
        limit,
        offset,
    })
}

/// Añade un juego a una lista curada en la posición indicada.
pub fn add_curated_game(connection: &mut Connection, input: &AddCuratedGameInput) -> AppResult<()> {
    let list_id = validate_identifier(&input.list_id)?.to_string();
    let note = validate_note(&input.note)?;
    if input.before_app_id == Some(input.app_id) {
        return Err(AppError::validation(
            "Un juego no puede colocarse antes de sí mismo.",
        ));
    }

    let transaction = connection.transaction()?;
    ensure_list_exists(&transaction, &list_id)?;
    ensure_game_is_listable(&transaction, input.app_id)?;

    let previous = item_order(&transaction, &list_id)?;
    if previous.contains(&input.app_id) {
        return Err(AppError::validation(
            "Ese juego ya está en la lista curada. Muévelo o edita su nota en vez de añadirlo otra vez.",
        ));
    }
    if previous.len() >= MAX_ITEMS_PER_LIST {
        return Err(AppError::validation(format!(
            "Una lista curada no puede superar {MAX_ITEMS_PER_LIST} juegos."
        )));
    }

    transaction.execute(
        "INSERT INTO curated_list_items(list_id, app_id, position, note, highlight)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            list_id,
            input.app_id,
            previous.len() as i64,
            note,
            i64::from(input.highlight),
        ],
    )?;

    let next = place_before(
        &previous,
        &[input.app_id],
        input.before_app_id,
        ANCHOR_MISSING,
    )?;
    write_item_order(&transaction, &list_id, &next)?;
    touch_list(&transaction, &list_id)?;
    transaction.commit()?;
    Ok(())
}

/// Actualiza la nota y el destacado de un juego ya presente en la lista.
pub fn update_curated_item(
    connection: &mut Connection,
    input: &UpdateCuratedItemInput,
) -> AppResult<()> {
    let list_id = validate_identifier(&input.list_id)?.to_string();
    let note = validate_note(&input.note)?;
    let transaction = connection.transaction()?;
    let updated = transaction.execute(
        "UPDATE curated_list_items
            SET note = ?3, highlight = ?4
          WHERE list_id = ?1 AND app_id = ?2",
        params![list_id, input.app_id, note, i64::from(input.highlight)],
    )?;
    if updated != 1 {
        return Err(AppError::not_found(
            "Ese juego ya no está en la lista curada.",
        ));
    }
    touch_list(&transaction, &list_id)?;
    transaction.commit()?;
    Ok(())
}

/// Quita un juego de una lista curada y compacta las posiciones restantes.
pub fn remove_curated_game(
    connection: &mut Connection,
    list_id: &str,
    app_id: u32,
) -> AppResult<()> {
    let list_id = validate_identifier(list_id)?.to_string();
    let transaction = connection.transaction()?;
    let deleted = transaction.execute(
        "DELETE FROM curated_list_items WHERE list_id = ?1 AND app_id = ?2",
        params![list_id, app_id],
    )?;
    if deleted != 1 {
        return Err(AppError::not_found(
            "Ese juego ya no está en la lista curada.",
        ));
    }
    let remaining = item_order(&transaction, &list_id)?;
    write_item_order(&transaction, &list_id, &remaining)?;
    touch_list(&transaction, &list_id)?;
    transaction.commit()?;
    Ok(())
}

/// Mueve un juego dentro de su lista curada.
///
/// `before_app_id` es el juego ante el cual colocarlo; `None` lo lleva al
/// final. Todo el reordenamiento ocurre en una única transacción.
pub fn move_curated_item(
    connection: &mut Connection,
    list_id: &str,
    app_id: u32,
    before_app_id: Option<u32>,
) -> AppResult<()> {
    let list_id = validate_identifier(list_id)?.to_string();
    if before_app_id == Some(app_id) {
        return Err(AppError::validation(
            "Un juego no puede colocarse antes de sí mismo.",
        ));
    }
    let transaction = connection.transaction()?;
    let previous = item_order(&transaction, &list_id)?;
    if !previous.contains(&app_id) {
        return Err(AppError::not_found(
            "Ese juego ya no está en la lista curada.",
        ));
    }
    let next = place_before(&previous, &[app_id], before_app_id, ANCHOR_MISSING)?;
    write_item_order(&transaction, &list_id, &next)?;
    touch_list(&transaction, &list_id)?;
    transaction.commit()?;
    Ok(())
}

/// Reescribe el orden completo de los juegos de una lista curada.
///
/// `ordered_app_ids` debe contener exactamente los juegos guardados en la
/// lista. Una lista vacía es válida si la lista curada no tiene juegos.
pub fn reorder_curated_items(
    connection: &mut Connection,
    list_id: &str,
    ordered_app_ids: &[u32],
) -> AppResult<()> {
    let list_id = validate_identifier(list_id)?.to_string();
    let transaction = connection.transaction()?;
    ensure_list_exists(&transaction, &list_id)?;
    ensure_exact_item_set(&transaction, &list_id, ordered_app_ids)?;
    write_item_order(&transaction, &list_id, ordered_app_ids)?;
    touch_list(&transaction, &list_id)?;
    transaction.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Orden compartido
// ---------------------------------------------------------------------------

/// Mensaje de error cuando el ancla de inserción ya no existe en una lista.
const ANCHOR_MISSING: &str = "El juego de referencia ya no está en la lista curada.";

/// Calcula el orden resultante de mover `app_ids` justo antes de
/// `before_app_id`, o al final si no hay ancla.
///
/// Es la misma estrategia que usa `db::library_dnd::build_collection_order`
/// para las colecciones manuales: retirar la selección del orden previo,
/// localizar el ancla en el resto y reinsertar el bloque completo. Se comparte
/// entre `db::curated` y `db::wishlist` para no repetirla dos veces más.
pub(crate) fn place_before(
    previous_order: &[u32],
    app_ids: &[u32],
    before_app_id: Option<u32>,
    anchor_missing: &'static str,
) -> AppResult<Vec<u32>> {
    if app_ids.is_empty() {
        return Err(AppError::validation("Selecciona al menos un juego."));
    }
    let selected: HashSet<u32> = app_ids.iter().copied().collect();
    if selected.len() != app_ids.len() || selected.contains(&0) {
        return Err(AppError::validation(
            "La selección contiene juegos duplicados o no válidos.",
        ));
    }
    if before_app_id.is_some_and(|anchor| selected.contains(&anchor)) {
        return Err(AppError::validation(
            "Un juego no puede colocarse antes de sí mismo.",
        ));
    }
    let mut next = previous_order
        .iter()
        .copied()
        .filter(|app_id| !selected.contains(app_id))
        .collect::<Vec<_>>();
    let insert_at = match before_app_id {
        Some(anchor) => next
            .iter()
            .position(|app_id| *app_id == anchor)
            .ok_or_else(|| AppError::not_found(anchor_missing))?,
        None => next.len(),
    };
    next.splice(insert_at..insert_at, app_ids.iter().copied());
    Ok(next)
}

// ---------------------------------------------------------------------------
// Auxiliares privados
// ---------------------------------------------------------------------------

fn map_curated_list(row: &Row<'_>) -> rusqlite::Result<CuratedList> {
    Ok(CuratedList {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        kind: row.get(3)?,
        accent: row.get(4)?,
        icon: row.get(5)?,
        cover_app_id: row.get(6)?,
        pinned: row.get::<_, i64>(7)? != 0,
        position: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        game_count: row.get(11)?,
        cover_url: row.get(12)?,
    })
}

fn item_order(connection: &Connection, list_id: &str) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM curated_list_items
          WHERE list_id = ?1
          ORDER BY position ASC, app_id ASC",
    )?;
    let order = statement
        .query_map([list_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(order)
}

fn write_item_order(connection: &Connection, list_id: &str, app_ids: &[u32]) -> AppResult<()> {
    let mut update = connection.prepare_cached(
        "UPDATE curated_list_items SET position = ?3 WHERE list_id = ?1 AND app_id = ?2",
    )?;
    for (position, app_id) in app_ids.iter().enumerate() {
        if update.execute(params![list_id, app_id, position as i64])? != 1 {
            return Err(AppError::not_found(
                "Uno o más juegos ya no están en la lista curada.",
            ));
        }
    }
    Ok(())
}

fn write_list_positions(connection: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    let mut update = connection.prepare_cached(
        "UPDATE curated_lists
            SET position = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
    )?;
    for (position, id) in ordered_ids.iter().enumerate() {
        if update.execute(params![id, position as i64])? != 1 {
            return Err(AppError::not_found(
                "Una de las listas curadas ya no existe.",
            ));
        }
    }
    Ok(())
}

fn normalize_list_positions(connection: &Connection) -> AppResult<()> {
    let mut statement = connection.prepare(
        "SELECT id FROM curated_lists ORDER BY position ASC, name COLLATE NOCASE ASC, id ASC",
    )?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    write_list_positions(connection, &ids)
}

fn touch_list(connection: &Connection, list_id: &str) -> AppResult<()> {
    connection.execute(
        "UPDATE curated_lists
            SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        [list_id],
    )?;
    Ok(())
}

fn ensure_list_exists(connection: &Connection, list_id: &str) -> AppResult<()> {
    connection
        .query_row(
            "SELECT 1 FROM curated_lists WHERE id = ?1",
            [list_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La lista curada ya no existe."))
}

fn ensure_exact_list_set(connection: &Connection, ordered_ids: &[String]) -> AppResult<()> {
    let unique: HashSet<&String> = ordered_ids.iter().collect();
    if unique.len() != ordered_ids.len() {
        return Err(AppError::validation(
            "La lista de ordenación repite alguna lista curada.",
        ));
    }
    let mut statement = connection.prepare("SELECT id FROM curated_lists")?;
    let stored = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<HashSet<_>, _>>()?;
    drop(statement);
    let requested: HashSet<String> = ordered_ids.iter().cloned().collect();
    if stored != requested {
        return Err(AppError::validation(
            "La lista de ordenación no coincide con las listas curadas guardadas.",
        ));
    }
    Ok(())
}

fn ensure_exact_item_set(
    connection: &Connection,
    list_id: &str,
    ordered_app_ids: &[u32],
) -> AppResult<()> {
    let unique: HashSet<u32> = ordered_app_ids.iter().copied().collect();
    if unique.len() != ordered_app_ids.len() || unique.contains(&0) {
        return Err(AppError::validation(
            "La lista de ordenación contiene juegos duplicados o no válidos.",
        ));
    }
    let stored: HashSet<u32> = item_order(connection, list_id)?.into_iter().collect();
    if stored != unique {
        return Err(AppError::validation(
            "La lista de ordenación no coincide con los juegos guardados en la lista curada.",
        ));
    }
    Ok(())
}

fn ensure_unique_name(connection: &Connection, name: &str, list_id: &str) -> AppResult<()> {
    if connection
        .query_row(
            "SELECT 1 FROM curated_lists WHERE name = ?1 COLLATE NOCASE AND id <> ?2",
            params![name, list_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Err(AppError::validation(
            "Ya existe una lista curada con ese nombre. Elige otro para distinguirlas.",
        ));
    }
    Ok(())
}

fn ensure_game_exists(connection: &Connection, app_id: u32) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El juego indicado no es válido."));
    }
    connection
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("El juego {app_id} no está en la biblioteca.")))
}

/// Comprueba que el juego exista y tenga registro personal: sin él,
/// `db::library::game_summary` no puede construir su [`GameSummary`].
pub(crate) fn ensure_game_is_listable(connection: &Connection, app_id: u32) -> AppResult<()> {
    ensure_game_exists(connection, app_id)?;
    connection
        .query_row(
            "SELECT 1 FROM game_personal WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::not_found(format!(
                "El juego {app_id} todavía no tiene ficha personal en la biblioteca."
            ))
        })
}

pub(crate) fn validate_identifier(value: &str) -> AppResult<&str> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 {
        return Err(AppError::validation(
            "El identificador indicado no es válido.",
        ));
    }
    Ok(value)
}

fn validate_name(value: &str) -> AppResult<String> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > MAX_NAME_LENGTH {
        return Err(AppError::validation(format!(
            "El nombre de la lista curada debe tener entre 1 y {MAX_NAME_LENGTH} caracteres."
        )));
    }
    Ok(value.to_string())
}

fn validate_description(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.chars().count() > MAX_DESCRIPTION_LENGTH {
        return Err(AppError::validation(format!(
            "La descripción de la lista curada no puede superar {MAX_DESCRIPTION_LENGTH} caracteres."
        )));
    }
    Ok(value.to_string())
}

pub(crate) fn validate_note(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.chars().count() > MAX_NOTE_LENGTH {
        return Err(AppError::validation(format!(
            "La nota no puede superar {MAX_NOTE_LENGTH} caracteres."
        )));
    }
    Ok(value.to_string())
}

fn validate_icon(value: &str) -> AppResult<String> {
    let value = value.trim();
    let length = value.chars().count();
    if length == 0 || length > MAX_ICON_LENGTH {
        return Err(AppError::validation(format!(
            "El icono de la lista curada debe tener entre 1 y {MAX_ICON_LENGTH} caracteres."
        )));
    }
    Ok(value.to_string())
}

pub(crate) fn validate_choice(value: &str, allowed: &[&str], label: &str) -> AppResult<String> {
    let value = value.trim();
    if allowed.contains(&value) {
        return Ok(value.to_string());
    }
    Err(AppError::validation(format!(
        "El {label} no es válido. Valores admitidos: {}.",
        allowed.join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrations, seed_defaults};

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar valores por defecto");
        connection
    }

    fn insert_game(connection: &Connection, app_id: u32) {
        connection
            .execute(
                "INSERT INTO games(app_id, title, cover_url) VALUES (?1, ?2, ?3)",
                params![
                    app_id,
                    format!("Juego {app_id}"),
                    format!("https://cdn.example/{app_id}.jpg")
                ],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar ficha personal");
    }

    fn list_input(name: &str) -> SaveCuratedListInput {
        SaveCuratedListInput {
            id: None,
            name: name.to_string(),
            description: String::new(),
            kind: "manual".to_string(),
            accent: "cyan".to_string(),
            icon: "list".to_string(),
            cover_app_id: None,
            pinned: false,
        }
    }

    fn add(connection: &mut Connection, list_id: &str, app_id: u32) {
        add_curated_game(
            connection,
            &AddCuratedGameInput {
                list_id: list_id.to_string(),
                app_id,
                note: String::new(),
                highlight: false,
                before_app_id: None,
            },
        )
        .expect("añadir juego a la lista curada");
    }

    fn order(connection: &Connection, list_id: &str) -> Vec<u32> {
        item_order(connection, list_id).expect("leer orden")
    }

    #[test]
    fn rejects_invalid_names_kinds_accents_and_icons() {
        let mut connection = database();

        let empty = save_curated_list(&mut connection, &list_input("   "))
            .expect_err("rechazar nombre vacío");
        assert_eq!(empty.code, "validation");
        assert!(empty.message.contains("nombre"));

        let long_name = "n".repeat(MAX_NAME_LENGTH + 1);
        let too_long = save_curated_list(&mut connection, &list_input(&long_name))
            .expect_err("rechazar nombre largo");
        assert_eq!(too_long.code, "validation");

        let mut input = list_input("Imprescindibles");
        input.description = "d".repeat(MAX_DESCRIPTION_LENGTH + 1);
        let long_description =
            save_curated_list(&mut connection, &input).expect_err("rechazar descripción larga");
        assert_eq!(long_description.code, "validation");

        let mut input = list_input("Imprescindibles");
        input.kind = "inventado".to_string();
        let kind = save_curated_list(&mut connection, &input).expect_err("rechazar tipo");
        assert_eq!(kind.code, "validation");
        assert!(kind.message.contains("showcase"));

        let mut input = list_input("Imprescindibles");
        input.accent = "#ff0000".to_string();
        let accent = save_curated_list(&mut connection, &input).expect_err("rechazar acento");
        assert_eq!(accent.code, "validation");

        let mut input = list_input("Imprescindibles");
        input.icon = String::new();
        let icon = save_curated_list(&mut connection, &input).expect_err("rechazar icono");
        assert_eq!(icon.code, "validation");
    }

    #[test]
    fn names_are_unique_ignoring_case_and_editing_keeps_its_own_name() {
        let mut connection = database();
        let first =
            save_curated_list(&mut connection, &list_input("Para el finde")).expect("crear lista");

        let duplicate = save_curated_list(&mut connection, &list_input("PARA EL FINDE"))
            .expect_err("rechazar nombre duplicado");
        assert_eq!(duplicate.code, "validation");
        assert!(duplicate.message.contains("Ya existe una lista curada"));

        let mut edit = list_input("Para el finde");
        edit.id = Some(first.id.clone());
        edit.description = "Sesiones cortas".to_string();
        let updated = save_curated_list(&mut connection, &edit).expect("editar sin cambiar nombre");
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.description, "Sesiones cortas");

        let mut missing = list_input("Fantasma");
        missing.id = Some("no-existe".to_string());
        let error = save_curated_list(&mut connection, &missing).expect_err("rechazar id ausente");
        assert_eq!(error.code, "not_found");
    }

    #[test]
    fn resolves_the_cover_from_the_explicit_game_or_the_first_item() {
        let mut connection = database();
        insert_game(&connection, 10);
        insert_game(&connection, 20);
        let list = save_curated_list(&mut connection, &list_input("Con portada")).expect("crear");

        assert!(
            curated_list(&connection, &list.id)
                .unwrap()
                .cover_url
                .is_none()
        );

        add(&mut connection, &list.id, 20);
        add(&mut connection, &list.id, 10);
        let derived = curated_list(&connection, &list.id).expect("leer lista");
        assert_eq!(derived.game_count, 2);
        assert_eq!(
            derived.cover_url.as_deref(),
            Some("https://cdn.example/20.jpg")
        );

        let mut explicit = list_input("Con portada");
        explicit.id = Some(list.id.clone());
        explicit.cover_app_id = Some(10);
        save_curated_list(&mut connection, &explicit).expect("fijar portada explícita");
        assert_eq!(
            curated_list(&connection, &list.id)
                .unwrap()
                .cover_url
                .as_deref(),
            Some("https://cdn.example/10.jpg")
        );

        let mut unknown = list_input("Con portada");
        unknown.id = Some(list.id.clone());
        unknown.cover_app_id = Some(999);
        let error = save_curated_list(&mut connection, &unknown)
            .expect_err("rechazar portada de un juego inexistente");
        assert_eq!(error.code, "not_found");
    }

    #[test]
    fn reorders_lists_only_with_the_exact_stored_set() {
        let mut connection = database();
        let first = save_curated_list(&mut connection, &list_input("Uno")).expect("crear uno");
        let second = save_curated_list(&mut connection, &list_input("Dos")).expect("crear dos");
        let third = save_curated_list(&mut connection, &list_input("Tres")).expect("crear tres");

        reorder_curated_lists(
            &mut connection,
            &[third.id.clone(), first.id.clone(), second.id.clone()],
        )
        .expect("reordenar");
        assert_eq!(
            list_curated_lists(&connection)
                .unwrap()
                .iter()
                .map(|list| list.name.clone())
                .collect::<Vec<_>>(),
            vec!["Tres", "Uno", "Dos"]
        );

        let incomplete = reorder_curated_lists(&mut connection, std::slice::from_ref(&first.id))
            .expect_err("rechazar orden incompleto");
        assert_eq!(incomplete.code, "validation");

        let repeated =
            reorder_curated_lists(&mut connection, &[first.id.clone(), first.id.clone()])
                .expect_err("rechazar repetidos");
        assert_eq!(repeated.code, "validation");
    }

    #[test]
    fn pinned_lists_lead_the_order_without_losing_their_position() {
        let mut connection = database();
        let first = save_curated_list(&mut connection, &list_input("Uno")).expect("crear uno");
        let second = save_curated_list(&mut connection, &list_input("Dos")).expect("crear dos");

        let mut pin = list_input("Dos");
        pin.id = Some(second.id.clone());
        pin.pinned = true;
        save_curated_list(&mut connection, &pin).expect("fijar la segunda");

        let names = list_curated_lists(&connection)
            .unwrap()
            .iter()
            .map(|list| list.name.clone())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["Dos", "Uno"]);
        assert_eq!(
            curated_list(&connection, &first.id).unwrap().position,
            0,
            "fijar no debe mover la posición guardada"
        );
    }

    #[test]
    fn deleting_a_list_compacts_positions_and_removes_its_items() {
        let mut connection = database();
        insert_game(&connection, 10);
        let first = save_curated_list(&mut connection, &list_input("Uno")).expect("crear uno");
        let second = save_curated_list(&mut connection, &list_input("Dos")).expect("crear dos");
        add(&mut connection, &first.id, 10);

        delete_curated_list(&mut connection, &first.id).expect("borrar la primera");
        assert_eq!(curated_list(&connection, &second.id).unwrap().position, 0);
        let orphan: i64 = connection
            .query_row("SELECT COUNT(*) FROM curated_list_items", [], |row| {
                row.get(0)
            })
            .expect("contar entradas");
        assert_eq!(orphan, 0, "la cascada debe llevarse las entradas");

        let missing =
            delete_curated_list(&mut connection, &first.id).expect_err("rechazar borrado repetido");
        assert_eq!(missing.code, "not_found");
    }

    #[test]
    fn deleting_a_game_removes_it_from_every_curated_list() {
        let mut connection = database();
        insert_game(&connection, 10);
        insert_game(&connection, 20);
        let list = save_curated_list(&mut connection, &list_input("Cascada")).expect("crear");
        add(&mut connection, &list.id, 10);
        add(&mut connection, &list.id, 20);

        connection
            .execute("DELETE FROM games WHERE app_id = 10", [])
            .expect("borrar juego");
        assert_eq!(order(&connection, &list.id), vec![20]);
        assert_eq!(curated_list(&connection, &list.id).unwrap().game_count, 1);
    }

    #[test]
    fn adds_games_with_note_and_highlight_and_rejects_duplicates() {
        let mut connection = database();
        insert_game(&connection, 10);
        let list = save_curated_list(&mut connection, &list_input("Notas")).expect("crear");

        add_curated_game(
            &mut connection,
            &AddCuratedGameInput {
                list_id: list.id.clone(),
                app_id: 10,
                note: "  Empezar por el DLC  ".to_string(),
                highlight: true,
                before_app_id: None,
            },
        )
        .expect("añadir con nota");

        let detail = curated_list_detail(&connection, &list.id, None, None).expect("detalle");
        assert_eq!(detail.total, 1);
        assert_eq!(detail.items[0].note, "Empezar por el DLC");
        assert!(detail.items[0].highlight);
        assert_eq!(detail.items[0].game.app_id, 10);
        assert_eq!(detail.items[0].game.title, "Juego 10");

        let duplicate = add_curated_game(
            &mut connection,
            &AddCuratedGameInput {
                list_id: list.id.clone(),
                app_id: 10,
                ..AddCuratedGameInput::default()
            },
        )
        .expect_err("rechazar duplicado");
        assert_eq!(duplicate.code, "validation");

        let long_note = add_curated_game(
            &mut connection,
            &AddCuratedGameInput {
                list_id: list.id.clone(),
                app_id: 10,
                note: "n".repeat(MAX_NOTE_LENGTH + 1),
                ..AddCuratedGameInput::default()
            },
        )
        .expect_err("rechazar nota larga");
        assert_eq!(long_note.code, "validation");

        let unknown_game = add_curated_game(
            &mut connection,
            &AddCuratedGameInput {
                list_id: list.id.clone(),
                app_id: 999,
                ..AddCuratedGameInput::default()
            },
        )
        .expect_err("rechazar juego desconocido");
        assert_eq!(unknown_game.code, "not_found");

        let unknown_list = add_curated_game(
            &mut connection,
            &AddCuratedGameInput {
                list_id: "no-existe".to_string(),
                app_id: 10,
                ..AddCuratedGameInput::default()
            },
        )
        .expect_err("rechazar lista desconocida");
        assert_eq!(unknown_list.code, "not_found");
    }

    #[test]
    fn updates_and_removes_items_compacting_positions() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        let list = save_curated_list(&mut connection, &list_input("Editable")).expect("crear");
        for app_id in [10, 20, 30] {
            add(&mut connection, &list.id, app_id);
        }

        update_curated_item(
            &mut connection,
            &UpdateCuratedItemInput {
                list_id: list.id.clone(),
                app_id: 20,
                note: "Rejugar el capítulo 3".to_string(),
                highlight: true,
            },
        )
        .expect("editar entrada");

        let missing = update_curated_item(
            &mut connection,
            &UpdateCuratedItemInput {
                list_id: list.id.clone(),
                app_id: 999,
                ..UpdateCuratedItemInput::default()
            },
        )
        .expect_err("rechazar entrada ausente");
        assert_eq!(missing.code, "not_found");

        remove_curated_game(&mut connection, &list.id, 10).expect("quitar el primero");
        assert_eq!(order(&connection, &list.id), vec![20, 30]);
        let positions: Vec<i64> = connection
            .prepare("SELECT position FROM curated_list_items WHERE list_id = ?1 ORDER BY position")
            .unwrap()
            .query_map([&list.id], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(positions, vec![0, 1], "las posiciones quedan compactadas");

        let detail = curated_list_detail(&connection, &list.id, None, None).expect("detalle");
        assert_eq!(detail.items[0].note, "Rejugar el capítulo 3");
        assert!(detail.items[0].highlight);
    }

    #[test]
    fn moves_items_to_the_start_the_middle_and_the_end() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        let list = save_curated_list(&mut connection, &list_input("Movimiento")).expect("crear");
        for app_id in [10, 20, 30] {
            add(&mut connection, &list.id, app_id);
        }
        assert_eq!(order(&connection, &list.id), vec![10, 20, 30]);

        move_curated_item(&mut connection, &list.id, 30, Some(10)).expect("mover al principio");
        assert_eq!(order(&connection, &list.id), vec![30, 10, 20]);

        move_curated_item(&mut connection, &list.id, 30, None).expect("mover al final");
        assert_eq!(order(&connection, &list.id), vec![10, 20, 30]);

        move_curated_item(&mut connection, &list.id, 10, Some(30)).expect("mover al medio");
        assert_eq!(order(&connection, &list.id), vec![20, 10, 30]);

        let itself = move_curated_item(&mut connection, &list.id, 10, Some(10))
            .expect_err("rechazar moverse ante sí mismo");
        assert_eq!(itself.code, "validation");

        let anchor = move_curated_item(&mut connection, &list.id, 10, Some(999))
            .expect_err("rechazar ancla desconocida");
        assert_eq!(anchor.code, "not_found");

        let absent = move_curated_item(&mut connection, &list.id, 999, None)
            .expect_err("rechazar juego ausente");
        assert_eq!(absent.code, "not_found");
    }

    #[test]
    fn adds_before_an_anchor_and_handles_single_and_empty_lists() {
        let mut connection = database();
        for app_id in [10, 20] {
            insert_game(&connection, app_id);
        }
        let list = save_curated_list(&mut connection, &list_input("Anclas")).expect("crear");

        reorder_curated_items(&mut connection, &list.id, &[]).expect("reordenar lista vacía");

        add(&mut connection, &list.id, 20);
        reorder_curated_items(&mut connection, &list.id, &[20]).expect("reordenar un solo juego");
        assert_eq!(order(&connection, &list.id), vec![20]);

        add_curated_game(
            &mut connection,
            &AddCuratedGameInput {
                list_id: list.id.clone(),
                app_id: 10,
                before_app_id: Some(20),
                ..AddCuratedGameInput::default()
            },
        )
        .expect("insertar antes del ancla");
        assert_eq!(order(&connection, &list.id), vec![10, 20]);
    }

    #[test]
    fn reorder_requires_the_exact_item_set() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        let list = save_curated_list(&mut connection, &list_input("Estricta")).expect("crear");
        for app_id in [10, 20, 30] {
            add(&mut connection, &list.id, app_id);
        }

        reorder_curated_items(&mut connection, &list.id, &[30, 20, 10])
            .expect("reordenar completo");
        assert_eq!(order(&connection, &list.id), vec![30, 20, 10]);

        let partial = reorder_curated_items(&mut connection, &list.id, &[30, 20])
            .expect_err("rechazar orden parcial");
        assert_eq!(partial.code, "validation");

        let repeated = reorder_curated_items(&mut connection, &list.id, &[10, 10, 20])
            .expect_err("rechazar repetidos");
        assert_eq!(repeated.code, "validation");

        let foreign = reorder_curated_items(&mut connection, &list.id, &[10, 20, 999])
            .expect_err("rechazar juego ajeno");
        assert_eq!(foreign.code, "validation");

        assert_eq!(
            order(&connection, &list.id),
            vec![30, 20, 10],
            "un orden rechazado no debe alterar la lista"
        );
    }

    #[test]
    fn paginates_the_detail_and_clamps_the_limit() {
        let mut connection = database();
        for app_id in [10, 20, 30, 40] {
            insert_game(&connection, app_id);
        }
        let list = save_curated_list(&mut connection, &list_input("Paginada")).expect("crear");
        for app_id in [10, 20, 30, 40] {
            add(&mut connection, &list.id, app_id);
        }

        let first = curated_list_detail(&connection, &list.id, Some(2), Some(0)).expect("página 1");
        assert_eq!(first.total, 4);
        assert_eq!(first.limit, 2);
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.game.app_id)
                .collect::<Vec<_>>(),
            vec![10, 20]
        );

        let second =
            curated_list_detail(&connection, &list.id, Some(2), Some(2)).expect("página 2");
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.game.app_id)
                .collect::<Vec<_>>(),
            vec![30, 40]
        );

        let clamped = curated_list_detail(&connection, &list.id, Some(0), None).expect("límite 0");
        assert_eq!(clamped.limit, 1);
        let capped =
            curated_list_detail(&connection, &list.id, Some(10_000), None).expect("límite alto");
        assert_eq!(capped.limit, MAX_PAGE_LIMIT);

        let missing = curated_list_detail(&connection, "no-existe", None, None)
            .expect_err("rechazar lista desconocida");
        assert_eq!(missing.code, "not_found");
    }

    #[test]
    fn place_before_covers_its_edge_cases() {
        assert_eq!(
            place_before(&[], &[10], None, ANCHOR_MISSING).unwrap(),
            vec![10]
        );
        assert_eq!(
            place_before(&[10], &[10], None, ANCHOR_MISSING).unwrap(),
            vec![10]
        );
        assert_eq!(
            place_before(&[10, 20, 30], &[20, 30], Some(10), ANCHOR_MISSING).unwrap(),
            vec![20, 30, 10]
        );
        assert_eq!(
            place_before(&[10, 20, 30], &[10], None, ANCHOR_MISSING).unwrap(),
            vec![20, 30, 10]
        );
        assert_eq!(
            place_before(&[10, 20, 30], &[20], Some(10), ANCHOR_MISSING).unwrap(),
            vec![20, 10, 30]
        );
        assert_eq!(
            place_before(&[], &[], None, ANCHOR_MISSING)
                .expect_err("rechazar selección vacía")
                .code,
            "validation"
        );
        assert_eq!(
            place_before(&[10], &[10, 10], None, ANCHOR_MISSING)
                .expect_err("rechazar duplicados")
                .code,
            "validation"
        );
        assert_eq!(
            place_before(&[10, 20], &[10], Some(999), ANCHOR_MISSING)
                .expect_err("rechazar ancla ausente")
                .code,
            "not_found"
        );
    }
}
