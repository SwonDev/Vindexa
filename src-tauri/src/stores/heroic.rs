//! Lectura de la caché de biblioteca que Heroic Games Launcher deja en disco.
//!
//! # Por qué existe este módulo
//!
//! Los manifiestos de instalación (`installed.json`, `goggame-*.info`, `*.item`)
//! sólo describen lo que está **instalado**. La biblioteca completa —todo lo que
//! la cuenta posee— no aparece por ninguna parte de ellos, y sin embargo Heroic
//! ya la tiene: cuando la persona usuaria inicia sesión en Epic o en GOG desde
//! Heroic, éste guarda el catálogo entero en `store_cache/legendary_library.json`
//! y `store_cache/gog_library.json`.
//!
//! Leer esos ficheros es lo que permite enseñar la biblioteca completa **sin
//! pedir credenciales, sin llamar a ninguna API privada y sin guardar ningún
//! token**: el trabajo de autenticarse ya lo hizo el cliente oficialmente
//! instalado por la persona usuaria, y Vindexa se limita a leer el resultado.
//!
//! # Lo que este módulo nunca toca
//!
//! Junto a esas cachés, Heroic guarda `gog_store/auth.json` y
//! `legendaryConfig/legendary/user.json`, que contienen los tokens de sesión.
//! **Vindexa no los lee jamás.** Como mucho comprueba si existen, para poder
//! decir «este cliente no ha iniciado sesión» en vez de fingir que no está
//! instalado.
//!
//! # Formato
//!
//! Heroic usa `electron-store` con `cwd: 'store_cache'`, así que el fichero es
//! un objeto con la clave `games` (la lista) y una clave hermana
//! `__timestamp.games` que aquí se ignora.

use crate::stores::{MAX_DISCOVERED_GAMES, sanitize_https_url, sanitize_path, sanitize_title};
use serde::Deserialize;

/// La caché de biblioteca de una cuenta con centenares de juegos ronda unos
/// pocos MiB. 64 MiB es una cota holgada que sigue impidiendo una lectura
/// desmesurada si el fichero está corrupto o manipulado.
pub(crate) const MAX_LIBRARY_CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// Nombre del fichero de caché de la biblioteca de Epic dentro de `store_cache`.
pub(crate) const LEGENDARY_LIBRARY_FILE: &str = "legendary_library.json";

/// Nombre del fichero de caché de la biblioteca de GOG dentro de `store_cache`.
pub(crate) const GOG_LIBRARY_FILE: &str = "gog_library.json";

/// Una entrada de la caché ya saneada, todavía sin identidad de tienda.
///
/// Los campos que Heroic no trae se quedan en `None`: aquí no se rellena nada
/// por deducción.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeroicLibraryEntry {
    /// `AppName` en Epic, `productId` en GOG. Sin validar todavía: quien lo
    /// consume aplica la allowlist de su tienda.
    pub app_name: String,
    pub title: String,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub install_path: Option<String>,
    pub executable: Option<String>,
    pub installed: bool,
}

/// Qué había en un fichero de caché de biblioteca.
///
/// Los tres casos se tratan distinto: uno es un fallo de lectura, otro es un
/// cliente que todavía no ha traído su biblioteca, y el tercero es la
/// biblioteca. Colapsarlos haría que «no has iniciado sesión» se contase como
/// «no se pudo leer», que es una acusación falsa.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LibraryCache {
    /// El fichero no es un objeto JSON: no es una caché de `electron-store`.
    Malformed,
    /// Es un objeto válido, pero todavía no guarda ninguna biblioteca. Es lo que
    /// deja un cliente instalado que no ha iniciado sesión o no ha sincronizado.
    Absent,
    /// La biblioteca, con las entradas utilizables y cuántas se descartaron.
    Games {
        entries: Vec<HeroicLibraryEntry>,
        skipped: u32,
    },
}

#[derive(Debug, Deserialize)]
struct LibraryCacheFile {
    games: Option<Vec<LibraryGame>>,
}

#[derive(Debug, Deserialize)]
struct LibraryGame {
    app_name: Option<String>,
    title: Option<String>,
    /// Portada vertical. Heroic la rellena desde la API de la tienda.
    art_cover: Option<String>,
    /// Arte cuadrado. Se usa como cabecera sólo si existe.
    art_square: Option<String>,
    #[serde(default)]
    is_installed: bool,
    /// Qué backend escribió la entrada: `legendary`, `gog`, `nile`, `sideload`…
    runner: Option<String>,
    #[serde(default)]
    install: LibraryInstall,
}

#[derive(Debug, Default, Deserialize)]
struct LibraryInstall {
    install_path: Option<String>,
    executable: Option<String>,
    // `install_size` viene como texto legible («12.3 GB»), no como bytes.
    // No se declara a propósito: adivinar el factor sería inventar un dato.
    #[serde(default)]
    is_dlc: bool,
}

/// Lee una caché de biblioteca de Heroic.
pub(crate) fn parse_library_cache(contents: &str, expected_runner: &str) -> LibraryCache {
    // `electron-store` siempre escribe un objeto. Un array o un escalar no son
    // una caché suya, y aceptarlos por la vía de serde (que sabe construir una
    // estructura a partir de una secuencia) haría pasar por «biblioteca vacía»
    // un fichero que no lo es.
    if !serde_json::from_str::<serde_json::Value>(contents).is_ok_and(|value| value.is_object()) {
        return LibraryCache::Malformed;
    }
    let Ok(file) = serde_json::from_str::<LibraryCacheFile>(contents) else {
        return LibraryCache::Malformed;
    };
    let Some(games) = file.games else {
        return LibraryCache::Absent;
    };
    let mut entries = Vec::new();
    let mut skipped = 0_u32;
    for game in games {
        if entries.len() >= MAX_DISCOVERED_GAMES {
            skipped = skipped.saturating_add(1);
            continue;
        }
        match parse_entry(game, expected_runner) {
            Some(entry) => entries.push(entry),
            None => skipped = skipped.saturating_add(1),
        }
    }
    LibraryCache::Games { entries, skipped }
}

fn parse_entry(game: LibraryGame, expected_runner: &str) -> Option<HeroicLibraryEntry> {
    // Heroic guarda una caché por tienda, pero el campo viaja en cada entrada:
    // si no coincide, la entrada no es de esta tienda y no se toca.
    if game
        .runner
        .as_deref()
        .is_some_and(|runner| runner != expected_runner)
    {
        return None;
    }
    if game.install.is_dlc {
        return None;
    }
    let app_name = game
        .app_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    // Sin título no se persiste: fabricar «Juego <id>» sería inventar un dato
    // que la caché no trae.
    let title = game.title.as_deref().and_then(sanitize_title)?;

    let install_path = game.install.install_path.as_deref().and_then(sanitize_path);
    // Heroic marca `is_installed`, pero la carpeta puede haberse borrado a mano
    // desde entonces. Quien consume esto revalida la ruta contra el disco.
    let installed = game.is_installed && install_path.is_some();

    Some(HeroicLibraryEntry {
        app_name: app_name.to_string(),
        title,
        cover_url: game.art_cover.as_deref().and_then(sanitize_https_url),
        header_url: game.art_square.as_deref().and_then(sanitize_https_url),
        install_path,
        executable: game
            .install
            .executable
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        installed,
    })
}

#[cfg(test)]
mod tests {
    use super::{HeroicLibraryEntry, LibraryCache, parse_library_cache};

    /// Desempaqueta el caso «hay biblioteca». Los demás casos tienen su propio
    /// test, así que aquí un desvío es un fallo del test, no del código.
    fn games(cache: LibraryCache) -> (Vec<HeroicLibraryEntry>, u32) {
        match cache {
            LibraryCache::Games { entries, skipped } => (entries, skipped),
            other => panic!("se esperaba una biblioteca y llegó {other:?}"),
        }
    }

    #[test]
    fn the_full_library_is_read_including_the_games_that_are_not_installed() {
        let contents = r#"{
            "games": [
                {"runner": "gog", "app_name": "1207658924", "title": "The Witcher",
                 "art_cover": "https://images.gog.com/witcher.jpg",
                 "art_square": "https://images.gog.com/witcher_square.jpg",
                 "is_installed": false, "install": {}},
                {"runner": "gog", "app_name": "1207666073", "title": "Beneath a Steel Sky",
                 "is_installed": true,
                 "install": {"install_path": "/Juegos/Steel Sky", "executable": "juego.exe",
                             "install_size": "1.2 GB", "is_dlc": false}}
            ],
            "__timestamp.games": "Mon Aug 18 2026 11:00:00 GMT+0100"
        }"#;
        let (entries, skipped) = games(parse_library_cache(contents, "gog"));
        assert_eq!(skipped, 0);
        assert_eq!(entries.len(), 2);
        // Un juego que no está instalado sigue siendo parte de la biblioteca.
        assert!(!entries[0].installed);
        assert_eq!(entries[0].title, "The Witcher");
        assert_eq!(
            entries[0].cover_url.as_deref(),
            Some("https://images.gog.com/witcher.jpg")
        );
        assert!(entries[1].installed);
        assert_eq!(
            entries[1].install_path.as_deref(),
            Some("/Juegos/Steel Sky")
        );
    }

    #[test]
    fn entries_of_another_runner_or_without_a_title_are_skipped_not_invented() {
        let contents = r#"{
            "games": [
                {"runner": "nile", "app_name": "amazon-1", "title": "Juego de Amazon"},
                {"runner": "gog", "app_name": "1207658924"},
                {"runner": "gog", "app_name": "   ", "title": "Sin identificador"},
                {"runner": "gog", "app_name": "1207666073", "title": "DLC suelto",
                 "install": {"is_dlc": true}},
                {"runner": "gog", "app_name": "1455545518", "title": "Válido"}
            ]
        }"#;
        let (entries, skipped) = games(parse_library_cache(contents, "gog"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].app_name, "1455545518");
        assert_eq!(skipped, 4);
    }

    #[test]
    fn artwork_that_is_not_absolute_https_never_becomes_a_cover() {
        let contents = r#"{
            "games": [
                {"runner": "legendary", "app_name": "Fortnite", "title": "Fortnite",
                 "art_cover": "http://cdn.epic.com/arte.jpg",
                 "art_square": "javascript:alert(1)"}
            ]
        }"#;
        let (entries, _) = games(parse_library_cache(contents, "legendary"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cover_url, None);
        assert_eq!(entries[0].header_url, None);
    }

    #[test]
    fn a_file_that_is_not_a_library_cache_is_refused_instead_of_read_as_empty() {
        assert_eq!(
            parse_library_cache("no soy json", "gog"),
            LibraryCache::Malformed
        );
        assert_eq!(parse_library_cache("[]", "gog"), LibraryCache::Malformed);
        // Un objeto sin `games` es lo que deja un cliente instalado que todavía
        // no ha traído su biblioteca. No es un fallo de lectura.
        assert_eq!(parse_library_cache("{}", "gog"), LibraryCache::Absent);
        assert_eq!(
            parse_library_cache(r#"{"__timestamp.games": "hoy"}"#, "gog"),
            LibraryCache::Absent
        );
        // Y `games: []` sí es una respuesta: «has iniciado sesión y no tienes
        // ningún juego».
        let (entries, skipped) = games(parse_library_cache(r#"{"games": []}"#, "gog"));
        assert!(entries.is_empty());
        assert_eq!(skipped, 0);
    }
}
