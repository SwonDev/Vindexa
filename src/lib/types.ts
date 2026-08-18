/**
 * Secciones de la aplicación. `couch` es el modo sofá: ocupa la ventana entera
 * y por eso no aparece en la barra de secciones, sólo en la paleta y su atajo.
 */
export type AppSection = "library" | "planner" | "wishlist" | "collections" | "tracking" | "couch";
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
  /**
   * Juegos archivados. No están sumados en `totalGames`: el total tiene que
   * coincidir con lo que se ve. Esta cifra existe para que el archivado nunca
   * sea un agujero negro y la interfaz pueda ofrecer verlos.
   */
  archivedGames: number;
}

export interface AppPreferences {
  density: "compact" | "comfortable";
  periodicSyncMinutes: number;
  confirmUninstall: boolean;
  librarySort: GameSort;
  /** Presupuesto en disco de la caché de arte, en MiB (128–8192). */
  artCacheMib: number;
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
  /**
   * `unknown` cuando no hay con qué comparar o la comparación no es fiable, y
   * `unreachable` cuando no se ha podido preguntar. Ninguno de los dos es «estás
   * al día»: decirlo sin saberlo sería inventarlo.
   */
  status: "upToDate" | "available" | "unknown" | "unreachable";
  currentVersion: string;
  availableVersion?: string;
  message: string;
  /** Dónde se descargan los instaladores. */
  releasePage: string;
}

export interface AppBootstrap {
  /** Versión que se está ejecutando, tal y como la declara el paquete. */
  appVersion: string;
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
  /**
   * Sistemas en los que la tienda ofrece el juego. `undefined` significa que
   * **no se sabe**, que no es lo mismo que «no compatible»: sin el dato, la
   * interfaz no puede desaconsejar una instalación.
   */
  platformWindows?: boolean | null;
  platformMac?: boolean | null;
  platformLinux?: boolean | null;
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
  /** Colecciones manuales a las que pertenece el juego. */
  collectionIds: string[];
  /** Géneros declarados por la tienda; permiten agrupar sin pedir la ficha. */
  genres: string[];
  developer?: string;
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

/**
 * La ficha viaja con sus metadatos enriquecidos aplanados en la misma
 * respuesta: abrirla es una sola petición y el contenido no salta después.
 */
export interface GameDetail extends GameSummary, RichGameMetadata {
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
  /** Ámbito de archivado. Sin él la biblioteca esconde lo archivado. */
  archiveScope?: ArchiveScope;
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
  appId: number;
  variant: string;
  localPath: string;
  /** Tamaño real del archivo cacheado; permite reservar el hueco exacto. */
  width: number | null;
  height: number | null;
  bytes: number;
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

export interface SyncRun {
  id: string;
  source: string;
  status: string;
  startedAt: string;
  finishedAt?: string;
  importedCount: number;
  updatedCount: number;
  errorMessage?: string;
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

export type DlcMetadataStatus = "pending" | "success" | "unavailable" | "failed";
export type DlcFilter = "visible" | "all" | "owned" | "notOwned" | "installed" | "hidden";
export type DlcEvidenceGap =
  | "dlc_evidence_steam_not_installed"
  | "dlc_evidence_libraries_unreadable"
  | "dlc_evidence_game_not_installed"
  | "dlc_evidence_invalid_app_id";

export interface GameDlc {
  appId: number;
  dlcAppId: number;
  title: string;
  capsuleUrl?: string;
  headerUrl?: string;
  shortDescription?: string;
  releaseDate?: string;
  isFree: boolean;
  priceCents?: number;
  currency?: string;
  discountPercent?: number;
  owned: boolean;
  installed: boolean;
  hidden: boolean;
  metadataStatus: DlcMetadataStatus;
  metadataFetchedAt?: string;
  position: number;
  updatedAt: string;
}

export interface DlcSummary {
  appId: number;
  total: number;
  owned: number;
  installed: number;
  hidden: number;
  free: number;
  pending: number;
  /** Solo cubre `pendingCounted`: nunca es un total cerrado si hay desconocidos. */
  pendingValueCents?: number;
  pendingValueCurrency?: string;
  pendingCounted: number;
  pendingUnknownPrice: number;
  pendingOtherCurrency: number;
}

export interface DlcImportSummary {
  appId: number;
  received: number;
  inserted: number;
  updated: number;
  withMetadata: number;
  withoutMetadata: number;
  owned: number;
  installed: number;
}

export interface DlcRefreshReport {
  appId: number;
  declared: number;
  truncated: boolean;
  fetchedDetails: number;
  unavailableDetails: number;
  failedDetails: number;
  pendingDetails: number;
  /** Motivo por el que la propiedad no pudo confirmarse; nunca se adivina. */
  ownershipEvidenceGap?: DlcEvidenceGap;
  ownershipEvidenceExplanation?: string;
  imported: DlcImportSummary;
  summary: DlcSummary;
}

export type DrmState = "unknown" | "drm_free" | "third_party_drm" | "steam_drm";

export interface DrmEvidence {
  source:
    | "drmNotice"
    | "extUserAccountNotice"
    | "legalNotice"
    | "categories"
    | "storeAppdetails"
    /** Política publicada del catálogo de una tienda externa (GOG). */
    | "storeCatalogue";
  match: string;
}

/** Clasificación DRM con su evidencia. Nunca se muestra sobre la carátula. */
export interface DrmAssessment {
  state: DrmState;
  evidence: DrmEvidence[];
}

export interface DrmStateCounts {
  unknown: number;
  drmFree: number;
  thirdPartyDrm: number;
  steamDrm: number;
}

export type DescriptionBlock =
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; ordered: boolean; items: string[] };

/** Descripción ya saneada por el backend: el frontend nunca inyecta HTML. */
export interface StructuredDescription {
  blocks: DescriptionBlock[];
}

export type GameMediaKind = "screenshot" | "movie";

export interface GameMediaItem {
  mediaId: string;
  kind: GameMediaKind;
  thumbnailUrl?: string | null;
  fullUrl?: string | null;
  altUrl?: string | null;
  position: number;
}

export interface LogoPosition {
  pinnedPosition: "BottomLeft" | "BottomCenter" | "CenterCenter" | "UpperCenter";
  widthPct: number;
  heightPct: number;
}

export interface RichGameMetadata {
  appId: number;
  detailedDescription?: StructuredDescription | null;
  aboutTheGame?: StructuredDescription | null;
  supportedLanguages?: string | null;
  websiteUrl?: string | null;
  metacriticScore?: number | null;
  metacriticUrl?: string | null;
  requiredAge?: number | null;
  controllerSupport?: string | null;
  backgroundUrl?: string | null;
  libraryHeroUrl?: string | null;
  libraryLogoUrl?: string | null;
  logoPosition?: LogoPosition | null;
  drmNotice?: string | null;
  drm: DrmAssessment;
  drmCheckedAt?: string | null;
  screenshots: GameMediaItem[];
  movies: GameMediaItem[];
}

export type CuratedListKind = "manual" | "wishlist" | "backlog" | "showcase";
export type CuratedAccent =
  | "cyan"
  | "blue"
  | "teal"
  | "lime"
  | "amber"
  | "rose"
  | "violet"
  | "slate";

export interface CuratedList {
  id: string;
  name: string;
  description: string;
  kind: CuratedListKind;
  accent: CuratedAccent;
  icon: string;
  coverAppId?: number;
  coverUrl?: string;
  pinned: boolean;
  position: number;
  gameCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface SaveCuratedListInput {
  id?: string;
  name: string;
  description: string;
  kind: CuratedListKind;
  accent: CuratedAccent;
  icon: string;
  coverAppId?: number;
  pinned: boolean;
}

export interface CuratedListEntry {
  game: GameSummary;
  note: string;
  highlight: boolean;
  position: number;
  addedAt: string;
}

export interface CuratedListDetail {
  list: CuratedList;
  items: CuratedListEntry[];
  total: number;
  limit: number;
  offset: number;
}

export interface AddCuratedGameInput {
  listId: string;
  appId: number;
  note: string;
  highlight: boolean;
  beforeAppId?: number;
}

export interface UpdateCuratedItemInput {
  listId: string;
  appId: number;
  note: string;
  highlight: boolean;
}

export type WishlistBucketId = "buying_now" | "waiting_sale" | "considering" | "watching";

/**
 * El juego al que apunta una entrada de deseados.
 *
 * `library` sólo existe si el juego está en la biblioteca: uno que no tienes no
 * tiene estado, ni progreso, ni horas jugadas, y enseñarlos a cero los
 * convertiría en afirmaciones falsas.
 */
export interface WishlistGame {
  appId: number;
  title: string;
  coverUrl?: string;
  headerUrl?: string;
  inLibrary: boolean;
  library?: GameSummary;
}

export interface WishlistEntry {
  game: WishlistGame;
  bucket: WishlistBucketId;
  priority: number;
  position: number;
  note: string;
  targetPriceCents?: number;
  currency?: string;
  addedAt: string;
  updatedAt: string;
}

export interface SaveWishlistEntryInput {
  appId: number;
  bucket: WishlistBucketId;
  priority: number;
  note: string;
  targetPriceCents?: number;
  currency?: string;
  beforeAppId?: number;
}

export interface WishlistBucket {
  bucket: WishlistBucketId;
  items: WishlistEntry[];
  total: number;
}

export interface WishlistTargetTotal {
  currency: string;
  totalCents: number;
  entries: number;
}

export interface WishlistOverview {
  buckets: WishlistBucket[];
  total: number;
  /** Agregado por moneda: nunca se suman monedas distintas. */
  targetTotals: WishlistTargetTotal[];
  entriesWithoutTarget: number;
}

/** Motivo por el que un juego de Steam no llegó a la lista de Vindexa. */
export type WishlistSkipReason = "unresolved_title" | "limit_reached";

export interface SkippedWishlistGame {
  appId: number;
  /** Nombre publicado por Steam, si se pudo resolver. */
  title: string | null;
  reason: WishlistSkipReason;
}

export interface WishlistImportReport {
  /** Juegos distintos que venían en la lista de Steam. */
  fetched: number;
  imported: number;
  alreadyPresent: number;
  skipped: SkippedWishlistGame[];
  limitReached: boolean;
}

export interface SteamWishlistImportResult {
  report: WishlistImportReport;
  titlesUnresolved: number;
  /** Steam devolvió la lista vacía sin decir si lo está o está escondida. */
  visibilityUnknown: boolean;
}

export type VideoProvider = "youtube" | "steam";
export type VideoKind = "gameplay" | "review" | "impressions" | "trailer" | "guide";
export type VideoSource = "manual" | "store";

export interface GameVideo {
  appId: number;
  videoId: string;
  provider: VideoProvider;
  kind: VideoKind;
  title: string;
  channel: string;
  durationSeconds?: number;
  publishedAt?: string;
  thumbnailUrl?: string;
  source: VideoSource;
  position: number;
  createdAt: string;
  /** La construye el backend tras validar el identificador. Nunca la concatenes. */
  embedUrl?: string;
}

export interface SaveGameVideoInput {
  appId: number;
  /** URL completa o identificador: Rust extrae y valida el identificador real. */
  video: string;
  provider?: VideoProvider;
  kind?: VideoKind;
  title?: string;
  channel?: string;
  durationSeconds?: number;
  publishedAt?: string;
  thumbnailUrl?: string;
  source?: VideoSource;
}

export interface GameVideoRef {
  provider: VideoProvider;
  videoId: string;
}

export interface ArtCacheMaintenanceReport {
  removedDirectories: number;
  removedFiles: number;
  removedRows: number;
  evictedFiles: number;
  bytesAfter: number;
}

// ── Avisos y bandeja de eventos ───────────────────────────────────────────

export type NotificationKind =
  | "manual"
  | "release_date"
  | "early_access_exit"
  | "official_news"
  | "dlc_release"
  | "reminder_digest";
export type NotificationRepeatRule = "none" | "daily" | "weekly" | "monthly";
export type NotificationSeverity = "info" | "success" | "warning" | "critical";
export type NotificationInboxScope = "pending" | "unread" | "dismissed" | "all";
/**
 * Tipos que el backend asigna a los eventos derivados de señales oficiales.
 *
 * `price_drop` no tiene regla equivalente en `NotificationKind`: no lo programa
 * nadie, lo produce una observación de precio que bajó del objetivo anotado.
 */
export type NotificationEventKind =
  | NotificationKind
  | "release_date_changed"
  | "reminder_due"
  | "price_drop";

export interface NotificationRule {
  id: string;
  appId?: number;
  gameTitle?: string;
  kind: NotificationKind;
  title: string;
  body: string;
  /** Ancla: la primera cita elegida. No se reescribe al dispararse. */
  scheduledFor?: string;
  repeatRule: NotificationRepeatRule;
  leadMinutes: number;
  enabled: boolean;
  lastFiredAt?: string;
  /** Última cita ya vencida. Derivado, no persistido. */
  currentOccurrence?: string;
  /** Próxima cita pendiente: es lo que se pinta como «próximo aviso». */
  nextOccurrence?: string;
  createdAt: string;
  updatedAt: string;
}

export interface SaveNotificationRuleInput {
  id?: string;
  appId?: number;
  kind: NotificationKind;
  title: string;
  body?: string;
  scheduledFor?: string;
  repeatRule: NotificationRepeatRule;
  leadMinutes?: number;
  enabled: boolean;
}

export interface NotificationEvent {
  id: string;
  ruleId?: string;
  appId?: number;
  gameTitle?: string;
  kind: NotificationEventKind;
  severity: NotificationSeverity;
  title: string;
  body: string;
  occurredAt: string;
  readAt?: string;
  dismissedAt?: string;
  dedupeKey?: string;
}

export interface NotificationCounters {
  total: number;
  info: number;
  success: number;
  warning: number;
  critical: number;
}

export interface NotificationInboxFilter {
  scope?: NotificationInboxScope;
  severity?: NotificationSeverity;
  appId?: number;
}

export interface NotificationInbox {
  items: NotificationEvent[];
  total: number;
  limit: number;
  offset: number;
  /** Contadores globales de no leídos: no siguen al filtro, son el distintivo. */
  unread: NotificationCounters;
}

export interface DerivedEventsReport {
  earlyAccessExits: number;
  releaseDateChanges: number;
  officialNews: number;
  dueReminders: number;
  newDlc: number;
  created: number;
  skippedDuplicates: number;
}

export interface NotificationRefreshReport {
  scheduledEvents: number;
  derived: DerivedEventsReport;
  unread: NotificationCounters;
}

// ── Prioridad dinámica y modelo de gustos ─────────────────────────────────

export type TasteFacetKind = "genre" | "category" | "developer" | "publisher" | "tag";
export type TasteVerdict = "interested" | "not_interested" | "owned_already";
export type TasteSurface = "upcoming" | "library" | "discovery" | "detail";
export type UpcomingSource = "store" | "library_relation" | "manual";

export interface PrioritySignal {
  /** Clave estable en snake_case; no se traduce. */
  signal: string;
  /** Aporte a la puntuación: positivo sube, negativo baja. */
  weight: number;
  /** Frase completa en español. */
  detail: string;
}

export interface PriorityRanking {
  appId: number;
  title: string;
  score: number;
  /** La que ordena: la derivada, o la manual proyectada si está anclada. */
  effectiveScore: number;
  derivedPriority: number;
  manualPriority: number;
  locked: boolean;
  reason: string;
}

export interface PriorityRecomputeReport {
  evaluated: number;
  updated: number;
  locked: number;
  settled: number;
  signalsWritten: number;
  highlights: PriorityRanking[];
  computedAt: string;
}

export interface PriorityExplanation {
  appId: number;
  title: string;
  score: number;
  effectiveScore: number;
  derivedPriority: number;
  manualPriority: number;
  locked: boolean;
  reason: string;
  computedAt: string | null;
  /** «Tu prioridad manual dice 5; las señales dicen 2. Manda la tuya.» */
  manualOverride: string | null;
  /** Ordenadas por peso absoluto descendente. */
  signals: PrioritySignal[];
}

export interface TasteFacet {
  facet: TasteFacetKind;
  facetLabel: string;
  value: string;
  weight: number;
  positiveSamples: number;
  negativeSamples: number;
}

export interface TasteReport {
  gamesAnalyzed: number;
  dismissedUpcomingUsed: number;
  facetsLearned: number;
  positiveFacets: number;
  negativeFacets: number;
  highlights: TasteFacet[];
  computedAt: string;
}

export interface UpcomingRelease {
  appId: number;
  title: string;
  capsuleUrl: string | null;
  headerUrl: string | null;
  releaseDate: string | null;
  releaseDateIsExact: boolean;
  genres: string[];
  categories: string[];
  developer: string | null;
  publisher: string | null;
  shortDescription: string | null;
  matchScore: number;
  /** Nunca nombra una faceta que el juego no tenga. */
  matchReason: string;
  source: UpcomingSource;
  dismissedAt: string | null;
  discoveredAt: string;
  updatedAt: string;
}

// --- Tiendas externas (Epic Games Store y GOG) ------------------------------

/** Allowlist cerrada: coincide con el `CHECK` de la migración 025. */
export type ExternalStoreId = "epic" | "gog";

export type ExternalScanStatus = "success" | "failed" | "unavailable";

/** Quién decidió el emparejado que hay guardado. */
export type ExternalMatchSource = "none" | "automatic" | "manual";

/**
 * Lo que Vindexa reconoce de una tienda en este equipo, antes de leer ningún
 * manifiesto. `searchedPaths` viaja siempre: decir «no se encontró» sin decir
 * dónde se buscó no es información, es un hueco.
 */
export interface StoreDetection {
  store: ExternalStoreId;
  displayName: string;
  detected: boolean;
  detectedPaths: string[];
  searchedPaths: string[];
}

export interface ExternalStoreAccount {
  store: ExternalStoreId;
  displayName: string | null;
  detectedRoot: string | null;
  linked: boolean;
  lastScanAt: string | null;
  lastScanStatus: ExternalScanStatus | null;
  lastScanErrorCode: string | null;
  lastScanErrorMessage: string | null;
  gameCount: number;
}

export interface ExternalGame {
  store: ExternalStoreId;
  externalId: string;
  title: string;
  coverUrl: string | null;
  headerUrl: string | null;
  installPath: string | null;
  installed: boolean;
  sizeOnDisk: number | null;
  launchTarget: string | null;
  drmState: DrmState;
  /** Por qué se afirma ese estado. Es un dato de ficha: no va sobre la carátula. */
  drmEvidence: DrmEvidence[];
  matchedAppId: number | null;
  matchedTitle: string | null;
  matchConfidence: number;
  matchSource: ExternalMatchSource;
  discoveredAt: string;
  updatedAt: string;
}

export type ExternalGameSort =
  | "alphabetical"
  | "alphabeticalDesc"
  | "installed"
  | "sizeDesc"
  | "discoveredDesc"
  | "updatedDesc";

export interface ExternalGameRequest {
  store?: ExternalStoreId;
  query?: string;
  installedOnly?: boolean;
  /** `true` sólo emparejados, `false` sólo sin emparejar. */
  matched?: boolean;
  drmFreeOnly?: boolean;
  sort?: ExternalGameSort;
  limit?: number;
  offset?: number;
}

export interface PagedExternalGames {
  items: ExternalGame[];
  total: number;
  limit: number;
  offset: number;
}

export interface ExternalStoreScanReport {
  store: ExternalStoreId;
  status: ExternalScanStatus;
  detectedRoot: string | null;
  discovered: number;
  matched: number;
  /** Entradas descartadas por corruptas, vacías o inseguras. */
  skipped: number;
  /** Juegos que dejaron de estar instalados porque su manifiesto ya no está. */
  expired: number;
  sources: string[];
  errorCode: string | null;
  errorMessage: string | null;
}

/* --- Precio observado ------------------------------------------------------ */

/**
 * Vigencia de una observación de precio. No es un booleano porque la interfaz
 * necesita distinguir «recién mirado» de «hace días» sin fijar ella el umbral.
 * `unknown` significa que el sello no se pudo interpretar: no se finge una edad.
 */
export type PriceFreshness = "fresh" | "aging" | "stale" | "unknown";
export type PriceSource = "steam_store" | "manual";

/**
 * Último precio conocido de un juego en una moneda. Nunca viaja sin moneda ni
 * sin `observedAt`: un importe sin las dos cosas no se puede presentar como un
 * precio.
 */
export interface GamePrice {
  appId: number;
  currency: string;
  /** País con el que se consultó; sin él la moneda no es reproducible. */
  countryCode?: string;
  finalCents: number;
  /** Precio de referencia sin descuento, tal y como lo publica la tienda. */
  initialCents: number;
  discountPercent: number;
  /** Mínimo observado por Vindexa, no el mínimo histórico real del juego. */
  lowestCents: number;
  lowestObservedAt: string;
  /** Cuándo se vio por primera vez el importe vigente. */
  changedAt: string;
  /** Cuándo se confirmó por última vez. */
  observedAt: string;
  source: PriceSource;
  freshness: PriceFreshness;
  ageMinutes?: number;
}

export interface PricePoint {
  observedAt: string;
  finalCents: number;
  initialCents: number;
  discountPercent: number;
  source: PriceSource;
}

export interface PriceHistory {
  appId: number;
  currency: string;
  points: PricePoint[];
}

/** Situación de precio de una entrada de deseados. */
export interface WishlistPriceStatus {
  appId: number;
  targetCents?: number;
  targetCurrency?: string;
  price?: GamePrice;
  /** Monedas observadas distintas de la del precio mostrado. */
  otherCurrencies: string[];
  /** Sólo cierto con objetivo y precio en la misma moneda. */
  comparable: boolean;
  /** `final - objetivo`. Negativo significa por debajo del objetivo. */
  differenceCents?: number;
  meetsTarget: boolean;
}

export interface PriceRefreshReport {
  observed: number;
  changed: number;
  alerts: number;
  /** Fichas sin bloque de precio: desconocido, que no es cero. */
  withoutPrice: number;
  failed: number;
}

/* --- Archivado ------------------------------------------------------------- */

export type ArchiveScope = "active" | "archived" | "all";

export interface ArchivedGame {
  game: GameSummary;
  reason: string;
  archivedAt: string;
}

export interface PagedArchivedGames {
  items: ArchivedGame[];
  total: number;
  limit: number;
  offset: number;
}

export interface ArchiveReport {
  requested: number;
  changed: number;
  /** Los que ya estaban así. No es un fallo: es un no-cambio. */
  unchanged: number;
  /** Afectados que siguen en el plan; archivar no los saca. */
  inPlanner: number;
  archivedTotal: number;
}

/**
 * Vínculo con la sesión de Steam que autoriza los servicios de Familia.
 *
 * `linked` sólo dice que hay un testigo guardado, no que siga sirviendo: eso
 * únicamente lo sabe Steam y comprobarlo costaría una petición. La caducidad
 * aparece al usarlo, y entonces llega en `lastErrorCode`.
 */
export interface FamilySessionStatus {
  linked: boolean;
  /** Cuándo terminó bien la última sincronización. */
  lastSyncAt?: string;
  /** Cuántos juegos trajo esa sincronización. */
  lastAppCount?: number;
  /** Código del último fallo, si el último intento falló. */
  lastErrorCode?: string;
}

/** Lo que deja una sincronización del catálogo de Familia. */
export interface FamilySyncReport {
  /** Juegos del catálogo compartido que se han guardado. */
  imported: number;
  /** Entradas que Steam devolvió sin AppID utilizable. */
  unusable: number;
  /** Entradas sin título publicado: sin nombre no hay ficha honesta. */
  withoutTitle: number;
  /** La cuenta no pertenece a ninguna Familia. No es un fallo. */
  noFamily: boolean;
}
