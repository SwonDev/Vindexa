use crate::art_cache::{self, ArtVariant, CachedArt};
use crate::db::recovery::StartupRecovery;
use crate::db::{
    CachedNewsInput, Database, DiscoverySnapshot, FamilyCatalogGame, FamilyCatalogRequest,
    GameReminder, LibraryDropInput, LibraryDropReceipt, LibraryDropResult, NewsRefreshReport,
    PagedFamilyCatalogGames, SavePersonalDatesInput, SaveReminderInput, SaveSessionInput,
    SaveTagInput, SteamProfileWrite, TagDefinition,
};
use crate::error::{AppError, AppResult};
use crate::models::{
    AppBootstrap, AppPreferences, BulkUpdateStatusInput, CollectionSummary, DatabaseDiagnostics,
    DatabaseRecoverySnapshot, GameDetail, GameListRequest, LibraryFilterOptions,
    LocalSteamImportResult, MetadataEnrichmentStatus, MovePlannerItemInput, PagedGameSessions,
    PagedGames, PlannerColumn, PlannerOverview, PlannerSettings, Recommendation,
    RecommendationRequest, SaveCollectionInput, SavePlannerItemInput, SmartRule, StatusDefinition,
    SteamConfiguration, SteamSyncResult, UpdateCheckResult, UpdateGameInput,
};
use crate::steam::{self, GameAction};
use crate::store_window;
use chrono::Utc;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};
use tokio::task::JoinSet;
use tokio::time::{Duration, sleep};

static NEWS_REFRESH_LOCK: Mutex<()> = Mutex::const_new(());
const NEWS_REFRESH_BATCH: usize = 4;

#[derive(Debug, Clone)]
pub struct AppState {
    pub database: Database,
    pub startup_recovery: Arc<StdMutex<StartupRecovery>>,
    pub cache_dir: PathBuf,
    pub maintenance: Arc<RwLock<()>>,
    pub steam_login_lock: Arc<Mutex<()>>,
    pub steam_sync_lock: Arc<Mutex<()>>,
    pub metadata_enrichment: Arc<steam::metadata_enrichment::MetadataEnrichmentCoordinator>,
    pub achievements_lock: Arc<Mutex<()>>,
}

fn lock_startup_recovery(
    recovery: &StdMutex<StartupRecovery>,
) -> AppResult<std::sync::MutexGuard<'_, StartupRecovery>> {
    recovery.lock().map_err(|_| {
        AppError::new(
            "database_recovery_state",
            "El estado de recuperación quedó bloqueado de forma inesperada.",
        )
    })
}

async fn blocking<T, F>(task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_error| {
            AppError::new(
                "background_task",
                "La tarea interna no pudo finalizar. Vuelve a intentarlo.",
            )
        })?
}

async fn database_read<T, F>(state: &State<'_, AppState>, task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(Database) -> AppResult<T> + Send + 'static,
{
    let database = state.database.clone();
    let maintenance = state.maintenance.clone();
    blocking(move || {
        let _guard = maintenance.blocking_read();
        task(database)
    })
    .await
}

async fn database_write<T, F>(state: &State<'_, AppState>, task: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce(Database) -> AppResult<T> + Send + 'static,
{
    let database = state.database.clone();
    let maintenance = state.maintenance.clone();
    blocking(move || {
        let _guard = maintenance.blocking_write();
        task(database)
    })
    .await
}

async fn discovery_database_at_generation<T, F>(
    database: Database,
    maintenance: Arc<RwLock<()>>,
    expected_generation: Option<u64>,
    task: F,
) -> AppResult<(u64, T)>
where
    T: Send + 'static,
    F: FnOnce(Database) -> AppResult<T> + Send + 'static,
{
    blocking(move || {
        // El guard se limita al acceso SQLite. Las esperas HTTP quedan fuera,
        // pero una restauración no puede cruzarse entre esta comprobación y
        // la operación reclamada sobre la generación activa.
        let _guard = maintenance.blocking_read();
        let generation = database.generation();
        if expected_generation.is_some_and(|expected| expected != generation) {
            return Err(stale_discovery_refresh_error());
        }
        Ok((generation, task(database)?))
    })
    .await
}

fn stale_discovery_refresh_error() -> AppError {
    AppError::new(
        "discovery_refresh_stale",
        "Los datos locales cambiaron durante la actualización. Vuelve a intentarlo para cargar publicaciones actuales.",
    )
}

fn steam_configuration(database: &Database) -> AppResult<SteamConfiguration> {
    let local = steam::detect_local_steam();
    let api_key_marker = database.steam_api_key_configured()?;
    Ok(SteamConfiguration {
        account: database.get_steam_account()?,
        api_key_configured: api_key_marker == Some(true),
        api_key_verification_required: api_key_marker.is_none(),
        local_steam_detected: local.is_some(),
        local_manifest_count: local.map_or(0, |(_, count)| count),
    })
}

fn try_begin_steam_login(lock: Arc<Mutex<()>>) -> AppResult<OwnedMutexGuard<()>> {
    lock.try_lock_owned().map_err(|_| {
        AppError::new(
            "openid_in_progress",
            "Ya hay un inicio de sesión con Steam en curso.",
        )
    })
}

fn try_begin_steam_sync(lock: Arc<Mutex<()>>) -> AppResult<OwnedMutexGuard<()>> {
    lock.try_lock_owned().map_err(|_| {
        AppError::new(
            "steam_sync_in_progress",
            "Ya hay una sincronización de Steam en curso.",
        )
    })
}

async fn await_steam_network<T, F>(request: F) -> AppResult<T>
where
    F: Future<Output = AppResult<T>>,
{
    // No recibe ni adquiere maintenance: toda espera de red debe terminar
    // antes de entrar en la sección exclusiva de persistencia.
    request.await
}

async fn persist_steam_sync_failure_if_current(
    state: &State<'_, AppState>,
    expected_generation: u64,
    steam_id: String,
    error: AppError,
) {
    let _ = database_write(state, move |database| {
        if database.generation() != expected_generation
            || database
                .get_steam_account()?
                .as_ref()
                .map(|account| &account.steam_id)
                != Some(&steam_id)
        {
            return Ok(());
        }
        if error.code == "steam_api_key_missing" {
            database.set_steam_api_key_configured(false)?;
        } else if error.code != "secure_storage" {
            database.set_steam_api_key_configured(true)?;
        }
        steam::mark_sync_failed(&database, &steam_id, &error)
    })
    .await;
}

#[tauri::command]
pub async fn bootstrap(state: State<'_, AppState>) -> AppResult<AppBootstrap> {
    let bootstrap = database_read(&state, move |database| {
        let steam = steam_configuration(&database)?;
        database.bootstrap(steam)
    })
    .await?;
    steam::metadata_enrichment::start_worker(
        state.database.clone(),
        state.metadata_enrichment.clone(),
        state.maintenance.clone(),
    );
    Ok(bootstrap)
}

#[tauri::command]
pub async fn get_database_recovery_status(
    state: State<'_, AppState>,
) -> AppResult<DatabaseRecoverySnapshot> {
    let recovery = state.startup_recovery.clone();
    blocking(move || Ok(lock_startup_recovery(&recovery)?.snapshot())).await
}

#[tauri::command]
pub async fn select_database_recovery_backup(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<DatabaseRecoverySnapshot> {
    let selected = blocking(move || {
        app.dialog()
            .file()
            .add_filter("Copia de Vindexa", &["sqlite3", "db"])
            .blocking_pick_file()
            .map(|path| {
                path.into_path().map_err(|_| {
                    AppError::new("dialog_path", "La copia seleccionada no es una ruta local.")
                })
            })
            .transpose()
    })
    .await?;
    let recovery = state.startup_recovery.clone();
    blocking(move || {
        let mut recovery = lock_startup_recovery(&recovery)?;
        if let Some(path) = selected {
            recovery.add_selected_candidate(path)?;
        }
        Ok(recovery.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn refresh_database_recovery_backups(
    state: State<'_, AppState>,
) -> AppResult<DatabaseRecoverySnapshot> {
    let recovery = state.startup_recovery.clone();
    blocking(move || {
        let mut recovery = lock_startup_recovery(&recovery)?;
        recovery.refresh_candidates();
        Ok(recovery.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn restore_database_recovery_backup(
    state: State<'_, AppState>,
    candidate_id: String,
    confirmation: String,
) -> AppResult<DatabaseRecoverySnapshot> {
    let recovery = state.startup_recovery.clone();
    let maintenance = state.maintenance.clone();
    blocking(move || {
        let _maintenance = maintenance.blocking_write();
        let mut recovery = lock_startup_recovery(&recovery)?;
        recovery.restore(&candidate_id, &confirmation)?;
        Ok(recovery.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn create_clean_database_after_recovery(
    state: State<'_, AppState>,
    confirmation: String,
) -> AppResult<DatabaseRecoverySnapshot> {
    let recovery = state.startup_recovery.clone();
    let maintenance = state.maintenance.clone();
    blocking(move || {
        let _maintenance = maintenance.blocking_write();
        let mut recovery = lock_startup_recovery(&recovery)?;
        recovery.create_clean(&confirmation)?;
        Ok(recovery.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn export_quarantined_database(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<bool> {
    let selected = blocking(move || {
        app.dialog()
            .file()
            .add_filter("Base SQLite en cuarentena", &["sqlite3"])
            .set_file_name(format!(
                "vindexa-quarantine-{}.sqlite3",
                Utc::now().format("%Y%m%d-%H%M%S")
            ))
            .blocking_save_file()
            .map(|path| {
                path.into_path().map_err(|_| {
                    AppError::new(
                        "dialog_path",
                        "El destino seleccionado no es una ruta local.",
                    )
                })
            })
            .transpose()
    })
    .await?;
    let Some(destination) = selected else {
        return Ok(false);
    };
    let recovery = state.startup_recovery.clone();
    blocking(move || {
        lock_startup_recovery(&recovery)?.export_quarantined_database(&destination)?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn list_games(
    state: State<'_, AppState>,
    request: GameListRequest,
) -> AppResult<PagedGames> {
    database_read(&state, move |database| database.list_games(&request)).await
}

#[tauri::command]
pub async fn get_library_filter_options(
    state: State<'_, AppState>,
) -> AppResult<LibraryFilterOptions> {
    database_read(&state, move |database| database.library_filter_options()).await
}

#[tauri::command]
pub async fn get_game_detail(state: State<'_, AppState>, app_id: u32) -> AppResult<GameDetail> {
    database_read(&state, move |database| database.game_detail(app_id)).await
}

#[tauri::command]
pub async fn list_tags(state: State<'_, AppState>) -> AppResult<Vec<TagDefinition>> {
    database_read(&state, move |database| database.list_tags()).await
}

#[tauri::command]
pub async fn save_tag(state: State<'_, AppState>, input: SaveTagInput) -> AppResult<TagDefinition> {
    database_write(&state, move |database| database.save_tag(&input)).await
}

#[tauri::command]
pub async fn delete_tag(state: State<'_, AppState>, id: String) -> AppResult<()> {
    database_write(&state, move |database| database.delete_tag(&id)).await
}

#[tauri::command]
pub async fn set_game_tags(
    state: State<'_, AppState>,
    app_id: u32,
    tag_ids: Vec<String>,
) -> AppResult<GameDetail> {
    database_write(&state, move |database| {
        database.set_game_tags(app_id, &tag_ids)
    })
    .await
}

#[tauri::command]
pub async fn save_game_session(
    state: State<'_, AppState>,
    input: SaveSessionInput,
) -> AppResult<GameDetail> {
    database_write(&state, move |database| database.save_session(&input)).await
}

#[tauri::command]
pub async fn list_game_sessions(
    state: State<'_, AppState>,
    app_id: u32,
    limit: u32,
    offset: u32,
) -> AppResult<PagedGameSessions> {
    database_read(&state, move |database| {
        database.list_game_sessions(app_id, limit, offset)
    })
    .await
}

#[tauri::command]
pub async fn delete_game_session(state: State<'_, AppState>, id: String) -> AppResult<GameDetail> {
    database_write(&state, move |database| database.delete_session(&id)).await
}

#[tauri::command]
pub async fn save_personal_dates(
    state: State<'_, AppState>,
    input: SavePersonalDatesInput,
) -> AppResult<GameDetail> {
    database_write(&state, move |database| database.save_personal_dates(&input)).await
}

#[tauri::command]
pub async fn refresh_game_metadata(
    state: State<'_, AppState>,
    app_id: u32,
    force: Option<bool>,
) -> AppResult<GameDetail> {
    let _maintenance_guard = state.maintenance.read().await;
    let database = state.database.clone();
    let refresh_due = blocking({
        let database = database.clone();
        move || database.store_metadata_refresh_due(app_id)
    })
    .await?;
    if !force.unwrap_or(false) && !refresh_due {
        return blocking(move || database.game_detail(app_id)).await;
    }

    match state.metadata_enrichment.fetch(app_id).await {
        Ok(steam::store_api::StoreMetadataOutcome::Found(metadata)) => {
            blocking(move || database.save_store_metadata(app_id, &metadata)).await
        }
        Ok(steam::store_api::StoreMetadataOutcome::Unavailable) => {
            blocking(move || database.mark_store_metadata_attempt(app_id, "unavailable")).await
        }
        Err(_) => blocking(move || database.mark_store_metadata_attempt(app_id, "failed")).await,
    }
}

#[tauri::command]
pub async fn start_metadata_enrichment(
    state: State<'_, AppState>,
    visible_app_ids: Vec<u32>,
    include_backlog: Option<bool>,
) -> AppResult<MetadataEnrichmentStatus> {
    database_read(&state, move |database| {
        database.enqueue_metadata_enrichment(&visible_app_ids, include_backlog.unwrap_or(false))?;
        Ok(())
    })
    .await?;
    steam::metadata_enrichment::start_worker(
        state.database.clone(),
        state.metadata_enrichment.clone(),
        state.maintenance.clone(),
    );
    metadata_enrichment_status(state).await
}

#[tauri::command]
pub async fn metadata_enrichment_status(
    state: State<'_, AppState>,
) -> AppResult<MetadataEnrichmentStatus> {
    let running = state.metadata_enrichment.is_running();
    let mut status = database_read(&state, move |database| {
        database.metadata_enrichment_status()
    })
    .await?;
    status.running = running;
    Ok(status)
}

#[tauri::command]
pub async fn refresh_game_achievements(
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<GameDetail> {
    let achievements_lock = state.achievements_lock.clone();
    let _achievements_guard = achievements_lock.lock().await;
    let _maintenance_guard = state.maintenance.read().await;
    let database = state.database.clone();
    let refresh_due = blocking({
        let database = database.clone();
        move || database.achievements_refresh_due(app_id)
    })
    .await?;
    if !refresh_due {
        return blocking(move || database.game_detail(app_id)).await;
    }
    let account = blocking({
        let database = database.clone();
        move || {
            database.get_steam_account()?.ok_or_else(|| {
                AppError::new(
                    "steam_not_linked",
                    "Vincula tu cuenta de Steam antes de actualizar los logros.",
                )
            })
        }
    })
    .await?;
    match steam::achievements::fetch_saved(&account.steam_id, app_id).await {
        Ok(steam::achievements::AchievementOutcome::Found(summary)) => {
            blocking(move || database.save_achievements(app_id, summary.unlocked, summary.total))
                .await
        }
        Ok(steam::achievements::AchievementOutcome::Unavailable) => {
            blocking(move || database.mark_achievements_attempt(app_id, "unavailable")).await
        }
        Err(error) => {
            let persisted = error.clone();
            let _ = blocking(move || database.mark_achievements_attempt(app_id, "failed")).await;
            Err(persisted)
        }
    }
}

#[tauri::command]
pub async fn update_game(
    state: State<'_, AppState>,
    input: UpdateGameInput,
) -> AppResult<GameDetail> {
    database_read(&state, move |database| database.update_game(&input)).await
}

#[tauri::command]
pub async fn bulk_update_status(
    state: State<'_, AppState>,
    app_ids: Vec<u32>,
    status_id: String,
) -> AppResult<usize> {
    let input = BulkUpdateStatusInput { app_ids, status_id };
    database_read(&state, move |database| database.bulk_update_status(&input)).await
}

#[tauri::command]
pub async fn apply_library_drop(
    state: State<'_, AppState>,
    input: LibraryDropInput,
) -> AppResult<LibraryDropResult> {
    database_read(&state, move |database| database.apply_library_drop(&input)).await
}

#[tauri::command]
pub async fn undo_library_drop(
    state: State<'_, AppState>,
    receipt: LibraryDropReceipt,
) -> AppResult<usize> {
    database_read(&state, move |database| database.undo_library_drop(&receipt)).await
}

#[tauri::command]
pub async fn save_collection(
    state: State<'_, AppState>,
    input: SaveCollectionInput,
) -> AppResult<CollectionSummary> {
    database_read(&state, move |database| database.save_collection(&input)).await
}

#[tauri::command]
pub async fn preview_smart_collection(
    state: State<'_, AppState>,
    input: SaveCollectionInput,
) -> AppResult<PagedGames> {
    database_read(&state, move |database| {
        database.preview_smart_collection(&input)
    })
    .await
}

#[tauri::command]
pub async fn delete_collection(state: State<'_, AppState>, id: String) -> AppResult<()> {
    database_read(&state, move |database| database.delete_collection(&id)).await
}

#[tauri::command]
pub async fn list_smart_rules(
    state: State<'_, AppState>,
    collection_id: String,
) -> AppResult<Vec<SmartRule>> {
    database_read(&state, move |database| database.smart_rules(&collection_id)).await
}

#[tauri::command]
pub async fn reorder_collections(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    database_read(&state, move |database| database.reorder_collections(&ids)).await
}

#[tauri::command]
pub async fn set_collection_games(
    state: State<'_, AppState>,
    collection_id: String,
    app_ids: Vec<u32>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.set_collection_games(&collection_id, &app_ids)
    })
    .await
}

#[tauri::command]
pub async fn set_game_collections(
    state: State<'_, AppState>,
    app_id: u32,
    collection_ids: Vec<String>,
) -> AppResult<GameDetail> {
    database_read(&state, move |database| {
        database.set_game_collections(app_id, &collection_ids)?;
        database.game_detail(app_id)
    })
    .await
}

#[tauri::command]
pub async fn move_planner_item(
    state: State<'_, AppState>,
    input: MovePlannerItemInput,
) -> AppResult<()> {
    database_read(&state, move |database| database.move_planner_item(&input)).await
}

#[tauri::command]
pub async fn get_planner_overview(state: State<'_, AppState>) -> AppResult<PlannerOverview> {
    database_read(&state, move |database| database.planner_overview()).await
}

#[tauri::command]
pub async fn move_planner_queue_item(
    state: State<'_, AppState>,
    app_id: u32,
    position: i64,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.move_planner_queue_item(app_id, position)
    })
    .await
}

#[tauri::command]
pub async fn save_planner_item(
    state: State<'_, AppState>,
    input: SavePlannerItemInput,
) -> AppResult<()> {
    database_read(&state, move |database| database.save_planner_item(&input)).await
}

#[tauri::command]
pub async fn save_planner_capacity(
    state: State<'_, AppState>,
    settings: PlannerSettings,
) -> AppResult<PlannerSettings> {
    database_read(&state, move |database| {
        database.save_planner_settings(&settings)
    })
    .await
}

#[tauri::command]
pub async fn remove_planner_item(state: State<'_, AppState>, app_id: u32) -> AppResult<()> {
    database_read(&state, move |database| database.remove_planner_item(app_id)).await
}

#[tauri::command]
pub async fn save_status(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    color: String,
) -> AppResult<StatusDefinition> {
    database_read(&state, move |database| {
        database.save_status(id.as_deref(), &name, &color)
    })
    .await
}

#[tauri::command]
pub async fn delete_status(
    state: State<'_, AppState>,
    id: String,
    replacement_id: String,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.delete_status(&id, &replacement_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_statuses(state: State<'_, AppState>, ids: Vec<String>) -> AppResult<()> {
    database_read(&state, move |database| database.reorder_statuses(&ids)).await
}

#[tauri::command]
pub async fn save_planner_column(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    color: String,
    wip_limit: Option<u32>,
) -> AppResult<PlannerColumn> {
    database_read(&state, move |database| {
        database.save_planner_column(id.as_deref(), &name, &color, wip_limit)
    })
    .await
}

#[tauri::command]
pub async fn delete_planner_column(
    state: State<'_, AppState>,
    id: String,
    replacement_id: Option<String>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.delete_planner_column(&id, replacement_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn reorder_planner_columns(
    state: State<'_, AppState>,
    ids: Vec<String>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.reorder_planner_columns(&ids)
    })
    .await
}

#[tauri::command]
pub async fn import_local_steam(state: State<'_, AppState>) -> AppResult<LocalSteamImportResult> {
    database_write(&state, move |database| {
        let scan = steam::scan_local_library()?;
        let (imported_games, updated_games) = database.upsert_imported_games(&scan.games, true)?;
        Ok(LocalSteamImportResult {
            steam_path: scan.steam_path,
            libraries_scanned: scan.libraries_scanned,
            imported_games,
            updated_games,
        })
    })
    .await
}

#[tauri::command]
pub async fn start_steam_login(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let _login_guard = try_begin_steam_login(state.steam_login_lock.clone())?;
    let _maintenance = state.maintenance.read().await;
    steam::authenticate_openid(&app, &state.database)
        .await
        .map(|_| ())
}

#[tauri::command]
pub async fn save_steam_api_key(state: State<'_, AppState>, api_key: String) -> AppResult<()> {
    let _credential_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;
    database_read(&state, move |database| {
        steam::save_api_key(&api_key)?;
        database.set_steam_api_key_configured(true)
    })
    .await
}

#[tauri::command]
pub async fn delete_steam_api_key(state: State<'_, AppState>) -> AppResult<()> {
    let _credential_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;
    database_read(&state, move |database| {
        steam::delete_api_key()?;
        database.set_steam_api_key_configured(false)
    })
    .await
}

#[tauri::command]
pub async fn verify_saved_steam_api_key(state: State<'_, AppState>) -> AppResult<bool> {
    let _credential_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;
    database_read(&state, move |database| {
        let configured = steam::has_api_key()?;
        database.set_steam_api_key_configured(configured)?;
        Ok(configured)
    })
    .await
}

#[tauri::command]
pub async fn sync_steam_library(state: State<'_, AppState>) -> AppResult<SteamSyncResult> {
    let _sync_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;
    let (account, generation) = database_read(&state, move |database| {
        let generation = database.generation();
        let account = database.get_steam_account()?.ok_or_else(|| {
            AppError::new(
                "steam_not_linked",
                "Vincula tu cuenta de Steam antes de sincronizar la biblioteca.",
            )
        })?;
        Ok((account, generation))
    })
    .await?;

    // La red queda deliberadamente fuera del guard de mantenimiento. Las
    // lecturas, autosaves y diagnósticos locales siguen disponibles aunque
    // Steam o un miembro de Family tarde en responder.
    let snapshot = match await_steam_network(steam::fetch_saved_account(&account.steam_id)).await {
        Ok(snapshot) => snapshot,
        Err(error) => {
            persist_steam_sync_failure_if_current(
                &state,
                generation,
                account.steam_id,
                error.clone(),
            )
            .await;
            return Err(error);
        }
    };
    let steam_id = snapshot.steam_id.clone();
    let profile = snapshot.profile.as_ref().map(|profile| SteamProfileWrite {
        persona_name: profile.persona_name.clone(),
        avatar_url: profile.avatar_url.clone(),
        profile_url: profile.profile_url.clone(),
        visibility: profile.visibility,
    });
    let result = database_write(&state, move |database| {
        let (imported_games, updated_games) = database.persist_steam_sync(
            generation,
            &snapshot.steam_id,
            profile.as_ref(),
            &snapshot.games,
            &snapshot.family_catalog,
            snapshot.family_catalog_complete,
        )?;
        Ok(SteamSyncResult {
            steam_id: snapshot.steam_id,
            imported_games,
            updated_games,
            private_library_suspected: snapshot.private_library_suspected,
            family_members_detected: snapshot.family_members_detected,
            family_members_inaccessible: snapshot.family_members_inaccessible,
            family_games_imported: snapshot.family_games_imported,
            family_catalog_games_detected: snapshot.family_catalog.len(),
            completed_at: Utc::now().to_rfc3339(),
        })
    })
    .await;
    if let Err(error) = &result {
        persist_steam_sync_failure_if_current(&state, generation, steam_id, error.clone()).await;
    }
    result
}

#[tauri::command]
pub async fn list_family_catalog(
    state: State<'_, AppState>,
    request: FamilyCatalogRequest,
) -> AppResult<PagedFamilyCatalogGames> {
    database_read(&state, move |database| {
        database.list_family_catalog(&request)
    })
    .await
}

#[tauri::command]
pub async fn get_family_catalog_game(
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<FamilyCatalogGame> {
    database_read(&state, move |database| database.family_catalog_game(app_id)).await
}

#[tauri::command]
pub async fn unlink_steam(state: State<'_, AppState>) -> AppResult<()> {
    database_read(&state, move |database| database.unlink_steam()).await
}

#[tauri::command]
pub async fn recommend_game(
    state: State<'_, AppState>,
    request: RecommendationRequest,
) -> AppResult<Recommendation> {
    database_read(&state, move |database| {
        database.recommend(&request)?.ok_or_else(|| {
            AppError::new(
                "recommendation_empty",
                "No hay ningún juego que encaje con esos criterios todavía.",
            )
        })
    })
    .await
}

#[tauri::command]
pub async fn get_discovery_snapshot(state: State<'_, AppState>) -> AppResult<DiscoverySnapshot> {
    database_read(&state, move |database| database.discovery_snapshot()).await
}

#[tauri::command]
pub async fn refresh_discovery_news(state: State<'_, AppState>) -> AppResult<NewsRefreshReport> {
    let _refresh_guard = NEWS_REFRESH_LOCK.lock().await;
    let mut refresh_generation = None;
    let mut attempted_games = 0_u32;
    let mut refreshed_games = 0_u32;
    let mut publications_saved = 0_u32;
    let mut failed_games = 0_u32;
    let mut last_failure = None;

    loop {
        let (generation, candidates) = discovery_database_at_generation(
            state.database.clone(),
            state.maintenance.clone(),
            refresh_generation,
            move |database| database.claim_news_refresh_candidates(NEWS_REFRESH_BATCH),
        )
        .await?;
        refresh_generation = Some(generation);
        if candidates.is_empty() {
            break;
        }
        attempted_games = attempted_games.saturating_add(candidates.len() as u32);
        let batch_was_full = candidates.len() == NEWS_REFRESH_BATCH;
        let mut tasks = JoinSet::new();
        for candidate in candidates {
            tasks.spawn(async move {
                let result = steam::news_api::fetch(candidate.app_id).await;
                (candidate, result)
            });
        }
        while let Some(task) = tasks.join_next().await {
            let (candidate, result) = task.map_err(|_| {
                AppError::new(
                    "steam_news_task",
                    "Una consulta de publicaciones terminó de forma inesperada.",
                )
            })?;
            match result {
                Ok(publications) => {
                    let inputs = publications
                        .into_iter()
                        .map(|publication| CachedNewsInput {
                            gid: publication.gid,
                            title: publication.title,
                            content_preview: publication.content_preview,
                            published_at: publication.published_at,
                            feed_label: publication.feed_label,
                            feed_name: publication.feed_name,
                        })
                        .collect::<Vec<_>>();
                    let saved_count = inputs.len() as u32;
                    discovery_database_at_generation(
                        state.database.clone(),
                        state.maintenance.clone(),
                        Some(generation),
                        move |database| database.save_news_success(candidate.app_id, &inputs),
                    )
                    .await?;
                    publications_saved = publications_saved.saturating_add(saved_count);
                    refreshed_games = refreshed_games.saturating_add(1);
                }
                Err(failure) => {
                    let delay = steam::news_api::retry_delay_seconds(
                        &failure.error.code,
                        candidate.attempts,
                        failure.retry_after_seconds,
                    );
                    let error_code = failure.error.code.clone();
                    discovery_database_at_generation(
                        state.database.clone(),
                        state.maintenance.clone(),
                        Some(generation),
                        move |database| {
                            database.save_news_failure(
                                candidate.app_id,
                                candidate.attempts,
                                &error_code,
                                delay,
                            )
                        },
                    )
                    .await?;
                    failed_games = failed_games.saturating_add(1);
                    last_failure = Some(failure.error);
                }
            }
        }
        if batch_was_full {
            sleep(Duration::from_millis(750)).await;
        }
    }

    let (_, snapshot) = discovery_database_at_generation(
        state.database.clone(),
        state.maintenance.clone(),
        refresh_generation,
        move |database| database.discovery_snapshot(),
    )
    .await?;
    if attempted_games > 0 && failed_games == attempted_games {
        return Err(last_failure.unwrap_or_else(|| {
            AppError::new(
                "steam_news_unavailable",
                "No se pudieron actualizar las publicaciones oficiales de Steam.",
            )
        }));
    }
    if attempted_games == 0
        && snapshot.capabilities.tracked_news_games > 0
        && snapshot.official_publications.is_empty()
        && snapshot.capabilities.news_last_refreshed_at.is_none()
        && snapshot.capabilities.news_next_refresh_at.is_some()
    {
        return Err(AppError::new(
            "steam_news_deferred",
            "Steam no respondió. El siguiente reintento seguro ya está programado.",
        ));
    }
    Ok(NewsRefreshReport {
        attempted_games,
        refreshed_games,
        publications_saved,
        failed_games,
        skipped_by_cadence: snapshot
            .capabilities
            .tracked_news_games
            .saturating_sub(attempted_games),
        next_refresh_at: snapshot.capabilities.news_next_refresh_at,
    })
}

#[tauri::command]
pub async fn save_reminder(
    state: State<'_, AppState>,
    input: SaveReminderInput,
) -> AppResult<GameReminder> {
    database_read(&state, move |database| database.save_reminder(&input)).await
}

#[tauri::command]
pub async fn complete_reminder(state: State<'_, AppState>, id: String) -> AppResult<()> {
    database_read(&state, move |database| database.complete_reminder(&id)).await
}

#[tauri::command]
pub async fn snooze_reminder(
    state: State<'_, AppState>,
    id: String,
    due_at: String,
) -> AppResult<GameReminder> {
    database_read(&state, move |database| {
        database.snooze_reminder(&id, &due_at)
    })
    .await
}

#[tauri::command]
pub async fn dismiss_recommendation(
    state: State<'_, AppState>,
    history_id: String,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.dismiss_recommendation(&history_id)
    })
    .await
}

#[tauri::command]
pub async fn restore_recommendation(
    state: State<'_, AppState>,
    history_id: String,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.restore_recommendation(&history_id)
    })
    .await
}

#[tauri::command]
pub async fn get_database_diagnostics(
    state: State<'_, AppState>,
) -> AppResult<DatabaseDiagnostics> {
    database_read(&state, move |database| database.diagnostics()).await
}

#[tauri::command]
pub async fn export_backup(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    let selected = blocking(move || {
        app.dialog()
            .file()
            .add_filter("Base de datos SQLite", &["sqlite3", "db"])
            .set_file_name(format!(
                "vindexa-backup-{}.sqlite3",
                Utc::now().format("%Y%m%d-%H%M%S")
            ))
            .blocking_save_file()
            .map(|path| {
                path.into_path().map_err(|_| {
                    AppError::new(
                        "dialog_path",
                        "El destino seleccionado no es una ruta local.",
                    )
                })
            })
            .transpose()
    })
    .await?;
    let Some(path) = selected else {
        return Ok(false);
    };
    database_write(&state, move |database| {
        database.export_backup(&path)?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub async fn import_backup(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    let selected = blocking(move || {
        app.dialog()
            .file()
            .add_filter("Base de datos SQLite", &["sqlite3", "db"])
            .blocking_pick_file()
            .map(|path| {
                path.into_path().map_err(|_| {
                    AppError::new(
                        "dialog_path",
                        "El archivo seleccionado no es una ruta local.",
                    )
                })
            })
            .transpose()
    })
    .await?;
    let Some(path) = selected else {
        return Ok(false);
    };
    database_write(&state, move |database| {
        database.import_backup(&path)?;
        Ok(true)
    })
    .await
}

#[tauri::command]
pub fn launch_game(app: AppHandle, app_id: u32) -> AppResult<()> {
    steam::open_game_action(&app, app_id, GameAction::Launch)
}

#[tauri::command]
pub fn install_game(app: AppHandle, app_id: u32) -> AppResult<()> {
    steam::open_game_action(&app, app_id, GameAction::Install)
}

#[tauri::command]
pub async fn uninstall_game(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<()> {
    let _maintenance = state.maintenance.read().await;
    steam::request_uninstall(&app, &state.database, app_id)
}

#[tauri::command]
pub async fn open_store(app: AppHandle, app_id: u32) -> AppResult<()> {
    store_window::open(&app, app_id).await
}

#[tauri::command]
pub async fn reveal_installation(
    app: AppHandle,
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<()> {
    let _maintenance = state.maintenance.read().await;
    steam::reveal_installation(&app, &state.database, app_id)
}

#[tauri::command]
pub async fn cache_game_art(
    state: State<'_, AppState>,
    app_id: u32,
    variant: String,
    source_url: Option<String>,
) -> AppResult<CachedArt> {
    let _maintenance = state.maintenance.read().await;
    let variant = ArtVariant::parse(&variant)?;
    art_cache::cache(
        &state.database,
        &state.cache_dir,
        app_id,
        variant,
        source_url.as_deref(),
    )
    .await
}

#[tauri::command]
pub async fn clear_art_cache(state: State<'_, AppState>) -> AppResult<()> {
    let cache_dir = state.cache_dir.clone();
    database_write(&state, move |database| {
        art_cache::clear(&database, &cache_dir)
    })
    .await
}

#[tauri::command]
pub async fn save_preferences(
    state: State<'_, AppState>,
    preferences: AppPreferences,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.save_preferences(&preferences)
    })
    .await
}

#[tauri::command]
pub fn check_for_updates(app: AppHandle) -> UpdateCheckResult {
    let current_version = app.package_info().version.to_string();
    UpdateCheckResult {
        status: "notConfigured".into(),
        current_version,
        available_version: None,
        message: "Este build no tiene todavía un endpoint de versiones ni una clave pública de firma configurados. Vindexa no descargará ni instalará nada automáticamente.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        await_steam_network, blocking, discovery_database_at_generation, steam_configuration,
        try_begin_steam_login, try_begin_steam_sync,
    };
    use crate::db::{CachedNewsInput, Database, ImportedGame};
    use crate::error::{AppError, AppResult};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::{Mutex, RwLock, oneshot};
    use tokio::time::{Duration, timeout};

    #[test]
    fn bootstrap_configuration_uses_the_non_secret_database_marker() {
        let directory = TempDir::new().expect("crear temporal");
        let database = Database::new(directory.path().join("bootstrap.sqlite3"));
        database.initialize().expect("inicializar base");

        let unknown = steam_configuration(&database).expect("leer configuración inicial");
        assert!(!unknown.api_key_configured);
        assert!(unknown.api_key_verification_required);

        database
            .set_steam_api_key_configured(true)
            .expect("guardar marcador no secreto");
        let configured = steam_configuration(&database).expect("leer configuración marcada");
        assert!(configured.api_key_configured);
        assert!(!configured.api_key_verification_required);

        database
            .set_steam_api_key_configured(false)
            .expect("borrar marcador no secreto");
        let missing = steam_configuration(&database).expect("leer configuración vacía");
        assert!(!missing.api_key_configured);
        assert!(!missing.api_key_verification_required);
    }

    #[test]
    fn steam_login_is_singleflight_and_recovers_after_completion() {
        let lock = Arc::new(Mutex::new(()));
        let first = try_begin_steam_login(lock.clone()).expect("iniciar primer login");
        let error = try_begin_steam_login(lock.clone()).expect_err("rechazar login paralelo");
        assert_eq!(error.code, "openid_in_progress");
        drop(first);
        try_begin_steam_login(lock).expect("permitir login posterior");
    }

    #[test]
    fn background_task_failures_do_not_expose_panic_details() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime");
        let error = runtime.block_on(async {
            blocking(|| -> AppResult<()> {
                panic!("token=fixture-secret at /Users/example/private.sqlite3")
            })
            .await
            .expect_err("convertir panic del worker en error público")
        });

        assert_eq!(error.code, "background_task");
        assert_eq!(
            error.message,
            "La tarea interna no pudo finalizar. Vuelve a intentarlo."
        );
        assert!(!error.message.contains("fixture-secret"));
        assert!(!error.message.contains("/Users/example"));
    }

    #[test]
    fn steam_sync_is_singleflight_and_recovers_after_completion() {
        let lock = Arc::new(Mutex::new(()));
        let first = try_begin_steam_sync(lock.clone()).expect("iniciar primera sincronización");
        let error =
            try_begin_steam_sync(lock.clone()).expect_err("rechazar sincronización paralela");
        assert_eq!(error.code, "steam_sync_in_progress");
        drop(first);
        try_begin_steam_sync(lock).expect("permitir sincronización posterior");
    }

    #[test]
    fn steam_credential_changes_share_the_sync_exclusion() {
        let lock = Arc::new(Mutex::new(()));
        let sync = try_begin_steam_sync(lock.clone()).expect("iniciar sincronización");
        let error = try_begin_steam_sync(lock.clone())
            .expect_err("rechazar acceso al secreto durante la sincronización");
        assert_eq!(error.code, "steam_sync_in_progress");
        drop(sync);
        try_begin_steam_sync(lock).expect("permitir acceso al secreto al terminar");
    }

    #[test]
    fn pending_steam_network_does_not_block_database_readers() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime");
        runtime.block_on(async {
            let directory = TempDir::new().expect("crear temporal");
            let database = Database::new(directory.path().join("steam-network-lock.sqlite3"));
            database.initialize().expect("inicializar base");
            let maintenance = Arc::new(RwLock::new(()));
            let (started_tx, started_rx) = oneshot::channel();
            let (release_tx, release_rx) = oneshot::channel();
            let network = tokio::spawn(await_steam_network(async move {
                let _ = started_tx.send(());
                let _ = release_rx.await;
                Err::<(), _>(AppError::new("test_network", "fin de la red simulada"))
            }));
            started_rx.await.expect("iniciar red simulada");

            let reader_database = database.clone();
            let reader_maintenance = maintenance.clone();
            let account = timeout(
                Duration::from_secs(1),
                blocking(move || {
                    let _reader = reader_maintenance.blocking_read();
                    reader_database.get_steam_account()
                }),
            )
            .await
            .expect("una lectura local debe seguir disponible durante la red");
            assert!(account.expect("leer base local").is_none());
            release_tx.send(()).expect("liberar red simulada");
            let error = network
                .await
                .expect("finalizar tarea")
                .expect_err("conservar error simulado");
            assert_eq!(error.code, "test_network");
        });
    }

    #[test]
    fn discovery_network_wait_does_not_block_restore_and_rejects_its_stale_result() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime");
        runtime.block_on(async {
            let directory = TempDir::new().expect("crear temporal");
            let database = Database::new(directory.path().join("discovery-generation.sqlite3"));
            database.initialize().expect("inicializar base");
            database
                .upsert_imported_games(
                    &[ImportedGame {
                        app_id: 10,
                        title: "Viajero".into(),
                        icon_url: None,
                        cover_url: None,
                        header_url: None,
                        playtime_minutes: 0,
                        playtime_recent_minutes: 0,
                        last_played_at: None,
                        ownership_source: "owned".into(),
                        family_availability: "not_applicable".into(),
                        installation: None,
                    }],
                    true,
                )
                .expect("importar juego seguido");
            database
                .open()
                .expect("abrir base")
                .execute(
                    "UPDATE game_personal SET tracking = 1 WHERE app_id = 10",
                    [],
                )
                .expect("activar seguimiento");

            let maintenance = Arc::new(RwLock::new(()));
            let refresh_database = database.clone();
            let refresh_maintenance = maintenance.clone();
            let (network_started_tx, network_started_rx) = oneshot::channel();
            let (network_release_tx, network_release_rx) = oneshot::channel();
            let refresh = tokio::spawn(async move {
                let (generation, candidates) = discovery_database_at_generation(
                    refresh_database.clone(),
                    refresh_maintenance.clone(),
                    None,
                    |database| database.claim_news_refresh_candidates(4),
                )
                .await?;
                assert_eq!(candidates.len(), 1);
                let _ = network_started_tx.send(());
                network_release_rx.await.expect("liberar respuesta HTTP");

                discovery_database_at_generation(
                    refresh_database,
                    refresh_maintenance,
                    Some(generation),
                    |database| {
                        database.save_news_success(
                            10,
                            &[CachedNewsInput {
                                gid: "1840944183772671".into(),
                                title: "Resultado anterior".into(),
                                content_preview: "No debe cruzar la restauración".into(),
                                published_at: "2026-08-14T12:00:00+00:00".into(),
                                feed_label: "Community Announcements".into(),
                                feed_name: "steam_community_announcements".into(),
                            }],
                        )
                    },
                )
                .await
            });
            network_started_rx.await.expect("iniciar espera HTTP");

            let restore_database = database.clone();
            let restore_maintenance = maintenance.clone();
            let restore = tokio::spawn(async move {
                let _guard = restore_maintenance.write().await;
                // El restore real incrementa esta misma generación al activar
                // la base sustituida.
                restore_database.advance_generation();
            });
            timeout(Duration::from_secs(1), restore)
                .await
                .expect("el escritor no debe esperar a toda la red")
                .expect("completar restauración simulada");

            network_release_tx
                .send(())
                .expect("resolver respuesta HTTP");
            let error = timeout(Duration::from_secs(1), refresh)
                .await
                .expect("el refresco debe terminar")
                .expect("unir tarea")
                .expect_err("rechazar el resultado de la generación anterior");
            assert_eq!(error.code, "discovery_refresh_stale");

            let (_, snapshot) =
                discovery_database_at_generation(database, maintenance, None, |database| {
                    database.discovery_snapshot()
                })
                .await
                .expect("consultar generación activa");
            assert!(snapshot.official_publications.is_empty());
        });
    }
}
