pub mod discovery;
pub mod family_catalog;
mod library;
pub mod library_dnd;
mod metadata_queue;
mod migrations;
mod organization;
pub mod personal;
pub mod recovery;

use crate::error::{AppError, AppResult};
use crate::models::{
    AppBootstrap, AppPreferences, BulkUpdateStatusInput, CollectionSummary, DatabaseDiagnostics,
    GameDetail, GameListRequest, LibraryFilterOptions, MetadataEnrichmentStatus,
    MovePlannerItemInput, PagedGameSessions, PagedGames, PlannerColumn, PlannerOverview,
    PlannerSettings, Recommendation, RecommendationRequest, SaveCollectionInput,
    SavePlannerItemInput, SmartRule, StatusDefinition, SteamConfiguration, UpdateGameInput,
};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension,
    backup::{Backup, StepResult},
    params,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use discovery::{
    CachedNewsInput, DiscoverySnapshot, GameReminder, NewsRefreshCandidate, NewsRefreshReport,
    SaveReminderInput,
};
pub use family_catalog::{
    FamilyCatalogGame, FamilyCatalogRequest, ImportedFamilyCatalogGame, PagedFamilyCatalogGames,
};
pub use library::{ImportedGame, ImportedInstallation, StoreMetadataUpdate};
pub use library_dnd::{LibraryDropInput, LibraryDropReceipt, LibraryDropResult};
pub(crate) use metadata_queue::MetadataJob;
pub use personal::{SavePersonalDatesInput, SaveSessionInput, SaveTagInput, TagDefinition};

#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
    maintenance_lock: Arc<Mutex<()>>,
    available: Arc<AtomicBool>,
}

impl Database {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            maintenance_lock: Arc::new(Mutex::new(())),
            available: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_available(&self) -> bool {
        self.available.load(Ordering::Acquire)
    }

    pub(crate) fn set_available(&self, available: bool) {
        self.available.store(available, Ordering::Release);
    }

    fn configure(connection: &Connection) -> AppResult<()> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA temp_store = MEMORY;",
        )?;
        Ok(())
    }

    pub fn open(&self) -> AppResult<Connection> {
        if !self.is_available() {
            return Err(AppError::new(
                "database_recovery_required",
                "La base local está aislada. Completa la recuperación segura antes de continuar.",
            ));
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(&self.path)?;
        Self::configure(&connection)?;
        Ok(connection)
    }

    pub fn initialize(&self) -> AppResult<()> {
        let mut connection = self.open()?;
        preflight_existing_database(&connection)?;
        migrations::migrate(&mut connection)?;
        validate_current_database(&connection, "database")?;
        seed_defaults(&mut connection)?;
        Ok(())
    }

    pub fn bootstrap(&self, steam: SteamConfiguration) -> AppResult<AppBootstrap> {
        let connection = self.open()?;
        let stats = library::library_stats(&connection)?;
        let statuses = organization::list_statuses(&connection)?;
        let collections = organization::list_collections(&connection)?;
        let planner = organization::list_planner(&connection)?;
        let preferences = organization::load_preferences(&connection)?;

        Ok(AppBootstrap {
            stats,
            statuses,
            collections,
            planner,
            steam,
            preferences,
            database_path: self.path.display().to_string(),
        })
    }

    pub fn export_backup(&self, destination: &Path) -> AppResult<()> {
        let _maintenance = self.maintenance_guard()?;
        self.export_backup_unlocked(destination)
    }

    fn export_backup_unlocked(&self, destination: &Path) -> AppResult<()> {
        validate_backup_path(&self.path, destination, BackupPathMode::Export)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = self.open()?;
        let mut target = Connection::open(destination)?;
        let backup = Backup::new(&source, &mut target)?;
        backup.run_to_completion(32, Duration::from_millis(10), None)?;
        drop(backup);
        validate_current_database(&target, "backup")?;
        Ok(())
    }

    pub fn import_backup(&self, source_path: &Path) -> AppResult<PathBuf> {
        let _maintenance = self.maintenance_guard()?;
        validate_backup_path(&self.path, source_path, BackupPathMode::Import)?;
        let source = open_read_only_database(source_path)?;
        validate_current_database(&source, "backup")?;

        let parent = self
            .path
            .parent()
            .ok_or_else(|| AppError::validation("La ruta de datos no tiene directorio padre."))?;
        let safety_path = parent.join(format!(
            "vindexa-before-restore-{}-{}.sqlite3",
            chrono::Utc::now().format("%Y%m%d-%H%M%S"),
            uuid::Uuid::new_v4()
        ));
        self.export_backup_unlocked(&safety_path)?;

        let mut destination = self.open()?;
        restore_with_rollback(&source, &mut destination, &safety_path, |restored| {
            Self::configure(restored)?;
            validate_current_database(restored, "backup")
        })?;
        Ok(safety_path)
    }

    fn maintenance_guard(&self) -> AppResult<std::sync::MutexGuard<'_, ()>> {
        self.maintenance_lock.lock().map_err(|_| {
            AppError::new(
                "database_maintenance",
                "La exclusión de mantenimiento quedó en un estado inválido.",
            )
        })
    }

    pub fn diagnostics(&self) -> AppResult<DatabaseDiagnostics> {
        let connection = self.open()?;
        let integrity: String =
            connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        let schema_version =
            connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        let journal_mode: String =
            connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
        let size_bytes = fs::metadata(&self.path)?.len();
        Ok(DatabaseDiagnostics {
            path: self.path.display().to_string(),
            size_bytes,
            schema_version,
            integrity,
            wal_enabled: journal_mode.eq_ignore_ascii_case("wal"),
        })
    }

    pub fn get_steam_account(&self) -> AppResult<Option<crate::models::SteamAccount>> {
        let connection = self.open()?;
        connection
            .query_row(
                "SELECT steam_id, persona_name, avatar_url, profile_url, visibility,
                        last_sync_at, last_sync_status, last_sync_error_code,
                        last_sync_error_message
                 FROM steam_accounts ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| {
                    Ok(crate::models::SteamAccount {
                        steam_id: row.get(0)?,
                        persona_name: row.get(1)?,
                        avatar_url: row.get(2)?,
                        profile_url: row.get(3)?,
                        visibility: row.get(4)?,
                        last_sync_at: row.get(5)?,
                        last_sync_status: row.get(6)?,
                        last_sync_error_code: row.get(7)?,
                        last_sync_error_message: row.get(8)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn steam_api_key_configured(&self) -> AppResult<Option<bool>> {
        let value = self
            .open()?
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'steam_api_key_configured'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match value.as_deref() {
            None => Ok(None),
            Some("true") => Ok(Some(true)),
            Some("false") => Ok(Some(false)),
            Some(_) => Err(AppError::new(
                "database_data",
                "El marcador de la clave de Steam guardado no es válido.",
            )),
        }
    }

    pub fn set_steam_api_key_configured(&self, configured: bool) -> AppResult<()> {
        self.open()?.execute(
            "INSERT INTO app_settings(key, value) VALUES ('steam_api_key_configured', ?1)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [configured.to_string()],
        )?;
        Ok(())
    }

    pub fn save_steam_identity(&self, steam_id: &str) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM steam_accounts WHERE steam_id <> ?1",
            [steam_id],
        )?;
        transaction.execute(
            "INSERT INTO steam_accounts(steam_id) VALUES (?1)
             ON CONFLICT(steam_id) DO UPDATE SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [steam_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn unlink_steam(&self) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM steam_accounts", [])?;
        transaction.execute("DELETE FROM family_catalog_games", [])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn use_openid_nonce(&self, nonce: &str) -> AppResult<()> {
        let connection = self.open()?;
        connection.execute(
            "DELETE FROM openid_nonces WHERE used_at < datetime('now', '-1 day')",
            [],
        )?;
        let inserted = connection.execute(
            "INSERT OR IGNORE INTO openid_nonces(nonce) VALUES (?1)",
            [nonce],
        )?;
        if inserted != 1 {
            return Err(AppError::new(
                "openid_replay",
                "Steam devolvió una respuesta de autenticación ya utilizada.",
            ));
        }
        Ok(())
    }

    pub fn list_games(&self, request: &GameListRequest) -> AppResult<PagedGames> {
        let connection = self.open()?;
        let collection_ids = request
            .collection_id
            .as_deref()
            .map(|id| organization::smart_collection_game_ids(&connection, id))
            .transpose()?
            .flatten();
        library::list_games(&connection, request, collection_ids.as_ref())
    }

    pub fn library_filter_options(&self) -> AppResult<LibraryFilterOptions> {
        library::filter_options(&self.open()?)
    }

    pub fn game_detail(&self, app_id: u32) -> AppResult<GameDetail> {
        library::get_game_detail(&self.open()?, app_id)
    }

    pub fn list_tags(&self) -> AppResult<Vec<TagDefinition>> {
        personal::list_tags(&self.open()?)
    }

    pub fn save_tag(&self, input: &SaveTagInput) -> AppResult<TagDefinition> {
        personal::save_tag(&mut self.open()?, input)
    }

    pub fn delete_tag(&self, id: &str) -> AppResult<()> {
        personal::delete_tag(&mut self.open()?, id)
    }

    pub fn set_game_tags(&self, app_id: u32, tag_ids: &[String]) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        personal::set_game_tags(&mut connection, app_id, tag_ids)?;
        personal::game_tag_ids(&connection, app_id)?;
        library::get_game_detail(&connection, app_id)
    }

    pub fn save_session(&self, input: &SaveSessionInput) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        personal::save_session(&mut connection, input)?;
        library::get_game_detail(&connection, input.app_id)
    }

    pub fn list_game_sessions(
        &self,
        app_id: u32,
        limit: u32,
        offset: u32,
    ) -> AppResult<PagedGameSessions> {
        personal::list_sessions(&self.open()?, app_id, limit, offset)
    }

    pub fn delete_session(&self, id: &str) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        let app_id = personal::delete_session(&mut connection, id)?;
        library::get_game_detail(&connection, app_id)
    }

    pub fn save_personal_dates(&self, input: &SavePersonalDatesInput) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        personal::save_personal_dates(&mut connection, input)?;
        library::get_game_detail(&connection, input.app_id)
    }

    pub fn store_metadata_refresh_due(&self, app_id: u32) -> AppResult<bool> {
        library::store_metadata_refresh_due(&self.open()?, app_id)
    }

    pub fn achievements_refresh_due(&self, app_id: u32) -> AppResult<bool> {
        library::achievements_refresh_due(&self.open()?, app_id)
    }

    pub fn save_store_metadata(
        &self,
        app_id: u32,
        metadata: &StoreMetadataUpdate,
    ) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        library::save_store_metadata(&connection, app_id, metadata)?;
        let (is_early_access, release_date, fetched_at) = connection.query_row(
            "SELECT is_early_access, release_date, metadata_fetched_at
               FROM games WHERE app_id = ?1",
            [app_id],
            |row| {
                Ok((
                    row.get::<_, Option<bool>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        discovery::record_metadata_observation(
            &mut connection,
            app_id,
            is_early_access,
            release_date.as_deref(),
            &fetched_at,
        )?;
        library::get_game_detail(&connection, app_id)
    }

    pub fn mark_store_metadata_attempt(&self, app_id: u32, status: &str) -> AppResult<GameDetail> {
        let connection = self.open()?;
        library::mark_store_metadata_attempt(&connection, app_id, status)?;
        library::get_game_detail(&connection, app_id)
    }

    pub fn enqueue_metadata_enrichment(
        &self,
        visible_app_ids: &[u32],
        include_backlog: bool,
    ) -> AppResult<usize> {
        metadata_queue::enqueue(&mut self.open()?, visible_app_ids, include_backlog)
    }

    pub(crate) fn claim_metadata_enrichment_jobs(
        &self,
        limit: usize,
    ) -> AppResult<Vec<MetadataJob>> {
        metadata_queue::claim_ready(&mut self.open()?, limit)
    }

    pub(crate) fn complete_metadata_enrichment(
        &self,
        app_id: u32,
        metadata: &StoreMetadataUpdate,
    ) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        library::save_store_metadata(&transaction, app_id, metadata)?;
        metadata_queue::mark_success(&transaction, app_id)?;
        transaction.commit()?;

        let (is_early_access, release_date, fetched_at) = connection.query_row(
            "SELECT is_early_access, release_date, metadata_fetched_at
               FROM games WHERE app_id = ?1",
            [app_id],
            |row| {
                Ok((
                    row.get::<_, Option<bool>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )?;
        discovery::record_metadata_observation(
            &mut connection,
            app_id,
            is_early_access,
            release_date.as_deref(),
            &fetched_at,
        )
    }

    pub(crate) fn complete_metadata_unavailable(&self, app_id: u32) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        library::mark_store_metadata_attempt(&transaction, app_id, "unavailable")?;
        metadata_queue::mark_unavailable(&transaction, app_id)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn retry_metadata_enrichment(
        &self,
        app_id: u32,
        error_code: &str,
        delay_seconds: u64,
    ) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        library::mark_store_metadata_attempt(&transaction, app_id, "failed")?;
        metadata_queue::schedule_retry(&transaction, app_id, error_code, delay_seconds)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn fail_metadata_enrichment(&self, app_id: u32, error_code: &str) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        library::mark_store_metadata_attempt(&transaction, app_id, "failed")?;
        metadata_queue::mark_failed(&transaction, app_id, error_code)?;
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn next_metadata_enrichment_delay_ms(&self) -> AppResult<Option<u64>> {
        metadata_queue::next_ready_delay_ms(&self.open()?)
    }

    pub fn metadata_enrichment_status(&self) -> AppResult<MetadataEnrichmentStatus> {
        metadata_queue::status(&self.open()?)
    }

    pub fn save_achievements(
        &self,
        app_id: u32,
        unlocked: u32,
        total: u32,
    ) -> AppResult<GameDetail> {
        let connection = self.open()?;
        library::save_achievements(&connection, app_id, unlocked, total)?;
        library::get_game_detail(&connection, app_id)
    }

    pub fn mark_achievements_attempt(&self, app_id: u32, status: &str) -> AppResult<GameDetail> {
        let connection = self.open()?;
        library::mark_achievements_attempt(&connection, app_id, status)?;
        library::get_game_detail(&connection, app_id)
    }

    pub fn update_game(&self, input: &UpdateGameInput) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        library::update_game(&mut connection, input)?;
        library::get_game_detail(&connection, input.app_id)
    }

    pub fn bulk_update_status(&self, input: &BulkUpdateStatusInput) -> AppResult<usize> {
        library::bulk_update_status(&mut self.open()?, input)
    }

    pub fn apply_library_drop(&self, input: &LibraryDropInput) -> AppResult<LibraryDropResult> {
        library_dnd::apply_drop(&mut self.open()?, input)
    }

    pub fn undo_library_drop(&self, receipt: &LibraryDropReceipt) -> AppResult<usize> {
        library_dnd::undo_drop(&mut self.open()?, receipt)
    }

    pub fn upsert_imported_games(
        &self,
        games: &[ImportedGame],
        reset_installed: bool,
    ) -> AppResult<(usize, usize)> {
        library::upsert_imported_games(&mut self.open()?, games, reset_installed)
    }

    pub fn save_family_catalog(
        &self,
        games: &[ImportedFamilyCatalogGame],
        complete_snapshot: bool,
    ) -> AppResult<()> {
        family_catalog::save(&mut self.open()?, games, complete_snapshot)
    }

    pub fn list_family_catalog(
        &self,
        request: &FamilyCatalogRequest,
    ) -> AppResult<PagedFamilyCatalogGames> {
        family_catalog::list(&self.open()?, request)
    }

    pub fn family_catalog_game(&self, app_id: u32) -> AppResult<FamilyCatalogGame> {
        family_catalog::get(&self.open()?, app_id)
    }

    pub fn recommend(&self, request: &RecommendationRequest) -> AppResult<Option<Recommendation>> {
        discovery::recommend(&mut self.open()?, request)
    }

    pub fn discovery_snapshot(&self) -> AppResult<DiscoverySnapshot> {
        discovery::snapshot(&self.open()?)
    }

    pub fn claim_news_refresh_candidates(
        &self,
        limit: usize,
    ) -> AppResult<Vec<NewsRefreshCandidate>> {
        discovery::claim_news_refresh_candidates(&mut self.open()?, limit)
    }

    pub fn save_news_success(&self, app_id: u32, items: &[CachedNewsInput]) -> AppResult<()> {
        discovery::save_news_success(&mut self.open()?, app_id, items)
    }

    pub fn save_news_failure(
        &self,
        app_id: u32,
        attempts: u32,
        error_code: &str,
        retry_delay_seconds: u64,
    ) -> AppResult<()> {
        discovery::save_news_failure(
            &self.open()?,
            app_id,
            attempts,
            error_code,
            retry_delay_seconds,
        )
    }

    pub fn save_reminder(&self, input: &SaveReminderInput) -> AppResult<GameReminder> {
        discovery::save_reminder(&self.open()?, input)
    }

    pub fn complete_reminder(&self, id: &str) -> AppResult<()> {
        discovery::complete_reminder(&self.open()?, id)
    }

    pub fn snooze_reminder(&self, id: &str, due_at: &str) -> AppResult<GameReminder> {
        discovery::snooze_reminder(&self.open()?, id, due_at)
    }

    pub fn dismiss_recommendation(&self, history_id: &str) -> AppResult<()> {
        discovery::dismiss_recommendation(&self.open()?, history_id)
    }

    pub fn restore_recommendation(&self, history_id: &str) -> AppResult<()> {
        discovery::restore_recommendation(&self.open()?, history_id)
    }

    pub fn save_collection(&self, input: &SaveCollectionInput) -> AppResult<CollectionSummary> {
        organization::save_collection(&mut self.open()?, input)
    }

    pub fn preview_smart_collection(&self, input: &SaveCollectionInput) -> AppResult<PagedGames> {
        let connection = self.open()?;
        let app_ids = organization::preview_smart_collection(&connection, input)?;
        let request = GameListRequest {
            limit: Some(8),
            sort: Some("manual".to_string()),
            ..GameListRequest::default()
        };
        library::list_games(&connection, &request, Some(&app_ids))
    }

    pub fn delete_collection(&self, id: &str) -> AppResult<()> {
        organization::delete_collection(&mut self.open()?, id)
    }

    pub fn reorder_collections(&self, ids: &[String]) -> AppResult<()> {
        organization::reorder_collections(&mut self.open()?, ids)
    }

    pub fn set_collection_games(&self, collection_id: &str, app_ids: &[u32]) -> AppResult<()> {
        organization::set_collection_games(&mut self.open()?, collection_id, app_ids)
    }

    pub fn smart_rules(&self, collection_id: &str) -> AppResult<Vec<SmartRule>> {
        organization::list_smart_rules(&self.open()?, collection_id)
    }

    pub fn set_game_collections(&self, app_id: u32, collection_ids: &[String]) -> AppResult<()> {
        organization::set_game_collections(&mut self.open()?, app_id, collection_ids)
    }

    pub fn move_planner_item(&self, input: &MovePlannerItemInput) -> AppResult<()> {
        organization::move_planner_item(&mut self.open()?, input)
    }

    pub fn planner_overview(&self) -> AppResult<PlannerOverview> {
        organization::planner_overview(&self.open()?)
    }

    pub fn move_planner_queue_item(&self, app_id: u32, position: i64) -> AppResult<()> {
        organization::move_planner_queue_item(&mut self.open()?, app_id, position)
    }

    pub fn save_planner_item(&self, input: &SavePlannerItemInput) -> AppResult<()> {
        organization::save_planner_item(&mut self.open()?, input)
    }

    pub fn save_planner_settings(&self, settings: &PlannerSettings) -> AppResult<PlannerSettings> {
        organization::save_planner_settings(&mut self.open()?, settings)
    }

    pub fn remove_planner_item(&self, app_id: u32) -> AppResult<()> {
        organization::remove_planner_item(&mut self.open()?, app_id)
    }

    pub fn save_preferences(&self, preferences: &AppPreferences) -> AppResult<()> {
        organization::save_preferences(&mut self.open()?, preferences)
    }

    pub fn save_status(
        &self,
        id: Option<&str>,
        name: &str,
        color: &str,
    ) -> AppResult<StatusDefinition> {
        organization::save_status(&mut self.open()?, id, name, color)
    }

    pub fn delete_status(&self, id: &str, replacement_id: &str) -> AppResult<()> {
        organization::delete_status(&mut self.open()?, id, replacement_id)
    }

    pub fn reorder_statuses(&self, ids: &[String]) -> AppResult<()> {
        organization::reorder_statuses(&mut self.open()?, ids)
    }

    pub fn save_planner_column(
        &self,
        id: Option<&str>,
        name: &str,
        color: &str,
        wip_limit: Option<u32>,
    ) -> AppResult<PlannerColumn> {
        organization::save_planner_column(&mut self.open()?, id, name, color, wip_limit)
    }

    pub fn delete_planner_column(&self, id: &str, replacement_id: Option<&str>) -> AppResult<()> {
        organization::delete_planner_column(&mut self.open()?, id, replacement_id)
    }

    pub fn reorder_planner_columns(&self, ids: &[String]) -> AppResult<()> {
        organization::reorder_planner_columns(&mut self.open()?, ids)
    }
}

#[derive(Clone, Copy)]
enum BackupPathMode {
    Export,
    Import,
}

fn validate_backup_path(active: &Path, selected: &Path, mode: BackupPathMode) -> AppResult<()> {
    let active = fs::canonicalize(active).map_err(|_| {
        AppError::new(
            "database_path",
            "No se pudo resolver la ubicación de la base activa.",
        )
    })?;
    let metadata = fs::symlink_metadata(selected).ok();
    if metadata
        .as_ref()
        .is_some_and(|value| value.file_type().is_symlink())
    {
        return Err(AppError::validation(
            "No se admiten enlaces simbólicos para copias de seguridad.",
        ));
    }
    if matches!(mode, BackupPathMode::Import)
        && metadata.as_ref().is_none_or(|value| !value.is_file())
    {
        return Err(AppError::validation(
            "La copia seleccionada no es un archivo regular.",
        ));
    }

    let resolved = if metadata.is_some() {
        fs::canonicalize(selected).map_err(|_| {
            AppError::validation("No se pudo resolver la ruta de la copia seleccionada.")
        })?
    } else {
        let parent = selected.parent().ok_or_else(|| {
            AppError::validation("La copia debe guardarse dentro de un directorio válido.")
        })?;
        let file_name = selected.file_name().ok_or_else(|| {
            AppError::validation("La copia necesita un nombre de archivo válido.")
        })?;
        fs::canonicalize(parent)
            .map_err(|_| AppError::validation("El directorio de destino no existe."))?
            .join(file_name)
    };

    if resolved == active || same_file_identity(&active, &resolved) {
        return Err(AppError::validation(
            "La base activa no puede utilizarse como archivo de copia.",
        ));
    }
    if is_active_database_sidecar(&active, &resolved) {
        return Err(AppError::validation(
            "Los archivos internos WAL, SHM y journal de la base activa no son destinos válidos.",
        ));
    }
    Ok(())
}

fn is_active_database_sidecar(active: &Path, selected: &Path) -> bool {
    if active.parent() != selected.parent() {
        return false;
    }
    let Some(active_name) = active.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(selected_name) = selected.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    ["-wal", "-shm", "-journal"]
        .iter()
        .any(|suffix| selected_name == format!("{active_name}{suffix}"))
}

#[cfg(unix)]
fn same_file_identity(active: &Path, selected: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let (Ok(active), Ok(selected)) = (fs::metadata(active), fs::metadata(selected)) else {
        return false;
    };
    active.dev() == selected.dev() && active.ino() == selected.ino()
}

#[cfg(not(unix))]
fn same_file_identity(_active: &Path, _selected: &Path) -> bool {
    false
}

fn open_read_only_database(path: &Path) -> AppResult<Connection> {
    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA query_only = ON;")?;
    Ok(connection)
}

fn restore_with_rollback<F>(
    source: &Connection,
    destination: &mut Connection,
    safety_path: &Path,
    validate_restored: F,
) -> AppResult<()>
where
    F: FnOnce(&Connection) -> AppResult<()>,
{
    let restore_result = copy_all_pages_atomically(source, destination)
        .and_then(|()| validate_restored(destination));
    if let Err(restore_error) = restore_result {
        let rollback_result = (|| -> AppResult<()> {
            let safety = open_read_only_database(safety_path)?;
            validate_current_database(&safety, "rollback")?;
            copy_all_pages_atomically(&safety, destination)?;
            Database::configure(destination)?;
            validate_current_database(destination, "rollback")
        })();
        if let Err(rollback_error) = rollback_result {
            return Err(AppError::new(
                "restore_rollback_failed",
                format!(
                    "La restauración falló ({restore_error}) y tampoco se pudo recuperar automáticamente la base anterior ({rollback_error}). La copia de seguridad permanece disponible."
                ),
            ));
        }
        return Err(AppError::new(
            restore_error.code,
            format!(
                "{} La base anterior se recuperó y verificó automáticamente.",
                restore_error.message
            ),
        ));
    }
    Ok(())
}

fn copy_all_pages_atomically(source: &Connection, destination: &mut Connection) -> AppResult<()> {
    // Una única llamada con -1 mantiene la sustitución dentro de una sola
    // transacción SQLite; ante caída de proceso, el journal conserva la base previa.
    let backup = Backup::new(source, destination)?;
    for _ in 0..100 {
        match backup.step(-1)? {
            StepResult::Done => return Ok(()),
            StepResult::More => continue,
            StepResult::Busy | StepResult::Locked => {
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => {
                return Err(AppError::new(
                    "restore_failed",
                    "SQLite devolvió un estado desconocido durante la restauración.",
                ));
            }
        }
    }
    Err(AppError::new(
        "restore_busy",
        "La base de datos permaneció ocupada y no pudo restaurarse de forma segura.",
    ))
}

fn validate_current_database(connection: &Connection, context: &str) -> AppResult<()> {
    validate_integrity(connection, context)?;
    migrations::validate_history(connection)?;
    validate_required_schema(connection, context)?;
    validate_schema_fingerprint(connection, context)?;

    let mut statement = connection.prepare("PRAGMA foreign_key_check")?;
    let mut rows = statement.query([])?;
    if rows.next()?.is_some() {
        return Err(AppError::new(
            format!("{context}_foreign_keys"),
            "La copia contiene relaciones rotas entre sus datos.",
        ));
    }
    Ok(())
}

fn preflight_existing_database(connection: &Connection) -> AppResult<()> {
    let user_table_count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        [],
        |row| row.get(0),
    )?;
    if user_table_count == 0 {
        return Ok(());
    }
    validate_integrity(connection, "database")?;
    let application_id: i64 =
        connection.pragma_query_value(None, "application_id", |row| row.get(0))?;
    let user_version: i64 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if application_id == migrations::APPLICATION_ID && user_version > migrations::CURRENT_VERSION {
        return Err(AppError::new(
            "database_version",
            format!(
                "La base usa el esquema {user_version}, más reciente que el esquema {} compatible con esta versión de Vindexa.",
                migrations::CURRENT_VERSION
            ),
        ));
    }
    Ok(())
}

fn validate_schema_fingerprint(connection: &Connection, context: &str) -> AppResult<()> {
    let mut canonical = Connection::open_in_memory()?;
    migrations::migrate(&mut canonical)?;
    if schema_definitions(connection)? != schema_definitions(&canonical)? {
        return Err(AppError::new(
            format!("{context}_schema"),
            "La definición interna del esquema no coincide exactamente con esta versión de Vindexa.",
        ));
    }
    Ok(())
}

fn schema_definitions(connection: &Connection) -> AppResult<Vec<(String, String, String)>> {
    let mut statement = connection.prepare(
        "SELECT type, name, sql FROM sqlite_master
          WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL
            AND name NOT IN (
                'game_search_data', 'game_search_idx', 'game_search_content',
                'game_search_docsize', 'game_search_config'
            )
          ORDER BY type ASC, name ASC",
    )?;
    Ok(statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?)
}

fn validate_integrity(connection: &Connection, context: &str) -> AppResult<()> {
    let mut statement = connection.prepare("PRAGMA integrity_check")?;
    let results = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if results.len() != 1 || results[0] != "ok" {
        return Err(AppError::new(
            format!("{context}_integrity"),
            "SQLite ha detectado daños en el archivo seleccionado.",
        ));
    }
    Ok(())
}

fn validate_required_schema(connection: &Connection, context: &str) -> AppResult<()> {
    const REQUIRED_TABLES: &[(&str, &[&str])] = &[
        ("schema_migrations", &["version", "name", "applied_at"]),
        ("app_settings", &["key", "value", "updated_at"]),
        (
            "steam_accounts",
            &[
                "steam_id",
                "last_sync_status",
                "last_sync_error_code",
                "last_sync_error_message",
                "updated_at",
            ],
        ),
        ("statuses", &["id", "name", "color", "position", "built_in"]),
        ("games", &["app_id", "title", "updated_at"]),
        (
            "game_personal",
            &[
                "app_id",
                "status_id",
                "progress",
                "notes",
                "manual_position",
            ],
        ),
        (
            "game_installations",
            &["app_id", "library_path", "install_path", "is_primary"],
        ),
        (
            "collections",
            &["id", "name", "kind", "match_mode", "position"],
        ),
        ("collection_games", &["collection_id", "app_id", "position"]),
        (
            "smart_rules",
            &[
                "id",
                "collection_id",
                "group_id",
                "field",
                "operator",
                "value_json",
            ],
        ),
        ("tags", &["id", "name", "color"]),
        ("game_tags", &["app_id", "tag_id"]),
        ("planner_columns", &["id", "name", "position", "wip_limit"]),
        ("planner_items", &["column_id", "app_id", "position"]),
        ("game_sessions", &["id", "app_id", "started_at"]),
        ("activity", &["id", "kind", "message", "created_at"]),
        ("sync_runs", &["id", "source", "status", "started_at"]),
        ("recommendation_history", &["id", "app_id", "created_at"]),
        ("openid_nonces", &["nonce", "used_at"]),
        ("image_cache", &["app_id", "variant", "local_path"]),
        (
            "steam_news_items",
            &[
                "app_id",
                "gid",
                "title",
                "content_preview",
                "published_at",
                "feed_label",
                "feed_name",
                "fetched_at",
            ],
        ),
        (
            "steam_news_fetch_state",
            &[
                "app_id",
                "consecutive_failures",
                "last_attempt_at",
                "last_success_at",
                "next_attempt_at",
                "last_error_code",
                "updated_at",
            ],
        ),
        (
            "game_search",
            &["title", "notes", "checkpoint", "next_action"],
        ),
    ];
    for (table, required_columns) in REQUIRED_TABLES {
        let exists = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::new(
                format!("{context}_schema"),
                format!("La copia no contiene la tabla obligatoria «{table}»."),
            ));
        }
        let mut statement = connection.prepare("SELECT name FROM pragma_table_info(?1)")?;
        let columns = statement
            .query_map([table], |row| row.get::<_, String>(0))?
            .collect::<Result<std::collections::HashSet<_>, _>>()?;
        if required_columns
            .iter()
            .any(|column| !columns.contains(*column))
        {
            return Err(AppError::new(
                format!("{context}_schema"),
                format!("La estructura de la tabla «{table}» no es compatible."),
            ));
        }
    }

    for trigger in [
        "games_search_insert",
        "games_search_title_update",
        "personal_search_update",
    ] {
        let exists = connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'trigger' AND name = ?1",
                [trigger],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(AppError::new(
                format!("{context}_schema"),
                "La copia no contiene todos los mecanismos de búsqueda obligatorios.",
            ));
        }
    }
    Ok(())
}

fn seed_defaults(connection: &mut Connection) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let statuses = [
        ("unclassified", "Sin clasificar", "#6F7B8A"),
        ("playing", "Jugando ahora", "#5CAAC1"),
        ("next", "Jugar después", "#4B8FB5"),
        ("backlog", "Backlog", "#7C8798"),
        ("paused", "Pausado", "#D6A64B"),
        ("completed", "Completado", "#7EA64B"),
        ("abandoned", "Abandonado", "#D85C5C"),
        ("recurring", "Infinito o recurrente", "#8E72B2"),
        ("multiplayer", "Solo multijugador", "#3F9A8B"),
        ("waiting_update", "Esperando actualización", "#B27D55"),
        (
            "waiting_early_access",
            "Esperando salir de Early Access",
            "#9A768D",
        ),
    ];
    for (position, (id, name, color)) in statuses.iter().enumerate() {
        transaction.execute(
            "INSERT OR IGNORE INTO statuses(id, name, color, position, built_in)
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![id, name, color, position as i64],
        )?;
    }

    let planner_columns = [
        ("playing", "Jugando ahora", "#5CAAC1", Some(3_u32)),
        ("next", "A continuación", "#4B8FB5", Some(5)),
        ("month", "Este mes", "#647C9D", None),
        ("later", "Más adelante", "#6F7B8A", None),
        ("paused", "Pausados", "#D6A64B", None),
        ("done", "Terminados", "#7EA64B", None),
    ];
    for (position, (id, name, color, limit)) in planner_columns.iter().enumerate() {
        transaction.execute(
            "INSERT OR IGNORE INTO planner_columns(id, name, color, position, wip_limit)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, name, color, position as i64, limit],
        )?;
    }

    let settings = [
        ("density", "compact"),
        ("periodic_sync_minutes", "0"),
        ("confirm_uninstall", "true"),
    ];
    for (key, value) in settings {
        transaction.execute(
            "INSERT OR IGNORE INTO app_settings(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod durability_tests {
    use super::*;
    use tempfile::TempDir;

    fn database_at(directory: &TempDir, name: &str) -> Database {
        let database = Database::new(directory.path().join(name));
        database.initialize().expect("inicializar base de prueba");
        database
    }

    fn insert_game(database: &Database, app_id: u32, title: &str) {
        let connection = database.open().expect("abrir base de prueba");
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
            .expect("insertar organización personal");
    }

    fn has_game(database: &Database, app_id: u32) -> bool {
        database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT 1 FROM games WHERE app_id = ?1",
                [app_id],
                |_| Ok(()),
            )
            .optional()
            .expect("consultar juego")
            .is_some()
    }

    fn status_of(database: &Database, app_id: u32) -> String {
        database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT status_id FROM game_personal WHERE app_id = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("consultar estado")
    }

    #[test]
    fn bulk_status_updates_every_game_in_one_successful_operation() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "bulk-success.sqlite3");
        insert_game(&database, 10, "Primero");
        insert_game(&database, 20, "Segundo");

        let changed = database
            .bulk_update_status(&BulkUpdateStatusInput {
                app_ids: vec![10, 20],
                status_id: "playing".to_string(),
            })
            .expect("actualizar selección");

        assert_eq!(changed, 2);
        assert_eq!(status_of(&database, 10), "playing");
        assert_eq!(status_of(&database, 20), "playing");
        let activity_count: i64 = database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT COUNT(*) FROM activity WHERE kind = 'personal_update'",
                [],
                |row| row.get(0),
            )
            .expect("contar actividad");
        assert_eq!(activity_count, 2);
    }

    #[test]
    fn bulk_status_rolls_back_the_whole_selection_when_a_game_is_missing() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "bulk-atomic.sqlite3");
        insert_game(&database, 10, "Permanece intacto");

        let error = database
            .bulk_update_status(&BulkUpdateStatusInput {
                app_ids: vec![10, 999],
                status_id: "playing".to_string(),
            })
            .expect_err("rechazar selección incompleta");

        assert_eq!(error.code, "not_found");
        assert_eq!(status_of(&database, 10), "unclassified");
        let activity_count: i64 = database
            .open()
            .expect("abrir base")
            .query_row("SELECT COUNT(*) FROM activity", [], |row| row.get(0))
            .expect("contar actividad");
        assert_eq!(activity_count, 0);
    }

    #[test]
    fn configured_connections_use_full_synchronous_durability() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "durability.sqlite3");
        let connection = database.open().expect("abrir base");
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .expect("leer synchronous");
        let application_id: i64 = connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .expect("leer application_id");
        assert_eq!(synchronous, 2, "SQLite FULL debe permanecer activo");
        assert_eq!(application_id, migrations::APPLICATION_ID);
    }

    #[test]
    fn import_rejects_wrong_identity_without_touching_active_data() {
        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "active.sqlite3");
        insert_game(&active, 10, "Biblioteca activa");
        let foreign = database_at(&directory, "foreign.sqlite3");
        insert_game(&foreign, 20, "No debe entrar");
        foreign
            .open()
            .expect("abrir copia ajena")
            .pragma_update(None, "application_id", 0x1234_i64)
            .expect("cambiar identidad");

        let error = active
            .import_backup(&foreign.path)
            .expect_err("rechazar identidad incorrecta");
        assert_eq!(error.code, "backup_identity");
        assert!(has_game(&active, 10));
        assert!(!has_game(&active, 20));
    }

    #[test]
    fn import_rejects_incomplete_schema_without_touching_active_data() {
        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "active.sqlite3");
        insert_game(&active, 10, "Biblioteca activa");
        let malformed = database_at(&directory, "malformed.sqlite3");
        insert_game(&malformed, 20, "No debe entrar");
        malformed
            .open()
            .expect("abrir copia incompleta")
            .execute_batch(
                "DROP TRIGGER personal_search_update;
                 CREATE TRIGGER personal_search_update
                 AFTER UPDATE OF notes ON game_personal BEGIN SELECT 1; END;",
            )
            .expect("alterar definición interna de prueba");

        let error = active
            .import_backup(&malformed.path)
            .expect_err("rechazar esquema incompleto");
        assert_eq!(error.code, "backup_schema");
        assert!(has_game(&active, 10));
        assert!(!has_game(&active, 20));
    }

    #[test]
    fn post_restore_failure_automatically_rolls_back_verified_data() {
        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "active.sqlite3");
        insert_game(&active, 10, "Biblioteca activa");
        let incoming = database_at(&directory, "incoming.sqlite3");
        insert_game(&incoming, 20, "Restauración entrante");
        let safety_path = directory.path().join("safety.sqlite3");
        active
            .export_backup(&safety_path)
            .expect("crear copia de seguridad");

        let source = open_read_only_database(&incoming.path).expect("abrir origen");
        let mut destination = active.open().expect("abrir destino");
        let error = restore_with_rollback(&source, &mut destination, &safety_path, |_| {
            Err(AppError::new(
                "simulated_failure",
                "Fallo posterior simulado.",
            ))
        })
        .expect_err("forzar rollback");

        assert_eq!(error.code, "simulated_failure");
        assert!(error.message.contains("recuperó y verificó"));
        assert!(
            destination
                .query_row("SELECT 1 FROM games WHERE app_id = 10", [], |_| Ok(()))
                .optional()
                .expect("consultar original")
                .is_some()
        );
        assert!(
            destination
                .query_row("SELECT 1 FROM games WHERE app_id = 20", [], |_| Ok(()))
                .optional()
                .expect("consultar entrante")
                .is_none()
        );
        validate_current_database(&destination, "test").expect("validar rollback final");
    }

    #[test]
    fn valid_restore_replaces_data_and_keeps_a_verified_safety_copy() {
        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "active.sqlite3");
        insert_game(&active, 10, "Biblioteca activa");
        let incoming = database_at(&directory, "incoming.sqlite3");
        insert_game(&incoming, 20, "Biblioteca restaurada");

        let safety_path = active
            .import_backup(&incoming.path)
            .expect("restaurar copia válida");
        assert!(!has_game(&active, 10));
        assert!(has_game(&active, 20));
        assert!(safety_path.exists());
        let safety = open_read_only_database(&safety_path).expect("abrir seguridad");
        validate_current_database(&safety, "test").expect("validar seguridad");
        assert!(
            safety
                .query_row("SELECT 1 FROM games WHERE app_id = 10", [], |_| Ok(()))
                .optional()
                .expect("consultar seguridad")
                .is_some()
        );
    }

    #[test]
    fn backup_boundary_rejects_active_database_sidecars() {
        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "active.sqlite3");
        let wal_path = directory.path().join("active.sqlite3-wal");

        let error = active
            .export_backup(&wal_path)
            .expect_err("rechazar sidecar activo");
        assert_eq!(error.code, "validation");
        assert!(!wal_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn backup_boundary_rejects_symlink_and_hardlink_aliases() {
        use std::os::unix::fs::symlink;

        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "active.sqlite3");
        insert_game(&active, 10, "Biblioteca activa");

        let symlink_path = directory.path().join("alias-simbolico.sqlite3");
        symlink(&active.path, &symlink_path).expect("crear enlace simbólico");
        let symlink_error = active
            .import_backup(&symlink_path)
            .expect_err("rechazar enlace simbólico");
        assert_eq!(symlink_error.code, "validation");

        let hardlink_path = directory.path().join("alias-fisico.sqlite3");
        fs::hard_link(&active.path, &hardlink_path).expect("crear enlace físico");
        let hardlink_error = active
            .export_backup(&hardlink_path)
            .expect_err("rechazar enlace físico");
        assert_eq!(hardlink_error.code, "validation");
        assert!(has_game(&active, 10));
    }
}
