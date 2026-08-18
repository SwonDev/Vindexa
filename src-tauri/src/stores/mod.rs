//! Vinculación con tiendas externas: Epic Games Store y GOG.
//!
//! # Las dos vías, y cuál manda
//!
//! Vindexa conoce la biblioteca de estas tiendas por dos caminos
//! independientes, y ninguno depende del otro:
//!
//! 1. **La cuenta.** La persona usuaria inicia sesión en la propia Vindexa
//!    ([`online`]) y se lee su biblioteca completa desde la API de la tienda.
//!    Es la vía principal y la única que funciona en un equipo sin ningún
//!    cliente instalado. El testigo vive en el llavero de macOS y en ningún
//!    otro sitio (ver [`secrets`]); **jamás** en SQLite, en un fichero, en un
//!    registro ni en la interfaz.
//! 2. **El disco.** Si en este equipo hay clientes de esas tiendas, se leen sus
//!    manifiestos. Es lo único que sabe qué está **descargado**, así que sigue
//!    haciendo falta aunque haya sesión: la API dice qué se posee, no qué se ha
//!    instalado. Los dos orígenes se distinguen con [`StoreOrigin`] justamente
//!    para que sincronizar la cuenta no borre lo que sabe el disco.
//!
//! Los orígenes locales que se reconocen son:
//!
//! * Epic Games Launcher → manifiestos `*.item` (JSON) de su carpeta `Data/Manifests`.
//! * Legendary / Heroic → `installed.json` (Epic sin el cliente oficial).
//! * Legendary → los `metadata/<AppName>.json` que guarda tras iniciar sesión,
//!   que describen la biblioteca **completa**, no sólo lo instalado.
//! * Heroic → `store_cache/legendary_library.json` y `store_cache/gog_library.json`,
//!   que también son la biblioteca completa de cada tienda.
//! * GOG Galaxy → su base `galaxy-2.0.db`, leída **sobre una copia temporal**.
//! * GOG sin Galaxy → los `goggame-<id>.info` que el instalador deja en la
//!   carpeta de cada juego.
//! * Heroic → `gog_store/installed.json`.
//!
//! Esas cachés de biblioteca son un respaldo, no el camino recomendado: sólo
//! existen si la persona usuaria ya tenía otro cliente y había iniciado sesión
//! en él. Quien no lo tenga obtiene su catálogo entero por la vía de la cuenta.
//! Los ficheros hermanos que **sí** guardan tokens de otros programas
//! (`gog_store/auth.json` y `legendaryConfig/legendary/user.json`) no se leen
//! nunca: Vindexa usa su propia sesión, no la de nadie más. De ellos sólo se
//! comprueba la existencia, para poder distinguir «no has iniciado sesión» de
//! «no tienes el cliente».
//!
//! Si un cliente no está instalado, el resultado es el estado explícito
//! [`ScanStatus::Unavailable`] con su motivo en español; nunca una lista vacía
//! indistinguible de «no tienes juegos» ni un error ruidoso.
//!
//! # Nunca se inventa un dato
//!
//! Un campo que el origen no trae se queda en `None`. Los manifiestos de Epic
//! no llevan carátula, y su catálogo sólo la publica para algunos títulos: en
//! ambos casos `cover_url` y `header_url` quedan vacías en vez de fabricarse a
//! partir de un patrón de URL adivinado. El emparejado con la biblioteca
//! de Steam solo se escribe cuando hay evidencia suficiente (ver [`matching`]);
//! por debajo del umbral, `matched_app_id` es `NULL`.
//!
//! # La marca DRM-Free es un dato de ficha, no un adorno de carátula
//!
//! Todo juego comprado en GOG es DRM-free por política publicada de la tienda,
//! así que se marca [`DrmState::DrmFree`] con la evidencia «catálogo GOG». Epic
//! no lo es por defecto: queda en [`DrmState::Unknown`] salvo evidencia.
//!
//! **Esta marca no debe aparecer nunca sobre la carátula del juego.** Es un dato
//! de ficha, se muestra dentro del detalle acompañado de su evidencia, igual que
//! la marca equivalente de Steam (ver `crate::steam::drm`). No diseñes
//! insignias, superposiciones ni bordes de portada a partir de este campo.

pub mod db;
pub mod epic;
pub mod epic_api;
pub mod gog;
pub mod gog_api;
pub(crate) mod heroic;
pub mod itch;
pub mod launch;
pub mod login_window;
pub mod matching;
pub(crate) mod net;
pub mod online;
pub(crate) mod paths;
#[cfg(test)]
mod scan_tests;
pub mod secrets;

use crate::db::rich_metadata::{DrmEvidence, DrmState};
use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

/// Tope de juegos que se aceptan de un único escaneo. Un manifiesto manipulado
/// no puede convertir una lectura local en una escritura ilimitada.
pub const MAX_DISCOVERED_GAMES: usize = 20_000;

/// Longitud máxima de un título aceptado desde un manifiesto de terceros.
pub const MAX_TITLE_CHARS: usize = 200;

/// Longitud máxima de una ruta de instalación persistida.
pub const MAX_PATH_CHARS: usize = 4_000;

/// Fuente de evidencia que se registra al marcar un juego de GOG como DRM-free.
pub const GOG_CATALOGUE_EVIDENCE: &str = "catálogo GOG";

// ---------------------------------------------------------------------------
// Identidad de tienda
// ---------------------------------------------------------------------------

/// Las dos únicas tiendas externas que reconoce el esquema (`CHECK` de la
/// migración 025). Es una allowlist cerrada: el frontend no puede introducir
/// una tienda nueva por el nombre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalStore {
    Epic,
    Gog,
}

impl ExternalStore {
    pub const ALL: [Self; 2] = [Self::Epic, Self::Gog];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Epic => "epic",
            Self::Gog => "gog",
        }
    }

    /// Nombre visible de la tienda. Es una constante del producto, no un dato
    /// leído de la cuenta de la persona usuaria: Vindexa nunca conoce su perfil
    /// de Epic ni de GOG.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Epic => "Epic Games Store",
            Self::Gog => "GOG",
        }
    }

    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "epic" => Ok(Self::Epic),
            "gog" => Ok(Self::Gog),
            _ => Err(AppError::validation("La tienda externa no es válida.")),
        }
    }

    /// Estado de DRM que la política publicada de la tienda permite afirmar sin
    /// inspeccionar nada más.
    ///
    /// GOG vende **solo** catálogo sin DRM y lo declara públicamente, así que la
    /// pertenencia al catálogo es evidencia suficiente. Epic no publica ninguna
    /// política equivalente: sus juegos pueden traer DRM de terceros o exigir
    /// Epic Online Services, de modo que la respuesta honesta es «no se sabe».
    pub fn catalogue_drm_state(self) -> DrmState {
        match self {
            Self::Epic => DrmState::Unknown,
            Self::Gog => DrmState::DrmFree,
        }
    }

    /// Evidencia que acompaña a [`Self::catalogue_drm_state`]. La ficha siempre
    /// muestra el porqué de la marca; nunca una insignia sin justificación.
    pub fn catalogue_drm_evidence(self) -> Vec<DrmEvidence> {
        match self {
            Self::Epic => Vec::new(),
            Self::Gog => vec![DrmEvidence::new("storeCatalogue", GOG_CATALOGUE_EVIDENCE)],
        }
    }
}

// ---------------------------------------------------------------------------
// Resultado de un escaneo
// ---------------------------------------------------------------------------

/// Estado de la última lectura local. Coincide con el `CHECK` de
/// `external_store_accounts.last_scan_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanStatus {
    /// Se leyó al menos un origen local y el resultado es fiable.
    Success,
    /// El cliente no está instalado en esta máquina. No es un error.
    Unavailable,
    /// El cliente está, pero su información no se pudo leer con seguridad.
    Failed,
}

impl ScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

/// Origen concreto del que salió la información, para que la ficha pueda
/// explicar de dónde viene cada juego sin inventar procedencia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanSource {
    /// Manifiestos `*.item` del Epic Games Launcher oficial.
    EpicManifests,
    /// `installed.json` de Legendary (o del Legendary que empaqueta Heroic).
    LegendaryInstalled,
    /// Base `galaxy-2.0.db` de GOG Galaxy, leída sobre una copia temporal.
    GogGalaxyDatabase,
    /// Ficheros `goggame-<id>.info` en la carpeta de cada juego instalado.
    GogGameInfo,
    /// `gog_store/installed.json` de Heroic.
    HeroicGogInstalled,
    /// `metadata/<AppName>.json` de Legendary: la biblioteca completa de Epic.
    LegendaryMetadata,
    /// `store_cache/legendary_library.json` de Heroic: biblioteca completa de Epic.
    HeroicEpicLibrary,
    /// `store_cache/gog_library.json` de Heroic: biblioteca completa de GOG.
    HeroicGogLibrary,
    /// La cuenta de Epic, leída con la sesión que la persona usuaria inició en
    /// Vindexa. Es la biblioteca completa y no depende de que haya ningún
    /// cliente instalado.
    EpicAccountLibrary,
    /// La cuenta de GOG, leída con la sesión que la persona usuaria inició en
    /// Vindexa.
    GogAccountLibrary,
}

impl ScanSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EpicManifests => "epicManifests",
            Self::LegendaryInstalled => "legendaryInstalled",
            Self::GogGalaxyDatabase => "gogGalaxyDatabase",
            Self::GogGameInfo => "gogGameInfo",
            Self::HeroicGogInstalled => "heroicGogInstalled",
            Self::LegendaryMetadata => "legendaryMetadata",
            Self::HeroicEpicLibrary => "heroicEpicLibrary",
            Self::HeroicGogLibrary => "heroicGogLibrary",
            Self::EpicAccountLibrary => "epicAccountLibrary",
            Self::GogAccountLibrary => "gogAccountLibrary",
        }
    }
}

/// De dónde salió un escaneo, que decide qué puede afirmar sobre la instalación.
///
/// La distinción no es cosmética. Un escaneo local sabe qué hay descargado en
/// este disco; uno de cuenta sabe qué se posee y **no sabe nada** de la
/// instalación. Si los dos escribieran igual, sincronizar la cuenta borraría la
/// ruta de instalación que acababa de encontrar el escáner local, y el botón de
/// jugar desaparecería sin que nadie hubiera desinstalado nada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StoreOrigin {
    /// Manifiestos y bases de datos de los clientes instalados en este equipo.
    #[default]
    Local,
    /// La API de la tienda, con la sesión de la persona usuaria.
    Account,
}

/// Un juego tal y como lo describe el manifiesto local, antes de emparejarlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredGame {
    /// Identificador estable dentro de la tienda: `AppName` en Epic, `productId`
    /// en GOG. Ya validado contra la allowlist de [`launch`].
    pub external_id: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub install_path: Option<String>,
    pub installed: bool,
    pub size_on_disk: Option<i64>,
    /// Ruta absoluta del ejecutable declarado, ya validada y contenida dentro de
    /// `install_path`. `None` cuando el manifiesto no la trae o no supera la
    /// validación: no se adivina.
    pub launch_target: Option<String>,
    pub drm_state: DrmState,
    pub source: ScanSource,
}

/// Resultado completo de leer los orígenes locales de una tienda.
#[derive(Debug, Clone)]
pub struct StoreScan {
    pub store: ExternalStore,
    pub status: ScanStatus,
    /// Carpeta raíz que se reconoció. Se muestra en Ajustes, igual que la ruta
    /// de la base de datos: es información de la propia máquina de la persona
    /// usuaria, no un dato que salga de ella.
    pub detected_root: Option<String>,
    /// Código estable del motivo cuando el estado no es `success`.
    pub error_code: Option<String>,
    /// Mensaje en español para la persona usuaria. **Nunca** contiene rutas ni
    /// contenido de los manifiestos.
    pub error_message: Option<String>,
    pub games: Vec<DiscoveredGame>,
    pub sources: Vec<ScanSource>,
    /// Entradas que se leyeron pero no llegaron a ser un juego: manifiestos
    /// corruptos, vacíos o fuera de los límites de seguridad, identificadores
    /// que no superan la allowlist, y complementos (DLC) que comparten
    /// manifiesto con su juego base. Se cuenta para poder decirlo en la
    /// interfaz en vez de dejar un hueco sin explicar.
    pub skipped: u32,
    /// Si esto lo leyó el disco o la cuenta. Ver [`StoreOrigin`].
    pub origin: StoreOrigin,
}

impl StoreScan {
    pub(crate) fn empty(store: ExternalStore, status: ScanStatus) -> Self {
        Self {
            store,
            status,
            detected_root: None,
            error_code: None,
            error_message: None,
            games: Vec::new(),
            sources: Vec::new(),
            skipped: 0,
            origin: StoreOrigin::Local,
        }
    }

    /// Construye el estado explícito «este cliente no está en esta máquina».
    pub(crate) fn unavailable(
        store: ExternalStore,
        code: &'static str,
        message: &'static str,
    ) -> Self {
        Self {
            error_code: Some(code.to_string()),
            error_message: Some(message.to_string()),
            ..Self::empty(store, ScanStatus::Unavailable)
        }
    }

    pub(crate) fn note_source(&mut self, source: ScanSource) {
        if !self.sources.contains(&source) {
            self.sources.push(source);
        }
    }
}

/// Escanea los orígenes locales de una tienda concreta.
///
/// Nunca devuelve `Err` por ausencia del cliente: eso es
/// [`ScanStatus::Unavailable`] dentro de un `Ok`. El `Err` queda reservado para
/// fallos que la persona usuaria debe poder distinguir (por ejemplo, un
/// manifiesto ilegible cuando el cliente sí está instalado).
pub fn scan(store: ExternalStore) -> AppResult<StoreScan> {
    match store {
        ExternalStore::Epic => epic::scan(),
        ExternalStore::Gog => gog::scan(),
    }
}

/// Escanea una tienda y persiste el resultado en una única transacción.
///
/// Es la operación que respalda el comando `scan_external_stores`.
pub fn scan_and_persist(
    connection: &mut rusqlite::Connection,
    store: ExternalStore,
) -> AppResult<db::ExternalStoreScanReport> {
    let result = scan(store)?;
    db::persist_scan(connection, &result)
}

/// Escanea todas las tiendas conocidas. Una tienda que falle no impide que las
/// demás se lean: cada una lleva su propio estado en el informe.
pub fn scan_all(
    connection: &mut rusqlite::Connection,
) -> AppResult<Vec<db::ExternalStoreScanReport>> {
    let mut reports = Vec::with_capacity(ExternalStore::ALL.len());
    for store in ExternalStore::ALL {
        reports.push(scan_and_persist(connection, store)?);
    }
    Ok(reports)
}

/// Vuelve a proponer el emparejado automático de todas las tiendas contra la
/// biblioteca de Steam actual.
///
/// Existe porque la biblioteca de Steam cambia sin que cambien los manifiestos
/// de Epic o GOG: un juego que ayer no estaba puede emparejar hoy, y obligar a
/// reescanear el disco entero para descubrirlo sería trabajo inútil. Las
/// correcciones manuales no se tocan (ver [`db::rematch`]).
pub fn rematch_all(connection: &mut rusqlite::Connection) -> AppResult<usize> {
    let mut changed = 0;
    for store in ExternalStore::ALL {
        changed += db::rematch(connection, store)?;
    }
    Ok(changed)
}

// ---------------------------------------------------------------------------
// Detección previa al escaneo
// ---------------------------------------------------------------------------

/// Lo que Vindexa reconoce de una tienda en **esta** máquina, antes de leer
/// ningún manifiesto.
///
/// `searched_paths` viaja siempre, también cuando no se detecta nada: decir «no
/// se encontró Epic» sin decir dónde se buscó es indistinguible de «Vindexa no
/// sabe buscarlo», y deja a la persona usuaria sin nada que comprobar.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDetection {
    pub store: String,
    pub display_name: String,
    pub detected: bool,
    /// Orígenes que existen de verdad en el disco.
    pub detected_paths: Vec<String>,
    /// Todas las rutas que se sondean, existan o no.
    pub searched_paths: Vec<String>,
}

/// Comprueba qué orígenes locales de una tienda existen en esta máquina.
///
/// «Detectada» significa que hay un cliente de esa tienda o una biblioteca suya
/// en el disco. **No** significa que exista una carpeta con un nombre parecido:
/// `~/Games/Heroic` sobrevive a la desinstalación de Heroic, y contarla hacía
/// que la tarjeta dijera «detectada» junto a «cliente no encontrado».
pub fn detect(store: ExternalStore) -> StoreDetection {
    let (detected_paths, searched_paths) = match store {
        ExternalStore::Epic => {
            let sources = epic::detect_sources();
            let mut found = sources.manifest_directories;
            found.extend(sources.installed_files);
            found.extend(sources.metadata_directories);
            found.extend(sources.library_caches);
            found.extend(sources.client_markers);
            (found, epic::searched_locations())
        }
        ExternalStore::Gog => {
            let sources = gog::detect_sources();
            let mut found = sources.galaxy_databases;
            found.extend(sources.install_roots);
            found.extend(sources.installed_files);
            found.extend(sources.library_caches);
            found.extend(sources.client_markers);
            (found, gog::searched_locations())
        }
    };
    let mut deduplicated = Vec::with_capacity(detected_paths.len());
    for path in detected_paths {
        if !deduplicated.contains(&path) {
            deduplicated.push(path);
        }
    }
    let detected_paths = deduplicated;
    let detected_paths = detected_paths
        .iter()
        .map(|path| paths::display_path(path))
        .collect::<Vec<_>>();
    StoreDetection {
        store: store.as_str().to_string(),
        display_name: store.display_name().to_string(),
        detected: !detected_paths.is_empty(),
        detected_paths,
        searched_paths: searched_paths
            .iter()
            .map(|path| paths::display_path(path))
            .collect(),
    }
}

/// Detección de todas las tiendas conocidas, en el orden de [`ExternalStore::ALL`].
pub fn detect_all() -> Vec<StoreDetection> {
    ExternalStore::ALL.iter().copied().map(detect).collect()
}

/// Resuelve cómo lanzar un juego externo ya persistido.
///
/// El identificador se valida contra la allowlist de [`launch`] **antes** de
/// tocar la base de datos y otra vez antes de construir la URL.
pub fn resolve_launch(
    connection: &rusqlite::Connection,
    store: ExternalStore,
    external_id: &str,
) -> AppResult<launch::LaunchTarget> {
    let external_id = launch::validate_external_id(store, external_id)?;
    let (install_path, launch_target) = db::launch_context(connection, store, external_id)?;
    launch::resolve_launch_target(
        store,
        external_id,
        install_path.as_deref(),
        launch_target.as_deref(),
    )
}

/// Entrega una acción sobre un juego externo al cliente oficial de su tienda.
///
/// Es la operación que respalda el comando `launch_external_game`: abre su
/// propia conexión, resuelve el destino y lo entrega al sistema operativo.
/// Vindexa no conoce el resultado; lo que pase después es del cliente de la
/// tienda.
pub fn open_game_action<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    database: &crate::db::Database,
    store: ExternalStore,
    external_id: &str,
    action: launch::ExternalGameAction,
) -> AppResult<()> {
    match action {
        launch::ExternalGameAction::Launch => {
            let target = resolve_launch(&database.open()?, store, external_id)?;
            launch::open_external_game(app, &target)
        }
    }
}

// ---------------------------------------------------------------------------
// Utilidades compartidas por los escáneres
// ---------------------------------------------------------------------------

/// Normaliza un título recibido de un manifiesto de terceros: recorta espacios,
/// colapsa saltos de línea y acota la longitud. Devuelve `None` si no queda nada
/// utilizable, porque un juego sin título no se persiste.
pub(crate) fn sanitize_title(value: &str) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    Some(collapsed.chars().take(MAX_TITLE_CHARS).collect())
}

/// Acota una ruta antes de persistirla. Una ruta absurdamente larga es señal de
/// manifiesto manipulado, no de una instalación real.
pub(crate) fn sanitize_path(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_PATH_CHARS {
        return None;
    }
    Some(trimmed.to_string())
}

/// Convierte un tamaño declarado en el manifiesto a un entero persistible.
/// Rechaza negativos porque la columna tiene `CHECK (size_on_disk >= 0)`.
pub(crate) fn sanitize_size(value: i64) -> Option<i64> {
    (value >= 0).then_some(value)
}

/// Acepta únicamente URLs absolutas `https`. Vindexa no descarga arte por `http`
/// ni admite esquemas inventados por un fichero de terceros (`javascript:`,
/// `file:`, `data:`).
pub(crate) fn sanitize_https_url(candidate: &str) -> Option<String> {
    let parsed = url::Url::parse(candidate.trim()).ok()?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return None;
    }
    let normalized: String = parsed.into();
    (normalized.chars().count() <= 2_000).then_some(normalized)
}

/// Consolida un juego descubierto sobre el mapa del escaneo.
///
/// La primera lectura manda —los manifiestos de instalación son la verdad sobre
/// el disco— pero los orígenes posteriores rellenan lo que estaba en `None`, que
/// es como la caché de biblioteca aporta carátulas a un juego que ya se conocía.
/// Nunca se sustituye un dato por otro.
pub(crate) fn merge_discovered(
    games: &mut std::collections::BTreeMap<String, DiscoveredGame>,
    scan: &mut StoreScan,
    game: DiscoveredGame,
) {
    match games.get_mut(&game.external_id) {
        Some(existing) => {
            existing.cover_url = existing.cover_url.take().or(game.cover_url);
            existing.header_url = existing.header_url.take().or(game.header_url);
            existing.install_path = existing.install_path.take().or(game.install_path);
            existing.launch_target = existing.launch_target.take().or(game.launch_target);
            existing.size_on_disk = existing.size_on_disk.or(game.size_on_disk);
            existing.installed = existing.installed || game.installed;
        }
        None => {
            if games.len() >= MAX_DISCOVERED_GAMES {
                scan.skipped = scan.skipped.saturating_add(1);
                return;
            }
            games.insert(game.external_id.clone(), game);
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Andamiaje común de los tests del módulo.
    //!
    //! `crate::db::migrations` es privado del módulo `db`, así que desde
    //! `stores` no se puede invocar directamente. Se usa la API pública
    //! `Database::initialize()`, que aplica exactamente las mismas migraciones
    //! y además deja los PRAGMA de producción (`foreign_keys`, WAL) activos, lo
    //! que hace el test más fiel que un `Connection::open_in_memory()` pelado.

    use crate::db::Database;
    use rusqlite::Connection;
    use tempfile::TempDir;

    /// Devuelve una base migrada y el directorio temporal que la contiene. El
    /// `TempDir` debe mantenerse vivo mientras se use la conexión.
    pub(crate) fn migrated_database() -> (TempDir, Connection) {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("aplicar migraciones");
        let connection = database.open().expect("abrir la base migrada");
        (directory, connection)
    }

    /// Inserta un juego de Steam con el que emparejar.
    pub(crate) fn insert_steam_game(connection: &Connection, app_id: u32, title: &str) {
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                rusqlite::params![app_id, title],
            )
            .expect("insertar juego de Steam");
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalStore, MAX_TITLE_CHARS, ScanStatus, StoreScan, sanitize_path, sanitize_size,
        sanitize_title,
    };
    use crate::db::rich_metadata::DrmState;

    #[test]
    fn the_store_identifier_is_a_closed_allowlist() {
        assert_eq!(ExternalStore::parse("epic").unwrap(), ExternalStore::Epic);
        assert_eq!(ExternalStore::parse("gog").unwrap(), ExternalStore::Gog);
        for invented in ["steam", "EPIC", "gog ", "'; DROP TABLE games; --", ""] {
            let error = ExternalStore::parse(invented).expect_err("rechazar tienda inventada");
            assert_eq!(error.code, "validation");
        }
    }

    #[test]
    fn only_gog_can_claim_drm_free_from_its_catalogue() {
        assert_eq!(ExternalStore::Gog.catalogue_drm_state(), DrmState::DrmFree);
        let evidence = ExternalStore::Gog.catalogue_drm_evidence();
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].matched, "catálogo GOG");

        // Epic no publica ninguna política de catálogo sin DRM: adivinarlo
        // sería inventar un dato de producto.
        assert_eq!(ExternalStore::Epic.catalogue_drm_state(), DrmState::Unknown);
        assert!(ExternalStore::Epic.catalogue_drm_evidence().is_empty());
    }

    #[test]
    fn an_absent_client_is_an_explicit_state_not_an_empty_list() {
        let scan = StoreScan::unavailable(
            ExternalStore::Epic,
            "epic_client_not_found",
            "No se encontró el Epic Games Launcher en este equipo.",
        );
        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.error_code.as_deref(), Some("epic_client_not_found"));
        assert!(scan.games.is_empty());
        // El motivo viaja siempre: una lista vacía sin explicación es
        // indistinguible de «no tienes juegos».
        assert!(scan.error_message.is_some());
    }

    #[test]
    fn the_detection_report_always_says_where_it_looked() {
        let reports = super::detect_all();
        assert_eq!(reports.len(), 2);
        for (report, store) in reports.iter().zip(ExternalStore::ALL) {
            assert_eq!(report.store, store.as_str());
            assert_eq!(report.display_name, store.display_name());
            // La lista de rutas sondeadas no depende de que la tienda esté
            // instalada: es justo lo que hay que enseñar cuando no lo está.
            assert!(!report.searched_paths.is_empty());
            assert_eq!(report.detected, !report.detected_paths.is_empty());
            for detected in &report.detected_paths {
                assert!(!detected.trim().is_empty());
            }
        }
    }

    #[test]
    fn titles_are_collapsed_and_bounded_and_never_become_empty_strings() {
        assert_eq!(
            sanitize_title("  Hollow\n\tKnight  ").as_deref(),
            Some("Hollow Knight")
        );
        assert_eq!(sanitize_title("   \n\t  "), None);
        assert_eq!(sanitize_title(""), None);
        let long = "a".repeat(MAX_TITLE_CHARS * 3);
        assert_eq!(
            sanitize_title(&long).map(|title| title.chars().count()),
            Some(MAX_TITLE_CHARS)
        );
    }

    #[test]
    fn oversized_or_empty_paths_and_negative_sizes_are_refused() {
        assert_eq!(
            sanitize_path("  /Users/Shared/Epic Games/Juego "),
            Some("/Users/Shared/Epic Games/Juego".to_string())
        );
        assert_eq!(sanitize_path("    "), None);
        assert_eq!(sanitize_path(&"x".repeat(8_000)), None);
        assert_eq!(sanitize_size(0), Some(0));
        assert_eq!(sanitize_size(4_294_967_296), Some(4_294_967_296));
        assert_eq!(sanitize_size(-1), None);
    }
}
