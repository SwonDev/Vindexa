import { invoke } from "@tauri-apps/api/core";
import { notifyArtworkCacheCleared } from "@/lib/artwork-cache-events";
import type {
  AppBootstrap,
  AppPreferences,
  BulkUpdateStatusInput,
  CachedArtwork,
  DatabaseDiagnostics,
  DatabaseRecoverySnapshot,
  DiscoverySnapshot,
  FamilyCatalogGame,
  FamilyCatalogRequest,
  GameDetail,
  GameListRequest,
  GameReminder,
  LibraryDropInput,
  LibraryDropReceipt,
  LibraryDropResult,
  LibraryFilterOptions,
  LocalSteamImportResult,
  MetadataEnrichmentStatus,
  NewsRefreshReport,
  PagedFamilyCatalogGames,
  PagedGameSessions,
  PagedGames,
  PlannerOverview,
  PlannerSettings,
  Recommendation,
  SaveCollectionInput,
  SavePersonalDatesInput,
  SavePlannerItemInput,
  SaveReminderInput,
  SaveSessionInput,
  SaveTagInput,
  SmartRule,
  SteamSyncResult,
  TagDefinition,
  UpdateCheckResult,
  UpdateGameInput,
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
  listGames: (request: GameListRequest) => invoke<PagedGames>("list_games", { request }),
  libraryFilterOptions: () => invoke<LibraryFilterOptions>("get_library_filter_options"),
  gameDetail: (appId: number) => invoke<GameDetail>("get_game_detail", { appId }),
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
  reorderCollections: (ids: string[]) => invoke<void>("reorder_collections", { ids }),
  importLocalSteam: () => invoke<LocalSteamImportResult>("import_local_steam"),
  startSteamLogin: () => invoke<void>("start_steam_login"),
  saveSteamApiKey: (apiKey: string) => invoke<void>("save_steam_api_key", { apiKey }),
  deleteSteamApiKey: () => invoke<void>("delete_steam_api_key"),
  verifySavedSteamApiKey: () => invoke<boolean>("verify_saved_steam_api_key"),
  syncSteamLibrary: () => invoke<SteamSyncResult>("sync_steam_library"),
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
