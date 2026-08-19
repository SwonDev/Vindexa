//! Caché local del arte oficial de Steam.
//!
//! Tres responsabilidades, en este orden:
//!
//! 1. **Calidad**: para cada hueco de la interfaz se pide la mejor variante que
//!    la CDN de Steam publica realmente (`library_600x900_2x.jpg` es 600×900;
//!    `library_600x900.jpg` es 300×450), con una escalera de reserva ordenada y
//!    verificada. El archivo se guarda **byte a byte tal cual llega**: Vindexa
//!    no reescala ni recomprime nunca.
//! 2. **Persistencia**: escritura atómica (temporal + `fsync` + `rename`),
//!    validación de integridad al leer (firma, dimensiones y cierre del
//!    formato), revalidación HTTP con `ETag`/`Last-Modified`, recolección de
//!    basura en ambas direcciones y desalojo LRU con presupuesto configurable.
//! 3. **Robustez de red**: tiempos límite, techo de bytes, presupuesto de
//!    descargas simultáneas, `Retry-After`, backoff exponencial acotado,
//!    deduplicación de vuelo y cancelación limpia.
//!
//! Toda ruta escrita o borrada queda dentro de `<cache>/steam-art/<app_id>/`;
//! cualquier candidata que se salga se rechaza antes de tocar el disco.

use crate::db::Database;
use crate::error::{AppError, AppResult};
use reqwest::{Client, Response, StatusCode, header, redirect::Policy};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use url::Url;
use uuid::Uuid;

// --- Límites de red y disco -------------------------------------------------

/// Techo por descarga. El activo oficial más pesado observado es
/// `library_hero_2x.jpg` (~1,2 MB); 10 MB deja margen sin permitir abusos.
const MAX_ART_BYTES: usize = 10 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Tiempo total de una petición. Cubre `library_hero_2x.jpg` en líneas lentas.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(25);
/// Corta descargas que se quedan colgadas sin cerrar la conexión.
const READ_TIMEOUT: Duration = Duration::from_secs(8);
/// Techo de toda la cadena de reserva: ninguna tarjeta espera más que esto.
const OVERALL_DEADLINE: Duration = Duration::from_secs(45);
const RETRY_BASE_DELAY: Duration = Duration::from_millis(200);
const MAX_RETRY_DELAY: Duration = Duration::from_secs(4);
/// `Retry-After` se respeta, pero nunca más allá de este techo: la interfaz no
/// puede quedarse esperando a que Steam levante un throttling largo.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(8);
const MAX_DOWNLOAD_ATTEMPTS: usize = 3;
const TRANSIENT_NEGATIVE_TTL: Duration = Duration::from_secs(30);
const DEFINITIVE_NEGATIVE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_CONCURRENT_DOWNLOADS: usize = 6;
const MAX_CANDIDATES: usize = 10;

/// Peldaño al que corresponde el fondo de la página de la tienda.
///
/// Steam sirve el mismo bitmap por dos rutas: como archivo de biblioteca
/// (`…/apps/{id}/page_bg_raw.jpg`) y por la ruta de la tienda
/// (`/images/storepagebackground/app/{id}`). Es la misma imagen y ocupa el
/// mismo puesto en la escalera del banner: el más bajo de los tres anchos,
/// porque Steam la publica ya oscurecida y difuminada para escribir encima.
const PAGE_BACKGROUND_FILE: &str = "page_bg_raw.jpg";

// --- Persistencia -----------------------------------------------------------

/// Steam sirve el arte con `cache-control: max-age=315360000`. Revalidar cada
/// treinta días es más que suficiente y mantiene el camino caliente sin red.
const REVALIDATE_AFTER: &str = "-30 days";
/// Presupuesto por defecto de la caché de arte en disco.
const DEFAULT_MAX_CACHE_BYTES: u64 = 512 * 1024 * 1024;
/// Bytes iniciales que se leen para validar firma y extraer dimensiones. El
/// marcador SOF de los JPEG oficiales aparece antes del byte 1.100 incluso con
/// EXIF (`page_bg_raw.jpg`); 8 KiB deja margen amplio.
const HEADER_SCAN_BYTES: usize = 8 * 1024;
/// Bytes finales que se leen para comprobar que el archivo está completo.
const TRAILER_SCAN_BYTES: i64 = 32;
/// Un temporal más antiguo que esto quedó de un corte y se puede purgar.
const TEMP_FILE_MAX_AGE: Duration = Duration::from_secs(60 * 60);
/// Cadencia del mantenimiento automático dentro de un mismo proceso.
const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(30 * 60);
/// Margen antes de contrastar la biblioteca con el índice de recursos de la
/// tienda. La primera pintada de la cuadrícula pide decenas de imágenes; darle
/// unos segundos de ventaja evita que las dos cosas compitan por la red.
const ART_INDEX_REFRESH_DELAY: Duration = Duration::from_secs(8);
/// Ventana durante la que un archivo servido se considera «en uso» y el
/// desalojo LRU no lo toca.
const RECENT_USE_WINDOW: Duration = Duration::from_secs(10 * 60);
const RECENT_USE_CAPACITY: usize = 16_384;
/// Vigencia de una validación memorizada. Pasado este tiempo se vuelve a leer
/// la firma, las dimensiones y el cierre del archivo desde el disco.
const VALIDATION_TTL: Duration = Duration::from_secs(60);
const VALIDATION_CAPACITY: usize = 8_192;

type ArtKey = (u32, ArtVariant, u64);

#[derive(Clone)]
struct NegativeEntry {
    until: Instant,
    error: AppError,
}

static CLIENT: OnceLock<Client> = OnceLock::new();
static IN_FLIGHT: OnceLock<Mutex<HashMap<ArtKey, Arc<AsyncMutex<()>>>>> = OnceLock::new();
static NEGATIVE_CACHE: OnceLock<Mutex<HashMap<ArtKey, NegativeEntry>>> = OnceLock::new();
static DOWNLOAD_SLOTS: OnceLock<Semaphore> = OnceLock::new();
static STEAM_LIBRARY_CACHE: OnceLock<Option<PathBuf>> = OnceLock::new();
static RECENT_USE: OnceLock<Mutex<HashMap<PathBuf, Instant>>> = OnceLock::new();
static LAST_MAINTENANCE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static MAINTENANCE_RUNNING: AtomicBool = AtomicBool::new(false);
static ART_INDEX_REFRESH_STARTED: AtomicBool = AtomicBool::new(false);
static MAX_CACHE_BYTES: AtomicU64 = AtomicU64::new(DEFAULT_MAX_CACHE_BYTES);
static VALIDATED: OnceLock<Mutex<HashMap<PathBuf, ValidationEntry>>> = OnceLock::new();

/// Resultado de una validación completa, reutilizable mientras el archivo no
/// cambie de tamaño ni de fecha. Una cuadrícula de mil tarjetas vuelve a pedir
/// las mismas rutas continuamente: revalidar el contenido en cada scroll sería
/// desperdiciar E/S sin ganar nada.
#[derive(Clone)]
struct ValidationEntry {
    bytes: u64,
    modified: Option<SystemTime>,
    checked: Instant,
    facts: ImageFacts,
}

/// Presupuesto máximo en disco para el arte cacheado. Al superarse, el
/// mantenimiento desaloja por orden de último uso.
#[allow(
    dead_code,
    reason = "punto de conexión con las preferencias de la aplicación"
)]
pub fn set_max_cache_bytes(limit: u64) {
    // Un presupuesto ridículo dejaría la biblioteca sin portadas.
    MAX_CACHE_BYTES.store(limit.max(1024 * 1024), Ordering::Relaxed);
}

fn max_cache_bytes() -> u64 {
    MAX_CACHE_BYTES.load(Ordering::Relaxed)
}

/// Cuánto ocupa la caché de arte y cuánto se le permite ocupar.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    pub bytes: u64,
    pub budget_bytes: u64,
}

/// Mide lo que ocupa la caché en disco.
///
/// Hace falta para que llenarla por adelantado sepa cuándo parar. Sin este dato
/// la precarga descarga sin fin: llega al techo, el mantenimiento desaloja lo
/// menos usado, y la siguiente tanda vuelve a bajar lo que se acaba de borrar.
/// Comprobado: con una biblioteca cuyo arte completo ocupa el doble del
/// presupuesto, la caché **bajaba** de tamaño mientras la precarga corría.
pub fn usage(cache_root: &Path) -> CacheUsage {
    let root = cache_root.join("steam-art");
    let mut bytes = 0_u64;
    if let Ok(directories) = fs::read_dir(&root) {
        for directory in directories.filter_map(Result::ok) {
            let Ok(files) = fs::read_dir(directory.path()) else {
                continue;
            };
            for file in files.filter_map(Result::ok) {
                if let Ok(metadata) = file.metadata()
                    && metadata.is_file()
                {
                    bytes = bytes.saturating_add(metadata.len());
                }
            }
        }
    }
    CacheUsage {
        bytes,
        budget_bytes: max_cache_bytes(),
    }
}

// --- Modelo de variantes ----------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtKind {
    Cover,
    Header,
    Icon,
    Hero,
    Logo,
}

/// Densidad de píxel pedida por la interfaz. La ventana de Vindexa vive casi
/// siempre en pantallas `devicePixelRatio = 2`, así que la densidad por defecto
/// de portada, cabecera, hero y logo es `X2`; el icono, que ocupa 38 px, se
/// queda en `X1` para no decodificar 600×900 en cada fila de una lista.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Density {
    X1,
    X2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArtVariant {
    pub kind: ArtKind,
    pub density: Density,
}

/// Un peldaño de la escalera de calidad: nombre real del archivo en la CDN y su
/// tamaño intrínseco medido contra respuestas reales de Steam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AssetRung {
    file: &'static str,
    width: u32,
    height: u32,
}

impl ArtKind {
    /// Proporción del hueco real en la interfaz, que pinta con
    /// `object-fit: cover` (ver `.artwork--*` en `src/index.css`). Comparar
    /// peldaños por píxeles brutos sería engañoso: una cápsula apaisada de
    /// 616×353 tiene más píxeles que una portada 300×450 pero, recortada a
    /// 2∶3, solo enseña 235×353. La comparación se hace sobre lo que se ve.
    #[cfg(test)]
    fn target_aspect(self) -> Option<f64> {
        match self {
            Self::Cover => Some(2.0 / 3.0),
            Self::Header => Some(460.0 / 215.0),
            // `.detail-hero` es una banda de 300–380 px de alto al ancho de la
            // hoja: en la práctica ronda 2,9∶1, muy cerca del 3,1∶1 nativo de
            // `library_hero`. No es 16∶9.
            Self::Hero => Some(2.9),
            Self::Icon => Some(1.0),
            // El logo se pinta completo, sin recorte.
            Self::Logo => None,
        }
    }

    /// Densidad por defecto de cada variante. Solo se sube a `X2` cuando el
    /// peldaño mayor es **la misma obra a más resolución**: portada y hero
    /// tienen gemelos `_2x` idénticos en encuadre. La cabecera se queda en
    /// `X1` porque su único peldaño superior (`capsule_616x353.jpg`) recorta
    /// distinto y esa decisión es de diseño, no de caché: la interfaz puede
    /// pedir `header@2x` cuando la quiera.
    fn default_density(self) -> Density {
        match self {
            Self::Icon | Self::Header => Density::X1,
            _ => Density::X2,
        }
    }
}

impl ArtVariant {
    /// Acepta `cover`, `header`, `icon`, `hero` y `logo`, con sufijo opcional
    /// `@1x`/`@2x`. Sin sufijo se usa la densidad por defecto de la variante,
    /// de modo que la interfaz actual obtiene la mejor calidad sin cambios.
    pub fn parse(value: &str) -> AppResult<Self> {
        let value = value.trim();
        let (name, density) = match value.split_once('@') {
            Some((name, "1x")) => (name, Some(Density::X1)),
            Some((name, "2x")) => (name, Some(Density::X2)),
            Some(_) => {
                return Err(AppError::validation("La variante de imagen no es válida."));
            }
            None => (value, None),
        };
        let kind = match name {
            "cover" => ArtKind::Cover,
            "header" => ArtKind::Header,
            "icon" => ArtKind::Icon,
            "hero" => ArtKind::Hero,
            "logo" => ArtKind::Logo,
            _ => return Err(AppError::validation("La variante de imagen no es válida.")),
        };
        Ok(Self {
            kind,
            density: density.unwrap_or_else(|| kind.default_density()),
        })
    }

    /// Identidad estable en SQLite y en el nombre de archivo. La densidad por
    /// defecto conserva la clave histórica (`cover`), así que las filas ya
    /// guardadas siguen siendo válidas.
    fn key(self) -> &'static str {
        match (self.kind, self.density) {
            (ArtKind::Cover, Density::X2) => "cover",
            (ArtKind::Cover, Density::X1) => "cover@1x",
            (ArtKind::Header, Density::X1) => "header",
            (ArtKind::Header, Density::X2) => "header@2x",
            (ArtKind::Icon, Density::X1) => "icon",
            (ArtKind::Icon, Density::X2) => "icon@2x",
            (ArtKind::Hero, Density::X2) => "hero",
            (ArtKind::Hero, Density::X1) => "hero@1x",
            (ArtKind::Logo, Density::X2) => "logo",
            (ArtKind::Logo, Density::X1) => "logo@1x",
        }
    }

    /// Escalera de calidad para esta variante, de mejor a peor. Los nombres y
    /// los tamaños están verificados contra respuestas reales de la CDN:
    ///
    /// | archivo | tamaño real |
    /// | --- | --- |
    /// | `library_600x900_2x.jpg` | 600×900 |
    /// | `library_600x900.jpg` | **300×450** |
    /// | `capsule_616x353.jpg` | 616×353 |
    /// | `header.jpg` | 460×215 |
    /// | `library_hero_2x.jpg` | 3840×1240 |
    /// | `library_hero.jpg` | 1920×620 |
    /// | `page_bg_raw.jpg` | 1438×810 |
    /// | `logo_2x.png` / `logo.png` | hasta 1280×720 / 640×360 |
    ///
    /// `header_2x.jpg` no existe en la CDN (404 en todas las muestras): la
    /// única forma de ganar resolución en la cabecera es `capsule_616x353.jpg`,
    /// que recorta distinto, así que solo se usa cuando se pide `header@2x`.
    fn ladder(self) -> &'static [AssetRung] {
        const COVER_2X: &[AssetRung] = &[
            AssetRung {
                file: "library_600x900_2x.jpg",
                width: 600,
                height: 900,
            },
            AssetRung {
                file: "library_600x900.jpg",
                width: 300,
                height: 450,
            },
            AssetRung {
                file: "capsule_616x353.jpg",
                width: 616,
                height: 353,
            },
            AssetRung {
                file: "header.jpg",
                width: 460,
                height: 215,
            },
        ];
        const COVER_1X: &[AssetRung] = &[
            AssetRung {
                file: "library_600x900.jpg",
                width: 300,
                height: 450,
            },
            AssetRung {
                file: "library_600x900_2x.jpg",
                width: 600,
                height: 900,
            },
            AssetRung {
                file: "capsule_616x353.jpg",
                width: 616,
                height: 353,
            },
            AssetRung {
                file: "header.jpg",
                width: 460,
                height: 215,
            },
        ];
        const HEADER_2X: &[AssetRung] = &[
            AssetRung {
                file: "capsule_616x353.jpg",
                width: 616,
                height: 353,
            },
            AssetRung {
                file: "header.jpg",
                width: 460,
                height: 215,
            },
        ];
        const HEADER_1X: &[AssetRung] = &[
            AssetRung {
                file: "header.jpg",
                width: 460,
                height: 215,
            },
            AssetRung {
                file: "capsule_616x353.jpg",
                width: 616,
                height: 353,
            },
        ];
        const HERO_2X: &[AssetRung] = &[
            AssetRung {
                file: "library_hero_2x.jpg",
                width: 3840,
                height: 1240,
            },
            AssetRung {
                file: "library_hero.jpg",
                width: 1920,
                height: 620,
            },
            AssetRung {
                file: "page_bg_raw.jpg",
                width: 1438,
                height: 810,
            },
            AssetRung {
                file: "capsule_616x353.jpg",
                width: 616,
                height: 353,
            },
            AssetRung {
                file: "header.jpg",
                width: 460,
                height: 215,
            },
        ];
        const HERO_1X: &[AssetRung] = &[
            AssetRung {
                file: "library_hero.jpg",
                width: 1920,
                height: 620,
            },
            AssetRung {
                file: "page_bg_raw.jpg",
                width: 1438,
                height: 810,
            },
            AssetRung {
                file: "header.jpg",
                width: 460,
                height: 215,
            },
        ];
        const LOGO_2X: &[AssetRung] = &[
            AssetRung {
                file: "logo_2x.png",
                width: 1280,
                height: 720,
            },
            AssetRung {
                file: "logo.png",
                width: 640,
                height: 360,
            },
        ];
        const LOGO_1X: &[AssetRung] = &[AssetRung {
            file: "logo.png",
            width: 640,
            height: 360,
        }];
        // El icono oficial vive en `media.steampowered.com` con un hash propio:
        // no hay hermanos derivables por convención dentro de `/apps/<id>/`.
        const ICON: &[AssetRung] = &[];

        match (self.kind, self.density) {
            (ArtKind::Cover, Density::X2) => COVER_2X,
            (ArtKind::Cover, Density::X1) => COVER_1X,
            (ArtKind::Header, Density::X2) => HEADER_2X,
            (ArtKind::Header, Density::X1) => HEADER_1X,
            (ArtKind::Hero, Density::X2) => HERO_2X,
            (ArtKind::Hero, Density::X1) => HERO_1X,
            (ArtKind::Logo, Density::X2) => LOGO_2X,
            (ArtKind::Logo, Density::X1) => LOGO_1X,
            (ArtKind::Icon, _) => ICON,
        }
    }

    /// Anchura mínima que hace útil un archivo ya presente en la caché del
    /// cliente de Steam. Sin esta puerta, adoptar un `library_600x900.jpg`
    /// local (300×450) impediría llegar a la variante 600×900 de la CDN.
    fn minimum_adoptable_width(self) -> u32 {
        match (self.kind, self.density) {
            (ArtKind::Cover, Density::X2) => 600,
            (ArtKind::Cover, Density::X1) => 300,
            (ArtKind::Header, Density::X2) => 600,
            (ArtKind::Header, Density::X1) => 460,
            (ArtKind::Hero, Density::X2) => 1920,
            (ArtKind::Hero, Density::X1) => 920,
            (ArtKind::Logo, _) => 320,
            (ArtKind::Icon, _) => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedArt {
    pub app_id: u32,
    pub variant: String,
    pub local_path: String,
    /// Tamaño real en píxeles leído del propio archivo. Permite a la interfaz
    /// reservar el hueco exacto y evitar saltos de maquetación.
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: u64,
}

/// Todo lo que se sabe de un archivo cacheado tras validarlo.
#[derive(Debug, Clone)]
struct ImageFacts {
    path: PathBuf,
    bytes: u64,
    width: Option<u32>,
    height: Option<u32>,
}

impl ImageFacts {
    fn into_cached(self, app_id: u32, variant: ArtVariant) -> CachedArt {
        CachedArt {
            app_id,
            variant: variant.key().to_owned(),
            local_path: self.path.display().to_string(),
            width: self.width,
            height: self.height,
            bytes: self.bytes,
        }
    }
}

// --- Orquestación -----------------------------------------------------------

pub async fn cache(
    database: &Database,
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    selected_source: Option<&str>,
) -> AppResult<CachedArt> {
    if app_id == 0 {
        return Err(AppError::validation(
            "El identificador del juego no es válido.",
        ));
    }
    let deadline = Instant::now() + OVERALL_DEADLINE;
    schedule_art_index_refresh(database);

    // Primero la caché local del propio cliente de Steam: es el arte exacto
    // que muestra la biblioteca oficial, sin red y sin esperas. Solo se adopta
    // si alcanza la resolución que pide la densidad, para no bloquear la
    // variante de mayor calidad de la CDN.
    if let Some(art) = adopt_local_art(database, cache_root, app_id, variant).await? {
        schedule_maintenance(database, cache_root);
        return Ok(art);
    }

    // Camino rápido: cuando la interfaz pasa una URL de biblioteca derivable,
    // la cabeza de la lista de candidatas se calcula sin abrir SQLite. Es el
    // caso de casi todas las tarjetas de la cuadrícula.
    for source in fast_path_candidates(app_id, variant, selected_source) {
        if validate_source_url(&source, app_id).is_err() {
            continue;
        }
        if let Some(facts) = existing_cache(cache_root, app_id, variant, &source) {
            let art = serve_cached(database, cache_root, app_id, variant, &source, facts).await;
            schedule_maintenance(database, cache_root);
            return Ok(art);
        }
    }

    let candidates = candidate_sources(database, app_id, variant, selected_source)?;
    if candidates.is_empty() {
        return Err(AppError::not_found(
            "Este juego no tiene una imagen oficial disponible para esta variante.",
        ));
    }

    let mut first_unavailable: Option<AppError> = None;
    for source in candidates {
        if Instant::now() >= deadline {
            break;
        }
        if validate_source_url(&source, app_id).is_err() {
            continue;
        }
        if let Some(facts) = existing_cache(cache_root, app_id, variant, &source) {
            let art = serve_cached(database, cache_root, app_id, variant, &source, facts).await;
            schedule_maintenance(database, cache_root);
            return Ok(art);
        }

        let key = (app_id, variant, source_fingerprint(&source));
        if negative_cache_hit(key).is_some() {
            continue;
        }

        // Deduplicación de vuelo: varias tarjetas pidiendo la misma imagen
        // comparten un único cerrojo, así que solo una descarga de verdad.
        let request_lock = request_lock(key);
        let _request_guard = request_lock.lock().await;

        // Otra tarjeta pudo completar la descarga mientras esperábamos.
        if let Some(facts) = existing_cache(cache_root, app_id, variant, &source) {
            let art = serve_cached(database, cache_root, app_id, variant, &source, facts).await;
            schedule_maintenance(database, cache_root);
            return Ok(art);
        }
        if negative_cache_hit(key).is_some() {
            continue;
        }

        match download_and_store(database, cache_root, app_id, variant, &source, None).await {
            Ok(art) => {
                clear_negative(key);
                schedule_maintenance(database, cache_root);
                return Ok(art);
            }
            Err(error) => {
                let definitive = is_definitive_art_error(&error);
                remember_negative(key, error.clone());
                if definitive {
                    // La URL no existe (o no es una imagen válida): se prueba la
                    // siguiente candidata de la cadena en vez de rendirse.
                    first_unavailable.get_or_insert(error);
                    continue;
                }
                return Err(error);
            }
        }
    }

    Err(first_unavailable.unwrap_or_else(|| {
        AppError::new(
            "art_unavailable",
            "Steam no dispone de esta imagen para el juego.",
        )
    }))
}

fn is_definitive_art_error(error: &AppError) -> bool {
    matches!(
        error.code.as_str(),
        "art_unavailable"
            | "art_too_large"
            | "art_content_type"
            | "art_empty"
            | "art_signature"
            | "art_not_modified"
    )
}

/// Devuelve el archivo ya cacheado y, si la fila relacional ha superado el TTL,
/// aprovecha para revalidarla contra Steam. Un fallo de red nunca degrada el
/// resultado: se sirve la copia local.
async fn serve_cached(
    database: &Database,
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
    facts: ImageFacts,
) -> CachedArt {
    // Solo la primera lectura de la sesión mira la fila relacional: abrir una
    // conexión SQLite por tarjeta arruinaría el desplazamiento de la cuadrícula.
    let already_served = used_recently(&facts.path);
    remember_use(&facts.path);
    if already_served {
        return facts.into_cached(app_id, variant);
    }
    let Some(conditional) = revalidation_due(database, app_id, variant, source) else {
        return facts.into_cached(app_id, variant);
    };
    match download_and_store(
        database,
        cache_root,
        app_id,
        variant,
        source,
        Some(conditional),
    )
    .await
    {
        Ok(refreshed) => refreshed,
        Err(error) if error.code == "art_not_modified" => {
            touch_cached_row(database, app_id, variant);
            facts.into_cached(app_id, variant)
        }
        // Steam no responde o devolvió basura: la copia local sigue siendo
        // válida y es preferible a un hueco vacío.
        Err(_) => facts.into_cached(app_id, variant),
    }
}

// --- Descarga ---------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Conditional {
    etag: Option<String>,
    last_modified: Option<String>,
}

impl Conditional {
    fn is_empty(&self) -> bool {
        self.etag.is_none() && self.last_modified.is_none()
    }
}

#[derive(Debug)]
struct FetchedAsset {
    bytes: Vec<u8>,
    extension: &'static str,
    mime: &'static str,
    etag: Option<String>,
    last_modified: Option<String>,
}

async fn download_and_store(
    database: &Database,
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
    conditional: Option<Conditional>,
) -> AppResult<CachedArt> {
    let _download_slot = download_slots().acquire().await.map_err(|_| {
        AppError::new(
            "art_download_queue",
            "No se pudo reservar una descarga de imagen.",
        )
    })?;
    let asset = fetch_asset(http_client()?, source, conditional.as_ref()).await?;
    store_asset(database, cache_root, app_id, variant, source, asset).await
}

/// Descarga verificada. Devuelve `art_not_modified` cuando Steam responde 304,
/// de modo que el llamante sabe que no hay nada que reescribir.
async fn fetch_asset(
    client: &Client,
    source: &str,
    conditional: Option<&Conditional>,
) -> AppResult<FetchedAsset> {
    let mut response = send_with_retry(client, source, conditional).await?;
    if response.status() == StatusCode::NOT_MODIFIED {
        return Err(AppError::new(
            "art_not_modified",
            "La imagen cacheada sigue vigente en Steam.",
        ));
    }
    if !response.status().is_success() {
        if retryable_status(response.status()) {
            return Err(download_error());
        }
        return Err(AppError::new(
            "art_unavailable",
            "Steam no dispone de esta imagen para el juego.",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ART_BYTES as u64)
    {
        return Err(AppError::new(
            "art_too_large",
            "La imagen de Steam supera el tamaño máximo permitido.",
        ));
    }
    let mime = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .unwrap_or_default()
        .to_owned();
    let (mime, extension) = allowed_content_type(&mime).ok_or_else(|| {
        AppError::new(
            "art_content_type",
            "Steam devolvió un contenido que no es una imagen compatible.",
        )
    })?;
    let etag = header_text(&response, header::ETAG);
    let last_modified = header_text(&response, header::LAST_MODIFIED);

    let mut bytes: Vec<u8> = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or(64 * 1024)
            .min(MAX_ART_BYTES as u64) as usize,
    );
    while let Some(chunk) = response.chunk().await.map_err(|_| download_error())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_ART_BYTES {
            return Err(AppError::new(
                "art_too_large",
                "La imagen de Steam supera el tamaño máximo permitido.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(AppError::new(
            "art_empty",
            "Steam devolvió una imagen vacía.",
        ));
    }
    // Firma, dimensiones y cierre del formato: una respuesta truncada o con el
    // tipo falseado no llega nunca al disco.
    if !matches_magic_bytes(mime, &bytes) || !has_valid_trailer(mime, &bytes) {
        return Err(AppError::new(
            "art_signature",
            "Steam devolvió una imagen cuya firma no coincide con su formato.",
        ));
    }

    Ok(FetchedAsset {
        bytes,
        extension,
        mime,
        etag,
        last_modified,
    })
}

fn header_text(response: &Response, name: header::HeaderName) -> Option<String> {
    let value = response.headers().get(name)?.to_str().ok()?.trim();
    // Cabeceras absurdamente largas no aportan nada y ensucian SQLite.
    if value.is_empty() || value.len() > 256 {
        return None;
    }
    Some(value.to_owned())
}

/// Escritura atómica: temporal en el mismo directorio, `fsync` del contenido,
/// `rename` sobre el destino y `fsync` del directorio. Un corte de corriente
/// deja el archivo anterior intacto o el nuevo completo, nunca uno a medias.
async fn store_asset(
    database: &Database,
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
    asset: FetchedAsset,
) -> AppResult<CachedArt> {
    let directory = art_directory(cache_root, app_id);
    let destination = directory.join(cache_file_name(variant, source, asset.extension));
    ensure_within_root(cache_root, &destination)?;
    let temporary = directory.join(format!(".{}.{}.part", variant.key(), Uuid::new_v4()));
    ensure_within_root(cache_root, &temporary)?;

    let dimensions = image_dimensions(asset.mime, &asset.bytes);
    let byte_size = asset.bytes.len() as u64;
    let bytes = asset.bytes;
    let destination_for_write = destination.clone();
    let write_task = tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
        let write_result = (|| {
            fs::create_dir_all(&directory)?;
            {
                let mut file = fs::File::create(&temporary)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
            }
            // `rename` sustituye el destino de forma atómica en POSIX y en
            // Windows (`MoveFileEx` con reemplazo), así que no se borra antes.
            fs::rename(&temporary, &destination_for_write)?;
            sync_directory(&directory);
            Ok(())
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    });
    join_art_cache_task(write_task).await?;

    let canonical = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.clone());
    let local_path = canonical.display().to_string();
    record_cached_path(
        database,
        app_id,
        variant,
        &local_path,
        asset.etag.as_deref(),
        asset.last_modified.as_deref(),
    )?;
    remember_use(&canonical);
    Ok(CachedArt {
        app_id,
        variant: variant.key().to_owned(),
        local_path,
        width: dimensions.map(|(width, _)| width),
        height: dimensions.map(|(_, height)| height),
        bytes: byte_size,
    })
}

#[cfg(unix)]
fn sync_directory(directory: &Path) {
    if let Ok(handle) = fs::File::open(directory) {
        let _ = handle.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) {}

async fn join_art_cache_task(
    task: tauri::async_runtime::JoinHandle<std::io::Result<()>>,
) -> AppResult<()> {
    task.await.map_err(|_error| {
        AppError::new(
            "art_cache_task",
            "No se pudo guardar la imagen en la caché local.",
        )
    })??;
    Ok(())
}

fn http_client() -> AppResult<&'static Client> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(Policy::none())
        .user_agent("Vindexa/0.1 (+https://vindexa.app)")
        .build()
        .map_err(|_| download_error())?;
    let _ = CLIENT.set(client);
    CLIENT.get().ok_or_else(download_error)
}

async fn send_with_retry(
    client: &Client,
    source: &str,
    conditional: Option<&Conditional>,
) -> AppResult<Response> {
    for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
        let mut request = client.get(source);
        if let Some(conditional) = conditional.filter(|value| !value.is_empty()) {
            if let Some(etag) = conditional.etag.as_deref() {
                request = request.header(header::IF_NONE_MATCH, etag);
            }
            if let Some(last_modified) = conditional.last_modified.as_deref() {
                request = request.header(header::IF_MODIFIED_SINCE, last_modified);
            }
        }
        let last_attempt = attempt + 1 >= MAX_DOWNLOAD_ATTEMPTS;
        match request.send().await {
            Ok(response) if retryable_status(response.status()) && !last_attempt => {
                let delay = retry_after(&response)
                    .unwrap_or_else(|| backoff_delay(attempt, source_fingerprint(source)));
                if delay > MAX_RETRY_AFTER {
                    // Steam pide esperar más de lo que la interfaz tolera: se
                    // devuelve un fallo transitorio y la caché negativa corta.
                    return Err(download_error());
                }
                tokio::time::sleep(delay).await;
            }
            Ok(response) => return Ok(response),
            Err(error) if retryable_request_error(&error) && !last_attempt => {
                tokio::time::sleep(backoff_delay(attempt, source_fingerprint(source))).await;
            }
            Err(_) => return Err(download_error()),
        }
    }
    Err(download_error())
}

/// Backoff exponencial con tope y una fluctuación determinista derivada de la
/// propia URL: evita que decenas de tarjetas reintenten en el mismo milisegundo
/// sin necesitar un generador aleatorio.
fn backoff_delay(attempt: usize, fingerprint: u64) -> Duration {
    let factor = 1_u32 << attempt.min(4);
    let base = RETRY_BASE_DELAY.saturating_mul(factor).min(MAX_RETRY_DELAY);
    let jitter = Duration::from_millis(fingerprint % 120);
    (base + jitter).min(MAX_RETRY_DELAY + Duration::from_millis(120))
}

/// `Retry-After` en segundos o como fecha HTTP. Se acota siempre.
fn retry_after(response: &Response) -> Option<Duration> {
    let raw = response
        .headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .to_owned();
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds.min(3_600)));
    }
    let target = chrono::DateTime::parse_from_rfc2822(&raw).ok()?;
    let delta = target.timestamp() - chrono::Utc::now().timestamp();
    Some(Duration::from_secs(delta.clamp(0, 3_600) as u64))
}

fn retryable_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn retryable_request_error(error: &reqwest::Error) -> bool {
    error.is_timeout() || error.is_connect() || error.is_request()
}

fn request_locks() -> &'static Mutex<HashMap<ArtKey, Arc<AsyncMutex<()>>>> {
    IN_FLIGHT.get_or_init(|| Mutex::new(HashMap::new()))
}

fn download_slots() -> &'static Semaphore {
    DOWNLOAD_SLOTS.get_or_init(|| Semaphore::new(MAX_CONCURRENT_DOWNLOADS))
}

fn request_lock(key: ArtKey) -> Arc<AsyncMutex<()>> {
    let mut locks = request_locks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Evita crecimiento permanente en sesiones con bibliotecas enormes.
    if locks.len() > 4_096 {
        locks.retain(|_, lock| Arc::strong_count(lock) > 1);
    }
    Arc::clone(locks.entry(key).or_default())
}

fn in_flight_paths(cache_root: &Path) -> Vec<PathBuf> {
    let locks = request_locks()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    locks
        .iter()
        .filter(|(_, lock)| Arc::strong_count(lock) > 1)
        .flat_map(|((app_id, variant, fingerprint), _)| {
            let directory = art_directory(cache_root, *app_id);
            let (variant, fingerprint) = (*variant, *fingerprint);
            ["jpg", "png", "webp"].into_iter().map(move |extension| {
                directory.join(format!("{}-{fingerprint:016x}.{extension}", variant.key()))
            })
        })
        .collect()
}

fn negative_cache() -> &'static Mutex<HashMap<ArtKey, NegativeEntry>> {
    NEGATIVE_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn negative_cache_hit(key: ArtKey) -> Option<AppError> {
    let mut cache = negative_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = cache.get(&key)?;
    if entry.until > Instant::now() {
        return Some(entry.error.clone());
    }
    cache.remove(&key);
    None
}

fn remember_negative(key: ArtKey, error: AppError) {
    let ttl = if matches!(
        error.code.as_str(),
        "art_download" | "steam_network" | "art_download_queue"
    ) {
        TRANSIENT_NEGATIVE_TTL
    } else {
        DEFINITIVE_NEGATIVE_TTL
    };
    let mut cache = negative_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.len() > 10_000 {
        let now = Instant::now();
        cache.retain(|_, entry| entry.until > now);
    }
    cache.insert(
        key,
        NegativeEntry {
            until: Instant::now() + ttl,
            error,
        },
    );
}

fn clear_negative(key: ArtKey) {
    negative_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&key);
}

// --- Uso reciente -----------------------------------------------------------

fn recent_use() -> &'static Mutex<HashMap<PathBuf, Instant>> {
    RECENT_USE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember_use(path: &Path) {
    let now = Instant::now();
    let mut map = recent_use()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if map.len() >= RECENT_USE_CAPACITY {
        map.retain(|_, seen| now.duration_since(*seen) < RECENT_USE_WINDOW);
    }
    map.insert(path.to_path_buf(), now);
}

fn used_recently(path: &Path) -> bool {
    let now = Instant::now();
    recent_use()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path)
        .is_some_and(|seen| now.duration_since(*seen) < RECENT_USE_WINDOW)
}

// --- Lectura y validación ---------------------------------------------------

fn art_directory(cache_root: &Path, app_id: u32) -> PathBuf {
    cache_root.join("steam-art").join(app_id.to_string())
}

fn existing_cache(
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
) -> Option<ImageFacts> {
    let directory = art_directory(cache_root, app_id);
    ["jpg", "png", "webp"]
        .into_iter()
        .map(|extension| directory.join(cache_file_name(variant, source, extension)))
        .find_map(|candidate| trusted_cached_path(cache_root, app_id, &candidate))
}

/// Rechaza cualquier ruta que se salga del directorio de caché de la
/// aplicación, tanto por componentes `..` como por enlaces simbólicos.
fn ensure_within_root(cache_root: &Path, candidate: &Path) -> AppResult<()> {
    let outside =
        || AppError::validation("La ruta de la imagen no pertenece a la caché de la aplicación.");
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(outside());
    }
    let root = cache_root.join("steam-art");
    let canonical_root = root.canonicalize().ok();
    // La ruta guardada en SQLite es canónica y el directorio de caché puede
    // llegar con enlaces por medio (`/var` → `/private/var` en macOS): las dos
    // formas del prefijo son válidas.
    let inside = candidate.starts_with(&root)
        || canonical_root
            .as_deref()
            .is_some_and(|canonical| candidate.starts_with(canonical));
    if !inside {
        return Err(outside());
    }
    // Si el destino ya existe, un enlace simbólico debe resolverse dentro.
    if let (Some(canonical_root), Ok(canonical_candidate)) =
        (canonical_root.as_deref(), candidate.canonicalize())
        && !canonical_candidate.starts_with(canonical_root)
    {
        return Err(outside());
    }
    Ok(())
}

fn trusted_cached_path(cache_root: &Path, app_id: u32, value: &Path) -> Option<ImageFacts> {
    if ensure_within_root(cache_root, value).is_err() {
        return None;
    }
    let expected_root = art_directory(cache_root, app_id).canonicalize().ok()?;
    let facts = valid_image_file(value)?;
    facts.path.starts_with(&expected_root).then_some(facts)
}

/// Un archivo solo es servible si existe, su tamaño es razonable, su extensión
/// concuerda con su firma binaria, se puede leer su tamaño en píxeles y el
/// formato está cerrado (EOI en JPEG, IEND en PNG, tamaño RIFF en WebP). Un
/// archivo truncado por un corte se detecta aquí y se vuelve a pedir.
fn valid_image_file(path: &Path) -> Option<ImageFacts> {
    let canonical_file = path.canonicalize().ok()?;
    let metadata = canonical_file.metadata().ok()?;
    if !metadata.is_file() {
        return None;
    }
    let length = metadata.len();
    if length == 0 || length > MAX_ART_BYTES as u64 {
        return None;
    }
    let modified = metadata.modified().ok();
    if let Some(facts) = validated_recently(&canonical_file, length, modified) {
        return Some(facts);
    }
    let mime = mime_for_extension(canonical_file.extension()?.to_str()?)?;

    let mut file = fs::File::open(&canonical_file).ok()?;
    let mut head = vec![0_u8; HEADER_SCAN_BYTES.min(length as usize)];
    let read = read_fully(&mut file, &mut head).ok()?;
    head.truncate(read);
    if !matches_magic_bytes(mime, &head) {
        return None;
    }

    let mut tail = [0_u8; TRAILER_SCAN_BYTES as usize];
    let tail_length = TRAILER_SCAN_BYTES.min(length as i64);
    file.seek(SeekFrom::End(-tail_length)).ok()?;
    let tail_read = read_fully(&mut file, &mut tail[..tail_length as usize]).ok()?;
    if !closes_correctly(mime, &head, &tail[..tail_read], length) {
        return None;
    }

    let dimensions = image_dimensions(mime, &head);
    let facts = ImageFacts {
        path: canonical_file,
        bytes: length,
        width: dimensions.map(|(width, _)| width),
        height: dimensions.map(|(_, height)| height),
    };
    remember_validation(&facts, modified);
    Some(facts)
}

fn validation_cache() -> &'static Mutex<HashMap<PathBuf, ValidationEntry>> {
    VALIDATED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn validated_recently(path: &Path, bytes: u64, modified: Option<SystemTime>) -> Option<ImageFacts> {
    let now = Instant::now();
    let cache = validation_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let entry = cache.get(path)?;
    // Un archivo truncado o reemplazado cambia tamaño o fecha: se revalida.
    (entry.bytes == bytes
        && entry.modified == modified
        && now.duration_since(entry.checked) < VALIDATION_TTL)
        .then(|| entry.facts.clone())
}

fn remember_validation(facts: &ImageFacts, modified: Option<SystemTime>) {
    let now = Instant::now();
    let mut cache = validation_cache()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.len() >= VALIDATION_CAPACITY {
        cache.retain(|_, entry| now.duration_since(entry.checked) < VALIDATION_TTL);
        if cache.len() >= VALIDATION_CAPACITY {
            cache.clear();
        }
    }
    cache.insert(
        facts.path.clone(),
        ValidationEntry {
            bytes: facts.bytes,
            modified,
            checked: now,
            facts: facts.clone(),
        },
    );
}

fn read_fully(file: &mut fs::File, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(filled)
}

// --- Arte local del cliente de Steam ---------------------------------------

fn steam_library_cache() -> Option<&'static Path> {
    STEAM_LIBRARY_CACHE
        .get_or_init(|| {
            steamlocate::locate()
                .ok()
                .map(|steam| steam.path().join("appcache").join("librarycache"))
        })
        .as_deref()
}

/// Un paso de búsqueda local: archivo plano en `<appid>/` o dentro de un
/// subdirectorio hash (`<appid>/<hash>/`), el layout nuevo del cliente.
enum LocalArtStep {
    Flat(&'static str),
    Hashed(&'static str),
}

fn local_art_plan(variant: ArtVariant) -> &'static [LocalArtStep] {
    use LocalArtStep::{Flat, Hashed};
    match variant.kind {
        ArtKind::Cover => &[
            Flat("library_600x900_2x.jpg"),
            Hashed("library_capsule_2x.jpg"),
            Flat("library_600x900.jpg"),
            Hashed("library_capsule.jpg"),
            Flat("header.jpg"),
        ],
        ArtKind::Header => &[
            Flat("capsule_616x353.jpg"),
            Flat("header.jpg"),
            Hashed("library_capsule.jpg"),
        ],
        ArtKind::Hero => &[
            Flat("library_hero_2x.jpg"),
            Flat("library_hero.jpg"),
            Hashed("library_hero.jpg"),
            Flat("page_bg_raw.jpg"),
            Flat("header.jpg"),
        ],
        ArtKind::Logo => &[
            Flat("logo_2x.png"),
            Flat("logo.png"),
            Hashed("logo.png"),
            Hashed("library_logo.png"),
        ],
        // El icono local es el mismo 32×32 de la CDN: no aporta nada.
        ArtKind::Icon => &[],
    }
}

/// El arte que el cliente de Steam ya descargó para su propia biblioteca:
/// idéntico al que ve el usuario en Steam, local e inmediato. Solo se acepta si
/// llega a la resolución mínima que exige la densidad pedida.
fn local_library_art(
    library_cache: &Path,
    app_id: u32,
    variant: ArtVariant,
) -> Option<(PathBuf, ImageFacts)> {
    let dir = library_cache.join(app_id.to_string());
    let minimum = variant.minimum_adoptable_width();
    let mut hashed_subdirs: Option<Vec<PathBuf>> = None;
    let acceptable = |path: PathBuf| -> Option<(PathBuf, ImageFacts)> {
        let facts = valid_image_file(&path)?;
        // Sin dimensiones legibles no se puede garantizar la calidad: mejor la
        // cadena de red, que sí conoce el tamaño de cada peldaño.
        let width = facts.width?;
        (width >= minimum).then_some((path, facts))
    };
    for step in local_art_plan(variant) {
        match step {
            LocalArtStep::Flat(name) => {
                if let Some(found) = acceptable(dir.join(name)) {
                    return Some(found);
                }
            }
            LocalArtStep::Hashed(name) => {
                let subdirs = hashed_subdirs.get_or_insert_with(|| {
                    let mut entries: Vec<PathBuf> = fs::read_dir(&dir)
                        .map(|read| {
                            read.filter_map(Result::ok)
                                .map(|entry| entry.path())
                                .filter(|path| path.is_dir())
                                .collect()
                        })
                        .unwrap_or_default();
                    entries.sort();
                    entries
                });
                for subdir in subdirs.iter() {
                    if let Some(found) = acceptable(subdir.join(name)) {
                        return Some(found);
                    }
                }
            }
        }
    }
    None
}

/// Adopta el arte local de Steam copiándolo a nuestra caché (el scope del
/// protocolo `asset:` solo cubre nuestro directorio). Devuelve `None` si no
/// hay arte local válido o la copia falla: la cadena de red sigue como reserva.
async fn adopt_local_art(
    database: &Database,
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
) -> AppResult<Option<CachedArt>> {
    let Some(library_cache) = steam_library_cache() else {
        return Ok(None);
    };
    let Some((source, source_facts)) = local_library_art(library_cache, app_id, variant) else {
        return Ok(None);
    };
    let extension = source
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("jpg")
        .to_owned();
    let identity = format!("local:{}:{}", source.display(), source_facts.bytes);
    let directory = art_directory(cache_root, app_id);
    let destination = directory.join(cache_file_name(variant, &identity, &extension));
    ensure_within_root(cache_root, &destination)?;
    if let Some(facts) = trusted_cached_path(cache_root, app_id, &destination) {
        remember_use(&facts.path);
        return Ok(Some(facts.into_cached(app_id, variant)));
    }

    let temporary = directory.join(format!(".{}.{}.part", variant.key(), Uuid::new_v4()));
    ensure_within_root(cache_root, &temporary)?;
    let copy_destination = destination.clone();
    let copy_task = tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
        let result = (|| {
            fs::create_dir_all(&directory)?;
            fs::copy(&source, &temporary)?;
            {
                let file = fs::File::open(&temporary)?;
                file.sync_all()?;
            }
            fs::rename(&temporary, &copy_destination)?;
            sync_directory(&directory);
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    });
    if join_art_cache_task(copy_task).await.is_err() {
        return Ok(None);
    }
    let Some(facts) = trusted_cached_path(cache_root, app_id, &destination) else {
        return Ok(None);
    };
    let local_path = facts.path.display().to_string();
    // El arte adoptado no procede de HTTP: no hay validadores que guardar.
    record_cached_path(database, app_id, variant, &local_path, None, None)?;
    remember_use(&facts.path);
    Ok(Some(facts.into_cached(app_id, variant)))
}

// --- Candidatas -------------------------------------------------------------

#[derive(Default)]
struct ArtColumns {
    cover: Option<String>,
    header: Option<String>,
    capsule: Option<String>,
    icon: Option<String>,
    hero: Option<String>,
}

fn load_art_columns(database: &Database, app_id: u32) -> AppResult<ArtColumns> {
    let connection = database.open()?;
    let mut art = connection
        .query_row(
            "SELECT cover_url, header_url, icon_url, hero_url, capsule_url FROM games WHERE app_id = ?1",
            [app_id],
            |row| {
                Ok(ArtColumns {
                    cover: row.get(0)?,
                    header: row.get(1)?,
                    icon: row.get(2)?,
                    hero: row.get(3)?,
                    capsule: row.get(4)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default();
    let family = connection
        .query_row(
            "SELECT cover_url, header_url, icon_url FROM family_catalog_games WHERE app_id = ?1",
            [app_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    if let Some((cover, header, icon)) = family {
        if art.cover.is_none() {
            art.cover = cover;
        }
        if art.header.is_none() {
            art.header = header;
        }
        if art.icon.is_none() {
            art.icon = icon;
        }
    }
    Ok(art)
}

/// Base canónica de la CDN de biblioteca. Steam publica los activos por
/// convención bajo `/apps/<id>/` incluso cuando la API de tienda devuelve rutas
/// con hash, así que sirve de base cuando no hay ninguna URL conocida.
fn convention_base(app_id: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/header.jpg")
}

/// Deriva un asset hermano dentro del mismo directorio `/apps/{app_id}/` de la
/// CDN oficial. Las portadas generadas por convención (`library_600x900.jpg`)
/// no existen para juegos retirados, herramientas o demos; los hermanos
/// (`capsule_616x353.jpg`, `header.jpg`…) suelen seguir disponibles.
///
/// El segmento hash que la API de tienda añade (`/apps/<id>/<sha1>/header.jpg`)
/// se descarta a propósito: dentro de un directorio hash solo existen las
/// derivadas de esa misma imagen fuente, mientras que la ruta sin hash sí
/// publica la familia completa.
fn derive_library_asset(source: &str, app_id: u32, file_name: &str) -> Option<String> {
    let url = Url::parse(source).ok()?;
    if url.scheme() != "https" || url.fragment().is_some() {
        return None;
    }
    let host = url.host_str()?;
    if !is_allowed_library_host(host) {
        return None;
    }
    let marker = format!("/apps/{app_id}/");
    let prefix = url.path().split_once(&marker)?.0;
    Some(format!("https://{host}{prefix}{marker}{file_name}"))
}

fn is_allowed_library_host(host: &str) -> bool {
    matches!(
        host,
        "shared.steamstatic.com"
            | "shared.cloudflare.steamstatic.com"
            | "cdn.cloudflare.steamstatic.com"
            | "shared.akamai.steamstatic.com"
            | "media.steampowered.com"
    )
}

/// Dominios desde los que las demás tiendas sirven su arte.
///
/// Medidos sobre una biblioteca real el 2026-08-19: GOG entrega sus carátulas
/// desde `images.gog.com`, Epic desde `cdn1.epicgames.com` e itch.io desde
/// `img.itch.zone`. Se admiten sus subdominios porque el reparto entre CDNs es
/// cosa de cada tienda y cambia sin avisar; lo que no cambia es el dominio.
const EXTERNAL_STORE_ART_DOMAINS: &[&str] = &[
    "gog.com",
    "epicgames.com",
    "unrealengine.com",
    "itch.zone",
    "itch.io",
];

/// ¿Sirve este anfitrión el arte de alguna de las tiendas soportadas?
///
/// Exige el punto separador, así que `epicgames.com.atacante.tld` no cuela.
fn is_external_store_art_host(host: &str) -> bool {
    EXTERNAL_STORE_ART_DOMAINS
        .iter()
        .any(|domain| match host.strip_suffix(domain) {
            None => false,
            Some("") => true,
            Some(prefix) => prefix.len() > 1 && prefix.ends_with('.'),
        })
}

/// Último segmento de la ruta de una URL de arte. Sirve igual para las rutas
/// planas (`/apps/570/header.jpg`) que para las que la API de tienda devuelve
/// con hash (`/apps/570/<sha1>/header.jpg`).
fn asset_file_name(source: &str) -> Option<String> {
    let url = Url::parse(source).ok()?;
    // El fondo de la página de la tienda no se pide por nombre de archivo sino
    // por ruta —`/images/storepagebackground/app/1337760`—, así que su «nombre»
    // sería el propio identificador del juego: un nombre que no está en ninguna
    // escalera. Aquí se le devuelve el que le corresponde, porque es el mismo
    // bitmap que `page_bg_raw.jpg`.
    if url.path().starts_with("/images/storepagebackground/app/") {
        return Some(PAGE_BACKGROUND_FILE.to_owned());
    }
    let tail = url.path().rsplit('/').next()?;
    (!tail.is_empty()).then(|| tail.to_owned())
}

/// Posición del archivo elegido por la interfaz dentro de la escalera de la
/// variante. Todo peldaño anterior es una mejora que merece intentarse antes.
///
/// Sin elección se devuelve el último puesto, para que la escalera completa se
/// pruebe por delante. Con una elección cuyo nombre **no** está en la escalera
/// se devuelve el primero: es el caso de las carátulas que publica el índice de
/// la tienda (`library_capsule_2x.jpg`, `portrait.png`,
/// `library_600x900_spanish_2x.jpg`, rutas con hash de contenido), y ahí el
/// nombre real vale más que cualquier conjetura por convención. Adelantar la
/// escalera derivada era precisamente lo que hacía caer la portada de esos
/// juegos hasta la cabecera apaisada, recortada a proporción de cartel.
///
/// La excepción es el fondo de la página de la tienda, que [`asset_file_name`]
/// traduce a su peldaño real. Sin esa traducción entraba aquí como nombre
/// desconocido, se quedaba en el primer puesto —«esto es lo mejor que hay»— y
/// **ningún** banner llegaba a intentar `library_hero`: la ficha de todos los
/// juegos se pintaba con el fondo que Steam publica ya oscurecido y difuminado
/// para poner texto encima, y la biblioteca entera se veía gris azulada.
fn selected_rank(variant: ArtVariant, selected: Option<&str>) -> usize {
    let ladder = variant.ladder();
    let Some(file) = selected.and_then(asset_file_name) else {
        return ladder.len();
    };
    ladder
        .iter()
        .position(|rung| rung.file == file)
        .unwrap_or(0)
}

/// Píxeles que de verdad quedan visibles al encajar un peldaño en el hueco de
/// su variante. Es el criterio con el que se decide si una URL elegida por la
/// interfaz se puede mejorar.
#[cfg(test)]
fn effective_pixels(variant: ArtVariant, rung: &AssetRung) -> u64 {
    let (width, height) = (f64::from(rung.width), f64::from(rung.height));
    let Some(aspect) = variant.kind.target_aspect() else {
        return u64::from(rung.width) * u64::from(rung.height);
    };
    let (visible_width, visible_height) = if width / height > aspect {
        (height * aspect, height)
    } else {
        (width, width / aspect)
    };
    (visible_width * visible_height) as u64
}

/// Peldaño conocido de la escalera para un nombre de archivo de la CDN.
#[cfg(test)]
fn rung_for(variant: ArtVariant, file_name: &str) -> Option<AssetRung> {
    variant
        .ladder()
        .iter()
        .copied()
        .find(|rung| rung.file == file_name)
}

/// Cabeza de la lista de candidatas que se puede calcular sin consultar la base
/// de datos: los peldaños mejores derivados de la propia URL elegida, seguidos
/// de esa URL. Es siempre un prefijo exacto de [`candidate_sources`], así que
/// resolver aquí devuelve lo mismo que el camino completo.
fn fast_path_candidates(
    app_id: u32,
    variant: ArtVariant,
    selected_source: Option<&str>,
) -> Vec<String> {
    let Some(selected) = selected_source
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    // Si la URL elegida no es derivable, la base de la escalera saldría de las
    // columnas de SQLite y el prefijo dejaría de ser exacto.
    if derive_library_asset(selected, app_id, "header.jpg").is_none() {
        return Vec::new();
    }
    let ladder = variant.ladder();
    let rank = selected_rank(variant, Some(selected)).min(ladder.len());
    let mut candidates: Vec<String> = ladder[..rank]
        .iter()
        .filter_map(|rung| derive_library_asset(selected, app_id, rung.file))
        .collect();
    if !candidates.iter().any(|existing| existing == selected) {
        candidates.push(selected.to_owned());
    }
    candidates
}

/// Lista ordenada y deduplicada de URLs candidatas para una variante:
///
/// 1. peldaños de la escalera con **más** resolución que la fuente elegida;
/// 2. la fuente elegida por la interfaz;
/// 3. la columna oficial de esa variante;
/// 4. el resto de peldaños derivados;
/// 5. las columnas de otras variantes, como última reserva.
///
/// El resultado nunca es peor que la URL que pedía la interfaz y, cuando Steam
/// publica una variante mayor, la usa.
fn candidate_sources(
    database: &Database,
    app_id: u32,
    variant: ArtVariant,
    selected_source: Option<&str>,
) -> AppResult<Vec<String>> {
    let art = load_art_columns(database, app_id)?;
    let mut candidates: Vec<String> = Vec::new();
    fn push(candidates: &mut Vec<String>, value: Option<&str>) {
        if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty())
            && !candidates.iter().any(|existing| existing == value)
        {
            candidates.push(value.to_owned());
        }
    }
    let selected = selected_source
        .map(str::trim)
        .filter(|value| !value.is_empty());

    // Un juego que no es de Steam no tiene escalera que subir: sus imágenes no
    // se derivan de un AppID por convención, las publica su tienda con la ruta
    // que quiera. Derivar aquí produciría URLs de Steam para un identificador
    // que Steam no conoce, y cada una costaría una petición y un 404.
    if crate::models::is_local_app_id(app_id) {
        let mut candidates: Vec<String> = Vec::new();
        push(&mut candidates, selected);
        push(&mut candidates, art.cover.as_deref());
        push(&mut candidates, art.header.as_deref());
        push(&mut candidates, art.icon.as_deref());
        candidates.truncate(MAX_CANDIDATES);
        return Ok(candidates);
    }

    let own_column = match variant.kind {
        ArtKind::Cover => art.cover.as_deref(),
        ArtKind::Header => art.header.as_deref(),
        ArtKind::Hero => art.hero.as_deref(),
        ArtKind::Icon => art.icon.as_deref(),
        ArtKind::Logo => None,
    };

    // Base para derivar hermanos. Si ninguna URL conocida sirve, se recurre a la
    // convención oficial: cubre juegos del catálogo familiar sin arte guardado.
    let fallback_base = convention_base(app_id);
    let library_base = [
        selected,
        own_column,
        art.cover.as_deref(),
        art.capsule.as_deref(),
        art.header.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|value| derive_library_asset(value, app_id, "header.jpg").is_some())
    .unwrap_or(fallback_base.as_str());

    // Puesto de la fuente elegida dentro de la escalera de calidad.
    let rank = selected_rank(variant, selected);

    let push_rung = |candidates: &mut Vec<String>, rung: &AssetRung| {
        if let Some(derived) = derive_library_asset(library_base, app_id, rung.file) {
            push(candidates, Some(&derived));
        }
    };

    // 1. Mejoras estrictas sobre lo que pedía la interfaz.
    for rung in &variant.ladder()[..rank.min(variant.ladder().len())] {
        push_rung(&mut candidates, rung);
    }
    // 2 y 3. La elección explícita y la columna propia.
    push(&mut candidates, selected);
    push(&mut candidates, own_column);
    // 4. El resto de la escalera, ya en orden descendente de calidad.
    for rung in variant.ladder() {
        push_rung(&mut candidates, rung);
    }
    // 5. Columnas de otras variantes: peor encuadre, pero mejor que un hueco.
    match variant.kind {
        ArtKind::Cover => {
            push(&mut candidates, art.capsule.as_deref());
            push(&mut candidates, art.header.as_deref());
            push(&mut candidates, art.icon.as_deref());
        }
        ArtKind::Header | ArtKind::Logo => {
            push(&mut candidates, art.capsule.as_deref());
            push(&mut candidates, art.cover.as_deref());
            push(&mut candidates, art.icon.as_deref());
        }
        ArtKind::Hero => {
            push(&mut candidates, art.capsule.as_deref());
            push(&mut candidates, art.header.as_deref());
            push(&mut candidates, art.cover.as_deref());
        }
        ArtKind::Icon => {
            push(&mut candidates, art.cover.as_deref());
            push(&mut candidates, art.header.as_deref());
        }
    }
    candidates.truncate(MAX_CANDIDATES);
    Ok(candidates)
}

fn source_fingerprint(source: &str) -> u64 {
    // FNV-1a estable: la identidad del archivo debe sobrevivir reinicios y
    // actualizaciones del runtime, a diferencia de un hasher aleatorizado.
    source
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
}

fn cache_file_name(variant: ArtVariant, source: &str, extension: &str) -> String {
    format!(
        "{}-{:016x}.{}",
        variant.key(),
        source_fingerprint(source),
        extension
    )
}

/// Extrae de un nombre de archivo cacheado la huella de la URL que lo originó.
fn fingerprint_from_file_name(file_name: &str) -> Option<u64> {
    let stem = file_name.rsplit_once('.').map(|(stem, _)| stem)?;
    let hex = stem.rsplit_once('-').map(|(_, hex)| hex)?;
    (hex.len() == 16)
        .then(|| u64::from_str_radix(hex, 16).ok())
        .flatten()
}

// --- Validación de origen ---------------------------------------------------

pub(crate) fn validate_source_url(value: &str, app_id: u32) -> AppResult<()> {
    let url = Url::parse(value)
        .map_err(|_| AppError::validation("La URL de imagen de Steam no es válida."))?;
    if url.scheme() != "https"
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(AppError::validation(
            "La imagen no procede de una conexión HTTPS oficial de Steam.",
        ));
    }
    let pairs = url.query_pairs().collect::<Vec<_>>();
    let valid_cache_buster = pairs.is_empty()
        || (pairs.len() == 1
            && pairs[0].0 == "t"
            && !pairs[0].1.is_empty()
            && pairs[0].1.bytes().all(|byte| byte.is_ascii_digit()));
    let valid_hero_source = url.host_str() == Some("store.akamai.steamstatic.com")
        && url.path() == format!("/images/storepagebackground/app/{app_id}")
        && valid_cache_buster;
    let host = url.host_str().unwrap_or_default();
    let valid_library_source = is_allowed_library_host(host)
        && url.path().contains(&format!("/apps/{app_id}/"))
        && valid_cache_buster;
    // El arte de Epic, GOG e itch.io no lleva el identificador en la ruta —cada
    // tienda numera a su manera— y su cadena de consulta no es un simple
    // rompecachés: GOG añade `?namespace=gamesdb`. Se comprueba lo que sí puede
    // comprobarse: que el juego sea de una de esas tiendas y que la imagen
    // venga de un dominio suyo. Sin esto, su arte no se podía guardar en local
    // y había que volver a descargarlo en cada arranque.
    let valid_external_store_source =
        crate::models::is_local_app_id(app_id) && is_external_store_art_host(host);
    let valid_source = valid_hero_source || valid_library_source || valid_external_store_source;
    if !valid_source || url.fragment().is_some() {
        return Err(AppError::validation(
            "La imagen no pertenece a un dominio y ruta oficiales permitidos de Steam.",
        ));
    }
    Ok(())
}

fn allowed_content_type(mime: &str) -> Option<(&'static str, &'static str)> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" => Some(("image/jpeg", "jpg")),
        "image/png" => Some(("image/png", "png")),
        "image/webp" => Some(("image/webp", "webp")),
        _ => None,
    }
}

fn mime_for_extension(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => Some("image/jpeg"),
        "png" => Some("image/png"),
        "webp" => Some("image/webp"),
        _ => None,
    }
}

fn matches_magic_bytes(mime: &str, bytes: &[u8]) -> bool {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/webp" => bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP",
        _ => false,
    }
}

/// Comprueba que el archivo está completo mirando su cierre. Es la diferencia
/// entre servir una imagen a medias y volver a pedirla.
fn has_valid_trailer(mime: &str, bytes: &[u8]) -> bool {
    let length = bytes.len() as u64;
    let start = bytes.len().saturating_sub(TRAILER_SCAN_BYTES as usize);
    closes_correctly(mime, bytes, &bytes[start..], length)
}

fn closes_correctly(mime: &str, head: &[u8], tail: &[u8], total_length: u64) -> bool {
    match mime {
        // Algunos JPEG llevan bytes de relleno tras el EOI: se busca en la cola.
        "image/jpeg" => tail.windows(2).any(|pair| pair == [0xff, 0xd9]),
        "image/png" => tail.windows(4).any(|chunk| chunk == b"IEND"),
        // El campo RIFF describe el tamaño del archivo menos su cabecera de 8
        // bytes: si no cuadra, la descarga se cortó.
        "image/webp" => head
            .get(4..8)
            .and_then(|raw| raw.try_into().ok())
            .is_some_and(|raw: [u8; 4]| {
                let declared = u64::from(u32::from_le_bytes(raw)).saturating_add(8);
                declared == total_length || declared + 1 == total_length
            }),
        _ => false,
    }
}

// --- Dimensiones ------------------------------------------------------------

/// Lector mínimo de cabeceras de imagen: JPEG (SOF), PNG (IHDR) y WebP
/// (VP8/VP8L/VP8X). Sin dependencias nuevas y acotado en pasos.
fn image_dimensions(mime: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match mime {
        "image/jpeg" => jpeg_dimensions(bytes),
        "image/png" => png_dimensions(bytes),
        "image/webp" => webp_dimensions(bytes),
        _ => None,
    }
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        *bytes.get(offset + 3)?,
    ]))
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
    ]))
}

fn le_u24(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset + 1)?,
        *bytes.get(offset + 2)?,
        0,
    ]))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = be_u32(bytes, 16)?;
    let height = be_u32(bytes, 20)?;
    (width > 0 && height > 0).then_some((width, height))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut offset = 2_usize;
    // Cota dura de segmentos: una cabecera manipulada no puede hacernos girar.
    for _ in 0..128 {
        while *bytes.get(offset)? == 0xff && *bytes.get(offset + 1)? == 0xff {
            offset += 1;
        }
        if *bytes.get(offset)? != 0xff {
            return None;
        }
        let marker = *bytes.get(offset + 1)?;
        match marker {
            // SOI, TEM y RSTn no llevan longitud.
            0xd8 | 0x01 => {
                offset += 2;
                continue;
            }
            0xd0..=0xd7 => {
                offset += 2;
                continue;
            }
            // EOI o inicio del escaneo: ya no habrá SOF.
            0xd9 | 0xda => return None,
            // SOF0..SOF15 salvo DHT (C4), JPGA (C8) y DAC (CC).
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                let height = u32::from(be_u16(bytes, offset + 5)?);
                let width = u32::from(be_u16(bytes, offset + 7)?);
                return (width > 0 && height > 0).then_some((width, height));
            }
            _ => {
                let length = usize::from(be_u16(bytes, offset + 2)?);
                if length < 2 {
                    return None;
                }
                offset = offset.checked_add(2 + length)?;
            }
        }
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 16 || !bytes.starts_with(b"RIFF") || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    match bytes.get(12..16)? {
        b"VP8X" => {
            let width = le_u24(bytes, 24)? + 1;
            let height = le_u24(bytes, 27)? + 1;
            Some((width, height))
        }
        b"VP8 " => {
            // Cabecera del fotograma: 3 bytes de etiqueta + código de sincronía.
            if bytes.get(23..26)? != [0x9d, 0x01, 0x2a] {
                return None;
            }
            let width = u32::from(le_u16(bytes, 26)? & 0x3fff);
            let height = u32::from(le_u16(bytes, 28)? & 0x3fff);
            (width > 0 && height > 0).then_some((width, height))
        }
        b"VP8L" => {
            if *bytes.get(20)? != 0x2f {
                return None;
            }
            let packed = u32::from_le_bytes([
                *bytes.get(21)?,
                *bytes.get(22)?,
                *bytes.get(23)?,
                *bytes.get(24)?,
            ]);
            let width = (packed & 0x3fff) + 1;
            let height = ((packed >> 14) & 0x3fff) + 1;
            Some((width, height))
        }
        _ => None,
    }
}

// --- Persistencia relacional ------------------------------------------------

fn record_cached_path(
    database: &Database,
    app_id: u32,
    variant: ArtVariant,
    local_path: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> AppResult<()> {
    database.open()?.execute(
        "INSERT INTO image_cache(app_id, variant, local_path, etag, last_modified)
         SELECT ?1, ?2, ?3, ?4, ?5
          WHERE EXISTS (SELECT 1 FROM games WHERE app_id = ?1)
         ON CONFLICT(app_id, variant) DO UPDATE SET
            local_path = excluded.local_path,
            etag = excluded.etag,
            last_modified = excluded.last_modified,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        rusqlite::params![app_id, variant.key(), local_path, etag, last_modified],
    )?;
    Ok(())
}

fn touch_cached_row(database: &Database, app_id: u32, variant: ArtVariant) {
    if let Ok(connection) = database.open() {
        let _ = connection.execute(
            "UPDATE image_cache
                SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE app_id = ?1 AND variant = ?2",
            rusqlite::params![app_id, variant.key()],
        );
    }
}

/// Devuelve los validadores HTTP solo si la fila superó el TTL **y** apunta al
/// mismo origen que se está sirviendo: reutilizar el `ETag` de otra URL
/// produciría un 304 falso.
fn revalidation_due(
    database: &Database,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
) -> Option<Conditional> {
    let connection = database.open().ok()?;
    let row = connection
        .query_row(
            "SELECT local_path, etag, last_modified
               FROM image_cache
              WHERE app_id = ?1
                AND variant = ?2
                AND updated_at <= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?3)",
            rusqlite::params![app_id, variant.key(), REVALIDATE_AFTER],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .ok()??;
    let (local_path, etag, last_modified) = row;
    let file_name = Path::new(&local_path)
        .file_name()
        .and_then(|name| name.to_str())?;
    if fingerprint_from_file_name(file_name)? != source_fingerprint(source) {
        return None;
    }
    let conditional = Conditional {
        etag,
        last_modified,
    };
    (!conditional.is_empty()).then_some(conditional)
}

pub fn clear(database: &Database, cache_root: &Path) -> AppResult<()> {
    let directory = cache_root.join("steam-art");
    if directory.exists() {
        fs::remove_dir_all(&directory)?;
    }
    database.open()?.execute("DELETE FROM image_cache", [])?;
    if let Ok(mut cache) = negative_cache().lock() {
        cache.clear();
    }
    if let Ok(mut used) = recent_use().lock() {
        used.clear();
    }
    if let Ok(mut validated) = validation_cache().lock() {
        validated.clear();
    }
    Ok(())
}

// --- Mantenimiento ----------------------------------------------------------

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceReport {
    pub removed_directories: usize,
    pub removed_files: usize,
    pub removed_rows: usize,
    pub evicted_files: usize,
    pub bytes_after: u64,
}

fn last_maintenance() -> &'static Mutex<Option<Instant>> {
    LAST_MAINTENANCE.get_or_init(|| Mutex::new(None))
}

/// Contrasta la biblioteca con el índice oficial de recursos de la tienda, una
/// sola vez por ejecución y sólo si ha pasado el intervalo que guarda
/// [`crate::steam::store_assets`].
///
/// Se lanza desde aquí porque es esta caché la que sufría el problema: las URL
/// derivadas por convención devuelven 404 para buena parte del catálogo
/// moderno, y al agotarse la escalera la portada acababa siendo la cabecera
/// apaisada recortada. El índice publica el nombre real de cada archivo, así
/// que corregir la columna es lo que hace que la escalera acierte a la primera.
///
/// Todo lo que falle se traga a propósito: el arte es accesorio y una biblioteca
/// sin red debe seguir pintándose con lo que ya tiene guardado.
fn schedule_art_index_refresh(database: &Database) {
    // En pruebas la resolución se invoca explícitamente para no salir a la red.
    if cfg!(test) {
        return;
    }
    if ART_INDEX_REFRESH_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let database = database.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(ART_INDEX_REFRESH_DELAY).await;
        if !matches!(crate::steam::store_assets::refresh_due(&database), Ok(true)) {
            return;
        }
        if let Err(error) = crate::steam::store_assets::refresh_library_art(&database).await {
            eprintln!(
                "Vindexa no pudo contrastar el arte con la tienda de Steam: {}",
                error.code
            );
        }
    });
}

/// Lanza el mantenimiento en segundo plano como mucho una vez cada media hora.
/// No toma el cerrojo global de mantenimiento de la aplicación: solo lee
/// `games`/`family_catalog_games` y borra archivos y filas de `image_cache`.
fn schedule_maintenance(database: &Database, cache_root: &Path) {
    // En pruebas el mantenimiento se invoca explícitamente para que los casos
    // sean deterministas.
    if cfg!(test) {
        return;
    }
    let now = Instant::now();
    {
        let mut last = last_maintenance()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.is_some_and(|previous| now.duration_since(previous) < MAINTENANCE_INTERVAL) {
            return;
        }
        *last = Some(now);
    }
    if MAINTENANCE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let database = database.clone();
    let cache_root = cache_root.to_path_buf();
    let in_flight = in_flight_paths(&cache_root);
    tauri::async_runtime::spawn_blocking(move || {
        let _ = maintain(&database, &cache_root, &in_flight);
        MAINTENANCE_RUNNING.store(false, Ordering::SeqCst);
    });
}

/// Recolección de basura en ambos sentidos más desalojo LRU:
///
/// - directorios cuyo `app_id` ya no existe en `games` ni en
///   `family_catalog_games` (juego borrado);
/// - temporales `.part` abandonados por un corte;
/// - archivos ilegibles, vacíos o con firma rota;
/// - filas de `image_cache` cuyo archivo ya no es servible;
/// - desalojo por último uso cuando se supera el presupuesto.
///
/// Nunca borra un archivo servido hace menos de diez minutos ni uno con una
/// descarga en vuelo.
pub fn maintain(
    database: &Database,
    cache_root: &Path,
    in_flight: &[PathBuf],
) -> AppResult<MaintenanceReport> {
    let mut report = MaintenanceReport::default();
    let root = cache_root.join("steam-art");
    if !root.exists() {
        return Ok(report);
    }
    let known = known_app_ids(database)?;
    // Una base sin juegos no dice «ninguna de estas imágenes vale»: dice «aún
    // no sé nada». Distinguirlo importa porque la biblioteca puede estar vacía
    // por un instante y con toda normalidad —el primer arranque, una base
    // recién puesta en cuarentena, una restauración a medias— y la regla de
    // abajo borra el directorio de cada AppID que no reconoce.
    //
    // Pasó de verdad: tras una cuarentena, la aplicación arrancó con la base
    // vacía, el barrido no reconoció ni un AppID y se llevó por delante todo el
    // arte guardado. Al repoblarse por red en vez de desde la caché local de
    // Steam, la biblioteca entera cambió de aspecto. Borrar cientos de megas de
    // trabajo por una lectura que aún no significa nada no compensa: si de
    // verdad sobran, el barrido siguiente los encontrará igual.
    if known.is_empty() {
        return Ok(report);
    }
    let now = SystemTime::now();
    let mut survivors: Vec<(PathBuf, SystemTime, u64)> = Vec::new();

    for entry in fs::read_dir(&root)?.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            // En la raíz solo hay directorios por AppID.
            if remove_file_within(cache_root, &path) {
                report.removed_files += 1;
            }
            continue;
        }
        let recognised = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.parse::<u32>().ok())
            .is_some_and(|app_id| known.contains(&app_id));
        if !recognised {
            if remove_directory_within(cache_root, &path) {
                report.removed_directories += 1;
            }
            continue;
        }
        let Ok(files) = fs::read_dir(&path) else {
            continue;
        };
        for file in files.filter_map(Result::ok) {
            let file_path = file.path();
            let file_name = file_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned();
            let metadata = match file.metadata() {
                Ok(metadata) if metadata.is_file() => metadata,
                _ => continue,
            };
            let age = metadata
                .modified()
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or_default();

            if file_name.ends_with(".part") {
                if age > TEMP_FILE_MAX_AGE && remove_file_within(cache_root, &file_path) {
                    report.removed_files += 1;
                }
                continue;
            }
            if valid_image_file(&file_path).is_none() {
                if remove_file_within(cache_root, &file_path) {
                    report.removed_files += 1;
                }
                continue;
            }
            let last_use = metadata
                .accessed()
                .ok()
                .into_iter()
                .chain(metadata.modified().ok())
                .max()
                .unwrap_or(now);
            survivors.push((file_path, last_use, metadata.len()));
        }
    }

    report.removed_rows = prune_orphan_rows(database, cache_root)?;

    // Desalojo LRU: se ordena por último uso ascendente y se elimina hasta
    // entrar en presupuesto, respetando lo que está en uso.
    let mut total: u64 = survivors.iter().map(|(_, _, size)| *size).sum();
    let budget = max_cache_bytes();
    if total > budget {
        survivors.sort_by_key(|(_, last_use, _)| *last_use);
        for (path, _, size) in &survivors {
            if total <= budget {
                break;
            }
            if used_recently(path) || in_flight.iter().any(|busy| busy == path) {
                continue;
            }
            if remove_file_within(cache_root, path) {
                total = total.saturating_sub(*size);
                report.evicted_files += 1;
            }
        }
        report.removed_rows += prune_orphan_rows(database, cache_root)?;
    }
    report.bytes_after = total;
    Ok(report)
}

fn known_app_ids(database: &Database) -> AppResult<std::collections::HashSet<u32>> {
    let connection = database.open()?;
    let mut statement = connection.prepare(
        "SELECT app_id FROM games
         UNION
         SELECT app_id FROM family_catalog_games",
    )?;
    let ids = statement
        .query_map([], |row| row.get::<_, u32>(0))?
        .filter_map(Result::ok)
        .collect();
    Ok(ids)
}

/// Borra las filas cuya imagen ya no se puede servir. La siguiente petición
/// vuelve a descargarla en vez de entregar una ruta rota a la interfaz.
fn prune_orphan_rows(database: &Database, cache_root: &Path) -> AppResult<usize> {
    let connection = database.open()?;
    let rows: Vec<(u32, String, String)> = {
        let mut statement =
            connection.prepare("SELECT app_id, variant, local_path FROM image_cache")?;
        let mapped = statement.query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        mapped.filter_map(Result::ok).collect()
    };
    let mut removed = 0;
    for (app_id, variant, local_path) in rows {
        if trusted_cached_path(cache_root, app_id, Path::new(&local_path)).is_some() {
            continue;
        }
        connection.execute(
            "DELETE FROM image_cache WHERE app_id = ?1 AND variant = ?2",
            rusqlite::params![app_id, variant],
        )?;
        removed += 1;
    }
    Ok(removed)
}

fn remove_file_within(cache_root: &Path, path: &Path) -> bool {
    ensure_within_root(cache_root, path).is_ok() && fs::remove_file(path).is_ok()
}

fn remove_directory_within(cache_root: &Path, path: &Path) -> bool {
    ensure_within_root(cache_root, path).is_ok() && fs::remove_dir_all(path).is_ok()
}

fn download_error() -> AppError {
    AppError::new(
        "art_download",
        "No se pudo descargar la imagen oficial desde Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ArtKind, ArtVariant, Conditional, DEFAULT_MAX_CACHE_BYTES, Density, FetchedAsset,
        MAX_CONCURRENT_DOWNLOADS, MaintenanceReport, NegativeEntry, PAGE_BACKGROUND_FILE,
        allowed_content_type, cache, cache_file_name, candidate_sources, clear_negative,
        derive_library_asset, download_slots, effective_pixels, ensure_within_root, existing_cache,
        fast_path_candidates, fetch_asset, fingerprint_from_file_name, has_valid_trailer,
        image_dimensions, join_art_cache_task, local_library_art, maintain, matches_magic_bytes,
        negative_cache, negative_cache_hit, record_cached_path, request_lock, retryable_status,
        revalidation_due, rung_for, selected_rank, set_max_cache_bytes, source_fingerprint,
        store_asset, trusted_cached_path, valid_image_file, validate_source_url,
    };
    use crate::db::Database;
    use crate::error::AppError;
    use reqwest::StatusCode;
    use rusqlite::params;
    use std::fs;
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tempfile::TempDir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::runtime::Runtime;

    const COVER: ArtVariant = ArtVariant {
        kind: ArtKind::Cover,
        density: Density::X2,
    };
    const COVER_1X: ArtVariant = ArtVariant {
        kind: ArtKind::Cover,
        density: Density::X1,
    };
    const HEADER: ArtVariant = ArtVariant {
        kind: ArtKind::Header,
        density: Density::X2,
    };
    const HEADER_1X: ArtVariant = ArtVariant {
        kind: ArtKind::Header,
        density: Density::X1,
    };
    const HERO: ArtVariant = ArtVariant {
        kind: ArtKind::Hero,
        density: Density::X2,
    };
    const ICON: ArtVariant = ArtVariant {
        kind: ArtKind::Icon,
        density: Density::X1,
    };

    // --- Utilidades de prueba ------------------------------------------------

    /// JPEG estructuralmente válido: SOI + APP0 + SOF0 con las dimensiones
    /// pedidas + EOI. Es lo mínimo que la caché acepta como servible.
    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        bytes.extend_from_slice(&[0xff, 0xe0, 0x00, 0x10]);
        bytes.extend_from_slice(b"JFIF\0");
        bytes.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
        bytes.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08]);
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&[0x03, 0x01, 0x11, 0x00, 0x02, 0x11, 0x01, 0x03, 0x11, 0x01]);
        bytes.extend_from_slice(&[0xff, 0xd9]);
        bytes
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13_u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[0x08, 0x06, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        bytes.extend_from_slice(&0_u32.to_be_bytes());
        bytes.extend_from_slice(b"IEND");
        bytes.extend_from_slice(&[0xae, 0x42, 0x60, 0x82]);
        bytes
    }

    fn webp_vp8x(width: u32, height: u32) -> Vec<u8> {
        let mut payload = vec![0x10, 0x00, 0x00, 0x00];
        payload.extend_from_slice(&(width - 1).to_le_bytes()[..3]);
        payload.extend_from_slice(&(height - 1).to_le_bytes()[..3]);
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&((12 + 8 + payload.len()) as u32 - 8).to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8X");
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn webp_vp8l(width: u32, height: u32) -> Vec<u8> {
        let packed = ((width - 1) & 0x3fff) | (((height - 1) & 0x3fff) << 14);
        let mut payload = vec![0x2f];
        payload.extend_from_slice(&packed.to_le_bytes());
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&((12 + 8 + payload.len()) as u32 - 8).to_le_bytes());
        bytes.extend_from_slice(b"WEBP");
        bytes.extend_from_slice(b"VP8L");
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }

    fn runtime() -> Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime de prueba")
    }

    fn temp_database(directory: &TempDir) -> Database {
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
        database
    }

    /// Servidor HTTP mínimo para ejercitar revalidación condicional y
    /// deduplicación sin depender de la red real ni de dependencias nuevas.
    struct TestServer {
        addr: SocketAddr,
        requests: Arc<AtomicUsize>,
        bodies_sent: Arc<AtomicUsize>,
    }

    fn spawn_server(runtime: &Runtime, body: Vec<u8>, etag: &'static str) -> TestServer {
        let listener = runtime
            .block_on(TcpListener::bind("127.0.0.1:0"))
            .expect("abrir puerto de prueba");
        let addr = listener.local_addr().expect("dirección local");
        let requests = Arc::new(AtomicUsize::new(0));
        let bodies_sent = Arc::new(AtomicUsize::new(0));
        let counters = (Arc::clone(&requests), Arc::clone(&bodies_sent));
        runtime.spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let body = body.clone();
                let (requests, bodies_sent) = (Arc::clone(&counters.0), Arc::clone(&counters.1));
                tokio::spawn(async move {
                    let mut buffer = vec![0_u8; 4096];
                    let mut filled = 0;
                    loop {
                        let Ok(read) = stream.read(&mut buffer[filled..]).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        filled += read;
                        if buffer[..filled].windows(4).any(|w| w == b"\r\n\r\n") {
                            break;
                        }
                        if filled == buffer.len() {
                            return;
                        }
                    }
                    requests.fetch_add(1, Ordering::SeqCst);
                    let request = String::from_utf8_lossy(&buffer[..filled]).to_ascii_lowercase();
                    let response = if request.contains("if-none-match") {
                        format!(
                            "HTTP/1.1 304 Not Modified\r\netag: {etag}\r\ncontent-length: 0\r\n\r\n"
                        )
                        .into_bytes()
                    } else {
                        bodies_sent.fetch_add(1, Ordering::SeqCst);
                        let mut head = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: image/jpeg\r\netag: {etag}\r\nlast-modified: Mon, 10 Feb 2025 17:57:16 GMT\r\ncontent-length: {}\r\n\r\n",
                            body.len()
                        )
                        .into_bytes();
                        head.extend_from_slice(&body);
                        head
                    };
                    let _ = stream.write_all(&response).await;
                    let _ = stream.flush().await;
                });
            }
        });
        TestServer {
            addr,
            requests,
            bodies_sent,
        }
    }

    fn plain_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("cliente de prueba")
    }

    // --- Variantes y contrato público ---------------------------------------

    #[test]
    fn variant_and_content_type_are_allowlisted() {
        assert_eq!(ArtVariant::parse("cover").unwrap(), COVER);
        assert_eq!(ArtVariant::parse("cover@1x").unwrap(), COVER_1X);
        assert_eq!(ArtVariant::parse("icon").unwrap(), ICON);
        // La cabecera se queda en 1x por defecto: subir a la cápsula cambia el
        // encuadre y esa es una decisión de la interfaz, no de la caché.
        assert_eq!(ArtVariant::parse("header").unwrap(), HEADER_1X);
        assert_eq!(ArtVariant::parse("header@2x").unwrap(), HEADER);
        assert!(ArtVariant::parse("../../secret").is_err());
        assert!(ArtVariant::parse("cover@3x").is_err());
        assert_eq!(
            allowed_content_type("image/jpeg"),
            Some(("image/jpeg", "jpg"))
        );
        assert_eq!(allowed_content_type("text/html"), None);
        assert!(matches_magic_bytes("image/jpeg", &[0xff, 0xd8, 0xff, 0xdb]));
        assert!(!matches_magic_bytes("image/jpeg", b"<html>"));
        // La clave de la densidad por defecto no cambia: las filas guardadas
        // por versiones anteriores siguen siendo válidas.
        assert_eq!(COVER.key(), "cover");
        assert_eq!(COVER_1X.key(), "cover@1x");
        assert_eq!(HEADER_1X.key(), "header");
        assert_eq!(HEADER.key(), "header@2x");
    }

    #[test]
    fn cache_worker_failures_do_not_expose_panic_details() {
        let error = runtime().block_on(async {
            let task = tauri::async_runtime::spawn_blocking(|| -> std::io::Result<()> {
                panic!("url=https://example.test/?key=fixture-secret /Users/example/cache")
            });
            join_art_cache_task(task)
                .await
                .expect_err("convertir panic del worker en error público")
        });

        assert_eq!(error.code, "art_cache_task");
        assert_eq!(
            error.message,
            "No se pudo guardar la imagen en la caché local."
        );
        assert!(!error.message.contains("fixture-secret"));
        assert!(!error.message.contains("/Users/example"));
    }

    // --- Lector de dimensiones ----------------------------------------------

    #[test]
    fn image_dimensions_are_read_from_jpeg_png_and_webp_headers() {
        assert_eq!(
            image_dimensions("image/jpeg", &jpeg(600, 900)),
            Some((600, 900))
        );
        assert_eq!(
            image_dimensions("image/jpeg", &jpeg(3840, 1240)),
            Some((3840, 1240))
        );
        assert_eq!(
            image_dimensions("image/png", &png(640, 360)),
            Some((640, 360))
        );
        assert_eq!(
            image_dimensions("image/webp", &webp_vp8x(1438, 810)),
            Some((1438, 810))
        );
        assert_eq!(
            image_dimensions("image/webp", &webp_vp8l(616, 353)),
            Some((616, 353))
        );

        // JPEG con segmentos previos (EXIF) antes del SOF.
        let mut with_exif = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x08];
        with_exif.extend_from_slice(b"Exif\0\0");
        with_exif.extend_from_slice(&jpeg(1920, 620)[2..]);
        assert_eq!(
            image_dimensions("image/jpeg", &with_exif),
            Some((1920, 620))
        );

        // Basura y cabeceras truncadas nunca hacen girar el lector.
        assert_eq!(image_dimensions("image/jpeg", b"<html>"), None);
        assert_eq!(image_dimensions("image/jpeg", &[0xff, 0xd8, 0xff]), None);
        assert_eq!(image_dimensions("image/png", &png(640, 360)[..12]), None);
        assert_eq!(image_dimensions("image/webp", b"RIFF"), None);
        let looping = [0xff, 0xd8, 0xff, 0xe0, 0x00, 0x02, 0xff, 0xe0, 0x00, 0x02];
        assert_eq!(image_dimensions("image/jpeg", &looping), None);
    }

    #[test]
    fn truncated_files_are_detected_by_their_trailer() {
        let complete = jpeg(600, 900);
        assert!(has_valid_trailer("image/jpeg", &complete));
        assert!(!has_valid_trailer(
            "image/jpeg",
            &complete[..complete.len() - 2]
        ));
        let complete_png = png(640, 360);
        assert!(has_valid_trailer("image/png", &complete_png));
        assert!(!has_valid_trailer("image/png", &complete_png[..20]));
    }

    // --- Escritura atómica ---------------------------------------------------

    #[test]
    fn downloads_are_written_atomically_and_byte_for_byte() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let cache_root = directory.path().join("cache");
        let app_id = 880_020;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880020/library_600x900_2x.jpg";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, 'Escritura atómica')",
                params![app_id],
            )
            .expect("insertar juego");
        let bytes = jpeg(600, 900);

        let stored = runtime()
            .block_on(store_asset(
                &database,
                &cache_root,
                app_id,
                COVER,
                source,
                FetchedAsset {
                    bytes: bytes.clone(),
                    extension: "jpg",
                    mime: "image/jpeg",
                    etag: Some("\"67aa3dfc-1ed4b\"".into()),
                    last_modified: Some("Mon, 10 Feb 2025 17:57:16 GMT".into()),
                },
            ))
            .expect("guardar la imagen");

        // El archivo es exactamente lo descargado: sin reescalado ni recompresión.
        assert_eq!(fs::read(&stored.local_path).expect("leer imagen"), bytes);
        assert_eq!(stored.width, Some(600));
        assert_eq!(stored.height, Some(900));
        assert_eq!(stored.bytes, bytes.len() as u64);

        // No queda ningún temporal a medias.
        let leftovers: Vec<_> = fs::read_dir(cache_root.join("steam-art").join(app_id.to_string()))
            .expect("listar caché")
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "quedaron temporales: {leftovers:?}");

        // El mantenimiento no puede borrar la fila que acaba de escribir el
        // propio flujo: la ruta guardada es canónica y el directorio de caché
        // puede tener enlaces por medio.
        let report = maintain(&database, &cache_root, &[]).expect("mantenimiento");
        assert_eq!(report.removed_rows, 0, "{report:?}");
        assert!(Path::new(&stored.local_path).exists());

        // Los validadores HTTP quedan persistidos para la revalidación futura.
        let (etag, last_modified): (Option<String>, Option<String>) = database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT etag, last_modified FROM image_cache WHERE app_id = ?1 AND variant = 'cover'",
                params![app_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer validadores");
        assert_eq!(etag.as_deref(), Some("\"67aa3dfc-1ed4b\""));
        assert_eq!(
            last_modified.as_deref(),
            Some("Mon, 10 Feb 2025 17:57:16 GMT")
        );
    }

    // --- Integridad ----------------------------------------------------------

    #[test]
    fn corrupt_absent_and_truncated_files_are_never_served() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let app_root = directory.path().join("steam-art/880003");
        fs::create_dir_all(&app_root).expect("crear caché de imagen");

        let valid = app_root.join("cover.jpg");
        fs::write(&valid, jpeg(600, 900)).expect("escribir JPEG completo");
        let truncated = app_root.join("truncated.jpg");
        let complete = jpeg(600, 900);
        fs::write(&truncated, &complete[..complete.len() - 4]).expect("escribir JPEG truncado");
        let corrupt = app_root.join("header.jpg");
        fs::write(&corrupt, b"<html>").expect("escribir archivo corrupto");
        let empty = app_root.join("empty.jpg");
        fs::write(&empty, b"").expect("escribir archivo vacío");

        let facts = trusted_cached_path(directory.path(), 880_003, &valid).expect("servir válido");
        assert_eq!(facts.width, Some(600));
        assert_eq!(facts.height, Some(900));
        assert!(trusted_cached_path(directory.path(), 880_003, &truncated).is_none());
        assert!(trusted_cached_path(directory.path(), 880_003, &corrupt).is_none());
        assert!(trusted_cached_path(directory.path(), 880_003, &empty).is_none());
        assert!(
            trusted_cached_path(directory.path(), 880_003, &app_root.join("ausente.jpg")).is_none()
        );
        // Otro AppID no puede leer el archivo de este.
        assert!(trusted_cached_path(directory.path(), 880_004, &valid).is_none());
    }

    #[test]
    fn una_base_vacia_no_se_lleva_por_delante_el_arte_guardado() {
        // Reproduce lo que ocurrió tras una cuarentena: la aplicación arranca
        // con la base vacía y el barrido no reconoce ni un AppID. Antes de esta
        // guarda, esa lectura —que sólo significa «aún no sé nada»— borraba
        // cientos de megas de arte y la biblioteca entera cambiaba de aspecto.
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let cache_root = directory.path().join("cache");
        let guardado = cache_root.join("steam-art").join("620");
        fs::create_dir_all(&guardado).expect("crear dir");
        let portada = guardado.join("cover-0000000000000009.jpg");
        fs::write(&portada, jpeg(600, 900)).expect("escribir portada");

        let report = maintain(&database, &cache_root, &[]).expect("mantener");

        assert!(
            portada.exists(),
            "el arte tiene que sobrevivir a una base que todavía no dice nada"
        );
        assert_eq!(report.removed_directories, 0);
        assert_eq!(report.removed_files, 0);
    }

    #[test]
    fn broken_rows_are_pruned_and_orphan_directories_removed() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let cache_root = directory.path().join("cache");
        let alive = 880_021_u32;
        let deleted = 880_022_u32;
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, 'Vivo')",
                params![alive],
            )
            .expect("insertar juego vivo");

        let alive_dir = cache_root.join("steam-art").join(alive.to_string());
        fs::create_dir_all(&alive_dir).expect("crear dir vivo");
        let good = alive_dir.join("cover-0000000000000001.jpg");
        fs::write(&good, jpeg(600, 900)).expect("escribir portada");
        let broken = alive_dir.join("header-0000000000000002.jpg");
        fs::write(&broken, b"<html>").expect("escribir archivo roto");
        let stale_part = alive_dir.join(".cover.abc.part");
        fs::write(&stale_part, b"parcial").expect("escribir temporal");
        let old = std::time::SystemTime::now() - Duration::from_secs(7_200);
        // Se abre con permiso de escritura a propósito: cambiar las marcas de
        // tiempo lo exige en Windows, donde `File::open` da acceso denegado.
        fs::OpenOptions::new()
            .write(true)
            .open(&stale_part)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(old)))
            .expect("envejecer temporal");

        let orphan_dir = cache_root.join("steam-art").join(deleted.to_string());
        fs::create_dir_all(&orphan_dir).expect("crear dir huérfano");
        fs::write(
            orphan_dir.join("cover-0000000000000003.jpg"),
            jpeg(600, 900),
        )
        .expect("escribir huérfano");
        let junk_dir = cache_root.join("steam-art").join("no-es-un-appid");
        fs::create_dir_all(&junk_dir).expect("crear dir basura");

        let connection = database.open().expect("abrir base");
        connection
            .execute(
                "INSERT INTO image_cache(app_id, variant, local_path) VALUES (?1, 'cover', ?2)",
                params![alive, good.display().to_string()],
            )
            .expect("registrar portada");
        connection
            .execute(
                "INSERT INTO image_cache(app_id, variant, local_path) VALUES (?1, 'header', ?2)",
                params![alive, broken.display().to_string()],
            )
            .expect("registrar rota");
        drop(connection);

        let report = maintain(&database, &cache_root, &[]).expect("mantenimiento");
        assert!(report.removed_directories >= 2, "{report:?}");
        assert!(report.removed_files >= 2, "{report:?}");
        assert_eq!(report.removed_rows, 1);
        assert!(good.exists(), "la portada válida no se toca");
        assert!(!broken.exists());
        assert!(!stale_part.exists());
        assert!(!orphan_dir.exists());
        assert!(!junk_dir.exists());

        let remaining: i64 = database
            .open()
            .expect("abrir base")
            .query_row("SELECT COUNT(*) FROM image_cache", [], |row| row.get(0))
            .expect("contar filas");
        assert_eq!(remaining, 1);
    }

    // --- LRU -----------------------------------------------------------------

    #[test]
    fn lru_eviction_frees_space_without_touching_files_in_use() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let cache_root = directory.path().join("cache");
        let app_id = 880_023_u32;
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, 'Desalojo')",
                params![app_id],
            )
            .expect("insertar juego");
        let art_dir = cache_root.join("steam-art").join(app_id.to_string());
        fs::create_dir_all(&art_dir).expect("crear caché");

        // Tres archivos de ~2 MiB con antigüedades escalonadas: 0 es el más
        // viejo y 2 el más reciente.
        let mut paths = Vec::new();
        for (index, age_secs) in [(0_u32, 9_000_u64), (1, 6_000), (2, 3_000)] {
            let mut bytes = jpeg(600, 900);
            bytes.resize(2 * 1024 * 1024, 0x00);
            bytes.extend_from_slice(&[0xff, 0xd9]);
            let path = art_dir.join(format!("cover-000000000000000{index}.jpg"));
            fs::write(&path, &bytes).expect("escribir imagen grande");
            let when = std::time::SystemTime::now() - Duration::from_secs(age_secs);
            fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .and_then(|file| {
                    file.set_times(fs::FileTimes::new().set_modified(when).set_accessed(when))
                })
                .expect("envejecer archivo");
            paths.push(path);
        }

        // Con presupuesto de sobra no se desaloja nada.
        set_max_cache_bytes(DEFAULT_MAX_CACHE_BYTES);
        let quiet = maintain(&database, &cache_root, &[]).expect("mantenimiento sin presión");
        assert_eq!(quiet.evicted_files, 0);
        assert!(paths.iter().all(|path| path.exists()));

        // Con 5 MiB de presupuesto (frente a ~6 MiB ocupados) basta con liberar
        // un archivo: el más antiguo está en vuelo, así que le toca al segundo.
        set_max_cache_bytes(5 * 1024 * 1024);
        let report = maintain(&database, &cache_root, &[paths[0].clone()]).expect("desalojo");
        assert_eq!(report.evicted_files, 1, "{report:?}");
        assert!(
            paths[0].exists(),
            "no se desaloja un archivo con descarga en vuelo"
        );
        assert!(!paths[1].exists(), "el más antiguo desalojable cae primero");
        assert!(paths[2].exists(), "el más reciente sobrevive");
        assert!(report.bytes_after <= 5 * 1024 * 1024, "{report:?}");
        set_max_cache_bytes(DEFAULT_MAX_CACHE_BYTES);
    }

    // --- Path traversal ------------------------------------------------------

    #[test]
    fn paths_outside_the_cache_root_are_rejected() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let root = directory.path();
        let art_root = root.join("steam-art");
        fs::create_dir_all(art_root.join("570")).expect("crear caché");

        assert!(ensure_within_root(root, &art_root.join("570/cover.jpg")).is_ok());
        // Componentes `..` explícitos.
        assert!(ensure_within_root(root, &art_root.join("570/../../../etc/passwd")).is_err());
        assert!(ensure_within_root(root, Path::new("/etc/passwd")).is_err());
        assert!(ensure_within_root(root, &root.join("otra-cosa/imagen.jpg")).is_err());
        // Nombre de variante manipulado que intenta salir del directorio.
        assert!(ArtVariant::parse("../../../etc/passwd").is_err());

        // Enlace simbólico que apunta fuera de la caché.
        let secret = root.join("secreto.jpg");
        fs::write(&secret, jpeg(10, 10)).expect("escribir secreto");
        #[cfg(unix)]
        {
            let link = art_root.join("570").join("cover-000000000000dead.jpg");
            std::os::unix::fs::symlink(&secret, &link).expect("crear enlace");
            assert!(
                ensure_within_root(root, &link).is_err(),
                "un enlace que escapa de la caché debe rechazarse"
            );
            assert!(trusted_cached_path(root, 570, &link).is_none());
        }
        // El archivo apuntado sigue intacto: nunca se borra nada fuera.
        assert!(secret.exists());
    }

    // --- Escalera de calidad -------------------------------------------------

    #[test]
    fn cover_candidates_upgrade_to_the_real_600x900_asset_first() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        // `library_600x900.jpg` es en realidad 300×450: la interfaz la pide y la
        // caché debe intentar antes la variante `_2x`, que sí es 600×900.
        let selected = "https://shared.steamstatic.com/store_item_assets/steam/apps/880009/library_600x900.jpg";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, cover_url) VALUES (880009, 'Retirado', ?1)",
                [selected],
            )
            .expect("insertar juego");

        let candidates = candidate_sources(&database, 880_009, COVER, Some(selected))
            .expect("listar candidatas de portada");
        assert!(
            candidates[0].ends_with("/apps/880009/library_600x900_2x.jpg"),
            "la primera candidata debe ser la de 600×900: {candidates:?}"
        );
        let position = |needle: &str| {
            candidates
                .iter()
                .position(|url| url.ends_with(needle))
                .unwrap_or_else(|| panic!("falta {needle} en {candidates:?}"))
        };
        assert!(position("library_600x900_2x.jpg") < position("library_600x900.jpg"));
        assert!(position("library_600x900.jpg") < position("capsule_616x353.jpg"));
        assert!(position("capsule_616x353.jpg") < position("header.jpg"));

        // El puesto de la elección dentro de la escalera es lo que decide qué
        // se intenta antes, y funciona igual con rutas con hash.
        assert_eq!(selected_rank(COVER, Some(selected)), 1);
        assert_eq!(
            selected_rank(
                HEADER,
                Some(
                    "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/abc123/header.jpg?t=1"
                )
            ),
            1
        );
        assert_eq!(selected_rank(COVER, None), COVER.ladder().len());

        // A 1x se respeta la variante ligera como primera opción.
        let light = candidate_sources(&database, 880_009, COVER_1X, Some(selected))
            .expect("listar candidatas 1x");
        assert!(light[0].ends_with("library_600x900.jpg"), "{light:?}");
    }

    #[test]
    fn a_cover_published_by_the_store_index_is_tried_before_any_derived_guess() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        // Carátula real de un juego moderno: otro nombre de archivo y una ruta
        // con hash de contenido. Ninguna de las derivadas por convención
        // existe —404 medido contra la CDN el 18-08-2026—, así que probarlas
        // primero sólo servía para agotar la escalera y acabar recortando la
        // cabecera apaisada a proporción de cartel.
        let indexada = "https://shared.steamstatic.com/store_item_assets/steam/apps/880012/c45a0dcc4361206e34be411af44de7cf0cd2cd5b/library_capsule_2x.jpg";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, cover_url) VALUES (880012, 'Moderno', ?1)",
                [indexada],
            )
            .expect("insertar juego");

        let candidates =
            candidate_sources(&database, 880_012, COVER, Some(indexada)).expect("candidatas");
        assert_eq!(candidates[0], indexada, "{candidates:?}");
        // La convención sobrevive detrás, como red de seguridad.
        assert!(
            candidates
                .iter()
                .any(|url| url.ends_with("/apps/880012/library_600x900_2x.jpg")),
            "{candidates:?}"
        );
        assert_eq!(
            fast_path_candidates(880_012, COVER, Some(indexada)),
            candidates[..1]
        );

        // Si la interfaz todavía arrastra la URL antigua, la columna ya
        // corregida entra justo detrás: una 404 y a la primera buena.
        let obsoleta = "https://shared.steamstatic.com/store_item_assets/steam/apps/880012/library_600x900_2x.jpg";
        let mixtas =
            candidate_sources(&database, 880_012, COVER, Some(obsoleta)).expect("candidatas");
        assert_eq!(mixtas[0], obsoleta, "{mixtas:?}");
        assert_eq!(mixtas[1], indexada, "{mixtas:?}");
    }

    #[test]
    fn the_fast_path_is_always_a_prefix_of_the_full_candidate_list() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let app_id = 880_060_u32;
        let selected = format!(
            "https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/library_600x900.jpg"
        );
        let hashed = format!(
            "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/{app_id}/abc123def/header.jpg?t=1"
        );
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, cover_url, header_url) VALUES (?1, 'Prefijo', ?2, ?3)",
                params![app_id, selected, hashed],
            )
            .expect("insertar juego");

        for (variant, chosen) in [
            (COVER, Some(selected.as_str())),
            (COVER_1X, Some(selected.as_str())),
            (HEADER, Some(hashed.as_str())),
            (HEADER_1X, Some(hashed.as_str())),
            (HERO, Some(selected.as_str())),
            (ICON, Some(selected.as_str())),
        ] {
            let fast = fast_path_candidates(app_id, variant, chosen);
            let full = candidate_sources(&database, app_id, variant, chosen).expect("candidatas");
            assert_eq!(
                fast,
                full[..fast.len()],
                "el camino rápido de {} debe ser prefijo exacto de la lista completa",
                variant.key()
            );
            assert!(!fast.is_empty(), "{} sin camino rápido", variant.key());
        }

        // Una URL no derivable (el fondo de tienda del hero) desactiva el atajo.
        let store_background = format!(
            "https://store.akamai.steamstatic.com/images/storepagebackground/app/{app_id}?t=1"
        );
        assert!(fast_path_candidates(app_id, HERO, Some(&store_background)).is_empty());
        assert!(fast_path_candidates(app_id, COVER, None).is_empty());
    }

    #[test]
    fn ladders_are_ordered_by_the_pixels_that_quedan_visibles() {
        let pixels = |variant: ArtVariant, file: &str| {
            effective_pixels(
                variant,
                &rung_for(variant, file).unwrap_or_else(|| panic!("falta {file}")),
            )
        };
        // Portada 2∶3: la cápsula apaisada tiene más píxeles brutos que la
        // portada 300×450, pero recortada enseña menos.
        assert!(pixels(COVER, "library_600x900_2x.jpg") > pixels(COVER, "library_600x900.jpg"));
        assert!(pixels(COVER, "library_600x900.jpg") > pixels(COVER, "capsule_616x353.jpg"));
        assert!(pixels(COVER, "capsule_616x353.jpg") > pixels(COVER, "header.jpg"));
        // Cabecera 460∶215: la cápsula sí gana porque el recorte es leve.
        assert!(pixels(HEADER, "capsule_616x353.jpg") > pixels(HEADER, "header.jpg"));
        // Banner ~2,9∶1: la escalera del hero es monótona decreciente. El fondo
        // de tienda (`page_bg_raw.jpg`, 1438×810) queda por debajo del hero
        // oficial porque en una banda tan apaisada se recorta casi el 40 %.
        assert!(pixels(HERO, "library_hero_2x.jpg") > pixels(HERO, "library_hero.jpg"));
        assert!(pixels(HERO, "library_hero.jpg") > pixels(HERO, "page_bg_raw.jpg"));
        assert!(pixels(HERO, "page_bg_raw.jpg") > pixels(HERO, "capsule_616x353.jpg"));
        assert!(pixels(HERO, "capsule_616x353.jpg") > pixels(HERO, "header.jpg"));
    }

    #[test]
    fn el_fondo_de_la_tienda_no_se_confunde_con_el_mejor_banner() {
        // Lo que la ficha pide de verdad: la columna `hero_url`, que appdetails
        // rellena con el fondo de la página de la tienda. La prueba de al lado
        // pasa `None` y por eso nunca vio este camino, que es el único que se
        // recorre en la aplicación.
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let fondo =
            "https://store.akamai.steamstatic.com/images/storepagebackground/app/1337760?t=1";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, header_url, hero_url)
                 VALUES (1337760, 'Potion Permit', 'https://shared.steamstatic.com/store_item_assets/steam/apps/1337760/header.jpg', ?1)",
                [fondo],
            )
            .expect("insertar juego");

        // Su puesto es el del bitmap que es, no el primero de la escalera.
        assert_eq!(
            selected_rank(HERO, Some(fondo)),
            HERO.ladder()
                .iter()
                .position(|rung| rung.file == PAGE_BACKGROUND_FILE)
                .expect("el fondo tiene su peldaño"),
        );

        let candidates = candidate_sources(&database, 1_337_760, HERO, Some(fondo))
            .expect("listar candidatas de hero");
        let position = |needle: &str| {
            candidates
                .iter()
                .position(|url| url.ends_with(needle))
                .unwrap_or_else(|| panic!("falta {needle} en {candidates:?}"))
        };
        // Las dos versiones a color se intentan antes que el fondo oscurecido.
        // Sin esto, la ficha de todos los juegos se pintaba con el fondo gris
        // aunque `library_hero.jpg` existiera y respondiera 200.
        assert!(
            candidates[0].ends_with("/apps/1337760/library_hero_2x.jpg"),
            "{candidates:?}"
        );
        assert!(position("library_hero_2x.jpg") < position("library_hero.jpg"));
        assert!(
            position("library_hero.jpg")
                < candidates
                    .iter()
                    .position(|url| url == fondo)
                    .expect("el fondo sigue estando, como último recurso")
        );
    }

    #[test]
    fn hero_and_header_ladders_follow_the_verified_cdn_names() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let hero = "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, header_url, hero_url)
                 VALUES (620, 'Portal 2', 'https://shared.steamstatic.com/store_item_assets/steam/apps/620/header.jpg', ?1)",
                [hero],
            )
            .expect("insertar juego");

        let hero_candidates =
            candidate_sources(&database, 620, HERO, None).expect("listar candidatas de hero");
        assert!(
            hero_candidates[0].ends_with("/apps/620/library_hero_2x.jpg"),
            "{hero_candidates:?}"
        );
        let hero_position = |needle: &str| hero_candidates.iter().position(|u| u.ends_with(needle));
        assert!(hero_position("library_hero_2x.jpg") < hero_position("library_hero.jpg"));
        assert!(hero_position("library_hero.jpg") < hero_position("page_bg_raw.jpg"));
        assert!(
            hero_candidates.iter().any(|url| url == hero),
            "la columna hero dedicada sigue en la cadena: {hero_candidates:?}"
        );

        // Por defecto la cabecera conserva su encuadre; a 2x sube a la cápsula
        // porque `header_2x.jpg` no existe en la CDN (404 en todas las muestras).
        let header_candidates = candidate_sources(&database, 620, HEADER_1X, None)
            .expect("listar candidatas de cabecera");
        assert!(
            header_candidates[0].ends_with("/apps/620/header.jpg"),
            "{header_candidates:?}"
        );
        let dense_header = candidate_sources(&database, 620, HEADER, None).expect("cabecera a 2x");
        assert!(
            dense_header[0].ends_with("/apps/620/capsule_616x353.jpg"),
            "{dense_header:?}"
        );
        assert!(
            header_candidates
                .iter()
                .any(|url| url.ends_with("/apps/620/header.jpg"))
        );
        // Todas las candidatas derivadas pasan la validación de origen.
        for candidate in hero_candidates
            .iter()
            .chain(header_candidates.iter())
            .chain(dense_header.iter())
        {
            assert!(
                validate_source_url(candidate, 620).is_ok(),
                "candidata fuera de la allowlist: {candidate}"
            );
        }
    }

    #[test]
    fn family_catalog_art_is_resolved_and_derives_the_convention_ladder() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let cover = "https://shared.steamstatic.com/store_item_assets/steam/apps/880007/library_600x900.jpg";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO family_catalog_games(app_id, title, cover_url, availability)
                 VALUES (880007, 'Catálogo familiar', ?1, 'unknown')",
                [cover],
            )
            .expect("insertar solo en catálogo familiar");

        let candidates = candidate_sources(&database, 880_007, COVER, None)
            .expect("leer arte familiar sin juego personal");
        assert!(
            candidates[0].ends_with("library_600x900_2x.jpg"),
            "{candidates:?}"
        );
        assert!(candidates.iter().any(|url| url == cover));

        // Sin ninguna URL guardada se recurre a la convención oficial.
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO family_catalog_games(app_id, title, availability)
                 VALUES (880011, 'Sin arte', 'unknown')",
                [],
            )
            .expect("insertar catálogo sin arte");
        let bare = candidate_sources(&database, 880_011, COVER, None).expect("convención");
        assert!(
            bare.first()
                .is_some_and(|url| url.ends_with("/apps/880011/library_600x900_2x.jpg")),
            "{bare:?}"
        );
    }

    #[test]
    fn selected_fallback_is_authoritative_without_a_personal_game_row() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let selected = "https://shared.steamstatic.com/store_item_assets/steam/apps/880006/library_600x900.jpg";

        // El icono no tiene escalera derivable: manda la elección de la interfaz.
        let candidates =
            candidate_sources(&database, 880_006, ICON, Some(selected)).expect("respetar elección");
        assert_eq!(candidates.first().map(String::as_str), Some(selected));
        assert!(validate_source_url(selected, 880_006).is_ok());
    }

    #[test]
    fn derived_assets_respect_host_app_and_path_rules() {
        let base =
            "https://shared.steamstatic.com/store_item_assets/steam/apps/570/library_600x900.jpg";
        assert_eq!(
            derive_library_asset(base, 570, "capsule_616x353.jpg").as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/570/capsule_616x353.jpg"
            )
        );
        // Otro AppID no puede derivarse desde esta URL.
        assert!(derive_library_asset(base, 730, "header.jpg").is_none());
        // Hosts ajenos no son derivables.
        assert!(
            derive_library_asset("https://example.com/apps/570/header.jpg", 570, "header.jpg")
                .is_none()
        );
        // Una ruta con hash pierde el hash: dentro del hash solo viven las
        // derivadas de esa misma imagen, mientras que la ruta plana publica la
        // familia completa.
        assert_eq!(
            derive_library_asset(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/3c96b19255b69aa9b2e131dda3e19d622b0d6562/header.jpg?t=1",
                3_483_510,
                "library_600x900_2x.jpg"
            )
            .as_deref(),
            Some(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/library_600x900_2x.jpg"
            )
        );
    }

    #[test]
    fn source_must_be_https_official_and_app_scoped() {
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/570/header.jpg",
                570,
            )
            .is_ok()
        );
        assert!(validate_source_url("https://example.com/apps/570/header.jpg", 570).is_err());
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/730/header.jpg",
                570,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com:8443/store_item_assets/steam/apps/570/header.jpg",
                570,
            )
            .is_err()
        );
    }

    #[test]
    fn hero_source_uses_the_exact_store_host_path_and_cache_buster() {
        let valid =
            "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1745363004";
        assert!(validate_source_url(valid, 620).is_ok());
        assert!(
            validate_source_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/730?t=1",
                620,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1&t=2",
                620,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com/images/storepagebackground/app/620?t=1",
                620,
            )
            .is_err()
        );
    }

    #[test]
    fn library_source_accepts_the_store_cache_buster() {
        assert!(
            validate_source_url(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/3c96b19255b69aa9b2e131dda3e19d622b0d6562/header.jpg?t=1782353681",
                3_483_510,
            )
            .is_ok()
        );
        assert!(
            validate_source_url(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/header.jpg?t=abc",
                3_483_510,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/3483510/header.jpg?x=1",
                3_483_510,
            )
            .is_err()
        );
    }

    #[test]
    fn disk_identity_changes_when_the_selected_fallback_changes() {
        let cover = "https://shared.steamstatic.com/store_item_assets/steam/apps/880008/library_600x900.jpg";
        let header =
            "https://shared.steamstatic.com/store_item_assets/steam/apps/880008/header.jpg";

        assert_ne!(
            cache_file_name(ICON, cover, "jpg"),
            cache_file_name(ICON, header, "jpg")
        );
        assert_eq!(
            cache_file_name(ICON, cover, "jpg"),
            cache_file_name(ICON, cover, "jpg")
        );
        // Y la huella se puede recuperar desde el nombre para revalidar.
        assert_eq!(
            fingerprint_from_file_name(&cache_file_name(COVER, cover, "jpg")),
            Some(source_fingerprint(cover))
        );
        assert_eq!(fingerprint_from_file_name("cover.jpg"), None);
    }

    // --- Caché local de Steam ------------------------------------------------

    #[test]
    fn local_steam_cache_is_only_adopted_when_it_reaches_the_requested_density() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let root = directory.path();

        // Layout clásico: archivos planos en <appid>/.
        let flat_dir = root.join("570");
        fs::create_dir_all(&flat_dir).expect("crear dir plano");
        fs::write(flat_dir.join("library_600x900_2x.jpg"), jpeg(600, 900)).expect("cover 2x");
        fs::write(flat_dir.join("header.jpg"), jpeg(460, 215)).expect("header");
        let (cover, facts) = local_library_art(root, 570, COVER).expect("cover plano");
        assert!(cover.ends_with("library_600x900_2x.jpg"));
        assert_eq!(facts.width, Some(600));

        // Un cliente con solo la versión pequeña no bloquea la descarga 2x.
        let small_dir = root.join("571");
        fs::create_dir_all(&small_dir).expect("crear dir pequeño");
        fs::write(small_dir.join("library_600x900.jpg"), jpeg(300, 450)).expect("cover 1x");
        assert!(
            local_library_art(root, 571, COVER).is_none(),
            "300×450 local no debe impedir bajar la variante 600×900"
        );
        let (small, _) = local_library_art(root, 571, COVER_1X).expect("a 1x sí sirve");
        assert!(small.ends_with("library_600x900.jpg"));

        // Layout nuevo: la portada vertical vive en un subdirectorio hash.
        let hashed_dir = root.join("3483510").join("c45a0dcc");
        fs::create_dir_all(&hashed_dir).expect("crear dir hash");
        fs::write(hashed_dir.join("library_capsule_2x.jpg"), jpeg(600, 900)).expect("cápsula");
        let (cover, _) = local_library_art(root, 3_483_510, COVER).expect("cover hash");
        assert!(cover.ends_with("library_capsule_2x.jpg"));

        // Contenido que no es imagen se rechaza aunque el nombre coincida.
        let bad_dir = root.join("9999");
        fs::create_dir_all(&bad_dir).expect("crear dir inválido");
        fs::write(bad_dir.join("library_600x900_2x.jpg"), b"<html>nope</html>")
            .expect("escribir falso jpg");
        assert!(local_library_art(root, 9999, COVER).is_none());
        // Iconos: nunca se resuelven en local.
        assert!(local_library_art(root, 570, ICON).is_none());
    }

    // --- Red -----------------------------------------------------------------

    #[test]
    fn a_304_response_reuses_the_cached_file_without_downloading_again() {
        let runtime = runtime();
        let body = jpeg(600, 900);
        let server = spawn_server(&runtime, body.clone(), "\"abc-123\"");
        let url = format!("http://{}/library_600x900_2x.jpg", server.addr);

        let fresh = runtime
            .block_on(fetch_asset(&plain_client(), &url, None))
            .expect("primera descarga");
        assert_eq!(fresh.bytes, body);
        assert_eq!(fresh.etag.as_deref(), Some("\"abc-123\""));
        assert_eq!(server.bodies_sent.load(Ordering::SeqCst), 1);

        let conditional = Conditional {
            etag: fresh.etag.clone(),
            last_modified: fresh.last_modified.clone(),
        };
        let revalidated = runtime
            .block_on(fetch_asset(&plain_client(), &url, Some(&conditional)))
            .expect_err("Steam responde 304");
        assert_eq!(revalidated.code, "art_not_modified");
        assert_eq!(
            server.bodies_sent.load(Ordering::SeqCst),
            1,
            "un 304 no debe volver a transferir el cuerpo"
        );
        assert_eq!(server.requests.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn concurrent_requests_for_the_same_artwork_download_only_once() {
        let runtime = runtime();
        let body = jpeg(600, 900);
        let server = spawn_server(&runtime, body, "\"dedupe\"");
        let url = format!("http://{}/library_600x900_2x.jpg", server.addr);
        let key = (880_030_u32, COVER, source_fingerprint(&url));
        let directory = TempDir::new().expect("crear directorio temporal");
        let stored: Arc<std::sync::Mutex<Option<PathBuf>>> = Arc::new(std::sync::Mutex::new(None));

        runtime.block_on(async {
            let mut tasks = Vec::new();
            for index in 0..8 {
                let url = url.clone();
                let stored = Arc::clone(&stored);
                let destination = directory.path().join(format!("descarga-{index}.jpg"));
                tasks.push(tokio::spawn(async move {
                    // Mismo patrón que producción: un cerrojo por obra y una
                    // segunda comprobación de la caché tras adquirirlo.
                    let lock = request_lock(key);
                    let _guard = lock.lock().await;
                    if stored.lock().expect("cerrojo de prueba").is_some() {
                        return;
                    }
                    let asset = fetch_asset(&plain_client(), &url, None)
                        .await
                        .expect("descargar");
                    std::fs::write(&destination, &asset.bytes).expect("escribir");
                    *stored.lock().expect("cerrojo de prueba") = Some(destination);
                }));
            }
            for task in tasks {
                task.await.expect("tarea concurrente");
            }
        });

        assert_eq!(
            server.requests.load(Ordering::SeqCst),
            1,
            "ocho tarjetas simultáneas deben producir una sola descarga"
        );
        assert!(stored.lock().expect("cerrojo de prueba").is_some());
        clear_negative(key);
    }

    #[test]
    fn global_download_budget_never_exceeds_six_distinct_images() {
        runtime().block_on(async {
            let permits = download_slots()
                .acquire_many(MAX_CONCURRENT_DOWNLOADS as u32)
                .await
                .expect("ocupar presupuesto completo");
            assert!(download_slots().try_acquire().is_err());
            drop(permits);
            assert!(download_slots().try_acquire().is_ok());
        });
    }

    #[test]
    fn concurrent_cards_share_one_request_lock_per_artwork() {
        let first = request_lock((880_001, COVER, 1));
        let duplicate = request_lock((880_001, COVER, 1));
        let different_variant = request_lock((880_001, HEADER, 1));
        let different_density = request_lock((880_001, COVER_1X, 1));
        let different_source = request_lock((880_001, COVER, 2));

        assert!(Arc::ptr_eq(&first, &duplicate));
        assert!(!Arc::ptr_eq(&first, &different_variant));
        assert!(!Arc::ptr_eq(&first, &different_density));
        assert!(!Arc::ptr_eq(&first, &different_source));
    }

    #[test]
    fn negative_cache_expires_without_losing_the_original_error_contract() {
        let key = (880_002, COVER, 1);
        let expected = AppError::new("art_unavailable", "Imagen no disponible.");
        negative_cache()
            .lock()
            .expect("bloquear caché negativa")
            .insert(
                key,
                NegativeEntry {
                    until: Instant::now() + Duration::from_secs(1),
                    error: expected.clone(),
                },
            );

        let hit = negative_cache_hit(key).expect("reutilizar resultado negativo vigente");
        assert_eq!(hit.code, expected.code);
        assert_eq!(hit.message, expected.message);

        negative_cache()
            .lock()
            .expect("bloquear caché negativa")
            .insert(
                key,
                NegativeEntry {
                    until: Instant::now() - Duration::from_millis(1),
                    error: expected,
                },
            );
        assert!(negative_cache_hit(key).is_none());
        clear_negative(key);
    }

    #[test]
    fn retries_only_throttling_and_server_failures() {
        assert!(retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(!retryable_status(StatusCode::NOT_FOUND));
        assert!(!retryable_status(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn backoff_grows_but_stays_bounded() {
        use super::{MAX_RETRY_DELAY, backoff_delay};
        let first = backoff_delay(0, 7);
        let second = backoff_delay(1, 7);
        assert!(second > first);
        for attempt in 0..16 {
            assert!(
                backoff_delay(attempt, u64::MAX) <= MAX_RETRY_DELAY + Duration::from_millis(120)
            );
        }
    }

    // --- Camino caliente -----------------------------------------------------

    #[test]
    fn one_thousand_hot_cache_reads_stay_inside_desktop_budget() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let app_id = 880_005;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880005/library_600x900_2x.jpg";
        let art_directory = directory
            .path()
            .join("cache")
            .join("steam-art")
            .join(app_id.to_string());
        fs::create_dir_all(&art_directory).expect("crear caché local");
        let local_path = art_directory.join(cache_file_name(COVER, source, "jpg"));
        fs::write(&local_path, jpeg(600, 900)).expect("escribir JPEG válido");
        let connection = database.open().expect("abrir base de prueba");
        connection
            .execute(
                "INSERT INTO games(app_id, title, cover_url) VALUES (?1, 'Gate de artwork', ?2)",
                params![app_id, source],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO image_cache(app_id, variant, local_path) VALUES (?1, 'cover', ?2)",
                params![app_id, local_path.display().to_string()],
            )
            .expect("registrar imagen local");
        drop(connection);
        let canonical_local_path = local_path
            .canonicalize()
            .expect("canonicalizar imagen de prueba")
            .display()
            .to_string();

        let runtime = runtime();
        let started = Instant::now();
        runtime.block_on(async {
            for _ in 0..1_000 {
                let result = cache(
                    &database,
                    &directory.path().join("cache"),
                    app_id,
                    COVER,
                    Some(source),
                )
                .await
                .expect("resolver desde caché local");
                assert_eq!(result.local_path, canonical_local_path);
                assert_eq!(result.width, Some(600));
                assert_eq!(result.height, Some(900));
            }
        });
        let elapsed = started.elapsed();
        eprintln!("gate artwork: 1000 lecturas locales en {elapsed:?}");
        assert!(
            elapsed < Duration::from_secs(5),
            "1000 lecturas de artwork local tardaron {elapsed:?}, por encima del presupuesto de 5 s"
        );
    }

    #[test]
    fn family_only_hot_cache_is_served_without_an_image_cache_foreign_key() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let app_id = 880_009;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880009/library_600x900_2x.jpg";
        let art_directory = directory
            .path()
            .join("cache")
            .join("steam-art")
            .join(app_id.to_string());
        fs::create_dir_all(&art_directory).expect("crear caché familiar");
        let local_path = art_directory.join(cache_file_name(COVER, source, "jpg"));
        fs::write(&local_path, jpeg(600, 900)).expect("escribir JPEG válido");
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO family_catalog_games(app_id, title, cover_url, availability)
                 VALUES (?1, 'Solo familiar', ?2, 'unknown')",
                params![app_id, source],
            )
            .expect("insertar catálogo familiar");

        let result = runtime()
            .block_on(cache(
                &database,
                &directory.path().join("cache"),
                app_id,
                COVER,
                Some(source),
            ))
            .expect("servir arte familiar local");

        assert_eq!(
            result.local_path,
            local_path
                .canonicalize()
                .expect("canonicalizar arte familiar")
                .display()
                .to_string()
        );
        let cached_rows: i64 = database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT COUNT(*) FROM image_cache WHERE app_id = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("contar caché relacional");
        assert_eq!(cached_rows, 0);
    }

    #[test]
    fn downloaded_family_art_does_not_require_a_games_foreign_key() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO family_catalog_games(app_id, title, availability)
                 VALUES (880010, 'Descarga familiar', 'unknown')",
                [],
            )
            .expect("insertar catálogo familiar");

        record_cached_path(
            &database,
            880_010,
            COVER,
            "/cache/steam-art/880010/cover.jpg",
            None,
            None,
        )
        .expect("omitir registro relacional sin fallar la descarga familiar");

        let cached_rows: i64 = database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT COUNT(*) FROM image_cache WHERE app_id = 880010",
                [],
                |row| row.get(0),
            )
            .expect("contar caché relacional");
        assert_eq!(cached_rows, 0);
    }

    #[test]
    fn revalidation_only_reuses_validators_from_the_same_source() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let app_id = 880_050;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880050/library_600x900_2x.jpg";
        let other = "https://shared.steamstatic.com/store_item_assets/steam/apps/880050/header.jpg";
        let connection = database.open().expect("abrir base");
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, 'Revalidación')",
                params![app_id],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO image_cache(app_id, variant, local_path, etag, updated_at)
                 VALUES (?1, 'cover', ?2, '\"abc\"', '2000-01-01T00:00:00.000Z')",
                params![
                    app_id,
                    format!(
                        "/cache/steam-art/880050/{}",
                        cache_file_name(COVER, source, "jpg")
                    )
                ],
            )
            .expect("registrar fila antigua");
        drop(connection);

        let due = revalidation_due(&database, app_id, COVER, source).expect("toca revalidar");
        assert_eq!(due.etag.as_deref(), Some("\"abc\""));
        // Con otra URL, el ETag guardado no vale: reutilizarlo daría un 304 falso.
        assert!(revalidation_due(&database, app_id, COVER, other).is_none());

        // Una fila recién escrita todavía no toca revalidarla.
        database
            .open()
            .expect("abrir base")
            .execute(
                "UPDATE image_cache SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now')
                  WHERE app_id = ?1",
                params![app_id],
            )
            .expect("refrescar fila");
        assert!(revalidation_due(&database, app_id, COVER, source).is_none());
    }

    #[test]
    fn existing_cache_finds_the_file_for_the_exact_source() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let cache_root = directory.path().join("cache");
        let app_id = 880_040;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880040/library_600x900_2x.jpg";
        let other = "https://shared.steamstatic.com/store_item_assets/steam/apps/880040/header.jpg";
        let art_dir = cache_root.join("steam-art").join(app_id.to_string());
        fs::create_dir_all(&art_dir).expect("crear caché");
        fs::write(
            art_dir.join(cache_file_name(COVER, source, "jpg")),
            jpeg(600, 900),
        )
        .expect("escribir portada");

        assert!(existing_cache(&cache_root, app_id, COVER, source).is_some());
        assert!(existing_cache(&cache_root, app_id, COVER, other).is_none());
        assert!(existing_cache(&cache_root, app_id, HEADER, source).is_none());
    }

    #[test]
    fn valid_image_file_rejects_directories_and_unknown_extensions() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let root = directory.path();
        fs::create_dir_all(root.join("subdir.jpg")).expect("crear directorio con extensión");
        assert!(valid_image_file(&root.join("subdir.jpg")).is_none());
        let odd = root.join("imagen.gif");
        fs::write(&odd, jpeg(10, 10)).expect("escribir gif falso");
        assert!(valid_image_file(&odd).is_none());
        let report = MaintenanceReport::default();
        assert_eq!(report.removed_files, 0);
    }
}
