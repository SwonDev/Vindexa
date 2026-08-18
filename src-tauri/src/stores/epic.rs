//! Detección local de la biblioteca de Epic Games, sin credenciales.
//!
//! # Orígenes que se leen
//!
//! | Origen | Ruta |
//! |---|---|
//! | Epic Games Launcher (Windows) | `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` |
//! | Epic Games Launcher (macOS) | `~/Library/Application Support/Epic/EpicGamesLauncher/Data/Manifests/*.item` |
//! | Legendary (Linux y cualquier sistema) | `$LEGENDARY_CONFIG_PATH`, `$XDG_CONFIG_HOME/legendary` o `~/.config/legendary` → `installed.json` |
//! | Heroic | `<config>/heroic/legendaryConfig/legendary/installed.json` |
//! | Heroic (Flatpak) | `~/.var/app/com.heroicgameslauncher.hgl/config/heroic/legendaryConfig/legendary/installed.json` |
//!
//! Los nombres de campo del `.item` (`AppName`, `DisplayName`, `InstallLocation`,
//! `InstallSize`, `LaunchExecutable`, `CatalogItemId`, `MainGameAppName`,
//! `bIsIncompleteInstall`) están tomados del mapeo que publica Legendary en
//! `legendary/models/egl.py`, que es la especificación pública de facto de este
//! formato. Los de `installed.json` vienen de `legendary/models/game.py`
//! (`app_name`, `title`, `install_path`, `executable`, `install_size`, `is_dlc`).
//!
//! # Epic no publica carátulas en el manifiesto
//!
//! `cover_url` y `header_url` quedan en `None` a propósito. Fabricar una URL a
//! partir del `CatalogItemId` sería inventar un dato que el manifiesto no trae.

use crate::db::rich_metadata::DrmState;
use crate::error::AppResult;
use crate::stores::launch::validate_epic_app_name;
use crate::stores::paths::{self, ReadOutcome};
use crate::stores::{
    DiscoveredGame, ExternalStore, MAX_DISCOVERED_GAMES, ScanSource, ScanStatus, StoreScan,
    heroic, merge_discovered, sanitize_https_url, sanitize_path, sanitize_size, sanitize_title,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Un `.item` real ocupa unos pocos kilobytes. 1 MiB es una cota generosa que
/// impide que un fichero manipulado provoque una lectura desmesurada.
const MAX_ITEM_BYTES: u64 = 1024 * 1024;

/// `installed.json` agrupa toda la biblioteca; se le da más margen, pero
/// acotado.
const MAX_INSTALLED_JSON_BYTES: u64 = 32 * 1024 * 1024;

/// Un `metadata/<AppName>.json` de Legendary es la ficha de catálogo de un solo
/// juego: unos pocos kilobytes. 4 MiB deja margen de sobra.
const MAX_METADATA_BYTES: u64 = 4 * 1024 * 1024;

/// Valor del runner con el que Heroic marca las entradas de Epic en su caché.
const HEROIC_EPIC_RUNNER: &str = "legendary";

/// Orígenes concretos que se van a leer. Se separa de [`scan`] para que los
/// tests puedan apuntar a un directorio temporal sin tocar variables de entorno
/// del proceso (que son globales y romperían los tests en paralelo).
#[derive(Debug, Clone, Default)]
pub struct EpicSources {
    /// Directorios con manifiestos `*.item`.
    pub manifest_directories: Vec<PathBuf>,
    /// Ficheros `installed.json` de Legendary o Heroic.
    pub installed_files: Vec<PathBuf>,
    /// Directorios `metadata/` de Legendary con al menos una ficha dentro: la
    /// biblioteca completa de la cuenta, no sólo lo instalado.
    pub metadata_directories: Vec<PathBuf>,
    /// `store_cache/legendary_library.json` de Heroic: biblioteca completa.
    pub library_caches: Vec<PathBuf>,
    /// Carpetas que prueban que un cliente de Epic está en este equipo aunque
    /// todavía no haya ninguna biblioteca que leer. Sirven para distinguir «no
    /// has iniciado sesión» de «no tienes el cliente», que son problemas
    /// distintos y se arreglan de forma distinta.
    pub client_markers: Vec<PathBuf>,
}

impl EpicSources {
    /// `true` cuando no hay **ninguna** biblioteca que leer. Los marcadores de
    /// cliente no cuentan: su presencia no aporta ni un juego.
    pub fn is_empty(&self) -> bool {
        self.manifest_directories.is_empty()
            && self.installed_files.is_empty()
            && self.metadata_directories.is_empty()
            && self.library_caches.is_empty()
    }

    /// `true` cuando hay algo de Epic en este equipo, con biblioteca o sin ella.
    pub fn client_is_present(&self) -> bool {
        !self.is_empty() || !self.client_markers.is_empty()
    }
}

/// Escanea la biblioteca de Epic en esta máquina.
pub fn scan() -> AppResult<StoreScan> {
    scan_sources(&detect_sources())
}

/// Resuelve los orígenes que existen de verdad en esta máquina.
pub fn detect_sources() -> EpicSources {
    let mut sources = EpicSources::default();

    for directory in candidate_manifest_directories() {
        if paths::is_real_directory(&directory) && !sources.manifest_directories.contains(&directory)
        {
            sources.manifest_directories.push(directory);
        }
    }
    for file in candidate_installed_files() {
        if paths::is_real_file(&file) && !sources.installed_files.contains(&file) {
            sources.installed_files.push(file);
        }
    }
    for directory in candidate_metadata_directories() {
        // Una carpeta `metadata/` vacía es justo lo que deja un Legendary
        // instalado y sin sesión: existe, pero no hay biblioteca dentro. Contarla
        // como origen sería volver a prometer datos que no hay.
        let has_metadata = paths::directory_is_readable(&directory)
            && !paths::list_files_with_extension(&directory, "json").is_empty();
        if has_metadata && !sources.metadata_directories.contains(&directory) {
            sources.metadata_directories.push(directory);
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

/// Todas las rutas que se sondean en esta máquina, existan o no.
///
/// La interfaz las enseña cuando no se detecta nada: «no se encontró Epic» sin
/// decir dónde se buscó es indistinguible de «Vindexa no sabe buscar».
pub fn searched_locations() -> Vec<PathBuf> {
    let mut locations = candidate_manifest_directories();
    locations.extend(candidate_installed_files());
    locations.extend(candidate_metadata_directories());
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

fn candidate_manifest_directories() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_data) = paths::program_data_directory() {
        candidates.push(
            program_data
                .join("Epic")
                .join("EpicGamesLauncher")
                .join("Data")
                .join("Manifests"),
        );
    }
    // En macOS el lanzador oficial escribe bajo la carpeta personal, no bajo la
    // raíz compartida.
    if let Some(home) = paths::home_directory() {
        candidates.push(
            home.join("Library")
                .join("Application Support")
                .join("Epic")
                .join("EpicGamesLauncher")
                .join("Data")
                .join("Manifests"),
        );
    }
    candidates
}

fn candidate_installed_files() -> Vec<PathBuf> {
    paths::legendary_config_directories()
        .into_iter()
        .map(|directory| directory.join("installed.json"))
        .collect()
}

/// Carpetas `metadata/` de Legendary. Ahí queda la ficha de **cada** juego que
/// la cuenta posee, la haya instalado o no.
fn candidate_metadata_directories() -> Vec<PathBuf> {
    paths::legendary_config_directories()
        .into_iter()
        .map(|directory| directory.join("metadata"))
        .collect()
}

/// Cachés de biblioteca de Heroic para Epic.
fn candidate_library_caches() -> Vec<PathBuf> {
    paths::heroic_data_directories()
        .into_iter()
        .map(|directory| {
            directory
                .join("store_cache")
                .join(heroic::LEGENDARY_LIBRARY_FILE)
        })
        .collect()
}

/// Carpetas cuya sola existencia prueba que hay un cliente de Epic instalado.
///
/// Se comprueba la carpeta, nunca su `user.json`: ese fichero guarda el token de
/// sesión de la persona usuaria y Vindexa no tiene ningún motivo para abrirlo.
fn candidate_client_markers() -> Vec<PathBuf> {
    let mut candidates = paths::legendary_config_directories();
    candidates.extend(paths::heroic_data_directories());
    candidates
}

/// Lee los orígenes indicados y consolida los juegos.
///
/// Cuando la lista está vacía el resultado es [`ScanStatus::Unavailable`] con su
/// motivo, nunca una lista vacía sin explicación.
pub fn scan_sources(sources: &EpicSources) -> AppResult<StoreScan> {
    if sources.is_empty() {
        return Ok(unavailable_for(sources));
    }

    let mut scan = StoreScan::empty(ExternalStore::Epic, ScanStatus::Success);
    // Se indexa por identificador para que un juego presente en dos orígenes no
    // se duplique. El manifiesto oficial se lee primero y manda.
    let mut games: BTreeMap<String, DiscoveredGame> = BTreeMap::new();

    for directory in &sources.manifest_directories {
        if scan.detected_root.is_none() {
            scan.detected_root = Some(paths::display_path(directory));
        }
        // Una carpeta de manifiestos vacía pero legible es una respuesta
        // legítima: Epic sólo escribe un `.item` por juego **instalado**.
        if !paths::directory_is_readable(directory) {
            scan.skipped = scan.skipped.saturating_add(1);
            continue;
        }
        scan.note_source(ScanSource::EpicManifests);
        for manifest in paths::list_files_with_extension(directory, "item") {
            if games.len() >= MAX_DISCOVERED_GAMES {
                scan.skipped = scan.skipped.saturating_add(1);
                continue;
            }
            match paths::read_text_file(&manifest, MAX_ITEM_BYTES) {
                ReadOutcome::Text(contents) => match parse_item_manifest(&contents) {
                    Some(game) => merge_discovered(&mut games, &mut scan, game),
                    None => scan.skipped = scan.skipped.saturating_add(1),
                },
                ReadOutcome::Missing => {}
                ReadOutcome::Unsafe | ReadOutcome::Unreadable => {
                    scan.skipped = scan.skipped.saturating_add(1);
                }
            }
        }
    }

    for file in &sources.installed_files {
        match paths::read_text_file(file, MAX_INSTALLED_JSON_BYTES) {
            ReadOutcome::Text(contents) => {
                let Some(entries) = parse_installed_json(&contents) else {
                    scan.skipped = scan.skipped.saturating_add(1);
                    continue;
                };
                scan.note_source(ScanSource::LegendaryInstalled);
                if scan.detected_root.is_none()
                    && let Some(parent) = file.parent()
                {
                    scan.detected_root = Some(paths::display_path(parent));
                }
                for (game, skipped) in entries {
                    scan.skipped = scan.skipped.saturating_add(skipped);
                    if let Some(game) = game {
                        merge_discovered(&mut games, &mut scan, game);
                    }
                }
            }
            ReadOutcome::Missing => {}
            ReadOutcome::Unsafe | ReadOutcome::Unreadable => {
                scan.skipped = scan.skipped.saturating_add(1);
            }
        }
    }

    // La biblioteca completa: fichas de Legendary y caché de Heroic. Se leen
    // después de los manifiestos a propósito, para que lo que dice el disco
    // sobre una instalación mande sobre lo que dice el catálogo.
    for directory in &sources.metadata_directories {
        if !paths::directory_is_readable(directory) {
            scan.skipped = scan.skipped.saturating_add(1);
            continue;
        }
        let mut read_any = false;
        for entry in paths::list_files_with_extension(directory, "json") {
            match paths::read_text_file(&entry, MAX_METADATA_BYTES) {
                ReadOutcome::Text(contents) => match parse_legendary_metadata(&contents) {
                    Some(game) => {
                        read_any = true;
                        merge_discovered(&mut games, &mut scan, game);
                    }
                    None => scan.skipped = scan.skipped.saturating_add(1),
                },
                ReadOutcome::Missing => {}
                ReadOutcome::Unsafe | ReadOutcome::Unreadable => {
                    scan.skipped = scan.skipped.saturating_add(1);
                }
            }
        }
        if read_any {
            scan.note_source(ScanSource::LegendaryMetadata);
            if scan.detected_root.is_none()
                && let Some(parent) = directory.parent()
            {
                scan.detected_root = Some(paths::display_path(parent));
            }
        }
    }

    for file in &sources.library_caches {
        match paths::read_text_file(file, heroic::MAX_LIBRARY_CACHE_BYTES) {
            ReadOutcome::Text(contents) => {
                let (entries, skipped) =
                    match heroic::parse_library_cache(&contents, HEROIC_EPIC_RUNNER) {
                        heroic::LibraryCache::Games { entries, skipped } => (entries, skipped),
                        // El cliente está instalado y todavía no ha traído su
                        // biblioteca: eso no es un fichero ilegible.
                        heroic::LibraryCache::Absent => continue,
                        heroic::LibraryCache::Malformed => {
                            scan.skipped = scan.skipped.saturating_add(1);
                            continue;
                        }
                    };
                scan.note_source(ScanSource::HeroicEpicLibrary);
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
        // Había algo y no se pudo leer: eso no es «cero juegos», es un fallo
        // que la persona usuaria debe poder ver.
        if scan.skipped > 0 {
            scan.status = ScanStatus::Failed;
            scan.error_code = Some("epic_manifests_unreadable".to_string());
            scan.error_message = Some(
                "Se encontró Epic en este equipo, pero ninguno de sus manifiestos se pudo leer."
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
/// Distinguir «no tienes el cliente» de «tienes el cliente y no has iniciado
/// sesión» importa porque lo que hay que hacer es distinto: instalar Heroic, o
/// abrirlo y entrar con tu cuenta de Epic.
fn unavailable_for(sources: &EpicSources) -> StoreScan {
    if sources.client_is_present() {
        StoreScan::unavailable(
            ExternalStore::Epic,
            "epic_not_signed_in",
            "Se encontró Legendary o Heroic en este equipo, pero no hay ninguna biblioteca de Epic guardada. Abre ese cliente, inicia sesión en Epic Games Store y vuelve a escanear.",
        )
    } else {
        StoreScan::unavailable(
            ExternalStore::Epic,
            "epic_client_not_found",
            "No se encontró el Epic Games Launcher ni una configuración de Legendary o Heroic en este equipo.",
        )
    }
}

// ---------------------------------------------------------------------------
// Manifiesto `.item` del Epic Games Launcher
// ---------------------------------------------------------------------------

/// Sólo los campos que Vindexa necesita. `serde` ignora el resto del manifiesto,
/// que es amplio y cambia entre versiones del lanzador.
#[derive(Debug, Deserialize)]
struct ItemManifest {
    #[serde(rename = "AppName")]
    app_name: Option<String>,
    #[serde(rename = "DisplayName")]
    display_name: Option<String>,
    #[serde(rename = "InstallLocation")]
    install_location: Option<String>,
    #[serde(rename = "InstallSize")]
    install_size: Option<i64>,
    #[serde(rename = "LaunchExecutable")]
    launch_executable: Option<String>,
    /// Presente y distinto de `AppName` cuando la entrada es un DLC o un
    /// complemento del juego principal; esas entradas no son juegos.
    #[serde(rename = "MainGameAppName")]
    main_game_app_name: Option<String>,
    #[serde(rename = "bIsIncompleteInstall")]
    is_incomplete_install: Option<bool>,
}

fn parse_item_manifest(contents: &str) -> Option<DiscoveredGame> {
    let manifest: ItemManifest = serde_json::from_str(contents).ok()?;
    let app_name = manifest.app_name.as_deref()?.trim();
    validate_epic_app_name(app_name).ok()?;

    // Un DLC comparte biblioteca con su juego base pero no es una entrada
    // independiente.
    if let Some(main_game) = manifest.main_game_app_name.as_deref()
        && !main_game.trim().is_empty()
        && main_game.trim() != app_name
    {
        return None;
    }

    // Sin título no se persiste: inventar «Epic App <id>» sería fabricar un
    // dato que el manifiesto no trae.
    let title = manifest.display_name.as_deref().and_then(sanitize_title)?;

    let install_directory = manifest
        .install_location
        .as_deref()
        .and_then(paths::canonical_install_directory);
    let launch_target = install_directory.as_ref().and_then(|directory| {
        let executable = manifest.launch_executable.as_deref()?;
        paths::resolve_executable_within(directory, executable).map(|path| paths::display_path(&path))
    });
    let install_path = install_directory
        .as_ref()
        .map(|directory| paths::display_path(directory))
        .and_then(|path| sanitize_path(&path));

    // «Instalado» significa que la carpeta existe de verdad y que el lanzador no
    // marcó la descarga como incompleta.
    let installed =
        install_path.is_some() && !manifest.is_incomplete_install.unwrap_or(false);

    Some(DiscoveredGame {
        external_id: app_name.to_string(),
        title,
        cover_url: None,
        header_url: None,
        install_path,
        installed,
        size_on_disk: manifest.install_size.and_then(sanitize_size),
        launch_target,
        drm_state: ExternalStore::Epic.catalogue_drm_state(),
        source: ScanSource::EpicManifests,
    })
}

// ---------------------------------------------------------------------------
// `installed.json` de Legendary y Heroic
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LegendaryInstalledGame {
    app_name: Option<String>,
    title: Option<String>,
    install_path: Option<String>,
    executable: Option<String>,
    install_size: Option<i64>,
    #[serde(default)]
    is_dlc: bool,
}

/// Devuelve una entrada por juego junto al recuento de descartes, o `None` si el
/// fichero entero no es un JSON válido.
fn parse_installed_json(contents: &str) -> Option<Vec<(Option<DiscoveredGame>, u32)>> {
    // Legendary escribe un objeto indexado por `app_name`.
    let entries: BTreeMap<String, serde_json::Value> = serde_json::from_str(contents).ok()?;
    Some(
        entries
            .into_iter()
            .map(|(key, value)| match serde_json::from_value::<LegendaryInstalledGame>(value) {
                Ok(entry) => match parse_installed_entry(&key, entry) {
                    Some(game) => (Some(game), 0),
                    None => (None, 1),
                },
                Err(_) => (None, 1),
            })
            .collect(),
    )
}

fn parse_installed_entry(key: &str, entry: LegendaryInstalledGame) -> Option<DiscoveredGame> {
    if entry.is_dlc {
        return None;
    }
    let app_name = entry
        .app_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(key.trim());
    validate_epic_app_name(app_name).ok()?;
    let title = entry.title.as_deref().and_then(sanitize_title)?;

    let install_directory = entry
        .install_path
        .as_deref()
        .and_then(paths::canonical_install_directory);
    let launch_target = install_directory.as_ref().and_then(|directory| {
        let executable = entry.executable.as_deref()?;
        paths::resolve_executable_within(directory, executable).map(|path| paths::display_path(&path))
    });
    let install_path = install_directory
        .as_ref()
        .map(|directory| paths::display_path(directory))
        .and_then(|path| sanitize_path(&path));

    Some(DiscoveredGame {
        external_id: app_name.to_string(),
        title,
        cover_url: None,
        header_url: None,
        installed: install_path.is_some(),
        install_path,
        size_on_disk: entry.install_size.and_then(sanitize_size),
        launch_target,
        drm_state: DrmState::Unknown,
        source: ScanSource::LegendaryInstalled,
    })
}

// ---------------------------------------------------------------------------
// `metadata/<AppName>.json` de Legendary: la biblioteca completa
// ---------------------------------------------------------------------------

/// Ficha de catálogo tal y como Legendary la vuelca en disco tras iniciar
/// sesión. Es la biblioteca **entera** de la cuenta: hay una ficha por juego
/// poseído, esté instalado o no.
#[derive(Debug, Deserialize)]
struct LegendaryMetadataFile {
    app_name: Option<String>,
    app_title: Option<String>,
    #[serde(default)]
    asset_infos: BTreeMap<String, LegendaryAssetInfo>,
    metadata: Option<LegendaryCatalogItem>,
}

#[derive(Debug, Deserialize)]
struct LegendaryAssetInfo {
    namespace: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegendaryCatalogItem {
    title: Option<String>,
    #[serde(rename = "keyImages", default)]
    key_images: Vec<LegendaryKeyImage>,
    #[serde(default)]
    categories: Vec<LegendaryCategory>,
    /// Presente sólo en los complementos: identifica el juego base al que
    /// pertenecen. Es el mismo criterio que usa Legendary para su `is_dlc`.
    #[serde(rename = "mainGameItem")]
    main_game_item: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct LegendaryKeyImage {
    #[serde(rename = "type")]
    image_type: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LegendaryCategory {
    path: Option<String>,
}

/// Namespace con el que Epic marca el contenido del Unreal Engine Marketplace.
/// No son juegos y Legendary también los descarta.
const UNREAL_ENGINE_NAMESPACE: &str = "ue";

fn parse_legendary_metadata(contents: &str) -> Option<DiscoveredGame> {
    let file: LegendaryMetadataFile = serde_json::from_str(contents).ok()?;
    let app_name = file.app_name.as_deref()?.trim();
    validate_epic_app_name(app_name).ok()?;

    // Assets del Unreal Engine Marketplace: contenido para desarrollar, no
    // juegos de la biblioteca.
    if file
        .asset_infos
        .values()
        .any(|asset| asset.namespace.as_deref() == Some(UNREAL_ENGINE_NAMESPACE))
    {
        return None;
    }

    let catalog = file.metadata.as_ref();
    // Un complemento comparte biblioteca con su juego base y no es una entrada
    // independiente. Los mods tampoco lo son.
    if catalog.is_some_and(|item| item.main_game_item.is_some()) {
        return None;
    }
    if catalog.is_some_and(|item| {
        item.categories
            .iter()
            .any(|category| category.path.as_deref() == Some("mods"))
    }) {
        return None;
    }

    // Sin título no se persiste: fabricarlo a partir del identificador sería
    // inventar un dato que la ficha no trae.
    let title = file
        .app_title
        .as_deref()
        .and_then(sanitize_title)
        .or_else(|| {
            catalog
                .and_then(|item| item.title.as_deref())
                .and_then(sanitize_title)
        })?;

    let key_image = |wanted: &str| -> Option<String> {
        catalog?
            .key_images
            .iter()
            .find(|image| image.image_type.as_deref() == Some(wanted))
            .and_then(|image| image.url.as_deref())
            .and_then(sanitize_https_url)
    };

    Some(DiscoveredGame {
        external_id: app_name.to_string(),
        title,
        // Nombres publicados por Epic en su propio catálogo: la portada vertical
        // y la imagen ancha de la ficha. Si Epic no las trae, se quedan vacías.
        cover_url: key_image("DieselGameBoxTall").or_else(|| key_image("OfferImageTall")),
        header_url: key_image("DieselGameBox").or_else(|| key_image("OfferImageWide")),
        // La ficha de catálogo describe lo que se posee, no lo que hay en disco.
        install_path: None,
        installed: false,
        size_on_disk: None,
        launch_target: None,
        drm_state: ExternalStore::Epic.catalogue_drm_state(),
        source: ScanSource::LegendaryMetadata,
    })
}

// ---------------------------------------------------------------------------
// `store_cache/legendary_library.json` de Heroic
// ---------------------------------------------------------------------------

/// Convierte una entrada de la caché de Heroic en un juego de Epic.
///
/// La ruta de instalación que trae la caché se revalida contra el disco: Heroic
/// puede haber guardado un juego que ya se borró a mano.
fn convert_heroic_entry(entry: heroic::HeroicLibraryEntry) -> Option<DiscoveredGame> {
    validate_epic_app_name(&entry.app_name).ok()?;
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
        size_on_disk: None,
        launch_target,
        drm_state: ExternalStore::Epic.catalogue_drm_state(),
        source: ScanSource::HeroicEpicLibrary,
    })
}

#[cfg(test)]
mod tests {
    use super::{EpicSources, scan_sources};
    use crate::db::rich_metadata::DrmState;
    use crate::stores::{ScanSource, ScanStatus};
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("crear carpeta");
        }
        fs::write(path, contents).expect("escribir fichero");
    }

    fn install_directory(root: &Path, name: &str, executable: &str) -> PathBuf {
        let directory = root.join(name);
        fs::create_dir_all(&directory).expect("crear instalación");
        fs::write(directory.join(executable), "binario").expect("escribir ejecutable");
        directory
    }

    #[test]
    fn an_absent_client_reports_unavailable_with_a_reason_in_spanish() {
        let scan = scan_sources(&EpicSources::default()).expect("escanear sin orígenes");
        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.error_code.as_deref(), Some("epic_client_not_found"));
        let message = scan.error_message.expect("motivo");
        assert!(message.contains("Epic Games Launcher"));
        assert!(scan.games.is_empty());
    }

    #[test]
    fn a_valid_manifest_becomes_a_game_with_its_real_installation() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        let install = install_directory(root.path(), "Hollow Knight", "hollow_knight.exe");
        write(
            &manifests.join("hk.item"),
            &format!(
                r#"{{
                  "FormatVersion": 0,
                  "AppName": "0a1b2c3d4e5f",
                  "DisplayName": "Hollow  Knight",
                  "CatalogItemId": "abcdef0123456789",
                  "CatalogNamespace": "espacio",
                  "InstallLocation": {install:?},
                  "InstallSize": 9663676416,
                  "LaunchExecutable": "hollow_knight.exe",
                  "bIsIncompleteInstall": false
                }}"#,
                install = install.to_str().expect("ruta utf-8")
            ),
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            installed_files: Vec::new(),
            ..Default::default()
        })
        .expect("escanear manifiestos");

        assert_eq!(scan.status, ScanStatus::Success);
        assert_eq!(scan.sources, vec![ScanSource::EpicManifests]);
        assert_eq!(scan.games.len(), 1);
        let game = &scan.games[0];
        assert_eq!(game.external_id, "0a1b2c3d4e5f");
        assert_eq!(game.title, "Hollow Knight");
        assert!(game.installed);
        assert_eq!(game.size_on_disk, Some(9_663_676_416));
        assert!(game.launch_target.as_deref().unwrap().ends_with("hollow_knight.exe"));
        // Epic no publica carátula en el manifiesto: no se inventa una URL.
        assert_eq!(game.cover_url, None);
        assert_eq!(game.header_url, None);
        // Y Epic no permite afirmar nada sobre DRM.
        assert_eq!(game.drm_state, DrmState::Unknown);
    }

    #[test]
    fn corrupt_empty_and_hostile_manifests_are_skipped_without_breaking_the_scan() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        let install = install_directory(root.path(), "Bueno", "bueno.exe");

        write(&manifests.join("corrupto.item"), "{ esto no es json");
        write(&manifests.join("vacio.item"), "");
        write(&manifests.join("array.item"), "[]");
        write(&manifests.join("sin-nombre.item"), r#"{"DisplayName": "Sin AppName"}"#);
        write(&manifests.join("sin-titulo.item"), r#"{"AppName": "SoloId"}"#);
        write(
            &manifests.join("titulo-vacio.item"),
            r#"{"AppName": "SoloId2", "DisplayName": "   "}"#,
        );
        // Identificador con forma de inyección: se descarta entero.
        write(
            &manifests.join("inyeccion.item"),
            r#"{"AppName": "malo?action=uninstall", "DisplayName": "Malicioso"}"#,
        );
        // Ruta que escapa de su carpeta: el juego se conserva, pero sin
        // instalación ni objetivo de lanzamiento.
        write(
            &manifests.join("escape.item"),
            r#"{"AppName": "Escapista", "DisplayName": "Escapista",
                "InstallLocation": "../../../etc", "LaunchExecutable": "../../bin/sh"}"#,
        );
        write(
            &manifests.join("bueno.item"),
            &format!(
                r#"{{"AppName": "Bueno", "DisplayName": "Juego Bueno",
                     "InstallLocation": {install:?}, "LaunchExecutable": "bueno.exe"}}"#,
                install = install.to_str().expect("ruta utf-8")
            ),
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            installed_files: Vec::new(),
            ..Default::default()
        })
        .expect("escanear manifiestos mixtos");

        assert_eq!(scan.status, ScanStatus::Success);
        let identifiers = scan
            .games
            .iter()
            .map(|game| game.external_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(identifiers, vec!["Bueno", "Escapista"]);

        let escapist = scan
            .games
            .iter()
            .find(|game| game.external_id == "Escapista")
            .expect("el juego se conserva");
        assert_eq!(escapist.install_path, None);
        assert_eq!(escapist.launch_target, None);
        assert!(!escapist.installed);

        // Siete entradas inservibles descartadas y contadas: corrupta, vacía,
        // array, sin AppName, sin título, título en blanco e identificador con
        // forma de inyección.
        assert_eq!(scan.skipped, 7);
    }

    #[test]
    fn dlc_entries_are_not_listed_as_games() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        write(
            &manifests.join("base.item"),
            r#"{"AppName": "JuegoBase", "DisplayName": "Juego Base",
                "MainGameAppName": "JuegoBase"}"#,
        );
        write(
            &manifests.join("dlc.item"),
            r#"{"AppName": "JuegoBaseDlc", "DisplayName": "Juego Base - Expansión",
                "MainGameAppName": "JuegoBase"}"#,
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            installed_files: Vec::new(),
            ..Default::default()
        })
        .expect("escanear con DLC");
        assert_eq!(scan.games.len(), 1);
        assert_eq!(scan.games[0].external_id, "JuegoBase");
    }

    #[test]
    fn an_incomplete_download_is_listed_but_not_marked_as_installed() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        let install = install_directory(root.path(), "Descargando", "juego.exe");
        write(
            &manifests.join("parcial.item"),
            &format!(
                r#"{{"AppName": "Parcial", "DisplayName": "Descarga a medias",
                     "InstallLocation": {install:?}, "bIsIncompleteInstall": true}}"#,
                install = install.to_str().expect("ruta utf-8")
            ),
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            installed_files: Vec::new(),
            ..Default::default()
        })
        .expect("escanear descarga parcial");
        assert_eq!(scan.games.len(), 1);
        assert!(!scan.games[0].installed);
        // La ruta sí se conserva: existe y es válida.
        assert!(scan.games[0].install_path.is_some());
    }

    #[test]
    fn legendary_installed_json_is_read_when_the_official_client_is_absent() {
        let root = TempDir::new().expect("crear temporal");
        let install = install_directory(root.path(), "Juego Legendary", "juego");
        let installed = root.path().join("legendary").join("installed.json");
        write(
            &installed,
            &format!(
                r#"{{
                  "Snapdragon": {{
                    "app_name": "Snapdragon",
                    "title": "Un Juego de Epic",
                    "version": "1.0",
                    "install_path": {install:?},
                    "executable": "juego",
                    "install_size": 1234567,
                    "is_dlc": false
                  }},
                  "SnapdragonDlc": {{
                    "app_name": "SnapdragonDlc",
                    "title": "Su expansión",
                    "version": "1.0",
                    "install_path": {install:?},
                    "is_dlc": true
                  }}
                }}"#,
                install = install.to_str().expect("ruta utf-8")
            ),
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: Vec::new(),
            installed_files: vec![installed],
            ..Default::default()
        })
        .expect("escanear legendary");

        assert_eq!(scan.status, ScanStatus::Success);
        assert_eq!(scan.sources, vec![ScanSource::LegendaryInstalled]);
        assert_eq!(scan.games.len(), 1);
        assert_eq!(scan.games[0].external_id, "Snapdragon");
        assert_eq!(scan.games[0].title, "Un Juego de Epic");
        assert_eq!(scan.games[0].size_on_disk, Some(1_234_567));
        assert_eq!(scan.skipped, 1);
    }

    #[test]
    fn the_official_manifest_wins_over_the_legendary_copy_of_the_same_game() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        write(
            &manifests.join("oficial.item"),
            r#"{"AppName": "Compartido", "DisplayName": "Título oficial"}"#,
        );
        let installed = root.path().join("installed.json");
        write(
            &installed,
            r#"{"Compartido": {"app_name": "Compartido", "title": "Título de Legendary",
                 "version": "1", "install_path": "/no/existe"}}"#,
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            installed_files: vec![installed],
            ..Default::default()
        })
        .expect("escanear ambos orígenes");
        assert_eq!(scan.games.len(), 1);
        assert_eq!(scan.games[0].title, "Título oficial");
        assert_eq!(scan.sources.len(), 2);
    }

    #[test]
    fn an_installed_client_with_no_installed_games_is_success_not_failure() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        fs::create_dir_all(&manifests).expect("crear carpeta de manifiestos");

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            installed_files: Vec::new(),
            ..Default::default()
        })
        .expect("escanear carpeta vacía");

        // Epic sólo escribe un `.item` por juego instalado: una carpeta vacía
        // significa «no tienes nada instalado», no «algo ha fallado».
        assert_eq!(scan.status, ScanStatus::Success);
        assert!(scan.games.is_empty());
        assert_eq!(scan.error_code, None);
        assert!(scan.detected_root.is_some());
    }

    #[test]
    fn a_present_but_unreadable_client_is_a_failure_not_an_empty_library() {
        let root = TempDir::new().expect("crear temporal");
        // Un `installed.json` ilegible (JSON roto) es el único origen: no se
        // puede afirmar que la biblioteca esté vacía.
        let installed = root.path().join("installed.json");
        write(&installed, "{ esto no es json");

        let scan = scan_sources(&EpicSources {
            manifest_directories: Vec::new(),
            installed_files: vec![installed],
            ..Default::default()
        })
        .expect("escanear origen ilegible");

        assert_eq!(scan.status, ScanStatus::Failed);
        assert_eq!(scan.error_code.as_deref(), Some("epic_manifests_unreadable"));
        assert_eq!(scan.skipped, 1);
        let message = scan.error_message.expect("motivo");
        // El mensaje no revela dónde estaba mirando.
        assert!(!message.contains('/'));
        assert!(!message.contains('\\'));
    }

    /// Escribe una ficha de catálogo de Legendary como la que deja tras iniciar
    /// sesión: una por juego poseído, esté instalado o no.
    fn legendary_metadata(directory: &Path, app_name: &str, body: &str) {
        write(&directory.join(format!("{app_name}.json")), body);
    }

    #[test]
    fn legendary_metadata_gives_the_whole_library_not_only_what_is_installed() {
        let root = TempDir::new().expect("crear temporal");
        let metadata = root.path().join("metadata");
        fs::create_dir_all(&metadata).expect("crear metadata");

        legendary_metadata(
            &metadata,
            "Fallout3",
            r#"{"app_name": "Fallout3", "app_title": "Fallout 3",
                "asset_infos": {"Windows": {"namespace": "espacio"}},
                "metadata": {"title": "Fallout 3",
                  "categories": [{"path": "games"}],
                  "keyImages": [
                    {"type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/fo3_tall.jpg"},
                    {"type": "DieselGameBox", "url": "https://cdn1.epicgames.com/fo3_wide.jpg"}]}}"#,
        );
        // Un complemento: comparte biblioteca con su juego base.
        legendary_metadata(
            &metadata,
            "Fallout3Dlc",
            r#"{"app_name": "Fallout3Dlc", "app_title": "Fallout 3 - Broken Steel",
                "metadata": {"title": "Broken Steel", "mainGameItem": {"id": "abc"}}}"#,
        );
        // Contenido del Unreal Engine Marketplace: no es un juego.
        legendary_metadata(
            &metadata,
            "PluginX",
            r#"{"app_name": "PluginX", "app_title": "Plugin X",
                "asset_infos": {"Windows": {"namespace": "ue"}},
                "metadata": {"title": "Plugin X"}}"#,
        );
        // Un mod: Legendary lo descarta y aquí también.
        legendary_metadata(
            &metadata,
            "ModY",
            r#"{"app_name": "ModY", "app_title": "Mod Y",
                "metadata": {"title": "Mod Y", "categories": [{"path": "mods"}]}}"#,
        );

        let scan = scan_sources(&EpicSources {
            metadata_directories: vec![metadata],
            ..Default::default()
        })
        .expect("escanear fichas de Legendary");

        assert_eq!(scan.status, ScanStatus::Success);
        assert!(scan.sources.contains(&ScanSource::LegendaryMetadata));
        assert_eq!(scan.games.len(), 1);
        let game = &scan.games[0];
        assert_eq!(game.external_id, "Fallout3");
        assert_eq!(game.title, "Fallout 3");
        // La ficha describe lo que se posee, no lo que hay en disco.
        assert!(!game.installed);
        assert_eq!(game.install_path, None);
        assert_eq!(
            game.cover_url.as_deref(),
            Some("https://cdn1.epicgames.com/fo3_tall.jpg")
        );
        assert_eq!(
            game.header_url.as_deref(),
            Some("https://cdn1.epicgames.com/fo3_wide.jpg")
        );
        assert_eq!(scan.skipped, 3);
    }

    #[test]
    fn a_client_that_never_signed_in_says_exactly_that_instead_of_pretending_to_be_absent() {
        let root = TempDir::new().expect("crear temporal");
        // Legendary instalado y sin sesión: su carpeta existe y no hay
        // biblioteca dentro. Es un problema distinto de «no tienes el cliente»
        // y se arregla de forma distinta.
        let config = root.path().join("legendary");
        fs::create_dir_all(config.join("metadata")).expect("crear config de Legendary");

        let scan = scan_sources(&EpicSources {
            client_markers: vec![config],
            ..Default::default()
        })
        .expect("escanear cliente sin sesión");

        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.error_code.as_deref(), Some("epic_not_signed_in"));
        assert!(scan.games.is_empty());
        let message = scan.error_message.expect("motivo");
        assert!(message.contains("inicia sesión"));
        assert!(!message.contains('/'));

        // Sin ninguna evidencia de cliente, el motivo vuelve a ser el otro.
        let scan = scan_sources(&EpicSources::default()).expect("escanear sin nada");
        assert_eq!(scan.error_code.as_deref(), Some("epic_client_not_found"));
    }

    #[test]
    fn the_heroic_library_cache_completes_the_manifest_without_overwriting_it() {
        let root = TempDir::new().expect("crear temporal");
        let manifests = root.path().join("Manifests");
        let install = install_directory(root.path(), "Hollow Knight", "juego.exe");
        write(
            &manifests.join("hk.item"),
            &format!(
                r#"{{"AppName": "HollowKnight", "DisplayName": "Hollow Knight",
                     "InstallLocation": {install:?}, "InstallSize": 4096,
                     "LaunchExecutable": "juego.exe", "bIsIncompleteInstall": false}}"#,
                install = install.to_str().expect("ruta utf-8")
            ),
        );

        let cache = root.path().join("store_cache").join("legendary_library.json");
        write(
            &cache,
            r#"{"games": [
                {"runner": "legendary", "app_name": "HollowKnight", "title": "Hollow Knight",
                 "art_cover": "https://cdn1.epicgames.com/hk.jpg", "is_installed": true,
                 "install": {"install_path": "/ruta/que/ya/no/existe"}},
                {"runner": "legendary", "app_name": "Celeste", "title": "Celeste",
                 "art_cover": "https://cdn1.epicgames.com/celeste.jpg", "is_installed": false,
                 "install": {}}
            ]}"#,
        );

        let scan = scan_sources(&EpicSources {
            manifest_directories: vec![manifests],
            library_caches: vec![cache],
            ..Default::default()
        })
        .expect("escanear manifiesto y caché");

        assert_eq!(scan.status, ScanStatus::Success);
        assert!(scan.sources.contains(&ScanSource::HeroicEpicLibrary));
        assert_eq!(scan.games.len(), 2);

        let hollow = scan
            .games
            .iter()
            .find(|game| game.external_id == "HollowKnight")
            .expect("el juego del manifiesto");
        // La instalación real del manifiesto manda: la caché no la sustituye.
        assert!(hollow.installed);
        let canonical = fs::canonicalize(&install).expect("canonicalizar la instalación");
        assert_eq!(
            hollow.install_path.as_deref(),
            Some(canonical.to_str().expect("ruta utf-8"))
        );
        // Pero la carátula, que el manifiesto no trae, sí se completa.
        assert_eq!(
            hollow.cover_url.as_deref(),
            Some("https://cdn1.epicgames.com/hk.jpg")
        );

        let celeste = scan
            .games
            .iter()
            .find(|game| game.external_id == "Celeste")
            .expect("el juego que sólo está en la biblioteca");
        assert!(!celeste.installed);
        assert_eq!(celeste.install_path, None);
    }
}
