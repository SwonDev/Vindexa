use crate::error::{AppError, AppResult};
use crate::models::MetadataEnrichmentStatus;
use rusqlite::{Connection, OptionalExtension, params};

pub const MAX_VISIBLE_PRIORITY_IDS: usize = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataJob {
    pub app_id: u32,
    pub attempts: u32,
}

pub fn enqueue(
    connection: &mut Connection,
    visible_app_ids: &[u32],
    include_backlog: bool,
) -> AppResult<usize> {
    if visible_app_ids.len() > MAX_VISIBLE_PRIORITY_IDS {
        return Err(AppError::new(
            "metadata_queue_visible_limit",
            format!(
                "Solo se pueden priorizar {MAX_VISIBLE_PRIORITY_IDS} juegos visibles por lote."
            ),
        ));
    }
    if visible_app_ids.contains(&0) {
        return Err(AppError::validation(
            "La cola de metadatos recibió un AppID no válido.",
        ));
    }

    let transaction = connection.transaction()?;
    let mut unique_visible = visible_app_ids.to_vec();
    unique_visible.sort_unstable();
    unique_visible.dedup();
    for app_id in unique_visible {
        enqueue_one(&transaction, app_id, 0)?;
    }
    if include_backlog {
        transaction.execute(
            "INSERT INTO metadata_enrichment_queue(app_id, priority)
             SELECT g.app_id, CASE WHEN p.installed = 1 THEN 50 ELSE 100 END
               FROM games g
               JOIN game_personal p ON p.app_id = g.app_id
              WHERE NOT (
                    g.ownership_source = 'family_shared'
                    AND g.family_availability <> 'confirmed'
              )
                -- Un juego de Epic, GOG o itch.io no existe en Steam: pedir su
                -- ficha allí devolvería vacío y lo dejaría marcado como «sin
                -- datos» para reintentarlo cada semana. Sus datos vienen de su
                -- propia tienda.
                AND g.external_store IS NULL
                AND (
                    g.metadata_fetched_at IS NULL
                    OR (g.metadata_status = 'success'
                        AND datetime(g.metadata_fetched_at) < datetime('now', '-7 days'))
                    OR (g.metadata_status = 'unavailable'
                        AND datetime(g.metadata_fetched_at) < datetime('now', '-1 day'))
                    OR (g.metadata_status = 'failed'
                        AND datetime(g.metadata_fetched_at) < datetime('now', '-2 hours'))
                )
             ON CONFLICT(app_id) DO UPDATE SET
                priority = CASE
                    WHEN metadata_enrichment_queue.state IN ('pending', 'processing', 'retrying')
                        THEN MIN(metadata_enrichment_queue.priority, excluded.priority)
                    ELSE excluded.priority
                END,
                state = CASE
                    WHEN metadata_enrichment_queue.state IN ('pending', 'processing', 'retrying')
                        THEN metadata_enrichment_queue.state
                    ELSE 'pending'
                END,
                attempts = CASE
                    WHEN metadata_enrichment_queue.state IN ('pending', 'processing', 'retrying')
                        THEN metadata_enrichment_queue.attempts
                    ELSE 0
                END,
                next_attempt_at = CASE
                    WHEN metadata_enrichment_queue.state IN ('processing', 'retrying')
                        THEN metadata_enrichment_queue.next_attempt_at
                    ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                END,
                last_error_code = CASE
                    WHEN metadata_enrichment_queue.state IN ('processing', 'retrying')
                        THEN metadata_enrichment_queue.last_error_code
                    ELSE NULL
                END,
                finished_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [],
        )?;
    }
    let active = transaction.query_row(
        "SELECT COUNT(*) FROM metadata_enrichment_queue
          WHERE state IN ('pending', 'processing', 'retrying')",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    transaction.commit()?;
    usize::try_from(active).map_err(|_| {
        AppError::new(
            "metadata_queue_count",
            "La cola de metadatos contiene un recuento no válido.",
        )
    })
}

fn enqueue_one(connection: &Connection, app_id: u32, priority: u8) -> AppResult<()> {
    connection.execute(
        "INSERT INTO metadata_enrichment_queue(app_id, priority)
         SELECT g.app_id, ?2
           FROM games g
          WHERE g.app_id = ?1
            AND g.external_store IS NULL
            AND NOT (
                g.ownership_source = 'family_shared'
                AND g.family_availability <> 'confirmed'
            )
            AND (
                g.metadata_fetched_at IS NULL
                OR (g.metadata_status = 'success'
                    AND datetime(g.metadata_fetched_at) < datetime('now', '-7 days'))
                OR (g.metadata_status = 'unavailable'
                    AND datetime(g.metadata_fetched_at) < datetime('now', '-1 day'))
                OR (g.metadata_status = 'failed'
                    AND datetime(g.metadata_fetched_at) < datetime('now', '-2 hours'))
            )
         ON CONFLICT(app_id) DO UPDATE SET
            priority = CASE
                WHEN metadata_enrichment_queue.state IN ('pending', 'processing', 'retrying')
                    THEN MIN(metadata_enrichment_queue.priority, excluded.priority)
                ELSE excluded.priority
            END,
            state = CASE
                WHEN metadata_enrichment_queue.state IN ('pending', 'processing', 'retrying')
                    THEN metadata_enrichment_queue.state
                ELSE 'pending'
            END,
            attempts = CASE
                WHEN metadata_enrichment_queue.state IN ('pending', 'processing', 'retrying')
                    THEN metadata_enrichment_queue.attempts
                ELSE 0
            END,
            next_attempt_at = CASE
                WHEN metadata_enrichment_queue.state IN ('processing', 'retrying')
                    THEN metadata_enrichment_queue.next_attempt_at
                ELSE strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
            END,
            last_error_code = CASE
                WHEN metadata_enrichment_queue.state IN ('processing', 'retrying')
                    THEN metadata_enrichment_queue.last_error_code
                ELSE NULL
            END,
            finished_at = NULL,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![app_id, priority],
    )?;
    Ok(())
}

pub fn claim_ready(connection: &mut Connection, limit: usize) -> AppResult<Vec<MetadataJob>> {
    if limit == 0 || limit > 8 {
        return Err(AppError::validation(
            "El tamaño de trabajo de metadatos debe estar entre 1 y 8.",
        ));
    }
    let transaction = connection.transaction()?;
    transaction.execute(
        "UPDATE metadata_enrichment_queue
            SET state = 'pending', started_at = NULL,
                next_attempt_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE state = 'processing'
            AND datetime(started_at) <= datetime('now', '-5 minutes')",
        [],
    )?;
    let candidates = {
        let mut statement = transaction.prepare(
            "SELECT app_id, attempts + 1
               FROM metadata_enrichment_queue
              WHERE state IN ('pending', 'retrying')
                AND datetime(next_attempt_at) <= datetime('now')
              ORDER BY priority ASC, datetime(next_attempt_at) ASC,
                       datetime(enqueued_at) ASC, app_id ASC
              LIMIT ?1",
        )?;
        statement
            .query_map([limit as i64], |row| {
                Ok(MetadataJob {
                    app_id: row.get(0)?,
                    attempts: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for job in &candidates {
        let changed = transaction.execute(
            "UPDATE metadata_enrichment_queue
                SET state = 'processing', attempts = ?2,
                    started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE app_id = ?1 AND state IN ('pending', 'retrying')",
            params![job.app_id, job.attempts],
        )?;
        if changed != 1 {
            return Err(AppError::new(
                "metadata_queue_race",
                "La cola de metadatos cambió mientras se preparaba el lote.",
            ));
        }
    }
    transaction.commit()?;
    Ok(candidates)
}

pub fn mark_success(connection: &Connection, app_id: u32) -> AppResult<()> {
    mark_terminal(connection, app_id, "success", None)
}

pub fn mark_unavailable(connection: &Connection, app_id: u32) -> AppResult<()> {
    mark_terminal(connection, app_id, "unavailable", None)
}

pub fn schedule_retry(
    connection: &Connection,
    app_id: u32,
    error_code: &str,
    delay_seconds: u64,
) -> AppResult<()> {
    if delay_seconds == 0 || delay_seconds > 3_600 {
        return Err(AppError::validation(
            "La espera de reintento de metadatos no es válida.",
        ));
    }
    let changed = connection.execute(
        "UPDATE metadata_enrichment_queue
            SET state = 'retrying', last_error_code = ?2,
                next_attempt_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ?3 || ' seconds'),
                started_at = NULL, finished_at = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1 AND state = 'processing'",
        params![app_id, error_code, delay_seconds as i64],
    )?;
    ensure_changed(changed)
}

pub fn mark_failed(connection: &Connection, app_id: u32, error_code: &str) -> AppResult<()> {
    mark_terminal(connection, app_id, "failed", Some(error_code))
}

fn mark_terminal(
    connection: &Connection,
    app_id: u32,
    state: &str,
    error_code: Option<&str>,
) -> AppResult<()> {
    let changed = connection.execute(
        "UPDATE metadata_enrichment_queue
            SET state = ?2, last_error_code = ?3, started_at = NULL,
                finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1 AND state = 'processing'",
        params![app_id, state, error_code],
    )?;
    ensure_changed(changed)
}

fn ensure_changed(changed: usize) -> AppResult<()> {
    if changed == 1 {
        Ok(())
    } else {
        Err(AppError::new(
            "metadata_queue_stale_job",
            "El trabajo de metadatos ya no estaba en ejecución.",
        ))
    }
}

pub fn next_ready_delay_ms(connection: &Connection) -> AppResult<Option<u64>> {
    let seconds = connection
        .query_row(
            "SELECT MAX(0, CAST(ROUND((julianday(MIN(next_attempt_at)) - julianday('now')) * 86400) AS INTEGER))
               FROM metadata_enrichment_queue
              WHERE state IN ('pending', 'retrying')",
            [],
            |row| row.get::<_, Option<i64>>(0),
        )?
        .and_then(|value| u64::try_from(value).ok());
    Ok(seconds.map(|value| value.saturating_mul(1_000)))
}

pub fn status(connection: &Connection) -> AppResult<MetadataEnrichmentStatus> {
    let total_games = count_to_u64(
        connection.query_row(
            "SELECT COUNT(*) FROM games
          WHERE NOT (ownership_source = 'family_shared' AND family_availability <> 'confirmed')",
            [],
            |row| row.get::<_, i64>(0),
        )?,
        "juegos elegibles",
    )?;
    let fresh_metadata = count_to_u64(
        connection.query_row(
            "SELECT COUNT(*) FROM games
          WHERE metadata_status = 'success'
            AND datetime(metadata_fetched_at) >= datetime('now', '-7 days')
            AND NOT (ownership_source = 'family_shared' AND family_availability <> 'confirmed')",
            [],
            |row| row.get::<_, i64>(0),
        )?,
        "metadatos vigentes",
    )?;
    let counts = connection.query_row(
        "SELECT
            COALESCE(SUM(state = 'pending'), 0),
            COALESCE(SUM(state = 'processing'), 0),
            COALESCE(SUM(state = 'retrying'), 0),
            COALESCE(SUM(state = 'success'), 0),
            COALESCE(SUM(state = 'unavailable'), 0),
            COALESCE(SUM(state = 'failed'), 0)
         FROM metadata_enrichment_queue",
        [],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
            ))
        },
    )?;
    let queued = count_to_u64(counts.0, "trabajos pendientes")?;
    let processing = count_to_u64(counts.1, "trabajos en curso")?;
    let retrying = count_to_u64(counts.2, "trabajos aplazados")?;
    let succeeded = count_to_u64(counts.3, "trabajos completados")?;
    let unavailable = count_to_u64(counts.4, "fichas no disponibles")?;
    let failed = count_to_u64(counts.5, "trabajos fallidos")?;
    let next_retry_at = connection.query_row(
        "SELECT MIN(next_attempt_at) FROM metadata_enrichment_queue
              WHERE state = 'retrying'",
        [],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let last_error_code = connection
        .query_row(
            "SELECT last_error_code FROM metadata_enrichment_queue
              WHERE last_error_code IS NOT NULL
              ORDER BY datetime(updated_at) DESC, app_id DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    Ok(MetadataEnrichmentStatus {
        running: false,
        total_games,
        fresh_metadata,
        queued,
        processing,
        retrying,
        succeeded,
        unavailable,
        failed,
        next_retry_at,
        last_error_code,
        steam_deck_availability: "disabled".to_string(),
        steam_deck_explanation: "Steam no publica el estado de compatibilidad con Steam Deck en el endpoint documentado usado por Vindexa; el filtro permanece desactivado para no inventar datos.".to_string(),
    })
}

fn count_to_u64(value: i64, label: &str) -> AppResult<u64> {
    u64::try_from(value).map_err(|_| {
        AppError::new(
            "metadata_queue_count",
            format!("El recuento de {label} guardado no es válido."),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use rusqlite::{Connection, params};
    use std::time::{Duration, Instant};

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir base temporal");
        migrations::migrate(&mut connection).expect("aplicar migraciones");
        connection
            .execute(
                "INSERT INTO statuses(id, name, color, position, built_in)
                 VALUES ('backlog', 'Pendiente', '#FFFFFF', 0, 1)",
                [],
            )
            .expect("crear estado");
        for (app_id, title, installed, ownership, availability) in [
            (10, "Visible", 0, "owned", "not_applicable"),
            (20, "Instalado", 1, "local", "not_applicable"),
            (30, "Backlog", 0, "owned", "not_applicable"),
            (40, "Family no confirmado", 0, "family_shared", "unknown"),
        ] {
            connection
                .execute(
                    "INSERT INTO games(app_id, title, ownership_source, family_availability)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![app_id, title, ownership, availability],
                )
                .expect("crear juego");
            connection
                .execute(
                    "INSERT INTO game_personal(app_id, status_id, installed)
                     VALUES (?1, 'backlog', ?2)",
                    params![app_id, installed],
                )
                .expect("crear datos personales");
        }
        connection
    }

    #[test]
    fn visible_games_are_claimed_before_the_incremental_backlog() {
        let mut connection = database();
        assert_eq!(enqueue(&mut connection, &[30], true).expect("encolar"), 3);

        let first = claim_ready(&mut connection, 1).expect("reclamar visible");
        assert_eq!(
            first,
            vec![MetadataJob {
                app_id: 30,
                attempts: 1
            }]
        );
        mark_success(&connection, 30).expect("completar visible");

        let next = claim_ready(&mut connection, 2).expect("reclamar backlog");
        assert_eq!(next[0].app_id, 20, "los instalados se enriquecen antes");
        assert_eq!(next[1].app_id, 10);
        assert!(next.iter().all(|job| job.app_id != 40));
    }

    #[test]
    fn enqueue_deduplicates_and_does_not_refetch_fresh_metadata() {
        let mut connection = database();
        enqueue(&mut connection, &[10, 10], false).expect("primera cola");
        enqueue(&mut connection, &[10], false).expect("segunda cola");
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM metadata_enrichment_queue",
                [],
                |row| row.get(0),
            )
            .expect("contar cola");
        assert_eq!(count, 1);

        connection
            .execute(
                "UPDATE games SET metadata_status = 'success',
                    metadata_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE app_id = 20",
                [],
            )
            .expect("marcar caché fresca");
        enqueue(&mut connection, &[], true).expect("encolar backlog");
        let fresh_queued: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM metadata_enrichment_queue WHERE app_id = 20",
                [],
                |row| row.get(0),
            )
            .expect("consultar caché");
        assert_eq!(fresh_queued, 0);
    }

    #[test]
    fn retry_is_persisted_and_is_not_claimed_before_its_backoff() {
        let mut connection = database();
        enqueue(&mut connection, &[10], false).expect("encolar");
        let job = claim_ready(&mut connection, 1).expect("reclamar").remove(0);
        schedule_retry(&connection, job.app_id, "steam_store_rate_limited", 90).expect("aplazar");

        assert!(
            claim_ready(&mut connection, 1)
                .expect("reclamar antes")
                .is_empty()
        );
        let snapshot = status(&connection).expect("estado");
        assert_eq!(snapshot.retrying, 1);
        assert_eq!(
            snapshot.last_error_code.as_deref(),
            Some("steam_store_rate_limited")
        );
        assert!(snapshot.next_retry_at.is_some());
        assert!(next_ready_delay_ms(&connection).expect("demora").is_some());
    }

    #[test]
    fn an_old_visible_priority_does_not_leak_into_a_later_ttl_refresh() {
        let mut connection = database();
        enqueue(&mut connection, &[10], false).expect("priorizar visible");
        claim_ready(&mut connection, 1).expect("reclamar visible");
        mark_failed(&connection, 10, "steam_store_response").expect("cerrar intento");
        connection
            .execute(
                "UPDATE games SET metadata_status = 'failed',
                    metadata_fetched_at = datetime('now', '-3 hours')
                  WHERE app_id = 10",
                [],
            )
            .expect("hacer vencer TTL");

        enqueue(&mut connection, &[], true).expect("reencolar incremental");
        let priority: i64 = connection
            .query_row(
                "SELECT priority FROM metadata_enrichment_queue WHERE app_id = 10",
                [],
                |row| row.get(0),
            )
            .expect("leer prioridad renovada");
        assert_eq!(priority, 100);
    }

    #[test]
    fn orphaned_processing_jobs_are_recovered_after_a_crash() {
        let mut connection = database();
        enqueue(&mut connection, &[10], false).expect("encolar");
        claim_ready(&mut connection, 1).expect("reclamar");
        connection
            .execute(
                "UPDATE metadata_enrichment_queue
                    SET started_at = datetime('now', '-10 minutes')
                  WHERE app_id = 10",
                [],
            )
            .expect("simular cierre");

        let recovered = claim_ready(&mut connection, 1).expect("recuperar");
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].app_id, 10);
        assert_eq!(recovered[0].attempts, 2);
    }

    #[test]
    fn visible_priority_input_is_bounded_and_validated() {
        let mut connection = database();
        let too_many = (1..=(MAX_VISIBLE_PRIORITY_IDS as u32 + 1)).collect::<Vec<_>>();
        let error = enqueue(&mut connection, &too_many, false).expect_err("rechazar lote enorme");
        assert_eq!(error.code, "metadata_queue_visible_limit");
        let error = enqueue(&mut connection, &[0], false).expect_err("rechazar AppID cero");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn a_realistic_large_library_is_queued_in_one_local_batch_not_network_fanout() {
        let mut connection = database();
        let transaction = connection.transaction().expect("abrir lote");
        for app_id in 1000..2754_u32 {
            transaction
                .execute(
                    "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                    params![app_id, format!("Juego {app_id}")],
                )
                .expect("crear juego de carga");
            transaction
                .execute(
                    "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'backlog')",
                    [app_id],
                )
                .expect("crear personal de carga");
        }
        transaction.commit().expect("confirmar lote");

        let started = Instant::now();
        let queued = enqueue(&mut connection, &[10, 20], true).expect("encolar biblioteca");
        let elapsed = started.elapsed();

        assert_eq!(queued, 1_757);
        assert!(
            elapsed < Duration::from_secs(3),
            "la planificación SQLite tardó {elapsed:?}"
        );
        assert_eq!(
            claim_ready(&mut connection, 2)
                .expect("reclamar lote")
                .len(),
            2,
            "el worker nunca convierte la cola completa en solicitudes simultáneas"
        );
    }
}
