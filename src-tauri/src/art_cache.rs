use crate::db::Database;
use crate::error::{AppError, AppResult};
use reqwest::{Client, Response, StatusCode, header, redirect::Policy};
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, Semaphore};
use url::Url;
use uuid::Uuid;

const MAX_ART_BYTES: usize = 10 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const RETRY_DELAY: Duration = Duration::from_millis(180);
const MAX_DOWNLOAD_ATTEMPTS: usize = 2;
const TRANSIENT_NEGATIVE_TTL: Duration = Duration::from_secs(30);
const DEFINITIVE_NEGATIVE_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_CONCURRENT_DOWNLOADS: usize = 6;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtVariant {
    Cover,
    Header,
    Icon,
    Hero,
}

impl ArtVariant {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "cover" => Ok(Self::Cover),
            "header" => Ok(Self::Header),
            "icon" => Ok(Self::Icon),
            "hero" => Ok(Self::Hero),
            _ => Err(AppError::validation("La variante de imagen no es válida.")),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Header => "header",
            Self::Icon => "icon",
            Self::Hero => "hero",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedArt {
    pub app_id: u32,
    pub variant: String,
    pub local_path: String,
}

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
    let source = source_url(database, app_id, variant, selected_source)?;
    validate_source_url(&source, app_id, variant)?;
    if let Some(existing) = existing_cache(cache_root, app_id, variant, &source) {
        return Ok(cached_art(app_id, variant, existing));
    }

    let key = (app_id, variant, source_fingerprint(&source));
    if let Some(error) = negative_cache_hit(key) {
        return Err(error);
    }

    let request_lock = request_lock(key);
    let _request_guard = request_lock.lock().await;

    // Otra tarjeta pudo completar la descarga mientras esperábamos este lock.
    if let Some(existing) = existing_cache(cache_root, app_id, variant, &source) {
        return Ok(cached_art(app_id, variant, existing));
    }
    if let Some(error) = negative_cache_hit(key) {
        return Err(error);
    }

    let result = download_and_store(database, cache_root, app_id, variant, &source).await;
    match &result {
        Ok(_) => clear_negative(key),
        Err(error) => remember_negative(key, error.clone()),
    }
    result
}

async fn download_and_store(
    database: &Database,
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
) -> AppResult<CachedArt> {
    let _download_slot = download_slots().acquire().await.map_err(|_| {
        AppError::new(
            "art_download_queue",
            "No se pudo reservar una descarga de imagen.",
        )
    })?;
    let mut response = send_with_retry(http_client()?, source).await?;
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
    let extension = allowed_extension(&mime).ok_or_else(|| {
        AppError::new(
            "art_content_type",
            "Steam devolvió un contenido que no es una imagen compatible.",
        )
    })?;
    let mut bytes = Vec::new();
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
    if !matches_magic_bytes(&mime, &bytes) {
        return Err(AppError::new(
            "art_signature",
            "Steam devolvió una imagen cuya firma no coincide con su formato.",
        ));
    }

    let directory = cache_root.join("steam-art").join(app_id.to_string());
    let destination = directory.join(cache_file_name(variant, source, extension));
    let temporary = directory.join(format!(".{}.{}.part", variant.key(), Uuid::new_v4()));
    let destination_for_write = destination.clone();
    let write_task = tauri::async_runtime::spawn_blocking(move || -> std::io::Result<()> {
        let write_result = (|| {
            fs::create_dir_all(&directory)?;
            fs::write(&temporary, bytes)?;
            if destination_for_write.exists() {
                fs::remove_file(&destination_for_write)?;
            }
            fs::rename(&temporary, &destination_for_write)
        })();
        if write_result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        write_result
    });
    join_art_cache_task(write_task).await?;

    let local_path = destination.display().to_string();
    record_cached_path(database, app_id, variant, &local_path)?;
    Ok(CachedArt {
        app_id,
        variant: variant.key().to_owned(),
        local_path,
    })
}

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

fn record_cached_path(
    database: &Database,
    app_id: u32,
    variant: ArtVariant,
    local_path: &str,
) -> AppResult<()> {
    database.open()?.execute(
        "INSERT INTO image_cache(app_id, variant, local_path)
         SELECT ?1, ?2, ?3
          WHERE EXISTS (SELECT 1 FROM games WHERE app_id = ?1)
         ON CONFLICT(app_id, variant) DO UPDATE SET
            local_path = excluded.local_path,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        rusqlite::params![app_id, variant.key(), local_path],
    )?;
    Ok(())
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
    Ok(())
}

fn cached_art(app_id: u32, variant: ArtVariant, local_path: PathBuf) -> CachedArt {
    CachedArt {
        app_id,
        variant: variant.key().to_owned(),
        local_path: local_path.display().to_string(),
    }
}

fn http_client() -> AppResult<&'static Client> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let client = Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(Policy::none())
        .user_agent("Vindexa/0.1 (+https://vindexa.app)")
        .build()
        .map_err(|_| download_error())?;
    let _ = CLIENT.set(client);
    CLIENT.get().ok_or_else(download_error)
}

async fn send_with_retry(client: &Client, source: &str) -> AppResult<Response> {
    for attempt in 0..MAX_DOWNLOAD_ATTEMPTS {
        match client.get(source).send().await {
            Ok(response)
                if retryable_status(response.status()) && attempt + 1 < MAX_DOWNLOAD_ATTEMPTS =>
            {
                tokio::time::sleep(RETRY_DELAY).await;
            }
            Ok(response) => return Ok(response),
            Err(error)
                if retryable_request_error(&error) && attempt + 1 < MAX_DOWNLOAD_ATTEMPTS =>
            {
                tokio::time::sleep(RETRY_DELAY).await;
            }
            Err(_) => return Err(download_error()),
        }
    }
    Err(download_error())
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
    let ttl = if matches!(error.code.as_str(), "art_download" | "steam_network") {
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

fn existing_cache(
    cache_root: &Path,
    app_id: u32,
    variant: ArtVariant,
    source: &str,
) -> Option<PathBuf> {
    let directory = cache_root.join("steam-art").join(app_id.to_string());
    ["jpg", "png", "webp"]
        .into_iter()
        .map(|extension| directory.join(cache_file_name(variant, source, extension)))
        .find_map(|candidate| {
            trusted_cached_path(cache_root, app_id, &candidate.display().to_string())
        })
}

fn source_url(
    database: &Database,
    app_id: u32,
    variant: ArtVariant,
    selected_source: Option<&str>,
) -> AppResult<String> {
    if let Some(source) = selected_source
        .map(str::trim)
        .filter(|source| !source.is_empty())
    {
        return Ok(source.to_owned());
    }
    let query = match variant {
        ArtVariant::Cover => {
            "SELECT COALESCE(
                (SELECT cover_url FROM games WHERE app_id = ?1),
                (SELECT cover_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT header_url FROM games WHERE app_id = ?1),
                (SELECT header_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT icon_url FROM games WHERE app_id = ?1),
                (SELECT icon_url FROM family_catalog_games WHERE app_id = ?1)
            )"
        }
        ArtVariant::Header => {
            "SELECT COALESCE(
                (SELECT header_url FROM games WHERE app_id = ?1),
                (SELECT header_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT cover_url FROM games WHERE app_id = ?1),
                (SELECT cover_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT icon_url FROM games WHERE app_id = ?1),
                (SELECT icon_url FROM family_catalog_games WHERE app_id = ?1)
            )"
        }
        ArtVariant::Icon => {
            "SELECT COALESCE(
                (SELECT icon_url FROM games WHERE app_id = ?1),
                (SELECT icon_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT cover_url FROM games WHERE app_id = ?1),
                (SELECT cover_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT header_url FROM games WHERE app_id = ?1),
                (SELECT header_url FROM family_catalog_games WHERE app_id = ?1)
            )"
        }
        ArtVariant::Hero => {
            "SELECT COALESCE(
                (SELECT hero_url FROM games WHERE app_id = ?1),
                (SELECT header_url FROM games WHERE app_id = ?1),
                (SELECT header_url FROM family_catalog_games WHERE app_id = ?1),
                (SELECT cover_url FROM games WHERE app_id = ?1),
                (SELECT cover_url FROM family_catalog_games WHERE app_id = ?1)
            )"
        }
    };
    let source: Option<String> = database
        .open()?
        .query_row(query, [app_id], |row| row.get(0))?;
    source.ok_or_else(|| {
        AppError::not_found("Este juego no tiene una imagen oficial disponible para esta variante.")
    })
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

fn trusted_cached_path(cache_root: &Path, app_id: u32, value: &str) -> Option<PathBuf> {
    let expected_root = cache_root.join("steam-art").join(app_id.to_string());
    let canonical_root = expected_root.canonicalize().ok()?;
    let canonical_file = Path::new(value).canonicalize().ok()?;
    if !canonical_file.is_file() || !canonical_file.starts_with(canonical_root) {
        return None;
    }
    let metadata = canonical_file.metadata().ok()?;
    if metadata.len() == 0 || metadata.len() > MAX_ART_BYTES as u64 {
        return None;
    }
    let mime = match canonical_file
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        _ => return None,
    };
    let mut prefix = [0_u8; 12];
    let mut file = fs::File::open(&canonical_file).ok()?;
    let read = file.read(&mut prefix).ok()?;
    matches_magic_bytes(mime, &prefix[..read]).then_some(canonical_file)
}

fn validate_source_url(value: &str, app_id: u32, _variant: ArtVariant) -> AppResult<()> {
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
    let valid_hero_source = url.host_str() == Some("store.akamai.steamstatic.com")
        && url.path() == format!("/images/storepagebackground/app/{app_id}")
        && (pairs.is_empty()
            || (pairs.len() == 1
                && pairs[0].0 == "t"
                && !pairs[0].1.is_empty()
                && pairs[0].1.bytes().all(|byte| byte.is_ascii_digit())));
    let host = url.host_str().unwrap_or_default();
    let allowed_host = matches!(
        host,
        "shared.steamstatic.com"
            | "shared.cloudflare.steamstatic.com"
            | "cdn.cloudflare.steamstatic.com"
            | "shared.akamai.steamstatic.com"
            | "media.steampowered.com"
    );
    let valid_library_source =
        allowed_host && url.path().contains(&format!("/apps/{app_id}/")) && url.query().is_none();
    let valid_source = valid_hero_source || valid_library_source;
    if !valid_source || url.fragment().is_some() {
        return Err(AppError::validation(
            "La imagen no pertenece a un dominio y ruta oficiales permitidos de Steam.",
        ));
    }
    Ok(())
}

fn allowed_extension(mime: &str) -> Option<&'static str> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/webp" => Some("webp"),
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

fn download_error() -> AppError {
    AppError::new(
        "art_download",
        "No se pudo descargar la imagen oficial desde Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ArtVariant, MAX_CONCURRENT_DOWNLOADS, NegativeEntry, allowed_extension, cache,
        cache_file_name, clear_negative, download_slots, join_art_cache_task, matches_magic_bytes,
        negative_cache, negative_cache_hit, record_cached_path, request_lock, retryable_status,
        source_url, trusted_cached_path, validate_source_url,
    };
    use crate::db::Database;
    use crate::error::AppError;
    use reqwest::StatusCode;
    use rusqlite::params;
    use std::fs;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    #[test]
    fn variant_and_content_type_are_allowlisted() {
        assert_eq!(ArtVariant::parse("cover").unwrap(), ArtVariant::Cover);
        assert!(ArtVariant::parse("../../secret").is_err());
        assert_eq!(allowed_extension("image/jpeg"), Some("jpg"));
        assert_eq!(allowed_extension("text/html"), None);
        assert!(matches_magic_bytes("image/jpeg", &[0xff, 0xd8, 0xff, 0xdb]));
        assert!(!matches_magic_bytes("image/jpeg", b"<html>"));
    }

    #[test]
    fn cache_worker_failures_do_not_expose_panic_details() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("crear runtime");
        let error = runtime.block_on(async {
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

    #[test]
    fn source_must_be_https_official_and_app_scoped() {
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/570/header.jpg",
                570,
                ArtVariant::Header,
            )
            .is_ok()
        );
        assert!(
            validate_source_url(
                "https://example.com/apps/570/header.jpg",
                570,
                ArtVariant::Header,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/730/header.jpg",
                570,
                ArtVariant::Header,
            )
            .is_err()
        );
    }

    #[test]
    fn hero_source_uses_the_exact_store_host_path_and_cache_buster() {
        let valid =
            "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1745363004";
        assert!(validate_source_url(valid, 620, ArtVariant::Hero).is_ok());
        assert!(
            validate_source_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/730?t=1",
                620,
                ArtVariant::Hero,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1&t=2",
                620,
                ArtVariant::Hero,
            )
            .is_err()
        );
        assert!(
            validate_source_url(
                "https://shared.steamstatic.com/images/storepagebackground/app/620?t=1",
                620,
                ArtVariant::Hero,
            )
            .is_err()
        );
        assert!(validate_source_url(valid, 620, ArtVariant::Header).is_ok());
    }

    #[test]
    fn hero_variant_reads_the_dedicated_database_column() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
        let hero = "https://store.akamai.steamstatic.com/images/storepagebackground/app/620?t=1";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, header_url, hero_url)
                 VALUES (620, 'Portal 2', 'https://shared.steamstatic.com/steam/apps/620/header.jpg', ?1)",
                [hero],
            )
            .expect("insertar juego con hero");

        assert_eq!(
            source_url(&database, 620, ArtVariant::Hero, None).expect("leer hero dedicado"),
            hero
        );
        let header = source_url(&database, 620, ArtVariant::Header, None).expect("leer header");
        let hero = source_url(&database, 620, ArtVariant::Hero, None).expect("leer hero");
        assert_ne!(header, hero);
    }

    #[test]
    fn selected_fallback_is_authoritative_without_a_personal_game_row() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
        let selected = "https://shared.steamstatic.com/store_item_assets/steam/apps/880006/library_600x900.jpg";

        assert_eq!(
            source_url(&database, 880_006, ArtVariant::Icon, Some(selected))
                .expect("respetar fallback elegido por la UI"),
            selected
        );
        assert!(validate_source_url(selected, 880_006, ArtVariant::Icon).is_ok());
    }

    #[test]
    fn family_catalog_art_is_resolved_when_games_has_no_row() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
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

        assert_eq!(
            source_url(&database, 880_007, ArtVariant::Cover, None)
                .expect("leer arte familiar sin juego personal"),
            cover
        );
    }

    #[test]
    fn disk_identity_changes_when_the_selected_fallback_changes() {
        let cover = "https://shared.steamstatic.com/store_item_assets/steam/apps/880008/library_600x900.jpg";
        let header =
            "https://shared.steamstatic.com/store_item_assets/steam/apps/880008/header.jpg";

        assert_ne!(
            cache_file_name(ArtVariant::Icon, cover, "jpg"),
            cache_file_name(ArtVariant::Icon, header, "jpg")
        );
        assert_eq!(
            cache_file_name(ArtVariant::Icon, cover, "jpg"),
            cache_file_name(ArtVariant::Icon, cover, "jpg")
        );
    }

    #[test]
    fn global_download_budget_never_exceeds_six_distinct_images() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("crear runtime de prueba");
        runtime.block_on(async {
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
        let first = request_lock((880_001, ArtVariant::Cover, 1));
        let duplicate = request_lock((880_001, ArtVariant::Cover, 1));
        let different_variant = request_lock((880_001, ArtVariant::Header, 1));
        let different_source = request_lock((880_001, ArtVariant::Cover, 2));

        assert!(Arc::ptr_eq(&first, &duplicate));
        assert!(!Arc::ptr_eq(&first, &different_variant));
        assert!(!Arc::ptr_eq(&first, &different_source));
    }

    #[test]
    fn negative_cache_expires_without_losing_the_original_error_contract() {
        let key = (880_002, ArtVariant::Cover, 1);
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
    fn cached_file_must_stay_in_scope_and_keep_a_valid_signature() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let app_root = directory.path().join("steam-art/880003");
        fs::create_dir_all(&app_root).expect("crear caché de imagen");
        let valid = app_root.join("cover.jpg");
        fs::write(&valid, [0xff, 0xd8, 0xff, 0xdb]).expect("escribir JPEG mínimo");
        let corrupt = app_root.join("header.jpg");
        fs::write(&corrupt, b"<html>").expect("escribir archivo corrupto");

        assert_eq!(
            trusted_cached_path(directory.path(), 880_003, &valid.display().to_string()),
            valid.canonicalize().ok()
        );
        assert!(
            trusted_cached_path(directory.path(), 880_003, &corrupt.display().to_string())
                .is_none()
        );
        assert!(
            trusted_cached_path(directory.path(), 880_004, &valid.display().to_string()).is_none()
        );
    }

    #[test]
    fn one_thousand_hot_cache_reads_stay_inside_desktop_budget() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
        let app_id = 880_005;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880005/library_600x900.jpg";
        let art_directory = directory
            .path()
            .join("cache")
            .join("steam-art")
            .join(app_id.to_string());
        fs::create_dir_all(&art_directory).expect("crear caché local");
        let local_path = art_directory.join(cache_file_name(ArtVariant::Cover, source, "jpg"));
        fs::write(&local_path, [0xff, 0xd8, 0xff, 0xdb]).expect("escribir JPEG mínimo");
        let connection = database.open().expect("abrir base de prueba");
        connection
            .execute(
                "INSERT INTO games(app_id, title, cover_url)
                 VALUES (?1, 'Gate de artwork', ?2)",
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

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("crear runtime de prueba");
        let started = Instant::now();
        runtime.block_on(async {
            for _ in 0..1_000 {
                let result = cache(
                    &database,
                    &directory.path().join("cache"),
                    app_id,
                    ArtVariant::Cover,
                    Some(source),
                )
                .await
                .expect("resolver desde caché local");
                assert_eq!(result.local_path, canonical_local_path);
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
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
        let app_id = 880_009;
        let source = "https://shared.steamstatic.com/store_item_assets/steam/apps/880009/library_600x900.jpg";
        let art_directory = directory
            .path()
            .join("cache")
            .join("steam-art")
            .join(app_id.to_string());
        fs::create_dir_all(&art_directory).expect("crear caché familiar");
        let local_path = art_directory.join(cache_file_name(ArtVariant::Cover, source, "jpg"));
        fs::write(&local_path, [0xff, 0xd8, 0xff, 0xdb]).expect("escribir JPEG mínimo");
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO family_catalog_games(app_id, title, cover_url, availability)
                 VALUES (?1, 'Solo familiar', ?2, 'unknown')",
                params![app_id, source],
            )
            .expect("insertar catálogo familiar");

        let result = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("crear runtime")
            .block_on(cache(
                &database,
                &directory.path().join("cache"),
                app_id,
                ArtVariant::Cover,
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
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar base de prueba");
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
            ArtVariant::Cover,
            "/cache/steam-art/880010/cover.jpg",
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
}
