export type AppSection = "library" | "planner" | "collections" | "tracking";
export type LibraryView = "grid" | "list" | "compact";
export type GameSort =
  | "manual"
  | "alphabetical"
  | "alphabeticalDesc"
  | "lastPlayed"
  | "recentlyAdded"
  | "releaseDate"
  | "releaseDateAsc"
  | "playtime"
  | "playtimeAsc"
  | "installedFirst"
  | "uninstalledFirst"
  | "sizeDesc"
  | "sizeAsc"
  | "progress"
  | "rating"
  | "targetDate"
  | "random";

export interface StatusDefinition {
  id: string;
  name: string;
  color: string;
  position: number;
  builtIn: boolean;
  gameCount: number;
}

export interface SmartRule {
  id?: string;
  groupId: number;
  field: string;
  operator: string;
  value: unknown;
  position: number;
}

export interface CollectionSummary {
  id: string;
  name: string;
  description: string;
  color: string;
  icon: string;
  kind: "manual" | "smart";
  matchMode: "all" | "any";
  position: number;
  gameCount: number;
}

export interface SaveCollectionInput {
  id?: string;
  name: string;
  description: string;
  color: string;
  icon: string;
  kind: "manual" | "smart";
  matchMode: "all" | "any";
  rules: SmartRule[];
}

export interface PlannerItem {
  appId: number;
  title: string;
  coverUrl?: string;
  progress: number;
  position: number;
  queuePosition: number;
  plannedFor?: string;
  objective?: string;
  targetDate?: string;
  estimatedMinutes?: number;
}

export interface PlannerSettings {
  weeklyCapacityMinutes: number;
  monthlyCapacityMinutes: number;
}

export interface PlannerOverview {
  columns: PlannerColumn[];
  queue: PlannerItem[];
  settings: PlannerSettings;
}

export interface SavePlannerItemInput {
  appId: number;
  objective: string | null;
  plannedFor: string | null;
  targetDate: string | null;
  estimatedMinutes: number | null;
}

export interface PlannerColumn {
  id: string;
  name: string;
  color: string;
  position: number;
  wipLimit?: number;
  items: PlannerItem[];
}

export interface SteamAccount {
  steamId: string;
  personaName?: string;
  avatarUrl?: string;
  profileUrl?: string;
  visibility?: number;
  lastSyncAt?: string;
  lastSyncStatus?: string;
  lastSyncErrorCode?: string;
  lastSyncErrorMessage?: string;
}

export interface SteamConfiguration {
  account?: SteamAccount;
  apiKeyConfigured: boolean;
  apiKeyVerificationRequired: boolean;
  localSteamDetected: boolean;
  localManifestCount: number;
}

export interface LibraryStats {
  totalGames: number;
  installedGames: number;
  playingGames: number;
  backlogGames: number;
  trackedGames: number;
  totalPlaytimeMinutes: number;
}

export interface AppPreferences {
  density: "compact" | "comfortable";
  periodicSyncMinutes: number;
  confirmUninstall: boolean;
  librarySort: GameSort;
  shortcuts: ShortcutBindings;
}

export type ShortcutAction =
  | "library"
  | "planner"
  | "collections"
  | "tracking"
  | "search"
  | "sync"
  | "closePanel";

export type ShortcutBindings = Record<ShortcutAction, string>;

export interface UpdateCheckResult {
  status: "notConfigured" | "upToDate" | "available";
  currentVersion: string;
  availableVersion?: string;
  message: string;
}

export interface AppBootstrap {
  stats: LibraryStats;
  statuses: StatusDefinition[];
  collections: CollectionSummary[];
  planner: PlannerColumn[];
  steam: SteamConfiguration;
  preferences: AppPreferences;
  databasePath: string;
}

export interface GameSummary {
  appId: number;
  title: string;
  iconUrl?: string;
  coverUrl?: string;
  headerUrl?: string;
  playtimeMinutes: number;
  playtimeRecentMinutes: number;
  lastPlayedAt?: string;
  releaseDate?: string;
  isEarlyAccess: boolean;
  isFree: boolean;
  steamDeckStatus?: string;
  achievementsUnlocked?: number;
  achievementsTotal?: number;
  ownershipSource: "owned" | "family_shared" | "local";
  familyAvailability: "not_applicable" | "unknown" | "confirmed";
  installed: boolean;
  installPath?: string;
  sizeOnDisk?: number;
  statusId: string;
  statusName: string;
  statusColor: string;
  progress: number;
  priority: number;
  pinned: boolean;
  tracking: boolean;
  rating?: number;
  estimatedMinutes?: number;
  targetDate?: string;
  nextAction?: string;
  checkpoint?: string;
  notes?: string;
  manualPosition: number;
}

export interface GameSession {
  id: string;
  startedAt: string;
  endedAt?: string;
  progressBefore?: number;
  progressAfter?: number;
  note: string;
}

export interface PagedGameSessions {
  items: GameSession[];
  total: number;
  limit: number;
  offset: number;
}

export interface TagDefinition {
  id: string;
  name: string;
  color: string;
}

export interface SaveTagInput {
  id?: string;
  name: string;
  color: string;
}

export interface SaveSessionInput {
  id?: string;
  appId: number;
  startedAt: string;
  endedAt?: string;
  progressBefore?: number;
  progressAfter?: number;
  note: string;
}

export interface SavePersonalDatesInput {
  appId: number;
  startedAt?: string;
  completedAt?: string;
  abandonedAt?: string;
}

export interface ActivityItem {
  id: string;
  kind: string;
  message: string;
  createdAt: string;
}

export interface GameDetail extends GameSummary {
  heroUrl?: string;
  shortDescription?: string;
  developer?: string;
  publisher?: string;
  genres: string[];
  categories: string[];
  metadataStatus: "pending" | "success" | "unavailable" | "failed";
  metadataFetchedAt?: string;
  achievementsStatus: "pending" | "success" | "unavailable" | "failed";
  achievementsFetchedAt?: string;
  collectionIds: string[];
  tags: string[];
  tagIds?: string[];
  sessions: GameSession[];
  sessionsTotal?: number;
  activity: ActivityItem[];
  startedAt?: string;
  completedAt?: string;
  abandonedAt?: string;
}

export interface LibraryFilterChoice {
  id: string;
  name: string;
}

export interface LibraryFilterOptions {
  genres: string[];
  categories: string[];
  tags: LibraryFilterChoice[];
  totalGames: number;
  metadataGames: number;
  achievementGames: number;
  steamDeckGames: number;
}

export interface MetadataEnrichmentStatus {
  running: boolean;
  totalGames: number;
  freshMetadata: number;
  queued: number;
  processing: number;
  retrying: number;
  succeeded: number;
  unavailable: number;
  failed: number;
  nextRetryAt?: string;
  lastErrorCode?: string;
  steamDeckAvailability: "disabled";
  steamDeckExplanation: string;
}

export interface GameListRequest {
  query?: string;
  statusId?: string;
  collectionId?: string;
  installed?: boolean;
  tracking?: boolean;
  earlyAccess?: boolean;
  isFree?: boolean;
  ownershipSource?: "owned" | "family_shared" | "local";
  neverPlayed?: boolean;
  minPlaytimeMinutes?: number;
  maxPlaytimeMinutes?: number;
  minProgress?: number;
  maxProgress?: number;
  minRating?: number;
  maxRating?: number;
  genre?: string;
  category?: string;
  tagId?: string;
  releaseFrom?: string;
  releaseTo?: string;
  lastPlayedFrom?: string;
  lastPlayedTo?: string;
  minAchievementPercent?: number;
  maxAchievementPercent?: number;
  steamDeckStatus?: string;
  targetDateFrom?: string;
  targetDateTo?: string;
  minSessionMinutes?: number;
  maxSessionMinutes?: number;
  sort?: GameSort;
  sortSeed?: number;
  limit?: number;
  offset?: number;
}

export interface PagedGames {
  items: GameSummary[];
  total: number;
  limit: number;
  offset: number;
}

export interface UpdateGameInput {
  appId: number;
  statusId: string;
  progress: number;
  priority: number;
  pinned: boolean;
  tracking: boolean;
  rating?: number | undefined;
  estimatedMinutes?: number | undefined;
  targetDate?: string | undefined;
  nextAction?: string | undefined;
  checkpoint?: string | undefined;
  notes?: string | undefined;
}

export interface BulkUpdateStatusInput {
  appIds: number[];
  statusId: string;
}

export type LibraryDropTarget =
  | { kind: "status"; id: string }
  | { kind: "collection"; id: string; beforeAppId?: number | undefined }
  | { kind: "manual"; beforeAppId: number };

export interface StatusPlacement {
  appId: number;
  statusId: string;
}

export type LibraryDropReceipt =
  | {
      kind: "status";
      operationId: string;
      targetId: string;
      appIds: number[];
      previous: StatusPlacement[];
      activityIds: string[];
    }
  | {
      kind: "collection";
      operationId: string;
      targetId: string;
      appIds: number[];
      beforeAppId?: number | undefined;
      previousOrder: number[];
      appliedOrder: number[];
    }
  | {
      kind: "manual";
      operationId: string;
      appIds: number[];
      beforeAppId: number;
      previousOrder: number[];
      appliedOrder: number[];
    };

export interface LibraryDropInput {
  appIds: number[];
  target: LibraryDropTarget;
}

export interface LibraryDropResult {
  moved: number;
  receipt: LibraryDropReceipt;
}

export interface Recommendation {
  historyId: string;
  game: GameSummary;
  reasons: string[];
}

export interface GameReminder {
  id: string;
  appId: number;
  title: string;
  iconUrl?: string;
  dueAt: string;
  note: string;
  completedAt?: string;
}

export interface SaveReminderInput {
  appId: number;
  dueAt: string;
  note: string;
}

export interface DiscoveryEvent {
  id: string;
  appId: number;
  title: string;
  iconUrl?: string;
  kind: "early_access_changed" | "release_date_changed";
  previousValue?: string;
  currentValue?: string;
  observedAt: string;
}

export interface DismissedRecommendation {
  id: string;
  appId: number;
  title: string;
  iconUrl?: string;
  durationMinutes?: number;
  mood?: string;
  createdAt: string;
}

export interface OfficialPublication {
  gid: string;
  appId: number;
  gameTitle: string;
  iconUrl?: string;
  title: string;
  contentPreview: string;
  publishedAt: string;
  feedLabel: string;
  feedName: "steam_community_announcements";
}

export interface RelatedRelease {
  appId: number;
  title: string;
  iconUrl?: string;
  coverUrl?: string;
  releaseDate: string;
  relatedToAppId: number;
  relatedToTitle: string;
  criterion: "developer" | "publisher";
  criterionValue: string;
}

export interface NewsRefreshReport {
  attemptedGames: number;
  refreshedGames: number;
  publicationsSaved: number;
  failedGames: number;
  skippedByCadence: number;
  nextRefreshAt?: string;
}

export interface DiscoveryCapabilities {
  metadataObservations: number;
  earlyAccessHistoryAvailable: boolean;
  trackedNewsGames: number;
  officialPublicationsAvailable: boolean;
  newsLastRefreshedAt?: string;
  newsNextRefreshAt?: string;
  relatedReleasesAvailable: boolean;
}

export interface DiscoverySnapshot {
  reminders: GameReminder[];
  forgotten: GameSummary[];
  almostFinished: GameSummary[];
  upcoming: GameSummary[];
  events: DiscoveryEvent[];
  officialPublications: OfficialPublication[];
  relatedReleases: RelatedRelease[];
  dismissedRecommendations: DismissedRecommendation[];
  capabilities: DiscoveryCapabilities;
}

export interface DatabaseDiagnostics {
  path: string;
  sizeBytes: number;
  schemaVersion: number;
  integrity: string;
  walEnabled: boolean;
}

export interface DatabaseRecoveryIssue {
  code: string;
  message: string;
}

export interface QuarantinedDatabaseSummary {
  id: string;
  detectedAt: string;
  fileName: string;
  sizeBytes: number;
  sidecarCount: number;
  integrity: string;
  schemaVersion?: number;
}

export interface RecoveryBackupSummary {
  id: string;
  label: string;
  sizeBytes: number;
  modifiedAt?: string;
  source: "safety" | "selected";
  valid: boolean;
  validationMessage: string;
}

export interface DatabaseRecoverySnapshot {
  required: boolean;
  issue?: DatabaseRecoveryIssue;
  quarantine?: QuarantinedDatabaseSummary;
  backups: RecoveryBackupSummary[];
  recoveryActionsAvailable: boolean;
}

export interface CachedArtwork {
  localPath: string;
}

export interface LocalSteamImportResult {
  steamPath: string;
  librariesScanned: number;
  importedGames: number;
  updatedGames: number;
}

export interface SteamSyncResult {
  steamId: string;
  importedGames: number;
  updatedGames: number;
  privateLibrarySuspected: boolean;
  familyMembersDetected: number;
  familyMembersInaccessible: number;
  familyGamesImported: number;
  familyCatalogGamesDetected: number;
  completedAt: string;
}

export interface FamilyCatalogGame {
  appId: number;
  title: string;
  iconUrl?: string;
  coverUrl?: string;
  headerUrl?: string;
  availability: "unknown" | "confirmed";
  discoveredAt: string;
  updatedAt: string;
}

export type FamilyCatalogAvailability = "all" | "confirmed" | "unknown";
export type FamilyCatalogSort =
  | "availability"
  | "alphabetical"
  | "alphabeticalDesc"
  | "updatedDesc"
  | "discoveredDesc";

export interface FamilyCatalogRequest {
  query?: string;
  availability?: Exclude<FamilyCatalogAvailability, "all">;
  sort?: FamilyCatalogSort;
  limit?: number;
  offset?: number;
}

export interface PagedFamilyCatalogGames {
  items: FamilyCatalogGame[];
  total: number;
  limit: number;
  offset: number;
}

export interface AppErrorShape {
  code?: string;
  message?: string;
}
