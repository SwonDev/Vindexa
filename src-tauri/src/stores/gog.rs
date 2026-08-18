//! Detección local de la biblioteca de GOG, sin credenciales.
//!
//! # Orígenes que se leen
//!
//! | Origen | Ruta |
//! |---|---|
//! | GOG Galaxy (Windows) | `%ProgramData%\GOG.com\Galaxy\storage\galaxy-2.0.db` |
//! | GOG Galaxy (macOS) | `/Users/Shared/GOG.com/Galaxy/Storage/galaxy-2.0.db` |
//! | Juegos instalados sin Galaxy | `<carpeta del juego>/goggame-<productId>.info` |
//! | Heroic | `<config>/heroic/gog_store/installed.json` |
//!
//! La ruta de macOS no es `~/Library/Application Support`: la documentación
//! oficial del SDK de integraciones de GOG publica `/Users/Shared/GOG.com/Galaxy/…`
//! como el equivalente exacto de `%programdata%\GOG.com\Galaxy\…`. Aun así se
//! sondea también la ruta de `Application Support` como respaldo, por si alguna
//! versión del cliente la usara: sondear de más no cuesta nada y no encontrar la
//! base sí.
//!
//! # Por qué se copia la base antes de leerla
//!
//! `galaxy-2.0.db` pertenece a otra aplicación y puede estar abierta en ese
//! mismo momento. Vindexa **nunca** la abre en el sitio: copia el fichero
//! principal y sus acompañantes de WAL a un directorio temporal propio, trabaja
//! sobre la copia y la borra. Cualquier recuperación de diario que SQLite
//! necesite ocurre sobre la copia; el original queda intacto.
//!
//! # Tolerancia al cambio de esquema
//!
//! El esquema de Galaxy no es una API pública y cambia entre versiones. Cada
//! consulta se intenta por separado: si una tabla o una columna no está, esa
//! consulta se descarta, se anota y el escaneo continúa con lo que sí se pudo
//! leer. Nunca se revienta ni se inventa un dato que falte.
//!
//! # DRM-Free
//!
//! Todo el catálogo de GOG se vende sin DRM por política publicada de la tienda,
//! así que cada juego se marca `drm_free` con la evidencia «catálogo GOG».
//! **Esa marca no debe aparecer nunca sobre la carátula**: es un dato de ficha.

use crate::error::AppResult;
use crate::stores::launch::validate_gog_product_id;
use crate::stores::paths::{self, ReadOutcome};
use crate::stores::{
    DiscoveredGame, ExternalStore, MAX_DISCOVERED_GAMES, ScanSource, ScanStatus, StoreScan, heroic,
    merge_discovered, sanitize_https_url, sanitize_path, sanitize_title,
};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Los `goggame-*.info` son JSON pequeños; 4 MiB es una cota holgada.
const MAX_INFO_BYTES: u64 = 4 * 1024 * 1024;

/// `installed.json` de Heroic agrupa toda la biblioteca.
const MAX_INSTALLED_JSON_BYTES: u64 = 32 * 1024 * 1024;

/// Cota de la base de Galaxy que se acepta copiar. Bibliotecas enormes rondan
/// las decenas de MiB; 1 GiB impide que un fichero anómalo llene el temporal.
const MAX_GALAXY_DATABASE_BYTES: u64 = 1024 * 1024 * 1024;

/// Profundidad máxima al buscar `goggame-*.info` bajo una raíz de instalación.
/// GOG deja el `.info` en la carpeta del juego, un nivel por debajo de la raíz.
const GOG_INSTALL_SCAN_DEPTH: usize = 2;

/// Valor del runner con el que Heroic marca las entradas de GOG en su caché.
const HEROIC_GOG_RUNNER: &str = "gog";

/// Orígenes concretos que se van a leer.
#[derive(Debug, Clone, Default)]
pub struct GogSources {
    /// Bases `galaxy-2.0.db` candidatas.
    pub galaxy_databases: Vec<PathBuf>,
    /// Raíces que contienen de verdad al menos un `goggame-*.info`.
    pub install_roots: Vec<PathBuf>,
    /// Raíces que existen pero no contienen ningún juego de GOG. **No son un
    /// origen**: `~/Games/Heroic` es la carpeta donde Heroic instala los juegos
    /// y donde crea los prefijos de Wine, y sigue ahí después de desinstalar
    /// Heroic. Tratar su mera existencia como «tienda detectada» era lo que
    /// hacía que Vindexa dijese «GOG detectada» y «cliente no encontrado» a la
    /// vez. Se guardan aparte para poder explicar exactamente eso.
    pub empty_install_roots: Vec<PathBuf>,
    /// `gog_store/installed.json` de Heroic.
    pub installed_files: Vec<PathBuf>,
    /// `store_cache/gog_library.json` de Heroic: biblioteca completa.
    pub library_caches: Vec<PathBuf>,
    /// Carpetas que prueban que hay un cliente con soporte de GOG en este
    /// equipo aunque todavía no haya biblioteca que leer.
    pub client_markers: Vec<PathBuf>,
}

impl GogSources {
    /// `true` cuando no hay **ninguna** biblioteca que leer.
    pub fn is_empty(&self) -> bool {
        self.galaxy_databases.is_empty()
            && self.install_roots.is_empty()
            && self.installed_files.is_empty()
            && self.library_caches.is_empty()
    }

    /// `true` cuando hay algo de GOG en este equipo, con biblioteca o sin ella.
    pub fn client_is_present(&self) -> bool {
        !self.is_empty() || !self.client_markers.is_empty()
    }
}

/// Escanea la biblioteca de GOG en esta máquina.
pub fn scan() -> AppResult<StoreScan> {
    scan_sources(&detect_sources())
}

/// Resuelve los orígenes que existen de verdad en esta máquina.
pub fn detect_sources() -> GogSources {
    let mut sources = GogSources::default();
    for database in candidate_galaxy_databases() {
        if paths::is_real_file(&database) && !sources.galaxy_databases.contains(&database) {
            sources.galaxy_databases.push(database);
        }
    }
    for root in candidate_install_roots() {
        if !paths::directory_is_readable(&root) {
            continue;
        }
        // Una carpeta candidata sólo es un origen si dentro hay un juego de GOG.
        // Existir no basta: existir es lo que hace una carpeta abandonada.
        if game_info_files_under(&root, GOG_INSTALL_SCAN_DEPTH).is_empty() {
            if !sources.empty_install_roots.contains(&root) {
                sources.empty_install_roots.push(root);
            }
        } else if !sources.install_roots.contains(&root) {
            sources.install_roots.push(root);
        }
    }
    for file in candidate_installed_files() {
        if paths::is_real_file(&file) && !sources.installed_files.contains(&file) {
            sources.installed_files.push(file);
        }
    }
    for file in candidate_library_caches() {
        if paths::is_real_file(&file) && !sources.library_caches.contains(&file) {
            sources.library_caches.push(file);
        }
    }
    for directory in candidate_client_markers() {
        if paths::is_real_directory(&directory) && !sources.client_markers.contains(&directory) {
            sources.client_markers.push(directory);
        }
    }
    sources
}

/// Todas las rutas que se sondean en esta máquina, existan o no. Ver
/// [`crate::stores::epic::searched_locations`].
pub fn searched_locations() -> Vec<PathBuf> {
    let mut locations = candidate_galaxy_databases();
    locations.extend(candidate_install_roots());
    locations.extend(candidate_installed_files());
    locations.extend(candidate_library_caches());
    // También las carpetas de cliente: si alguien tiene Heroic o Legendary en un
    // sitio raro, tiene que poder comprobar dónde se miró.
    locations.extend(candidate_client_markers());
    let mut deduplicated = Vec::with_capacity(locations.len());
    for location in locations {
        if !deduplicated.contains(&location) {
            deduplicated.push(location);
        }
    }
    deduplicated
}

fn candidate_galaxy_databases() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let mut push_variants = |base: PathBuf| {
        // Windows escribe `storage` en minúscula y macOS `Storage`; el sistema
        // de ficheros de macOS suele ser insensible a mayúsculas, pero no
        // siempre, así que se sondean ambas.
        for storage in ["storage", "Storage"] {
            candidates.push(base.join(storage).join("galaxy-2.0.db"));
        }
    };
    if let Some(program_data) = paths::program_data_directory() {
        push_variants(program_data.join("GOG.com").join("Galaxy"));
    }
    if let Some(home) = paths::home_directory() {
        push_variants(
            home.join("Library")
                .join("Application Support")
                .join("GOG.com")
                .join("Galaxy"),
        );
    }
    candidates
}

fn candidate_install_roots() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if cfg!(target_os = "windows") {
        for drive in ['C', 'D', 'E'] {
            candidates.push(PathBuf::from(format!("{drive}:\\GOG Games")));
            candidates.push(PathBuf::from(format!(
                "{drive}:\\Program Files (x86)\\GOG Galaxy\\Games"
            )));
        }
    }
    if let Some(home) = paths::home_directory() {
        // Los instaladores de GOG dejan cada juego bajo `~/GOG Games/<Juego>`.
        candidates.push(home.join("GOG Games"));
        candidates.push(home.join("Games").join("Heroic"));
    }
    candidates
}

fn candidate_installed_files() -> Vec<PathBuf> {
    paths::heroic_data_directories()
        .into_iter()
        .map(|directory| directory.join("gog_store").join("installed.json"))
        .collect()
}

/// Cachés de biblioteca de Heroic para GOG.
fn candidate_library_caches() -> Vec<PathBuf> {
    paths::heroic_data_directories()
        .into_iter()
        .map(|directory| directory.join("store_cache").join(heroic::GOG_LIBRARY_FILE))
        .collect()
}

/// Carpetas cuya sola existencia prueba que hay un cliente con soporte de GOG.
///
/// Se comprueba la carpeta, nunca el `gog_store/auth.json` que hay dentro: ese
/// fichero guarda el token de sesión de GOG y Vindexa no lo abre jamás.
fn candidate_client_markers() -> Vec<PathBuf> {
    let mut candidates = paths::heroic_data_directories();
    if let Some(program_data) = paths::program_data_directory() {
        candidates.push(program_data.join("GOG.com").join("Galaxy"));
    }
    if let Some(home) = paths::home_directory() {
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("GOG.com")
                .join("Galaxy"),
        );
    }
    candidates
}

/// Lee los orígenes indicados y consolida los juegos.
pub fn scan_sources(sources: &GogSources) -> AppResult<StoreScan> {
    if sources.is_empty() {
        return Ok(unavailable_for(sources));
    }

    let mut scan = StoreScan::empty(ExternalStore::Gog, ScanStatus::Success);
    let mut games: BTreeMap<String, DiscoveredGame> = BTreeMap::new();

    for database in &sources.galaxy_databases {
        match read_galaxy_database(database) {
            Ok(entries) => {
                scan.note_source(ScanSource::GogGalaxyDatabase);
                if scan.detected_root.is_none()
                    && let Some(parent) = database.parent()
                {
                    scan.detected_root = Some(paths::display_path(parent));
                }
                for game in entries {
                    merge_discovered(&mut games, &mut scan, game);
                }
            }
            Err(()) => scan.skipped = scan.skipped.saturating_add(1),
        }
    }

    for root in &sources.install_roots {
        if !paths::directory_is_readable(root) {
            continue;
        }
        let found = scan_install_root(root, GOG_INSTALL_SCAN_DEPTH, &mut scan);
        if !found.is_empty() {
            scan.note_source(ScanSource::GogGameInfo);
            if scan.detected_root.is_none() {
                scan.detected_root = Some(paths::display_path(root));
            }
        }
        for game in found {
            merge_discovered(&mut games, &mut scan, game);
        }
    }

    for file in &sources.installed_files {
        match paths::read_text_file(file, MAX_INSTALLED_JSON_BYTES) {
            ReadOutcome::Text(contents) => match parse_heroic_installed(&contents) {
                Some(entries) => {
                    scan.note_source(ScanSource::HeroicGogInstalled);
                    for (game, skipped) in entries {
                        scan.skipped = scan.skipped.saturating_add(skipped);
                        if let Some(game) = game {
                            merge_discovered(&mut games, &mut scan, game);
                        }
                    }
                }
                None => scan.skipped = scan.skipped.saturating_add(1),
            },
            ReadOutcome::Missing => {}
            ReadOutcome::Unsafe | ReadOutcome::Unreadable => {
                scan.skipped = scan.skipped.saturating_add(1);
            }
        }
    }

    // La biblioteca completa que Heroic guarda tras iniciar sesión. Va la última
    // para que lo que dice el disco sobre una instalación mande sobre el
    // catálogo, y el catálogo sólo rellene lo que faltaba (título y carátulas).
    for file in &sources.library_caches {
        match paths::read_text_file(file, heroic::MAX_LIBRARY_CACHE_BYTES) {
            ReadOutcome::Text(contents) => {
                let (entries, skipped) =
                    match heroic::parse_library_cache(&contents, HEROIC_GOG_RUNNER) {
                        heroic::LibraryCache::Games { entries, skipped } => (entries, skipped),
                        // El cliente está instalado y todavía no ha traído su
                        // biblioteca: eso no es un fichero ilegible.
                        heroic::LibraryCache::Absent => continue,
                        heroic::LibraryCache::Malformed => {
                            scan.skipped = scan.skipped.saturating_add(1);
                            continue;
                        }
                    };
                scan.note_source(ScanSource::HeroicGogLibrary);
                scan.skipped = scan.skipped.saturating_add(skipped);
                if scan.detected_root.is_none()
                    && let Some(parent) = file.parent().and_then(|store_cache| store_cache.parent())
                {
                    scan.detected_root = Some(paths::display_path(parent));
                }
                for entry in entries {
                    match convert_heroic_entry(entry) {
                        Some(game) => merge_discovered(&mut games, &mut scan, game),
                        None => scan.skipped = scan.skipped.saturating_add(1),
                    }
                }
            }
            ReadOutcome::Missing => {}
            ReadOutcome::Unsafe | ReadOutcome::Unreadable => {
                scan.skipped = scan.skipped.saturating_add(1);
            }
        }
    }

    if scan.sources.is_empty() {
        // Se descartó algo: había información y no se pudo leer. Eso es un
        // fallo que la persona usuaria tiene que ver.
        if scan.skipped > 0 {
            scan.status = ScanStatus::Failed;
            scan.error_code = Some("gog_library_unreadable".to_string());
            scan.error_message = Some(
                "Se encontró GOG en este equipo, pero su biblioteca local no se pudo leer."
                    .to_string(),
            );
        } else {
            let reason = unavailable_for(sources);
            scan.status = reason.status;
            scan.error_code = reason.error_code;
            scan.error_message = reason.error_message;
        }
        return Ok(scan);
    }

    scan.games = games.into_values().collect();
    Ok(scan)
}

/// Motivo exacto por el que no hay nada que leer.
///
/// Los tres casos se arreglan de forma distinta, así que se dicen por separado:
/// falta el cliente, el cliente está pero sin sesión, o lo único que hay es una
/// carpeta de juegos vacía que un Heroic anterior dejó atrás.
fn unavailable_for(sources: &GogSources) -> StoreScan {
    if sources.client_is_present() {
        StoreScan::unavailable(
            ExternalStore::Gog,
            "gog_not_signed_in",
            "Se encontró GOG Galaxy o Heroic en este equipo, pero no hay ninguna biblioteca de GOG guardada. Abre ese cliente, inicia sesión en GOG y vuelve a escanear.",
        )
    } else if !sources.empty_install_roots.is_empty() {
        StoreScan::unavailable(
            ExternalStore::Gog,
            "gog_install_folder_empty",
            "Sólo se encontró la carpeta donde Heroic o los instaladores de GOG dejan los juegos, y está vacía. No hay ningún cliente de GOG instalado ni ningún juego que leer.",
        )
    } else {
        StoreScan::unavailable(
            ExternalStore::Gog,
            "gog_client_not_found",
            "No se encontró GOG Galaxy, ni juegos de GOG instalados, ni una configuración de Heroic en este equipo.",
        )
    }
}

// ---------------------------------------------------------------------------
// Base `galaxy-2.0.db`
// ---------------------------------------------------------------------------

/// Copia temporal de la base de otra aplicación. Se borra al soltarse.
struct GalaxyDatabaseCopy {
    directory: PathBuf,
    database: PathBuf,
}

impl GalaxyDatabaseCopy {
    /// Copia la base y sus acompañantes de WAL. Devuelve `Err(())` si el
    /// original no es seguro de copiar o si la copia falla: el escáner lo trata
    /// como un origen descartado, no como un pánico.
    fn create(original: &Path) -> Result<Self, ()> {
        let metadata = fs::symlink_metadata(original).map_err(|_| ())?;
        if !metadata.file_type().is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_GALAXY_DATABASE_BYTES
        {
            return Err(());
        }
        let directory = std::env::temp_dir().join(format!("vindexa-gog-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).map_err(|_| ())?;
        let database = directory.join("galaxy-2.0.db");
        let copy = Self {
            directory,
            database,
        };
        if fs::copy(original, &copy.database).is_err() {
            return Err(());
        }
        // El diario WAL y su índice viven junto a la base. Sin ellos, la copia
        // podría no reflejar las últimas escrituras de Galaxy.
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = original.as_os_str().to_os_string();
            sidecar.push(suffix);
            let sidecar = PathBuf::from(sidecar);
            if !paths::is_real_file(&sidecar) {
                continue;
            }
            let mut destination = copy.database.as_os_str().to_os_string();
            destination.push(suffix);
            let _ = fs::copy(&sidecar, PathBuf::from(destination));
        }
        Ok(copy)
    }
}

impl Drop for GalaxyDatabaseCopy {
    fn drop(&mut self) {
        // La copia es efímera por diseño: no se deja rastro de la biblioteca de
        // otra tienda en el disco.
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Lee la copia de la base de Galaxy. `Err(())` sólo cuando no se pudo abrir
/// nada en absoluto; la ausencia de una tabla concreta degrada con elegancia.
fn read_galaxy_database(original: &Path) -> Result<Vec<DiscoveredGame>, ()> {
    let copy = GalaxyDatabaseCopy::create(original)?;
    // La copia se abre con permiso de escritura **a propósito**: si Galaxy dejó
    // un WAL sin consolidar, SQLite necesita poder recuperarlo, y hacerlo sobre
    // nuestra copia privada no toca la base original. Aquí sólo se ejecutan
    // SELECT.
    let connection = Connection::open_with_flags(
        &copy.database,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| ())?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| ())?;

    // Un Galaxy recién instalado tiene el esquema pero ninguna fila: eso es
    // «no tienes juegos», no un fallo. Lo que sí es un fallo es no reconocer
    // ninguna de las tablas que la consulta necesita.
    if !galaxy_schema_is_recognized(&connection) {
        return Err(());
    }

    let installed = read_installed_base_products(&connection);
    let metadata = read_game_pieces(&connection);
    let launch_parameters = read_play_task_parameters(&connection);

    // La lista autoritativa de «qué hay en esta máquina» es la de productos
    // instalados. Si esa tabla cambió de nombre, se cae a los metadatos, que al
    // menos dan título; en ese caso `installed` queda en false porque no hay
    // evidencia de instalación.
    let identifiers: Vec<String> = if installed.is_empty() {
        metadata.keys().cloned().collect()
    } else {
        installed.keys().cloned().collect()
    };

    let mut games = Vec::new();
    for product_id in identifiers {
        if validate_gog_product_id(&product_id).is_err() {
            continue;
        }
        let installation = installed.get(&product_id);
        let piece = metadata.get(&product_id);

        let title = installation
            .and_then(|entry| entry.title.as_deref())
            .or_else(|| piece.and_then(|entry| entry.title.as_deref()))
            .and_then(sanitize_title);
        // Sin título no se persiste: fabricar «GOG 1207658924» sería inventar.
        let Some(title) = title else { continue };

        let install_directory = installation
            .and_then(|entry| entry.installation_path.as_deref())
            .and_then(paths::canonical_install_directory);
        let launch_target = install_directory.as_ref().and_then(|directory| {
            let declared = launch_parameters.get(&product_id)?;
            paths::resolve_executable_within(directory, declared)
                .map(|path| paths::display_path(&path))
        });
        let install_path = install_directory
            .as_ref()
            .map(|directory| paths::display_path(directory))
            .and_then(|path| sanitize_path(&path));

        games.push(DiscoveredGame {
            external_id: product_id,
            title,
            cover_url: installation
                .and_then(|entry| entry.image_url.clone())
                .or_else(|| piece.and_then(|entry| entry.image_url.clone())),
            header_url: None,
            installed: install_path.is_some(),
            install_path,
            size_on_disk: None,
            launch_target,
            drm_state: ExternalStore::Gog.catalogue_drm_state(),
            source: ScanSource::GogGalaxyDatabase,
        });
    }
    Ok(games)
}

/// Comprueba que la copia contiene al menos una de las tablas que Vindexa sabe
/// leer. Sin ninguna, el esquema cambió tanto que no se puede afirmar nada.
fn galaxy_schema_is_recognized(connection: &Connection) -> bool {
    connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
              WHERE type = 'table'
                AND name IN ('InstalledBaseProducts', 'GamePieces', 'LimitedDetails')",
            [],
            |row| row.get::<_, i64>(0),
        )
        .is_ok_and(|count| count > 0)
}

#[derive(Debug, Default, Clone)]
struct GalaxyInstallation {
    installation_path: Option<String>,
    title: Option<String>,
    image_url: Option<String>,
}

/// `InstalledBaseProducts` (productos instalados) unida a `LimitedDetails`
/// (título e imágenes). Si `LimitedDetails` no existe, se reintenta sin ella.
fn read_installed_base_products(connection: &Connection) -> BTreeMap<String, GalaxyInstallation> {
    let with_details = query_installations(
        connection,
        "SELECT installed.productId, installed.installationPath, details.title, details.images
           FROM InstalledBaseProducts installed
           LEFT OUTER JOIN LimitedDetails details ON details.productId = installed.productId",
        true,
    );
    if !with_details.is_empty() {
        return with_details;
    }
    query_installations(
        connection,
        "SELECT productId, installationPath FROM InstalledBaseProducts",
        false,
    )
}

fn query_installations(
    connection: &Connection,
    sql: &str,
    with_details: bool,
) -> BTreeMap<String, GalaxyInstallation> {
    let Ok(mut statement) = connection.prepare(sql) else {
        return BTreeMap::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        let product_id: i64 = row.get(0)?;
        let installation_path: Option<String> = row.get(1)?;
        let (title, images) = if with_details {
            (
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            )
        } else {
            (None, None)
        };
        Ok((product_id, installation_path, title, images))
    }) else {
        return BTreeMap::new();
    };
    let mut installations = BTreeMap::new();
    for row in rows.take(MAX_DISCOVERED_GAMES).flatten() {
        let (product_id, installation_path, title, images) = row;
        if product_id <= 0 {
            continue;
        }
        installations.insert(
            product_id.to_string(),
            GalaxyInstallation {
                installation_path,
                title,
                image_url: images.as_deref().and_then(extract_image_url),
            },
        );
    }
    installations
}

#[derive(Debug, Default, Clone)]
struct GalaxyPiece {
    title: Option<String>,
    image_url: Option<String>,
}

/// `GamePieces` guarda los metadatos como JSON indexado por `releaseKey`
/// (`gog_<productId>`) y por tipo (`GamePieceTypes.type`). Es el origen que usan
/// las integraciones publicadas de Galaxy.
fn read_game_pieces(connection: &Connection) -> BTreeMap<String, GalaxyPiece> {
    let Ok(mut statement) = connection.prepare(
        "SELECT pieces.releaseKey, types.type, pieces.value
           FROM GamePieces pieces
           JOIN GamePieceTypes types ON types.id = pieces.gamePieceTypeId
          WHERE pieces.releaseKey LIKE 'gog\\_%' ESCAPE '\\'
            AND types.type IN ('title', 'originalTitle', 'images', 'originalImages')",
    ) else {
        return BTreeMap::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }) else {
        return BTreeMap::new();
    };

    let mut pieces: BTreeMap<String, GalaxyPiece> = BTreeMap::new();
    // Cuatro filas por juego como mucho, más margen para claves inesperadas.
    for row in rows.take(MAX_DISCOVERED_GAMES * 8).flatten() {
        let (release_key, piece_type, value) = row;
        let Some(product_id) = release_key.strip_prefix("gog_") else {
            continue;
        };
        if validate_gog_product_id(product_id).is_err() {
            continue;
        }
        let entry = pieces.entry(product_id.to_string()).or_default();
        match piece_type.as_str() {
            // `originalTitle` es el nombre del catálogo; `title` puede estar
            // renombrado por la persona usuaria. Se prefiere el original para
            // emparejar, pero cualquiera de los dos vale si falta el otro.
            "originalTitle" => {
                entry.title = extract_json_string(&value, "title").or(entry.title.take());
            }
            "title" if entry.title.is_none() => {
                entry.title = extract_json_string(&value, "title");
            }
            "originalImages" => {
                entry.image_url = extract_image_url(&value).or(entry.image_url.take());
            }
            "images" if entry.image_url.is_none() => {
                entry.image_url = extract_image_url(&value);
            }
            _ => {}
        }
    }
    pieces
}

/// `PlayTasks` + `PlayTaskLaunchParameters` dan el ejecutable que Galaxy usa
/// para arrancar cada juego. Es opcional: sin él se cae a la URL de protocolo.
fn read_play_task_parameters(connection: &Connection) -> BTreeMap<String, String> {
    let Ok(mut statement) = connection.prepare(
        "SELECT tasks.gameReleaseKey, parameters.executablePath
           FROM PlayTasks tasks
           JOIN PlayTaskLaunchParameters parameters ON parameters.playTaskId = tasks.id
          WHERE tasks.gameReleaseKey LIKE 'gog\\_%' ESCAPE '\\'
            AND parameters.executablePath IS NOT NULL",
    ) else {
        return BTreeMap::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) else {
        return BTreeMap::new();
    };
    let mut parameters = BTreeMap::new();
    for row in rows.take(MAX_DISCOVERED_GAMES * 4).flatten() {
        let (release_key, executable) = row;
        let Some(product_id) = release_key.strip_prefix("gog_") else {
            continue;
        };
        if validate_gog_product_id(product_id).is_err() || executable.trim().is_empty() {
            continue;
        }
        parameters
            .entry(product_id.to_string())
            .or_insert(executable);
    }
    parameters
}

fn extract_json_string(value: &str, key: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    parsed.get(key)?.as_str().and_then(sanitize_title)
}

/// Las imágenes de Galaxy son un objeto JSON con varias variantes. Se toma la
/// primera que sea una URL `https` válida, sin fabricar ninguna.
fn extract_image_url(value: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(value).ok()?;
    let object = parsed.as_object()?;
    for key in ["verticalCover", "squareIcon", "logo", "background", "icon"] {
        let Some(candidate) = object.get(key).and_then(serde_json::Value::as_str) else {
            continue;
        };
        if let Some(url) = sanitize_https_url(candidate) {
            return Some(url);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// `goggame-<productId>.info`
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GogGameInfo {
    #[serde(rename = "gameId")]
    game_id: Option<String>,
    #[serde(rename = "rootGameId")]
    root_game_id: Option<String>,
    name: Option<String>,
    #[serde(rename = "playTasks", default)]
    play_tasks: Vec<GogPlayTask>,
}

#[derive(Debug, Deserialize)]
struct GogPlayTask {
    #[serde(rename = "type")]
    task_type: Option<String>,
    path: Option<String>,
    #[serde(rename = "isPrimary", default)]
    is_primary: bool,
    category: Option<String>,
}

/// Lista los `goggame-<id>.info` que hay bajo una raíz **sin leerlos**.
///
/// Está separado de la lectura porque la detección necesita responder «¿hay
/// algún juego de GOG aquí dentro?» sin abrir ni un fichero, y el escaneo
/// necesita recorrer exactamente los mismos sitios. Con una sola función, no
/// pueden discrepar.
fn game_info_files_under(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_game_info_files(root, depth, &mut found);
    found
}

fn collect_game_info_files(directory: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth == 0 || found.len() >= MAX_DISCOVERED_GAMES {
        return;
    }
    for info in paths::list_files_with_extension(directory, "info") {
        let matches_prefix = info
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("goggame-"));
        if matches_prefix {
            found.push(info);
        }
    }
    for subdirectory in paths::list_subdirectories(directory) {
        collect_game_info_files(&subdirectory, depth - 1, found);
    }
}

/// Recorre una raíz leyendo las carpetas de juego que [`game_info_files_under`]
/// encontró.
fn scan_install_root(root: &Path, depth: usize, scan: &mut StoreScan) -> Vec<DiscoveredGame> {
    let mut games = Vec::new();
    for info in game_info_files_under(root, depth) {
        let Some(directory) = info.parent() else {
            continue;
        };
        match paths::read_text_file(&info, MAX_INFO_BYTES) {
            ReadOutcome::Text(contents) => match parse_game_info(&contents, directory) {
                Some(game) => games.push(game),
                None => scan.skipped = scan.skipped.saturating_add(1),
            },
            ReadOutcome::Missing => {}
            ReadOutcome::Unsafe | ReadOutcome::Unreadable => {
                scan.skipped = scan.skipped.saturating_add(1);
            }
        }
    }
    games
}

fn parse_game_info(contents: &str, install_directory: &Path) -> Option<DiscoveredGame> {
    let info: GogGameInfo = serde_json::from_str(contents).ok()?;
    // `rootGameId` identifica al juego base; los DLC comparten carpeta y traen
    // su propio `.info` con un `gameId` distinto.
    let game_id = info.game_id.as_deref()?.trim();
    if let Some(root_id) = info.root_game_id.as_deref()
        && !root_id.trim().is_empty()
        && root_id.trim() != game_id
    {
        return None;
    }
    validate_gog_product_id(game_id).ok()?;
    let title = info.name.as_deref().and_then(sanitize_title)?;

    let canonical_directory =
        paths::canonical_install_directory(&paths::display_path(install_directory));
    let launch_target = canonical_directory.as_ref().and_then(|directory| {
        let declared = primary_play_task(&info.play_tasks)?;
        paths::resolve_executable_within(directory, declared).map(|path| paths::display_path(&path))
    });
    let install_path = canonical_directory
        .as_ref()
        .map(|directory| paths::display_path(directory))
        .and_then(|path| sanitize_path(&path));

    Some(DiscoveredGame {
        external_id: game_id.to_string(),
        title,
        cover_url: None,
        header_url: None,
        installed: install_path.is_some(),
        install_path,
        size_on_disk: None,
        launch_target,
        drm_state: ExternalStore::Gog.catalogue_drm_state(),
        source: ScanSource::GogGameInfo,
    })
}

/// Devuelve la ruta declarada de la tarea de juego principal. Sólo se aceptan
/// tareas de tipo `FileTask` de categoría `game`: una `URLTask` abre una web y
/// nunca debe convertirse en un ejecutable.
fn primary_play_task(tasks: &[GogPlayTask]) -> Option<&str> {
    let is_game_file_task = |task: &&GogPlayTask| {
        task.task_type.as_deref() == Some("FileTask")
            && task.category.as_deref().is_none_or(|value| value == "game")
    };
    tasks
        .iter()
        .find(|task| is_game_file_task(task) && task.is_primary)
        .or_else(|| tasks.iter().find(is_game_file_task))
        .and_then(|task| task.path.as_deref())
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

// ---------------------------------------------------------------------------
// `gog_store/installed.json` de Heroic
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct HeroicInstalledFile {
    #[serde(default)]
    installed: Vec<HeroicInstalledGame>,
}

#[derive(Debug, Deserialize)]
struct HeroicInstalledGame {
    #[serde(rename = "appName")]
    app_name: Option<String>,
    install_path: Option<String>,
    executable: Option<String>,
    // `install_size` existe en el fichero de Heroic pero es texto legible
    // («12.3 GB»), no bytes. No se declara aquí a propósito: convertirlo
    // adivinando el factor sería inventar un dato que el fichero no da.
    #[serde(default)]
    is_dlc: bool,
}

fn parse_heroic_installed(contents: &str) -> Option<Vec<(Option<DiscoveredGame>, u32)>> {
    let file: HeroicInstalledFile = serde_json::from_str(contents).ok()?;
    Some(
        file.installed
            .into_iter()
            .map(|entry| match parse_heroic_entry(entry) {
                Some(game) => (Some(game), 0),
                None => (None, 1),
            })
            .collect(),
    )
}

fn parse_heroic_entry(entry: HeroicInstalledGame) -> Option<DiscoveredGame> {
    if entry.is_dlc {
        return None;
    }
    let app_name = entry.app_name.as_deref()?.trim();
    validate_gog_product_id(app_name).ok()?;
    // `installed.json` no trae el título: vive en la caché de biblioteca, que se
    // lee aparte y puede no estar. Aquí el nombre sale del `.info` que el propio
    // instalador dejó en la carpeta; sin ninguno de los dos no hay juego que
    // persistir, porque inventar «Juego 1207658924» sería fabricar un dato.
    let install_directory = entry
        .install_path
        .as_deref()
        .and_then(paths::canonical_install_directory)?;
    // El `.info` que el propio instalador dejó en la carpeta sí trae el nombre.
    let title = read_title_from_install_directory(&install_directory, app_name)?;

    let launch_target = entry.executable.as_deref().and_then(|executable| {
        paths::resolve_executable_within(&install_directory, executable)
            .map(|path| paths::display_path(&path))
    });
    let install_path = sanitize_path(&paths::display_path(&install_directory));

    Some(DiscoveredGame {
        external_id: app_name.to_string(),
        title,
        cover_url: None,
        header_url: None,
        installed: install_path.is_some(),
        install_path,
        // Ver la nota de `HeroicInstalledGame`: el tamaño de Heroic no es un
        // número de bytes, así que aquí no hay dato.
        size_on_disk: None,
        launch_target,
        drm_state: ExternalStore::Gog.catalogue_drm_state(),
        source: ScanSource::HeroicGogInstalled,
    })
}

// ---------------------------------------------------------------------------
// `store_cache/gog_library.json` de Heroic: la biblioteca completa
// ---------------------------------------------------------------------------

/// Convierte una entrada de la caché de Heroic en un juego de GOG.
///
/// La ruta de instalación que trae la caché se revalida contra el disco: Heroic
/// puede haber guardado un juego que después se borró a mano.
fn convert_heroic_entry(entry: heroic::HeroicLibraryEntry) -> Option<DiscoveredGame> {
    validate_gog_product_id(&entry.app_name).ok()?;
    let install_directory = entry
        .install_path
        .as_deref()
        .and_then(paths::canonical_install_directory);
    let launch_target = install_directory.as_ref().and_then(|directory| {
        let executable = entry.executable.as_deref()?;
        paths::resolve_executable_within(directory, executable)
            .map(|path| paths::display_path(&path))
    });
    let install_path = install_directory
        .as_ref()
        .map(|directory| paths::display_path(directory))
        .and_then(|path| sanitize_path(&path));

    Some(DiscoveredGame {
        external_id: entry.app_name,
        title: entry.title,
        cover_url: entry.cover_url,
        header_url: entry.header_url,
        installed: entry.installed && install_path.is_some(),
        install_path,
        // Heroic guarda el tamaño como texto legible («12.3 GB»), no en bytes.
        size_on_disk: None,
        launch_target,
        drm_state: ExternalStore::Gog.catalogue_drm_state(),
        source: ScanSource::HeroicGogLibrary,
    })
}

fn read_title_from_install_directory(directory: &Path, product_id: &str) -> Option<String> {
    let info = directory.join(format!("goggame-{product_id}.info"));
    let ReadOutcome::Text(contents) = paths::read_text_file(&info, MAX_INFO_BYTES) else {
        return None;
    };
    let info: GogGameInfo = serde_json::from_str(&contents).ok()?;
    info.name.as_deref().and_then(sanitize_title)
}

#[cfg(test)]
mod tests {
    use super::{GogSources, extract_image_url, primary_play_task, scan_sources};
    use crate::db::rich_metadata::DrmState;
    use crate::stores::{ScanSource, ScanStatus};
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("crear carpeta");
        }
        fs::write(path, contents).expect("escribir fichero");
    }

    /// Crea una base con la forma real de `galaxy-2.0.db` en la parte que
    /// Vindexa consulta.
    fn galaxy_database(path: &Path, installation_path: &str) {
        let connection = Connection::open(path).expect("crear base de Galaxy");
        connection
            .execute_batch(
                "CREATE TABLE InstalledBaseProducts(
                    productId INTEGER PRIMARY KEY,
                    installationPath TEXT
                 );
                 CREATE TABLE LimitedDetails(
                    id INTEGER PRIMARY KEY,
                    productId INTEGER,
                    title TEXT,
                    images TEXT
                 );
                 CREATE TABLE GamePieceTypes(id INTEGER PRIMARY KEY, type TEXT);
                 CREATE TABLE GamePieces(
                    id INTEGER PRIMARY KEY,
                    releaseKey TEXT,
                    gamePieceTypeId INTEGER,
                    value TEXT
                 );
                 CREATE TABLE PlayTasks(
                    id INTEGER PRIMARY KEY,
                    gameReleaseKey TEXT,
                    isPrimary INTEGER
                 );
                 CREATE TABLE PlayTaskLaunchParameters(
                    id INTEGER PRIMARY KEY,
                    playTaskId INTEGER,
                    executablePath TEXT
                 );
                 INSERT INTO GamePieceTypes(id, type) VALUES (1, 'originalTitle'), (2, 'originalImages');",
            )
            .expect("crear esquema de Galaxy");
        connection
            .execute(
                "INSERT INTO InstalledBaseProducts(productId, installationPath) VALUES (1207658924, ?1)",
                [installation_path],
            )
            .expect("insertar instalación");
        connection
            .execute_batch(
                "INSERT INTO LimitedDetails(id, productId, title, images)
                   VALUES (1, 1207658924, 'The Witcher: Enhanced Edition',
                           '{\"squareIcon\": \"https://images.gog.com/icono.png\"}');
                 INSERT INTO GamePieces(id, releaseKey, gamePieceTypeId, value)
                   VALUES (1, 'gog_1207658924', 1, '{\"title\": \"The Witcher\"}');
                 INSERT INTO PlayTasks(id, gameReleaseKey, isPrimary)
                   VALUES (1, 'gog_1207658924', 1);
                 INSERT INTO PlayTaskLaunchParameters(id, playTaskId, executablePath)
                   VALUES (1, 1, 'witcher.exe');",
            )
            .expect("insertar metadatos");
    }

    fn install_directory(root: &Path, name: &str, executable: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("crear instalación");
        fs::write(directory.join(executable), "binario").expect("escribir ejecutable");
        directory
    }

    #[test]
    fn an_absent_client_reports_unavailable_with_a_reason_in_spanish() {
        let scan = scan_sources(&GogSources::default()).expect("escanear sin orígenes");
        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.error_code.as_deref(), Some("gog_client_not_found"));
        assert!(scan.error_message.expect("motivo").contains("GOG Galaxy"));
    }

    #[test]
    fn the_galaxy_database_is_read_from_a_copy_and_the_original_is_never_touched() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "The Witcher", "witcher.exe");
        let database = root.path().join("storage").join("galaxy-2.0.db");
        fs::create_dir_all(database.parent().unwrap()).expect("crear storage");
        galaxy_database(&database, install.to_str().expect("ruta utf-8"));
        let before = fs::metadata(&database).expect("metadatos previos").len();

        let scan = scan_sources(&GogSources {
            galaxy_databases: vec![database.clone()],
            ..GogSources::default()
        })
        .expect("escanear Galaxy");

        assert_eq!(scan.status, ScanStatus::Success);
        assert_eq!(scan.sources, vec![ScanSource::GogGalaxyDatabase]);
        assert_eq!(scan.games.len(), 1);
        let game = &scan.games[0];
        assert_eq!(game.external_id, "1207658924");
        assert_eq!(game.title, "The Witcher: Enhanced Edition");
        assert!(game.installed);
        assert!(
            game.launch_target
                .as_deref()
                .unwrap()
                .ends_with("witcher.exe")
        );
        assert_eq!(
            game.cover_url.as_deref(),
            Some("https://images.gog.com/icono.png")
        );
        // Todo GOG es DRM-free por política de catálogo.
        assert_eq!(game.drm_state, DrmState::DrmFree);

        // El original conserva su tamaño: sólo se leyó la copia.
        let after = fs::metadata(&database)
            .expect("metadatos posteriores")
            .len();
        assert_eq!(before, after);
    }

    #[test]
    fn the_temporary_copy_of_someone_elses_library_never_survives_the_scan() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "The Witcher", "witcher.exe");
        let database = root.path().join("galaxy-2.0.db");
        galaxy_database(&database, install.to_str().expect("ruta utf-8"));
        // Un `-wal` junto al original también debe copiarse y desaparecer.
        write(&root.path().join("galaxy-2.0.db-wal"), "diario simulado");

        let directory = {
            let copy = super::GalaxyDatabaseCopy::create(&database).expect("copiar la base");
            let directory = copy.directory.clone();
            assert!(directory.is_dir(), "la copia debe existir mientras se lee");
            assert!(copy.database.is_file());
            assert!(
                directory.join("galaxy-2.0.db-wal").is_file(),
                "el diario acompaña a la copia"
            );
            directory
        };
        // Al soltarse, no queda rastro de la biblioteca de otra tienda en disco.
        assert!(
            !directory.exists(),
            "la copia temporal debe borrarse al terminar"
        );
        // Y el original sigue con sus dos ficheros intactos.
        assert!(database.is_file());
        assert!(root.path().join("galaxy-2.0.db-wal").is_file());
    }

    #[test]
    fn a_galaxy_schema_without_limiteddetails_degrades_instead_of_breaking() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "Juego", "juego.exe");
        let database = root.path().join("galaxy-2.0.db");
        let connection = Connection::open(&database).expect("crear base");
        connection
            .execute_batch(
                "CREATE TABLE InstalledBaseProducts(productId INTEGER PRIMARY KEY, installationPath TEXT);
                 CREATE TABLE GamePieceTypes(id INTEGER PRIMARY KEY, type TEXT);
                 CREATE TABLE GamePieces(id INTEGER PRIMARY KEY, releaseKey TEXT, gamePieceTypeId INTEGER, value TEXT);
                 INSERT INTO GamePieceTypes(id, type) VALUES (1, 'originalTitle');",
            )
            .expect("crear esquema reducido");
        connection
            .execute(
                "INSERT INTO InstalledBaseProducts(productId, installationPath) VALUES (1207658924, ?1)",
                [install.to_str().expect("ruta utf-8")],
            )
            .expect("insertar instalación");
        connection
            .execute_batch(
                "INSERT INTO GamePieces(id, releaseKey, gamePieceTypeId, value)
                   VALUES (1, 'gog_1207658924', 1, '{\"title\": \"Juego de GOG\"}');",
            )
            .expect("insertar título");
        drop(connection);

        let scan = scan_sources(&GogSources {
            galaxy_databases: vec![database],
            ..GogSources::default()
        })
        .expect("escanear esquema reducido");

        assert_eq!(scan.status, ScanStatus::Success);
        assert_eq!(scan.games.len(), 1);
        assert_eq!(scan.games[0].title, "Juego de GOG");
        // Sin `PlayTaskLaunchParameters` no hay ejecutable: se caerá a la URL de
        // protocolo en el momento de lanzar.
        assert_eq!(scan.games[0].launch_target, None);
    }

    #[test]
    fn a_corrupt_galaxy_database_is_skipped_and_reported_as_a_failure() {
        let root = TempDir::new().expect("crear temporal");
        let database = root.path().join("galaxy-2.0.db");
        write(&database, "esto no es una base SQLite");

        let scan = scan_sources(&GogSources {
            galaxy_databases: vec![database],
            ..GogSources::default()
        })
        .expect("escanear base corrupta");

        assert_eq!(scan.status, ScanStatus::Failed);
        assert_eq!(scan.error_code.as_deref(), Some("gog_library_unreadable"));
        assert_eq!(scan.skipped, 1);
        let message = scan.error_message.expect("motivo");
        assert!(!message.contains('/'));
    }

    #[test]
    fn installed_games_are_found_without_galaxy_through_their_info_file() {
        let root = TempDir::new().expect("crear temporal");
        let games_root = root.path().join("GOG Games");
        let install = install_directory(&games_root, "Cyberpunk 2077", "REDprelauncher.exe");
        write(
            &install.join("goggame-1423049311.info"),
            r#"{
              "version": 1,
              "gameId": "1423049311",
              "rootGameId": "1423049311",
              "name": "Cyberpunk 2077",
              "language": "English",
              "playTasks": [
                {"category": "launcher", "isPrimary": false, "name": "Web", "type": "URLTask",
                 "link": "https://www.cyberpunk.net"},
                {"category": "game", "isPrimary": true, "name": "Cyberpunk 2077",
                 "path": "REDprelauncher.exe", "type": "FileTask"}
              ]
            }"#,
        );
        // Un DLC en la misma carpeta no se cuenta como juego.
        write(
            &install.join("goggame-1423049312.info"),
            r#"{"version": 1, "gameId": "1423049312", "rootGameId": "1423049311",
                "name": "Phantom Liberty", "playTasks": []}"#,
        );
        // Un `.info` corrupto se descarta sin romper el recorrido.
        write(&install.join("goggame-9999999999.info"), "{ roto");

        let scan = scan_sources(&GogSources {
            install_roots: vec![games_root],
            ..GogSources::default()
        })
        .expect("escanear instalaciones");

        assert_eq!(scan.status, ScanStatus::Success);
        assert_eq!(scan.sources, vec![ScanSource::GogGameInfo]);
        assert_eq!(scan.games.len(), 1);
        let game = &scan.games[0];
        assert_eq!(game.external_id, "1423049311");
        assert_eq!(game.title, "Cyberpunk 2077");
        assert!(game.installed);
        assert!(
            game.launch_target
                .as_deref()
                .unwrap()
                .ends_with("REDprelauncher.exe")
        );
        assert_eq!(game.drm_state, DrmState::DrmFree);
        // Dos entradas leídas que no son juegos: el DLC y el `.info` corrupto.
        assert_eq!(scan.skipped, 2);
    }

    #[test]
    fn a_url_task_never_becomes_an_executable() {
        let tasks: Vec<super::GogPlayTask> = serde_json::from_str(
            r#"[{"category": "game", "isPrimary": true, "type": "URLTask",
                 "link": "https://ejemplo.invalid"}]"#,
        )
        .expect("interpretar tareas");
        assert_eq!(primary_play_task(&tasks), None);

        let mixed: Vec<super::GogPlayTask> = serde_json::from_str(
            r#"[{"category": "game", "isPrimary": false, "type": "FileTask", "path": "juego.exe"},
                {"category": "game", "isPrimary": true, "type": "URLTask", "link": "https://x.invalid"}]"#,
        )
        .expect("interpretar tareas mixtas");
        assert_eq!(primary_play_task(&mixed), Some("juego.exe"));
    }

    #[test]
    fn only_absolute_https_artwork_urls_are_accepted() {
        assert_eq!(
            extract_image_url(r#"{"squareIcon": "https://images.gog.com/a.png"}"#).as_deref(),
            Some("https://images.gog.com/a.png")
        );
        assert_eq!(
            extract_image_url(r#"{"squareIcon": "http://images.gog.com/a.png"}"#),
            None
        );
        assert_eq!(
            extract_image_url(r#"{"squareIcon": "javascript:alert(1)"}"#),
            None
        );
        assert_eq!(
            extract_image_url(r#"{"squareIcon": "/relativa.png"}"#),
            None
        );
        assert_eq!(extract_image_url("no es json"), None);
        assert_eq!(extract_image_url("{}"), None);
    }

    #[test]
    fn heroic_entries_take_their_title_from_the_info_file_and_never_invent_one() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "Juego Heroic", "start.sh");
        write(
            &install.join("goggame-1207658924.info"),
            r#"{"version": 1, "gameId": "1207658924", "rootGameId": "1207658924",
                "name": "Juego de GOG en Heroic", "playTasks": []}"#,
        );
        let sin_info = install_directory(root.path(), "Sin Info", "start.sh");

        let installed = root.path().join("installed.json");
        write(
            &installed,
            &format!(
                r#"{{"installed": [
                    {{"appName": "1207658924", "install_path": {install:?},
                      "executable": "start.sh", "install_size": "12.3 GB",
                      "is_dlc": false, "platform": "linux", "version": "1.0"}},
                    {{"appName": "1111111111", "install_path": {sin_info:?},
                      "executable": "start.sh", "install_size": "1 GB",
                      "is_dlc": false, "platform": "linux", "version": "1.0"}},
                    {{"appName": "2222222222", "install_path": {install:?},
                      "is_dlc": true, "platform": "linux", "version": "1.0"}}
                ]}}"#,
                install = install.to_str().expect("ruta utf-8"),
                sin_info = sin_info.to_str().expect("ruta utf-8")
            ),
        );

        let scan = scan_sources(&GogSources {
            installed_files: vec![installed],
            ..GogSources::default()
        })
        .expect("escanear Heroic");

        assert_eq!(scan.status, ScanStatus::Success);
        assert_eq!(scan.sources, vec![ScanSource::HeroicGogInstalled]);
        assert_eq!(scan.games.len(), 1);
        assert_eq!(scan.games[0].title, "Juego de GOG en Heroic");
        // El tamaño de Heroic es texto («12.3 GB»): no se convierte a bytes
        // adivinando el factor.
        assert_eq!(scan.games[0].size_on_disk, None);
        // El DLC y la entrada sin `.info` se descartan y se cuentan.
        assert_eq!(scan.skipped, 2);
    }

    #[test]
    fn several_sources_complete_each_other_without_overwriting() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "The Witcher", "witcher.exe");
        let database = root.path().join("galaxy-2.0.db");
        galaxy_database(&database, install.to_str().expect("ruta utf-8"));
        write(
            &install.join("goggame-1207658924.info"),
            r#"{"version": 1, "gameId": "1207658924", "rootGameId": "1207658924",
                "name": "Nombre del instalador", "playTasks": []}"#,
        );

        let scan = scan_sources(&GogSources {
            galaxy_databases: vec![database],
            install_roots: vec![root.path().to_path_buf()],
            ..GogSources::default()
        })
        .expect("escanear varios orígenes");

        assert_eq!(scan.games.len(), 1);
        // Galaxy se lee primero y su título manda; el `.info` sólo completa lo
        // que faltase.
        assert_eq!(scan.games[0].title, "The Witcher: Enhanced Edition");
        assert_eq!(scan.sources.len(), 2);
    }

    #[test]
    fn an_empty_heroic_games_folder_is_not_a_detected_store() {
        let root = TempDir::new().expect("crear temporal");
        // Exactamente lo que deja Heroic al desinstalarse: la carpeta de juegos
        // con su subcarpeta de prefijos de Wine, y ni un juego dentro.
        let games = root.path().join("Games").join("Heroic");
        fs::create_dir_all(games.join("Prefixes")).expect("crear carpeta de Heroic");

        // Sondear la carpeta no encuentra ningún juego, así que no es un origen.
        assert!(super::game_info_files_under(&games, super::GOG_INSTALL_SCAN_DEPTH).is_empty());

        let scan = scan_sources(&GogSources {
            empty_install_roots: vec![games],
            ..GogSources::default()
        })
        .expect("escanear carpeta abandonada");

        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.error_code.as_deref(), Some("gog_install_folder_empty"));
        assert!(scan.games.is_empty());
        let message = scan.error_message.expect("motivo");
        assert!(message.contains("vacía"));
        assert!(!message.contains('/'));
    }

    #[test]
    fn a_client_that_never_signed_in_says_exactly_that_instead_of_pretending_to_be_absent() {
        let root = TempDir::new().expect("crear temporal");
        let heroic = root.path().join("heroic");
        fs::create_dir_all(&heroic).expect("crear carpeta de Heroic");

        let scan = scan_sources(&GogSources {
            client_markers: vec![heroic],
            ..GogSources::default()
        })
        .expect("escanear cliente sin sesión");

        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.error_code.as_deref(), Some("gog_not_signed_in"));
        let message = scan.error_message.expect("motivo");
        assert!(message.contains("inicia sesión"));

        let scan = scan_sources(&GogSources::default()).expect("escanear sin nada");
        assert_eq!(scan.error_code.as_deref(), Some("gog_client_not_found"));
    }

    #[test]
    fn the_heroic_library_cache_lists_the_games_that_are_not_installed_too() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "Steel Sky", "juego.exe");
        let cache = root.path().join("store_cache").join("gog_library.json");
        write(
            &cache,
            &format!(
                r#"{{"games": [
                    {{"runner": "gog", "app_name": "1207658924", "title": "The Witcher",
                      "art_cover": "https://images.gog.com/witcher.jpg",
                      "is_installed": false, "install": {{}}}},
                    {{"runner": "gog", "app_name": "1207666073", "title": "Beneath a Steel Sky",
                      "is_installed": true,
                      "install": {{"install_path": {install:?}, "executable": "juego.exe"}}}},
                    {{"runner": "gog", "app_name": "no-es-un-id", "title": "Identificador inválido"}}
                ]}}"#,
                install = install.to_str().expect("ruta utf-8")
            ),
        );

        let scan = scan_sources(&GogSources {
            library_caches: vec![cache],
            ..GogSources::default()
        })
        .expect("escanear la caché de Heroic");

        assert_eq!(scan.status, ScanStatus::Success);
        assert!(scan.sources.contains(&ScanSource::HeroicGogLibrary));
        assert_eq!(scan.games.len(), 2);
        // Un identificador que no supera la allowlist se descarta, no se limpia.
        assert_eq!(scan.skipped, 1);

        let witcher = scan
            .games
            .iter()
            .find(|game| game.external_id == "1207658924")
            .expect("el juego no instalado");
        assert!(!witcher.installed);
        assert_eq!(witcher.install_path, None);
        assert_eq!(
            witcher.cover_url.as_deref(),
            Some("https://images.gog.com/witcher.jpg")
        );
        // Todo el catálogo de GOG se vende sin DRM: también lo no instalado.
        assert_eq!(witcher.drm_state, DrmState::DrmFree);

        let steel_sky = scan
            .games
            .iter()
            .find(|game| game.external_id == "1207666073")
            .expect("el juego instalado");
        assert!(steel_sky.installed);
        assert!(steel_sky.launch_target.is_some());
    }
}
