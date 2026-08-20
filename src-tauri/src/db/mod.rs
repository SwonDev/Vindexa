pub mod archive;
mod catalog;
pub mod curated;
pub mod deals;
pub mod discovery;
pub mod dlc;
pub mod epic_free;
pub mod family_catalog;
mod library;
pub mod library_dnd;
mod metadata_queue;
pub(crate) mod migrations;
pub mod notifications;
pub mod organization;
pub mod personal;
pub mod preview;
pub mod pricing;
pub mod priority;
pub mod recovery;
pub mod rich_metadata;
pub mod saved_views;
pub mod wishlist;

use crate::error::{AppError, AppResult};
use crate::models::{
    AppBootstrap, AppPreferences, BulkUpdateStatusInput, CollectionSummary, DatabaseDiagnostics,
    GameDetail, GameListRequest, LibraryFilterOptions, MetadataEnrichmentStatus,
    MovePlannerItemInput, PagedGameSessions, PagedGames, PlannerColumn, PlannerOverview,
    PlannerSettings, Recommendation, RecommendationRequest, SaveCollectionInput,
    SavePlannerItemInput, SmartRule, StatusDefinition, SteamConfiguration, SyncRun,
    UpdateGameInput,
};
use chrono::{DateTime, Utc};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension,
    backup::{Backup, StepResult},
    params,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use archive::{ArchiveReport, PagedArchivedGames};
pub use curated::{
    AddCuratedGameInput, CuratedList, CuratedListDetail, SaveCuratedListInput,
    UpdateCuratedItemInput,
};
pub use discovery::{
    CachedNewsInput, DiscoverySnapshot, GameReminder, NewsRefreshCandidate, NewsRefreshReport,
    SaveReminderInput,
};
pub use dlc::{
    DlcFilter, DlcImportSummary, DlcRefreshCandidate, DlcRefreshReport, DlcSummary, GameDlc,
    ImportedDlc,
};
pub use family_catalog::{
    FamilyCatalogGame, FamilyCatalogRequest, ImportedFamilyCatalogGame, PagedFamilyCatalogGames,
};
pub use library::{ArtworkTarget, ImportedGame, ImportedInstallation, StoreMetadataUpdate};
pub use library_dnd::{LibraryDropInput, LibraryDropReceipt, LibraryDropResult};
pub(crate) use metadata_queue::MetadataJob;
pub use notifications::{
    NotificationInbox, NotificationInboxFilter, NotificationRefreshReport, NotificationRule,
    SaveNotificationRuleInput,
};
pub use personal::{SavePersonalDatesInput, SaveSessionInput, SaveTagInput, TagDefinition};
pub use pricing::{
    GamePrice, PriceHistory, PriceObservation, PriceRefreshReport, RecordedPrice,
    WishlistPriceStatus,
};
pub use priority::{
    PriorityExplanation, PriorityRanking, PriorityRecomputeReport, TasteReport, UpcomingRelease,
};
pub use rich_metadata::{DrmStateCounts, RichGameMetadata, RichMetadataUpdate};
pub use saved_views::{SaveViewInput, SavedView};
pub use wishlist::{
    GameVideo, GameVideoRef, ImportedWishlistGame, SaveGameVideoInput, SaveWishlistEntryInput,
    SteamWishlistImportResult, WishlistEntry, WishlistImportReport, WishlistOverview,
};

#[derive(Debug, Clone)]
pub(crate) struct SteamProfileWrite {
    pub(crate) persona_name: String,
    pub(crate) avatar_url: Option<String>,
    pub(crate) profile_url: Option<String>,
    pub(crate) visibility: Option<u8>,
}

#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
    maintenance_lock: Arc<Mutex<()>>,
    available: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
}

impl Database {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            maintenance_lock: Arc::new(Mutex::new(())),
            available: Arc::new(AtomicBool::new(true)),
            generation: Arc::new(AtomicU64::new(0)),
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

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn advance_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
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

    pub fn bootstrap(
        &self,
        steam: SteamConfiguration,
        app_version: String,
    ) -> AppResult<AppBootstrap> {
        let connection = self.open()?;
        let stats = library::library_stats(&connection)?;
        let statuses = organization::list_statuses(&connection)?;
        let collections = organization::list_collections(&connection)?;
        let planner = organization::list_planner(&connection)?;
        let preferences = organization::load_preferences(&connection)?;

        Ok(AppBootstrap {
            app_version,
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
        self.advance_generation();
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

    #[allow(clippy::too_many_arguments)]
    pub fn record_sync_run(
        &self,
        source: &str,
        status: &str,
        started_at: &str,
        finished_at: &str,
        imported_count: usize,
        updated_count: usize,
        error_message: Option<&str>,
    ) -> AppResult<()> {
        self.open()?.execute(
            "INSERT INTO sync_runs(id, source, status, started_at, finished_at,
                                  imported_count, updated_count, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                uuid::Uuid::new_v4().to_string(),
                source,
                status,
                started_at,
                finished_at,
                imported_count as i64,
                updated_count as i64,
                error_message,
            ],
        )?;
        Ok(())
    }

    pub fn list_sync_runs(&self, limit: u32) -> AppResult<Vec<SyncRun>> {
        let connection = self.open()?;
        let mut statement = connection.prepare(
            "SELECT id, source, status, started_at, finished_at,
                    imported_count, updated_count, error_message
             FROM sync_runs ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map([limit], |row| {
            Ok(SyncRun {
                id: row.get(0)?,
                source: row.get(1)?,
                status: row.get(2)?,
                started_at: row.get(3)?,
                finished_at: row.get(4)?,
                imported_count: row.get(5)?,
                updated_count: row.get(6)?,
                error_message: row.get(7)?,
            })
        })?;
        let mut runs = Vec::new();
        for row in rows {
            runs.push(row?);
        }
        Ok(runs)
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
        Self::detail_with_rich(&self.open()?, app_id)
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
        Self::detail_with_rich(&connection, app_id)
    }

    pub fn save_session(&self, input: &SaveSessionInput) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        personal::save_session(&mut connection, input)?;
        Self::detail_with_rich(&connection, input.app_id)
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
        Self::detail_with_rich(&connection, app_id)
    }

    pub fn save_personal_dates(&self, input: &SavePersonalDatesInput) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        personal::save_personal_dates(&mut connection, input)?;
        Self::detail_with_rich(&connection, input.app_id)
    }

    pub fn store_metadata_refresh_due(&self, app_id: u32) -> AppResult<bool> {
        library::store_metadata_refresh_due(&self.open()?, app_id)
    }

    pub fn achievements_refresh_due(&self, app_id: u32) -> AppResult<bool> {
        library::achievements_refresh_due(&self.open()?, app_id)
    }

    pub fn mark_store_metadata_attempt(&self, app_id: u32, status: &str) -> AppResult<GameDetail> {
        let connection = self.open()?;
        library::mark_store_metadata_attempt(&connection, app_id, status)?;
        Self::detail_with_rich(&connection, app_id)
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
        Self::detail_with_rich(&connection, app_id)
    }

    pub fn mark_achievements_attempt(&self, app_id: u32, status: &str) -> AppResult<GameDetail> {
        let connection = self.open()?;
        library::mark_achievements_attempt(&connection, app_id, status)?;
        Self::detail_with_rich(&connection, app_id)
    }

    pub fn update_game(&self, input: &UpdateGameInput) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        library::update_game(&mut connection, input)?;
        Self::detail_with_rich(&connection, input.app_id)
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

    /// Persiste una sincronización completa de Steam como una sola unidad.
    ///
    /// El llamador debe conservar el guard exclusivo de mantenimiento durante
    /// toda la operación. La generación impide aplicar una respuesta de red
    /// obtenida contra una base que ya fue sustituida mediante restauración.
    pub(crate) fn persist_steam_sync(
        &self,
        expected_generation: u64,
        steam_id: &str,
        profile: Option<&SteamProfileWrite>,
        games: &[ImportedGame],
        family_catalog_games: &[ImportedFamilyCatalogGame],
        family_catalog_complete: bool,
    ) -> AppResult<(usize, usize)> {
        let _maintenance = self.maintenance_guard()?;
        if self.generation() != expected_generation {
            return Err(stale_steam_sync_error());
        }

        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        let current_steam_id = transaction
            .query_row(
                "SELECT steam_id FROM steam_accounts ORDER BY updated_at DESC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if current_steam_id.as_deref() != Some(steam_id) {
            return Err(AppError::new(
                "steam_account_changed",
                "La cuenta vinculada cambió durante la sincronización. Vuelve a intentarlo.",
            ));
        }

        transaction.execute(
            "INSERT INTO app_settings(key, value) VALUES ('steam_api_key_configured', 'true')
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            [],
        )?;
        family_catalog::save_in_transaction(
            &transaction,
            family_catalog_games,
            family_catalog_complete,
        )?;
        let counts = library::upsert_imported_games_in_transaction(&transaction, games, false)?;

        let (persona_name, avatar_url, profile_url, visibility) =
            profile.map_or((None, None, None, None), |profile| {
                (
                    Some(profile.persona_name.as_str()),
                    profile.avatar_url.as_deref(),
                    profile.profile_url.as_deref(),
                    profile.visibility,
                )
            });
        transaction.execute(
            "UPDATE steam_accounts SET
                persona_name = COALESCE(?2, persona_name),
                avatar_url = COALESCE(?3, avatar_url),
                profile_url = COALESCE(?4, profile_url),
                visibility = COALESCE(?5, visibility),
                last_sync_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                last_sync_status = 'success',
                last_sync_error_code = NULL,
                last_sync_error_message = NULL,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE steam_id = ?1",
            params![steam_id, persona_name, avatar_url, profile_url, visibility],
        )?;
        transaction.commit()?;
        Ok(counts)
    }

    pub fn list_family_catalog(
        &self,
        request: &FamilyCatalogRequest,
    ) -> AppResult<PagedFamilyCatalogGames> {
        family_catalog::list(&self.open()?, request)
    }

    /// Ficha completa: los datos de biblioteca más los metadatos enriquecidos.
    /// Es el único punto que compone ambas mitades, para que ninguna ruta pueda
    /// devolver una ficha a medias.
    fn detail_with_rich(connection: &Connection, app_id: u32) -> AppResult<GameDetail> {
        let mut detail = library::get_game_detail(connection, app_id)?;
        detail.rich = rich_metadata::get(connection, app_id)?;
        Ok(detail)
    }

    pub fn rich_game_metadata(&self, app_id: u32) -> AppResult<RichGameMetadata> {
        rich_metadata::get(&self.open()?, app_id)
    }

    pub fn drm_state_counts(&self) -> AppResult<DrmStateCounts> {
        rich_metadata::drm_state_counts(&self.open()?)
    }

    /// Persiste en una sola transacción todo lo que aporta una única respuesta
    /// de la tienda: los campos de biblioteca y los metadatos de ficha.
    pub fn save_store_bundle(
        &self,
        app_id: u32,
        metadata: &StoreMetadataUpdate,
        rich: &RichMetadataUpdate,
    ) -> AppResult<GameDetail> {
        let mut connection = self.open()?;
        {
            let transaction = connection.transaction()?;
            library::save_store_metadata(&transaction, app_id, metadata)?;
            rich_metadata::save_in_transaction(&transaction, app_id, rich)?;
            transaction.commit()?;
        }
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
        Self::detail_with_rich(&connection, app_id)
    }

    pub(crate) fn complete_metadata_enrichment_bundle(
        &self,
        app_id: u32,
        metadata: &StoreMetadataUpdate,
        rich: &RichMetadataUpdate,
    ) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        library::save_store_metadata(&transaction, app_id, metadata)?;
        rich_metadata::save_in_transaction(&transaction, app_id, rich)?;
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

    /// Reemplaza el catálogo de Familia con lo que se acaba de traer.
    ///
    /// `complete_snapshot` distingue una lectura completa —la que borra lo que
    /// ya no está— de una parcial, que sólo añade. Presentar una parcial como
    /// completa dejaría fuera juegos que sí siguen prestados.
    pub fn save_family_catalog(
        &self,
        games: &[ImportedFamilyCatalogGame],
        complete_snapshot: bool,
    ) -> AppResult<()> {
        family_catalog::save(&mut self.open()?, games, complete_snapshot)
    }

    // --- Sesión de Steam para Familia ---------------------------------------

    /// Resultado de la última sincronización del catálogo de Familia.
    ///
    /// Vive en `app_settings` y no en una tabla propia porque son tres valores
    /// sueltos de diagnóstico, no una entidad. El testigo **no** está aquí: ese
    /// vive en el llavero del sistema y en ningún otro sitio.
    pub fn family_session_diagnostics(
        &self,
    ) -> AppResult<(Option<String>, Option<u32>, Option<String>)> {
        let connection = self.open()?;
        let leer = |clave: &str| -> AppResult<Option<String>> {
            Ok(connection
                .query_row(
                    "SELECT value FROM app_settings WHERE key = ?1",
                    [clave],
                    |row| row.get::<_, String>(0),
                )
                .optional()?)
        };
        let momento = leer("steam_family_last_sync_at")?;
        let cuenta = leer("steam_family_last_app_count")?.and_then(|valor| valor.parse().ok());
        let error = leer("steam_family_last_error_code")?;
        Ok((momento, cuenta, error))
    }

    /// Anota que la sincronización terminó bien, y borra el fallo anterior.
    pub fn record_family_session_success(&self, moment: &str, app_count: u32) -> AppResult<()> {
        let mut connection = self.open()?;
        let transaction = connection.transaction()?;
        for (clave, valor) in [
            ("steam_family_last_sync_at", moment.to_string()),
            ("steam_family_last_app_count", app_count.to_string()),
        ] {
            transaction.execute(
                "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![clave, valor],
            )?;
        }
        transaction.execute(
            "DELETE FROM app_settings WHERE key = 'steam_family_last_error_code'",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Anota el fallo del último intento **sin** borrar lo que trajo el anterior:
    /// un catálogo de ayer sigue siendo mejor que ninguno, y quien lo mira tiene
    /// que poder ver las dos cosas.
    pub fn record_family_session_failure(&self, error_code: &str) -> AppResult<()> {
        self.open()?.execute(
            "INSERT INTO app_settings(key, value) VALUES ('steam_family_last_error_code', ?1)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![error_code],
        )?;
        Ok(())
    }

    // --- Avisos y bandeja de eventos ----------------------------------------

    pub fn list_notification_rules(
        &self,
        app_id: Option<u32>,
        now: DateTime<Utc>,
    ) -> AppResult<Vec<NotificationRule>> {
        notifications::list_rules(&self.open()?, app_id, now)
    }

    pub fn save_notification_rule(
        &self,
        input: &SaveNotificationRuleInput,
        now: DateTime<Utc>,
    ) -> AppResult<NotificationRule> {
        notifications::save_rule(&mut self.open()?, input, now)
    }

    pub fn delete_notification_rule(&self, id: &str) -> AppResult<()> {
        notifications::delete_rule(&self.open()?, id)
    }

    pub fn notification_inbox(
        &self,
        filter: &NotificationInboxFilter,
        limit: u32,
        offset: u32,
    ) -> AppResult<NotificationInbox> {
        notifications::inbox(&self.open()?, filter, limit, offset)
    }

    pub fn mark_notification_read(&self, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        notifications::mark_read(&self.open()?, event_id, now)
    }

    pub fn mark_all_notifications_read(&self, now: DateTime<Utc>) -> AppResult<u32> {
        notifications::mark_all_read(&mut self.open()?, now)
    }

    /// Borra los avisos descartados hace más de `retention_days` días.
    ///
    /// Nunca toca uno pendiente. Sin esto, `notification_events` crece sin tope
    /// durante toda la vida de la instalación.
    pub fn prune_notification_events(
        &self,
        now: DateTime<Utc>,
        retention_days: u32,
    ) -> AppResult<u32> {
        notifications::prune_events(&self.open()?, now, retention_days)
    }

    pub fn dismiss_all_notifications(&self, now: DateTime<Utc>) -> AppResult<u32> {
        notifications::dismiss_all(&mut self.open()?, now)
    }

    pub fn dismiss_notification(&self, event_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        notifications::dismiss_event(&self.open()?, event_id, now)
    }

    pub fn refresh_notification_events(
        &self,
        now: DateTime<Utc>,
    ) -> AppResult<NotificationRefreshReport> {
        notifications::refresh(&mut self.open()?, now)
    }

    // --- Prioridad dinámica y modelo de gustos -------------------------------
    // `Utc::now()` se resuelve en la frontera y no dentro del cálculo: el
    // núcleo es puro y determinista respecto al instante que recibe.

    pub fn recompute_priorities(&self) -> AppResult<PriorityRecomputeReport> {
        priority::recompute_priorities(&mut self.open()?, Utc::now())
    }

    pub fn explain_priority(&self, app_id: u32) -> AppResult<PriorityExplanation> {
        priority::explain_priority(&self.open()?, app_id)
    }

    pub fn set_priority_lock(&self, app_id: u32, locked: bool) -> AppResult<()> {
        priority::set_priority_lock(&self.open()?, app_id, locked)
    }

    pub fn list_priority_ranking(&self, limit: u32) -> AppResult<Vec<PriorityRanking>> {
        priority::list_priority_ranking(&self.open()?, limit)
    }

    pub fn learn_taste(&self) -> AppResult<TasteReport> {
        priority::learn_taste(&mut self.open()?, Utc::now())
    }

    pub fn record_taste_feedback(
        &self,
        app_id: u32,
        verdict: &str,
        surface: &str,
    ) -> AppResult<()> {
        priority::record_taste_feedback(&self.open()?, app_id, verdict, surface)
    }

    pub fn score_upcoming_releases(&self) -> AppResult<usize> {
        priority::score_upcoming(&mut self.open()?, Utc::now())
    }

    pub fn list_upcoming_releases(&self, limit: u32) -> AppResult<Vec<UpcomingRelease>> {
        priority::list_upcoming(&self.open()?, limit)
    }

    pub fn dismiss_upcoming_release(&self, app_id: u32) -> AppResult<()> {
        priority::dismiss_upcoming(&self.open()?, app_id)
    }

    // --- Precio observado -------------------------------------------------

    pub fn record_price_observation(
        &self,
        observation: &PriceObservation,
        now: DateTime<Utc>,
    ) -> AppResult<RecordedPrice> {
        pricing::record_observation(&mut self.open()?, observation, now)
    }

    /// Guarda que la tienda respondió por estos juegos sin dar precio.
    pub fn record_price_absences(
        &self,
        app_ids: &[u32],
        outcome: &str,
        now: DateTime<Utc>,
    ) -> AppResult<usize> {
        pricing::record_absences(&mut self.open()?, app_ids, outcome, now)
    }

    pub fn game_prices(&self, app_id: u32, now: DateTime<Utc>) -> AppResult<Vec<GamePrice>> {
        pricing::prices_for_game(&self.open()?, app_id, now)
    }

    pub fn game_price_history(
        &self,
        app_id: u32,
        currency: &str,
        limit: u32,
    ) -> AppResult<PriceHistory> {
        pricing::price_history(&self.open()?, app_id, currency, limit)
    }

    pub fn wishlist_price_statuses(
        &self,
        now: DateTime<Utc>,
    ) -> AppResult<Vec<WishlistPriceStatus>> {
        pricing::wishlist_price_statuses(&self.open()?, now)
    }

    /// Guarda sólo el veredicto de DRM de un juego.
    ///
    /// Lo usa la pasada ligera: el resto de la ficha ya está y no se toca.
    /// `drm_checked_at` queda sellado aunque el veredicto siga siendo
    /// desconocido, para no volver a preguntar por lo mismo mañana.
    pub fn save_drm_assessment(
        &self,
        app_id: u32,
        assessment: &rich_metadata::DrmAssessment,
    ) -> AppResult<()> {
        let mut connection = self.open()?;
        rich_metadata::save(
            &mut connection,
            app_id,
            &RichMetadataUpdate {
                drm: Some(assessment.clone()),
                ..RichMetadataUpdate::default()
            },
        )
    }

    /// Capturas guardadas de un juego, sin salir a la red.
    pub fn stored_preview(&self, app_id: u32) -> AppResult<preview::GamePreview> {
        preview::stored(&self.open()?, app_id)
    }

    /// Guarda las capturas que la tienda ha dado, incluida su ausencia.
    pub fn save_preview(
        &self,
        app_id: u32,
        thumbnails: &[String],
        now: DateTime<Utc>,
    ) -> AppResult<preview::GamePreview> {
        let mut connection = self.open()?;
        preview::save(&mut connection, app_id, thumbnails, now)
    }

    // --- Ofertas de la tienda ----------------------------------------------

    pub fn sync_store_deals(
        &self,
        store: &str,
        incoming: &[deals::IncomingDeal],
        now: DateTime<Utc>,
    ) -> AppResult<deals::DealSyncReport> {
        let mut connection = self.open()?;
        deals::sync(&mut connection, store, incoming, now)
    }

    pub fn pending_deal_facets(&self, limit: u32) -> AppResult<Vec<u32>> {
        deals::pending_facets(&self.open()?, limit)
    }

    pub fn save_deal_facets(
        &self,
        app_id: u32,
        genres: &[String],
        categories: &[String],
        developer: Option<&str>,
        publisher: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        deals::save_facets(
            &self.open()?,
            app_id,
            genres,
            categories,
            developer,
            publisher,
            now,
        )
    }

    pub fn score_store_deals(&self) -> AppResult<usize> {
        priority::score_deals(&mut self.open()?, Utc::now())
    }

    /// Las ofertas guardadas, con la fecha de la última tanda.
    pub fn store_deals_view(&self, limit: u32) -> AppResult<deals::StoreDealsView> {
        let connection = self.open()?;
        Ok(deals::StoreDealsView {
            deals: deals::list(&connection, limit)?,
            checked_at: deals::last_checked_at(&connection)?,
        })
    }

    /// La dirección de una oferta en su tienda.
    pub fn store_deal_url(&self, store: &str, external_id: &str) -> AppResult<String> {
        deals::url_of(&self.open()?, store, external_id)
    }

    pub fn dismiss_store_deal(
        &self,
        store: &str,
        external_id: &str,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        deals::dismiss(&self.open()?, store, external_id, now)
    }

    // --- Regalos de Epic ---------------------------------------------------

    /// Guarda una tanda de regalos y deja aviso de lo que sea noticia.
    pub fn sync_epic_free_offers(
        &self,
        games: &[crate::stores::epic_free::EpicFreeGame],
        now: DateTime<Utc>,
    ) -> AppResult<epic_free::EpicFreeSyncReport> {
        let mut connection = self.open()?;
        epic_free::sync(&mut connection, games, now)
    }

    /// Lo guardado, cruzado con la biblioteca.
    pub fn epic_free_offers(&self, now: DateTime<Utc>) -> AppResult<Vec<epic_free::EpicFreeOffer>> {
        epic_free::list(&self.open()?, now)
    }

    /// Descarta un regalo para que deje de aparecer.
    pub fn dismiss_epic_free_offer(&self, offer_id: &str, now: DateTime<Utc>) -> AppResult<()> {
        epic_free::dismiss(&self.open()?, offer_id, now)
    }

    pub fn stale_wishlist_price_targets(
        &self,
        now: DateTime<Utc>,
        limit: u32,
    ) -> AppResult<Vec<u32>> {
        pricing::stale_wishlist_app_ids(&self.open()?, now, limit)
    }

    pub fn forget_game_prices(&self, app_id: u32) -> AppResult<()> {
        pricing::forget_prices(&mut self.open()?, app_id)
    }

    // --- Archivado --------------------------------------------------------

    pub fn archive_games(
        &self,
        app_ids: &[u32],
        reason: &str,
        now: DateTime<Utc>,
    ) -> AppResult<ArchiveReport> {
        archive::archive_games(&mut self.open()?, app_ids, reason, now)
    }

    pub fn unarchive_games(&self, app_ids: &[u32]) -> AppResult<ArchiveReport> {
        archive::unarchive_games(&mut self.open()?, app_ids)
    }

    pub fn archived_games(&self, limit: u32, offset: u32) -> AppResult<PagedArchivedGames> {
        archive::list_archived(&self.open()?, limit, offset)
    }

    pub fn archived_game_count(&self) -> AppResult<i64> {
        archive::count(&self.open()?)
    }

    pub fn is_game_archived(&self, app_id: u32) -> AppResult<bool> {
        archive::is_archived(&self.open()?, app_id)
    }

    // --- Vistas guardadas ---------------------------------------------------

    pub fn list_saved_views(&self) -> AppResult<Vec<SavedView>> {
        saved_views::list(&self.open()?)
    }

    pub fn save_saved_view(&self, input: &SaveViewInput) -> AppResult<SavedView> {
        saved_views::save(&mut self.open()?, input)
    }

    pub fn delete_saved_view(&self, view_id: &str) -> AppResult<()> {
        saved_views::delete(&mut self.open()?, view_id)
    }

    pub fn reorder_saved_views(&self, ordered_ids: &[String]) -> AppResult<()> {
        saved_views::reorder(&mut self.open()?, ordered_ids)
    }

    pub fn mark_saved_view_used(&self, view_id: &str) -> AppResult<SavedView> {
        saved_views::mark_used(&self.open()?, view_id)
    }

    // --- Listas curadas -----------------------------------------------------

    pub fn list_curated_lists(&self) -> AppResult<Vec<CuratedList>> {
        curated::list_curated_lists(&self.open()?)
    }

    pub fn save_curated_list(&self, input: &SaveCuratedListInput) -> AppResult<CuratedList> {
        curated::save_curated_list(&mut self.open()?, input)
    }

    pub fn delete_curated_list(&self, list_id: &str) -> AppResult<()> {
        curated::delete_curated_list(&mut self.open()?, list_id)
    }

    pub fn reorder_curated_lists(&self, ordered_ids: &[String]) -> AppResult<()> {
        curated::reorder_curated_lists(&mut self.open()?, ordered_ids)
    }

    pub fn curated_list_detail(
        &self,
        list_id: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> AppResult<CuratedListDetail> {
        curated::curated_list_detail(&self.open()?, list_id, limit, offset)
    }

    pub fn add_curated_game(&self, input: &AddCuratedGameInput) -> AppResult<()> {
        curated::add_curated_game(&mut self.open()?, input)
    }

    pub fn update_curated_item(&self, input: &UpdateCuratedItemInput) -> AppResult<()> {
        curated::update_curated_item(&mut self.open()?, input)
    }

    pub fn remove_curated_game(&self, list_id: &str, app_id: u32) -> AppResult<()> {
        curated::remove_curated_game(&mut self.open()?, list_id, app_id)
    }

    pub fn move_curated_item(
        &self,
        list_id: &str,
        app_id: u32,
        before_app_id: Option<u32>,
    ) -> AppResult<()> {
        curated::move_curated_item(&mut self.open()?, list_id, app_id, before_app_id)
    }

    pub fn reorder_curated_items(&self, list_id: &str, ordered_app_ids: &[u32]) -> AppResult<()> {
        curated::reorder_curated_items(&mut self.open()?, list_id, ordered_app_ids)
    }

    // --- Deseados y vídeos --------------------------------------------------

    pub fn wishlist_overview(&self) -> AppResult<WishlistOverview> {
        wishlist::wishlist_overview(&self.open()?)
    }

    pub fn save_wishlist_entry(&self, input: &SaveWishlistEntryInput) -> AppResult<WishlistEntry> {
        wishlist::save_wishlist_entry(&mut self.open()?, input)
    }

    pub fn remove_wishlist_entry(&self, app_id: u32) -> AppResult<()> {
        wishlist::remove_wishlist_entry(&mut self.open()?, app_id)
    }

    pub fn move_wishlist_entry(
        &self,
        app_id: u32,
        bucket: &str,
        before_app_id: Option<u32>,
    ) -> AppResult<()> {
        wishlist::move_wishlist_entry(&mut self.open()?, app_id, bucket, before_app_id)
    }

    pub fn reorder_wishlist_bucket(&self, bucket: &str, ordered_app_ids: &[u32]) -> AppResult<()> {
        wishlist::reorder_wishlist_bucket(&mut self.open()?, bucket, ordered_app_ids)
    }

    pub fn import_steam_wishlist(
        &self,
        games: &[ImportedWishlistGame],
    ) -> AppResult<WishlistImportReport> {
        wishlist::import_steam_wishlist(&mut self.open()?, games)
    }

    pub fn list_game_videos(&self, app_id: u32, kind: Option<&str>) -> AppResult<Vec<GameVideo>> {
        wishlist::list_game_videos(&self.open()?, app_id, kind)
    }

    pub fn save_game_video(&self, input: &SaveGameVideoInput) -> AppResult<GameVideo> {
        wishlist::save_game_video(&mut self.open()?, input)
    }

    pub fn delete_game_video(&self, app_id: u32, provider: &str, video_id: &str) -> AppResult<()> {
        wishlist::delete_game_video(&mut self.open()?, app_id, provider, video_id)
    }

    pub fn reorder_game_videos(
        &self,
        app_id: u32,
        kind: &str,
        ordered: &[GameVideoRef],
    ) -> AppResult<()> {
        wishlist::reorder_game_videos(&mut self.open()?, app_id, kind, ordered)
    }

    pub fn list_game_dlc(&self, app_id: u32, filter: DlcFilter) -> AppResult<Vec<GameDlc>> {
        dlc::list_dlc(&self.open()?, app_id, filter)
    }

    pub fn game_dlc_summary(&self, app_id: u32) -> AppResult<DlcSummary> {
        dlc::dlc_summary(&self.open()?, app_id)
    }

    pub fn save_game_dlc(&self, app_id: u32, items: &[ImportedDlc]) -> AppResult<DlcImportSummary> {
        dlc::upsert_dlc_batch(&mut self.open()?, app_id, items)
    }

    pub fn claim_game_dlc_refresh(
        &self,
        app_id: u32,
        limit: usize,
    ) -> AppResult<Vec<DlcRefreshCandidate>> {
        dlc::claim_game_dlc_refresh_candidates(&mut self.open()?, app_id, limit)
    }

    pub fn mark_game_dlc_failed(&self, app_id: u32, dlc_app_id: u32) -> AppResult<()> {
        dlc::mark_dlc_metadata_failed(&self.open()?, app_id, dlc_app_id)
    }

    pub fn set_game_dlc_owned(
        &self,
        app_id: u32,
        dlc_app_id: u32,
        owned: bool,
    ) -> AppResult<GameDlc> {
        dlc::set_dlc_owned(&self.open()?, app_id, dlc_app_id, owned)
    }

    pub fn set_game_dlc_hidden(
        &self,
        app_id: u32,
        dlc_app_id: u32,
        hidden: bool,
    ) -> AppResult<GameDlc> {
        dlc::set_dlc_hidden(&self.open()?, app_id, dlc_app_id, hidden)
    }

    pub fn set_game_dlc_installed(
        &self,
        app_id: u32,
        dlc_app_id: u32,
        installed: bool,
    ) -> AppResult<GameDlc> {
        dlc::set_dlc_installed(&self.open()?, app_id, dlc_app_id, installed)
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

    pub fn artwork_targets(&self) -> AppResult<Vec<library::ArtworkTarget>> {
        library::artwork_targets(&self.open()?)
    }

    pub fn set_collection_appearance(&self, id: &str, color: &str, icon: &str) -> AppResult<()> {
        organization::set_collection_appearance(&mut self.open()?, id, color, icon)
    }

    pub fn reorder_collections(&self, ids: &[String]) -> AppResult<()> {
        organization::reorder_collections(&mut self.open()?, ids)
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

fn stale_steam_sync_error() -> AppError {
    AppError::new(
        "steam_sync_stale",
        "Los datos locales cambiaron durante la sincronización. Vuelve a intentarlo para aplicar una instantánea actual.",
    )
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
        ("art_cache_mib", "512"),
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

    fn valid_game_update(app_id: u32) -> UpdateGameInput {
        UpdateGameInput {
            app_id,
            status_id: "playing".to_string(),
            progress: 55,
            priority: 4,
            pinned: true,
            tracking: true,
            rating: Some(9),
            estimated_minutes: Some(180),
            target_date: Some("2026-09-01".to_string()),
            next_action: Some("Terminar el segundo acto".to_string()),
            checkpoint: Some("Campamento".to_string()),
            notes: Some("Decisiones pendientes".to_string()),
        }
    }

    #[test]
    fn sync_runs_record_and_list_newest_first() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = database_at(&directory, "sync-runs.sqlite3");

        database
            .record_sync_run(
                "local",
                "success",
                "2026-08-15T10:00:00Z",
                "2026-08-15T10:00:04Z",
                12,
                3,
                None,
            )
            .expect("registrar importación local");
        database
            .record_sync_run(
                "steam",
                "error",
                "2026-08-15T11:00:00Z",
                "2026-08-15T11:00:01Z",
                0,
                0,
                Some("steam_network"),
            )
            .expect("registrar fallo de sync");
        database
            .record_sync_run(
                "steam",
                "success",
                "2026-08-15T12:00:00Z",
                "2026-08-15T12:00:09Z",
                4,
                1810,
                None,
            )
            .expect("registrar sync correcta");

        let runs = database.list_sync_runs(8).expect("listar historial");
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].source, "steam");
        assert_eq!(runs[0].status, "success");
        assert_eq!(runs[0].updated_count, 1810);
        assert_eq!(runs[1].status, "error");
        assert_eq!(runs[1].error_message.as_deref(), Some("steam_network"));
        assert_eq!(runs[2].source, "local");
        assert_eq!(runs[2].imported_count, 12);

        let limited = database.list_sync_runs(1).expect("limitar historial");
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].started_at, "2026-08-15T12:00:00Z");
    }

    #[test]
    fn game_update_accepts_frontend_boundaries_and_unicode_character_counts() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "game-input-boundaries.sqlite3");
        insert_game(&database, 620, "Portal 2");
        let mut input = valid_game_update(620);
        input.next_action = Some("á".repeat(500));
        input.checkpoint = Some("ñ".repeat(2_000));
        input.notes = Some("🎮".repeat(20_000));

        let detail = database
            .update_game(&input)
            .expect("aceptar límites exactos en caracteres");
        assert_eq!(detail.summary.next_action, input.next_action);
        assert_eq!(detail.summary.checkpoint, input.checkpoint);
        assert_eq!(detail.summary.notes, input.notes);
    }

    #[test]
    fn game_update_rejects_text_over_frontend_limits() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "game-input-text.sqlite3");
        insert_game(&database, 620, "Portal 2");

        for (field, expected_message) in [
            (
                "next_action",
                "La siguiente acción no puede superar 500 caracteres.",
            ),
            (
                "checkpoint",
                "El checkpoint no puede superar 2000 caracteres.",
            ),
            ("notes", "Las notas no pueden superar 20000 caracteres."),
        ] {
            let mut input = valid_game_update(620);
            match field {
                "next_action" => input.next_action = Some("a".repeat(501)),
                "checkpoint" => input.checkpoint = Some("a".repeat(2_001)),
                "notes" => input.notes = Some("a".repeat(20_001)),
                _ => unreachable!(),
            }
            let error = database.update_game(&input).expect_err("rechazar exceso");
            assert_eq!(error.code, "validation");
            assert_eq!(error.message, expected_message);
        }
    }

    #[test]
    fn game_update_rejects_zero_duration_and_non_iso_or_impossible_dates() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "game-input-date.sqlite3");
        insert_game(&database, 620, "Portal 2");
        let mut zero_duration = valid_game_update(620);
        zero_duration.estimated_minutes = Some(0);
        assert_eq!(
            database
                .update_game(&zero_duration)
                .expect_err("rechazar duración cero")
                .message,
            "La duración estimada debe ser mayor que cero."
        );

        for invalid_date in ["01/09/2026", "2026-02-30", "2026-9-1"] {
            let mut input = valid_game_update(620);
            input.target_date = Some(invalid_date.to_string());
            let error = database
                .update_game(&input)
                .expect_err("rechazar fecha no válida");
            assert_eq!(error.code, "validation");
            assert_eq!(
                error.message,
                "La fecha objetivo debe usar el formato AAAA-MM-DD y ser válida."
            );
        }
    }

    #[test]
    fn blank_optional_game_date_is_persisted_as_no_date() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "game-input-blank-date.sqlite3");
        insert_game(&database, 620, "Portal 2");
        let mut input = valid_game_update(620);
        input.target_date = Some("   ".to_string());

        let detail = database
            .update_game(&input)
            .expect("aceptar fecha opcional vacía");
        assert_eq!(detail.summary.target_date, None);
    }

    #[test]
    fn game_update_rejects_invalid_payload_without_mutating_the_game() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "game-input-atomic.sqlite3");
        insert_game(&database, 620, "Portal 2");
        let mut original = valid_game_update(620);
        original.notes = Some("Original".to_string());
        database
            .update_game(&original)
            .expect("guardar estado inicial");

        let mut invalid = original.clone();
        invalid.notes = Some("x".repeat(20_001));
        let error = database
            .update_game(&invalid)
            .expect_err("rechazar payload directo no confiable");
        assert_eq!(error.code, "validation");
        assert_eq!(
            database
                .game_detail(620)
                .expect("leer juego sin mutar")
                .summary
                .notes
                .as_deref(),
            Some("Original")
        );
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
    fn steam_sync_rolls_back_catalog_library_profile_and_key_marker_together() {
        let directory = TempDir::new().expect("crear temporal");
        let database = database_at(&directory, "steam-sync-atomic.sqlite3");
        let steam_id = "76561198000000001";
        database
            .save_steam_identity(steam_id)
            .expect("vincular cuenta de prueba");
        database
            .set_steam_api_key_configured(false)
            .expect("guardar marcador inicial");
        family_catalog::save(
            &mut database.open().expect("abrir base"),
            &[ImportedFamilyCatalogGame {
                app_id: 10,
                title: "Catálogo anterior".into(),
                icon_url: None,
                cover_url: None,
                header_url: None,
                availability: "unknown".into(),
            }],
            true,
        )
        .expect("guardar catálogo inicial");
        database
            .open()
            .expect("abrir base")
            .execute_batch(
                "CREATE TRIGGER fail_profile_sync_for_test
                 BEFORE UPDATE OF last_sync_status ON steam_accounts
                 WHEN NEW.last_sync_status = 'success'
                 BEGIN
                    SELECT RAISE(ABORT, 'fallo de perfil simulado');
                 END;",
            )
            .expect("instalar failpoint transaccional");

        let imported_game = ImportedGame {
            app_id: 20,
            title: "No debe persistir".into(),
            icon_url: None,
            cover_url: None,
            header_url: None,
            playtime_minutes: 0,
            playtime_recent_minutes: 0,
            last_played_at: None,
            ownership_source: "owned".into(),
            family_availability: "not_applicable".into(),
            installation: None,
        };
        let error = database
            .persist_steam_sync(
                database.generation(),
                steam_id,
                Some(&SteamProfileWrite {
                    persona_name: "Perfil nuevo".into(),
                    avatar_url: None,
                    profile_url: None,
                    visibility: Some(3),
                }),
                &[imported_game],
                &[ImportedFamilyCatalogGame {
                    app_id: 30,
                    title: "Catálogo nuevo".into(),
                    icon_url: None,
                    cover_url: None,
                    header_url: None,
                    availability: "confirmed".into(),
                }],
                true,
            )
            .expect_err("forzar fallo de perfil tras escribir catálogo y biblioteca");

        assert_eq!(error.code, "database");
        let catalog = database
            .list_family_catalog(&FamilyCatalogRequest::default())
            .expect("leer catálogo tras rollback");
        assert_eq!(catalog.items.len(), 1);
        assert_eq!(catalog.items[0].app_id, 10);
        assert!(!has_game(&database, 20));
        let account = database
            .get_steam_account()
            .expect("leer cuenta")
            .expect("cuenta vinculada");
        assert_eq!(account.persona_name, None);
        assert_eq!(account.last_sync_at, None);
        assert_eq!(
            database.steam_api_key_configured().expect("leer marcador"),
            Some(false)
        );
    }

    #[test]
    fn successful_restore_advances_generation_and_rejects_an_old_steam_snapshot() {
        let directory = TempDir::new().expect("crear temporal");
        let active = database_at(&directory, "generation-active.sqlite3");
        active
            .save_steam_identity("76561198000000001")
            .expect("vincular cuenta activa");
        let captured_generation = active.generation();

        let restored = database_at(&directory, "generation-restored.sqlite3");
        restored
            .save_steam_identity("76561198000000002")
            .expect("vincular cuenta restaurada");
        active
            .import_backup(restored.path())
            .expect("restaurar copia válida");
        assert!(active.generation() > captured_generation);

        let error = active
            .persist_steam_sync(
                captured_generation,
                "76561198000000001",
                None,
                &[],
                &[],
                true,
            )
            .expect_err("rechazar instantánea obtenida antes de restaurar");
        assert_eq!(error.code, "steam_sync_stale");
        assert_eq!(
            active
                .get_steam_account()
                .expect("leer cuenta restaurada")
                .expect("cuenta disponible")
                .steam_id,
            "76561198000000002"
        );
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

#[cfg(test)]
mod pruebas_del_recuento_de_familia {
    use super::{library, migrations, organization, seed_defaults};
    use rusqlite::Connection;

    fn base() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar");
        connection
    }

    fn juego_propio(connection: &Connection, app_id: u32) {
        connection
            .execute(
                "INSERT INTO games(app_id, title, playtime_minutes) VALUES (?1, ?2, 0)",
                rusqlite::params![app_id, format!("Juego {app_id}")],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar ficha personal");
    }

    /// Un juego que llega por el préstamo familiar: vive en la biblioteca como
    /// `family_shared`, que es lo que le permite tener ficha personal.
    fn juego_prestado(connection: &Connection, app_id: u32) {
        connection
            .execute(
                "INSERT INTO games(app_id, title, playtime_minutes, ownership_source,
                                   family_availability)
                 VALUES (?1, ?2, 0, 'family_shared', 'unknown')",
                rusqlite::params![app_id, format!("Prestado {app_id}")],
            )
            .expect("insertar juego prestado");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar ficha personal");
    }

    #[test]
    fn la_cifra_de_una_tienda_es_la_que_se_encuentra_al_entrar() {
        // Ha fallado dos veces por el mismo motivo: contar por un lado y listar
        // por otro. La primera enseñaba juegos que la lista no traía; la
        // segunda contaba los archivados, y «Epic Games 553» abría una pantalla
        // con 395. La cifra se cuenta con las exclusiones del listado o miente.
        let connection = base();
        for (app_id, external_id) in [(2_000_000_001_u32, "uno"), (2_000_000_002, "dos")] {
            connection
                .execute(
                    "INSERT INTO games(app_id, title, ownership_source, external_store)
                     VALUES (?1, 'Un juego', 'owned', 'gog')",
                    [app_id],
                )
                .expect("insertar juego de la tienda");
            connection
                .execute(
                    "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                    [app_id],
                )
                .expect("insertar ficha personal");
            connection
                .execute(
                    "INSERT INTO external_games(store, external_id, title, local_app_id)
                     VALUES ('gog', ?1, 'Un juego', ?2)",
                    rusqlite::params![external_id, app_id],
                )
                .expect("vincular");
        }

        let stats = library::library_stats(&connection).expect("estadísticas");
        assert_eq!(stats.external_store_games.get("gog"), Some(&2));

        // Archivar uno lo saca del listado, así que también de la cifra.
        connection
            .execute("INSERT INTO game_archive(app_id) VALUES (2000000002)", [])
            .expect("archivar");
        let stats = library::library_stats(&connection).expect("estadísticas");
        assert_eq!(
            stats.external_store_games.get("gog"),
            Some(&1),
            "un juego archivado no se enseña, así que tampoco se cuenta"
        );
    }

    #[test]
    fn los_prestados_se_cuentan_aparte_de_los_propios() {
        // La cifra tiene que ser la misma que se encuentra al entrar en «Steam
        // Family»: un recuento que no coincide con lo que hay dentro es un
        // fallo. Y no se suma a los propios, porque tener un juego prestado a la
        // vista no es tenerlo.
        let connection = base();
        juego_propio(&connection, 10);
        juego_propio(&connection, 20);
        juego_prestado(&connection, 30);
        juego_prestado(&connection, 40);

        let stats = library::library_stats(&connection).expect("estadísticas");

        assert_eq!(stats.total_games, 2, "los propios son dos");
        assert_eq!(stats.family_catalog_games, 2, "los prestados son otros dos");
    }

    #[test]
    fn la_migracion_deja_las_caratulas_de_familia_en_la_variante_grande() {
        // La 034 hizo esto con `games` y dejó fuera el catálogo de Family, que
        // se alimenta por otro camino: las mismas carátulas se veían peor en una
        // pantalla que en la otra, y las que no publican la variante pequeña ni
        // llegaban a cargar.
        let connection = base();
        connection
            .execute(
                "INSERT INTO family_catalog_games(app_id, title, cover_url, availability)
                 VALUES (?1, 'Prestado', ?2, 'unknown')",
                rusqlite::params![
                    10,
                    "https://shared.steamstatic.com/store_item_assets/steam/apps/10/library_600x900.jpg"
                ],
            )
            .expect("insertar juego de familia");

        // La migración ya corrió al crear la base, así que se aplica su misma
        // sentencia sobre la fila recién insertada para comprobar el efecto.
        connection
            .execute(
                "UPDATE family_catalog_games
                    SET cover_url = replace(cover_url, 'library_600x900.jpg', 'library_600x900_2x.jpg')
                  WHERE cover_url LIKE '%library_600x900.jpg'",
                [],
            )
            .expect("aplicar la reescritura");

        let url: String = connection
            .query_row(
                "SELECT cover_url FROM family_catalog_games WHERE app_id = 10",
                [],
                |row| row.get(0),
            )
            .expect("leer la portada");
        assert!(url.ends_with("library_600x900_2x.jpg"), "quedó en {url}");
    }

    #[test]
    fn el_recuento_de_un_estado_coincide_con_lo_que_enseña_al_pulsarlo() {
        // Un número en la barra lateral que lleva a «ningún juego coincide» es
        // peor que no tener número. Pasó al dar ficha personal al catálogo de
        // Family: el recuento los contaba y el listado los escondía.
        let connection = base();
        juego_propio(&connection, 10);
        juego_prestado(&connection, 20);
        juego_prestado(&connection, 30);

        let estados = organization::list_statuses(&connection).expect("estados");
        let sin_clasificar = estados
            .iter()
            .find(|estado| estado.id == "unclassified")
            .expect("existe el estado por defecto");

        let request = crate::models::GameListRequest {
            status_id: Some("unclassified".to_string()),
            ..Default::default()
        };
        let listado = library::list_games(&connection, &request, None).expect("listar");

        assert_eq!(
            sin_clasificar.game_count as usize,
            listado.items.len(),
            "la barra lateral ofrece {} y el listado enseña {}",
            sin_clasificar.game_count,
            listado.items.len()
        );
        assert_eq!(
            sin_clasificar.game_count, 1,
            "sólo el propio está en la biblioteca"
        );
    }

    #[test]
    fn el_ambito_de_familia_sí_enseña_los_prestados_por_estado() {
        // Y clasificar un juego prestado tiene que servir de algo: dentro de su
        // ámbito, el filtro por estado lo encuentra.
        let connection = base();
        juego_prestado(&connection, 20);

        let request = crate::models::GameListRequest {
            status_id: Some("unclassified".to_string()),
            ownership_source: Some("family_shared".to_string()),
            ..Default::default()
        };
        let listado = library::list_games(&connection, &request, None).expect("listar");

        assert_eq!(listado.items.len(), 1);
        assert_eq!(listado.items[0].app_id, 20);
    }

    #[test]
    fn sin_catalogo_de_familia_la_cifra_es_cero_y_no_una_ausencia() {
        // Cero es una respuesta: significa que no hay nada prestado. La interfaz
        // la usa para no enseñar un recuento vacío junto a «Steam Family».
        let connection = base();
        juego_propio(&connection, 10);

        let stats = library::library_stats(&connection).expect("estadísticas");

        assert_eq!(stats.family_catalog_games, 0);
    }
}
