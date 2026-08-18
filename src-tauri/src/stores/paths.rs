//! Resolución y lectura **defensiva** de los ficheros que los clientes de Epic
//! y GOG escriben en disco.
//!
//! Replica la disciplina de `crate::steam::family` y `crate::steam::local`:
//!
//! * Se comprueba el tipo real del fichero con `symlink_metadata`, de modo que
//!   un enlace simbólico colocado por otro proceso no consiga que Vindexa lea
//!   un fichero arbitrario del sistema.
//! * Todo fichero leído tiene un límite de tamaño explícito. Ningún manifiesto
//!   de terceros puede provocar una lectura ilimitada.
//! * La ausencia del fichero **no es un error**: es [`ReadOutcome::Missing`], que
//!   el escáner traduce a un estado explícito.
//! * Un fichero ilegible o inseguro se descarta y se cuenta; no revienta el
//!   escaneo completo.
//! * Las rutas de instalación se canonicalizan y se comprueba que sigan
//!   contenidas dentro de la carpeta declarada antes de usarse.

use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Tope de entradas que se recorren en un directorio de manifiestos. Un
/// directorio con millones de entradas no debe bloquear la aplicación.
pub(crate) const MAX_DIRECTORY_ENTRIES: usize = 20_000;

/// Resultado de intentar leer un fichero de un cliente de terceros.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadOutcome {
    /// El fichero no existe. El cliente puede no estar instalado.
    Missing,
    /// Existe, pero no es un fichero regular, es un enlace simbólico o supera
    /// el límite de tamaño. Se descarta deliberadamente.
    Unsafe,
    /// Existe y es seguro, pero el sistema de archivos no lo devolvió (permisos,
    /// disco desconectado, contenido no UTF-8).
    Unreadable,
    Text(String),
}

/// Lee un fichero de texto respetando el límite de tamaño y rechazando enlaces
/// simbólicos y ficheros especiales.
pub(crate) fn read_text_file(path: &Path, max_bytes: u64) -> ReadOutcome {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ReadOutcome::Missing,
        Err(_) => return ReadOutcome::Unreadable,
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return ReadOutcome::Unsafe;
    }
    if metadata.len() > max_bytes {
        return ReadOutcome::Unsafe;
    }
    match fs::read_to_string(path) {
        Ok(contents) => ReadOutcome::Text(contents),
        Err(_) => ReadOutcome::Unreadable,
    }
}

/// Comprueba que una ruta apunta a un directorio real (no a un enlace).
pub(crate) fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

/// Comprueba que un directorio existe **y** que el sistema deja listarlo. Un
/// directorio vacío pero legible es una respuesta legítima («no hay juegos
/// instalados»); uno ilegible es un fallo que hay que decir.
pub(crate) fn directory_is_readable(path: &Path) -> bool {
    is_real_directory(path) && fs::read_dir(path).is_ok()
}

/// Comprueba que una ruta apunta a un fichero real (no a un enlace).
pub(crate) fn is_real_file(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_file() && !metadata.file_type().is_symlink())
        .unwrap_or(false)
}

/// Lista los ficheros regulares de un directorio cuya extensión coincide,
/// ordenados para que el escaneo sea determinista. Nunca sigue enlaces y nunca
/// recorre más de [`MAX_DIRECTORY_ENTRIES`].
pub(crate) fn list_files_with_extension(directory: &Path, extension: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files = Vec::new();
    for entry in entries.take(MAX_DIRECTORY_ENTRIES) {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(extension) {
            continue;
        }
        files.push(path);
    }
    files.sort();
    files
}

/// Lista los subdirectorios inmediatos de una carpeta, ordenados. Se usa para
/// recorrer las carpetas de juegos de GOG cuando Galaxy no está instalado.
pub(crate) fn list_subdirectories(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut directories = Vec::new();
    for entry in entries.take(MAX_DIRECTORY_ENTRIES) {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        directories.push(entry.path());
    }
    directories.sort();
    directories
}

// ---------------------------------------------------------------------------
// Validación de rutas declaradas por un manifiesto
// ---------------------------------------------------------------------------

/// Canonicaliza un directorio declarado en un manifiesto. Devuelve `None`
/// cuando la ruta no existe, no es un directorio o el manifiesto trae una ruta
/// relativa (que sería ambigua respecto al directorio de trabajo).
pub(crate) fn canonical_install_directory(declared: &str) -> Option<PathBuf> {
    let raw = Path::new(declared.trim());
    if raw.as_os_str().is_empty() || !raw.is_absolute() {
        return None;
    }
    let canonical = fs::canonicalize(raw).ok()?;
    is_real_directory(&canonical).then_some(canonical)
}

/// Resuelve el ejecutable de arranque declarado por un manifiesto **dentro** de
/// la carpeta de instalación.
///
/// Esta es la frontera que impide que un manifiesto manipulado convierta
/// «lanzar un juego» en «ejecutar cualquier binario del sistema»: la ruta
/// relativa se rechaza si contiene `..`, una raíz o un prefijo de volumen, y el
/// resultado canonicalizado debe seguir contenido dentro de la instalación
/// canonicalizada.
pub(crate) fn resolve_executable_within(
    install_directory: &Path,
    declared_executable: &str,
) -> Option<PathBuf> {
    let relative = Path::new(declared_executable.trim());
    if relative.as_os_str().is_empty() {
        return None;
    }
    // Una ruta absoluta solo se acepta si ya está contenida en la instalación:
    // los `goggame-*.info` de GOG declaran rutas absolutas legítimas.
    let candidate = if relative.is_absolute() {
        relative.to_path_buf()
    } else {
        if relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        }) {
            return None;
        }
        install_directory.join(relative)
    };
    let canonical = fs::canonicalize(&candidate).ok()?;
    if !is_real_file(&canonical) {
        return None;
    }
    is_contained_in(install_directory, &canonical).then_some(canonical)
}

/// Comprueba que una ruta ya canonicalizada sigue contenida dentro de otra.
pub(crate) fn is_contained_in(root: &Path, candidate: &Path) -> bool {
    let (Ok(root), Ok(candidate)) = (fs::canonicalize(root), fs::canonicalize(candidate)) else {
        return false;
    };
    candidate.starts_with(root)
}

// ---------------------------------------------------------------------------
// Directorios base del sistema operativo
// ---------------------------------------------------------------------------

fn env_path(key: &str) -> Option<PathBuf> {
    let value: OsString = std::env::var_os(key)?;
    if value.is_empty() {
        return None;
    }
    let path = PathBuf::from(value);
    path.is_absolute().then_some(path)
}

/// Carpeta personal de la persona usuaria.
pub(crate) fn home_directory() -> Option<PathBuf> {
    env_path("HOME")
        .or_else(|| {
            // Windows anterior a la existencia de `USERPROFILE` en el entorno
            // del proceso: `%HOMEDRIVE%%HOMEPATH%` concatenados.
            let mut combined = std::env::var_os("HOMEDRIVE")?;
            combined.push(std::env::var_os("HOMEPATH")?);
            let combined = PathBuf::from(combined);
            combined.is_absolute().then_some(combined)
        })
        .or_else(|| env_path("USERPROFILE"))
}

/// Equivalente de `%ProgramData%`: datos compartidos por todas las cuentas.
///
/// En macOS, la documentación oficial del SDK de GOG Galaxy publica
/// `/Users/Shared/GOG.com/Galaxy/...` como el equivalente exacto de
/// `%programdata%\GOG.com\Galaxy\...`, por eso es la raíz que se usa aquí.
pub(crate) fn program_data_directory() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env_path("ProgramData").or_else(|| Some(PathBuf::from("C:\\ProgramData")))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Users/Shared"))
    } else {
        None
    }
}

/// Equivalente de `%APPDATA%` (Windows) / `~/Library/Application Support`
/// (macOS) / `$XDG_CONFIG_HOME` o `~/.config` (Linux). Es la raíz donde Electron
/// —y por tanto Heroic— coloca su carpeta de configuración.
pub(crate) fn roaming_config_directory() -> Option<PathBuf> {
    if cfg!(target_os = "windows") {
        env_path("APPDATA")
    } else if cfg!(target_os = "macos") {
        Some(
            home_directory()?
                .join("Library")
                .join("Application Support"),
        )
    } else {
        xdg_config_home()
    }
}

/// `$XDG_CONFIG_HOME` con su valor por defecto `~/.config`.
pub(crate) fn xdg_config_home() -> Option<PathBuf> {
    env_path("XDG_CONFIG_HOME").or_else(|| Some(home_directory()?.join(".config")))
}

/// Raíz de configuración de las aplicaciones Flatpak.
pub(crate) fn flatpak_config_directory(application_id: &str) -> Option<PathBuf> {
    Some(
        home_directory()?
            .join(".var")
            .join("app")
            .join(application_id)
            .join("config"),
    )
}

/// Identificador de la aplicación Flatpak de Heroic Games Launcher.
///
/// Vive aquí, y no en `epic.rs` y `gog.rs` por separado, porque Heroic es un
/// único cliente que guarda las dos tiendas bajo la misma carpeta: duplicar la
/// constante haría que arreglar una ruta dejara la otra rota.
pub(crate) const HEROIC_FLATPAK_ID: &str = "com.heroicgameslauncher.hgl";

/// Carpetas de datos de Heroic en esta máquina, en orden de preferencia.
///
/// Heroic es una aplicación Electron: escribe en `app.getPath('appData')/heroic`,
/// que en macOS es `~/Library/Application Support/heroic`, en Windows
/// `%APPDATA%\heroic` y en Linux `~/.config/heroic`. Bajo esa carpeta cuelgan
/// `gog_store/`, `legendaryConfig/` y `store_cache/`.
///
/// **`~/Games/Heroic` no es esta carpeta**: es la carpeta donde Heroic *instala*
/// los juegos y donde crea los prefijos de Wine, y sobrevive a la desinstalación
/// del cliente. Confundirlas es exactamente lo que hacía que Vindexa dijera
/// «tienda detectada» sobre los restos de un Heroic que ya no está.
pub(crate) fn heroic_data_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut push = |directory: Option<PathBuf>| {
        if let Some(directory) = directory
            && !directories.contains(&directory)
        {
            directories.push(directory);
        }
    };
    push(roaming_config_directory().map(|root| root.join("heroic")));
    push(flatpak_config_directory(HEROIC_FLATPAK_ID).map(|root| root.join("heroic")));
    push(xdg_config_home().map(|root| root.join("heroic")));
    directories
}

/// Carpetas de configuración de Legendary en esta máquina, en orden de
/// preferencia.
///
/// Legendary se usa de dos maneras: suelto (su propia carpeta de configuración,
/// que respeta `LEGENDARY_CONFIG_PATH`) y empotrado en Heroic
/// (`<datos de Heroic>/legendaryConfig/legendary`). Ambas se sondean.
pub(crate) fn legendary_config_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    let mut push = |directory: Option<PathBuf>| {
        if let Some(directory) = directory
            && !directories.contains(&directory)
        {
            directories.push(directory);
        }
    };
    push(
        std::env::var_os("LEGENDARY_CONFIG_PATH")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute()),
    );
    push(xdg_config_home().map(|root| root.join("legendary")));
    for heroic in heroic_data_directories() {
        push(Some(heroic.join("legendaryConfig").join("legendary")));
    }
    push(flatpak_config_directory(HEROIC_FLATPAK_ID).map(|root| root.join("legendary")));
    directories
}

/// Devuelve la ruta como texto persistible.
pub(crate) fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        ReadOutcome, canonical_install_directory, is_contained_in, is_real_directory,
        list_files_with_extension, list_subdirectories, read_text_file, resolve_executable_within,
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn a_missing_file_is_absence_not_failure() {
        let directory = TempDir::new().expect("crear temporal");
        assert_eq!(
            read_text_file(&directory.path().join("no-existe.item"), 1024),
            ReadOutcome::Missing
        );
    }

    #[test]
    fn an_oversized_manifest_is_skipped_instead_of_being_loaded() {
        let directory = TempDir::new().expect("crear temporal");
        let path = directory.path().join("grande.item");
        fs::write(&path, "x".repeat(4096)).expect("escribir manifiesto grande");
        assert_eq!(read_text_file(&path, 1024), ReadOutcome::Unsafe);
        assert!(matches!(
            read_text_file(&path, 8192),
            ReadOutcome::Text(contents) if contents.len() == 4096
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_manifest_is_never_followed() {
        // Se importa aquí porque sólo esta prueba lo necesita, y sólo existe
        // enlace simbólico que seguir en los sistemas tipo Unix.
        use super::is_real_file;

        let directory = TempDir::new().expect("crear temporal");
        let secret = directory.path().join("secreto.txt");
        fs::write(&secret, "token=fixture-secret").expect("escribir secreto");
        let link = directory.path().join("manifiesto.item");
        std::os::unix::fs::symlink(&secret, &link).expect("crear enlace");

        assert_eq!(read_text_file(&link, 8192), ReadOutcome::Unsafe);
        // Y tampoco aparece al listar el directorio de manifiestos.
        assert!(list_files_with_extension(directory.path(), "item").is_empty());
        assert!(!is_real_file(&link));
    }

    #[test]
    fn directory_listings_are_deterministic_and_filtered_by_extension() {
        let directory = TempDir::new().expect("crear temporal");
        for name in ["c.item", "a.item", "b.item", "otro.txt"] {
            fs::write(directory.path().join(name), "{}").expect("escribir fichero");
        }
        fs::create_dir(directory.path().join("subcarpeta")).expect("crear subcarpeta");

        let files = list_files_with_extension(directory.path(), "item");
        let names = files
            .iter()
            .map(|path| path.file_name().unwrap().to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["a.item", "b.item", "c.item"]);

        let subdirectories = list_subdirectories(directory.path());
        assert_eq!(subdirectories.len(), 1);
        assert!(is_real_directory(&subdirectories[0]));
    }

    #[test]
    fn the_gog_manifest_listing_ignores_its_neighbouring_files() {
        let directory = TempDir::new().expect("crear temporal");
        fs::write(directory.path().join("goggame-1207658924.info"), "{}").expect("escribir info");
        fs::write(directory.path().join("goggame-1207658924.hashdb"), "x")
            .expect("escribir hashdb");
        fs::write(directory.path().join("readme.txt"), "x").expect("escribir readme");

        // Junto al `.info` conviven el `.hashdb` y ficheros del propio juego:
        // sólo el manifiesto entra en el escaneo.
        let info = list_files_with_extension(directory.path(), "info");
        assert_eq!(info.len(), 1);
        assert!(
            info[0]
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("goggame-"))
        );
    }

    #[test]
    fn an_executable_that_escapes_the_installation_is_refused() {
        let directory = TempDir::new().expect("crear temporal");
        let install = directory.path().join("Juego");
        fs::create_dir_all(&install).expect("crear instalación");
        let binary = install.join("Juego.app");
        fs::write(&binary, "binario").expect("escribir ejecutable");
        // Un binario fuera de la instalación, como si el manifiesto quisiera
        // apuntar a algo del sistema.
        let outside = directory.path().join("fuera.sh");
        fs::write(&outside, "#!/bin/sh").expect("escribir binario externo");

        assert!(resolve_executable_within(&install, "Juego.app").is_some());
        assert!(resolve_executable_within(&install, "../fuera.sh").is_none());
        assert!(resolve_executable_within(&install, "./../fuera.sh").is_none());
        assert!(resolve_executable_within(&install, "subdir/../../fuera.sh").is_none());
        assert!(resolve_executable_within(&install, "").is_none());
        assert!(resolve_executable_within(&install, "no-existe.exe").is_none());
        // Una ruta absoluta a un binario fuera de la instalación tampoco pasa.
        assert!(
            resolve_executable_within(&install, outside.to_str().expect("ruta utf-8")).is_none()
        );
        // Pero una ruta absoluta contenida sí, porque es lo que declaran los
        // `goggame-*.info`.
        assert!(
            resolve_executable_within(&install, binary.to_str().expect("ruta utf-8")).is_some()
        );
    }

    #[test]
    fn install_directories_must_be_absolute_existing_directories() {
        let directory = TempDir::new().expect("crear temporal");
        let install = directory.path().join("Instalado");
        fs::create_dir_all(&install).expect("crear instalación");
        let file = directory.path().join("fichero.txt");
        fs::write(&file, "x").expect("escribir fichero");

        assert!(canonical_install_directory(install.to_str().unwrap()).is_some());
        assert!(canonical_install_directory("relativa/instalacion").is_none());
        assert!(canonical_install_directory("").is_none());
        assert!(canonical_install_directory(file.to_str().unwrap()).is_none());
        assert!(is_contained_in(directory.path(), &install));
        assert!(!is_contained_in(&install, directory.path()));
    }
}
