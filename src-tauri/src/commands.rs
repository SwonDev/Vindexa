use crate::agent::{
    self, AgentAuditEntry, AgentClientSummary, AgentOutcome, AgentRequest, IssuedAgentClient,
    NewAgentClient, Requester,
};
use crate::art_cache::{self, ArtVariant, CachedArt};
use crate::db::recovery::StartupRecovery;
use crate::db::{
    AddCuratedGameInput, ArchiveReport, CachedNewsInput, CuratedList, CuratedListDetail, Database,
    DiscoverySnapshot, DlcFilter, DlcRefreshReport, DlcSummary, DrmStateCounts, FamilyCatalogGame,
    FamilyCatalogRequest, GameDlc, GamePrice, GameReminder, GameVideo, GameVideoRef, ImportedDlc,
    ImportedWishlistGame, LibraryDropInput, LibraryDropReceipt, LibraryDropResult,
    NewsRefreshReport, NotificationInbox, NotificationInboxFilter, NotificationRefreshReport,
    NotificationRule, PagedArchivedGames, PagedFamilyCatalogGames, PriceHistory,
    PriceRefreshReport, PriorityExplanation, PriorityRanking, PriorityRecomputeReport,
    RichGameMetadata, SaveCuratedListInput, SaveGameVideoInput, SaveNotificationRuleInput,
    SavePersonalDatesInput, SaveReminderInput, SaveSessionInput, SaveTagInput, SaveViewInput,
    SaveWishlistEntryInput, SavedView, SteamProfileWrite, SteamWishlistImportResult, TagDefinition,
    TasteReport, UpcomingRelease, UpdateCuratedItemInput, WishlistEntry, WishlistOverview,
    WishlistPriceStatus,
};
use crate::error::{AppError, AppResult};
use crate::localmodel::{self, LocalModelSurvey};
use crate::vindagent;
use crate::models::{
    AppBootstrap, AppPreferences, BulkUpdateStatusInput, CollectionSummary, DatabaseDiagnostics,
    DatabaseRecoverySnapshot, GameDetail, GameListRequest, LibraryFilterOptions,
    LocalSteamImportResult, MetadataEnrichmentStatus, MovePlannerItemInput, PagedGameSessions,
    PagedGames, PlannerColumn, PlannerOverview, PlannerSettings, Recommendation,
    RecommendationRequest, SaveCollectionInput, SavePlannerItemInput, SmartRule, StatusDefinition,
    SteamConfiguration, SteamSyncResult, SyncRun, UpdateCheckResult, UpdateGameInput,
};
use crate::steam::{self, GameAction};
use crate::store_window;
use crate::stores::{
    self, ExternalStore, StoreDetection,
    db::{
        ExternalGame, ExternalGameRequest, ExternalStoreAccount, ExternalStoreScanReport,
        PagedExternalGames,
    },
    launch::ExternalGameAction,
    online::{ExternalStoreSession, SignOutReport as StoreSignOutReport, StoreLoginPrompt},
};
use crate::updates;
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
static PRICE_REFRESH_LOCK: Mutex<()> = Mutex::const_new(());
/// Mismo ritmo que el enriquecimiento de fichas. `appdetails` no documenta
/// su límite de peticiones, así que se reutiliza el intervalo que Vindexa ya
/// considera prudente en lugar de inventar uno nuevo.
/// Días que se conserva un aviso ya descartado.
///
/// Tres meses: lo bastante para que alguien pueda revisar qué pasó el trimestre
/// pasado, y lo bastante poco para que la tabla no crezca sin fin.
const NOTIFICATION_RETENTION_DAYS: u32 = 90;

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
pub async fn bootstrap(app: AppHandle, state: State<'_, AppState>) -> AppResult<AppBootstrap> {
    let app_version = app.package_info().version.to_string();
    let bootstrap = database_read(&state, move |database| {
        let steam = steam_configuration(&database)?;
        database.bootstrap(steam, app_version)
    })
    .await?;

    // Poda de avisos ya descartados, una vez por arranque. Sin ella la tabla
    // crece durante toda la vida de la instalación. Un fallo aquí no puede
    // impedir que la aplicación abra: es limpieza, no un requisito.
    let ahora = Utc::now();
    if let Err(error) = database_read(&state, move |database| {
        database.prune_notification_events(ahora, NOTIFICATION_RETENTION_DAYS)
    })
    .await
    {
        eprintln!("Vindexa no pudo podar los avisos antiguos: {}", error.code);
    }

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

    match state.metadata_enrichment.fetch_bundle(app_id).await {
        Ok(steam::store_api::StoreBundleOutcome::Found(bundle)) => {
            blocking(move || database.save_store_bundle(app_id, &bundle.metadata, &bundle.rich))
                .await
        }
        Ok(steam::store_api::StoreBundleOutcome::Unavailable) => {
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

/// Cambia el color y el icono de una colección desde el menú de acciones
/// rápidas, sin tocar su nombre, su descripción ni sus reglas.
#[tauri::command]
pub async fn set_collection_appearance(
    state: State<'_, AppState>,
    id: String,
    color: String,
    icon: String,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.set_collection_appearance(&id, &color, &icon)
    })
    .await
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
pub async fn list_sync_runs(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> AppResult<Vec<SyncRun>> {
    database_read(&state, move |database| {
        database.list_sync_runs(limit.unwrap_or(8).min(50))
    })
    .await
}

/// Contrasta el arte de la biblioteca con el índice oficial de recursos de la
/// tienda de Steam y corrige las columnas que estén mal.
///
/// Existe como acción explícita además del refresco automático porque la
/// sincronización vuelve a sembrar `cover_url` y `header_url` con la URL
/// derivada por convención, que no existe para buena parte del catálogo
/// moderno: quien vea una carátula rota puede forzar la corrección sin esperar.
#[tauri::command]
pub async fn refresh_steam_art(
    state: State<'_, AppState>,
) -> AppResult<steam::store_assets::ArtIndexReport> {
    steam::store_assets::refresh_library_art(&state.database).await
}

/// Lanza en segundo plano la corrección del arte contra el índice de la tienda.
///
/// No se espera al resultado: la importación ya ha terminado y el arte es
/// accesorio. Un fallo de red deja la biblioteca con lo que tenía.
fn schedule_steam_art_refresh(database: &Database) {
    let database = database.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = steam::store_assets::refresh_library_art(&database).await {
            eprintln!(
                "Vindexa no pudo contrastar el arte con la tienda de Steam: {}",
                error.code
            );
        }
    });
}

#[tauri::command]
pub async fn import_local_steam(state: State<'_, AppState>) -> AppResult<LocalSteamImportResult> {
    let started_at = Utc::now().to_rfc3339();
    let result = database_write(&state, move |database| {
        let scan = steam::scan_local_library()?;
        let (imported_games, updated_games) = database.upsert_imported_games(&scan.games, true)?;
        // El historial nunca puede tumbar la importación: se registra a fuego lento.
        let _ = database.record_sync_run(
            "local",
            "success",
            &started_at,
            &Utc::now().to_rfc3339(),
            imported_games,
            updated_games,
            None,
        );
        Ok(LocalSteamImportResult {
            steam_path: scan.steam_path,
            libraries_scanned: scan.libraries_scanned,
            imported_games,
            updated_games,
        })
    })
    .await;
    if result.is_ok() {
        schedule_steam_art_refresh(&state.database);
    }
    result
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
    let started_at = Utc::now().to_rfc3339();
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
            let message = error.message.clone();
            let started = started_at.clone();
            let _ = database_read(&state, move |database| {
                database.record_sync_run(
                    "steam",
                    "error",
                    &started,
                    &Utc::now().to_rfc3339(),
                    0,
                    0,
                    Some(&message),
                )
            })
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
        let _ = database.record_sync_run(
            "steam",
            "success",
            &started_at,
            &Utc::now().to_rfc3339(),
            imported_games,
            updated_games,
            None,
        );
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
    } else {
        // `persist_steam_sync` vuelve a escribir `cover_url` y `header_url` con
        // la URL derivada por convención, que devuelve 404 para buena parte del
        // catálogo moderno. El índice de la tienda tiene el nombre real de cada
        // archivo: se contrasta en segundo plano para no retrasar el resultado.
        schedule_steam_art_refresh(&state.database);
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

/// Presupuesto de fichas individuales que una actualización explícita pide de
/// una sentada. Lo que no entre queda en la cola con estado `pending`.
const DLC_DETAIL_BUDGET: usize = 40;

// ---------------------------------------------------------------------------
// Avisos y bandeja de eventos
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_notification_rules(
    state: State<'_, AppState>,
    app_id: Option<u32>,
) -> AppResult<Vec<NotificationRule>> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.list_notification_rules(app_id, now)
    })
    .await
}

#[tauri::command]
pub async fn save_notification_rule(
    state: State<'_, AppState>,
    input: SaveNotificationRuleInput,
) -> AppResult<NotificationRule> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.save_notification_rule(&input, now)
    })
    .await
}

#[tauri::command]
pub async fn delete_notification_rule(state: State<'_, AppState>, id: String) -> AppResult<()> {
    database_read(&state, move |database| {
        database.delete_notification_rule(&id)
    })
    .await
}

#[tauri::command]
pub async fn get_notification_inbox(
    state: State<'_, AppState>,
    filter: Option<NotificationInboxFilter>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> AppResult<NotificationInbox> {
    let filter = filter.unwrap_or_default();
    database_read(&state, move |database| {
        database.notification_inbox(&filter, limit.unwrap_or(0), offset.unwrap_or(0))
    })
    .await
}

#[tauri::command]
pub async fn mark_notification_read(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.mark_notification_read(&id, now)
    })
    .await
}

#[tauri::command]
pub async fn mark_all_notifications_read(state: State<'_, AppState>) -> AppResult<u32> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.mark_all_notifications_read(now)
    })
    .await
}

/// Descarta todos los avisos pendientes de una vez. Devuelve cuántos cambiaron.
///
/// «Marcar todo como leído» y «descartar todos» son cosas distintas: lo primero
/// deja el aviso en la bandeja sin resaltar, lo segundo lo saca de la vista de
/// pendientes. Quien vuelve tras una semana quiere lo segundo.
#[tauri::command]
pub async fn dismiss_all_notifications(state: State<'_, AppState>) -> AppResult<u32> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.dismiss_all_notifications(now)
    })
    .await
}

#[tauri::command]
pub async fn dismiss_notification(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.dismiss_notification(&id, now)
    })
    .await
}

#[tauri::command]
pub async fn refresh_notification_events(
    state: State<'_, AppState>,
) -> AppResult<NotificationRefreshReport> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.refresh_notification_events(now)
    })
    .await
}

// ---------------------------------------------------------------------------
// Prioridad dinámica y modelo de gustos
// ---------------------------------------------------------------------------

/// Recalcular reescribe `priority_signals` entera, así que toma exclusión de
/// mantenimiento; el resto son operaciones fila a fila y no la necesitan.
#[tauri::command]
pub async fn recompute_priorities(
    state: State<'_, AppState>,
) -> AppResult<PriorityRecomputeReport> {
    database_write(&state, move |database| database.recompute_priorities()).await
}

#[tauri::command]
pub async fn explain_priority(
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<PriorityExplanation> {
    database_read(&state, move |database| database.explain_priority(app_id)).await
}

#[tauri::command]
pub async fn set_priority_lock(
    state: State<'_, AppState>,
    app_id: u32,
    locked: bool,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.set_priority_lock(app_id, locked)
    })
    .await
}

#[tauri::command]
pub async fn list_priority_ranking(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> AppResult<Vec<PriorityRanking>> {
    database_read(&state, move |database| {
        database.list_priority_ranking(limit.unwrap_or(60))
    })
    .await
}

#[tauri::command]
pub async fn learn_taste(state: State<'_, AppState>) -> AppResult<TasteReport> {
    database_write(&state, move |database| database.learn_taste()).await
}

/// Revisa una tanda de tu lista de deseados y guarda los que aún no han salido.
///
/// Es lo que da de comer al motor de gustos: sin candidatos no hay nada que
/// puntuar. Va por tandas porque cada ficha cuesta una petición a la tienda, y
/// devuelve cuántos quedan para que la interfaz pueda decirlo en vez de dejar a
/// la persona adivinando si ya está.
#[tauri::command]
pub async fn refresh_upcoming_releases(
    state: State<'_, AppState>,
) -> AppResult<steam::upcoming::UpcomingRefreshReport> {
    let database = state.database.clone();
    steam::upcoming::refresh_from_wishlist(&database).await
}

#[tauri::command]
pub async fn record_taste_feedback(
    state: State<'_, AppState>,
    app_id: u32,
    verdict: String,
    surface: String,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.record_taste_feedback(app_id, &verdict, &surface)
    })
    .await
}

#[tauri::command]
pub async fn score_upcoming_releases(state: State<'_, AppState>) -> AppResult<usize> {
    database_write(&state, move |database| database.score_upcoming_releases()).await
}

#[tauri::command]
pub async fn list_upcoming_releases(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> AppResult<Vec<UpcomingRelease>> {
    database_read(&state, move |database| {
        database.list_upcoming_releases(limit.unwrap_or(40))
    })
    .await
}

#[tauri::command]
pub async fn dismiss_upcoming_release(state: State<'_, AppState>, app_id: u32) -> AppResult<()> {
    database_read(&state, move |database| {
        database.dismiss_upcoming_release(app_id)
    })
    .await
}

// --- Precio observado ---------------------------------------------------------

#[tauri::command]
pub async fn list_wishlist_prices(
    state: State<'_, AppState>,
) -> AppResult<Vec<WishlistPriceStatus>> {
    let now = Utc::now();
    database_read(&state, move |database| {
        database.wishlist_price_statuses(now)
    })
    .await
}

#[tauri::command]
pub async fn get_game_prices(state: State<'_, AppState>, app_id: u32) -> AppResult<Vec<GamePrice>> {
    let now = Utc::now();
    database_read(&state, move |database| database.game_prices(app_id, now)).await
}

#[tauri::command]
pub async fn get_game_price_history(
    state: State<'_, AppState>,
    app_id: u32,
    currency: String,
    limit: Option<u32>,
) -> AppResult<PriceHistory> {
    database_read(&state, move |database| {
        database.game_price_history(app_id, &currency, limit.unwrap_or(0))
    })
    .await
}

#[tauri::command]
pub async fn forget_game_prices(state: State<'_, AppState>, app_id: u32) -> AppResult<()> {
    database_read(&state, move |database| database.forget_game_prices(app_id)).await
}

/// Vuelve a preguntar el precio de los deseados cuya observación ha caducado.
///
/// Es la única puerta que habla con la tienda. El ritmo y el respeto al
/// `Retry-After` viven aquí; `db::pricing` sólo persiste lo que se le entrega,
/// que es lo que permite probar el modelo entero sin red.
///
/// Se pregunta **por lotes**. Antes se pedía la ficha completa de un juego por
/// petición, con tres cuartos de segundo entre una y otra: una lista de mil
/// quinientos deseados tardaba casi veinte minutos y en la práctica nadie
/// llegaba a ver un precio. El endpoint de la tienda acepta cien AppID de una
/// vez cuando se le pide sólo el bloque de precio, así que la misma lista son
/// quince peticiones.
#[tauri::command]
pub async fn refresh_wishlist_prices(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> AppResult<PriceRefreshReport> {
    // El cerrojo evita que el botón y la tanda automática se pisen: dos
    // barridos a la vez sólo consiguen que la tienda corte a los dos.
    let _guard = PRICE_REFRESH_LOCK.lock().await;
    let database = state.database.clone();
    let requested = limit.unwrap_or(0);
    steam::prices::refresh(&database, requested).await
}

// --- Vista rápida -------------------------------------------------------------

/// Las capturas que enseña el emergente al pasar el ratón por encima.
///
/// Lee lo guardado y, sólo si nunca se preguntó, le pide a la tienda las
/// miniaturas: la respuesta filtrada pesa menos de un kilobyte. Un juego sin
/// capturas queda marcado como preguntado para no repetir la consulta en cada
/// pasada del ratón.
#[tauri::command]
pub async fn game_preview(
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<crate::db::preview::GamePreview> {
    let database = state.database.clone();
    let guardado = {
        let database = database.clone();
        blocking(move || database.stored_preview(app_id)).await?
    };
    if guardado.checked {
        return Ok(guardado);
    }
    // Un fallo de red no vacía la vista: se devuelve lo que hubiera, sin marcar
    // el juego como preguntado, para volver a intentarlo más tarde.
    let Ok(capturas) = steam::store_api::fetch_screenshots(app_id).await else {
        return Ok(guardado);
    };
    let now = Utc::now();
    blocking(move || database.save_preview(app_id, &capturas, now)).await
}

// --- Regalos de Epic ----------------------------------------------------------

/// Los juegos que Epic regala esta semana, con lo que Vindexa sabe de tu
/// biblioteca encima.
///
/// `refresh` decide si además se le pregunta a Epic. Sin él sólo se devuelve lo
/// guardado, que es lo que quiere la pantalla al abrirse: enseñar algo al
/// instante y no esperar a una petición de red.
#[tauri::command]
pub async fn epic_free_games(
    state: State<'_, AppState>,
    refresh: Option<bool>,
) -> AppResult<Vec<crate::db::epic_free::EpicFreeOffer>> {
    let database = state.database.clone();
    if refresh.unwrap_or(false) {
        // Un fallo de red no deja la pantalla en blanco: se enseña lo último
        // que se supo y el fallo se cuenta aparte.
        if let Ok(juegos) = stores::epic_free::fetch("ES", "es-ES").await {
            let now = Utc::now();
            let database = database.clone();
            blocking(move || database.sync_epic_free_offers(&juegos, now)).await?;
        }
    }
    let now = Utc::now();
    blocking(move || database.epic_free_offers(now)).await
}

/// Descarta un regalo para que deje de aparecer y de avisar.
#[tauri::command]
pub async fn dismiss_epic_free_game(
    state: State<'_, AppState>,
    offer_id: String,
) -> AppResult<()> {
    let database = state.database.clone();
    let now = Utc::now();
    blocking(move || database.dismiss_epic_free_offer(&offer_id, now)).await
}

/// Lleva a la ficha del regalo en el navegador integrado de Epic.
///
/// Vindexa **no** reclama el juego por ti: hacerlo exigiría conducir tu sesión
/// por un flujo de compra. Lo que hace es dejarte en la página exacta, donde ya
/// estás identificado, a un clic de «Obtener».
#[tauri::command]
pub async fn open_epic_free_game(app: AppHandle, url: String) -> AppResult<()> {
    store_window::open_store_url(&app, "epic", &url).await
}

// --- Archivado de biblioteca --------------------------------------------------

#[tauri::command]
pub async fn archive_games(
    state: State<'_, AppState>,
    app_ids: Vec<u32>,
    reason: Option<String>,
) -> AppResult<ArchiveReport> {
    let now = Utc::now();
    let reason = reason.unwrap_or_default();
    database_read(&state, move |database| {
        database.archive_games(&app_ids, &reason, now)
    })
    .await
}

#[tauri::command]
pub async fn unarchive_games(
    state: State<'_, AppState>,
    app_ids: Vec<u32>,
) -> AppResult<ArchiveReport> {
    database_read(&state, move |database| database.unarchive_games(&app_ids)).await
}

#[tauri::command]
pub async fn list_archived_games(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> AppResult<PagedArchivedGames> {
    database_read(&state, move |database| {
        database.archived_games(limit.unwrap_or(0), offset.unwrap_or(0))
    })
    .await
}

#[tauri::command]
pub async fn count_archived_games(state: State<'_, AppState>) -> AppResult<i64> {
    database_read(&state, move |database| database.archived_game_count()).await
}

#[tauri::command]
pub async fn is_game_archived(state: State<'_, AppState>, app_id: u32) -> AppResult<bool> {
    database_read(&state, move |database| database.is_game_archived(app_id)).await
}

/// Importa la lista de deseados de Steam a la de Vindexa.
///
/// Comparte el cerrojo de sincronización con la biblioteca porque comparte el
/// límite de peticiones de Steam. No necesita la clave Web API: el endpoint de
/// deseados sólo pide el SteamID64.
#[tauri::command]
pub async fn import_steam_wishlist(
    state: State<'_, AppState>,
) -> AppResult<SteamWishlistImportResult> {
    let _sync_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;
    let account = database_read(&state, move |database| {
        database.get_steam_account()?.ok_or_else(|| {
            AppError::new(
                "steam_not_linked",
                "Vincula tu cuenta de Steam antes de importar la lista de deseados.",
            )
        })
    })
    .await?;

    // La red queda fuera del cerrojo de mantenimiento, igual que en la
    // sincronización de biblioteca.
    let snapshot = await_steam_network(steam::wishlist::fetch(
        &account.steam_id,
        account.visibility,
    ))
    .await?;
    let titles_unresolved = snapshot.titles_unresolved;
    let visibility_unknown = snapshot.visibility_unknown;
    let games = snapshot
        .items
        .into_iter()
        .map(|item| ImportedWishlistGame {
            app_id: item.app_id,
            title: item.title,
            added_at: item.added_at,
        })
        .collect::<Vec<_>>();

    let report = database_write(&state, move |database| {
        database.import_steam_wishlist(&games)
    })
    .await?;

    Ok(SteamWishlistImportResult {
        report,
        titles_unresolved,
        visibility_unknown,
    })
}

/// Importa la lista de deseados leyéndola de la sesión abierta en el navegador.
///
/// Es la salida cuando el perfil de Steam no es público: `import_steam_wishlist`
/// pregunta desde fuera y Steam calla. Aquí no se pregunta desde fuera, se abre
/// la lista dentro del navegador integrado —donde Steam ya se la ha renderizado
/// a quien ha iniciado sesión— y se lee lo que hay en esa página.
///
/// El cerrojo de sincronización se toma **después** de leer la página. Lo que
/// protege es el límite de peticiones a Steam, y la única red de este comando
/// —resolver los nombres— llega al final; tomarlo antes dejaría la biblioteca
/// bloqueada durante todo el rato que alguien tarde en iniciar sesión.
#[tauri::command]
pub async fn import_steam_wishlist_from_browser(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<steam::wishlist_session::BrowserWishlistImportResult> {
    let linked = database_read(&state, move |database| database.get_steam_account()).await?;
    let mut snapshot = steam::wishlist_session::read_wishlist(&app).await?;
    steam::wishlist_session::ensure_same_account(
        linked.as_ref().map(|account| account.steam_id.as_str()),
        &snapshot.steam_id,
    )?;

    let _sync_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;
    await_steam_network(steam::wishlist::resolve_store_titles(&mut snapshot.items)).await?;

    let titles_unresolved = snapshot
        .items
        .iter()
        .filter(|item| item.title.is_none())
        .count();
    let steam_id = snapshot.steam_id.clone();
    let hidden_by_filters = snapshot.hidden_by_filters;
    let games = steam::wishlist_session::to_imported_games(&snapshot.items);

    let report = database_write(&state, move |database| {
        database.import_steam_wishlist(&games)
    })
    .await?;

    Ok(steam::wishlist_session::BrowserWishlistImportResult {
        report,
        steam_id,
        titles_unresolved,
        hidden_by_filters,
    })
}

// --- Catálogo de Steam Family ------------------------------------------------

/// Estado del vínculo con la sesión de Steam que autoriza los servicios de
/// Familia.
#[tauri::command]
pub async fn steam_family_session_status(
    state: State<'_, AppState>,
) -> AppResult<steam::family_api::FamilySessionStatus> {
    let linked = steam::secrets::has_session_token()?;
    let (last_sync_at, last_app_count, last_error_code) = database_read(&state, move |database| {
        database.family_session_diagnostics()
    })
    .await?;
    Ok(steam::family_api::FamilySessionStatus {
        linked,
        last_sync_at,
        last_app_count,
        last_error_code,
    })
}

/// Toma el testigo de la sesión abierta en el navegador integrado y lo guarda.
///
/// No sincroniza nada: vincular y traer el catálogo son dos decisiones, y
/// juntarlas dejaría a quien sólo quiere vincular esperando una descarga larga.
#[tauri::command]
pub async fn link_steam_family_session(
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<steam::family_api::FamilySessionStatus> {
    let token = steam::family_session::read_session_token(&app).await?;
    steam::secrets::save_session_token(token.as_str())?;
    steam_family_session_status(state).await
}

/// Olvida el testigo. El catálogo ya importado se queda: es un dato válido que
/// se obtuvo legítimamente, y borrarlo al desvincular castigaría por cerrar
/// sesión.
#[tauri::command]
pub async fn unlink_steam_family_session(
    state: State<'_, AppState>,
) -> AppResult<steam::family_api::FamilySessionStatus> {
    steam::secrets::delete_session_token()?;
    steam_family_session_status(state).await
}

/// Anota el fallo de una sincronización de Familia y devuelve el error tal cual.
///
/// Se anota aunque la operación termine mal: perder el diagnóstico dejaría la
/// pantalla diciendo que la última sincronización fue bien. Y un testigo
/// caducado se borra, para que la pantalla pida vincular en lugar de ofrecer
/// una sincronización que va a volver a fallar.
async fn anotar_fallo_de_familia(state: &State<'_, AppState>, error: AppError) -> AppError {
    let code = error.code.clone();
    let _ = database_read(state, move |database| {
        database.record_family_session_failure(&code)
    })
    .await;
    if error.code == "steam_family_session_expired" {
        let _ = steam::secrets::delete_session_token();
    }
    error
}

/// Trae el catálogo completo de la Familia con el testigo guardado.
///
/// Esta es la vía que ve **todo** el catálogo. La que había —preguntar por cada
/// miembro con la Web API Key— sólo devolvía lo de quien tuviera la biblioteca
/// pública, y por eso faltaban miles de juegos que el cliente de Steam sí
/// enseña. Aquella vía se conserva dentro de la sincronización normal: lo que
/// aporte, aporta.
#[tauri::command]
pub async fn sync_steam_family_catalog(
    state: State<'_, AppState>,
) -> AppResult<steam::family_api::FamilySyncReport> {
    let Some(raw_token) = steam::secrets::load_session_token()? else {
        return Err(AppError::new(
            "steam_family_not_linked",
            "Vindexa no tiene una sesión de Steam vinculada. Vincúlala en Ajustes y vuelve a intentarlo.",
        ));
    };
    let account = database_read(&state, move |database| database.get_steam_account()).await?;
    let Some(account) = account else {
        return Err(AppError::new(
            "steam_family_no_account",
            "Todavía no has vinculado una cuenta de Steam, así que no se sabe de quién es la Familia.",
        ));
    };

    // Se vuelve a validar lo guardado en vez de confiar en que sigue bien: el
    // llavero es un almacén compartido y una entrada puede haberse tocado fuera.
    let token = steam::family_session::SessionToken::parse(&raw_token)?;

    let _sync_guard = try_begin_steam_sync(state.steam_sync_lock.clone())?;

    let group = match steam::family_api::fetch_family_group(&token, &account.steam_id).await {
        Ok(group) => group,
        Err(error) => return Err(anotar_fallo_de_familia(&state, error).await),
    };

    let group_id = match group {
        steam::family_api::FamilyGroup::Member { group_id } => group_id,
        steam::family_api::FamilyGroup::None => {
            let moment = Utc::now().to_rfc3339();
            database_read(&state, move |database| {
                database.record_family_session_success(&moment, 0)
            })
            .await?;
            return Ok(steam::family_api::FamilySyncReport {
                no_family: true,
                ..Default::default()
            });
        }
    };

    let library = match steam::family_api::fetch_shared_library(&token, &group_id).await {
        Ok(library) => library,
        Err(error) => return Err(anotar_fallo_de_familia(&state, error).await),
    };

    // Sin título no se puede presentar una ficha honesta, así que esas entradas
    // se cuentan y no se guardan. Inventar el nombre a partir del AppID sería
    // exactamente lo que este proyecto no hace.
    let without_title = library
        .apps
        .iter()
        .filter(|app| app.title.is_none())
        .count() as u32;
    let games: Vec<crate::db::ImportedFamilyCatalogGame> = library
        .apps
        .iter()
        .filter_map(|app| {
            app.title
                .as_ref()
                .map(|title| crate::db::ImportedFamilyCatalogGame {
                    app_id: app.app_id,
                    title: title.clone(),
                    icon_url: None,
                    cover_url: None,
                    header_url: None,
                    // Catálogo visible no es licencia: sólo la evidencia local
                    // confirma que se puede jugar.
                    availability: "unknown".to_string(),
                })
        })
        .collect();

    let imported = games.len() as u32;
    let unusable = library.unusable as u32;
    database_write(&state, move |database| {
        // Es una instantánea completa: lo que ya no está en la Familia deja de
        // estar en el catálogo.
        database.save_family_catalog(&games, true)
    })
    .await?;

    let moment = Utc::now().to_rfc3339();
    database_read(&state, move |database| {
        database.record_family_session_success(&moment, imported)
    })
    .await?;

    Ok(steam::family_api::FamilySyncReport {
        imported,
        unusable,
        without_title,
        no_family: false,
    })
}

// --- Vistas guardadas de biblioteca ------------------------------------------

#[tauri::command]
pub async fn list_saved_views(state: State<'_, AppState>) -> AppResult<Vec<SavedView>> {
    database_read(&state, move |database| database.list_saved_views()).await
}

#[tauri::command]
pub async fn save_saved_view(
    state: State<'_, AppState>,
    input: SaveViewInput,
) -> AppResult<SavedView> {
    database_read(&state, move |database| database.save_saved_view(&input)).await
}

#[tauri::command]
pub async fn delete_saved_view(state: State<'_, AppState>, view_id: String) -> AppResult<()> {
    database_read(&state, move |database| database.delete_saved_view(&view_id)).await
}

#[tauri::command]
pub async fn reorder_saved_views(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.reorder_saved_views(&ordered_ids)
    })
    .await
}

#[tauri::command]
pub async fn mark_saved_view_used(
    state: State<'_, AppState>,
    view_id: String,
) -> AppResult<SavedView> {
    database_read(&state, move |database| {
        database.mark_saved_view_used(&view_id)
    })
    .await
}

// --- Listas curadas ---------------------------------------------------------

#[tauri::command]
pub async fn list_curated_lists(state: State<'_, AppState>) -> AppResult<Vec<CuratedList>> {
    database_read(&state, move |database| database.list_curated_lists()).await
}

#[tauri::command]
pub async fn save_curated_list(
    state: State<'_, AppState>,
    input: SaveCuratedListInput,
) -> AppResult<CuratedList> {
    database_read(&state, move |database| database.save_curated_list(&input)).await
}

#[tauri::command]
pub async fn delete_curated_list(state: State<'_, AppState>, list_id: String) -> AppResult<()> {
    database_read(&state, move |database| {
        database.delete_curated_list(&list_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_curated_lists(
    state: State<'_, AppState>,
    ordered_ids: Vec<String>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.reorder_curated_lists(&ordered_ids)
    })
    .await
}

#[tauri::command]
pub async fn get_curated_list_detail(
    state: State<'_, AppState>,
    list_id: String,
    limit: Option<u32>,
    offset: Option<u32>,
) -> AppResult<CuratedListDetail> {
    database_read(&state, move |database| {
        database.curated_list_detail(&list_id, limit, offset)
    })
    .await
}

#[tauri::command]
pub async fn add_curated_game(
    state: State<'_, AppState>,
    input: AddCuratedGameInput,
) -> AppResult<()> {
    database_read(&state, move |database| database.add_curated_game(&input)).await
}

#[tauri::command]
pub async fn update_curated_item(
    state: State<'_, AppState>,
    input: UpdateCuratedItemInput,
) -> AppResult<()> {
    database_read(&state, move |database| database.update_curated_item(&input)).await
}

#[tauri::command]
pub async fn remove_curated_game(
    state: State<'_, AppState>,
    list_id: String,
    app_id: u32,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.remove_curated_game(&list_id, app_id)
    })
    .await
}

#[tauri::command]
pub async fn move_curated_item(
    state: State<'_, AppState>,
    list_id: String,
    app_id: u32,
    before_app_id: Option<u32>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.move_curated_item(&list_id, app_id, before_app_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_curated_items(
    state: State<'_, AppState>,
    list_id: String,
    ordered_app_ids: Vec<u32>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.reorder_curated_items(&list_id, &ordered_app_ids)
    })
    .await
}

// --- Deseados y vídeos ------------------------------------------------------

#[tauri::command]
pub async fn get_wishlist_overview(state: State<'_, AppState>) -> AppResult<WishlistOverview> {
    database_read(&state, move |database| database.wishlist_overview()).await
}

#[tauri::command]
pub async fn save_wishlist_entry(
    state: State<'_, AppState>,
    input: SaveWishlistEntryInput,
) -> AppResult<WishlistEntry> {
    database_read(&state, move |database| database.save_wishlist_entry(&input)).await
}

#[tauri::command]
pub async fn remove_wishlist_entry(state: State<'_, AppState>, app_id: u32) -> AppResult<()> {
    database_read(&state, move |database| {
        database.remove_wishlist_entry(app_id)
    })
    .await
}

#[tauri::command]
pub async fn move_wishlist_entry(
    state: State<'_, AppState>,
    app_id: u32,
    bucket: String,
    before_app_id: Option<u32>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.move_wishlist_entry(app_id, &bucket, before_app_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_wishlist_bucket(
    state: State<'_, AppState>,
    bucket: String,
    ordered_app_ids: Vec<u32>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.reorder_wishlist_bucket(&bucket, &ordered_app_ids)
    })
    .await
}

#[tauri::command]
pub async fn list_game_videos(
    state: State<'_, AppState>,
    app_id: u32,
    kind: Option<String>,
) -> AppResult<Vec<GameVideo>> {
    database_read(&state, move |database| {
        database.list_game_videos(app_id, kind.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn save_game_video(
    state: State<'_, AppState>,
    input: SaveGameVideoInput,
) -> AppResult<GameVideo> {
    database_read(&state, move |database| database.save_game_video(&input)).await
}

#[tauri::command]
pub async fn delete_game_video(
    state: State<'_, AppState>,
    app_id: u32,
    provider: String,
    video_id: String,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.delete_game_video(app_id, &provider, &video_id)
    })
    .await
}

#[tauri::command]
pub async fn reorder_game_videos(
    state: State<'_, AppState>,
    app_id: u32,
    kind: String,
    ordered: Vec<GameVideoRef>,
) -> AppResult<()> {
    database_read(&state, move |database| {
        database.reorder_game_videos(app_id, &kind, &ordered)
    })
    .await
}

/// Todas las carátulas que la biblioteca puede llegar a enseñar.
///
/// La interfaz adelanta las de la página cargada; con esta lista puede además
/// ir completando el resto mientras está ociosa, para que la biblioteca deje de
/// depender de la red al desplazarse. Devuelve dos columnas por juego porque se
/// pide entera.
#[tauri::command]
pub async fn list_artwork_targets(
    state: State<'_, AppState>,
) -> AppResult<Vec<crate::db::ArtworkTarget>> {
    database_read(&state, move |database| database.artwork_targets()).await
}

/// Cuánto ocupa la caché de arte y cuánto se le permite ocupar.
#[tauri::command]
pub async fn get_art_cache_usage(state: State<'_, AppState>) -> AppResult<art_cache::CacheUsage> {
    Ok(art_cache::usage(&state.cache_dir))
}

#[tauri::command]
pub async fn maintain_art_cache(
    state: State<'_, AppState>,
) -> AppResult<art_cache::MaintenanceReport> {
    let cache_dir = state.cache_dir.clone();
    database_read(&state, move |database| {
        art_cache::maintain(&database, &cache_dir, &[])
    })
    .await
}

#[tauri::command]
pub async fn get_rich_game_metadata(
    state: State<'_, AppState>,
    app_id: u32,
) -> AppResult<RichGameMetadata> {
    database_read(&state, move |database| database.rich_game_metadata(app_id)).await
}

#[tauri::command]
pub async fn get_drm_state_counts(state: State<'_, AppState>) -> AppResult<DrmStateCounts> {
    database_read(&state, move |database| database.drm_state_counts()).await
}

#[tauri::command]
pub async fn list_game_dlc(
    state: State<'_, AppState>,
    app_id: u32,
    filter: Option<String>,
) -> AppResult<Vec<GameDlc>> {
    let filter = DlcFilter::parse(filter.as_deref())?;
    database_read(&state, move |database| {
        database.list_game_dlc(app_id, filter)
    })
    .await
}

#[tauri::command]
pub async fn get_dlc_summary(state: State<'_, AppState>, app_id: u32) -> AppResult<DlcSummary> {
    database_read(&state, move |database| database.game_dlc_summary(app_id)).await
}

#[tauri::command]
pub async fn set_dlc_owned(
    state: State<'_, AppState>,
    app_id: u32,
    dlc_app_id: u32,
    owned: bool,
) -> AppResult<GameDlc> {
    database_read(&state, move |database| {
        database.set_game_dlc_owned(app_id, dlc_app_id, owned)
    })
    .await
}

#[tauri::command]
pub async fn set_dlc_hidden(
    state: State<'_, AppState>,
    app_id: u32,
    dlc_app_id: u32,
    hidden: bool,
) -> AppResult<GameDlc> {
    database_read(&state, move |database| {
        database.set_game_dlc_hidden(app_id, dlc_app_id, hidden)
    })
    .await
}

#[tauri::command]
pub async fn set_dlc_installed(
    state: State<'_, AppState>,
    app_id: u32,
    dlc_app_id: u32,
    installed: bool,
) -> AppResult<GameDlc> {
    database_read(&state, move |database| {
        database.set_game_dlc_installed(app_id, dlc_app_id, installed)
    })
    .await
}

#[tauri::command]
pub async fn refresh_game_dlc(
    state: State<'_, AppState>,
    app_id: u32,
    detail_budget: Option<usize>,
) -> AppResult<DlcRefreshReport> {
    // El guardián se sostiene desde antes de la petición hasta después del
    // último commit: una restauración nunca puede cruzarse con una respuesta
    // antigua de la tienda.
    let _maintenance = state.maintenance.read().await;
    let database = state.database.clone();

    let evidence = blocking(move || Ok(steam::dlc::scan_installed_dlc(app_id))).await?;
    let catalog = steam::dlc::fetch_catalog(app_id)
        .await
        .map_err(|failure| failure.error)?;
    let mut items = catalog.items;
    steam::dlc::apply_local_evidence(&mut items, &evidence);

    let imported = {
        let database = database.clone();
        blocking(move || database.save_game_dlc(app_id, &items)).await?
    };

    let budget = detail_budget
        .unwrap_or(DLC_DETAIL_BUDGET)
        .min(DLC_DETAIL_BUDGET);
    let mut fetched_details = 0_usize;
    let mut unavailable_details = 0_usize;
    let mut failed_details = 0_usize;
    if budget > 0 {
        let candidates = {
            let database = database.clone();
            blocking(move || database.claim_game_dlc_refresh(app_id, budget)).await?
        };
        for candidate in candidates {
            match steam::dlc::fetch_detail(app_id, candidate.dlc_app_id, candidate.position).await {
                Ok(steam::dlc::DlcDetailOutcome::Found(detail)) => {
                    let mut detail = vec![*detail];
                    steam::dlc::apply_local_evidence(&mut detail, &evidence);
                    let database = database.clone();
                    blocking(move || database.save_game_dlc(app_id, &detail)).await?;
                    fetched_details += 1;
                }
                Ok(steam::dlc::DlcDetailOutcome::Unavailable) => {
                    let missing = vec![ImportedDlc::unavailable(
                        candidate.dlc_app_id,
                        candidate.position,
                    )];
                    let database = database.clone();
                    blocking(move || database.save_game_dlc(app_id, &missing)).await?;
                    unavailable_details += 1;
                }
                Err(failure) => {
                    // Un fallo transitorio se deja arrendado: la cola lo reintenta
                    // sola al vencer. Uno de contrato se marca como fallido.
                    if steam::dlc::retry_delay_seconds(&failure.error.code, 1, failure.retry_after)
                        .is_none()
                    {
                        let database = database.clone();
                        let dlc_app_id = candidate.dlc_app_id;
                        blocking(move || database.mark_game_dlc_failed(app_id, dlc_app_id)).await?;
                    }
                    failed_details += 1;
                }
            }
        }
    }

    let summary = {
        let database = database.clone();
        blocking(move || database.game_dlc_summary(app_id)).await?
    };
    let pending_details = blocking(move || {
        Ok(database
            .list_game_dlc(app_id, DlcFilter::All)?
            .iter()
            .filter(|dlc| dlc.metadata_status != "success")
            .count())
    })
    .await?;

    Ok(DlcRefreshReport {
        app_id,
        declared: catalog.declared,
        truncated: catalog.truncated,
        fetched_details,
        unavailable_details,
        failed_details,
        pending_details,
        ownership_evidence_gap: evidence.gap_code().map(str::to_owned),
        ownership_evidence_explanation: evidence.gap_explanation().map(str::to_owned),
        imported,
        summary,
    })
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

/// Abre la portada de una tienda en el navegador integrado.
///
/// Cada tienda tiene su propio almacén de datos aislado, así que la ventana se
/// abre **con la sesión que ya tengas iniciada en ella** y sin ver las cookies
/// de las demás ni las de la aplicación. La allowlist de destinos y el filtro
/// de contenido son los mismos que cuando se abre la ficha de un juego: esto no
/// añade superficie, sólo un punto de entrada.
#[tauri::command]
pub async fn open_store_browser(app: AppHandle, store_id: String) -> AppResult<()> {
    store_window::open_store_home(&app, &store_id).await
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
    // Sin maintenance: sólo lee games/family y escribe en image_cache, ambas
    // operaciones pequeñas y serializadas por SQLite (WAL + busy_timeout).
    // Bloquearla durante una sincronización dejaba todas las carátulas en
    // espera hasta que el sync terminaba.
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
    // El techo de la caché se aplicaba sólo al arrancar, así que cambiarlo aquí
    // no hacía nada hasta reiniciar. Se aplica en cuanto se guarda, y sólo si
    // se guarda: un valor rechazado por la validación no debe llegar a mandar.
    let art_cache_mib = preferences.art_cache_mib;
    database_read(&state, move |database| {
        database.save_preferences(&preferences)
    })
    .await?;
    art_cache::set_max_cache_bytes(u64::from(art_cache_mib) * 1024 * 1024);
    Ok(())
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> UpdateCheckResult {
    let current_version = app.package_info().version.to_string();
    match updates::latest_published_version().await {
        Ok(Some(latest)) => match updates::compare(&current_version, &latest) {
            Some(std::cmp::Ordering::Less) => UpdateCheckResult {
                status: "available".into(),
                current_version,
                available_version: Some(latest.clone()),
                release_page: updates::RELEASES_PAGE.to_string(),
                message: format!(
                    "Hay una versión nueva: {latest}. Vindexa no la descarga ni la instala: ábrela en la página de versiones y decide tú."
                ),
            },
            Some(_) => UpdateCheckResult {
                status: "upToDate".into(),
                current_version: current_version.clone(),
                available_version: Some(latest),
                release_page: updates::RELEASES_PAGE.to_string(),
                message: format!(
                    "Estás en la versión {current_version}, que es la última publicada."
                ),
            },
            // Una de las dos versiones no tiene la forma esperada. Decirlo es
            // más útil que elegir una de las dos respuestas al azar.
            None => UpdateCheckResult {
                status: "unknown".into(),
                current_version,
                available_version: Some(latest),
                release_page: updates::RELEASES_PAGE.to_string(),
                message: "No se ha podido comparar tu versión con la última publicada.".into(),
            },
        },
        // Sin ninguna versión publicada todavía no hay con qué comparar. No es
        // un fallo: es el estado de un proyecto recién abierto.
        Ok(None) => UpdateCheckResult {
            status: "unknown".into(),
            current_version,
            available_version: None,
            release_page: updates::RELEASES_PAGE.to_string(),
            message: "Todavía no hay ninguna versión publicada con la que comparar.".into(),
        },
        Err(error) => UpdateCheckResult {
            status: "unreachable".into(),
            current_version,
            available_version: None,
            release_page: updates::RELEASES_PAGE.to_string(),
            message: error.message,
        },
    }
}

// ---------------------------------------------------------------------------
// Puente para agentes externos
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn agent_dispatch(
    state: State<'_, AppState>,
    request: AgentRequest,
) -> AppResult<AgentOutcome> {
    database_read(&state, move |database| {
        agent::bridge::dispatch(&mut database.open()?, agent::ratelimit::shared(), &request)
    })
    .await
}

#[tauri::command]
pub async fn agent_confirm(
    state: State<'_, AppState>,
    audit_id: String,
    approve: bool,
) -> AppResult<AgentOutcome> {
    database_read(&state, move |database| {
        agent::bridge::confirm(&mut database.open()?, &audit_id, approve)
    })
    .await
}

#[tauri::command]
pub async fn agent_undo(state: State<'_, AppState>, undo_token: String) -> AppResult<AgentOutcome> {
    database_read(&state, move |database| {
        agent::bridge::undo(&mut database.open()?, &undo_token, &Requester::Human)
    })
    .await
}

#[tauri::command]
pub async fn agent_undo_as_client(
    state: State<'_, AppState>,
    token: String,
    undo_token: String,
) -> AppResult<AgentOutcome> {
    database_read(&state, move |database| {
        agent::bridge::undo_as_client(
            &mut database.open()?,
            agent::ratelimit::shared(),
            &token,
            &undo_token,
        )
    })
    .await
}

#[tauri::command]
pub async fn issue_agent_client(
    state: State<'_, AppState>,
    input: NewAgentClient,
) -> AppResult<IssuedAgentClient> {
    database_read(&state, move |database| {
        agent::clients::issue(&mut database.open()?, &input, agent::TokenPolicy::default())
    })
    .await
}

/// Qué agentes compatibles hay en este ordenador.
///
/// Es sólo lectura: mira si el ejecutable existe y monta el comando que se
/// usaría. No conecta nada ni ejecuta nada.
#[tauri::command]
pub async fn detect_agent_hosts() -> AppResult<Vec<agent::hosts::AgentHost>> {
    agent::hosts::detect()
}

/// Qué hay en esta máquina para conducir Vindexa hablando sin depender de nadie:
/// motores instalados, modelos ya descargados y qué le cabe al ordenador.
///
/// Es sólo lectura y no toca la red: mira el disco y pregunta al sistema.
#[tauri::command]
pub async fn local_model_survey() -> AppResult<LocalModelSurvey> {
    // El rastreo toca disco, así que sale del hilo de la interfaz. Los
    // servidores se preguntan por red, así que van por otro lado.
    let disco = tauri::async_runtime::spawn_blocking(|| {
        (
            localmodel::runtimes(),
            localmodel::scan_models(),
            localmodel::hardware(),
        )
    });
    let endpoints = localmodel::endpoints::discover().await;
    let (runtimes, models, hardware) = disco
        .await
        .map_err(|error| AppError::new("local_model", format!("El rastreo falló: {error}")))?;
    Ok(LocalModelSurvey {
        runtimes,
        models,
        hardware,
        endpoints,
    })
}

/// Qué modelo proponerle a esta máquina, preguntado a Hugging Face en el
/// momento. Una lista escrita a mano envejecería en semanas y acabaría
/// recomendando repositorios que ya no existen.
#[tauri::command]
pub async fn suggest_local_models(
    usable_bytes: Option<u64>,
) -> AppResult<localmodel::catalog::CatalogSuggestions> {
    localmodel::catalog::suggest(usable_bytes).await
}

/// Qué haría falta para tener llama.cpp en esta máquina. No instala nada: dice
/// con qué gestor de paquetes se haría y cuál sería la orden exacta.
#[tauri::command]
pub async fn local_model_install_plan() -> AppResult<localmodel::install::InstallPlan> {
    Ok(localmodel::install::plan())
}

/// Instala llama.cpp con el gestor de paquetes del sistema.
///
/// Sólo se llama cuando alguien lo pide viendo antes la orden: instalar un
/// paquete toca el sistema entero, no sólo Vindexa.
#[tauri::command]
pub async fn install_local_runtime() -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(localmodel::install::run)
        .await
        .map_err(|error| {
            AppError::new(
                "local_model_install",
                format!("La instalación no pudo lanzarse: {error}"),
            )
        })?
}

/// Con qué modelo habla el agente de casa. Vacío significa «el que se detecte».
#[tauri::command]
pub async fn vindagent_config(
    state: State<'_, AppState>,
) -> AppResult<vindagent::config::ModelConfig> {
    database_read(&state, move |database| vindagent::config::load(&database)).await
}

/// Guarda con qué modelo hablar. Apuntar fuera de este ordenador exige haberlo
/// marcado: escribir mal una dirección no puede acabar mandando los títulos de
/// una biblioteca a un servicio ajeno.
#[tauri::command]
pub async fn save_vindagent_config(
    state: State<'_, AppState>,
    input: vindagent::config::SaveModelConfig,
) -> AppResult<vindagent::config::ModelConfig> {
    database_read(&state, move |database| {
        vindagent::config::save(&database, &input)
    })
    .await
}

/// Encargos que el agente repite solo.
#[tauri::command]
pub async fn list_agent_tasks(
    state: State<'_, AppState>,
) -> AppResult<Vec<vindagent::schedule::ScheduledTask>> {
    database_read(&state, move |database| {
        vindagent::schedule::list(&database)
    })
    .await
}

/// Crea o edita un encargo.
#[tauri::command]
pub async fn save_agent_task(
    state: State<'_, AppState>,
    input: vindagent::schedule::SaveScheduledTask,
) -> AppResult<vindagent::schedule::ScheduledTask> {
    database_read(&state, move |database| {
        vindagent::schedule::save(&database, &input)
    })
    .await
}

/// Borra un encargo.
#[tauri::command]
pub async fn delete_agent_task(state: State<'_, AppState>, task_id: String) -> AppResult<()> {
    database_read(&state, move |database| {
        vindagent::schedule::delete(&database, &task_id)
    })
    .await
}

/// ¿Hay un transcriptor en este ordenador con el que dictar?
///
/// `None` no es un error: es que no lo hay, y entonces el botón de dictar ni
/// siquiera aparece. Ofrecerlo para que falle sería peor.
#[tauri::command]
pub async fn speech_endpoint() -> AppResult<Option<localmodel::speech::SpeechEndpoint>> {
    Ok(localmodel::speech::discover().await)
}

/// Convierte un dictado en texto, contra un transcriptor local.
///
/// El audio llega como bytes desde la ventana y no se guarda en ninguna parte:
/// se manda, se recibe el texto y se olvida.
#[tauri::command]
pub async fn transcribe_dictation(
    base_url: String,
    audio: Vec<u8>,
    mime: String,
) -> AppResult<String> {
    localmodel::speech::transcribe(&base_url, audio, &mime).await
}

/// Un turno de conversación con el agente que vive dentro de Vindexa.
///
/// Devuelve la respuesta y, por separado, lo que haya hecho por el camino: un
/// agente que ordena tu biblioteca sin contar qué tocó no es de fiar.
#[tauri::command]
pub async fn vindagent_chat(
    state: State<'_, AppState>,
    base_url: String,
    model: String,
    history: Vec<vindagent::ChatMessage>,
) -> AppResult<vindagent::ChatTurn> {
    let database = state.database.clone();
    vindagent::chat(&database, &base_url, &model, &history).await
}

/// Qué agentes tiene Vindexa conectados ahora mismo, y si el automatismo está
/// encendido. Es lo que enseña Ajustes → Agentes.
#[tauri::command]
pub async fn agent_autolink_state(
    state: State<'_, AppState>,
) -> AppResult<agent::autolink::AutolinkStatus> {
    database_read(&state, move |database| {
        let connection = database.open()?;
        Ok(agent::autolink::AutolinkStatus {
            disabled: agent::autolink::disabled(&connection),
            links: agent::autolink::state(&connection),
            hosts: agent::hosts::detect()?,
        })
    })
    .await
}

/// Enciende o apaga el enlace automático con agentes.
#[tauri::command]
pub async fn set_agent_autolink_disabled(
    state: State<'_, AppState>,
    disabled: bool,
) -> AppResult<()> {
    database_read(&state, move |database| {
        agent::autolink::set_disabled(&database.open()?, disabled)
    })
    .await
}

/// Emite un testigo y da de alta Vindexa en el agente indicado.
///
/// Las dos cosas van juntas a propósito: un testigo emitido y no entregado no
/// sirve para nada, y entregarlo a mano obliga a copiar un secreto por pantalla.
/// El testigo no vuelve a la interfaz: va directo al proceso del agente.
#[tauri::command]
pub async fn connect_agent_host(
    state: State<'_, AppState>,
    host_id: String,
    input: NewAgentClient,
) -> AppResult<String> {
    let issued = database_read(&state, move |database| {
        agent::clients::issue(&mut database.open()?, &input, agent::TokenPolicy::default())
    })
    .await?;
    agent::hosts::connect(&host_id, &issued.token)
}

#[tauri::command]
pub async fn rotate_agent_token(
    state: State<'_, AppState>,
    client_id: String,
) -> AppResult<IssuedAgentClient> {
    database_read(&state, move |database| {
        agent::clients::rotate(
            &mut database.open()?,
            &client_id,
            agent::TokenPolicy::default(),
        )
    })
    .await
}

#[tauri::command]
pub async fn set_agent_client_scopes(
    state: State<'_, AppState>,
    client_id: String,
    scopes: Vec<String>,
) -> AppResult<AgentClientSummary> {
    database_read(&state, move |database| {
        agent::clients::set_scopes(&mut database.open()?, &client_id, &scopes)
    })
    .await
}

#[tauri::command]
pub async fn set_agent_client_enabled(
    state: State<'_, AppState>,
    client_id: String,
    enabled: bool,
) -> AppResult<AgentClientSummary> {
    database_read(&state, move |database| {
        agent::clients::set_enabled(&mut database.open()?, &client_id, enabled)
    })
    .await
}

#[tauri::command]
pub async fn revoke_agent_client(state: State<'_, AppState>, client_id: String) -> AppResult<()> {
    database_read(&state, move |database| {
        agent::clients::revoke(&mut database.open()?, &client_id)?;
        agent::ratelimit::shared().forget(&client_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn list_agent_clients(state: State<'_, AppState>) -> AppResult<Vec<AgentClientSummary>> {
    database_read(&state, move |database| {
        agent::clients::list(&database.open()?)
    })
    .await
}

#[tauri::command]
pub async fn list_agent_audit(
    state: State<'_, AppState>,
    limit: u32,
) -> AppResult<Vec<AgentAuditEntry>> {
    database_read(&state, move |database| {
        agent::audit::list(&database.open()?, limit)
    })
    .await
}

// --- Tiendas externas (Epic Games Store y GOG) ------------------------------

#[tauri::command]
pub async fn detect_external_stores() -> AppResult<Vec<StoreDetection>> {
    // Sólo mira el disco local: no necesita la base ni el cerrojo de
    // mantenimiento, pero sí salir del hilo de la interfaz.
    blocking(|| Ok(stores::detect_all())).await
}

#[tauri::command]
pub async fn list_external_store_accounts(
    state: State<'_, AppState>,
) -> AppResult<Vec<ExternalStoreAccount>> {
    database_read(&state, move |database| {
        stores::db::list_accounts(&database.open()?)
    })
    .await
}

#[tauri::command]
pub async fn scan_external_store(
    state: State<'_, AppState>,
    store: String,
) -> AppResult<ExternalStoreScanReport> {
    let store = ExternalStore::parse(&store)?;
    database_read(&state, move |database| {
        stores::scan_and_persist(&mut database.open()?, store)
    })
    .await
}

#[tauri::command]
pub async fn scan_external_stores(
    state: State<'_, AppState>,
) -> AppResult<Vec<ExternalStoreScanReport>> {
    database_read(&state, move |database| {
        stores::scan_all(&mut database.open()?)
    })
    .await
}

#[tauri::command]
pub async fn list_external_games(
    state: State<'_, AppState>,
    request: ExternalGameRequest,
) -> AppResult<PagedExternalGames> {
    database_read(&state, move |database| {
        stores::db::list(&database.open()?, &request)
    })
    .await
}

#[tauri::command]
pub async fn set_external_game_match(
    state: State<'_, AppState>,
    store: String,
    external_id: String,
    app_id: Option<u32>,
) -> AppResult<ExternalGame> {
    let store = ExternalStore::parse(&store)?;
    database_read(&state, move |database| {
        stores::db::set_manual_match(&database.open()?, store, &external_id, app_id)
    })
    .await
}

#[tauri::command]
pub async fn clear_external_game_match(
    state: State<'_, AppState>,
    store: String,
    external_id: String,
) -> AppResult<ExternalGame> {
    let store = ExternalStore::parse(&store)?;
    database_read(&state, move |database| {
        stores::db::clear_manual_match(&database.open()?, store, &external_id)
    })
    .await
}

#[tauri::command]
pub async fn link_external_store(
    state: State<'_, AppState>,
    store: String,
) -> AppResult<ExternalStoreAccount> {
    let store = ExternalStore::parse(&store)?;
    database_read(&state, move |database| {
        stores::db::link(&database.open()?, store)
    })
    .await
}

#[tauri::command]
pub async fn unlink_external_store(state: State<'_, AppState>, store: String) -> AppResult<()> {
    let store = ExternalStore::parse(&store)?;
    database_read(&state, move |database| {
        stores::db::unlink(&mut database.open()?, store)
    })
    .await
}

// ---------------------------------------------------------------------------
// itch.io
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn itch_session_state(
    state: State<'_, AppState>,
) -> AppResult<stores::itch::ItchSessionState> {
    database_read(&state, move |database| {
        stores::itch::session_state(&database.open()?)
    })
    .await
}

/// Comprueba la clave contra itch.io **antes** de guardarla, para que una
/// equivocada no llegue nunca al llavero y el error salga al pegarla.
#[tauri::command]
pub async fn save_itch_api_key(key: String) -> AppResult<stores::itch::ItchAccountProfile> {
    let profile = stores::itch::verify_key(&key).await?;
    stores::itch::secrets::save_api_key(&key)?;
    Ok(profile)
}

/// La red queda fuera del cerrojo de la base, igual que en la sincronización de
/// Steam. Un fallo se anota en la cuenta para poder decir qué pasó.
#[tauri::command]
pub async fn import_itch_library(
    state: State<'_, AppState>,
) -> AppResult<stores::itch::ItchImportReport> {
    let fetch = match stores::itch::fetch_library().await {
        Ok(fetch) => fetch,
        Err(error) => {
            let anotado = error.clone();
            database_read(&state, move |database| {
                stores::itch::record_failure(&mut database.open()?, &anotado)
            })
            .await?;
            return Err(error);
        }
    };
    database_read(&state, move |database| {
        stores::itch::persist_library(&mut database.open()?, &fetch)
    })
    .await
}

#[tauri::command]
pub async fn sign_out_itch(state: State<'_, AppState>) -> AppResult<()> {
    database_read(&state, move |database| {
        stores::itch::sign_out(&mut database.open()?)
    })
    .await
}

/// Destructivo y explícito: la interfaz lo ofrece como acción aparte, nunca
/// como efecto secundario de cerrar sesión.
#[tauri::command]
pub async fn forget_itch_library(state: State<'_, AppState>) -> AppResult<usize> {
    database_read(&state, move |database| {
        stores::itch::forget(&mut database.open()?)
    })
    .await
}

#[tauri::command]
pub async fn rematch_external_stores(state: State<'_, AppState>) -> AppResult<usize> {
    database_read(&state, move |database| {
        stores::rematch_all(&mut database.open()?)
    })
    .await
}

#[tauri::command]
pub async fn launch_external_game(
    app: AppHandle,
    state: State<'_, AppState>,
    store: String,
    external_id: String,
    action: String,
) -> AppResult<()> {
    let store = ExternalStore::parse(&store)?;
    let action = ExternalGameAction::parse(&action)?;
    let _maintenance = state.maintenance.read().await;
    stores::open_game_action(&app, &state.database, store, &external_id, action)
}

// --- Sesión de cuenta de las tiendas externas ------------------------------

/// Estado de la sesión de cada tienda. **No devuelve ningún secreto**: el
/// testigo vive en el llavero y no cruza a la interfaz (ver `stores::online`).
#[tauri::command]
pub async fn list_external_store_sessions() -> AppResult<Vec<ExternalStoreSession>> {
    // Leer el llavero es una llamada al sistema, no cálculo: fuera del hilo de
    // la interfaz, como el resto de accesos a almacenamiento.
    blocking(stores::online::list_sessions).await
}

/// Abre la página de inicio de sesión de la tienda en el navegador del sistema
/// y devuelve qué hay que traerse de vuelta.
///
/// La abre Rust para que las credenciales se escriban en el navegador de
/// siempre, con su gestor de contraseñas y su doble factor, y la ventana de
/// Vindexa no llegue a ver el formulario.
#[tauri::command]
pub async fn begin_external_store_login(
    app: AppHandle,
    store: String,
) -> AppResult<StoreLoginPrompt> {
    let store = ExternalStore::parse(&store)?;
    stores::online::open_login_page(&app, store)
}

/// Inicia sesión en la tienda dentro de Vindexa, de principio a fin.
///
/// Abre la página de la tienda en el navegador integrado y espera a que la
/// persona se identifique; el código de autorización lo recoge Vindexa de la
/// página de retorno. No hay nada que copiar ni ningún JSON que leer.
///
/// Tarda lo que tarde alguien en escribir su contraseña y su segundo factor, así
/// que la interfaz la trata como una espera larga, cancelable cerrando la
/// ventana.
#[tauri::command]
pub async fn sign_in_external_store(
    app: AppHandle,
    store: String,
) -> AppResult<ExternalStoreSession> {
    let store = ExternalStore::parse(&store)?;
    stores::online::sign_in(&app, store).await
}

/// Canjea el código de autorización y guarda la sesión en el llavero.
///
/// `code` es material sensible de un solo uso: no se registra, no se devuelve y
/// no aparece en ningún mensaje de error.
#[tauri::command]
pub async fn complete_external_store_login(
    store: String,
    code: String,
) -> AppResult<ExternalStoreSession> {
    let store = ExternalStore::parse(&store)?;
    stores::online::complete_login(store, &code).await
}

/// Cierra la sesión: revoca en la tienda cuando se puede y borra el llavero
/// siempre, devolviendo qué se ha podido comprobar.
#[tauri::command]
pub async fn sign_out_external_store(store: String) -> AppResult<StoreSignOutReport> {
    let store = ExternalStore::parse(&store)?;
    stores::online::sign_out(store).await
}

/// Lee la biblioteca completa de la cuenta y la persiste.
///
/// La red va primero y la escritura después, en dos pasos deliberados: la
/// petición puede tardar y no debe retener el cerrojo de mantenimiento mientras
/// alguien inicia sesión o mientras se pagina un catálogo grande.
#[tauri::command]
pub async fn sync_external_store_library(
    state: State<'_, AppState>,
    store: String,
) -> AppResult<ExternalStoreScanReport> {
    let store = ExternalStore::parse(&store)?;
    let scan = stores::online::sync_library(store).await?;
    database_read(&state, move |database| {
        stores::db::persist_scan(&mut database.open()?, &scan)
    })
    .await
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
