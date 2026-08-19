import { invoke } from "@tauri-apps/api/core";
import type { SavedLibraryView, SaveViewInput } from "@/features/library/library-views";
import type {
  AgentAuditEntry,
  AgentAutolinkStatus,
  AgentClientSummary,
  AgentModelConfig,
  AgentOutcome,
  AgentRequest,
  AgentScope,
  CatalogSuggestions,
  ChatMessage,
  ChatTurn,
  InstallPlan,
  IssuedAgentClient,
  LocalModelSurvey,
  NewAgentClient,
  SaveAgentModelConfig,
  SaveScheduledTask,
  ScheduledTask,
} from "@/lib/agent-types";
import { notifyArtworkCacheCleared } from "@/lib/artwork-cache-events";
import type {
  AddCuratedGameInput,
  AppBootstrap,
  AppPreferences,
  ArchiveReport,
  ArtCacheMaintenanceReport,
  ArtCacheUsage,
  ArtworkTarget,
  BulkUpdateStatusInput,
  CachedArtwork,
  CuratedList,
  CuratedListDetail,
  DatabaseDiagnostics,
  DatabaseRecoverySnapshot,
  DiscoverySnapshot,
  DlcFilter,
  DlcRefreshReport,
  DlcSummary,
  DrmStateCounts,
  ExternalGame,
  ExternalGameRequest,
  ExternalStoreAccount,
  ExternalStoreId,
  ExternalStoreScanReport,
  FamilyCatalogGame,
  FamilyCatalogRequest,
  FamilySessionStatus,
  FamilySyncReport,
  GameDetail,
  GameDlc,
  GameListRequest,
  GamePrice,
  GameReminder,
  GameVideo,
  GameVideoRef,
  LibraryDropInput,
  LibraryDropReceipt,
  LibraryDropResult,
  LibraryFilterOptions,
  LocalSteamImportResult,
  MetadataEnrichmentStatus,
  NewsRefreshReport,
  NotificationInbox,
  NotificationInboxFilter,
  NotificationRefreshReport,
  NotificationRule,
  PagedArchivedGames,
  PagedExternalGames,
  PagedFamilyCatalogGames,
  PagedGameSessions,
  PagedGames,
  PlannerOverview,
  PlannerSettings,
  PriceHistory,
  PriceRefreshReport,
  PriorityExplanation,
  PriorityRanking,
  PriorityRecomputeReport,
  Recommendation,
  RichGameMetadata,
  SaveCollectionInput,
  SaveCuratedListInput,
  SaveGameVideoInput,
  SaveNotificationRuleInput,
  SavePersonalDatesInput,
  SavePlannerItemInput,
  SaveReminderInput,
  SaveSessionInput,
  SaveTagInput,
  SaveWishlistEntryInput,
  SmartRule,
  SteamSyncResult,
  SteamWishlistImportResult,
  StoreDetection,
  SyncRun,
  TagDefinition,
  TasteReport,
  TasteSurface,
  TasteVerdict,
  UpcomingRefreshReport,
  UpcomingRelease,
  UpdateCheckResult,
  UpdateCuratedItemInput,
  UpdateGameInput,
  VideoKind,
  VideoProvider,
  WishlistBucketId,
  WishlistEntry,
  WishlistOverview,
  WishlistPriceStatus,
} from "@/lib/types";

export const api = {
  databaseRecoveryStatus: () => invoke<DatabaseRecoverySnapshot>("get_database_recovery_status"),
  selectDatabaseRecoveryBackup: () =>
    invoke<DatabaseRecoverySnapshot>("select_database_recovery_backup"),
  refreshDatabaseRecoveryBackups: () =>
    invoke<DatabaseRecoverySnapshot>("refresh_database_recovery_backups"),
  restoreDatabaseRecoveryBackup: (candidateId: string, confirmation: string) =>
    invoke<DatabaseRecoverySnapshot>("restore_database_recovery_backup", {
      candidateId,
      confirmation,
    }),
  createCleanDatabaseAfterRecovery: (confirmation: string) =>
    invoke<DatabaseRecoverySnapshot>("create_clean_database_after_recovery", { confirmation }),
  exportQuarantinedDatabase: () => invoke<boolean>("export_quarantined_database"),
  bootstrap: () => invoke<AppBootstrap>("bootstrap"),
  listGameDlc: (appId: number, filter?: DlcFilter) =>
    invoke<GameDlc[]>("list_game_dlc", { appId, filter }),
  refreshGameDlc: (appId: number, detailBudget?: number) =>
    invoke<DlcRefreshReport>("refresh_game_dlc", { appId, detailBudget }),
  setDlcOwned: (appId: number, dlcAppId: number, owned: boolean) =>
    invoke<GameDlc>("set_dlc_owned", { appId, dlcAppId, owned }),
  setDlcHidden: (appId: number, dlcAppId: number, hidden: boolean) =>
    invoke<GameDlc>("set_dlc_hidden", { appId, dlcAppId, hidden }),
  setDlcInstalled: (appId: number, dlcAppId: number, installed: boolean) =>
    invoke<GameDlc>("set_dlc_installed", { appId, dlcAppId, installed }),
  dlcSummary: (appId: number) => invoke<DlcSummary>("get_dlc_summary", { appId }),
  listSavedViews: () => invoke<SavedLibraryView[]>("list_saved_views"),
  saveSavedView: (input: SaveViewInput) => invoke<SavedLibraryView>("save_saved_view", { input }),
  deleteSavedView: (viewId: string) => invoke<void>("delete_saved_view", { viewId }),
  reorderSavedViews: (orderedIds: string[]) => invoke<void>("reorder_saved_views", { orderedIds }),
  markSavedViewUsed: (viewId: string) =>
    invoke<SavedLibraryView>("mark_saved_view_used", { viewId }),
  listCuratedLists: () => invoke<CuratedList[]>("list_curated_lists"),
  saveCuratedList: (input: SaveCuratedListInput) =>
    invoke<CuratedList>("save_curated_list", { input }),
  deleteCuratedList: (listId: string) => invoke<void>("delete_curated_list", { listId }),
  reorderCuratedLists: (orderedIds: string[]) =>
    invoke<void>("reorder_curated_lists", { orderedIds }),
  curatedListDetail: (listId: string, limit?: number, offset?: number) =>
    invoke<CuratedListDetail>("get_curated_list_detail", { listId, limit, offset }),
  addCuratedGame: (input: AddCuratedGameInput) => invoke<void>("add_curated_game", { input }),
  updateCuratedItem: (input: UpdateCuratedItemInput) =>
    invoke<void>("update_curated_item", { input }),
  removeCuratedGame: (listId: string, appId: number) =>
    invoke<void>("remove_curated_game", { listId, appId }),
  moveCuratedItem: (listId: string, appId: number, beforeAppId?: number) =>
    invoke<void>("move_curated_item", { listId, appId, beforeAppId }),
  reorderCuratedItems: (listId: string, orderedAppIds: number[]) =>
    invoke<void>("reorder_curated_items", { listId, orderedAppIds }),
  wishlistOverview: () => invoke<WishlistOverview>("get_wishlist_overview"),
  /**
   * Situación de precio de cada entrada de deseados. Se pide aparte de
   * `wishlistOverview` a propósito: el tablero se pinta con lo que ya hay
   * guardado aunque el precio todavía no haya llegado.
   */
  wishlistPrices: () => invoke<WishlistPriceStatus[]>("list_wishlist_prices"),
  gamePrices: (appId: number) => invoke<GamePrice[]>("get_game_prices", { appId }),
  gamePriceHistory: (appId: number, currency: string, limit?: number) =>
    invoke<PriceHistory>("get_game_price_history", { appId, currency, limit }),
  forgetGamePrices: (appId: number) => invoke<void>("forget_game_prices", { appId }),
  /** Única llamada que habla con la tienda: pregunta el precio de lo caducado. */
  refreshWishlistPrices: (limit?: number) =>
    invoke<PriceRefreshReport>("refresh_wishlist_prices", { limit }),
  archiveGames: (appIds: number[], reason?: string) =>
    invoke<ArchiveReport>("archive_games", { appIds, reason }),
  unarchiveGames: (appIds: number[]) => invoke<ArchiveReport>("unarchive_games", { appIds }),
  listArchivedGames: (limit?: number, offset?: number) =>
    invoke<PagedArchivedGames>("list_archived_games", { limit, offset }),
  countArchivedGames: () => invoke<number>("count_archived_games"),
  isGameArchived: (appId: number) => invoke<boolean>("is_game_archived", { appId }),
  saveWishlistEntry: (input: SaveWishlistEntryInput) =>
    invoke<WishlistEntry>("save_wishlist_entry", { input }),
  removeWishlistEntry: (appId: number) => invoke<void>("remove_wishlist_entry", { appId }),
  moveWishlistEntry: (appId: number, bucket: WishlistBucketId, beforeAppId?: number) =>
    invoke<void>("move_wishlist_entry", { appId, bucket, beforeAppId }),
  reorderWishlistBucket: (bucket: WishlistBucketId, orderedAppIds: number[]) =>
    invoke<void>("reorder_wishlist_bucket", { bucket, orderedAppIds }),
  /**
   * Importa la lista de deseados de Steam. No usa la clave Web API: el
   * endpoint de deseados sólo necesita el SteamID64 ya vinculado.
   */
  importSteamWishlist: () => invoke<SteamWishlistImportResult>("import_steam_wishlist"),
  detectExternalStores: () => invoke<StoreDetection[]>("detect_external_stores"),
  listExternalStoreAccounts: () => invoke<ExternalStoreAccount[]>("list_external_store_accounts"),
  scanExternalStore: (store: ExternalStoreId) =>
    invoke<ExternalStoreScanReport>("scan_external_store", { store }),
  scanExternalStores: () => invoke<ExternalStoreScanReport[]>("scan_external_stores"),
  rematchExternalStores: () => invoke<number>("rematch_external_stores"),
  listExternalGames: (request: ExternalGameRequest) =>
    invoke<PagedExternalGames>("list_external_games", { request }),
  setExternalGameMatch: (store: ExternalStoreId, externalId: string, appId: number | null) =>
    invoke<ExternalGame>("set_external_game_match", { store, externalId, appId }),
  clearExternalGameMatch: (store: ExternalStoreId, externalId: string) =>
    invoke<ExternalGame>("clear_external_game_match", { store, externalId }),
  linkExternalStore: (store: ExternalStoreId) =>
    invoke<ExternalStoreAccount>("link_external_store", { store }),
  unlinkExternalStore: (store: ExternalStoreId) => invoke<void>("unlink_external_store", { store }),
  // La acción viaja explícita para que el puente IPC valide contra la
  // allowlist del backend en vez de confiar en el nombre del comando.
  launchExternalGame: (store: ExternalStoreId, externalId: string) =>
    invoke<void>("launch_external_game", { store, externalId, action: "launch" }),
  listGameVideos: (appId: number, kind?: VideoKind) =>
    invoke<GameVideo[]>("list_game_videos", { appId, kind }),
  saveGameVideo: (input: SaveGameVideoInput) => invoke<GameVideo>("save_game_video", { input }),
  deleteGameVideo: (appId: number, provider: VideoProvider, videoId: string) =>
    invoke<void>("delete_game_video", { appId, provider, videoId }),
  reorderGameVideos: (appId: number, kind: VideoKind, ordered: GameVideoRef[]) =>
    invoke<void>("reorder_game_videos", { appId, kind, ordered }),
  listGames: (request: GameListRequest) => invoke<PagedGames>("list_games", { request }),
  libraryFilterOptions: () => invoke<LibraryFilterOptions>("get_library_filter_options"),
  gameDetail: (appId: number) => invoke<GameDetail>("get_game_detail", { appId }),
  richGameMetadata: (appId: number) =>
    invoke<RichGameMetadata>("get_rich_game_metadata", { appId }),
  drmStateCounts: () => invoke<DrmStateCounts>("get_drm_state_counts"),
  maintainArtCache: () => invoke<ArtCacheMaintenanceReport>("maintain_art_cache"),
  /** Carátulas que la biblioteca puede llegar a enseñar, para completar la caché. */
  listArtworkTargets: () => invoke<ArtworkTarget[]>("list_artwork_targets"),
  /** Cuánto ocupa la caché de arte y cuánto se le permite ocupar. */
  getArtCacheUsage: () => invoke<ArtCacheUsage>("get_art_cache_usage"),
  listNotificationRules: (appId?: number) =>
    invoke<NotificationRule[]>("list_notification_rules", { appId }),
  saveNotificationRule: (input: SaveNotificationRuleInput) =>
    invoke<NotificationRule>("save_notification_rule", { input }),
  deleteNotificationRule: (id: string) => invoke<void>("delete_notification_rule", { id }),
  notificationInbox: (filter?: NotificationInboxFilter, limit?: number, offset?: number) =>
    invoke<NotificationInbox>("get_notification_inbox", { filter, limit, offset }),
  markNotificationRead: (id: string) => invoke<void>("mark_notification_read", { id }),
  markAllNotificationsRead: () => invoke<number>("mark_all_notifications_read"),
  dismissNotification: (id: string) => invoke<void>("dismiss_notification", { id }),
  dismissAllNotifications: () => invoke<number>("dismiss_all_notifications"),
  refreshNotificationEvents: () => invoke<NotificationRefreshReport>("refresh_notification_events"),
  recomputePriorities: () => invoke<PriorityRecomputeReport>("recompute_priorities"),
  explainPriority: (appId: number) => invoke<PriorityExplanation>("explain_priority", { appId }),
  setPriorityLock: (appId: number, locked: boolean) =>
    invoke<void>("set_priority_lock", { appId, locked }),
  priorityRanking: (limit?: number) =>
    invoke<PriorityRanking[]>("list_priority_ranking", { limit }),
  learnTaste: () => invoke<TasteReport>("learn_taste"),
  recordTasteFeedback: (appId: number, verdict: TasteVerdict, surface: TasteSurface) =>
    invoke<void>("record_taste_feedback", { appId, verdict, surface }),
  scoreUpcomingReleases: () => invoke<number>("score_upcoming_releases"),
  /** Revisa una tanda de deseados y guarda los que aún no han salido. */
  refreshUpcomingReleases: () => invoke<UpcomingRefreshReport>("refresh_upcoming_releases"),
  upcomingReleases: (limit?: number) =>
    invoke<UpcomingRelease[]>("list_upcoming_releases", { limit }),
  dismissUpcomingRelease: (appId: number) => invoke<void>("dismiss_upcoming_release", { appId }),
  listTags: () => invoke<TagDefinition[]>("list_tags"),
  saveTag: (input: SaveTagInput) => invoke<TagDefinition>("save_tag", { input }),
  deleteTag: (id: string) => invoke<void>("delete_tag", { id }),
  setGameTags: (appId: number, tagIds: string[]) =>
    invoke<GameDetail>("set_game_tags", { appId, tagIds }),
  listGameSessions: (appId: number, limit = 50, offset = 0) =>
    invoke<PagedGameSessions>("list_game_sessions", { appId, limit, offset }),
  saveGameSession: (input: SaveSessionInput) => invoke<GameDetail>("save_game_session", { input }),
  deleteGameSession: (id: string) => invoke<GameDetail>("delete_game_session", { id }),
  savePersonalDates: (input: SavePersonalDatesInput) =>
    invoke<GameDetail>("save_personal_dates", { input }),
  refreshGameMetadata: (appId: number, force = false) =>
    invoke<GameDetail>("refresh_game_metadata", { appId, force }),
  startMetadataEnrichment: (visibleAppIds: number[], includeBacklog = true) =>
    invoke<MetadataEnrichmentStatus>("start_metadata_enrichment", {
      visibleAppIds,
      includeBacklog,
    }),
  metadataEnrichmentStatus: () => invoke<MetadataEnrichmentStatus>("metadata_enrichment_status"),
  refreshGameAchievements: (appId: number) =>
    invoke<GameDetail>("refresh_game_achievements", { appId }),
  updateGame: (input: UpdateGameInput) => invoke<GameDetail>("update_game", { input }),
  bulkUpdateStatus: (input: BulkUpdateStatusInput) =>
    invoke<number>("bulk_update_status", { appIds: input.appIds, statusId: input.statusId }),
  applyLibraryDrop: (input: LibraryDropInput) =>
    invoke<LibraryDropResult>("apply_library_drop", { input }),
  undoLibraryDrop: (receipt: LibraryDropReceipt) =>
    invoke<number>("undo_library_drop", { receipt }),
  setGameCollections: (appId: number, collectionIds: string[]) =>
    invoke<GameDetail>("set_game_collections", { appId, collectionIds }),
  movePlannerItem: (appId: number, columnId: string, position: number) =>
    invoke<void>("move_planner_item", { input: { appId, columnId, position } }),
  getPlannerOverview: () => invoke<PlannerOverview>("get_planner_overview"),
  movePlannerQueueItem: (appId: number, position: number) =>
    invoke<void>("move_planner_queue_item", { appId, position }),
  savePlannerItem: (input: SavePlannerItemInput) => invoke<void>("save_planner_item", { input }),
  savePlannerCapacity: (settings: PlannerSettings) =>
    invoke<PlannerSettings>("save_planner_capacity", { settings }),
  removePlannerItem: (appId: number) => invoke<void>("remove_planner_item", { appId }),
  saveStatus: (id: string | undefined, name: string, color: string) =>
    invoke("save_status", { id, name, color }),
  deleteStatus: (id: string, replacementId: string) =>
    invoke<void>("delete_status", { id, replacementId }),
  reorderStatuses: (ids: string[]) => invoke<void>("reorder_statuses", { ids }),
  savePlannerColumn: (id: string | undefined, name: string, color: string, wipLimit?: number) =>
    invoke("save_planner_column", { id, name, color, wipLimit }),
  deletePlannerColumn: (id: string, replacementId?: string) =>
    invoke<void>("delete_planner_column", { id, replacementId }),
  reorderPlannerColumns: (ids: string[]) => invoke<void>("reorder_planner_columns", { ids }),
  saveCollection: (input: SaveCollectionInput) => invoke<void>("save_collection", { input }),
  previewSmartCollection: (input: SaveCollectionInput) =>
    invoke<PagedGames>("preview_smart_collection", { input }),
  listSmartRules: (collectionId: string) =>
    invoke<SmartRule[]>("list_smart_rules", { collectionId }),
  deleteCollection: (id: string) => invoke<void>("delete_collection", { id }),
  /** Cambia sólo la apariencia: no toca el nombre, la descripción ni las reglas. */
  setCollectionAppearance: (id: string, color: string, icon: string) =>
    invoke<void>("set_collection_appearance", { id, color, icon }),
  reorderCollections: (ids: string[]) => invoke<void>("reorder_collections", { ids }),
  importLocalSteam: () => invoke<LocalSteamImportResult>("import_local_steam"),
  startSteamLogin: () => invoke<void>("start_steam_login"),
  saveSteamApiKey: (apiKey: string) => invoke<void>("save_steam_api_key", { apiKey }),
  deleteSteamApiKey: () => invoke<void>("delete_steam_api_key"),
  verifySavedSteamApiKey: () => invoke<boolean>("verify_saved_steam_api_key"),
  syncSteamLibrary: () => invoke<SteamSyncResult>("sync_steam_library"),
  listSyncRuns: (limit?: number) => invoke<SyncRun[]>("list_sync_runs", { limit }),
  listFamilyCatalog: (request: FamilyCatalogRequest) =>
    invoke<PagedFamilyCatalogGames>("list_family_catalog", { request }),
  familyCatalogGame: (appId: number) =>
    invoke<FamilyCatalogGame>("get_family_catalog_game", { appId }),
  unlinkSteam: () => invoke<void>("unlink_steam"),
  recommendGame: (durationMinutes?: number, mood?: string) =>
    invoke<Recommendation>("recommend_game", {
      request: { durationMinutes, mood },
    }),
  discoverySnapshot: () => invoke<DiscoverySnapshot>("get_discovery_snapshot"),
  refreshDiscoveryNews: () => invoke<NewsRefreshReport>("refresh_discovery_news"),
  saveReminder: (input: SaveReminderInput) => invoke<GameReminder>("save_reminder", { input }),
  completeReminder: (id: string) => invoke<void>("complete_reminder", { id }),
  snoozeReminder: (id: string, dueAt: string) =>
    invoke<GameReminder>("snooze_reminder", { id, dueAt }),
  dismissRecommendation: (historyId: string) =>
    invoke<void>("dismiss_recommendation", { historyId }),
  restoreRecommendation: (historyId: string) =>
    invoke<void>("restore_recommendation", { historyId }),
  diagnostics: () => invoke<DatabaseDiagnostics>("get_database_diagnostics"),
  exportBackup: () => invoke<boolean>("export_backup"),
  importBackup: () => invoke<boolean>("import_backup"),
  launchGame: (appId: number) => invoke<void>("launch_game", { appId }),
  installGame: (appId: number) => invoke<void>("install_game", { appId }),
  uninstallGame: (appId: number) => invoke<void>("uninstall_game", { appId }),
  openStore: (appId: number) => invoke<void>("open_store", { appId }),
  /** Abre la portada de una tienda en el navegador integrado, con su sesión. */
  openStoreBrowser: (storeId: string) => invoke<void>("open_store_browser", { storeId }),
  revealInstallation: (appId: number) => invoke<void>("reveal_installation", { appId }),
  cacheGameArt: (
    appId: number,
    variant: "icon" | "cover" | "header" | "hero",
    sourceUrl?: string,
  ) => invoke<CachedArtwork>("cache_game_art", { appId, variant, sourceUrl }),
  clearArtCache: async () => {
    await invoke<void>("clear_art_cache");
    notifyArtworkCacheCleared();
  },
  savePreferences: (preferences: AppPreferences) =>
    invoke<void>("save_preferences", { preferences }),
  checkForUpdates: () => invoke<UpdateCheckResult>("check_for_updates"),
  steamFamilySessionStatus: () => invoke<FamilySessionStatus>("steam_family_session_status"),
  linkSteamFamilySession: () => invoke<FamilySessionStatus>("link_steam_family_session"),
  unlinkSteamFamilySession: () => invoke<FamilySessionStatus>("unlink_steam_family_session"),
  syncSteamFamilyCatalog: () => invoke<FamilySyncReport>("sync_steam_family_catalog"),
  agentDispatch: (request: AgentRequest) => invoke<AgentOutcome>("agent_dispatch", { request }),
  agentConfirm: (auditId: string, approve: boolean) =>
    invoke<AgentOutcome>("agent_confirm", { auditId, approve }),
  agentUndo: (undoToken: string) => invoke<AgentOutcome>("agent_undo", { undoToken }),
  issueAgentClient: (input: NewAgentClient) =>
    invoke<IssuedAgentClient>("issue_agent_client", { input }),
  rotateAgentToken: (clientId: string) =>
    invoke<IssuedAgentClient>("rotate_agent_token", { clientId }),
  setAgentClientScopes: (clientId: string, scopes: AgentScope[]) =>
    invoke<AgentClientSummary>("set_agent_client_scopes", { clientId, scopes }),
  setAgentClientEnabled: (clientId: string, enabled: boolean) =>
    invoke<AgentClientSummary>("set_agent_client_enabled", { clientId, enabled }),
  revokeAgentClient: (clientId: string) => invoke<void>("revoke_agent_client", { clientId }),
  localModelSurvey: () => invoke<LocalModelSurvey>("local_model_survey"),
  suggestLocalModels: (usableBytes: number | null) =>
    invoke<CatalogSuggestions>("suggest_local_models", { usableBytes }),
  localModelInstallPlan: () => invoke<InstallPlan>("local_model_install_plan"),
  installLocalRuntime: () => invoke<string>("install_local_runtime"),
  listAgentTasks: () => invoke<ScheduledTask[]>("list_agent_tasks"),
  saveAgentTask: (input: SaveScheduledTask) => invoke<ScheduledTask>("save_agent_task", { input }),
  deleteAgentTask: (taskId: string) => invoke<void>("delete_agent_task", { taskId }),
  vindagentConfig: () => invoke<AgentModelConfig>("vindagent_config"),
  saveVindagentConfig: (input: SaveAgentModelConfig) =>
    invoke<AgentModelConfig>("save_vindagent_config", { input }),
  vindagentChat: (baseUrl: string, model: string, history: ChatMessage[]) =>
    invoke<ChatTurn>("vindagent_chat", { baseUrl, model, history }),
  agentAutolinkState: () => invoke<AgentAutolinkStatus>("agent_autolink_state"),
  setAgentAutolinkDisabled: (disabled: boolean) =>
    invoke<void>("set_agent_autolink_disabled", { disabled }),
  listAgentClients: () => invoke<AgentClientSummary[]>("list_agent_clients"),
  listAgentAudit: (limit: number) => invoke<AgentAuditEntry[]>("list_agent_audit", { limit }),
};

export function getErrorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object") {
    const candidate = error as { message?: unknown; code?: unknown };
    if (typeof candidate.message === "string") return candidate.message;
    if (typeof candidate.code === "string") return candidate.code;
  }
  return "Vindexa no pudo completar la operación.";
}
