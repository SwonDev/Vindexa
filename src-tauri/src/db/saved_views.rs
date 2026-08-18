//! Vistas guardadas de biblioteca (migración 028).
//!
//! Una vista congela una combinación completa de consulta bajo un nombre. La
//! diferencia con los presets de otras aplicaciones es que **varias vistas se
//! pueden combinar**: la interfaz interseca sus filtros en lugar de sustituir
//! uno por otro. Aquí solo se guarda la instantánea; combinar es una decisión de
//! presentación y vive en el frontend.
//!
//! El contenido de `query_json` es opaco para Rust a propósito: el conjunto de
//! filtros de la biblioteca crece con el producto, y una columna por filtro
//! obligaría a migrar el esquema en cada añadido. Lo que sí se valida aquí es
//! que sea un objeto JSON y que no supere un tamaño razonable.

use crate::error::{AppError, AppResult};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_NAME_LENGTH: usize = 60;
pub const MAX_DESCRIPTION_LENGTH: usize = 240;
pub const MAX_ICON_LENGTH: usize = 32;
pub const MAX_QUERY_BYTES: usize = 8 * 1024;
pub const MAX_SAVED_VIEWS: usize = 100;

/// Acentos admitidos, los mismos que las listas curadas para no inventar una
/// segunda paleta paralela.
pub const SAVED_VIEW_ACCENTS: [&str; 8] = [
    "cyan", "blue", "teal", "lime", "amber", "rose", "violet", "slate",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub icon: String,
    pub accent: String,
    /// Instantánea de la consulta, tal cual la entiende la interfaz.
    pub query: serde_json::Value,
    pub pinned: bool,
    pub position: i64,
    pub last_used_at: Option<String>,
    pub use_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveViewInput {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub accent: String,
    #[serde(default)]
    pub query: serde_json::Value,
    #[serde(default)]
    pub pinned: bool,
}

const SELECT: &str = "SELECT id, name, description, icon, accent, query_json, pinned, position,
                             last_used_at, use_count, created_at, updated_at
                        FROM saved_views";

fn map(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedView> {
    let raw: String = row.get(5)?;
    Ok(SavedView {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        icon: row.get(3)?,
        accent: row.get(4)?,
        // Un JSON corrupto degrada a vista vacía en lugar de tumbar la lista
        // entera: la persona puede volver a guardarla desde la interfaz.
        query: serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({})),
        pinned: row.get::<_, i64>(6)? != 0,
        position: row.get(7)?,
        last_used_at: row.get(8)?,
        use_count: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn validate(input: &SaveViewInput) -> AppResult<(String, String, String)> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::validation("La vista necesita un nombre."));
    }
    if name.chars().count() > MAX_NAME_LENGTH {
        return Err(AppError::validation(format!(
            "El nombre de la vista no puede superar {MAX_NAME_LENGTH} caracteres."
        )));
    }
    if input.description.chars().count() > MAX_DESCRIPTION_LENGTH {
        return Err(AppError::validation(format!(
            "La descripción no puede superar {MAX_DESCRIPTION_LENGTH} caracteres."
        )));
    }
    let icon = if input.icon.trim().is_empty() {
        "bookmark".to_string()
    } else {
        input.icon.trim().to_string()
    };
    if icon.chars().count() > MAX_ICON_LENGTH {
        return Err(AppError::validation("El icono indicado no es válido."));
    }
    let accent = if input.accent.trim().is_empty() {
        "cyan".to_string()
    } else {
        input.accent.trim().to_string()
    };
    if !SAVED_VIEW_ACCENTS.contains(&accent.as_str()) {
        return Err(AppError::validation(
            "El acento de la vista no está entre los admitidos.",
        ));
    }
    if !input.query.is_object() {
        return Err(AppError::validation(
            "La consulta guardada debe ser un objeto.",
        ));
    }
    let serialized = serde_json::to_string(&input.query)
        .map_err(|_| AppError::validation("La consulta guardada no se pudo serializar."))?;
    if serialized.len() > MAX_QUERY_BYTES {
        return Err(AppError::validation(
            "La consulta guardada es demasiado grande.",
        ));
    }
    Ok((name.to_string(), icon, accent))
}

pub fn list(connection: &Connection) -> AppResult<Vec<SavedView>> {
    let mut statement = connection.prepare(&format!(
        "{SELECT} ORDER BY pinned DESC, position ASC, name COLLATE NOCASE ASC"
    ))?;
    let views = statement
        .query_map([], map)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(views)
}

pub fn get(connection: &Connection, id: &str) -> AppResult<SavedView> {
    connection
        .query_row(&format!("{SELECT} WHERE id = ?1"), [id], map)
        .optional()?
        .ok_or_else(|| AppError::not_found("Esa vista guardada ya no existe."))
}

pub fn save(connection: &mut Connection, input: &SaveViewInput) -> AppResult<SavedView> {
    let (name, icon, accent) = validate(input)?;
    let serialized = serde_json::to_string(&input.query)
        .map_err(|_| AppError::validation("La consulta guardada no se pudo serializar."))?;
    let transaction = connection.transaction()?;

    let duplicate: Option<String> = transaction
        .query_row(
            "SELECT id FROM saved_views WHERE name = ?1 COLLATE NOCASE",
            [&name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = duplicate
        && input.id.as_deref() != Some(existing.as_str())
    {
        return Err(AppError::validation(
            "Ya existe una vista con ese nombre. Elige otro.",
        ));
    }

    let id = match input.id.as_deref() {
        Some(existing) => {
            let updated = transaction.execute(
                "UPDATE saved_views
                    SET name = ?2, description = ?3, icon = ?4, accent = ?5,
                        query_json = ?6, pinned = ?7,
                        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  WHERE id = ?1",
                params![
                    existing,
                    name,
                    input.description.trim(),
                    icon,
                    accent,
                    serialized,
                    i64::from(input.pinned)
                ],
            )?;
            if updated == 0 {
                return Err(AppError::not_found("Esa vista guardada ya no existe."));
            }
            existing.to_string()
        }
        None => {
            let total: i64 =
                transaction.query_row("SELECT COUNT(*) FROM saved_views", [], |row| row.get(0))?;
            if total as usize >= MAX_SAVED_VIEWS {
                return Err(AppError::validation(format!(
                    "No puedes guardar más de {MAX_SAVED_VIEWS} vistas."
                )));
            }
            let id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO saved_views(id, name, description, icon, accent, query_json, pinned, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    id,
                    name,
                    input.description.trim(),
                    icon,
                    accent,
                    serialized,
                    i64::from(input.pinned),
                    total
                ],
            )?;
            id
        }
    };
    transaction.commit()?;
    get(connection, &id)
}

pub fn delete(connection: &mut Connection, id: &str) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let removed = transaction.execute("DELETE FROM saved_views WHERE id = ?1", [id])?;
    if removed == 0 {
        return Err(AppError::not_found("Esa vista guardada ya no existe."));
    }
    // Las posiciones se compactan para que reordenar siga siendo determinista.
    transaction.execute(
        "UPDATE saved_views
            SET position = (
                SELECT COUNT(*) FROM saved_views older
                 WHERE older.position < saved_views.position
                    OR (older.position = saved_views.position AND older.id < saved_views.id)
            )",
        [],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn reorder(connection: &mut Connection, ordered_ids: &[String]) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let mut statement = transaction.prepare("SELECT id FROM saved_views")?;
    let existing = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    drop(statement);
    if existing.len() != ordered_ids.len()
        || !ordered_ids.iter().all(|id| existing.contains(id))
        || ordered_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != ordered_ids.len()
    {
        return Err(AppError::validation(
            "El nuevo orden debe contener exactamente las vistas guardadas.",
        ));
    }
    for (position, id) in ordered_ids.iter().enumerate() {
        transaction.execute(
            "UPDATE saved_views
                SET position = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE id = ?1",
            params![id, position as i64],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

/// Registra que una vista se ha aplicado. Sirve para ordenar por uso reciente
/// sin que la persona tenga que mantener el orden a mano.
pub fn mark_used(connection: &Connection, id: &str) -> AppResult<SavedView> {
    let updated = connection.execute(
        "UPDATE saved_views
            SET use_count = use_count + 1,
                last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1",
        [id],
    )?;
    if updated == 0 {
        return Err(AppError::not_found("Esa vista guardada ya no existe."));
    }
    get(connection, id)
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

    fn input(name: &str) -> SaveViewInput {
        SaveViewInput {
            id: None,
            name: name.to_string(),
            description: String::new(),
            icon: String::new(),
            accent: String::new(),
            query: serde_json::json!({ "sort": "recent", "statusIds": ["playing"] }),
            pinned: false,
        }
    }

    #[test]
    fn guarda_y_recupera_una_vista_completa() {
        let mut connection = database();
        let saved = save(&mut connection, &input("En curso")).expect("guardar");
        assert_eq!(saved.name, "En curso");
        // Los valores omitidos caen en el predeterminado, no en cadena vacía.
        assert_eq!(saved.icon, "bookmark");
        assert_eq!(saved.accent, "cyan");
        assert_eq!(saved.query["sort"], "recent");
        assert_eq!(saved.use_count, 0);
        assert!(saved.last_used_at.is_none());

        let listed = list(&connection).expect("listar");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, saved.id);
    }

    #[test]
    fn rechaza_nombres_duplicados_sin_distinguir_mayusculas() {
        let mut connection = database();
        save(&mut connection, &input("Pendientes")).expect("guardar");
        let error = save(&mut connection, &input("pendientes")).expect_err("debe rechazar");
        assert!(error.to_string().contains("Ya existe una vista"));
    }

    #[test]
    fn permite_renombrar_una_vista_conservando_su_propio_nombre() {
        let mut connection = database();
        let saved = save(&mut connection, &input("Pendientes")).expect("guardar");
        let mut update = input("Pendientes");
        update.id = Some(saved.id.clone());
        update.description = "Lo que quiero empezar".to_string();
        let updated = save(&mut connection, &update).expect("actualizar");
        assert_eq!(updated.id, saved.id);
        assert_eq!(updated.description, "Lo que quiero empezar");
    }

    #[test]
    fn rechaza_consultas_que_no_son_objetos() {
        let mut connection = database();
        let mut invalid = input("Rara");
        invalid.query = serde_json::json!(["no", "soy", "objeto"]);
        let error = save(&mut connection, &invalid).expect_err("debe rechazar");
        assert!(error.to_string().contains("debe ser un objeto"));
    }

    #[test]
    fn rechaza_acentos_fuera_del_catalogo() {
        let mut connection = database();
        let mut invalid = input("Rara");
        invalid.accent = "fucsia".to_string();
        let error = save(&mut connection, &invalid).expect_err("debe rechazar");
        assert!(error.to_string().contains("acento"));
    }

    #[test]
    fn rechaza_consultas_desmesuradas() {
        let mut connection = database();
        let mut invalid = input("Enorme");
        let relleno = "x".repeat(MAX_QUERY_BYTES + 1);
        invalid.query = serde_json::json!({ "search": relleno });
        let error = save(&mut connection, &invalid).expect_err("debe rechazar");
        assert!(error.to_string().contains("demasiado grande"));
    }

    #[test]
    fn las_ancladas_encabezan_la_lista() {
        let mut connection = database();
        save(&mut connection, &input("Alfa")).expect("guardar");
        let mut anclada = input("Zeta");
        anclada.pinned = true;
        save(&mut connection, &anclada).expect("guardar");

        let listed = list(&connection).expect("listar");
        assert_eq!(listed[0].name, "Zeta");
        assert!(listed[0].pinned);
    }

    #[test]
    fn reordenar_exige_el_conjunto_exacto() {
        let mut connection = database();
        let primera = save(&mut connection, &input("Alfa")).expect("guardar");
        let segunda = save(&mut connection, &input("Beta")).expect("guardar");

        // Falta una: se rechaza en lugar de dejar posiciones a medias.
        let error =
            reorder(&mut connection, std::slice::from_ref(&primera.id)).expect_err("debe rechazar");
        assert!(error.to_string().contains("exactamente"));

        // Repetida: también se rechaza.
        let error = reorder(&mut connection, &[primera.id.clone(), primera.id.clone()])
            .expect_err("debe rechazar");
        assert!(error.to_string().contains("exactamente"));

        reorder(&mut connection, &[segunda.id.clone(), primera.id.clone()]).expect("reordenar");
        let listed = list(&connection).expect("listar");
        assert_eq!(listed[0].id, segunda.id);
        assert_eq!(listed[1].id, primera.id);
    }

    #[test]
    fn borrar_compacta_las_posiciones_restantes() {
        let mut connection = database();
        let primera = save(&mut connection, &input("Alfa")).expect("guardar");
        let segunda = save(&mut connection, &input("Beta")).expect("guardar");
        let tercera = save(&mut connection, &input("Gamma")).expect("guardar");

        delete(&mut connection, &segunda.id).expect("borrar");
        let listed = list(&connection).expect("listar");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, primera.id);
        assert_eq!(listed[0].position, 0);
        assert_eq!(listed[1].id, tercera.id);
        assert_eq!(listed[1].position, 1);
    }

    #[test]
    fn marcar_uso_incrementa_el_contador_y_sella_la_fecha() {
        let mut connection = database();
        let saved = save(&mut connection, &input("Alfa")).expect("guardar");
        let used = mark_used(&connection, &saved.id).expect("marcar");
        assert_eq!(used.use_count, 1);
        assert!(used.last_used_at.is_some());
        let used = mark_used(&connection, &saved.id).expect("marcar");
        assert_eq!(used.use_count, 2);
    }

    #[test]
    fn operar_sobre_una_vista_inexistente_da_error_claro() {
        let mut connection = database();
        assert!(get(&connection, "fantasma").is_err());
        assert!(delete(&mut connection, "fantasma").is_err());
        assert!(mark_used(&connection, "fantasma").is_err());
        let mut update = input("Alfa");
        update.id = Some("fantasma".to_string());
        assert!(save(&mut connection, &update).is_err());
    }

    #[test]
    fn un_json_corrupto_degrada_a_consulta_vacia() {
        let mut connection = database();
        let saved = save(&mut connection, &input("Alfa")).expect("guardar");
        connection
            .execute(
                "UPDATE saved_views SET query_json = '{no soy json' WHERE id = ?1",
                [&saved.id],
            )
            .expect("corromper");
        let recovered = get(&connection, &saved.id).expect("leer");
        assert!(recovered.query.is_object());
        assert_eq!(recovered.query.as_object().expect("objeto").len(), 0);
    }
}
