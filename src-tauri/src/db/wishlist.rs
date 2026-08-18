//! Lista de deseados propia y vídeos asociados a un juego (migración 022).
//!
//! # Privacidad: Vindexa no habla con YouTube
//!
//! Este módulo **no consulta la API de YouTube** ni ninguna otra API de Google:
//! no hay clave, no hay peticiones de metadatos, no se resuelve el título ni la
//! duración de un vídeo por red. Todo lo que se guarda aquí lo escribe la
//! persona usuaria a mano (o proviene de la ficha oficial de Steam, con
//! `provider = 'steam'`).
//!
//! La única forma de que un dato salga hacia Google es que la persona usuaria
//! pulse reproducir: entonces la interfaz monta un `iframe` contra
//! `https://www.youtube-nocookie.com/embed/<id>` y esa reproducción concreta —
//! y sólo ella — llega a sus servidores. Por eso:
//!
//! - Guardamos únicamente el identificador canónico del vídeo, nunca la URL
//!   pegada. [`parse_youtube_video_id`] es la frontera de seguridad: extrae los
//!   11 caracteres del alfabeto `[A-Za-z0-9_-]` y rechaza cualquier otra cosa,
//!   de modo que la URL del `iframe` no puede ser inyectada desde el texto que
//!   se pega en el formulario.
//! - [`GameVideo::embed_url`] la construye el backend, no el frontend. La
//!   interfaz sólo tiene que usar la cadena que recibe.
//! - Se rechazan las miniaturas alojadas en hosts de Google
//!   ([`validate_thumbnail_url`]): pintarlas supondría una petición a Google
//!   *antes* de pulsar reproducir, que es justo lo que este módulo promete que
//!   no ocurre. Sólo se admiten miniaturas del CDN oficial de Steam, que ya
//!   están permitidas por la CSP de la ventana principal.
//!
//! Esto es coherente con `PRIVACY.md`: sin telemetría, sin terceros salvo los
//! que la persona usuaria activa de forma explícita.
//!
//! # Una lista, dos tablas
//!
//! Una lista de deseados es, sobre todo, juegos que **no** tienes, y la
//! biblioteca sólo guarda los que sí. Por eso una entrada de deseados vive en
//! `wishlist_entries` cuando el juego está en la biblioteca y en
//! `catalog_wishlist_entries` cuando no (ver [`crate::db::catalog`]). Las dos
//! tablas tienen las mismas columnas y este módulo las lee como una sola lista:
//! los cubos, el orden y las sumas por moneda abarcan siempre las dos.
//!
//! El reparto no es visible desde fuera. [`WishlistGame`] dice, con
//! [`WishlistGame::in_library`], si el juego se tiene; y sólo trae la ficha
//! completa de biblioteca cuando existe de verdad, en vez de rellenarla con
//! ceros que pasarían por hechos.
//!
//! # Fronteras con el resto de `db`
//!
//! - El mapeo de [`GameSummary`] pertenece a `db::library`; aquí se invoca
//!   [`crate::db::library::game_summary`].
//! - La reordenación reutiliza [`crate::db::curated::place_before`], que a su
//!   vez replica el enfoque de `db::library_dnd`.

use crate::db::catalog::{self, CatalogGame};
use crate::db::curated::{ensure_game_is_listable, place_before, validate_choice, validate_note};
use crate::error::{AppError, AppResult};
use crate::models::GameSummary;
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use url::Url;

/// Cubos válidos de la lista de deseados; coinciden con el `CHECK` de la
/// migración 022 y con el orden en el que se presentan.
pub const WISHLIST_BUCKETS: [&str; 4] = ["buying_now", "waiting_sale", "considering", "watching"];

/// Proveedores de vídeo admitidos.
pub const VIDEO_PROVIDERS: [&str; 2] = ["youtube", "steam"];
/// Tipos de vídeo admitidos.
pub const VIDEO_KINDS: [&str; 5] = ["gameplay", "review", "impressions", "trailer", "guide"];
/// Procedencias admitidas de un vídeo.
pub const VIDEO_SOURCES: [&str; 2] = ["manual", "store"];

/// Número máximo de juegos en la lista de deseados.
/// Tope de entradas. Existe para que un error de importación no llene la
/// base, no para limitar a nadie: una lista de deseados real ronda el millar y
/// media, así que el tope tiene que quedar muy por encima de lo normal.
pub const MAX_WISHLIST_ENTRIES: usize = 10_000;
/// Prioridad máxima admitida (0 = sin prioridad).
pub const MAX_PRIORITY: u8 = 5;
/// Precio objetivo máximo, en céntimos de la moneda indicada.
pub const MAX_TARGET_PRICE_CENTS: i64 = 100_000_000;
/// Número máximo de vídeos por juego.
pub const MAX_VIDEOS_PER_GAME: usize = 40;
/// Longitud máxima del título de un vídeo.
pub const MAX_VIDEO_TITLE_LENGTH: usize = 200;
/// Longitud máxima del nombre de canal.
pub const MAX_CHANNEL_LENGTH: usize = 120;
/// Longitud máxima de cualquier URL persistida.
pub const MAX_URL_LENGTH: usize = 2_048;
/// Duración máxima admitida de un vídeo (24 horas).
pub const MAX_DURATION_SECONDS: u32 = 86_400;
/// Longitud exacta de un identificador de vídeo de YouTube.
pub const YOUTUBE_ID_LENGTH: usize = 11;
/// Longitud máxima del identificador numérico de un vídeo de Steam.
pub const MAX_STEAM_VIDEO_ID_LENGTH: usize = 20;

/// Hosts admitidos para una miniatura. Son los mismos que ya autoriza la CSP de
/// la ventana principal para imágenes; deliberadamente no incluyen `ytimg.com`.
const ALLOWED_THUMBNAIL_HOSTS: [&str; 8] = [
    "shared.steamstatic.com",
    "shared.fastly.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
    "shared.akamai.steamstatic.com",
    "cdn.cloudflare.steamstatic.com",
    "cdn.akamai.steamstatic.com",
    "store.akamai.steamstatic.com",
    "media.steampowered.com",
];

/// Hosts de YouTube desde los que aceptamos extraer un identificador.
const YOUTUBE_HOSTS: [&str; 6] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "youtube-nocookie.com",
    "www.youtube-nocookie.com",
    "youtu.be",
];

/// Segmentos de ruta de YouTube que preceden al identificador.
const YOUTUBE_ID_PREFIXES: [&str; 4] = ["embed", "shorts", "live", "v"];

const ANCHOR_MISSING: &str = "El juego de referencia ya no está en ese cubo de deseados.";

/// La lista de deseados completa, venga de la biblioteca o del catálogo.
///
/// La última columna, `in_library`, es la que decide después con qué ficha se
/// hidrata cada fila. Es la fuente de todo lo que cuenta filas —el límite, el
/// orden dentro de un cubo— porque una entrada invisible sigue ocupando sitio.
const ALL_ENTRIES: &str = "
    SELECT app_id, bucket, priority, position, note, target_price_cents,
           currency, added_at, updated_at, 1 AS in_library
      FROM wishlist_entries
     UNION ALL
    SELECT app_id, bucket, priority, position, note, target_price_cents,
           currency, added_at, updated_at, 0 AS in_library
      FROM catalog_wishlist_entries";

/// Lo mismo, pero sólo lo que se puede presentar.
///
/// Un juego de la biblioteca sin ficha en `game_personal` no tiene
/// [`GameSummary`] que enseñar, así que queda fuera de la vista y de las sumas
/// en vez de romper la consulta. Las entradas de catálogo siempre son visibles:
/// su ficha se deriva del AppID y no depende de ninguna otra tabla.
const VISIBLE_ENTRIES: &str = "
    SELECT app_id, bucket, priority, position, note, target_price_cents,
           currency, added_at, updated_at, 1 AS in_library
      FROM wishlist_entries w
     WHERE EXISTS (SELECT 1 FROM game_personal p WHERE p.app_id = w.app_id)
     UNION ALL
    SELECT app_id, bucket, priority, position, note, target_price_cents,
           currency, added_at, updated_at, 0 AS in_library
      FROM catalog_wishlist_entries";

// ---------------------------------------------------------------------------
// Modelos: deseados
// ---------------------------------------------------------------------------

/// El juego al que apunta una entrada de deseados.
///
/// Lo poco que se sabe siempre —AppID, nombre y arte derivada del AppID— vive
/// arriba, para que quien lo pinte no tenga que preguntar primero si el juego se
/// posee. La ficha de biblioteca va aparte y **sólo existe si el juego está en
/// la biblioteca**: un juego que no tienes no tiene estado, ni progreso, ni
/// horas jugadas, y presentar esos campos a cero los convertiría en afirmaciones
/// falsas.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistGame {
    pub app_id: u32,
    pub title: String,
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    /// `true` cuando el juego tiene ficha en la biblioteca.
    pub in_library: bool,
    /// Ficha completa de biblioteca. Ausente mientras el juego no se tenga.
    pub library: Option<GameSummary>,
}

impl WishlistGame {
    fn from_library(summary: GameSummary) -> Self {
        Self {
            app_id: summary.app_id,
            title: summary.title.clone(),
            cover_url: summary.cover_url.clone(),
            header_url: summary.header_url.clone(),
            in_library: true,
            library: Some(summary),
        }
    }

    fn from_catalog(game: CatalogGame) -> Self {
        Self {
            app_id: game.app_id,
            title: game.title,
            cover_url: Some(game.cover_url),
            header_url: Some(game.header_url),
            in_library: false,
            library: None,
        }
    }
}

/// Un juego de la lista de deseados con su ficha resuelta.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistEntry {
    pub game: WishlistGame,
    pub bucket: String,
    pub priority: u8,
    pub position: i64,
    pub note: String,
    pub target_price_cents: Option<i64>,
    pub currency: Option<String>,
    pub added_at: String,
    pub updated_at: String,
}

/// Alta o edición de una entrada de deseados.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SaveWishlistEntryInput {
    pub app_id: u32,
    pub bucket: String,
    #[serde(default)]
    pub priority: u8,
    #[serde(default)]
    pub note: String,
    /// Precio objetivo en céntimos. Exige `currency`; ninguno de los dos vale
    /// por separado porque un importe sin moneda no se puede agregar.
    #[serde(default)]
    pub target_price_cents: Option<i64>,
    #[serde(default)]
    pub currency: Option<String>,
    /// Juego ante el cual colocarla dentro del cubo. `None` la deja al final.
    #[serde(default)]
    pub before_app_id: Option<u32>,
}

/// Un cubo de la lista de deseados con sus juegos ya ordenados.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistBucket {
    pub bucket: String,
    pub items: Vec<WishlistEntry>,
    pub total: i64,
}

/// Suma de precios objetivo de una sola moneda.
///
/// Nunca se mezclan monedas distintas: Vindexa no conoce tipos de cambio y
/// convertirlos sería inventar un dato.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistTargetTotal {
    pub currency: String,
    pub total_cents: i64,
    pub entries: i64,
}

/// Vista completa de la lista de deseados.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistOverview {
    pub buckets: Vec<WishlistBucket>,
    pub total: i64,
    /// Sumas por moneda. Sólo incluye entradas con importe y moneda conocidos.
    pub target_totals: Vec<WishlistTargetTotal>,
    /// Entradas sin precio objetivo utilizable; se informan aparte en vez de
    /// contarlas como cero.
    pub entries_without_target: i64,
}

// ---------------------------------------------------------------------------
// Modelos: vídeos
// ---------------------------------------------------------------------------

/// Un vídeo asociado a un juego.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVideo {
    pub app_id: u32,
    pub video_id: String,
    pub provider: String,
    pub kind: String,
    pub title: String,
    pub channel: String,
    pub duration_seconds: Option<u32>,
    pub published_at: Option<String>,
    pub thumbnail_url: Option<String>,
    pub source: String,
    pub position: i64,
    pub created_at: String,
    /// URL de incrustación construida por el backend. Sólo existe para
    /// `provider = "youtube"` y siempre apunta a `youtube-nocookie.com`.
    pub embed_url: Option<String>,
}

/// Alta o edición de un vídeo.
///
/// `Default` reproduce a propósito los mismos valores que los `serde(default)`
/// de este struct: un `SaveGameVideoInput::default()` y un JSON que omita esos
/// campos deben describir exactamente la misma petición.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveGameVideoInput {
    pub app_id: u32,
    /// URL completa o identificador pegado por la persona usuaria.
    pub video: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default = "default_source")]
    pub source: String,
}

/// Referencia estable a un vídeo dentro de un juego.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameVideoRef {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub video_id: String,
}

fn default_provider() -> String {
    "youtube".to_string()
}

fn default_kind() -> String {
    "gameplay".to_string()
}

fn default_source() -> String {
    "manual".to_string()
}

impl Default for SaveGameVideoInput {
    fn default() -> Self {
        Self {
            app_id: 0,
            video: String::new(),
            provider: default_provider(),
            kind: default_kind(),
            title: String::new(),
            channel: String::new(),
            duration_seconds: None,
            published_at: None,
            thumbnail_url: None,
            source: default_source(),
        }
    }
}

impl Default for GameVideoRef {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            video_id: String::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Deseados
// ---------------------------------------------------------------------------

/// Devuelve la lista de deseados completa, agrupada por cubos.
pub fn wishlist_overview(connection: &Connection) -> AppResult<WishlistOverview> {
    let mut buckets = Vec::with_capacity(WISHLIST_BUCKETS.len());
    let mut total = 0;
    for bucket in WISHLIST_BUCKETS {
        let items = bucket_entries(connection, bucket)?;
        let bucket_total = i64::try_from(items.len()).unwrap_or(i64::MAX);
        total += bucket_total;
        buckets.push(WishlistBucket {
            bucket: bucket.to_string(),
            items,
            total: bucket_total,
        });
    }

    let mut statement = connection.prepare(&format!(
        "SELECT currency, SUM(target_price_cents), COUNT(*)
           FROM ({VISIBLE_ENTRIES})
          WHERE target_price_cents IS NOT NULL
            AND currency IS NOT NULL
            AND TRIM(currency) <> ''
          GROUP BY currency
          ORDER BY currency ASC"
    ))?;
    let target_totals = statement
        .query_map([], |row| {
            Ok(WishlistTargetTotal {
                currency: row.get(0)?,
                total_cents: row.get(1)?,
                entries: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let entries_without_target: i64 = connection.query_row(
        &format!(
            "SELECT COUNT(*) FROM ({VISIBLE_ENTRIES})
              WHERE target_price_cents IS NULL
                 OR currency IS NULL
                 OR TRIM(currency) = ''"
        ),
        [],
        |row| row.get(0),
    )?;

    Ok(WishlistOverview {
        buckets,
        total,
        target_totals,
        entries_without_target,
    })
}

/// Devuelve una entrada concreta de la lista de deseados.
pub fn wishlist_entry(connection: &Connection, app_id: u32) -> AppResult<WishlistEntry> {
    let row = connection
        .query_row(
            &format!("SELECT * FROM ({ALL_ENTRIES}) WHERE app_id = ?1"),
            [app_id],
            map_wishlist_row,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("Ese juego no está en la lista de deseados."))?;
    hydrate(connection, row)
}

/// Dónde vive —o dónde debe vivir— la entrada de deseados de un juego.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WishlistHome {
    /// El juego está en la biblioteca: su entrada va en `wishlist_entries`.
    Library,
    /// El juego está en el catálogo: su entrada va en `catalog_wishlist_entries`.
    Catalog,
}

impl WishlistHome {
    const fn table(self) -> &'static str {
        match self {
            Self::Library => "wishlist_entries",
            Self::Catalog => "catalog_wishlist_entries",
        }
    }
}

/// Decide en qué tabla vive la entrada de un juego.
///
/// Un juego sólo puede estar en un sitio: la migración 030 impide con un
/// disparador que un AppID esté a la vez en `games` y en `catalog_games`.
fn wishlist_home(connection: &Connection, app_id: u32) -> AppResult<WishlistHome> {
    if catalog::in_library(connection, app_id)? {
        return Ok(WishlistHome::Library);
    }
    if catalog::is_in_catalog(connection, app_id)? {
        return Ok(WishlistHome::Catalog);
    }
    Err(AppError::not_found(format!(
        "El juego {app_id} no está ni en la biblioteca ni en el catálogo de deseados."
    )))
}

/// Crea o actualiza una entrada de deseados dentro de una transacción.
pub fn save_wishlist_entry(
    connection: &mut Connection,
    input: &SaveWishlistEntryInput,
) -> AppResult<WishlistEntry> {
    let bucket = validate_choice(&input.bucket, &WISHLIST_BUCKETS, "cubo de deseados")?;
    let note = validate_note(&input.note)?;
    let priority = validate_priority(input.priority)?;
    let (target_price_cents, currency) =
        validate_target_price(input.target_price_cents, input.currency.as_deref())?;
    if input.before_app_id == Some(input.app_id) {
        return Err(AppError::validation(
            "Un juego no puede colocarse antes de sí mismo.",
        ));
    }

    let transaction = connection.transaction()?;
    let home = wishlist_home(&transaction, input.app_id)?;
    if home == WishlistHome::Library {
        ensure_game_is_listable(&transaction, input.app_id)?;
    }

    let previous_bucket = current_bucket(&transaction, input.app_id)?;
    if previous_bucket.is_none() {
        let total = wishlist_total(&transaction)?;
        if usize::try_from(total).unwrap_or(usize::MAX) >= MAX_WISHLIST_ENTRIES {
            return Err(AppError::validation(format!(
                "La lista de deseados no puede superar {MAX_WISHLIST_ENTRIES} juegos."
            )));
        }
    }

    let tail_position = bucket_order(&transaction, &bucket)?.len() as i64;
    let table = home.table();
    transaction.execute(
        &format!(
            "INSERT INTO {table}(
                app_id, bucket, priority, position, note, target_price_cents, currency
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(app_id) DO UPDATE SET
                bucket = excluded.bucket,
                priority = excluded.priority,
                note = excluded.note,
                target_price_cents = excluded.target_price_cents,
                currency = excluded.currency,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')"
        ),
        params![
            input.app_id,
            bucket,
            priority,
            tail_position,
            note,
            target_price_cents,
            currency,
        ],
    )?;

    // El cubo de origen sólo hay que compactarlo cuando el juego cambia de sitio.
    if let Some(origin) = previous_bucket.as_deref()
        && origin != bucket
    {
        let remaining = bucket_order(&transaction, origin)?;
        write_bucket_order(&transaction, origin, &remaining)?;
    }
    let target_order = bucket_order(&transaction, &bucket)?;
    let next_order =
        if previous_bucket.as_deref() == Some(bucket.as_str()) && input.before_app_id.is_none() {
            // Editar la nota, la prioridad o el precio no debe mover el juego de
            // sitio: sólo se recoloca al entrar en el cubo o al pedirlo con un ancla.
            target_order
        } else {
            place_before(
                &target_order,
                &[input.app_id],
                input.before_app_id,
                ANCHOR_MISSING,
            )?
        };
    write_bucket_order(&transaction, &bucket, &next_order)?;
    transaction.commit()?;

    wishlist_entry(connection, input.app_id)
}

/// Quita un juego de la lista de deseados y compacta su cubo.
///
/// Si el juego era del catálogo se retira también su ficha: existía únicamente
/// para respaldar este deseo, y sin él no describe nada que Vindexa tenga que
/// recordar.
pub fn remove_wishlist_entry(connection: &mut Connection, app_id: u32) -> AppResult<()> {
    let transaction = connection.transaction()?;
    let bucket = current_bucket(&transaction, app_id)?
        .ok_or_else(|| AppError::not_found("Ese juego no está en la lista de deseados."))?;
    if catalog::is_in_catalog(&transaction, app_id)? {
        catalog::delete_catalog_game(&transaction, app_id)?;
    } else {
        transaction.execute("DELETE FROM wishlist_entries WHERE app_id = ?1", [app_id])?;
    }
    let remaining = bucket_order(&transaction, &bucket)?;
    write_bucket_order(&transaction, &bucket, &remaining)?;
    transaction.commit()?;
    Ok(())
}

/// Mueve un juego dentro de su cubo o hacia otro cubo, en una transacción.
pub fn move_wishlist_entry(
    connection: &mut Connection,
    app_id: u32,
    bucket: &str,
    before_app_id: Option<u32>,
) -> AppResult<()> {
    let bucket = validate_choice(bucket, &WISHLIST_BUCKETS, "cubo de deseados")?;
    if before_app_id == Some(app_id) {
        return Err(AppError::validation(
            "Un juego no puede colocarse antes de sí mismo.",
        ));
    }
    let transaction = connection.transaction()?;
    let origin = current_bucket(&transaction, app_id)?
        .ok_or_else(|| AppError::not_found("Ese juego no está en la lista de deseados."))?;

    if origin != bucket {
        let table = wishlist_home(&transaction, app_id)?.table();
        transaction.execute(
            &format!(
                "UPDATE {table}
                    SET bucket = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                  WHERE app_id = ?1"
            ),
            params![app_id, bucket],
        )?;
        let remaining = bucket_order(&transaction, &origin)?;
        write_bucket_order(&transaction, &origin, &remaining)?;
    }
    let target_order = bucket_order(&transaction, &bucket)?;
    let placed = place_before(&target_order, &[app_id], before_app_id, ANCHOR_MISSING)?;
    write_bucket_order(&transaction, &bucket, &placed)?;
    transaction.commit()?;
    Ok(())
}

/// Reescribe el orden completo de un cubo.
///
/// `ordered_app_ids` debe contener exactamente los juegos guardados en ese
/// cubo. Un cubo vacío admite una lista vacía.
pub fn reorder_wishlist_bucket(
    connection: &mut Connection,
    bucket: &str,
    ordered_app_ids: &[u32],
) -> AppResult<()> {
    let bucket = validate_choice(bucket, &WISHLIST_BUCKETS, "cubo de deseados")?;
    let transaction = connection.transaction()?;
    let unique: HashSet<u32> = ordered_app_ids.iter().copied().collect();
    if unique.len() != ordered_app_ids.len() || unique.contains(&0) {
        return Err(AppError::validation(
            "La lista de ordenación contiene juegos duplicados o no válidos.",
        ));
    }
    let stored: HashSet<u32> = bucket_order(&transaction, &bucket)?.into_iter().collect();
    if stored != unique {
        return Err(AppError::validation(
            "La lista de ordenación no coincide con los juegos guardados en ese cubo.",
        ));
    }
    write_bucket_order(&transaction, &bucket, ordered_app_ids)?;
    transaction.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Vídeos
// ---------------------------------------------------------------------------

/// Lista los vídeos de un juego, opcionalmente filtrados por tipo.
pub fn list_game_videos(
    connection: &Connection,
    app_id: u32,
    kind: Option<&str>,
) -> AppResult<Vec<GameVideo>> {
    let kind = match kind.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Some(validate_choice(value, &VIDEO_KINDS, "tipo de vídeo")?),
        None => None,
    };
    let mut statement = connection.prepare(
        "SELECT app_id, video_id, provider, kind, title, channel, duration_seconds,
                published_at, thumbnail_url, source, position, created_at
           FROM game_videos
          WHERE app_id = ?1 AND (?2 IS NULL OR kind = ?2)
          ORDER BY kind ASC, position ASC, video_id ASC",
    )?;
    let videos = statement
        .query_map(params![app_id, kind], map_game_video)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(videos)
}

/// Guarda un vídeo asociado a un juego.
///
/// El identificador se normaliza siempre: para YouTube se extraen los 11
/// caracteres canónicos de la URL o del texto pegado; para Steam se exige un
/// identificador numérico.
pub fn save_game_video(
    connection: &mut Connection,
    input: &SaveGameVideoInput,
) -> AppResult<GameVideo> {
    let provider = validate_choice(&input.provider, &VIDEO_PROVIDERS, "proveedor de vídeo")?;
    let kind = validate_choice(&input.kind, &VIDEO_KINDS, "tipo de vídeo")?;
    let source = validate_choice(&input.source, &VIDEO_SOURCES, "origen del vídeo")?;
    let video_id = parse_video_reference(&provider, &input.video)?;
    let title = validate_text(&input.title, MAX_VIDEO_TITLE_LENGTH, "título del vídeo")?;
    let channel = validate_text(&input.channel, MAX_CHANNEL_LENGTH, "canal del vídeo")?;
    let duration_seconds = validate_duration(input.duration_seconds)?;
    let published_at = validate_published_at(input.published_at.as_deref())?;
    let thumbnail_url = validate_thumbnail_url(input.thumbnail_url.as_deref())?;

    let transaction = connection.transaction()?;
    ensure_game_is_listable(&transaction, input.app_id)?;

    let previous_kind = transaction
        .query_row(
            "SELECT kind FROM game_videos WHERE app_id = ?1 AND provider = ?2 AND video_id = ?3",
            params![input.app_id, provider, video_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if previous_kind.is_none() {
        let total: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM game_videos WHERE app_id = ?1",
            [input.app_id],
            |row| row.get(0),
        )?;
        if usize::try_from(total).unwrap_or(usize::MAX) >= MAX_VIDEOS_PER_GAME {
            return Err(AppError::validation(format!(
                "Un juego no puede tener más de {MAX_VIDEOS_PER_GAME} vídeos guardados."
            )));
        }
    }

    let target_order = video_order(&transaction, input.app_id, &kind)?;
    let position = target_order
        .iter()
        .position(|reference| reference.provider == provider && reference.video_id == video_id)
        .unwrap_or(target_order.len()) as i64;

    transaction.execute(
        "INSERT INTO game_videos(
            app_id, video_id, provider, kind, title, channel, duration_seconds,
            published_at, thumbnail_url, source, position
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(app_id, provider, video_id) DO UPDATE SET
            kind = excluded.kind,
            title = excluded.title,
            channel = excluded.channel,
            duration_seconds = excluded.duration_seconds,
            published_at = excluded.published_at,
            thumbnail_url = excluded.thumbnail_url,
            source = excluded.source",
        params![
            input.app_id,
            video_id,
            provider,
            kind,
            title,
            channel,
            duration_seconds,
            published_at,
            thumbnail_url,
            source,
            position,
        ],
    )?;
    // Si el vídeo cambió de tipo hay que compactar también el tipo de origen.
    if let Some(origin) = previous_kind.as_deref()
        && origin != kind
    {
        let remaining = video_order(&transaction, input.app_id, origin)?;
        write_video_order(&transaction, input.app_id, &remaining)?;
    }
    let order = video_order(&transaction, input.app_id, &kind)?;
    write_video_order(&transaction, input.app_id, &order)?;
    transaction.commit()?;

    game_video(connection, input.app_id, &provider, &video_id)
}

/// Devuelve un vídeo concreto.
pub fn game_video(
    connection: &Connection,
    app_id: u32,
    provider: &str,
    video_id: &str,
) -> AppResult<GameVideo> {
    connection
        .query_row(
            "SELECT app_id, video_id, provider, kind, title, channel, duration_seconds,
                    published_at, thumbnail_url, source, position, created_at
               FROM game_videos
              WHERE app_id = ?1 AND provider = ?2 AND video_id = ?3",
            params![app_id, provider, video_id],
            map_game_video,
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("Ese vídeo ya no está guardado."))
}

/// Borra un vídeo y compacta las posiciones de su tipo.
pub fn delete_game_video(
    connection: &mut Connection,
    app_id: u32,
    provider: &str,
    video_id: &str,
) -> AppResult<()> {
    let provider = validate_choice(provider, &VIDEO_PROVIDERS, "proveedor de vídeo")?;
    let transaction = connection.transaction()?;
    let kind = transaction
        .query_row(
            "SELECT kind FROM game_videos WHERE app_id = ?1 AND provider = ?2 AND video_id = ?3",
            params![app_id, provider, video_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("Ese vídeo ya no está guardado."))?;
    transaction.execute(
        "DELETE FROM game_videos WHERE app_id = ?1 AND provider = ?2 AND video_id = ?3",
        params![app_id, provider, video_id],
    )?;
    let remaining = video_order(&transaction, app_id, &kind)?;
    write_video_order(&transaction, app_id, &remaining)?;
    transaction.commit()?;
    Ok(())
}

/// Reescribe el orden de los vídeos de un juego dentro de un tipo.
pub fn reorder_game_videos(
    connection: &mut Connection,
    app_id: u32,
    kind: &str,
    ordered: &[GameVideoRef],
) -> AppResult<()> {
    let kind = validate_choice(kind, &VIDEO_KINDS, "tipo de vídeo")?;
    let mut normalized = Vec::with_capacity(ordered.len());
    for reference in ordered {
        let provider =
            validate_choice(&reference.provider, &VIDEO_PROVIDERS, "proveedor de vídeo")?;
        let video_id = parse_video_reference(&provider, &reference.video_id)?;
        normalized.push(GameVideoRef { provider, video_id });
    }
    let unique: HashSet<(String, String)> = normalized
        .iter()
        .map(|reference| (reference.provider.clone(), reference.video_id.clone()))
        .collect();
    if unique.len() != normalized.len() {
        return Err(AppError::validation(
            "La lista de ordenación repite algún vídeo.",
        ));
    }

    let transaction = connection.transaction()?;
    let stored: HashSet<(String, String)> = video_order(&transaction, app_id, &kind)?
        .into_iter()
        .map(|reference| (reference.provider, reference.video_id))
        .collect();
    if stored != unique {
        return Err(AppError::validation(
            "La lista de ordenación no coincide con los vídeos guardados de ese tipo.",
        ));
    }
    write_video_order(&transaction, app_id, &normalized)?;
    transaction.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Frontera de seguridad: identificadores de vídeo
// ---------------------------------------------------------------------------

/// Extrae el identificador canónico de 11 caracteres de un vídeo de YouTube.
///
/// Acepta el identificador suelto o una URL completa de `youtube.com/watch`,
/// `youtu.be`, `youtube.com/embed`, `/shorts`, `/live` y `/v`, con o sin
/// esquema y con parámetros adicionales. Rechaza cualquier otra cosa: otros
/// dominios, esquemas no HTTP(S), rutas relativas, marcado y longitudes
/// distintas de 11.
///
/// Esto es una frontera de seguridad: el identificador devuelto se interpola en
/// `https://www.youtube-nocookie.com/embed/<id>`, así que sólo puede contener
/// caracteres seguros en una ruta.
pub fn parse_youtube_video_id(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(invalid_youtube());
    }
    if trimmed.chars().count() > MAX_URL_LENGTH {
        return Err(invalid_youtube());
    }
    if is_youtube_id(trimmed) {
        return Ok(trimmed.to_string());
    }

    let candidate = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let url = Url::parse(&candidate).map_err(|_| invalid_youtube())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid_youtube());
    }
    let host = url
        .host_str()
        .ok_or_else(invalid_youtube)?
        .to_ascii_lowercase();
    if !YOUTUBE_HOSTS.contains(&host.as_str()) {
        return Err(invalid_youtube());
    }

    let segments = url
        .path_segments()
        .map(|segments| {
            segments
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if host == "youtu.be" {
        let candidate = segments.first().copied().ok_or_else(invalid_youtube)?;
        return accept_youtube_id(candidate);
    }
    if let Some(first) = segments.first()
        && YOUTUBE_ID_PREFIXES.contains(first)
    {
        let candidate = segments.get(1).copied().ok_or_else(invalid_youtube)?;
        return accept_youtube_id(candidate);
    }
    if segments.first().copied() == Some("watch") {
        let candidate = url
            .query_pairs()
            .find(|(key, _)| key == "v")
            .map(|(_, value)| value.into_owned())
            .ok_or_else(invalid_youtube)?;
        return accept_youtube_id(&candidate);
    }
    Err(invalid_youtube())
}

/// Normaliza el identificador de un vídeo según su proveedor.
pub fn parse_video_reference(provider: &str, raw: &str) -> AppResult<String> {
    match provider {
        "youtube" => parse_youtube_video_id(raw),
        "steam" => parse_steam_video_id(raw),
        _ => Err(AppError::validation(
            "El proveedor de vídeo no es válido. Valores admitidos: youtube, steam.",
        )),
    }
}

/// Identificador numérico de un vídeo oficial de la ficha de Steam.
fn parse_steam_video_id(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    let length = trimmed.chars().count();
    if length == 0
        || length > MAX_STEAM_VIDEO_ID_LENGTH
        || !trimmed.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(AppError::validation(
            "El identificador de un vídeo de Steam debe ser numérico.",
        ));
    }
    Ok(trimmed.to_string())
}

fn accept_youtube_id(candidate: &str) -> AppResult<String> {
    if is_youtube_id(candidate) {
        Ok(candidate.to_string())
    } else {
        Err(invalid_youtube())
    }
}

fn is_youtube_id(candidate: &str) -> bool {
    candidate.len() == YOUTUBE_ID_LENGTH
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn invalid_youtube() -> AppError {
    AppError::validation(
        "Pega el enlace de un vídeo de YouTube (youtube.com/watch, youtu.be o /embed) o su identificador de 11 caracteres.",
    )
}

/// Construye la URL de incrustación sin cookies de terceros.
///
/// Sólo se usa con un identificador ya validado por
/// [`parse_youtube_video_id`].
fn youtube_embed_url(video_id: &str) -> Option<String> {
    is_youtube_id(video_id).then(|| format!("https://www.youtube-nocookie.com/embed/{video_id}"))
}

// ---------------------------------------------------------------------------
// Auxiliares privados
// ---------------------------------------------------------------------------

fn bucket_entries(connection: &Connection, bucket: &str) -> AppResult<Vec<WishlistEntry>> {
    let mut statement = connection.prepare(&format!(
        "SELECT * FROM ({VISIBLE_ENTRIES})
          WHERE bucket = ?1
          ORDER BY priority DESC, position ASC, app_id ASC"
    ))?;
    let rows = statement
        .query_map([bucket], map_wishlist_row)?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    rows.into_iter()
        .map(|row| hydrate(connection, row))
        .collect()
}

struct WishlistRow {
    app_id: u32,
    bucket: String,
    priority: u8,
    position: i64,
    note: String,
    target_price_cents: Option<i64>,
    currency: Option<String>,
    added_at: String,
    updated_at: String,
    in_library: bool,
}

fn map_wishlist_row(row: &Row<'_>) -> rusqlite::Result<WishlistRow> {
    Ok(WishlistRow {
        app_id: row.get(0)?,
        bucket: row.get(1)?,
        priority: row.get(2)?,
        position: row.get(3)?,
        note: row.get(4)?,
        target_price_cents: row.get(5)?,
        currency: row.get(6)?,
        added_at: row.get(7)?,
        updated_at: row.get(8)?,
        in_library: row.get::<_, i64>(9)? != 0,
    })
}

fn hydrate(connection: &Connection, row: WishlistRow) -> AppResult<WishlistEntry> {
    let game = if row.in_library {
        WishlistGame::from_library(crate::db::library::game_summary(connection, row.app_id)?)
    } else {
        let catalog = catalog::catalog_game(connection, row.app_id)?.ok_or_else(|| {
            AppError::not_found("Ese juego ya no está en el catálogo de deseados.")
        })?;
        WishlistGame::from_catalog(catalog)
    };
    Ok(WishlistEntry {
        game,
        bucket: row.bucket,
        priority: row.priority,
        position: row.position,
        note: row.note,
        target_price_cents: row.target_price_cents,
        currency: row.currency,
        added_at: row.added_at,
        updated_at: row.updated_at,
    })
}

fn map_game_video(row: &Row<'_>) -> rusqlite::Result<GameVideo> {
    let video_id: String = row.get(1)?;
    let provider: String = row.get(2)?;
    let embed_url = if provider == "youtube" {
        youtube_embed_url(&video_id)
    } else {
        None
    };
    Ok(GameVideo {
        app_id: row.get(0)?,
        video_id,
        provider,
        kind: row.get(3)?,
        title: row.get(4)?,
        channel: row.get(5)?,
        duration_seconds: row.get(6)?,
        published_at: row.get(7)?,
        thumbnail_url: row.get(8)?,
        source: row.get(9)?,
        position: row.get(10)?,
        created_at: row.get(11)?,
        embed_url,
    })
}

/// Cubo en el que está guardado un juego, mire donde mire.
fn current_bucket(connection: &Connection, app_id: u32) -> AppResult<Option<String>> {
    Ok(connection
        .query_row(
            &format!("SELECT bucket FROM ({ALL_ENTRIES}) WHERE app_id = ?1"),
            [app_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?)
}

/// Cuántos juegos hay en la lista de deseados, estén o no en la biblioteca.
fn wishlist_total(connection: &Connection) -> AppResult<i64> {
    Ok(connection.query_row(
        &format!("SELECT COUNT(*) FROM ({ALL_ENTRIES})"),
        [],
        |row| row.get(0),
    )?)
}

/// Orden completo de un cubo, con las dos procedencias entremezcladas.
///
/// Deliberadamente no filtra por visibilidad: una entrada que ahora no se puede
/// pintar sigue ocupando su posición, y compactar el cubo sin contarla dejaría
/// dos juegos con el mismo número.
fn bucket_order(connection: &Connection, bucket: &str) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(&format!(
        "SELECT app_id, position FROM ({ALL_ENTRIES})
          WHERE bucket = ?1
          ORDER BY position ASC, app_id ASC"
    ))?;
    let order = statement
        .query_map([bucket], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(order)
}

fn write_bucket_order(connection: &Connection, bucket: &str, app_ids: &[u32]) -> AppResult<()> {
    let mut update_library = connection.prepare_cached(
        "UPDATE wishlist_entries
            SET position = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1 AND bucket = ?2",
    )?;
    let mut update_catalog = connection.prepare_cached(
        "UPDATE catalog_wishlist_entries
            SET position = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
          WHERE app_id = ?1 AND bucket = ?2",
    )?;
    for (position, app_id) in app_ids.iter().enumerate() {
        let arguments = params![app_id, bucket, position as i64];
        // Un AppID sólo puede estar en una de las dos tablas, así que la suma
        // vale exactamente uno cuando el juego sigue en ese cubo.
        let changed = update_library.execute(arguments)? + update_catalog.execute(arguments)?;
        if changed != 1 {
            return Err(AppError::not_found(
                "Uno o más juegos ya no están en la lista de deseados.",
            ));
        }
    }
    Ok(())
}

fn video_order(connection: &Connection, app_id: u32, kind: &str) -> AppResult<Vec<GameVideoRef>> {
    let mut statement = connection.prepare(
        "SELECT provider, video_id FROM game_videos
          WHERE app_id = ?1 AND kind = ?2
          ORDER BY position ASC, video_id ASC",
    )?;
    let order = statement
        .query_map(params![app_id, kind], |row| {
            Ok(GameVideoRef {
                provider: row.get(0)?,
                video_id: row.get(1)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(order)
}

fn write_video_order(
    connection: &Connection,
    app_id: u32,
    ordered: &[GameVideoRef],
) -> AppResult<()> {
    let mut update = connection.prepare_cached(
        "UPDATE game_videos SET position = ?4
          WHERE app_id = ?1 AND provider = ?2 AND video_id = ?3",
    )?;
    for (position, reference) in ordered.iter().enumerate() {
        let changed = update.execute(params![
            app_id,
            reference.provider,
            reference.video_id,
            position as i64
        ])?;
        if changed != 1 {
            return Err(AppError::not_found("Ese vídeo ya no está guardado."));
        }
    }
    Ok(())
}

fn validate_priority(priority: u8) -> AppResult<u8> {
    if priority > MAX_PRIORITY {
        return Err(AppError::validation(format!(
            "La prioridad debe estar entre 0 y {MAX_PRIORITY}."
        )));
    }
    Ok(priority)
}

fn validate_target_price(
    cents: Option<i64>,
    currency: Option<&str>,
) -> AppResult<(Option<i64>, Option<String>)> {
    let currency = currency
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                Ok(value.to_ascii_uppercase())
            } else {
                Err(AppError::validation(
                    "La moneda debe ser un código ISO 4217 de tres letras, por ejemplo EUR.",
                ))
            }
        })
        .transpose()?;
    match (cents, currency) {
        (None, None) => Ok((None, None)),
        (Some(cents), Some(currency)) => {
            if !(0..=MAX_TARGET_PRICE_CENTS).contains(&cents) {
                return Err(AppError::validation(format!(
                    "El precio objetivo debe estar entre 0 y {MAX_TARGET_PRICE_CENTS} céntimos."
                )));
            }
            Ok((Some(cents), Some(currency)))
        }
        (Some(_), None) => Err(AppError::validation(
            "Indica la moneda del precio objetivo: un importe sin moneda no se puede sumar.",
        )),
        (None, Some(_)) => Err(AppError::validation(
            "Indica el precio objetivo o quita la moneda: una moneda sola no aporta nada.",
        )),
    }
}

fn validate_text(value: &str, max_length: usize, label: &str) -> AppResult<String> {
    let value = value.trim();
    if value.chars().count() > max_length {
        return Err(AppError::validation(format!(
            "El {label} no puede superar {max_length} caracteres."
        )));
    }
    Ok(value.to_string())
}

fn validate_duration(duration: Option<u32>) -> AppResult<Option<u32>> {
    match duration {
        Some(seconds) if seconds > MAX_DURATION_SECONDS => Err(AppError::validation(format!(
            "La duración del vídeo no puede superar {MAX_DURATION_SECONDS} segundos."
        ))),
        other => Ok(other),
    }
}

fn validate_published_at(value: Option<&str>) -> AppResult<Option<String>> {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if let Ok(moment) = DateTime::parse_from_rfc3339(raw) {
        return Ok(Some(
            moment
                .with_timezone(&Utc)
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
        ));
    }
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").map_err(|_| {
        AppError::validation(
            "La fecha de publicación debe usar el formato AAAA-MM-DD o una marca ISO 8601 completa.",
        )
    })?;
    if date.format("%Y-%m-%d").to_string() != raw {
        return Err(AppError::validation(
            "La fecha de publicación debe ser una fecha real en formato AAAA-MM-DD.",
        ));
    }
    Ok(Some(raw.to_string()))
}

/// Valida la miniatura de un vídeo.
///
/// Sólo se admiten hosts del CDN oficial de Steam. Una miniatura de YouTube
/// obligaría a pedirle una imagen a Google al pintar la lista, antes de que la
/// persona usuaria decida reproducir nada: eso rompería la promesa de este
/// módulo y la CSP de la ventana principal tampoco la cargaría.
pub fn validate_thumbnail_url(value: Option<&str>) -> AppResult<Option<String>> {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if raw.chars().count() > MAX_URL_LENGTH {
        return Err(AppError::validation(
            "La dirección de la miniatura es demasiado larga.",
        ));
    }
    let url = Url::parse(raw)
        .map_err(|_| AppError::validation("La dirección de la miniatura no es una URL válida."))?;
    if url.scheme() != "https" {
        return Err(AppError::validation(
            "La miniatura debe servirse por HTTPS.",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| AppError::validation("La dirección de la miniatura no tiene host."))?
        .to_ascii_lowercase();
    if !ALLOWED_THUMBNAIL_HOSTS.contains(&host.as_str()) {
        return Err(AppError::validation(
            "Sólo se admiten miniaturas del CDN oficial de Steam: Vindexa no pide imágenes a YouTube antes de que decidas reproducir.",
        ));
    }
    Ok(Some(url.to_string()))
}

// ---------------------------------------------------------------------------
// Importación desde la lista de deseados de Steam
// ---------------------------------------------------------------------------

/// Cubo en el que aterriza todo lo que llega de Steam.
///
/// Steam no tiene cubos: su lista es una sola. `considering` es el valor por
/// defecto de la migración 022 y el único que no afirma nada sobre la intención
/// de compra, que es justo lo que Vindexa no sabe todavía.
pub const WISHLIST_IMPORT_BUCKET: &str = "considering";

/// Steam no devolvió el nombre del juego.
///
/// Es el único motivo que impide dar de alta un deseado que no está en la
/// biblioteca: la ficha de catálogo necesita un nombre real y ponerle «Steam App
/// 12345» sería inventarlo.
pub const WISHLIST_SKIP_UNRESOLVED_TITLE: &str = "unresolved_title";
/// Se alcanzó [`MAX_WISHLIST_ENTRIES`] antes de llegar a este juego.
pub const WISHLIST_SKIP_LIMIT_REACHED: &str = "limit_reached";

/// Un juego tal y como llega de la lista de deseados de Steam.
///
/// `title` es opcional a propósito: `IWishlistService/GetWishlist` no devuelve
/// el nombre y la consulta que lo resuelve puede fallar para algún juego. Sin
/// nombre real no se rellena nada; se informa y punto.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedWishlistGame {
    pub app_id: u32,
    #[serde(default)]
    pub title: Option<String>,
    /// Cuándo se añadió a la lista **de Steam**, en RFC 3339.
    #[serde(default)]
    pub added_at: Option<String>,
}

/// Un juego de Steam que no llegó a la lista de Vindexa, con su motivo.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedWishlistGame {
    pub app_id: u32,
    /// Nombre publicado por Steam, si se pudo resolver. Sirve para que el aviso
    /// diga qué juego se quedó fuera en vez de un número de AppID a secas.
    pub title: Option<String>,
    /// Uno de [`WISHLIST_SKIP_UNRESOLVED_TITLE`] o [`WISHLIST_SKIP_LIMIT_REACHED`].
    pub reason: String,
}

/// Recuento honesto de una importación.
///
/// No hay un campo «correcto»: `fetched` es lo que Steam devolvió, y la suma de
/// `imported`, `already_present` y `skipped` vuelve a dar `fetched`. Así el
/// aviso puede cuadrar sin que nadie tenga que fiarse de un único número.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistImportReport {
    /// Juegos distintos que venían en la lista de Steam.
    pub fetched: usize,
    /// Juegos nuevos añadidos a la lista de Vindexa.
    pub imported: usize,
    /// Juegos que ya estaban en la lista y no se han tocado.
    pub already_present: usize,
    /// Juegos que se quedaron fuera, con su motivo.
    pub skipped: Vec<SkippedWishlistGame>,
    /// La lista alcanzó [`MAX_WISHLIST_ENTRIES`] durante la importación.
    pub limit_reached: bool,
}

/// Resultado de importar la lista de deseados de Steam.
///
/// Separa lo que entró en la base de datos de lo que sólo sabe la red: sin esa
/// separación no se podría decir «Steam devolvió cero juegos y no dice si tu
/// lista está vacía o escondida», que es la única frase honesta en ese caso.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamWishlistImportResult {
    pub report: WishlistImportReport,
    /// Juegos cuyo nombre no pudo resolverse en la tienda.
    pub titles_unresolved: usize,
    /// Steam devolvió la lista vacía sin decir si lo está o está oculta.
    pub visibility_unknown: bool,
}

/// Vuelca en `wishlist_entries` los juegos de la lista de deseados de Steam.
///
/// # Qué gana cada conflicto
///
/// **Lo escrito a mano, siempre.** Si el juego ya está en la lista, la
/// importación no lo toca: ni el cubo, ni la nota, ni el precio objetivo, ni la
/// prioridad, ni la posición. La razón no es prudencia genérica, es que Steam no
/// tiene con qué pisarlos: su lista no conoce cubos, ni notas, ni precios
/// objetivo, y su `priority` es un orden interno de su propia lista —cero en la
/// mayoría de cuentas— que no significa lo mismo que la prioridad de Vindexa.
/// Sobrescribir con eso sería degradar información real a ruido.
///
/// De Steam sólo se acepta un dato para los juegos **nuevos**: `added_at`, la
/// fecha en la que se añadió allí. Es un hecho comprobable y conserva la
/// cronología real de la lista; si no viene o no es una marca válida, la fila
/// se queda con la fecha de la importación en vez de inventar una.
///
/// Por eso la operación es idempotente: repetirla no duplica filas —`app_id` es
/// clave primaria— ni modifica nada de lo que ya existía.
///
/// # Dónde aterriza cada juego
///
/// Los que ya están en la biblioteca van a `wishlist_entries`, como siempre. Los
/// que no —la mayor parte de cualquier lista de deseados— entran en el catálogo
/// (`catalog_games` + `catalog_wishlist_entries`), fuera de `games`, de modo que
/// no aparecen ni en la biblioteca ni en sus recuentos.
///
/// Lo único que puede dejar fuera a un juego que no tienes es que Steam no haya
/// devuelto su nombre: sin él no hay ficha honesta que crear, y se informa con
/// [`WISHLIST_SKIP_UNRESOLVED_TITLE`].
pub fn import_steam_wishlist(
    connection: &mut Connection,
    games: &[ImportedWishlistGame],
) -> AppResult<WishlistImportReport> {
    let ordered = order_wishlist_import(games);
    let mut report = WishlistImportReport {
        fetched: ordered.len(),
        imported: 0,
        already_present: 0,
        skipped: Vec::new(),
        limit_reached: false,
    };

    let transaction = connection.transaction()?;
    let mut total = wishlist_total(&transaction)?;
    let mut position = bucket_order(&transaction, WISHLIST_IMPORT_BUCKET)?.len() as i64;
    let mut insert_library = transaction.prepare_cached(
        "INSERT INTO wishlist_entries(app_id, bucket, priority, position, note, added_at)
         VALUES (?1, ?2, 0, ?3, '', COALESCE(?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
    )?;
    let mut insert_catalog = transaction.prepare_cached(
        "INSERT INTO catalog_wishlist_entries(app_id, bucket, priority, position, note, added_at)
         VALUES (?1, ?2, 0, ?3, '', COALESCE(?4, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
    )?;

    for game in &ordered {
        if current_bucket(&transaction, game.app_id)?.is_some() {
            report.already_present += 1;
            continue;
        }
        let in_library = catalog::in_library(&transaction, game.app_id)?;
        // Sin nombre no hay ficha de catálogo posible. En la biblioteca el
        // nombre ya está guardado, así que allí da igual que Steam no lo mande.
        let title = game.title.as_deref().map(str::trim).filter(|value| {
            !value.is_empty() && value.chars().count() <= catalog::MAX_CATALOG_TITLE_LENGTH
        });
        if !in_library && title.is_none() {
            report.skipped.push(SkippedWishlistGame {
                app_id: game.app_id,
                title: game.title.clone(),
                reason: WISHLIST_SKIP_UNRESOLVED_TITLE.to_string(),
            });
            continue;
        }
        if usize::try_from(total).unwrap_or(usize::MAX) >= MAX_WISHLIST_ENTRIES {
            report.limit_reached = true;
            report.skipped.push(SkippedWishlistGame {
                app_id: game.app_id,
                title: game.title.clone(),
                reason: WISHLIST_SKIP_LIMIT_REACHED.to_string(),
            });
            continue;
        }

        let arguments = params![
            game.app_id,
            WISHLIST_IMPORT_BUCKET,
            position,
            normalize_wishlist_added_at(game.added_at.as_deref()),
        ];
        if in_library {
            insert_library.execute(arguments)?;
        } else {
            catalog::upsert_catalog_game(
                &transaction,
                game.app_id,
                title.unwrap_or_default(),
                "steam_wishlist",
            )?;
            insert_catalog.execute(arguments)?;
        }
        position += 1;
        total += 1;
        report.imported += 1;
    }

    drop(insert_library);
    drop(insert_catalog);
    transaction.commit()?;
    Ok(report)
}

/// Deja la entrada de Steam en un orden reproducible: primero lo más antiguo,
/// sin AppIDs repetidos ni AppID cero. Dos importaciones de la misma lista
/// colocan los juegos nuevos exactamente en el mismo sitio.
fn order_wishlist_import(games: &[ImportedWishlistGame]) -> Vec<ImportedWishlistGame> {
    let mut ordered = games
        .iter()
        .filter(|game| game.app_id > 0)
        .cloned()
        .collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.added_at
            .cmp(&right.added_at)
            .then(left.app_id.cmp(&right.app_id))
    });
    let mut seen = HashSet::new();
    ordered.retain(|game| seen.insert(game.app_id));
    ordered
}

/// Normaliza la fecha de alta de Steam al mismo formato que escribe la columna.
///
/// Una marca ilegible no detiene la importación ni se corrige a ojo: devuelve
/// `None` y la fila se queda con la fecha de la importación.
fn normalize_wishlist_added_at(value: Option<&str>) -> Option<String> {
    let raw = value.map(str::trim).filter(|value| !value.is_empty())?;
    let moment = DateTime::parse_from_rfc3339(raw).ok()?;
    Some(
        moment
            .with_timezone(&Utc)
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrations, seed_defaults};

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar valores por defecto");
        connection
    }

    fn insert_game(connection: &Connection, app_id: u32) {
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                params![app_id, format!("Juego {app_id}")],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'unclassified')",
                [app_id],
            )
            .expect("insertar ficha personal");
    }

    fn wish(app_id: u32, bucket: &str) -> SaveWishlistEntryInput {
        SaveWishlistEntryInput {
            app_id,
            bucket: bucket.to_string(),
            ..SaveWishlistEntryInput::default()
        }
    }

    fn bucket_ids(overview: &WishlistOverview, bucket: &str) -> Vec<u32> {
        overview
            .buckets
            .iter()
            .find(|candidate| candidate.bucket == bucket)
            .map(|candidate| {
                candidate
                    .items
                    .iter()
                    .map(|entry| entry.game.app_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Deseados
    // -----------------------------------------------------------------------

    #[test]
    fn rejects_invalid_buckets_priorities_and_prices() {
        let mut connection = database();
        insert_game(&connection, 10);

        let bucket = save_wishlist_entry(&mut connection, &wish(10, "algún_día"))
            .expect_err("rechazar cubo inventado");
        assert_eq!(bucket.code, "validation");
        assert!(bucket.message.contains("waiting_sale"));

        let mut input = wish(10, "considering");
        input.priority = MAX_PRIORITY + 1;
        let priority =
            save_wishlist_entry(&mut connection, &input).expect_err("rechazar prioridad alta");
        assert_eq!(priority.code, "validation");

        let mut input = wish(10, "considering");
        input.target_price_cents = Some(1999);
        let without_currency =
            save_wishlist_entry(&mut connection, &input).expect_err("rechazar importe sin moneda");
        assert_eq!(without_currency.code, "validation");

        let mut input = wish(10, "considering");
        input.currency = Some("EUR".to_string());
        let without_price =
            save_wishlist_entry(&mut connection, &input).expect_err("rechazar moneda sin importe");
        assert_eq!(without_price.code, "validation");

        let mut input = wish(10, "considering");
        input.target_price_cents = Some(1999);
        input.currency = Some("euros".to_string());
        let bad_currency =
            save_wishlist_entry(&mut connection, &input).expect_err("rechazar moneda no ISO");
        assert_eq!(bad_currency.code, "validation");

        let mut input = wish(10, "considering");
        input.target_price_cents = Some(-1);
        input.currency = Some("EUR".to_string());
        let negative =
            save_wishlist_entry(&mut connection, &input).expect_err("rechazar importe negativo");
        assert_eq!(negative.code, "validation");

        let mut input = wish(10, "considering");
        input.note = "n".repeat(501);
        let note = save_wishlist_entry(&mut connection, &input).expect_err("rechazar nota larga");
        assert_eq!(note.code, "validation");

        let unknown = save_wishlist_entry(&mut connection, &wish(999, "considering"))
            .expect_err("rechazar juego desconocido");
        assert_eq!(unknown.code, "not_found");
    }

    #[test]
    fn normalizes_the_currency_and_keeps_the_entry_readable() {
        let mut connection = database();
        insert_game(&connection, 10);
        let mut input = wish(10, "waiting_sale");
        input.priority = 4;
        input.note = "  Esperar rebajas de verano  ".to_string();
        input.target_price_cents = Some(1999);
        input.currency = Some(" eur ".to_string());

        let entry = save_wishlist_entry(&mut connection, &input).expect("guardar deseado");
        assert_eq!(entry.bucket, "waiting_sale");
        assert_eq!(entry.priority, 4);
        assert_eq!(entry.note, "Esperar rebajas de verano");
        assert_eq!(entry.currency.as_deref(), Some("EUR"));
        assert_eq!(entry.game.title, "Juego 10");

        assert_eq!(
            wishlist_entry(&connection, 10).unwrap().target_price_cents,
            Some(1999)
        );
        assert_eq!(
            wishlist_entry(&connection, 999).unwrap_err().code,
            "not_found"
        );
    }

    #[test]
    fn aggregates_target_prices_per_currency_and_reports_unknowns_apart() {
        let mut connection = database();
        for app_id in [10, 20, 30, 40] {
            insert_game(&connection, app_id);
        }
        let priced = |app_id: u32, cents: i64, currency: &str| SaveWishlistEntryInput {
            app_id,
            bucket: "buying_now".to_string(),
            target_price_cents: Some(cents),
            currency: Some(currency.to_string()),
            ..SaveWishlistEntryInput::default()
        };
        save_wishlist_entry(&mut connection, &priced(10, 1999, "EUR")).expect("guardar 10");
        save_wishlist_entry(&mut connection, &priced(20, 1000, "EUR")).expect("guardar 20");
        save_wishlist_entry(&mut connection, &priced(30, 2500, "USD")).expect("guardar 30");
        save_wishlist_entry(&mut connection, &wish(40, "watching")).expect("guardar 40 sin precio");

        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(overview.total, 4);
        assert_eq!(overview.entries_without_target, 1);
        assert_eq!(overview.target_totals.len(), 2);
        assert_eq!(overview.target_totals[0].currency, "EUR");
        assert_eq!(overview.target_totals[0].total_cents, 2999);
        assert_eq!(overview.target_totals[0].entries, 2);
        assert_eq!(overview.target_totals[1].currency, "USD");
        assert_eq!(overview.target_totals[1].total_cents, 2500);
        assert_eq!(overview.target_totals[1].entries, 1);

        assert_eq!(
            overview
                .buckets
                .iter()
                .map(|bucket| bucket.bucket.as_str())
                .collect::<Vec<_>>(),
            WISHLIST_BUCKETS.to_vec()
        );
    }

    #[test]
    fn orders_each_bucket_by_priority_then_manual_position() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        save_wishlist_entry(&mut connection, &wish(10, "considering")).expect("guardar 10");
        save_wishlist_entry(&mut connection, &wish(20, "considering")).expect("guardar 20");
        let mut urgent = wish(30, "considering");
        urgent.priority = 5;
        save_wishlist_entry(&mut connection, &urgent).expect("guardar 30");

        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(bucket_ids(&overview, "considering"), vec![30, 10, 20]);
    }

    #[test]
    fn editing_an_entry_does_not_move_it_inside_its_bucket() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        for app_id in [10, 20, 30] {
            save_wishlist_entry(&mut connection, &wish(app_id, "considering")).expect("guardar");
        }

        let mut edit = wish(10, "considering");
        edit.note = "Mirar análisis antes".to_string();
        save_wishlist_entry(&mut connection, &edit).expect("editar la nota");
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![10, 20, 30],
            "editar no debe mandar el juego al final del cubo"
        );

        let mut anchored = wish(10, "considering");
        anchored.before_app_id = Some(30);
        save_wishlist_entry(&mut connection, &anchored).expect("recolocar con ancla");
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![20, 10, 30]
        );
    }

    #[test]
    fn moves_entries_inside_and_between_buckets() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        for app_id in [10, 20, 30] {
            save_wishlist_entry(&mut connection, &wish(app_id, "considering"))
                .expect("guardar deseado");
        }
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![10, 20, 30]
        );

        move_wishlist_entry(&mut connection, 30, "considering", Some(10))
            .expect("mover al principio");
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![30, 10, 20]
        );

        move_wishlist_entry(&mut connection, 30, "considering", None).expect("mover al final");
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![10, 20, 30]
        );

        move_wishlist_entry(&mut connection, 20, "buying_now", None).expect("cambiar de cubo");
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![10, 30]
        );
        assert_eq!(bucket_order(&connection, "buying_now").unwrap(), vec![20]);
        let positions: Vec<i64> = connection
            .prepare(
                "SELECT position FROM wishlist_entries WHERE bucket = 'considering' ORDER BY position",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(positions, vec![0, 1], "el cubo de origen queda compactado");

        assert_eq!(
            move_wishlist_entry(&mut connection, 10, "considering", Some(10))
                .expect_err("rechazar moverse ante sí mismo")
                .code,
            "validation"
        );
        assert_eq!(
            move_wishlist_entry(&mut connection, 10, "considering", Some(999))
                .expect_err("rechazar ancla ausente")
                .code,
            "not_found"
        );
        assert_eq!(
            move_wishlist_entry(&mut connection, 999, "considering", None)
                .expect_err("rechazar juego ausente")
                .code,
            "not_found"
        );
        assert_eq!(
            move_wishlist_entry(&mut connection, 10, "inventado", None)
                .expect_err("rechazar cubo inventado")
                .code,
            "validation"
        );
    }

    #[test]
    fn reorders_a_bucket_only_with_its_exact_set() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }

        reorder_wishlist_bucket(&mut connection, "watching", &[]).expect("reordenar cubo vacío");

        save_wishlist_entry(&mut connection, &wish(10, "watching")).expect("guardar 10");
        reorder_wishlist_bucket(&mut connection, "watching", &[10]).expect("reordenar uno solo");

        for app_id in [20, 30] {
            save_wishlist_entry(&mut connection, &wish(app_id, "watching")).expect("guardar");
        }
        reorder_wishlist_bucket(&mut connection, "watching", &[30, 10, 20]).expect("reordenar");
        assert_eq!(
            bucket_order(&connection, "watching").unwrap(),
            vec![30, 10, 20]
        );

        assert_eq!(
            reorder_wishlist_bucket(&mut connection, "watching", &[30, 10])
                .expect_err("rechazar orden parcial")
                .code,
            "validation"
        );
        assert_eq!(
            reorder_wishlist_bucket(&mut connection, "watching", &[10, 10, 20])
                .expect_err("rechazar repetidos")
                .code,
            "validation"
        );
        assert_eq!(
            bucket_order(&connection, "watching").unwrap(),
            vec![30, 10, 20],
            "un orden rechazado no altera el cubo"
        );
    }

    #[test]
    fn removing_and_deleting_games_keeps_the_wishlist_consistent() {
        let mut connection = database();
        for app_id in [10, 20, 30] {
            insert_game(&connection, app_id);
        }
        for app_id in [10, 20, 30] {
            save_wishlist_entry(&mut connection, &wish(app_id, "considering")).expect("guardar");
        }

        remove_wishlist_entry(&mut connection, 10).expect("quitar el primero");
        assert_eq!(
            bucket_order(&connection, "considering").unwrap(),
            vec![20, 30]
        );
        assert_eq!(
            remove_wishlist_entry(&mut connection, 10)
                .expect_err("rechazar borrado repetido")
                .code,
            "not_found"
        );

        connection
            .execute("DELETE FROM games WHERE app_id = 20", [])
            .expect("borrar juego de la biblioteca");
        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(overview.total, 1);
        assert_eq!(bucket_ids(&overview, "considering"), vec![30]);
    }

    // -----------------------------------------------------------------------
    // Vídeos
    // -----------------------------------------------------------------------

    #[test]
    fn accepts_every_documented_youtube_shape() {
        for input in [
            "dQw4w9WgXcQ",
            "  dQw4w9WgXcQ  ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42s&list=PL123",
            "http://youtube.com/watch?v=dQw4w9WgXcQ",
            "youtube.com/watch?v=dQw4w9WgXcQ",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?si=AbCdEf",
            "youtu.be/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
            "https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/live/dQw4w9WgXcQ",
            "https://www.youtube.com/v/dQw4w9WgXcQ",
            "HTTPS://WWW.YOUTUBE.COM/watch?v=dQw4w9WgXcQ",
            // Relativa al protocolo: el host sigue siendo `youtube.com` y el
            // identificador se valida igual, así que aceptarla no amplía nada.
            "//youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(
                parse_youtube_video_id(input)
                    .unwrap_or_else(|error| panic!("debería aceptar {input}: {}", error.message)),
                "dQw4w9WgXcQ"
            );
        }
        assert_eq!(
            parse_youtube_video_id("_-aB3cD4eF5").unwrap(),
            "_-aB3cD4eF5"
        );
    }

    #[test]
    fn rejects_hostile_and_malformed_video_references() {
        for input in [
            "",
            "   ",
            "../",
            "../../etc/passwd",
            "../../../../etc/passwd",
            "\"><script>alert(1)</script>",
            "<iframe src=x onerror=alert(1)>",
            "javascript:alert(1)",
            "javascript:alert('dQw4w9WgXcQ')",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "vbscript:msgbox(1)",
            "dQw4w9WgXc",
            "dQw4w9WgXcQQ",
            "dQw4w9WgX Q",
            "dQw4w9WgX/Q",
            "dQw4w9WgX.Q",
            "dQw4w9WgX?Q",
            "https://evil.com/embed/dQw4w9WgXcQ",
            "https://youtube.com.evil.com/watch?v=dQw4w9WgXcQ",
            "https://evil.com/watch?v=dQw4w9WgXcQ",
            "https://notyoutube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=../../secret",
            "https://www.youtube.com/watch?v=%2E%2E%2F%2E%2E",
            "https://www.youtube.com/embed/%2E%2E",
            "https://www.youtube.com/watch",
            "https://www.youtube.com/",
            "https://www.youtube.com/results?search_query=algo",
            "https://www.youtube.com/embed/",
            "https://youtu.be/",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ\"><script>",
            // Confusión de autoridad: el host real es `evil.com`, no YouTube.
            "https://youtube.com@evil.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com@evil.com/embed/dQw4w9WgXcQ",
            "https://usuario:clave@evil.com/watch?v=dQw4w9WgXcQ",
            "https://evil.com/?u=https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://evil.com#https://youtu.be/dQw4w9WgXcQ",
        ] {
            let error = parse_youtube_video_id(input)
                .err()
                .unwrap_or_else(|| panic!("debería rechazar {input:?}"));
            assert_eq!(error.code, "validation", "entrada {input:?}");
        }

        let too_long = format!("https://youtu.be/{}", "a".repeat(MAX_URL_LENGTH));
        assert_eq!(
            parse_youtube_video_id(&too_long)
                .expect_err("rechazar URL desmesurada")
                .code,
            "validation"
        );
    }

    #[test]
    fn every_accepted_identifier_is_safe_inside_the_embed_url() {
        // Propiedad que sostiene la frontera: pase lo que pase por el
        // validador, lo que sale sólo puede contener caracteres seguros en una
        // ruta, así que la URL del iframe no admite inyección.
        for input in [
            "dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=_-aB3cD4eF5",
            "https://youtu.be/00000000000",
            "https://www.youtube.com/shorts/ZZZZZZZZZZZ",
        ] {
            let id = parse_youtube_video_id(input).expect("aceptar identificador válido");
            assert_eq!(id.len(), YOUTUBE_ID_LENGTH);
            assert!(
                id.bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'),
                "identificador con caracteres inesperados: {id}"
            );
            let embed = youtube_embed_url(&id).expect("construir la URL de incrustación");
            assert_eq!(
                embed,
                format!("https://www.youtube-nocookie.com/embed/{id}")
            );
            let parsed = Url::parse(&embed).expect("la URL de incrustación debe ser válida");
            assert_eq!(parsed.host_str(), Some("www.youtube-nocookie.com"));
            assert_eq!(parsed.query(), None);
            assert_eq!(parsed.path(), format!("/embed/{id}"));
        }
    }

    #[test]
    fn steam_video_identifiers_must_be_numeric() {
        assert_eq!(
            parse_video_reference("steam", " 256658589 ").unwrap(),
            "256658589"
        );
        for input in ["", "abc", "256658589a", "../256658589", "-1"] {
            assert_eq!(
                parse_video_reference("steam", input)
                    .expect_err("rechazar identificador de Steam no numérico")
                    .code,
                "validation"
            );
        }
        assert_eq!(
            parse_video_reference("vimeo", "12345")
                .expect_err("rechazar proveedor desconocido")
                .code,
            "validation"
        );
    }

    #[test]
    fn saves_videos_with_a_backend_built_embed_url() {
        let mut connection = database();
        insert_game(&connection, 10);

        let video = save_game_video(
            &mut connection,
            &SaveGameVideoInput {
                app_id: 10,
                video: "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42".to_string(),
                provider: "youtube".to_string(),
                kind: "gameplay".to_string(),
                title: "  Una hora de partida  ".to_string(),
                channel: "  Canal  ".to_string(),
                duration_seconds: Some(3600),
                published_at: Some("2026-03-04".to_string()),
                thumbnail_url: None,
                source: "manual".to_string(),
            },
        )
        .expect("guardar vídeo");

        assert_eq!(video.video_id, "dQw4w9WgXcQ");
        assert_eq!(video.title, "Una hora de partida");
        assert_eq!(video.channel, "Canal");
        assert_eq!(video.published_at.as_deref(), Some("2026-03-04"));
        assert_eq!(
            video.embed_url.as_deref(),
            Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ")
        );

        let steam = save_game_video(
            &mut connection,
            &SaveGameVideoInput {
                app_id: 10,
                video: "256658589".to_string(),
                provider: "steam".to_string(),
                kind: "trailer".to_string(),
                source: "store".to_string(),
                ..SaveGameVideoInput::default()
            },
        )
        .expect("guardar vídeo de Steam");
        assert!(
            steam.embed_url.is_none(),
            "un vídeo de Steam no se incrusta en YouTube"
        );
        assert_eq!(steam.source, "store");
    }

    #[test]
    fn rejects_video_metadata_that_does_not_hold_up() {
        let mut connection = database();
        insert_game(&connection, 10);
        let base = |video: &str| SaveGameVideoInput {
            app_id: 10,
            video: video.to_string(),
            ..SaveGameVideoInput::default()
        };

        assert_eq!(
            save_game_video(&mut connection, &base("javascript:alert(1)"))
                .expect_err("rechazar identificador hostil")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.kind = "resumen".to_string();
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar tipo inventado")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.source = "scraper".to_string();
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar origen inventado")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.duration_seconds = Some(MAX_DURATION_SECONDS + 1);
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar duración imposible")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.published_at = Some("04/03/2026".to_string());
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar fecha no ISO")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.published_at = Some("2026-02-31".to_string());
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar fecha inexistente")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.title = "t".repeat(MAX_VIDEO_TITLE_LENGTH + 1);
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar título largo")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.thumbnail_url = Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hq.jpg".to_string());
        let thumbnail = save_game_video(&mut connection, &input)
            .expect_err("rechazar miniatura alojada en Google");
        assert_eq!(thumbnail.code, "validation");
        assert!(thumbnail.message.contains("CDN oficial de Steam"));

        let mut input = base("dQw4w9WgXcQ");
        input.thumbnail_url = Some("http://shared.steamstatic.com/x.jpg".to_string());
        assert_eq!(
            save_game_video(&mut connection, &input)
                .expect_err("rechazar miniatura sin HTTPS")
                .code,
            "validation"
        );

        let mut input = base("dQw4w9WgXcQ");
        input.thumbnail_url =
            Some("https://shared.steamstatic.com/store_item_assets/x.jpg".to_string());
        save_game_video(&mut connection, &input).expect("aceptar miniatura del CDN de Steam");

        let unknown_game = SaveGameVideoInput {
            app_id: 999,
            video: "dQw4w9WgXcQ".to_string(),
            ..SaveGameVideoInput::default()
        };
        assert_eq!(
            save_game_video(&mut connection, &unknown_game)
                .expect_err("rechazar juego desconocido")
                .code,
            "not_found"
        );
    }

    #[test]
    fn lists_filters_reorders_and_deletes_videos() {
        let mut connection = database();
        insert_game(&connection, 10);
        let ids: [String; 3] = ["a", "b", "c"].map(|letter| letter.repeat(YOUTUBE_ID_LENGTH));
        for id in &ids {
            save_game_video(
                &mut connection,
                &SaveGameVideoInput {
                    app_id: 10,
                    video: id.clone(),
                    ..SaveGameVideoInput::default()
                },
            )
            .expect("guardar vídeo de gameplay");
        }
        save_game_video(
            &mut connection,
            &SaveGameVideoInput {
                app_id: 10,
                video: "d".repeat(YOUTUBE_ID_LENGTH),
                kind: "impressions".to_string(),
                ..SaveGameVideoInput::default()
            },
        )
        .expect("guardar impresiones");

        assert_eq!(list_game_videos(&connection, 10, None).unwrap().len(), 4);
        let gameplay = list_game_videos(&connection, 10, Some("gameplay")).unwrap();
        assert_eq!(
            gameplay
                .iter()
                .map(|video| video.video_id.clone())
                .collect::<Vec<_>>(),
            ids.to_vec()
        );

        assert_eq!(
            list_game_videos(&connection, 10, Some("guide"))
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            list_game_videos(&connection, 10, Some("inventado"))
                .expect_err("rechazar tipo inventado")
                .code,
            "validation"
        );

        let reference = |video_id: &str| GameVideoRef {
            provider: "youtube".to_string(),
            video_id: video_id.to_string(),
        };
        reorder_game_videos(
            &mut connection,
            10,
            "gameplay",
            &[reference(&ids[2]), reference(&ids[0]), reference(&ids[1])],
        )
        .expect("reordenar");
        assert_eq!(
            list_game_videos(&connection, 10, Some("gameplay"))
                .unwrap()
                .iter()
                .map(|video| video.video_id.clone())
                .collect::<Vec<_>>(),
            vec![ids[2].clone(), ids[0].clone(), ids[1].clone()]
        );

        assert_eq!(
            reorder_game_videos(&mut connection, 10, "gameplay", &[reference(&ids[0])])
                .expect_err("rechazar orden parcial")
                .code,
            "validation"
        );
        assert_eq!(
            reorder_game_videos(
                &mut connection,
                10,
                "gameplay",
                &[reference(&ids[0]), reference(&ids[0]), reference(&ids[1]),]
            )
            .expect_err("rechazar repetidos")
            .code,
            "validation"
        );
        reorder_game_videos(&mut connection, 10, "guide", &[]).expect("reordenar tipo vacío");

        delete_game_video(&mut connection, 10, "youtube", &ids[0]).expect("borrar vídeo");
        let remaining = list_game_videos(&connection, 10, Some("gameplay")).unwrap();
        assert_eq!(
            remaining
                .iter()
                .map(|video| (video.video_id.clone(), video.position))
                .collect::<Vec<_>>(),
            vec![(ids[2].clone(), 0), (ids[1].clone(), 1)]
        );
        assert_eq!(
            delete_game_video(&mut connection, 10, "youtube", &ids[0])
                .expect_err("rechazar borrado repetido")
                .code,
            "not_found"
        );

        connection
            .execute("DELETE FROM games WHERE app_id = 10", [])
            .expect("borrar juego");
        assert_eq!(list_game_videos(&connection, 10, None).unwrap().len(), 0);
    }

    #[test]
    fn saving_the_same_video_twice_updates_it_without_duplicating() {
        let mut connection = database();
        insert_game(&connection, 10);
        let mut input = SaveGameVideoInput {
            app_id: 10,
            video: "https://youtu.be/dQw4w9WgXcQ".to_string(),
            title: "Primera versión".to_string(),
            ..SaveGameVideoInput::default()
        };
        save_game_video(&mut connection, &input).expect("guardar");
        input.video = "dQw4w9WgXcQ".to_string();
        input.title = "Segunda versión".to_string();
        let updated = save_game_video(&mut connection, &input).expect("actualizar");

        assert_eq!(updated.title, "Segunda versión");
        assert_eq!(list_game_videos(&connection, 10, None).unwrap().len(), 1);
    }

    // -----------------------------------------------------------------------
    // Importación desde Steam
    // -----------------------------------------------------------------------

    fn steam(app_id: u32, title: &str, added_at: Option<&str>) -> ImportedWishlistGame {
        ImportedWishlistGame {
            app_id,
            title: Some(title.to_string()),
            added_at: added_at.map(str::to_string),
        }
    }

    fn stored_entry(
        connection: &Connection,
        app_id: u32,
    ) -> (String, u8, String, Option<i64>, String) {
        connection
            .query_row(
                "SELECT bucket, priority, note, target_price_cents, added_at
                   FROM wishlist_entries WHERE app_id = ?1",
                [app_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .expect("leer entrada guardada")
    }

    #[test]
    fn importing_a_wishlist_keeps_the_steam_chronology_and_reports_what_it_skipped() {
        let mut connection = database();
        insert_game(&connection, 10);
        insert_game(&connection, 20);

        let report = import_steam_wishlist(
            &mut connection,
            &[
                steam(20, "Segundo", Some("2024-02-23T22:56:12+00:00")),
                steam(10, "Primero", Some("2013-12-16T17:34:30+00:00")),
                ImportedWishlistGame {
                    app_id: 999,
                    title: None,
                    added_at: Some("2020-01-01T00:00:00+00:00".to_string()),
                },
            ],
        )
        .expect("importar");

        assert_eq!(report.fetched, 3);
        assert_eq!(report.imported, 2);
        assert_eq!(report.already_present, 0);
        assert_eq!(report.skipped.len(), 1);
        assert_eq!(report.skipped[0].app_id, 999);
        assert_eq!(report.skipped[0].reason, WISHLIST_SKIP_UNRESOLVED_TITLE);
        assert_eq!(report.skipped[0].title, None);
        assert!(!report.limit_reached);
        assert_eq!(
            report.imported + report.already_present + report.skipped.len(),
            report.fetched
        );

        // El más antiguo en Steam entra primero, no el que llegó primero.
        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(bucket_ids(&overview, WISHLIST_IMPORT_BUCKET), vec![10, 20]);

        let (bucket, priority, note, target, added_at) = stored_entry(&connection, 10);
        assert_eq!(bucket, WISHLIST_IMPORT_BUCKET);
        assert_eq!(priority, 0);
        assert_eq!(note, "");
        assert_eq!(target, None);
        assert_eq!(added_at, "2013-12-16T17:34:30.000Z");
    }

    // -----------------------------------------------------------------------
    // Catálogo: juegos deseados que no están en la biblioteca
    // -----------------------------------------------------------------------

    fn catalog_count(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM catalog_games", [], |row| row.get(0))
            .expect("contar el catálogo")
    }

    /// Cuenta lo que ve la biblioteca por todos los caminos que podrían
    /// enseñarla: la tabla, el recuento del resumen, la búsqueda de texto
    /// completo y la lista paginada.
    fn library_sightings(connection: &Connection, app_id: u32) -> (i64, i64, i64, usize) {
        let in_games: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM games WHERE app_id = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("contar en games");
        let stats = crate::db::library::library_stats(connection).expect("estadísticas");
        let in_search: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM game_search WHERE rowid = ?1",
                [app_id],
                |row| row.get(0),
            )
            .expect("contar en la búsqueda");
        let listed = crate::db::library::list_games(connection, &Default::default(), None)
            .expect("listar biblioteca")
            .items
            .len();
        (in_games, stats.total_games, in_search, listed)
    }

    #[test]
    fn a_game_you_do_not_own_enters_the_wishlist_without_entering_the_library() {
        let mut connection = database();
        insert_game(&connection, 10);

        let report = import_steam_wishlist(
            &mut connection,
            &[
                steam(10, "Ya lo tengo", Some("2013-12-16T17:34:30+00:00")),
                steam(999, "Silksong", Some("2020-01-01T00:00:00+00:00")),
            ],
        )
        .expect("importar");

        assert_eq!(report.imported, 2);
        assert!(report.skipped.is_empty());

        // Está en los deseados, con su nombre y su arte derivada del AppID.
        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(bucket_ids(&overview, WISHLIST_IMPORT_BUCKET), vec![10, 999]);
        assert_eq!(overview.total, 2);
        let entry = wishlist_entry(&connection, 999).expect("entrada del catálogo");
        assert_eq!(entry.game.title, "Silksong");
        assert!(!entry.game.in_library);
        assert!(entry.game.library.is_none());
        assert_eq!(
            entry.game.cover_url.as_deref(),
            Some(crate::steam::local::cover_url(999).as_str())
        );

        // Y no está en la biblioteca por ninguna de sus puertas.
        let (in_games, total_games, in_search, listed) = library_sightings(&connection, 999);
        assert_eq!(in_games, 0);
        assert_eq!(total_games, 1);
        assert_eq!(in_search, 0);
        assert_eq!(listed, 1);
    }

    #[test]
    fn buying_the_game_moves_the_wish_into_the_library_without_losing_it() {
        let mut connection = database();
        import_steam_wishlist(
            &mut connection,
            &[steam(999, "Silksong", Some("2020-01-01T00:00:00+00:00"))],
        )
        .expect("importar");

        let mut plan = wish(999, "waiting_sale");
        plan.note = "Esperar a que baje de 20 €".to_string();
        plan.priority = 4;
        plan.target_price_cents = Some(1_999);
        plan.currency = Some("EUR".to_string());
        save_wishlist_entry(&mut connection, &plan).expect("planear la compra");

        // Lo compro: la sincronización de Steam lo trae como propio.
        insert_game(&connection, 999);

        let entry = wishlist_entry(&connection, 999).expect("entrada tras la compra");
        assert!(entry.game.in_library);
        assert!(entry.game.library.is_some());
        assert_eq!(entry.bucket, "waiting_sale");
        assert_eq!(entry.note, "Esperar a que baje de 20 €");
        assert_eq!(entry.priority, 4);
        assert_eq!(entry.target_price_cents, Some(1_999));
        assert_eq!(entry.currency.as_deref(), Some("EUR"));
        assert_eq!(entry.added_at, "2020-01-01T00:00:00.000Z");

        // Ni duplicada ni huérfana: una sola fila, y ya no en el catálogo.
        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(overview.total, 1);
        assert_eq!(bucket_ids(&overview, "waiting_sale"), vec![999]);
        assert_eq!(catalog_count(&connection), 0);
        let (_, total_games, in_search, _) = library_sightings(&connection, 999);
        assert_eq!(total_games, 1);
        assert_eq!(in_search, 1);
    }

    #[test]
    fn promotion_appends_the_wish_to_the_tail_of_its_bucket() {
        let mut connection = database();
        insert_game(&connection, 10);
        insert_game(&connection, 20);
        import_steam_wishlist(
            &mut connection,
            &[
                steam(10, "Primero", Some("2013-01-01T00:00:00+00:00")),
                steam(20, "Segundo", Some("2014-01-01T00:00:00+00:00")),
                steam(999, "Comprado después", Some("2015-01-01T00:00:00+00:00")),
            ],
        )
        .expect("importar");

        insert_game(&connection, 999);

        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(
            bucket_ids(&overview, WISHLIST_IMPORT_BUCKET),
            vec![10, 20, 999]
        );
    }

    #[test]
    fn catalog_entries_share_order_and_totals_with_library_entries() {
        let mut connection = database();
        insert_game(&connection, 10);
        import_steam_wishlist(
            &mut connection,
            &[
                steam(10, "De la biblioteca", Some("2013-01-01T00:00:00+00:00")),
                steam(999, "Del catálogo", Some("2014-01-01T00:00:00+00:00")),
            ],
        )
        .expect("importar");

        // Un juego del catálogo se coloca delante de uno de la biblioteca.
        move_wishlist_entry(&mut connection, 999, WISHLIST_IMPORT_BUCKET, Some(10))
            .expect("mover el del catálogo delante");
        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(bucket_ids(&overview, WISHLIST_IMPORT_BUCKET), vec![999, 10]);

        // Y el precio objetivo de un juego que no tienes suma como cualquier otro.
        let mut plan = wish(999, WISHLIST_IMPORT_BUCKET);
        plan.target_price_cents = Some(2_000);
        plan.currency = Some("EUR".to_string());
        save_wishlist_entry(&mut connection, &plan).expect("guardar precio objetivo");
        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(overview.target_totals.len(), 1);
        assert_eq!(overview.target_totals[0].currency, "EUR");
        assert_eq!(overview.target_totals[0].total_cents, 2_000);
        assert_eq!(overview.entries_without_target, 1);
    }

    #[test]
    fn removing_a_catalog_entry_takes_its_catalog_record_with_it() {
        let mut connection = database();
        import_steam_wishlist(
            &mut connection,
            &[steam(999, "Silksong", Some("2020-01-01T00:00:00+00:00"))],
        )
        .expect("importar");

        remove_wishlist_entry(&mut connection, 999).expect("quitar de deseados");

        assert_eq!(catalog_count(&connection), 0);
        assert_eq!(wishlist_overview(&connection).expect("resumen").total, 0);
        assert_eq!(
            wishlist_entry(&connection, 999)
                .expect_err("ya no está")
                .code,
            "not_found"
        );
    }

    #[test]
    fn reimporting_a_catalog_game_neither_duplicates_it_nor_overwrites_the_plan() {
        let mut connection = database();
        let from_steam = [steam(999, "Silksong", Some("2020-01-01T00:00:00+00:00"))];
        import_steam_wishlist(&mut connection, &from_steam).expect("primera importación");

        let mut plan = wish(999, "buying_now");
        plan.note = "Sale en marzo".to_string();
        save_wishlist_entry(&mut connection, &plan).expect("planear");

        let second = import_steam_wishlist(&mut connection, &from_steam).expect("reimportar");

        assert_eq!(second.imported, 0);
        assert_eq!(second.already_present, 1);
        assert!(second.skipped.is_empty());
        assert_eq!(catalog_count(&connection), 1);
        let entry = wishlist_entry(&connection, 999).expect("entrada");
        assert_eq!(entry.bucket, "buying_now");
        assert_eq!(entry.note, "Sale en marzo");
    }

    #[test]
    fn the_wishlist_limit_counts_both_tables() {
        let mut connection = database();
        import_steam_wishlist(
            &mut connection,
            &[steam(
                999,
                "Del catálogo",
                Some("2020-01-01T00:00:00+00:00"),
            )],
        )
        .expect("importar");
        insert_game(&connection, 10);

        assert_eq!(wishlist_total(&connection).expect("contar"), 1);
        assert_eq!(wishlist_overview(&connection).expect("resumen").total, 1);
        save_wishlist_entry(&mut connection, &wish(10, WISHLIST_IMPORT_BUCKET))
            .expect("añadir uno de biblioteca");
        assert_eq!(wishlist_total(&connection).expect("contar"), 2);
    }

    #[test]
    fn a_wish_over_an_unknown_game_is_refused_instead_of_inventing_a_record() {
        let mut connection = database();
        let refused = save_wishlist_entry(&mut connection, &wish(999, WISHLIST_IMPORT_BUCKET))
            .expect_err("rechazar juego desconocido");
        assert_eq!(refused.code, "not_found");
        assert_eq!(catalog_count(&connection), 0);
    }

    #[test]
    fn reimporting_does_not_duplicate_nor_overwrite_what_was_written_by_hand() {
        let mut connection = database();
        insert_game(&connection, 10);

        let mut edited = wish(10, "buying_now");
        edited.note = "Esperar a que baje de 20 €".to_string();
        edited.priority = 4;
        edited.target_price_cents = Some(1_999);
        edited.currency = Some("EUR".to_string());
        save_wishlist_entry(&mut connection, &edited).expect("guardar a mano");

        let steam_wants_another_bucket = steam(10, "Primero", Some("2013-12-16T17:34:30+00:00"));
        let from_steam = std::slice::from_ref(&steam_wants_another_bucket);
        let first =
            import_steam_wishlist(&mut connection, from_steam).expect("primera importación");
        let second =
            import_steam_wishlist(&mut connection, from_steam).expect("segunda importación");

        assert_eq!(first.imported, 0);
        assert_eq!(first.already_present, 1);
        assert_eq!(second.imported, 0);
        assert_eq!(second.already_present, 1);

        let total: i64 = connection
            .query_row("SELECT COUNT(*) FROM wishlist_entries", [], |row| {
                row.get(0)
            })
            .expect("contar");
        assert_eq!(total, 1);

        // Ni el cubo, ni la prioridad, ni la nota, ni el precio objetivo se
        // mueven: Steam no tiene ninguno de esos datos que aportar.
        let (bucket, priority, note, target, _) = stored_entry(&connection, 10);
        assert_eq!(bucket, "buying_now");
        assert_eq!(priority, 4);
        assert_eq!(note, "Esperar a que baje de 20 €");
        assert_eq!(target, Some(1_999));
    }

    #[test]
    fn an_empty_wishlist_changes_nothing() {
        let mut connection = database();
        insert_game(&connection, 10);
        save_wishlist_entry(&mut connection, &wish(10, "watching")).expect("guardar");

        let report = import_steam_wishlist(&mut connection, &[]).expect("importar vacío");

        assert_eq!(report.fetched, 0);
        assert_eq!(report.imported, 0);
        assert_eq!(report.already_present, 0);
        assert!(report.skipped.is_empty());
        assert_eq!(stored_entry(&connection, 10).0, "watching");
    }

    #[test]
    fn imported_games_are_appended_after_the_ones_already_in_the_bucket() {
        let mut connection = database();
        insert_game(&connection, 10);
        insert_game(&connection, 20);
        save_wishlist_entry(&mut connection, &wish(10, WISHLIST_IMPORT_BUCKET)).expect("guardar");

        import_steam_wishlist(
            &mut connection,
            &[steam(20, "Nuevo", Some("2001-01-01T00:00:00+00:00"))],
        )
        .expect("importar");

        let overview = wishlist_overview(&connection).expect("resumen");
        assert_eq!(bucket_ids(&overview, WISHLIST_IMPORT_BUCKET), vec![10, 20]);
    }

    #[test]
    fn repeated_app_ids_and_unusable_dates_do_not_break_the_import() {
        let mut connection = database();
        insert_game(&connection, 10);

        let report = import_steam_wishlist(
            &mut connection,
            &[
                steam(10, "Repetido", Some("2020-01-01T00:00:00+00:00")),
                steam(10, "Repetido", Some("2024-01-01T00:00:00+00:00")),
                ImportedWishlistGame {
                    app_id: 0,
                    title: None,
                    added_at: None,
                },
            ],
        )
        .expect("importar");

        assert_eq!(report.fetched, 1);
        assert_eq!(report.imported, 1);
        assert_eq!(stored_entry(&connection, 10).4, "2020-01-01T00:00:00.000Z");
        assert_eq!(normalize_wishlist_added_at(Some("ayer")), None);
        assert_eq!(normalize_wishlist_added_at(Some("  ")), None);
    }
}
