use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDefinition {
    pub id: String,
    pub name: String,
    pub color: String,
    pub position: i64,
    pub built_in: bool,
    pub game_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub color: String,
    pub icon: String,
    pub kind: String,
    pub match_mode: String,
    pub position: i64,
    pub game_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartRule {
    pub id: Option<String>,
    pub group_id: i64,
    pub field: String,
    pub operator: String,
    pub value: serde_json::Value,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveCollectionInput {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub color: String,
    pub icon: String,
    pub kind: String,
    pub match_mode: String,
    pub rules: Vec<SmartRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerItem {
    pub app_id: u32,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
    pub progress: u8,
    pub position: i64,
    pub queue_position: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_for: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerSettings {
    pub weekly_capacity_minutes: u32,
    pub monthly_capacity_minutes: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerOverview {
    pub columns: Vec<PlannerColumn>,
    pub queue: Vec<PlannerItem>,
    pub settings: PlannerSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannerColumn {
    pub id: String,
    pub name: String,
    pub color: String,
    pub position: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wip_limit: Option<u32>,
    pub items: Vec<PlannerItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamAccount {
    pub steam_id: String,
    pub persona_name: Option<String>,
    pub avatar_url: Option<String>,
    pub profile_url: Option<String>,
    pub visibility: Option<u8>,
    pub last_sync_at: Option<String>,
    pub last_sync_status: Option<String>,
    pub last_sync_error_code: Option<String>,
    pub last_sync_error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamConfiguration {
    pub account: Option<SteamAccount>,
    pub api_key_configured: bool,
    pub api_key_verification_required: bool,
    pub local_steam_detected: bool,
    pub local_manifest_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryStats {
    pub total_games: i64,
    pub installed_games: i64,
    pub playing_games: i64,
    pub backlog_games: i64,
    pub tracked_games: i64,
    pub total_playtime_minutes: i64,
    /// Juegos archivados. No se suman a `total_games`: el total tiene que
    /// coincidir con lo que se ve. Este recuento existe para que el archivado
    /// nunca sea un agujero negro y la interfaz pueda ofrecer verlos.
    pub archived_games: i64,
    /// Juegos que la tienda oficial declara sin DRM.
    ///
    /// Es el recuento del acceso directo de la barra lateral. No incluye lo que
    /// aún no se ha comprobado: `unknown` es «no se sabe», no «lleva DRM».
    pub drm_free_games: i64,
    /// Juegos a los que todavía no se ha preguntado por su DRM.
    ///
    /// El repaso va por tandas y tarda horas en una biblioteca grande. Sin esta
    /// cifra al lado, «Sin DRM 604» se lee como el total cuando es un avance.
    pub drm_pending_games: i64,
    /// Juegos visibles en el catálogo de Steam Family.
    ///
    /// No se suman a `total_games`, porque catálogo visible no es licencia y
    /// mezclarlos diría que se tienen. Pero el recuento viaja siempre: sin él
    /// la barra lateral enseñaba «Steam Family» sin número y parecía que esos
    /// juegos no estaban, cuando estaban a un clic.
    pub family_catalog_games: i64,
    /// Juegos por tienda vinculada, indexados por su identificador.
    ///
    /// Igual que el catálogo de Family, no se suman a `total_games`: son juegos
    /// de otra tienda. El recuento viaja siempre para que la barra lateral pueda
    /// enseñar dónde están sin obligar a entrar en cada sección.
    pub external_store_games: std::collections::BTreeMap<String, i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    pub density: String,
    pub periodic_sync_minutes: u32,
    pub confirm_uninstall: bool,
    pub library_sort: String,
    /// Techo en disco de la caché de arte, en mebibytes. Cero —lo normal—
    /// significa sin techo: el arte de la biblioteca se queda entero en el
    /// disco y sólo lo desaloja quedarse sin espacio libre de verdad.
    #[serde(default = "default_art_cache_mib")]
    pub art_cache_mib: u32,
    #[serde(default)]
    pub shortcuts: ShortcutBindings,
}

/// Sin techo: la caché de arte crece con la biblioteca.
///
/// Es el valor de fábrica y no hay ninguna razón para que sea otro. Vindexa es
/// una aplicación local: el arte vive en el disco de quien la usa, y ponerle un
/// número arbitrario sólo servía para que su propia biblioteca no cupiera en su
/// propio disco. Tampoco crece sin fin: lo acota el número de juegos que se
/// tienen.
///
/// El único límite real es físico, y lo vigila la caché: nunca se come el
/// margen de disco libre que el sistema necesita para funcionar.
pub const UNLIMITED_ART_CACHE_MIB: u32 = 0;
pub const DEFAULT_ART_CACHE_MIB: u32 = UNLIMITED_ART_CACHE_MIB;
/// Techo mínimo que se puede fijar a mano. Por debajo, la biblioteca se
/// quedaría sin portadas.
pub const MIN_ART_CACHE_MIB: u32 = 128;
/// Techo máximo que se puede fijar a mano.
pub const MAX_ART_CACHE_MIB: u32 = 65_536;

/// ¿Es un techo de caché de arte que se pueda guardar?
///
/// Cero es válido y es lo habitual: significa que no hay techo.
pub fn is_valid_art_cache_mib(mib: u32) -> bool {
    mib == UNLIMITED_ART_CACHE_MIB || (MIN_ART_CACHE_MIB..=MAX_ART_CACHE_MIB).contains(&mib)
}

/// Primer identificador que Vindexa asigna por su cuenta.
///
/// La biblioteca se indexa por AppID de Steam, y un juego de Epic, de GOG o de
/// itch.io no tiene ninguno. Para que pueda organizarse como los demás se le da
/// uno local a partir de aquí: Steam anda por los siete millones largos y
/// numera de forma creciente, así que no alcanzará este tramo.
///
/// Sirve además de frontera: por encima de este número, **el juego no existe
/// para Steam**. Ninguna consulta a su API —fichas, precios, logros, arte— debe
/// llevar uno de estos identificadores, porque no significa nada allí.
pub const LOCAL_APP_ID_BASE: u32 = 2_000_000_000;

/// ¿Este identificador lo asignó Vindexa en vez de Steam?
pub fn is_local_app_id(app_id: u32) -> bool {
    app_id >= LOCAL_APP_ID_BASE
}

fn default_art_cache_mib() -> u32 {
    DEFAULT_ART_CACHE_MIB
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutBindings {
    pub library: String,
    pub planner: String,
    /// Sección de deseados. Lleva `default` para que una preferencia guardada
    /// antes de que existiera la sección siga deserializando.
    #[serde(default = "default_wishlist_shortcut")]
    pub wishlist: String,
    pub collections: String,
    pub tracking: String,
    pub search: String,
    pub sync: String,
    pub close_panel: String,
}

fn default_wishlist_shortcut() -> String {
    "Mod+5".into()
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            library: "Mod+1".into(),
            planner: "Mod+2".into(),
            wishlist: "Mod+5".into(),
            collections: "Mod+3".into(),
            tracking: "Mod+4".into(),
            search: "Mod+F".into(),
            sync: "Mod+Shift+S".into(),
            close_panel: "Escape".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub status: String,
    pub current_version: String,
    pub available_version: Option<String>,
    pub message: String,
    /// Dónde se descargan los instaladores. Viaja con el resultado para que la
    /// interfaz no repita la dirección escrita a mano.
    pub release_page: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrap {
    /// Versión que se está ejecutando. Viaja desde el paquete para que la
    /// interfaz no tenga que repetirla escrita a mano, que es como acabó
    /// enseñando una que ya no era la suya.
    pub app_version: String,
    pub stats: LibraryStats,
    pub statuses: Vec<StatusDefinition>,
    pub collections: Vec<CollectionSummary>,
    pub planner: Vec<PlannerColumn>,
    pub steam: SteamConfiguration,
    pub preferences: AppPreferences,
    pub database_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    pub app_id: u32,
    pub title: String,
    pub icon_url: Option<String>,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub playtime_minutes: u64,
    pub playtime_recent_minutes: u64,
    pub last_played_at: Option<String>,
    pub release_date: Option<String>,
    pub is_early_access: bool,
    pub is_free: bool,
    pub steam_deck_status: Option<String>,
    /// Sistemas en los que la tienda ofrece el juego. `None` significa que aún
    /// no se sabe: **no es lo mismo que «no compatible»**, y la interfaz no
    /// puede desaconsejar una instalación sin el dato.
    pub platform_windows: Option<bool>,
    pub platform_mac: Option<bool>,
    pub platform_linux: Option<bool>,
    pub achievements_unlocked: Option<u32>,
    pub achievements_total: Option<u32>,
    pub ownership_source: String,
    pub family_availability: String,
    pub installed: bool,
    pub install_path: Option<String>,
    pub size_on_disk: Option<u64>,
    pub status_id: String,
    pub status_name: String,
    pub status_color: String,
    pub progress: u8,
    pub priority: u8,
    pub pinned: bool,
    pub tracking: bool,
    pub rating: Option<u8>,
    pub estimated_minutes: Option<u32>,
    pub target_date: Option<String>,
    pub next_action: Option<String>,
    pub checkpoint: Option<String>,
    pub notes: Option<String>,
    pub manual_position: i64,
    /// Colecciones manuales a las que pertenece. Se resuelve en la propia
    /// consulta de biblioteca para que el menú contextual pueda marcar la
    /// pertenencia sin una petición adicional por juego.
    pub collection_ids: Vec<String>,
    /// Géneros declarados por la tienda. Permiten agrupar la lista sin pedir la
    /// ficha completa de cada juego.
    pub genres: Vec<String>,
    pub developer: Option<String>,
    /// Tienda que vendió el juego cuando no es Steam: `epic`, `gog` o `itch`.
    ///
    /// Sin esto, la ficha de un juego de Epic decía «STEAM · APP 2000000002»:
    /// un identificador que Vindexa se inventa para poder organizarlo y que en
    /// Steam no significa nada. Enseñarlo era afirmar algo falso.
    pub external_store: Option<String>,
    /// Cómo se protege el juego: `drm_free`, `third_party_drm`, `steam_drm` o
    /// `unknown`.
    ///
    /// Viaja en el resumen —y no sólo en la ficha— para que la lista pueda
    /// señalar lo que no depende de ninguna tienda sin abrir juego por juego.
    /// Nunca se pinta sobre la carátula: una carátula es del juego, no de cómo
    /// se distribuye.
    pub drm_state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSession {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub progress_before: Option<u8>,
    pub progress_after: Option<u8>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedGameSessions {
    pub items: Vec<GameSession>,
    pub total: u64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityItem {
    pub id: String,
    pub kind: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameDetail {
    #[serde(flatten)]
    pub summary: GameSummary,
    pub hero_url: Option<String>,
    pub short_description: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub metadata_status: String,
    pub metadata_fetched_at: Option<String>,
    pub achievements_status: String,
    pub achievements_fetched_at: Option<String>,
    pub collection_ids: Vec<String>,
    pub tags: Vec<String>,
    pub tag_ids: Vec<String>,
    pub sessions: Vec<GameSession>,
    pub sessions_total: u64,
    pub activity: Vec<ActivityItem>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub abandoned_at: Option<String>,
    /// Metadatos enriquecidos de tienda. Viajan con la ficha para que abrirla
    /// sea una sola petición y el contenido no salte al terminar una segunda.
    #[serde(flatten)]
    pub rich: RichGameMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFilterChoice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFilterOptions {
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub tags: Vec<LibraryFilterChoice>,
    pub total_games: u64,
    pub metadata_games: u64,
    pub achievement_games: u64,
    pub steam_deck_games: u64,
    /// Juegos con el DRM ya comprobado. Sin ninguno, el filtro se ofrece
    /// apagado: filtrar por un dato que nadie tiene sólo devuelve una lista
    /// vacía y parece que la biblioteca está rota.
    pub drm_games: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataEnrichmentStatus {
    pub running: bool,
    pub total_games: u64,
    pub fresh_metadata: u64,
    pub queued: u64,
    pub processing: u64,
    pub retrying: u64,
    pub succeeded: u64,
    pub unavailable: u64,
    pub failed: u64,
    pub next_retry_at: Option<String>,
    pub last_error_code: Option<String>,
    pub steam_deck_availability: String,
    pub steam_deck_explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GameListRequest {
    pub query: Option<String>,
    pub status_id: Option<String>,
    pub collection_id: Option<String>,
    pub installed: Option<bool>,
    pub tracking: Option<bool>,
    pub early_access: Option<bool>,
    pub is_free: Option<bool>,
    pub ownership_source: Option<String>,
    /// Ámbito de una tienda que no es Steam: `epic`, `gog` o `itch`.
    ///
    /// Devuelve **todos** los juegos que esa tienda te ha vendido, incluidos los
    /// que además tienes en Steam. Tener un juego en dos tiendas no lo convierte
    /// en dos juegos, pero sí tiene que aparecer al mirar cualquiera de ellas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_store: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub never_played: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_playtime_minutes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_playtime_minutes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rating: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rating: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_achievement_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_achievement_percent: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_deck_status: Option<String>,
    /// Estado de DRM: `drm_free`, `third_party_drm`, `steam_drm` o `unknown`.
    ///
    /// Existe para poder ver de un vistazo qué parte de la biblioteca se puede
    /// jugar sin depender de ninguna tienda. Va aquí y no en la carátula porque
    /// una carátula es del juego, no de cómo se distribuye.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drm_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_date_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_session_minutes: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_session_minutes: Option<u32>,
    /// Ámbito de archivado: `active` (predeterminado), `archived` o `all`.
    /// Sin él la biblioteca esconde lo archivado, que es el sentido de
    /// archivar; con `archived` se consulta lo escondido de forma explícita.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_scope: Option<String>,
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_seed: Option<u32>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// Ámbitos admitidos por `GameListRequest::archive_scope`.
///
/// `active` es el predeterminado y es lo que hace que archivar signifique algo.
/// `archived` es el escondite consultable. `all` existe para poder comprobar
/// que archivar no ha perdido nada.
pub const ARCHIVE_SCOPES: [&str; 3] = ["active", "archived", "all"];

pub fn is_valid_archive_scope(value: &str) -> bool {
    ARCHIVE_SCOPES.contains(&value)
}

/// Qué juegos existen en la tienda de Steam.
///
/// Es la pregunta que hay detrás de tres cosas distintas: si se le puede pedir
/// la ficha, si se le puede preguntar por su DRM y si cuenta como pendiente de
/// una u otra. Un juego de Epic o de itch.io no está en Steam —su identificador
/// se lo inventó Vindexa— y GOG llega ya marcado sin DRM, porque su catálogo
/// entero lo es y así lo declara.
///
/// Se escribe una vez porque, cuando eran tres condiciones parecidas, cada una
/// dejaba fuera cosas distintas: la biblioteca prometía comprobar 274 juegos
/// que nadie iba a preguntar nunca, y abrir la ficha de un juego de Epic le
/// pedía a Steam un AppID inventado.
///
/// Vive aquí, junto a `archive_scope_clause`, por lo mismo que aquélla: las
/// pruebas de integración compilan `db/library.rs` con `models.rs` y sin el
/// resto de `db`.
///
/// La tabla `games` tiene que estar aliasada como `g`.
pub const ES_DE_STEAM: &str = "(g.external_store IS NULL OR g.external_store = '')";

/// Cláusula que filtra la consulta de biblioteca según el ámbito. `None`
/// significa «no filtres», que es exactamente lo que pide `all`.
///
/// Vive junto a `GameListRequest` y no en `db::archive` porque las pruebas de
/// integración compilan `db/library.rs` con `models.rs` y sin el resto de `db`.
pub fn archive_scope_clause(scope: &str) -> Option<&'static str> {
    match scope {
        "archived" => Some("EXISTS (SELECT 1 FROM game_archive a WHERE a.app_id = g.app_id)"),
        "all" => None,
        _ => Some("NOT EXISTS (SELECT 1 FROM game_archive a WHERE a.app_id = g.app_id)"),
    }
}

pub fn is_valid_game_sort(value: &str) -> bool {
    matches!(
        value,
        "manual"
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
            | "random"
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PagedGames {
    pub items: Vec<GameSummary>,
    pub total: i64,
    pub limit: u32,
    pub offset: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGameInput {
    pub app_id: u32,
    pub status_id: String,
    pub progress: u8,
    pub priority: u8,
    pub pinned: bool,
    pub tracking: bool,
    pub rating: Option<u8>,
    pub estimated_minutes: Option<u32>,
    pub target_date: Option<String>,
    pub next_action: Option<String>,
    pub checkpoint: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkUpdateStatusInput {
    pub app_ids: Vec<u32>,
    pub status_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovePlannerItemInput {
    pub app_id: u32,
    pub column_id: String,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavePlannerItemInput {
    pub app_id: u32,
    pub objective: Option<String>,
    pub planned_for: Option<String>,
    pub target_date: Option<String>,
    pub estimated_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSteamImportResult {
    pub steam_path: String,
    pub libraries_scanned: usize,
    pub imported_games: usize,
    pub updated_games: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamSyncResult {
    pub steam_id: String,
    pub imported_games: usize,
    pub updated_games: usize,
    pub private_library_suspected: bool,
    pub family_members_detected: usize,
    pub family_members_inaccessible: usize,
    pub family_games_imported: usize,
    pub family_catalog_games_detected: usize,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRun {
    pub id: String,
    pub source: String,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub imported_count: i64,
    pub updated_count: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendationRequest {
    pub duration_minutes: Option<u32>,
    pub mood: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommendation {
    pub history_id: String,
    pub game: GameSummary,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseDiagnostics {
    pub path: String,
    pub size_bytes: u64,
    pub schema_version: i64,
    pub integrity: String,
    pub wal_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRecoveryIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuarantinedDatabaseSummary {
    pub id: String,
    pub detected_at: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub sidecar_count: usize,
    pub integrity: String,
    pub schema_version: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBackupSummary {
    pub id: String,
    pub label: String,
    pub size_bytes: u64,
    pub modified_at: Option<String>,
    pub source: String,
    pub valid: bool,
    pub validation_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseRecoverySnapshot {
    pub required: bool,
    pub issue: Option<DatabaseRecoveryIssue>,
    pub quarantine: Option<QuarantinedDatabaseSummary>,
    pub backups: Vec<RecoveryBackupSummary>,
    pub recovery_actions_available: bool,
}

// ── Contrato de ficha enriquecida ──────────────────────────────────────────
// Estos tipos son parte del contrato con la interfaz, así que viven aquí y no
// en `db::rich_metadata`, que conserva su lógica de saneado y persistencia.
// `models.rs` se compila aislado en las pruebas de contrato: no puede depender
// de ningún otro módulo del proyecto.

/// Un bloque de la descripción ya convertido a texto plano seguro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DescriptionBlock {
    /// Encabezado con su nivel original acotado a 1..=6.
    Heading { level: u8, text: String },
    /// Párrafo de texto plano.
    Paragraph { text: String },
    /// Lista ordenada o sin ordenar de textos planos.
    List { ordered: bool, items: Vec<String> },
}

/// Descripción completa de un juego como secuencia de bloques seguros.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuredDescription {
    pub blocks: Vec<DescriptionBlock>,
}

/// Clasificación de DRM derivada solo de señales oficiales de la tienda.
///
/// Los valores serializados coinciden exactamente con el `CHECK` de la
/// migración 018 y con la convención del proyecto para valores de enumeración
/// (`ownership_source = 'family_shared'`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrmState {
    /// No hay señal suficiente. Es el valor por defecto: nunca se adivina.
    #[default]
    Unknown,
    /// La tienda no declara DRM de terceros ni cuenta externa.
    DrmFree,
    /// La tienda declara un DRM o lanzador de terceros.
    ThirdPartyDrm,
    /// La tienda declara Steamworks DRM o el cliente de Steam.
    SteamDrm,
}

/// Señal exacta que motivó una clasificación, para que la ficha pueda
/// justificarla ante el usuario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrmEvidence {
    /// Campo oficial del que procede la señal (`drmNotice`, `legalNotice`…).
    pub source: String,
    /// Texto exacto o etiqueta canónica de la coincidencia.
    #[serde(rename = "match")]
    pub matched: String,
}

/// Estado de DRM más las evidencias que lo sostienen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrmAssessment {
    pub state: DrmState,
    pub evidence: Vec<DrmEvidence>,
}

/// Recuento de juegos por estado de DRM, para los filtros de biblioteca.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrmStateCounts {
    pub unknown: i64,
    pub drm_free: i64,
    pub third_party_drm: i64,
    pub steam_drm: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GameMediaKind {
    Screenshot,
    Movie,
}

/// Una captura o un vídeo oficial del juego.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameMediaItem {
    /// Identificador estable dentro del juego (`screenshot:12`, `movie:2565`).
    pub media_id: String,
    pub kind: GameMediaKind,
    pub thumbnail_url: Option<String>,
    pub full_url: Option<String>,
    /// Codificación alternativa del mismo recurso (por ejemplo WebM frente a MP4).
    pub alt_url: Option<String>,
    /// Orden declarado por la tienda dentro de su tipo.
    pub position: u32,
}

/// Colocación del logotipo sobre el hero, tal y como la publica Steam para su
/// propia biblioteca. `appdetails` no la expone: la columna queda a `NULL`
/// mientras ninguna fuente oficial la aporte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogoPosition {
    pub pinned_position: String,
    pub width_pct: f64,
    pub height_pct: f64,
}

/// Lectura completa para la ficha inmersiva.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RichGameMetadata {
    pub app_id: u32,
    pub detailed_description: Option<StructuredDescription>,
    pub about_the_game: Option<StructuredDescription>,
    pub supported_languages: Option<String>,
    pub website_url: Option<String>,
    pub metacritic_score: Option<u8>,
    pub metacritic_url: Option<String>,
    pub required_age: Option<u32>,
    pub controller_support: Option<String>,
    pub background_url: Option<String>,
    pub library_hero_url: Option<String>,
    pub library_logo_url: Option<String>,
    pub logo_position: Option<LogoPosition>,
    pub drm_notice: Option<String>,
    pub drm: DrmAssessment,
    pub drm_checked_at: Option<String>,
    pub screenshots: Vec<GameMediaItem>,
    pub movies: Vec<GameMediaItem>,
}
