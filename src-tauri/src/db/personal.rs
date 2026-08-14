use crate::error::{AppError, AppResult};
use crate::models::{GameSession, PagedGameSessions};
use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TagDefinition {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTagInput {
    pub id: Option<String>,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveSessionInput {
    pub id: Option<String>,
    pub app_id: u32,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub progress_before: Option<u8>,
    pub progress_after: Option<u8>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePersonalDatesInput {
    pub app_id: u32,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub abandoned_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDates {
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub abandoned_at: Option<String>,
}

pub fn list_tags(connection: &Connection) -> AppResult<Vec<TagDefinition>> {
    let mut statement = connection
        .prepare("SELECT id, name, color FROM tags ORDER BY name COLLATE NOCASE ASC, id ASC")?;
    Ok(statement
        .query_map([], |row| {
            Ok(TagDefinition {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

pub fn save_tag(connection: &mut Connection, input: &SaveTagInput) -> AppResult<TagDefinition> {
    let name = validate_tag_name(&input.name)?;
    let color = validate_color(&input.color)?;
    let id = input
        .id
        .as_deref()
        .map(|value| required_id(value, "La etiqueta").map(str::to_owned))
        .transpose()?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let duplicate = connection
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE AND id <> ?2 LIMIT 1",
            params![name, id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .is_some();
    if duplicate {
        return Err(AppError::validation(
            "Ya existe una etiqueta personal con ese nombre.",
        ));
    }

    let transaction = connection.transaction()?;
    if input.id.is_some() {
        let changed = transaction.execute(
            "UPDATE tags SET name = ?2, color = ?3 WHERE id = ?1",
            params![id, name, color],
        )?;
        if changed != 1 {
            return Err(AppError::not_found("La etiqueta ya no existe."));
        }
    } else {
        transaction.execute(
            "INSERT INTO tags(id, name, color) VALUES (?1, ?2, ?3)",
            params![id, name, color],
        )?;
    }
    transaction.commit()?;
    Ok(TagDefinition { id, name, color })
}

pub fn delete_tag(connection: &mut Connection, id: &str) -> AppResult<()> {
    let id = required_id(id, "La etiqueta")?;
    let transaction = connection.transaction()?;
    let changed = transaction.execute("DELETE FROM tags WHERE id = ?1", [id])?;
    if changed != 1 {
        return Err(AppError::not_found("La etiqueta ya no existe."));
    }
    transaction.commit()?;
    Ok(())
}

pub fn set_game_tags(
    connection: &mut Connection,
    app_id: u32,
    tag_ids: &[String],
) -> AppResult<()> {
    if app_id == 0 || tag_ids.len() > 64 {
        return Err(AppError::validation(
            "La asignación de etiquetas no es válida.",
        ));
    }
    let mut normalized = tag_ids
        .iter()
        .map(|id| required_id(id, "La etiqueta").map(str::to_owned))
        .collect::<AppResult<Vec<_>>>()?;
    normalized.sort();
    normalized.dedup();

    let transaction = connection.transaction()?;
    if transaction
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    let mut validate = transaction.prepare("SELECT 1 FROM tags WHERE id = ?1")?;
    for id in &normalized {
        if validate.query_row([id], |_| Ok(())).optional()?.is_none() {
            return Err(AppError::validation(
                "Una de las etiquetas seleccionadas ya no existe.",
            ));
        }
    }
    drop(validate);

    transaction.execute("DELETE FROM game_tags WHERE app_id = ?1", [app_id])?;
    {
        let mut insert =
            transaction.prepare("INSERT INTO game_tags(app_id, tag_id) VALUES (?1, ?2)")?;
        for id in &normalized {
            insert.execute(params![app_id, id])?;
        }
    }
    transaction.execute(
        "INSERT INTO activity(id, kind, app_id, message) VALUES (?1, 'tags_update', ?2, ?3)",
        params![
            Uuid::new_v4().to_string(),
            app_id,
            if normalized.is_empty() {
                "Se retiraron las etiquetas personales."
            } else {
                "Se actualizaron las etiquetas personales."
            }
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn game_tag_ids(connection: &Connection, app_id: u32) -> AppResult<Vec<String>> {
    let mut statement =
        connection.prepare("SELECT tag_id FROM game_tags WHERE app_id = ?1 ORDER BY tag_id ASC")?;
    Ok(statement
        .query_map([app_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}

pub fn save_session(
    connection: &mut Connection,
    input: &SaveSessionInput,
) -> AppResult<GameSession> {
    if input.app_id == 0
        || input.progress_before.is_some_and(|value| value > 100)
        || input.progress_after.is_some_and(|value| value > 100)
    {
        return Err(AppError::validation(
            "Los datos de la sesión no son válidos.",
        ));
    }
    let started = parse_session_date(&input.started_at, "inicio")?;
    let ended = input
        .ended_at
        .as_deref()
        .map(|value| parse_session_date(value, "final"))
        .transpose()?;
    if ended.is_some_and(|value| value < started) {
        return Err(AppError::validation(
            "El final de la sesión no puede ser anterior al inicio.",
        ));
    }
    let note = input.note.trim();
    if note.chars().count() > 2_000 {
        return Err(AppError::validation(
            "La nota de sesión no puede superar 2.000 caracteres.",
        ));
    }
    let id = input
        .id
        .as_deref()
        .map(|value| required_id(value, "La sesión").map(str::to_owned))
        .transpose()?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let started_at = started.to_rfc3339_opts(SecondsFormat::Secs, true);
    let ended_at = ended.map(|value| value.to_rfc3339_opts(SecondsFormat::Secs, true));

    let transaction = connection.transaction()?;
    if transaction
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [input.app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }
    let kind = if input.id.is_some() {
        let changed = transaction.execute(
            "UPDATE game_sessions SET
                started_at = ?3, ended_at = ?4, progress_before = ?5,
                progress_after = ?6, note = ?7
              WHERE id = ?1 AND app_id = ?2",
            params![
                id,
                input.app_id,
                started_at,
                ended_at,
                input.progress_before,
                input.progress_after,
                note
            ],
        )?;
        if changed != 1 {
            return Err(AppError::not_found(
                "La sesión ya no existe para este juego.",
            ));
        }
        "session_update"
    } else {
        transaction.execute(
            "INSERT INTO game_sessions(
                id, app_id, started_at, ended_at, progress_before, progress_after, note
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                input.app_id,
                started_at,
                ended_at,
                input.progress_before,
                input.progress_after,
                note
            ],
        )?;
        "session_create"
    };
    transaction.execute(
        "INSERT INTO activity(id, kind, app_id, message) VALUES (?1, ?2, ?3, ?4)",
        params![
            Uuid::new_v4().to_string(),
            kind,
            input.app_id,
            if kind == "session_create" {
                "Se registró una sesión de juego."
            } else {
                "Se actualizó una sesión de juego."
            }
        ],
    )?;
    transaction.commit()?;
    Ok(GameSession {
        id,
        started_at,
        ended_at,
        progress_before: input.progress_before,
        progress_after: input.progress_after,
        note: note.to_string(),
    })
}

pub fn list_sessions(
    connection: &Connection,
    app_id: u32,
    limit: u32,
    offset: u32,
) -> AppResult<PagedGameSessions> {
    if app_id == 0 || !(1..=100).contains(&limit) {
        return Err(AppError::validation(
            "La página de sesiones solicitada no es válida.",
        ));
    }
    if connection
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Err(AppError::not_found("El juego ya no está en la biblioteca."));
    }

    let total = connection.query_row(
        "SELECT COUNT(*) FROM game_sessions WHERE app_id = ?1",
        [app_id],
        |row| row.get::<_, i64>(0),
    )?;
    let total = u64::try_from(total)
        .map_err(|_| AppError::new("database_data", "El total de sesiones no es válido."))?;
    let mut statement = connection.prepare(
        "SELECT id, started_at, ended_at, progress_before, progress_after, note
         FROM game_sessions
         WHERE app_id = ?1
         ORDER BY started_at DESC, id DESC
         LIMIT ?2 OFFSET ?3",
    )?;
    let items = statement
        .query_map(params![app_id, limit, offset], |row| {
            Ok(GameSession {
                id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                progress_before: row.get(3)?,
                progress_after: row.get(4)?,
                note: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(PagedGameSessions {
        items,
        total,
        limit,
        offset,
    })
}

pub fn delete_session(connection: &mut Connection, id: &str) -> AppResult<u32> {
    let id = required_id(id, "La sesión")?;
    let transaction = connection.transaction()?;
    let app_id = transaction
        .query_row(
            "SELECT app_id FROM game_sessions WHERE id = ?1",
            [id],
            |row| row.get::<_, u32>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("La sesión ya no existe."))?;
    transaction.execute("DELETE FROM game_sessions WHERE id = ?1", [id])?;
    transaction.execute(
        "INSERT INTO activity(id, kind, app_id, message)
         VALUES (?1, 'session_delete', ?2, 'Se eliminó una sesión de juego.')",
        params![Uuid::new_v4().to_string(), app_id],
    )?;
    transaction.commit()?;
    Ok(app_id)
}

pub fn save_personal_dates(
    connection: &mut Connection,
    input: &SavePersonalDatesInput,
) -> AppResult<PersonalDates> {
    if input.app_id == 0 {
        return Err(AppError::validation("El juego no es válido."));
    }
    let started = parse_personal_date(input.started_at.as_deref(), "de inicio")?;
    let completed = parse_personal_date(input.completed_at.as_deref(), "de finalización")?;
    let abandoned = parse_personal_date(input.abandoned_at.as_deref(), "de abandono")?;
    if completed.is_some() && abandoned.is_some() {
        return Err(AppError::validation(
            "Un juego no puede estar finalizado y abandonado a la vez.",
        ));
    }
    let final_date = completed.or(abandoned);
    if matches!((started, final_date), (Some(start), Some(end)) if end < start) {
        return Err(AppError::validation(
            "La fecha final no puede ser anterior a la fecha de inicio.",
        ));
    }
    let dates = PersonalDates {
        started_at: started.map(|date| date.format("%Y-%m-%d").to_string()),
        completed_at: completed.map(|date| date.format("%Y-%m-%d").to_string()),
        abandoned_at: abandoned.map(|date| date.format("%Y-%m-%d").to_string()),
    };
    let transaction = connection.transaction()?;
    let changed = transaction.execute(
        "UPDATE game_personal SET
            started_at = ?2, completed_at = ?3, abandoned_at = ?4,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1",
        params![
            input.app_id,
            dates.started_at,
            dates.completed_at,
            dates.abandoned_at
        ],
    )?;
    if changed != 1 {
        return Err(AppError::not_found(
            "La organización personal del juego ya no existe.",
        ));
    }
    transaction.execute(
        "INSERT INTO activity(id, kind, app_id, message)
         VALUES (?1, 'personal_dates_update', ?2, 'Se actualizaron las fechas personales.')",
        params![Uuid::new_v4().to_string(), input.app_id],
    )?;
    transaction.commit()?;
    Ok(dates)
}

fn validate_tag_name(value: &str) -> AppResult<String> {
    let value = value.trim();
    let length = value.chars().count();
    if !(1..=40).contains(&length) {
        return Err(AppError::validation(
            "El nombre de la etiqueta debe tener entre 1 y 40 caracteres.",
        ));
    }
    Ok(value.to_string())
}

fn validate_color(value: &str) -> AppResult<String> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() != 7
        || !value.starts_with('#')
        || !value[1..]
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(AppError::validation(
            "El color debe usar el formato hexadecimal #RRGGBB.",
        ));
    }
    Ok(value)
}

fn required_id<'a>(value: &'a str, label: &str) -> AppResult<&'a str> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 100 {
        return Err(AppError::validation(format!("{label} no es válida.")));
    }
    Ok(value)
}

fn parse_session_date(value: &str, label: &str) -> AppResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value.trim())
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| {
            AppError::validation(format!("La fecha de {label} de la sesión no es válida."))
        })
}

fn parse_personal_date(value: Option<&str>, label: &str) -> AppResult<Option<NaiveDate>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| {
            AppError::validation(format!("La fecha {label} debe usar el formato AAAA-MM-DD."))
        })
}

#[cfg(test)]
mod tests {
    use super::{
        PersonalDates, SavePersonalDatesInput, SaveSessionInput, SaveTagInput, delete_session,
        delete_tag, game_tag_ids, list_sessions, list_tags, save_personal_dates, save_session,
        save_tag, set_game_tags,
    };
    use crate::db::library::{ImportedGame, upsert_imported_games};
    use crate::db::migrations;
    use rusqlite::Connection;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("aplicar migraciones");
        connection
    }

    #[test]
    fn tag_crud_is_observable_through_the_public_listing_seam() {
        let mut connection = database();
        let created = save_tag(
            &mut connection,
            &SaveTagInput {
                id: None,
                name: "  Cooperativo  ".into(),
                color: "#5CAAC1".into(),
            },
        )
        .expect("crear etiqueta");

        assert_eq!(created.name, "Cooperativo");
        let edited = save_tag(
            &mut connection,
            &SaveTagInput {
                id: Some(created.id),
                name: "Cooperativo local".into(),
                color: "#A4D007".into(),
            },
        )
        .expect("editar etiqueta");
        assert_eq!(edited.name, "Cooperativo local");
        assert_eq!(edited.color, "#A4D007");
        assert_eq!(list_tags(&connection).expect("listar"), vec![edited]);
    }

    #[test]
    fn game_tag_assignment_and_deletion_are_transactional_and_audited() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Portal 2')",
                [],
            )
            .expect("insertar juego");
        let tag = save_tag(
            &mut connection,
            &SaveTagInput {
                id: None,
                name: "Cooperativo".into(),
                color: "#5CAAC1".into(),
            },
        )
        .expect("crear etiqueta");

        set_game_tags(&mut connection, 10, std::slice::from_ref(&tag.id))
            .expect("asignar etiqueta");
        assert_eq!(
            game_tag_ids(&connection, 10).expect("leer"),
            vec![tag.id.clone()]
        );
        let activity: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM activity WHERE app_id = 10 AND kind = 'tags_update'",
                [],
                |row| row.get(0),
            )
            .expect("leer actividad");
        assert_eq!(activity, 1);

        delete_tag(&mut connection, &tag.id).expect("eliminar etiqueta");
        assert!(game_tag_ids(&connection, 10).expect("leer").is_empty());
    }

    #[test]
    fn session_create_edit_delete_roundtrip_validates_time_and_writes_activity() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Portal 2')",
                [],
            )
            .expect("insertar juego");
        let created = save_session(
            &mut connection,
            &SaveSessionInput {
                id: None,
                app_id: 10,
                started_at: "2026-08-14T18:00:00Z".into(),
                ended_at: Some("2026-08-14T19:15:00Z".into()),
                progress_before: Some(20),
                progress_after: Some(35),
                note: "  Cámara de pruebas 6  ".into(),
            },
        )
        .expect("crear sesión");
        assert_eq!(created.note, "Cámara de pruebas 6");

        let edited = save_session(
            &mut connection,
            &SaveSessionInput {
                id: Some(created.id.clone()),
                app_id: 10,
                started_at: created.started_at.clone(),
                ended_at: created.ended_at.clone(),
                progress_before: created.progress_before,
                progress_after: Some(40),
                note: "Cámara de pruebas 7".into(),
            },
        )
        .expect("editar sesión");
        assert_eq!(edited.progress_after, Some(40));
        assert_eq!(edited.id, created.id);

        let invalid = save_session(
            &mut connection,
            &SaveSessionInput {
                id: None,
                app_id: 10,
                started_at: "2026-08-14T20:00:00Z".into(),
                ended_at: Some("2026-08-14T19:00:00Z".into()),
                progress_before: None,
                progress_after: None,
                note: String::new(),
            },
        )
        .expect_err("rechazar intervalo invertido");
        assert_eq!(invalid.code, "validation");

        assert_eq!(
            delete_session(&mut connection, &created.id).expect("eliminar"),
            10
        );
        let activity: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM activity WHERE app_id = 10 AND kind LIKE 'session_%'",
                [],
                |row| row.get(0),
            )
            .expect("leer actividad");
        assert_eq!(activity, 3);
    }

    #[test]
    fn session_history_pages_without_hiding_older_entries() {
        let connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Portal 2')",
                [],
            )
            .expect("insertar juego");
        for index in 0..55 {
            connection
                .execute(
                    "INSERT INTO game_sessions(id, app_id, started_at, note)
                     VALUES (?1, 10, ?2, '')",
                    rusqlite::params![
                        format!("session-{index:02}"),
                        format!("2026-06-{:02}T{:02}:00:00Z", (index % 28) + 1, index / 28)
                    ],
                )
                .expect("insertar sesión");
        }

        let first = list_sessions(&connection, 10, 50, 0).expect("primera página");
        let second = list_sessions(&connection, 10, 50, 50).expect("segunda página");
        assert_eq!(first.total, 55);
        assert_eq!(first.items.len(), 50);
        assert_eq!(second.total, 55);
        assert_eq!(second.items.len(), 5);
        assert!(first.items[0].started_at > second.items[0].started_at);
    }

    #[test]
    fn personal_dates_roundtrip_rejects_conflicting_or_inverted_milestones() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Portal 2')",
                [],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO statuses(id, name, color, position, built_in)
                 VALUES ('unclassified', 'Sin clasificar', '#ABB7B5', 0, 1)",
                [],
            )
            .expect("insertar estado");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (10, 'unclassified')",
                [],
            )
            .expect("insertar organización");
        let dates = save_personal_dates(
            &mut connection,
            &SavePersonalDatesInput {
                app_id: 10,
                started_at: Some("2026-08-10".into()),
                completed_at: Some("2026-08-14".into()),
                abandoned_at: None,
            },
        )
        .expect("guardar fechas");
        assert_eq!(
            dates,
            PersonalDates {
                started_at: Some("2026-08-10".into()),
                completed_at: Some("2026-08-14".into()),
                abandoned_at: None,
            }
        );

        let conflict = save_personal_dates(
            &mut connection,
            &SavePersonalDatesInput {
                app_id: 10,
                started_at: Some("2026-08-10".into()),
                completed_at: Some("2026-08-14".into()),
                abandoned_at: Some("2026-08-15".into()),
            },
        )
        .expect_err("rechazar dos hitos finales");
        assert_eq!(conflict.code, "validation");

        let inverted = save_personal_dates(
            &mut connection,
            &SavePersonalDatesInput {
                app_id: 10,
                started_at: Some("2026-08-20".into()),
                completed_at: Some("2026-08-14".into()),
                abandoned_at: None,
            },
        )
        .expect_err("rechazar orden invertido");
        assert_eq!(inverted.code, "validation");
        let activity: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM activity WHERE app_id = 10 AND kind = 'personal_dates_update'",
                [],
                |row| row.get(0),
            )
            .expect("leer actividad");
        assert_eq!(activity, 1);
    }

    #[test]
    fn steam_resync_preserves_tags_sessions_and_personal_dates() {
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Portal 2')",
                [],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO statuses(id, name, color, position, built_in)
                 VALUES ('unclassified', 'Sin clasificar', '#ABB7B5', 0, 1)",
                [],
            )
            .expect("insertar estado");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (10, 'unclassified')",
                [],
            )
            .expect("insertar organización");
        let tag = save_tag(
            &mut connection,
            &SaveTagInput {
                id: None,
                name: "Cooperativo".into(),
                color: "#5CAAC1".into(),
            },
        )
        .expect("crear etiqueta");
        set_game_tags(&mut connection, 10, std::slice::from_ref(&tag.id))
            .expect("asignar etiqueta");
        save_session(
            &mut connection,
            &SaveSessionInput {
                id: None,
                app_id: 10,
                started_at: "2026-08-14T18:00:00Z".into(),
                ended_at: None,
                progress_before: Some(20),
                progress_after: None,
                note: "Cámara 6".into(),
            },
        )
        .expect("crear sesión");
        let dates = SavePersonalDatesInput {
            app_id: 10,
            started_at: Some("2026-08-10".into()),
            completed_at: None,
            abandoned_at: None,
        };
        save_personal_dates(&mut connection, &dates).expect("guardar fechas");

        upsert_imported_games(
            &mut connection,
            &[ImportedGame {
                app_id: 10,
                title: "Portal 2 actualizado".into(),
                icon_url: None,
                cover_url: None,
                header_url: None,
                playtime_minutes: 900,
                playtime_recent_minutes: 30,
                last_played_at: Some("2026-08-14T19:00:00Z".into()),
                ownership_source: "owned".into(),
                family_availability: "not_applicable".into(),
                installation: None,
            }],
            false,
        )
        .expect("resincronizar Steam");

        assert_eq!(
            game_tag_ids(&connection, 10).expect("leer etiquetas"),
            vec![tag.id]
        );
        assert_eq!(
            list_sessions(&connection, 10, 50, 0)
                .expect("leer sesiones")
                .total,
            1
        );
        let stored_dates: (Option<String>, Option<String>, Option<String>) = connection
            .query_row(
                "SELECT started_at, completed_at, abandoned_at
                 FROM game_personal WHERE app_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("leer fechas");
        assert_eq!(stored_dates, (dates.started_at, None, None));
    }
}
