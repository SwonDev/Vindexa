use crate::db::library;
use crate::error::{AppError, AppResult};
use crate::models::{GameSummary, Recommendation, RecommendationRequest};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DISCOVERY_LIST_LIMIT: usize = 12;
const HISTORY_LIMIT: usize = 30;
const RECENT_RECOMMENDATION_WINDOW: usize = 24;
const NEWS_LIST_LIMIT: usize = 20;
const RELATED_RELEASES_LIMIT: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameReminder {
    pub id: String,
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub due_at: String,
    pub note: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReminderInput {
    pub app_id: u32,
    pub due_at: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryEvent {
    pub id: String,
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub kind: String,
    pub previous_value: Option<String>,
    pub current_value: Option<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DismissedRecommendation {
    pub id: String,
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub duration_minutes: Option<u32>,
    pub mood: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsRefreshCandidate {
    pub app_id: u32,
    pub attempts: u32,
}

#[derive(Debug, Clone)]
pub struct CachedNewsInput {
    pub gid: String,
    pub title: String,
    pub content_preview: String,
    pub published_at: String,
    pub feed_label: String,
    pub feed_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfficialPublication {
    pub gid: String,
    pub app_id: u32,
    pub game_title: String,
    pub icon_url: Option<String>,
    pub title: String,
    pub content_preview: String,
    pub published_at: String,
    pub feed_label: String,
    pub feed_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedRelease {
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub cover_url: Option<String>,
    pub release_date: String,
    pub related_to_app_id: u32,
    pub related_to_title: String,
    pub criterion: String,
    pub criterion_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewsRefreshReport {
    pub attempted_games: u32,
    pub refreshed_games: u32,
    pub publications_saved: u32,
    pub failed_games: u32,
    pub skipped_by_cadence: u32,
    pub next_refresh_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryCapabilities {
    pub metadata_observations: u32,
    pub early_access_history_available: bool,
    pub tracked_news_games: u32,
    pub official_publications_available: bool,
    pub news_last_refreshed_at: Option<String>,
    pub news_next_refresh_at: Option<String>,
    pub related_releases_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySnapshot {
    pub reminders: Vec<GameReminder>,
    pub forgotten: Vec<GameSummary>,
    pub almost_finished: Vec<GameSummary>,
    pub upcoming: Vec<GameSummary>,
    pub events: Vec<DiscoveryEvent>,
    pub official_publications: Vec<OfficialPublication>,
    pub related_releases: Vec<RelatedRelease>,
    pub dismissed_recommendations: Vec<DismissedRecommendation>,
    pub capabilities: DiscoveryCapabilities,
}

pub fn snapshot(connection: &Connection) -> AppResult<DiscoverySnapshot> {
    let reminders = list_reminders(connection)?;
    let forgotten = summaries_for_query(
        connection,
        "SELECT g.app_id
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          WHERE p.status_id NOT IN ('completed', 'abandoned')
            AND p.progress < 100
            AND (
                (g.last_played_at IS NOT NULL
                 AND julianday('now') - julianday(g.last_played_at) >= 365)
                OR
                (g.last_played_at IS NULL
                 AND julianday('now') - julianday(g.imported_at) >= 180)
            )
          ORDER BY g.last_played_at IS NULL ASC,
                   g.last_played_at ASC,
                   g.imported_at ASC,
                   g.title COLLATE NOCASE ASC,
                   g.app_id ASC
          LIMIT ?1",
        DISCOVERY_LIST_LIMIT,
    )?;
    let almost_finished = summaries_for_query(
        connection,
        "SELECT g.app_id
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          WHERE p.progress BETWEEN 75 AND 99
            AND p.status_id NOT IN ('completed', 'abandoned')
          ORDER BY p.progress DESC,
                   g.last_played_at IS NULL ASC,
                   g.last_played_at DESC,
                   g.title COLLATE NOCASE ASC,
                   g.app_id ASC
          LIMIT ?1",
        DISCOVERY_LIST_LIMIT,
    )?;
    let upcoming = summaries_for_query(
        connection,
        "SELECT g.app_id
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          WHERE g.release_date IS NOT NULL
            AND date(g.release_date) > date('now')
            AND p.status_id <> 'abandoned'
          ORDER BY date(g.release_date) ASC,
                   g.title COLLATE NOCASE ASC,
                   g.app_id ASC
          LIMIT ?1",
        DISCOVERY_LIST_LIMIT,
    )?;
    let events = list_events(connection)?;
    let official_publications = list_official_publications(connection)?;
    let related_releases = list_related_releases(connection)?;
    let dismissed_recommendations = list_dismissed_recommendations(connection)?;
    let metadata_observations = connection.query_row(
        "SELECT COUNT(*) FROM game_metadata_observations",
        [],
        |row| row.get::<_, u32>(0),
    )?;
    let comparable_metadata_available = connection.query_row(
        "SELECT EXISTS(
            SELECT 1
              FROM game_metadata_observations
             GROUP BY app_id
            HAVING COUNT(*) >= 2
         )",
        [],
        |row| row.get::<_, bool>(0),
    )?;
    let tracked_news_games = connection.query_row(
        "SELECT COUNT(*) FROM game_personal WHERE tracking = 1",
        [],
        |row| row.get::<_, u32>(0),
    )?;
    let news_last_refreshed_at = connection.query_row(
        "SELECT MAX(s.last_success_at)
           FROM steam_news_fetch_state s
           JOIN game_personal p ON p.app_id = s.app_id
          WHERE p.tracking = 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let news_next_refresh_at = connection.query_row(
        "SELECT MIN(s.next_attempt_at)
               FROM steam_news_fetch_state s
               JOIN game_personal p ON p.app_id = s.app_id
              WHERE p.tracking = 1",
        [],
        |row| row.get::<_, Option<String>>(0),
    )?;
    let official_publications_available = !official_publications.is_empty();
    let related_releases_available = !related_releases.is_empty();

    Ok(DiscoverySnapshot {
        reminders,
        forgotten,
        almost_finished,
        upcoming,
        events,
        official_publications,
        related_releases,
        dismissed_recommendations,
        capabilities: DiscoveryCapabilities {
            metadata_observations,
            early_access_history_available: comparable_metadata_available,
            tracked_news_games,
            official_publications_available,
            news_last_refreshed_at,
            news_next_refresh_at,
            related_releases_available,
        },
    })
}

pub fn save_reminder(
    connection: &Connection,
    input: &SaveReminderInput,
) -> AppResult<GameReminder> {
    validate_reminder_input(input)?;
    let game_exists = connection
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [input.app_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !game_exists {
        return Err(AppError::not_found("El juego del recordatorio no existe."));
    }

    let id = Uuid::new_v4().to_string();
    connection.execute(
        "INSERT INTO game_reminders(id, app_id, due_at, note)
         VALUES (?1, ?2, ?3, ?4)",
        params![id, input.app_id, input.due_at, input.note.trim()],
    )?;
    reminder_by_id(connection, &id)
}

pub fn complete_reminder(connection: &Connection, id: &str) -> AppResult<()> {
    validate_uuid(id, "El identificador del recordatorio no es válido.")?;
    let changed = connection.execute(
        "UPDATE game_reminders
            SET completed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1 AND completed_at IS NULL",
        [id],
    )?;
    if changed == 0 {
        return Err(AppError::not_found(
            "El recordatorio pendiente ya no existe.",
        ));
    }
    Ok(())
}

pub fn snooze_reminder(connection: &Connection, id: &str, due_at: &str) -> AppResult<GameReminder> {
    validate_uuid(id, "El identificador del recordatorio no es válido.")?;
    validate_due_at(due_at)?;
    let changed = connection.execute(
        "UPDATE game_reminders
            SET due_at = ?2,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE id = ?1 AND completed_at IS NULL",
        params![id, due_at],
    )?;
    if changed == 0 {
        return Err(AppError::not_found(
            "El recordatorio pendiente ya no existe.",
        ));
    }
    reminder_by_id(connection, id)
}

pub fn dismiss_recommendation(connection: &Connection, history_id: &str) -> AppResult<()> {
    validate_uuid(
        history_id,
        "El identificador de la recomendación no es válido.",
    )?;
    let changed = connection.execute(
        "UPDATE recommendation_history SET dismissed = 1 WHERE id = ?1",
        [history_id],
    )?;
    if changed == 0 {
        return Err(AppError::not_found("La recomendación guardada no existe."));
    }
    Ok(())
}

pub fn restore_recommendation(connection: &Connection, history_id: &str) -> AppResult<()> {
    validate_uuid(
        history_id,
        "El identificador de la recomendación no es válido.",
    )?;
    let changed = connection.execute(
        "DELETE FROM recommendation_history WHERE id = ?1 AND dismissed = 1",
        [history_id],
    )?;
    if changed == 0 {
        return Err(AppError::not_found(
            "La recomendación descartada ya no existe.",
        ));
    }
    Ok(())
}

pub fn recommend(
    connection: &mut Connection,
    request: &RecommendationRequest,
) -> AppResult<Option<Recommendation>> {
    validate_recommendation_request(request)?;
    let mood = normalized_mood(request.mood.as_deref())?;
    let mut chosen = find_recommendation(connection, request.duration_minutes, mood, true)?;
    if chosen.is_none() {
        chosen = find_recommendation(connection, request.duration_minutes, mood, false)?;
    }
    let Some((game, duration_matched, experience_reason)) = chosen else {
        return Ok(None);
    };

    let history_id = Uuid::new_v4().to_string();
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO recommendation_history(id, app_id, duration_minutes, mood, dismissed)
         VALUES (?1, ?2, ?3, ?4, 0)",
        params![history_id, game.app_id, request.duration_minutes, mood],
    )?;
    transaction.commit()?;

    let mut reasons = Vec::new();
    if game.priority > 0 {
        reasons.push(format!("Prioridad {}/5", game.priority));
    }
    if game.tracking {
        reasons.push("Está en seguimiento".to_string());
    }
    if game.progress >= 75 {
        reasons.push(format!("Está al {} %", game.progress));
    }
    if let Some(duration) = request.duration_minutes {
        if duration_matched {
            reasons.push(format!("Tu estimación encaja en {duration} minutos"));
        } else {
            reasons.push("Sin estimación comparable; no se inventó una duración".to_string());
        }
    }
    if let Some(reason) = experience_reason {
        reasons.push(reason);
    }
    if reasons.is_empty() {
        reasons.push("Siguiente título según tu organización personal".to_string());
    }

    Ok(Some(Recommendation {
        history_id,
        game,
        reasons,
    }))
}

pub fn record_metadata_observation(
    connection: &mut Connection,
    app_id: u32,
    is_early_access: Option<bool>,
    release_date: Option<&str>,
    source_fetched_at: &str,
) -> AppResult<()> {
    DateTime::parse_from_rfc3339(source_fetched_at)
        .map_err(|_| AppError::validation("La fecha de la observación no es válida."))?;
    let previous = connection
        .query_row(
            "SELECT is_early_access, release_date
               FROM game_metadata_observations
              WHERE app_id = ?1
              ORDER BY observed_at DESC, rowid DESC
              LIMIT 1",
            [app_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.map(|value| value != 0),
                    row.get::<_, Option<String>>(1)?,
                ))
            },
        )
        .optional()?;

    let transaction = connection.transaction()?;
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO game_metadata_observations(
             id, app_id, is_early_access, release_date, source_fetched_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Uuid::new_v4().to_string(),
            app_id,
            is_early_access,
            release_date,
            source_fetched_at
        ],
    )?;
    if inserted == 0 {
        transaction.commit()?;
        return Ok(());
    }

    if let Some((previous_early_access, previous_release_date)) = previous {
        if let (Some(before), Some(after)) = (previous_early_access, is_early_access)
            && before != after
        {
            insert_event(
                &transaction,
                app_id,
                "early_access_changed",
                Some(if before { "early_access" } else { "released" }),
                Some(if after { "early_access" } else { "released" }),
            )?;
        }
        if let (Some(before), Some(after)) = (previous_release_date.as_deref(), release_date)
            && before != after
        {
            insert_event(
                &transaction,
                app_id,
                "release_date_changed",
                Some(before),
                Some(after),
            )?;
        }
    }
    transaction.commit()?;
    Ok(())
}

pub fn claim_news_refresh_candidates(
    connection: &mut Connection,
    limit: usize,
) -> AppResult<Vec<NewsRefreshCandidate>> {
    if limit == 0 || limit > 8 {
        return Err(AppError::validation(
            "El lote de publicaciones debe contener entre 1 y 8 juegos.",
        ));
    }
    let transaction = connection.transaction()?;
    let candidates = {
        let mut statement = transaction.prepare(
            "SELECT g.app_id, COALESCE(s.consecutive_failures, 0) + 1
               FROM games g
               JOIN game_personal p ON p.app_id = g.app_id
               LEFT JOIN steam_news_fetch_state s ON s.app_id = g.app_id
              WHERE p.tracking = 1
                AND (s.next_attempt_at IS NULL OR datetime(s.next_attempt_at) <= datetime('now'))
              ORDER BY s.last_success_at IS NOT NULL ASC,
                       datetime(s.last_success_at) ASC,
                       g.title COLLATE NOCASE ASC,
                       g.app_id ASC
              LIMIT ?1",
        )?;
        statement
            .query_map([limit as i64], |row| {
                Ok(NewsRefreshCandidate {
                    app_id: row.get(0)?,
                    attempts: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
    };
    for candidate in &candidates {
        transaction.execute(
            "INSERT INTO steam_news_fetch_state(
                 app_id, consecutive_failures, last_attempt_at, next_attempt_at
             ) VALUES (
                 ?1, 0, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+5 minutes')
             )
             ON CONFLICT(app_id) DO UPDATE SET
                last_attempt_at = excluded.last_attempt_at,
                next_attempt_at = excluded.next_attempt_at,
                updated_at = excluded.last_attempt_at",
            [candidate.app_id],
        )?;
    }
    transaction.commit()?;
    Ok(candidates)
}

pub fn save_news_success(
    connection: &mut Connection,
    app_id: u32,
    items: &[CachedNewsInput],
) -> AppResult<()> {
    if app_id == 0 || items.len() > 8 {
        return Err(AppError::validation(
            "El resultado de publicaciones de Steam no es válido.",
        ));
    }
    for item in items {
        validate_cached_news(item)?;
    }
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM steam_news_items WHERE app_id = ?1", [app_id])?;
    {
        let mut insert = transaction.prepare_cached(
            "INSERT INTO steam_news_items(
                 app_id, gid, title, content_preview, published_at, feed_label, feed_name
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for item in items {
            insert.execute(params![
                app_id,
                item.gid,
                item.title,
                item.content_preview,
                item.published_at,
                item.feed_label,
                item.feed_name,
            ])?;
        }
    }
    transaction.execute(
        "INSERT INTO steam_news_fetch_state(
             app_id, consecutive_failures, last_attempt_at, last_success_at,
             next_attempt_at, last_error_code
         ) VALUES (
             ?1, 0,
             strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+6 hours'),
             NULL
         )
         ON CONFLICT(app_id) DO UPDATE SET
            consecutive_failures = 0,
            last_attempt_at = excluded.last_attempt_at,
            last_success_at = excluded.last_success_at,
            next_attempt_at = excluded.next_attempt_at,
            last_error_code = NULL,
            updated_at = excluded.last_attempt_at",
        [app_id],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn save_news_failure(
    connection: &Connection,
    app_id: u32,
    attempts: u32,
    error_code: &str,
    retry_delay_seconds: u64,
) -> AppResult<()> {
    if app_id == 0
        || attempts == 0
        || error_code.is_empty()
        || error_code.len() > 80
        || !(60..=24 * 60 * 60).contains(&retry_delay_seconds)
    {
        return Err(AppError::validation(
            "El reintento del feed de Steam no es válido.",
        ));
    }
    connection.execute(
        "INSERT INTO steam_news_fetch_state(
             app_id, consecutive_failures, last_attempt_at, next_attempt_at, last_error_code
         ) VALUES (
             ?1, ?2,
             strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
             strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+' || ?4 || ' seconds'),
             ?3
         )
         ON CONFLICT(app_id) DO UPDATE SET
            consecutive_failures = excluded.consecutive_failures,
            last_attempt_at = excluded.last_attempt_at,
            next_attempt_at = excluded.next_attempt_at,
            last_error_code = excluded.last_error_code,
            updated_at = excluded.last_attempt_at",
        params![app_id, attempts, error_code, retry_delay_seconds as i64],
    )?;
    Ok(())
}

fn validate_cached_news(item: &CachedNewsInput) -> AppResult<()> {
    if item.gid.is_empty()
        || item.gid.len() > 40
        || !item.gid.bytes().all(|byte| byte.is_ascii_digit())
        || item.title.trim().is_empty()
        || item.title.chars().count() > 240
        || item.content_preview.chars().count() > 360
        || item.feed_label.chars().count() > 80
        || item.feed_name != "steam_community_announcements"
        || DateTime::parse_from_rfc3339(&item.published_at).is_err()
    {
        return Err(AppError::validation(
            "Steam devolvió una publicación que no cumple el contrato local.",
        ));
    }
    Ok(())
}

fn list_official_publications(connection: &Connection) -> AppResult<Vec<OfficialPublication>> {
    let mut statement = connection.prepare(
        "SELECT n.gid, n.app_id, g.title, g.icon_url, n.title, n.content_preview,
                n.published_at, n.feed_label, n.feed_name
           FROM steam_news_items n
           JOIN games g ON g.app_id = n.app_id
           JOIN game_personal p ON p.app_id = n.app_id
          WHERE p.tracking = 1
          ORDER BY datetime(n.published_at) DESC, g.title COLLATE NOCASE ASC,
                   n.gid ASC
          LIMIT ?1",
    )?;
    statement
        .query_map([NEWS_LIST_LIMIT as i64], |row| {
            Ok(OfficialPublication {
                gid: row.get(0)?,
                app_id: row.get(1)?,
                game_title: row.get(2)?,
                icon_url: row.get(3)?,
                title: row.get(4)?,
                content_preview: row.get(5)?,
                published_at: row.get(6)?,
                feed_label: row.get(7)?,
                feed_name: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[derive(Debug)]
struct RelatedGameFacts {
    app_id: u32,
    title: String,
    icon_url: Option<String>,
    cover_url: Option<String>,
    release_date: Option<String>,
    developers: Vec<String>,
    publishers: Vec<String>,
    engagement: i64,
}

fn list_related_releases(connection: &Connection) -> AppResult<Vec<RelatedRelease>> {
    let mut statement = connection.prepare(
        "SELECT g.app_id, g.title, g.icon_url, g.cover_url, g.release_date,
                g.developer, g.publisher,
                (CASE WHEN p.tracking = 1 THEN 100000 ELSE 0 END) +
                (CASE WHEN p.pinned = 1 THEN 50000 ELSE 0 END) +
                (p.progress * 100) + MIN(g.playtime_minutes, 10000)
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          WHERE g.metadata_status = 'success'
            AND g.metadata_fetched_at IS NOT NULL
            AND (g.ownership_source <> 'family_shared' OR g.family_availability = 'confirmed')
            AND (
                (g.developer IS NOT NULL AND trim(g.developer) <> '')
                OR (g.publisher IS NOT NULL AND trim(g.publisher) <> '')
            )",
    )?;
    let games = statement
        .query_map([], |row| {
            Ok(RelatedGameFacts {
                app_id: row.get(0)?,
                title: row.get(1)?,
                icon_url: row.get(2)?,
                cover_url: row.get(3)?,
                release_date: row.get(4)?,
                developers: normalize_companies(row.get::<_, Option<String>>(5)?.as_deref()),
                publishers: normalize_companies(row.get::<_, Option<String>>(6)?.as_deref()),
                engagement: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let today = Utc::now().date_naive();
    let mut future_games = games
        .iter()
        .filter(|game| {
            game.release_date
                .as_deref()
                .and_then(parse_iso_date)
                .is_some_and(|date| date > today)
        })
        .collect::<Vec<_>>();
    future_games.sort_by(|left, right| {
        left.release_date
            .cmp(&right.release_date)
            .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
            .then_with(|| left.app_id.cmp(&right.app_id))
    });

    let is_historical_source = |game: &RelatedGameFacts| {
        game.engagement > 0
            && game
                .release_date
                .as_deref()
                .and_then(parse_iso_date)
                .is_none_or(|date| date <= today)
    };
    let mut developer_index: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut publisher_index: HashMap<&str, Vec<usize>> = HashMap::new();
    for (index, game) in games
        .iter()
        .enumerate()
        .filter(|(_, game)| is_historical_source(game))
    {
        for company in &game.developers {
            developer_index.entry(company).or_default().push(index);
        }
        for company in &game.publishers {
            publisher_index.entry(company).or_default().push(index);
        }
    }

    let mut related = Vec::new();
    for future in future_games {
        let mut seen = HashSet::new();
        let mut matches = Vec::new();
        for company in &future.developers {
            for &index in developer_index.get(company.as_str()).into_iter().flatten() {
                if seen.insert(index) {
                    matches.push((&games[index], "developer", company.clone()));
                }
            }
        }
        for company in &future.publishers {
            for &index in publisher_index.get(company.as_str()).into_iter().flatten() {
                if seen.insert(index) {
                    matches.push((&games[index], "publisher", company.clone()));
                }
            }
        }
        matches.sort_by(|(left, left_kind, _), (right, right_kind, _)| {
            (*left_kind != "developer")
                .cmp(&(*right_kind != "developer"))
                .then_with(|| right.engagement.cmp(&left.engagement))
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
        });
        let Some((source, criterion, criterion_value)) = matches.into_iter().next() else {
            continue;
        };
        related.push(RelatedRelease {
            app_id: future.app_id,
            title: future.title.clone(),
            icon_url: future.icon_url.clone(),
            cover_url: future.cover_url.clone(),
            release_date: future.release_date.clone().unwrap_or_default(),
            related_to_app_id: source.app_id,
            related_to_title: source.title.clone(),
            criterion: criterion.to_string(),
            criterion_value,
        });
        if related.len() >= RELATED_RELEASES_LIMIT {
            break;
        }
    }
    Ok(related)
}

fn normalize_company(value: &str) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn normalize_companies(value: Option<&str>) -> Vec<String> {
    value
        .into_iter()
        .flat_map(|value| value.split(','))
        .filter_map(normalize_company)
        .collect()
}

fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

fn summaries_for_query(
    connection: &Connection,
    sql: &str,
    limit: usize,
) -> AppResult<Vec<GameSummary>> {
    let mut statement = connection.prepare(sql)?;
    let ids = statement
        .query_map([limit as i64], |row| row.get::<_, u32>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|app_id| library::game_summary(connection, app_id))
        .collect()
}

fn list_reminders(connection: &Connection) -> AppResult<Vec<GameReminder>> {
    let mut statement = connection.prepare(
        "SELECT r.id, r.app_id, g.title, g.icon_url, r.due_at, r.note, r.completed_at
           FROM game_reminders r
           JOIN games g ON g.app_id = r.app_id
          WHERE r.completed_at IS NULL
          ORDER BY r.due_at ASC, g.title COLLATE NOCASE ASC, r.id ASC
          LIMIT 100",
    )?;
    statement
        .query_map([], map_reminder)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn reminder_by_id(connection: &Connection, id: &str) -> AppResult<GameReminder> {
    connection
        .query_row(
            "SELECT r.id, r.app_id, g.title, g.icon_url, r.due_at, r.note, r.completed_at
               FROM game_reminders r
               JOIN games g ON g.app_id = r.app_id
              WHERE r.id = ?1",
            [id],
            map_reminder,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El recordatorio no existe."))
}

fn map_reminder(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameReminder> {
    Ok(GameReminder {
        id: row.get(0)?,
        app_id: row.get(1)?,
        title: row.get(2)?,
        icon_url: row.get(3)?,
        due_at: row.get(4)?,
        note: row.get(5)?,
        completed_at: row.get(6)?,
    })
}

fn list_events(connection: &Connection) -> AppResult<Vec<DiscoveryEvent>> {
    let mut statement = connection.prepare(
        "SELECT e.id, e.app_id, g.title, g.icon_url, e.kind,
                e.previous_value, e.current_value, e.observed_at
           FROM discovery_events e
           JOIN games g ON g.app_id = e.app_id
          ORDER BY e.observed_at DESC, e.id ASC
          LIMIT 50",
    )?;
    statement
        .query_map([], |row| {
            Ok(DiscoveryEvent {
                id: row.get(0)?,
                app_id: row.get(1)?,
                title: row.get(2)?,
                icon_url: row.get(3)?,
                kind: row.get(4)?,
                previous_value: row.get(5)?,
                current_value: row.get(6)?,
                observed_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn list_dismissed_recommendations(
    connection: &Connection,
) -> AppResult<Vec<DismissedRecommendation>> {
    let mut statement = connection.prepare(
        "SELECT h.id, h.app_id, g.title, g.icon_url,
                h.duration_minutes, h.mood, h.created_at
           FROM recommendation_history h
           JOIN games g ON g.app_id = h.app_id
          WHERE h.dismissed = 1
          ORDER BY h.created_at DESC, h.id ASC
          LIMIT ?1",
    )?;
    statement
        .query_map([HISTORY_LIMIT as i64], |row| {
            Ok(DismissedRecommendation {
                id: row.get(0)?,
                app_id: row.get(1)?,
                title: row.get(2)?,
                icon_url: row.get(3)?,
                duration_minutes: row.get(4)?,
                mood: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(Into::into)
}

fn find_recommendation(
    connection: &Connection,
    duration_minutes: Option<u32>,
    mood: Option<&str>,
    exclude_recent: bool,
) -> AppResult<Option<(GameSummary, bool, Option<String>)>> {
    let mut statement = connection.prepare(
        "SELECT g.app_id, g.genres_json, g.categories_json, p.estimated_minutes
           FROM games g
           JOIN game_personal p ON p.app_id = g.app_id
          WHERE p.status_id NOT IN ('completed', 'abandoned')
            AND NOT EXISTS (
                SELECT 1 FROM recommendation_history dismissed
                 WHERE dismissed.app_id = g.app_id AND dismissed.dismissed = 1
            )
            AND (?1 = 0 OR g.app_id NOT IN (
                SELECT recent.app_id
                  FROM recommendation_history recent
                 WHERE recent.dismissed = 0
                 ORDER BY recent.created_at DESC
                 LIMIT ?2
            ))
          ORDER BY p.priority DESC, p.tracking DESC, p.pinned DESC,
                   p.progress DESC, g.last_played_at IS NULL ASC,
                   g.last_played_at DESC, g.title COLLATE NOCASE ASC, g.app_id ASC
          LIMIT 5000",
    )?;
    let candidates = statement
        .query_map(
            params![exclude_recent, RECENT_RECOMMENDATION_WINDOW as i64],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<u32>>(3)?,
                ))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    let mut fallback_without_duration = None;
    for (app_id, genres_json, categories_json, estimate) in candidates {
        let experience_reason = experience_match_reason(mood, &genres_json, &categories_json)?;
        if mood.is_some() && experience_reason.is_none() {
            continue;
        }
        let game = library::game_summary(connection, app_id)?;
        let duration_matched = duration_minutes
            .zip(estimate)
            .is_some_and(|(available, estimate)| estimate <= available);
        if duration_minutes.is_none() || duration_matched {
            return Ok(Some((game, duration_matched, experience_reason)));
        }
        if estimate.is_none() && fallback_without_duration.is_none() {
            fallback_without_duration = Some((game, false, experience_reason));
        }
    }
    Ok(fallback_without_duration)
}

fn experience_match_reason(
    mood: Option<&str>,
    genres_json: &str,
    categories_json: &str,
) -> AppResult<Option<String>> {
    let Some(mood) = mood else {
        return Ok(None);
    };
    let parse_facts = |value: &str| {
        serde_json::from_str::<Vec<String>>(value).map_err(|_| {
            AppError::new(
                "database_data",
                "Los géneros guardados para la recomendación no son válidos.",
            )
        })
    };
    let mut facts = parse_facts(genres_json)?;
    facts.extend(parse_facts(categories_json)?);
    let (labels, title) = match mood {
        "relaxed" => (
            &["casual", "simulation", "simulación"][..],
            "experiencia relajada",
        ),
        "focused" => (
            &["strategy", "estrategia", "puzzle", "rpg"][..],
            "experiencia concentrada",
        ),
        "competitive" => (
            &[
                "pvp",
                "multi-player",
                "multijugador",
                "sports",
                "deportes",
                "racing",
                "carreras",
            ][..],
            "experiencia competitiva",
        ),
        "story" => (
            &[
                "adventure",
                "aventura",
                "rpg",
                "single-player",
                "un jugador",
            ][..],
            "experiencia narrativa",
        ),
        _ => return Ok(None),
    };
    let matched = facts.iter().find(|fact| {
        let normalized = fact.to_lowercase();
        labels.iter().any(|label| normalized.contains(label))
    });
    Ok(matched.map(|fact| format!("{title}: dato de Steam «{fact}»")))
}

fn insert_event(
    connection: &Connection,
    app_id: u32,
    kind: &str,
    previous_value: Option<&str>,
    current_value: Option<&str>,
) -> AppResult<()> {
    connection.execute(
        "INSERT INTO discovery_events(id, app_id, kind, previous_value, current_value)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            Uuid::new_v4().to_string(),
            app_id,
            kind,
            previous_value,
            current_value
        ],
    )?;
    Ok(())
}

fn validate_reminder_input(input: &SaveReminderInput) -> AppResult<()> {
    if input.app_id == 0 {
        return Err(AppError::validation("El juego no es válido."));
    }
    validate_due_at(&input.due_at)?;
    if input.note.chars().count() > 500 {
        return Err(AppError::validation(
            "La nota del recordatorio no puede superar 500 caracteres.",
        ));
    }
    Ok(())
}

fn validate_due_at(value: &str) -> AppResult<()> {
    DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| AppError::validation("La fecha del recordatorio no es válida."))
}

fn validate_recommendation_request(request: &RecommendationRequest) -> AppResult<()> {
    if request
        .duration_minutes
        .is_some_and(|duration| !matches!(duration, 30 | 60 | 120))
    {
        return Err(AppError::validation(
            "El tiempo disponible debe ser 30, 60 o 120 minutos.",
        ));
    }
    normalized_mood(request.mood.as_deref())?;
    Ok(())
}

fn normalized_mood(value: Option<&str>) -> AppResult<Option<&str>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if matches!(value, "relaxed" | "focused" | "competitive" | "story") {
        Ok(Some(value))
    } else {
        Err(AppError::validation(
            "El tipo de experiencia seleccionado no es válido.",
        ))
    }
}

fn validate_uuid(value: &str, message: &str) -> AppResult<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| AppError::validation(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite");
        migrations::migrate(&mut connection).expect("aplicar migraciones");
        connection
            .execute_batch(
                "INSERT INTO statuses(id, name, color, position, built_in)
                 VALUES ('unclassified', 'Sin clasificar', '#71838E', 0, 1);
                 INSERT INTO games(app_id, title, genres_json, categories_json, imported_at)
                 VALUES
                   (10, 'Viajero', '[\"Adventure\"]', '[\"Single-player\"]', '2024-01-01T00:00:00Z'),
                   (20, 'Estratega', '[\"Strategy\"]', '[]', '2024-01-01T00:00:00Z'),
                   (30, 'Pendiente', '[]', '[]', '2024-01-01T00:00:00Z');
                 INSERT INTO game_personal(app_id, status_id, progress, priority, estimated_minutes)
                 VALUES
                   (10, 'unclassified', 80, 2, 60),
                   (20, 'unclassified', 10, 4, 120),
                   (30, 'unclassified', 0, 1, NULL);",
            )
            .expect("preparar juegos");
        connection
    }

    #[test]
    fn reminder_round_trip_is_persistent_and_recoverable() {
        let connection = database();
        let reminder = save_reminder(
            &connection,
            &SaveReminderInput {
                app_id: 10,
                due_at: "2026-08-20T18:00:00+00:00".to_string(),
                note: "Retomar el capítulo".to_string(),
            },
        )
        .expect("guardar recordatorio");

        assert_eq!(snapshot(&connection).expect("snapshot").reminders.len(), 1);
        let snoozed = snooze_reminder(&connection, &reminder.id, "2026-08-27T18:00:00+00:00")
            .expect("posponer");
        assert_eq!(snoozed.due_at, "2026-08-27T18:00:00+00:00");
        complete_reminder(&connection, &reminder.id).expect("completar");
        assert!(
            snapshot(&connection)
                .expect("snapshot final")
                .reminders
                .is_empty()
        );
    }

    #[test]
    fn snapshot_derives_forgotten_almost_finished_and_upcoming_from_saved_facts() {
        let connection = database();
        connection
            .execute(
                "UPDATE games SET last_played_at = datetime('now', '-500 days') WHERE app_id = 20",
                [],
            )
            .expect("antigüedad real");
        connection
            .execute(
                "UPDATE games SET release_date = date('now', '+1 year') WHERE app_id = 30",
                [],
            )
            .expect("lanzamiento futuro");

        let result = snapshot(&connection).expect("obtener descubrimiento");
        assert!(result.forgotten.iter().any(|game| game.app_id == 20));
        assert!(result.almost_finished.iter().any(|game| game.app_id == 10));
        assert!(result.upcoming.iter().any(|game| game.app_id == 30));
    }

    #[test]
    fn recommendation_uses_real_genre_data_and_persists_dismissal() {
        let mut connection = database();
        let recommendation = recommend(
            &mut connection,
            &RecommendationRequest {
                duration_minutes: Some(60),
                mood: Some("story".to_string()),
            },
        )
        .expect("recomendar")
        .expect("resultado");

        assert_eq!(recommendation.game.app_id, 10);
        assert!(
            recommendation
                .reasons
                .iter()
                .any(|reason| reason.contains("Adventure"))
        );
        dismiss_recommendation(&connection, &recommendation.history_id).expect("descartar");
        let history = snapshot(&connection)
            .expect("snapshot")
            .dismissed_recommendations;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].title, "Viajero");

        restore_recommendation(&connection, &recommendation.history_id).expect("restaurar");
        assert!(
            snapshot(&connection)
                .expect("snapshot restaurado")
                .dismissed_recommendations
                .is_empty()
        );
    }

    #[test]
    fn metadata_events_require_two_distinct_real_observations() {
        let mut connection = database();
        record_metadata_observation(
            &mut connection,
            10,
            Some(true),
            Some("2026-10-01"),
            "2026-08-10T10:00:00+00:00",
        )
        .expect("primera observación");
        let first_snapshot = snapshot(&connection).expect("primer snapshot");
        assert!(first_snapshot.events.is_empty());
        assert!(!first_snapshot.capabilities.early_access_history_available);

        record_metadata_observation(
            &mut connection,
            10,
            Some(false),
            Some("2026-11-01"),
            "2026-08-14T10:00:00+00:00",
        )
        .expect("segunda observación");
        let second_snapshot = snapshot(&connection).expect("segundo snapshot");
        assert!(second_snapshot.capabilities.early_access_history_available);
        let events = second_snapshot.events;
        assert_eq!(events.len(), 2);
        assert!(
            events
                .iter()
                .any(|event| event.kind == "early_access_changed")
        );
        assert!(
            events
                .iter()
                .any(|event| event.kind == "release_date_changed")
        );
    }

    #[test]
    fn unsupported_recommendation_filters_are_rejected() {
        let mut connection = database();
        let error = recommend(
            &mut connection,
            &RecommendationRequest {
                duration_minutes: Some(240),
                mood: None,
            },
        )
        .expect_err("rechazar duración inventada");
        assert_eq!(error.code, "validation");
    }

    #[test]
    fn official_news_cache_obeys_refresh_cadence_and_failure_backoff() {
        let mut connection = database();
        connection
            .execute(
                "UPDATE game_personal SET tracking = 1 WHERE app_id = 10",
                [],
            )
            .expect("activar seguimiento");

        let claimed = claim_news_refresh_candidates(&mut connection, 4).expect("reclamar lote");
        assert_eq!(
            claimed,
            vec![NewsRefreshCandidate {
                app_id: 10,
                attempts: 1
            }]
        );
        save_news_success(
            &mut connection,
            10,
            &[CachedNewsInput {
                gid: "1840944183772671".into(),
                title: "Gameplay patch".into(),
                content_preview: "Notas verificadas".into(),
                published_at: "2026-07-30T23:58:15+00:00".into(),
                feed_label: "Community Announcements".into(),
                feed_name: "steam_community_announcements".into(),
            }],
        )
        .expect("guardar publicación");

        assert!(
            claim_news_refresh_candidates(&mut connection, 4)
                .expect("respetar caché")
                .is_empty()
        );
        let result = snapshot(&connection).expect("leer caché");
        assert_eq!(result.official_publications.len(), 1);
        assert_eq!(result.official_publications[0].game_title, "Viajero");
        assert_eq!(result.official_publications[0].title, "Gameplay patch");
    }

    #[test]
    fn related_future_releases_require_exact_normalized_persisted_company_metadata() {
        let connection = database();
        connection
            .execute_batch(
                "UPDATE games
                    SET developer = '  Forge   One  ', release_date = '2024-01-01',
                        playtime_minutes = 120
                  WHERE app_id = 10;
                 UPDATE games
                    SET developer = 'forge one, Other Studio', publisher = 'Otro editor',
                        release_date = date('now', '+3 months')
                  WHERE app_id = 30;
                 UPDATE games
                    SET developer = 'Forge One', release_date = date('now', '+2 months'),
                        ownership_source = 'family_shared', family_availability = 'unknown'
                  WHERE app_id = 20;",
            )
            .expect("guardar metadatos reales");

        let unverified = snapshot(&connection).expect("ignorar metadatos no verificados");
        assert!(unverified.related_releases.is_empty());

        connection
            .execute(
                "UPDATE games
                    SET metadata_status = 'success',
                        metadata_fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  WHERE app_id IN (10, 20, 30)",
                [],
            )
            .expect("marcar metadatos verificados");

        let result = snapshot(&connection).expect("derivar relaciones");
        assert_eq!(result.related_releases.len(), 1);
        let relation = &result.related_releases[0];
        assert_eq!(relation.app_id, 30);
        assert_eq!(relation.related_to_app_id, 10);
        assert_eq!(relation.criterion, "developer");
        assert_eq!(relation.criterion_value, "forge one");
    }
}
