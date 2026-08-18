//! Archivado de juegos: «no me lo enseñes más» (migración 029).
//!
//! # Archivar no es un estado
//!
//! Un estado (`jugando`, `pendiente`, `abandonado`) describe tu relación con el
//! juego. Archivar no describe nada del juego: dice que **no quieres verlo** en
//! la vista por defecto. Con mil quinientos títulos y cuatrocientos venidos de
//! paquetes que nunca se van a abrir, esa distinción es la diferencia entre una
//! biblioteca y un vertedero ordenado.
//!
//! Por eso vive en su propia tabla y no como una columna más de
//! `game_personal`: mezclarlas invitaría justo a la confusión que el archivado
//! viene a resolver, y obligaría a que cada consulta de estado se acordara de
//! excluir un valor especial.
//!
//! # Archivar nunca borra
//!
//! La fila de `games` y la de `game_personal` siguen intactas: horas jugadas,
//! notas, valoración, logros. Desarchivar es borrar una fila de `game_archive`
//! y todo vuelve exactamente como estaba. Ninguna operación de este módulo
//! escribe en las tablas de biblioteca.
//!
//! # Qué cuenta y qué no (decidido, no elegido al azar)
//!
//! - **Totales de biblioteca**: un juego archivado *no* cuenta. El número que
//!   encabeza la pantalla tiene que coincidir con lo que se ve debajo; si no,
//!   el total deja de ser comprobable. El recuento de archivados se sirve
//!   aparte con [`count`], para que el dato exista y sea visible.
//! - **Búsqueda y listados**: excluidos por defecto y visibles con un filtro
//!   explícito. El archivado no es un agujero negro: [`count`] permite que la
//!   interfaz diga siempre «hay N archivados» y ofrezca verlos. El vocabulario
//!   del filtro ([`crate::models::ARCHIVE_SCOPES`]) vive junto a
//!   `GameListRequest`, con el resto de los filtros de biblioteca.
//! - **Colecciones inteligentes**: las reglas siguen evaluándose sobre todos
//!   los juegos, pero el listado y el recuento excluyen los archivados, porque
//!   una colección es una vista de biblioteca y hereda su regla. Si archivas un
//!   juego que cumplía la regla, deja de aparecer y el contador baja.
//! - **Planificador**: **se conserva**. Un elemento del plan es una decisión
//!   explícita que tomaste; retirarlo en silencio al archivar sería borrar esa
//!   decisión sin decírtelo. [`ArchiveReport::in_planner`] cuenta cuántos de
//!   los archivados siguen en el plan para que la interfaz pueda avisarte y
//!   ofrecerte quitarlos, en lugar de decidirlo por ti.

use crate::error::{AppError, AppResult};
use crate::models::GameSummary;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Juegos que una sola operación puede archivar o desarchivar. Cuatrocientos
/// juegos de paquete no se archivan de uno en uno; dos mil de golpe ya sólo
/// pueden venir de un error de la interfaz.
pub const MAX_ARCHIVE_BATCH: usize = 2_000;
/// Longitud máxima del motivo, igual que el `CHECK` de la migración 029.
pub const MAX_REASON_LENGTH: usize = 200;
/// Página máxima del listado de archivados.
pub const MAX_ARCHIVE_PAGE: u32 = 200;
const DEFAULT_ARCHIVE_PAGE: u32 = 120;

/// Un juego archivado con su ficha de biblioteca resuelta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedGame {
    pub game: GameSummary,
    pub reason: String,
    pub archived_at: String,
}

/// Página del listado de archivados.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedArchivedGames {
    pub items: Vec<ArchivedGame>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

/// Resultado de archivar o desarchivar.
///
/// `unchanged` no es un fallo: archivar algo ya archivado es la operación que
/// se espera que no haga nada, y decirlo permite que la interfaz no anuncie un
/// cambio que no ocurrió.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveReport {
    pub requested: u32,
    pub changed: u32,
    pub unchanged: u32,
    /// Cuántos de los juegos afectados siguen en el planificador. Archivar no
    /// los saca: este número existe para poder avisar, no para justificar un
    /// borrado silencioso.
    pub in_planner: u32,
    /// Total de archivados después de la operación.
    pub archived_total: i64,
}

fn validate_batch(app_ids: &[u32]) -> AppResult<Vec<u32>> {
    if app_ids.is_empty() {
        return Err(AppError::validation(
            "Indica al menos un juego que archivar.",
        ));
    }
    if app_ids.len() > MAX_ARCHIVE_BATCH {
        return Err(AppError::validation(format!(
            "No se pueden archivar más de {MAX_ARCHIVE_BATCH} juegos de una vez."
        )));
    }
    if app_ids.contains(&0) {
        return Err(AppError::validation("El juego indicado no es válido."));
    }
    // El mismo juego repetido en la selección no debe contarse dos veces.
    let mut seen = HashSet::new();
    Ok(app_ids
        .iter()
        .copied()
        .filter(|app_id| seen.insert(*app_id))
        .collect())
}

/// Comprueba que todos los juegos existan **antes** de tocar nada.
///
/// Se rechaza el lote entero si alguno falta, en vez de archivar los que sí
/// están: un informe que dice «archivados 399 de 400» sin decir cuál falló es
/// peor que un error claro, y una selección de la biblioteca no puede contener
/// identificadores inventados.
fn ensure_all_exist(connection: &Connection, app_ids: &[u32]) -> AppResult<()> {
    let placeholders = vec!["?"; app_ids.len()].join(", ");
    let found: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM games WHERE app_id IN ({placeholders})"),
        params_from_iter(app_ids.iter()),
        |row| row.get(0),
    )?;
    if found != app_ids.len() as i64 {
        let missing = app_ids.len() as i64 - found;
        return Err(AppError::not_found(format!(
            "{missing} de los {} juegos indicados ya no están en la biblioteca.",
            app_ids.len()
        )));
    }
    Ok(())
}

fn validate_reason(reason: &str) -> AppResult<String> {
    let trimmed = reason.trim();
    if trimmed.chars().count() > MAX_REASON_LENGTH {
        return Err(AppError::validation(format!(
            "El motivo no puede superar {MAX_REASON_LENGTH} caracteres."
        )));
    }
    Ok(trimmed.to_string())
}

/// Archiva un lote de juegos. Es idempotente: los que ya estaban archivados
/// conservan su fecha y su motivo originales y se cuentan en `unchanged`.
pub fn archive_games(
    connection: &mut Connection,
    app_ids: &[u32],
    reason: &str,
    now: DateTime<Utc>,
) -> AppResult<ArchiveReport> {
    let app_ids = validate_batch(app_ids)?;
    let reason = validate_reason(reason)?;
    ensure_all_exist(connection, &app_ids)?;
    let archived_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);

    let transaction = connection.transaction()?;
    let mut changed = 0u32;
    {
        let mut statement = transaction.prepare(
            "INSERT OR IGNORE INTO game_archive(app_id, reason, archived_at)
             VALUES (?1, ?2, ?3)",
        )?;
        for app_id in &app_ids {
            changed += statement.execute(params![app_id, reason, archived_at])? as u32;
        }
    }
    let in_planner = count_in_planner(&transaction, &app_ids)?;
    let archived_total = count(&transaction)?;
    transaction.commit()?;

    Ok(ArchiveReport {
        requested: app_ids.len() as u32,
        changed,
        unchanged: app_ids.len() as u32 - changed,
        in_planner,
        archived_total,
    })
}

/// Devuelve a la biblioteca un lote de juegos archivados. También idempotente:
/// desarchivar algo que no estaba archivado no es un error, es un no-cambio.
pub fn unarchive_games(connection: &mut Connection, app_ids: &[u32]) -> AppResult<ArchiveReport> {
    let app_ids = validate_batch(app_ids)?;
    ensure_all_exist(connection, &app_ids)?;

    let transaction = connection.transaction()?;
    let mut changed = 0u32;
    {
        let mut statement = transaction.prepare("DELETE FROM game_archive WHERE app_id = ?1")?;
        for app_id in &app_ids {
            changed += statement.execute([app_id])? as u32;
        }
    }
    let in_planner = count_in_planner(&transaction, &app_ids)?;
    let archived_total = count(&transaction)?;
    transaction.commit()?;

    Ok(ArchiveReport {
        requested: app_ids.len() as u32,
        changed,
        unchanged: app_ids.len() as u32 - changed,
        in_planner,
        archived_total,
    })
}

fn count_in_planner(connection: &Connection, app_ids: &[u32]) -> AppResult<u32> {
    let placeholders = vec!["?"; app_ids.len()].join(", ");
    let total: i64 = connection.query_row(
        &format!("SELECT COUNT(*) FROM planner_items WHERE app_id IN ({placeholders})"),
        params_from_iter(app_ids.iter()),
        |row| row.get(0),
    )?;
    Ok(total.clamp(0, i64::from(u32::MAX)) as u32)
}

/// Cuántos juegos hay archivados. Es el dato que impide que el archivado sea un
/// agujero negro: la interfaz siempre puede decir cuántos hay y ofrecer verlos.
pub fn count(connection: &Connection) -> AppResult<i64> {
    connection
        .query_row("SELECT COUNT(*) FROM game_archive", [], |row| row.get(0))
        .map_err(Into::into)
}

/// `true` si el juego está archivado. Lo usa la ficha para poder ofrecer
/// «devolver a la biblioteca» en lugar de «archivar».
pub fn is_archived(connection: &Connection, app_id: u32) -> AppResult<bool> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM game_archive WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// Listado paginado de archivados, del más reciente al más antiguo.
pub fn list_archived(
    connection: &Connection,
    limit: u32,
    offset: u32,
) -> AppResult<PagedArchivedGames> {
    let limit = if limit == 0 {
        DEFAULT_ARCHIVE_PAGE
    } else {
        limit.min(MAX_ARCHIVE_PAGE)
    };
    let total = count(connection)?;

    let mut statement = connection.prepare(
        "SELECT a.app_id, a.reason, a.archived_at
           FROM game_archive a
           JOIN games g ON g.app_id = a.app_id
           JOIN game_personal p ON p.app_id = a.app_id
          ORDER BY a.archived_at DESC, a.app_id ASC
          LIMIT ?1 OFFSET ?2",
    )?;
    let rows = statement
        .query_map(params![limit, offset], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let mut items = Vec::with_capacity(rows.len());
    for (app_id, reason, archived_at) in rows {
        items.push(ArchivedGame {
            // La ficha la construye siempre `db::library`: aquí no se
            // reconstruye un `GameSummary` a mano.
            game: crate::db::library::game_summary(connection, app_id)?,
            reason,
            archived_at,
        });
    }
    Ok(PagedArchivedGames {
        items,
        total,
        limit,
        offset,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrations, seed_defaults};
    use chrono::TimeZone;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar valores por defecto");
        connection
    }

    fn game(connection: &Connection, app_id: u32, title: &str) {
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                params![app_id, title],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'backlog')",
                [app_id],
            )
            .expect("insertar ficha personal");
    }

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, day, 12, 0, 0)
            .single()
            .expect("instante válido")
    }

    #[test]
    fn archivar_saca_el_juego_de_la_vista_sin_borrarlo() {
        let mut connection = database();
        game(&connection, 10, "Basura de paquete");

        let report =
            archive_games(&mut connection, &[10], "Vino en un bundle", at(1)).expect("archivar");
        assert_eq!(report.changed, 1);
        assert_eq!(report.archived_total, 1);
        assert!(is_archived(&connection, 10).expect("consultar"));

        // El juego y su ficha personal siguen exactamente donde estaban.
        let games: i64 = connection
            .query_row("SELECT COUNT(*) FROM games", [], |row| row.get(0))
            .expect("contar juegos");
        assert_eq!(games, 1);
        let personal: i64 = connection
            .query_row("SELECT COUNT(*) FROM game_personal", [], |row| row.get(0))
            .expect("contar fichas");
        assert_eq!(personal, 1);
    }

    #[test]
    fn archivar_y_desarchivar_son_idempotentes() {
        let mut connection = database();
        game(&connection, 10, "Repetido");

        let primero = archive_games(&mut connection, &[10], "", at(1)).expect("archivar");
        assert_eq!(primero.changed, 1);
        assert_eq!(primero.unchanged, 0);

        let segundo = archive_games(&mut connection, &[10], "otro motivo", at(5)).expect("repetir");
        assert_eq!(segundo.changed, 0);
        assert_eq!(segundo.unchanged, 1);
        // La fecha y el motivo originales se conservan: repetir no reescribe.
        let (reason, archived_at): (String, String) = connection
            .query_row(
                "SELECT reason, archived_at FROM game_archive WHERE app_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer fila");
        assert_eq!(reason, "");
        assert!(archived_at.starts_with("2026-03-01"));

        let vuelta = unarchive_games(&mut connection, &[10]).expect("desarchivar");
        assert_eq!(vuelta.changed, 1);
        assert_eq!(vuelta.archived_total, 0);

        let repetida = unarchive_games(&mut connection, &[10]).expect("repetir");
        assert_eq!(repetida.changed, 0);
        assert_eq!(repetida.unchanged, 1);
        assert!(!is_archived(&connection, 10).expect("consultar"));
    }

    #[test]
    fn archiva_una_seleccion_grande_de_una_sola_vez() {
        let mut connection = database();
        for app_id in 1..=400u32 {
            game(&connection, app_id, &format!("Paquete {app_id}"));
        }
        let ids = (1..=400u32).collect::<Vec<_>>();
        let report = archive_games(&mut connection, &ids, "Bundle", at(1)).expect("archivar");
        assert_eq!(report.requested, 400);
        assert_eq!(report.changed, 400);
        assert_eq!(report.archived_total, 400);
    }

    #[test]
    fn un_juego_repetido_en_la_seleccion_cuenta_una_vez() {
        let mut connection = database();
        game(&connection, 10, "Uno");
        let report = archive_games(&mut connection, &[10, 10, 10], "", at(1)).expect("archivar");
        assert_eq!(report.requested, 1);
        assert_eq!(report.changed, 1);
    }

    #[test]
    fn un_lote_con_un_juego_inexistente_se_rechaza_entero() {
        let mut connection = database();
        game(&connection, 10, "Existe");
        let error =
            archive_games(&mut connection, &[10, 999], "", at(1)).expect_err("debe rechazar");
        assert!(error.to_string().contains("ya no están en la biblioteca"));
        // No queda nada a medias.
        assert_eq!(count(&connection).expect("contar"), 0);
    }

    #[test]
    fn el_lote_vacio_o_desmesurado_se_rechaza() {
        let mut connection = database();
        assert!(archive_games(&mut connection, &[], "", at(1)).is_err());
        let enorme = (1..=(MAX_ARCHIVE_BATCH as u32 + 1)).collect::<Vec<_>>();
        let error = archive_games(&mut connection, &enorme, "", at(1)).expect_err("debe rechazar");
        assert!(error.to_string().contains("de una vez"));
    }

    #[test]
    fn el_motivo_tiene_un_tope() {
        let mut connection = database();
        game(&connection, 10, "Uno");
        let largo = "x".repeat(MAX_REASON_LENGTH + 1);
        let error =
            archive_games(&mut connection, &[10], &largo, at(1)).expect_err("debe rechazar");
        assert!(error.to_string().contains("motivo"));
    }

    #[test]
    fn el_planificador_conserva_lo_archivado_y_se_avisa_de_ello() {
        let mut connection = database();
        game(&connection, 10, "En el plan");
        let column: String = connection
            .query_row("SELECT id FROM planner_columns LIMIT 1", [], |row| {
                row.get(0)
            })
            .expect("columna sembrada");
        connection
            .execute(
                "INSERT INTO planner_items(column_id, app_id, position)
                 VALUES (?1, 10, 0)",
                [&column],
            )
            .expect("insertar en el plan");

        let report = archive_games(&mut connection, &[10], "", at(1)).expect("archivar");
        assert_eq!(report.in_planner, 1);
        // El elemento del plan sigue ahí: archivar no borra una decisión.
        let items: i64 = connection
            .query_row("SELECT COUNT(*) FROM planner_items", [], |row| row.get(0))
            .expect("contar plan");
        assert_eq!(items, 1);
    }

    #[test]
    fn el_listado_ordena_por_fecha_y_resuelve_la_ficha() {
        let mut connection = database();
        game(&connection, 10, "Antiguo");
        game(&connection, 20, "Reciente");
        archive_games(&mut connection, &[10], "primero", at(1)).expect("archivar");
        archive_games(&mut connection, &[20], "después", at(5)).expect("archivar");

        let page = list_archived(&connection, 0, 0).expect("listar");
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].game.app_id, 20);
        assert_eq!(page.items[0].game.title, "Reciente");
        assert_eq!(page.items[0].reason, "después");
        assert_eq!(page.items[1].game.app_id, 10);
    }

    #[test]
    fn el_ambito_traduce_a_la_clausula_correcta_y_rechaza_lo_desconocido() {
        use crate::models::{archive_scope_clause, is_valid_archive_scope};

        assert!(
            archive_scope_clause("active")
                .expect("cláusula")
                .starts_with("NOT EXISTS")
        );
        assert!(
            archive_scope_clause("archived")
                .expect("cláusula")
                .starts_with("EXISTS")
        );
        assert!(archive_scope_clause("all").is_none());
        assert!(is_valid_archive_scope("active"));
        assert!(!is_valid_archive_scope("papelera"));
    }

    #[test]
    fn borrar_el_juego_arrastra_su_fila_de_archivo() {
        let mut connection = database();
        game(&connection, 10, "Se va");
        archive_games(&mut connection, &[10], "", at(1)).expect("archivar");
        connection
            .execute("DELETE FROM games WHERE app_id = 10", [])
            .expect("borrar juego");
        assert_eq!(count(&connection).expect("contar"), 0);
    }
}
