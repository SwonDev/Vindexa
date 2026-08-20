//! Índice oficial de recursos de arte de la tienda de Steam.
//!
//! # Por qué existe
//!
//! Vindexa derivaba las URL de arte por convención, con el AppID como única
//! entrada: `/store_item_assets/steam/apps/<id>/library_600x900_2x.jpg`. Esa
//! ruta sólo existe para una parte del catálogo. Medido contra la biblioteca
//! real de la persona usuaria —1815 juegos, 18-08-2026— **445 carátulas
//! apuntaban a un archivo que devuelve 404**. Al fallar, la escalera de
//! [`crate::art_cache`] bajaba hasta la cabecera apaisada y la recortaba a
//! proporción de cartel: de ahí los títulos partidos por la mitad.
//!
//! Los motivos son dos y ninguno se puede adivinar desde el AppID:
//!
//! 1. **Ruta con hash de contenido.** Los juegos publicados en los últimos años
//!    guardan cada archivo bajo su propio SHA-1
//!    (`/apps/3483510/c45a0dcc…/library_capsule_2x.jpg`). El hash es *por
//!    archivo*: el del `header` no sirve para la carátula.
//! 2. **Nombre distinto.** Conviven al menos cuatro familias:
//!    `library_600x900_2x.jpg` (1458 juegos), `library_capsule_2x.jpg` (146),
//!    `portrait.png` (152, sólo en su variante sencilla) y las localizadas
//!    (`library_600x900_spanish_2x.jpg`).
//!
//! `IStoreBrowseService/GetItems/v1` con `data_request.include_assets` publica
//! el nombre real de cada archivo y la plantilla de ruta que le corresponde. Es
//! el mismo servicio —y aquí, el mismo cliente HTTP, la misma espera mínima
//! entre peticiones, el mismo tope de cuerpo y la misma traducción de `429`—
//! que [`super::wishlist`] ya usa para resolver títulos. No pide clave de API.
//!
//! # Qué no hace
//!
//! **Un juego sin arte publicada se queda sin arte.** Cuando el índice responde
//! `success != 1` —retirado, restringido, beta cerrada como el AppID 397080— o
//! su bloque `assets` no trae carátula, aquí no se escribe nada: ni la
//! convención, ni la imagen de otra variante, ni la de otro juego. La interfaz
//! muestra su monograma, que es la respuesta correcta.
//!
//! La convención sigue viva como red de seguridad: [`super::local`] la usa para
//! sembrar las filas nuevas y la escalera de [`crate::art_cache`] la deriva
//! cuando no hay nada mejor. Este módulo añade precisión, no la sustituye.

use crate::db::Database;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Raíz de la CDN de recursos de tienda. `asset_url_format` llega relativo a
/// ella (`steam/apps/<id>/${FILENAME}?t=<epoch>`).
const ASSET_ROOT: &str = "https://shared.steamstatic.com/store_item_assets/";
/// Hueco que el índice deja para el nombre de archivo dentro de la plantilla.
const FILENAME_PLACEHOLDER: &str = "${FILENAME}";
/// AppID por petición, el mismo lote que el resolutor de títulos. La biblioteca
/// de 1815 juegos cabe en diez peticiones.
const BATCH: usize = 200;
/// Clave de `app_settings` con la última pasada completada, en RFC 3339.
const LAST_REFRESH_KEY: &str = "art_index_refreshed_at";
/// Cada cuánto se vuelve a contrastar la biblioteca entera con el índice. El
/// arte de Steam cambia de tarde en tarde; medio día es suficiente para
/// recoger un rebranding sin repetir diez peticiones en cada arranque.
const REFRESH_INTERVAL_HOURS: i64 = 12;
/// Techo de juegos por pasada. Una biblioteca gigantesca no debe convertir el
/// refresco en cientos de peticiones seguidas; lo que no entre se resuelve en
/// la siguiente pasada.
const MAX_GAMES_PER_RUN: usize = 5_000;
/// Código con el que este módulo informa de que Steam ha cortado el grifo.
const RATE_LIMITED_CODE: &str = "steam_store_assets_rate_limited";

/// Arte real de un juego según el índice. Cada campo es `None` cuando el índice
/// no publica esa imagen; nunca se rellena por convención desde aquí.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameArt {
    pub cover_url: Option<String>,
    pub header_url: Option<String>,
    pub capsule_url: Option<String>,
    /// El arte apaisado de biblioteca —1920×620 y a todo color—, que es el
    /// banner de la ficha. No confundir con `games.hero_url`, que guarda el
    /// fondo de la página de tienda: un degradado oscuro pensado para llevar
    /// texto encima.
    pub library_hero_url: Option<String>,
}

impl GameArt {
    fn is_empty(&self) -> bool {
        self.cover_url.is_none()
            && self.header_url.is_none()
            && self.capsule_url.is_none()
            && self.library_hero_url.is_none()
    }
}

/// Resultado de contrastar la biblioteca con el índice.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtIndexReport {
    /// Juegos por los que se preguntó.
    pub examined: usize,
    /// Juegos para los que el índice publicó al menos una imagen.
    pub resolved: usize,
    /// Filas de `games` cuya URL cambió de verdad.
    pub updated: usize,
    /// Juegos sin arte publicada. Se quedan con su monograma a propósito.
    pub without_art: usize,
    /// Lotes que no se pudieron consultar. Sus juegos conservan lo que tenían.
    pub failed_batches: usize,
}

#[derive(Debug, Deserialize)]
struct ItemsEnvelope {
    response: ItemsResponse,
}

#[derive(Debug, Default, Deserialize)]
struct ItemsResponse {
    #[serde(default)]
    store_items: Vec<ItemPayload>,
}

#[derive(Debug, Deserialize)]
struct ItemPayload {
    #[serde(default)]
    appid: Option<u32>,
    #[serde(default)]
    id: Option<u32>,
    #[serde(default)]
    success: u32,
    #[serde(default)]
    assets: Option<AssetsPayload>,
}

/// Sólo se declaran los recursos que Vindexa guarda. El índice publica muchos
/// más (fondos, iconos de comunidad, cápsulas pequeñas) y `serde` los ignora.
#[derive(Debug, Deserialize)]
struct AssetsPayload {
    #[serde(default)]
    asset_url_format: Option<String>,
    #[serde(default)]
    library_capsule: Option<String>,
    #[serde(default)]
    library_capsule_2x: Option<String>,
    #[serde(default)]
    header: Option<String>,
    #[serde(default)]
    main_capsule: Option<String>,
    #[serde(default)]
    library_hero: Option<String>,
    #[serde(default)]
    library_hero_2x: Option<String>,
}

/// Pregunta al índice por un lote de AppID y devuelve el arte real de cada uno.
///
/// El resultado sólo contiene juegos con al menos una imagen publicada: un
/// AppID ausente significa «el índice no tiene arte para esto», nunca «usa
/// otra cosa». Los lotes se despachan de doscientos en doscientos.
pub async fn resolve(app_ids: &[u32]) -> AppResult<Vec<(u32, GameArt)>> {
    let client = super::wishlist::wishlist_client()?;
    let mut resolved = Vec::new();
    for chunk in app_ids.chunks(BATCH) {
        let bytes = super::wishlist::get_bytes(
            client,
            super::wishlist::STORE_ITEMS_ENDPOINT,
            &[("input_json", request_body(chunk))],
        )
        .await
        .map_err(translate_error)?;
        resolved.extend(parse_assets(&bytes)?);
    }
    Ok(resolved)
}

/// Contrasta toda la biblioteca con el índice y corrige lo que esté mal.
///
/// Se recorre entera en vez de filtrar «las que parecen rotas» porque la URL
/// correcta y la derivada por convención son idénticas para los 1341 juegos
/// antiguos: no hay forma de distinguirlas mirando la columna. A cambio, el
/// coste está acotado por el tamaño de la biblioteca —diez peticiones para
/// 1815 juegos— y no por lo que se sospeche de cada fila.
///
/// Un lote que falle no cancela la pasada: sus juegos conservan lo que tenían
/// y se cuentan en [`ArtIndexReport::failed_batches`]. El límite de peticiones
/// sí la corta, porque insistir sólo lo empeora.
pub async fn refresh_library_art(database: &Database) -> AppResult<ArtIndexReport> {
    let app_ids = library_app_ids(database)?;
    let mut report = ArtIndexReport {
        examined: app_ids.len(),
        ..ArtIndexReport::default()
    };
    if app_ids.is_empty() {
        record_refresh(database)?;
        return Ok(report);
    }

    for chunk in app_ids.chunks(BATCH) {
        let art = match resolve(chunk).await {
            Ok(art) => art,
            Err(error) if error.code == RATE_LIMITED_CODE => return Err(error),
            Err(_) => {
                report.failed_batches += 1;
                continue;
            }
        };
        report.resolved += art.len();
        report.without_art += chunk.len().saturating_sub(art.len());
        report.updated += persist(database, &art)?;
    }

    // La marca sólo se pone cuando la pasada fue completa. Con lotes caídos hay
    // juegos que siguen apuntando a un archivo inexistente, y hacerles esperar
    // medio día por una caída de red pasajera sería castigarlos dos veces.
    if report.failed_batches == 0 {
        record_refresh(database)?;
    }
    Ok(report)
}

/// ¿Ha pasado ya el intervalo desde la última pasada completa?
///
/// Una marca ilegible o ausente cuenta como «toca»: es preferible una consulta
/// de más a dejar la biblioteca con carátulas rotas.
pub fn refresh_due(database: &Database) -> AppResult<bool> {
    let connection = database.open()?;
    let last: Option<String> = connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [LAST_REFRESH_KEY],
            |row| row.get(0),
        )
        .optional()?;
    let Some(last) = last
        .as_deref()
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
    else {
        return Ok(true);
    };
    Ok(Utc::now().signed_duration_since(last.with_timezone(&Utc))
        >= ChronoDuration::hours(REFRESH_INTERVAL_HOURS))
}

/// Cuerpo `input_json` del índice, pidiendo únicamente los recursos.
///
/// El idioma importa: con `spanish` el índice devuelve la carátula localizada
/// (`library_600x900_spanish_2x.jpg`) cuando el estudio publica una.
fn request_body(app_ids: &[u32]) -> String {
    let ids = app_ids
        .iter()
        .map(|app_id| format!("{{\"appid\":{app_id}}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"ids\":[{ids}],\"context\":{{\"language\":\"spanish\",\"country_code\":\"ES\"}},\
         \"data_request\":{{\"include_assets\":true}}}}"
    )
}

fn parse_assets(bytes: &[u8]) -> AppResult<Vec<(u32, GameArt)>> {
    let envelope: ItemsEnvelope = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_store_assets_response",
            "La tienda de Steam devolvió un índice de recursos que Vindexa no pudo interpretar.",
        )
    })?;
    Ok(envelope
        .response
        .store_items
        .into_iter()
        .filter_map(|item| {
            let app_id = item.appid.or(item.id).filter(|app_id| *app_id > 0)?;
            // `success != 1` es una ficha que la tienda no resolvió: llega sin
            // bloque `assets` y no se compensa con nada.
            if item.success != 1 {
                return None;
            }
            let assets = item.assets?;
            let format = assets.asset_url_format.as_deref()?;
            let art = GameArt {
                // La carátula grande es `_2x`; la sencilla mide la mitad. Ciento
                // setenta y cuatro juegos del catálogo sólo publican la
                // sencilla, y para ellos es la mejor que existe.
                cover_url: assets
                    .library_capsule_2x
                    .as_deref()
                    .or(assets.library_capsule.as_deref())
                    .and_then(|file| asset_url(format, file, app_id)),
                header_url: assets
                    .header
                    .as_deref()
                    .and_then(|file| asset_url(format, file, app_id)),
                capsule_url: assets
                    .main_capsule
                    .as_deref()
                    .and_then(|file| asset_url(format, file, app_id)),
                // El banner de la ficha. El `_2x` cuando lo hay: el hueco mide
                // más de mil píxeles y la versión sencilla llega ampliada.
                library_hero_url: assets
                    .library_hero_2x
                    .as_deref()
                    .or(assets.library_hero.as_deref())
                    .and_then(|file| asset_url(format, file, app_id)),
            };
            (!art.is_empty()).then_some((app_id, art))
        })
        .collect())
}

/// Compone la URL final a partir de la plantilla del índice y el nombre real
/// del archivo, y la somete a la misma validación que aplica la caché de arte
/// antes de descargar nada.
///
/// El sufijo `?t=<epoch>` de la plantilla se descarta: sólo sirve para forzar
/// recarga en el navegador de la tienda y cambia cada vez que el estudio toca
/// su ficha, lo que reescribiría la columna sin que la imagen cambie.
fn asset_url(format: &str, file_name: &str, app_id: u32) -> Option<String> {
    let template = format.split('?').next()?.trim();
    let file_name = file_name.trim();
    if template.is_empty() || file_name.is_empty() || !template.contains(FILENAME_PLACEHOLDER) {
        return None;
    }
    let url = format!(
        "{ASSET_ROOT}{}",
        template.replace(FILENAME_PLACEHOLDER, file_name)
    );
    crate::art_cache::validate_source_url(&url, app_id).ok()?;
    Some(url)
}

/// AppID de la biblioteca, del más antiguo de actualizar al más reciente, para
/// que una biblioteca mayor que [`MAX_GAMES_PER_RUN`] avance en cada pasada.
fn library_app_ids(database: &Database) -> AppResult<Vec<u32>> {
    let connection = database.open()?;
    let mut statement = connection
        .prepare("SELECT app_id FROM games ORDER BY updated_at ASC, app_id ASC LIMIT ?1")?;
    let ids = statement
        .query_map([MAX_GAMES_PER_RUN as i64], |row| row.get::<_, u32>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Guarda el arte resuelto y devuelve cuántas filas cambiaron de verdad.
///
/// La condición del `WHERE` evita reescribir `updated_at` en los 1341 juegos
/// cuya URL ya era correcta: sin ella, cada pasada marcaría la biblioteca
/// entera como modificada.
fn persist(database: &Database, art: &[(u32, GameArt)]) -> AppResult<usize> {
    if art.is_empty() {
        return Ok(0);
    }
    let mut connection = database.open()?;
    let transaction = connection.transaction()?;
    let mut updated = 0;
    {
        let mut statement = transaction.prepare(
            // El banner es la excepción a la regla de `COALESCE`: se escribe
            // **tal cual**, incluido el vacío. El índice acaba de decir qué arte
            // publica este juego, así que si no trae banner es que no lo hay, y
            // dejar puesta una URL derivada por convención sería conservar una
            // dirección que devuelve 404. Con las demás no se hace porque su
            // valor anterior puede venir de la ficha de la tienda, que también
            // es una fuente buena.
            "UPDATE games
                SET cover_url = COALESCE(?2, cover_url),
                    header_url = COALESCE(?3, header_url),
                    capsule_url = COALESCE(?4, capsule_url),
                    library_hero_url = ?5,
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
              WHERE app_id = ?1
                AND (COALESCE(?2, cover_url) IS NOT cover_url
                  OR COALESCE(?3, header_url) IS NOT header_url
                  OR COALESCE(?4, capsule_url) IS NOT capsule_url
                  OR ?5 IS NOT library_hero_url)",
        )?;
        for (app_id, art) in art {
            updated += statement.execute(params![
                app_id,
                art.cover_url,
                art.header_url,
                art.capsule_url,
                art.library_hero_url,
            ])?;
        }
    }
    transaction.commit()?;
    Ok(updated)
}

fn record_refresh(database: &Database) -> AppResult<()> {
    database.open()?.execute(
        "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET
            value = excluded.value,
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![LAST_REFRESH_KEY, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

/// El transporte se comparte con la lista de deseados, pero sus mensajes hablan
/// de deseados. Aquí se traducen para que nadie lea «no se pudo leer la lista
/// de deseados» cuando lo que falló fue el arte.
fn translate_error(error: AppError) -> AppError {
    let message = match error.code.as_str() {
        "steam_wishlist_rate_limited" => {
            return AppError::new(RATE_LIMITED_CODE, error.message);
        }
        "steam_wishlist_timeout" => {
            "Steam no respondió a tiempo al pedir el índice de recursos. Vuelve a intentarlo."
        }
        "steam_wishlist_connection" => {
            "No se pudo conectar con Steam para consultar el índice de recursos."
        }
        "steam_wishlist_unavailable" => {
            "Steam no está disponible temporalmente. Vuelve a intentarlo más tarde."
        }
        _ => "No se pudo consultar el índice de recursos de la tienda de Steam.",
    };
    AppError::new("steam_store_assets_unavailable", message)
}

#[cfg(test)]
mod tests {
    use super::{
        ArtIndexReport, GameArt, LAST_REFRESH_KEY, RATE_LIMITED_CODE, asset_url, library_app_ids,
        parse_assets, persist, refresh_due, request_body, translate_error,
    };
    use crate::db::Database;
    use crate::error::AppError;
    use rusqlite::params;
    use tempfile::TempDir;

    /// Respuesta real del índice, recortada a los campos que se leen aquí:
    /// un juego moderno con hash por archivo, uno antiguo sin hash, uno sin
    /// ficha pública y uno cuya carátula grande no existe.
    const INDEX_OK: &str = r#"{"response":{"store_items":[
        {"item_type":0,"id":3483510,"success":1,"visible":true,
         "name":"The Adventures of Elliot: The Millennium Tales","appid":3483510,
         "assets":{
            "asset_url_format":"steam/apps/3483510/${FILENAME}?t=1782353681",
            "main_capsule":"0d5cbb1d81642a256b5a690d5c27b57cdd7891d4/capsule_616x353.jpg",
            "header":"3c96b19255b69aa9b2e131dda3e19d622b0d6562/header.jpg",
            "library_capsule":"c45a0dcc4361206e34be411af44de7cf0cd2cd5b/library_capsule.jpg",
            "library_capsule_2x":"c45a0dcc4361206e34be411af44de7cf0cd2cd5b/library_capsule_2x.jpg",
            "library_hero":"a38bcd252faf83c70e24dd9f17731bfd0c0aaa6f/library_hero.jpg",
            "library_hero_2x":"a38bcd252faf83c70e24dd9f17731bfd0c0aaa6f/library_hero_2x.jpg",
            "community_icon":"f38cd000ec6342a00c744f6d8ca7c877bde4b9f9"}},
        {"item_type":0,"id":927380,"success":1,"visible":true,
         "name":"Yakuza Kiwami 2 (Legacy)","appid":927380,
         "assets":{
            "asset_url_format":"steam/apps/927380/${FILENAME}?t=1765536902",
            "main_capsule":"capsule_616x353.jpg",
            "header":"header.jpg",
            "library_capsule":"library_600x900.jpg",
            "library_capsule_2x":"library_600x900_2x.jpg",
            "library_hero":"library_hero.jpg"}},
        {"item_type":0,"id":397080,"success":15,"visible":false,"name":"","appid":397080},
        {"item_type":0,"id":2700,"success":1,"visible":true,"name":"Half-Life: Source","appid":2700,
         "assets":{
            "asset_url_format":"steam/apps/2700/${FILENAME}?t=1745364618",
            "main_capsule":"capsule_616x353.jpg",
            "header":"header.jpg",
            "library_capsule":"portrait.png"}}
    ]}}"#;

    fn temp_database(directory: &TempDir) -> Database {
        let database = Database::new(directory.path().join("prueba.sqlite3"));
        database.initialize().expect("preparar base de prueba");
        database
    }

    fn art_of(resolved: &[(u32, GameArt)], app_id: u32) -> Option<&GameArt> {
        resolved
            .iter()
            .find(|(id, _)| *id == app_id)
            .map(|(_, art)| art)
    }

    #[test]
    fn a_modern_game_resolves_its_hashed_cover() {
        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        let art = art_of(&resolved, 3_483_510).expect("juego moderno resuelto");

        assert_eq!(
            art.cover_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/3483510/c45a0dcc4361206e34be411af44de7cf0cd2cd5b/library_capsule_2x.jpg"
            )
        );
        assert_eq!(
            art.header_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/3483510/3c96b19255b69aa9b2e131dda3e19d622b0d6562/header.jpg"
            )
        );
        assert_eq!(
            art.capsule_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/3483510/0d5cbb1d81642a256b5a690d5c27b57cdd7891d4/capsule_616x353.jpg"
            )
        );
        // El hash es por archivo: el de la carátula no es el de la cabecera.
        assert_ne!(art.cover_url, art.header_url);
        // El `?t=` de la plantilla no viaja a la columna.
        assert!(!art.cover_url.as_deref().unwrap_or_default().contains('?'));
    }

    /// El banner de la ficha sale del índice, no de una convención.
    ///
    /// Es la respuesta al fallo que se veía en la aplicación: la ficha enseñaba
    /// el fondo de la página de tienda —un degradado oscuro— porque el arte de
    /// biblioteca vive bajo un hash que no se puede adivinar desde el AppID. El
    /// índice publica el nombre real, incluido el `_2x`, que es el que cabe en
    /// un hueco de mil píxeles.
    #[test]
    fn el_banner_de_biblioteca_sale_del_indice_con_su_hash() {
        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");

        let moderno = art_of(&resolved, 3_483_510).expect("juego moderno resuelto");
        assert_eq!(
            moderno.library_hero_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/3483510/a38bcd252faf83c70e24dd9f17731bfd0c0aaa6f/library_hero_2x.jpg"
            ),
            "con `_2x` publicado, el banner grande"
        );

        let antiguo = art_of(&resolved, 927_380).expect("juego antiguo resuelto");
        assert_eq!(
            antiguo.library_hero_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/927380/library_hero.jpg"
            ),
            "sin `_2x`, el sencillo, que es el mejor que existe"
        );

        // Y un juego que no publica banner se queda sin banner: escribir la URL
        // por convención sería guardar una dirección que devuelve 404.
        let sin_banner = art_of(&resolved, 2700).expect("juego sin banner resuelto");
        assert_eq!(sin_banner.library_hero_url, None);
    }

    #[test]
    fn an_old_game_keeps_the_conventional_names() {
        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        let art = art_of(&resolved, 927_380).expect("juego antiguo resuelto");

        assert_eq!(
            art.cover_url.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/927380/library_600x900_2x.jpg"
            )
        );
        assert_eq!(
            art.header_url.as_deref(),
            Some("https://shared.steamstatic.com/store_item_assets/steam/apps/927380/header.jpg")
        );
    }

    #[test]
    fn a_game_without_published_art_is_left_alone() {
        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");

        assert!(
            art_of(&resolved, 397_080).is_none(),
            "un juego sin ficha pública no puede recibir arte de ningún sitio"
        );
    }

    #[test]
    fn a_game_without_the_large_cover_falls_back_to_the_small_one() {
        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        let art = art_of(&resolved, 2_700).expect("juego con carátula sencilla");

        assert_eq!(
            art.cover_url.as_deref(),
            Some("https://shared.steamstatic.com/store_item_assets/steam/apps/2700/portrait.png")
        );
    }

    #[test]
    fn a_corrupt_response_fails_with_its_own_code() {
        let error = parse_assets(b"{\"response\":{\"store_items\":[{\"appid\":")
            .expect_err("rechazar JSON roto");
        assert_eq!(error.code, "steam_store_assets_response");

        // Un bloque `assets` sin plantilla no produce ninguna URL a medias.
        let sin_plantilla = br#"{"response":{"store_items":[
            {"id":70,"success":1,"appid":70,"assets":{"header":"header.jpg"}}]}}"#;
        assert!(parse_assets(sin_plantilla).expect("leer índice").is_empty());
    }

    #[test]
    fn a_partial_batch_resolves_what_it_can() {
        // La respuesta trae cuatro fichas para un lote de cinco AppID: la que
        // falta no se inventa y las demás se resuelven igual.
        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        let pedidos = [3_483_510_u32, 927_380, 397_080, 2_700, 4_173_590];

        assert_eq!(resolved.len(), 3);
        assert!(art_of(&resolved, 4_173_590).is_none());
        let sin_arte = pedidos.len() - resolved.len();
        assert_eq!(sin_arte, 2);
    }

    #[test]
    fn hostile_asset_paths_never_reach_the_database() {
        // Nombre que intenta salirse de `/apps/<id>/`.
        assert!(asset_url("steam/apps/70/${FILENAME}", "../../evil.jpg", 70).is_none());
        // Plantilla de otro juego: la validación exige el AppID en la ruta.
        assert!(asset_url("steam/apps/70/${FILENAME}", "header.jpg", 620).is_none());
        // Plantilla sin hueco para el nombre.
        assert!(asset_url("steam/apps/70/header.jpg", "header.jpg", 70).is_none());
        // Nombre vacío.
        assert!(asset_url("steam/apps/70/${FILENAME}", "   ", 70).is_none());
    }

    #[test]
    fn the_request_asks_the_index_for_the_assets_block() {
        let body = request_body(&[10, 20]);

        assert!(body.contains("{\"appid\":10}"));
        assert!(body.contains("\"include_assets\":true"));
        assert!(!body.contains("key"));
    }

    #[test]
    fn persisting_only_touches_the_rows_that_change() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let convencional = "https://shared.steamstatic.com/store_item_assets/steam/apps/3483510/library_600x900_2x.jpg";
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, cover_url) VALUES (3483510, 'Elliot', ?1)",
                [convencional],
            )
            .expect("insertar juego");

        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        let elliot = resolved
            .iter()
            .filter(|(app_id, _)| *app_id == 3_483_510)
            .cloned()
            .collect::<Vec<_>>();

        assert_eq!(persist(&database, &elliot).expect("guardar"), 1);
        // La segunda pasada no reescribe nada: la fila ya está al día.
        assert_eq!(persist(&database, &elliot).expect("guardar de nuevo"), 0);

        let guardada: String = database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT cover_url FROM games WHERE app_id = 3483510",
                [],
                |row| row.get(0),
            )
            .expect("leer carátula");
        assert!(guardada.contains("/c45a0dcc4361206e34be411af44de7cf0cd2cd5b/"));
    }

    /// El banner llega hasta la columna, y una convención equivocada se borra.
    ///
    /// Analizar bien la respuesta no sirve de nada si el `UPDATE` no la escribe.
    /// Y hay un caso que no puede tratarse como los demás: cuando el índice
    /// resuelve el juego y **no** publica banner, dejar puesta una URL derivada
    /// por convención es conservar una dirección que devuelve 404, y el caché de
    /// arte gastaría una petición en ella cada vez.
    #[test]
    fn el_banner_llega_a_la_columna_y_borra_la_conjetura_equivocada() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let conjetura =
            "https://shared.steamstatic.com/store_item_assets/steam/apps/2700/library_hero.jpg";
        {
            let connection = database.open().expect("abrir base");
            connection
                .execute(
                    "INSERT INTO games(app_id, title, library_hero_url)
                     VALUES (3483510, 'Elliot', NULL)",
                    [],
                )
                .expect("insertar el moderno");
            // Este sí tiene una conjetura guardada, y el índice dice que no hay
            // banner: la 049 la escribió para toda la biblioteca.
            connection
                .execute(
                    "INSERT INTO games(app_id, title, library_hero_url)
                     VALUES (2700, 'Half-Life: Source', ?1)",
                    [conjetura],
                )
                .expect("insertar el que no tiene banner");
        }

        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        persist(&database, &resolved).expect("guardar");

        let connection = database.open().expect("abrir base");
        let moderno: Option<String> = connection
            .query_row(
                "SELECT library_hero_url FROM games WHERE app_id = 3483510",
                [],
                |row| row.get(0),
            )
            .expect("leer el moderno");
        assert_eq!(
            moderno.as_deref(),
            Some(
                "https://shared.steamstatic.com/store_item_assets/steam/apps/3483510/a38bcd252faf83c70e24dd9f17731bfd0c0aaa6f/library_hero_2x.jpg"
            ),
            "el banner con su hash tiene que llegar a la columna"
        );

        let sin_banner: Option<String> = connection
            .query_row(
                "SELECT library_hero_url FROM games WHERE app_id = 2700",
                [],
                |row| row.get(0),
            )
            .expect("leer el que no tiene banner");
        assert_eq!(
            sin_banner, None,
            "sin banner publicado, la conjetura se borra en vez de quedarse dando 404"
        );
    }

    /// Contra el índice de verdad.
    ///
    /// Apagada por defecto. Comprueba lo único que se rompe solo: que el índice
    /// sigue publicando `library_hero` y que en un juego moderno viene bajo un
    /// hash —que es justo lo que ninguna convención alcanza y el motivo por el
    /// que la ficha enseñaba el fondo oscuro de la tienda—.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_el_indice_de_verdad
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_el_indice_de_verdad_el_banner_sigue_publicandose() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        // Portal 2, de 2011, y DragonSword : Awakening, de 2026: uno con los
        // nombres de siempre y otro con hash por archivo.
        let art = runtime
            .block_on(super::resolve(&[620, 4_570_720]))
            .expect("preguntar al índice");

        for app_id in [620_u32, 4_570_720] {
            let (_, juego) = art
                .iter()
                .find(|(id, _)| *id == app_id)
                .unwrap_or_else(|| panic!("el índice ya no resuelve {app_id}"));
            let banner = juego
                .library_hero_url
                .as_deref()
                .unwrap_or_else(|| panic!("el índice dejó de publicar el banner de {app_id}"));
            assert!(
                banner.contains("library_hero"),
                "el banner de {app_id} dejó de llamarse library_hero: {banner}"
            );
        }

        let moderno = art
            .iter()
            .find(|(id, _)| *id == 4_570_720)
            .expect("el moderno")
            .1
            .library_hero_url
            .clone()
            .expect("banner del moderno");
        let ruta = moderno
            .rsplit_once("/apps/4570720/")
            .expect("la ruta nombra el AppID")
            .1;
        assert!(
            ruta.contains('/'),
            "un juego moderno guarda su banner bajo un hash; sin él, la convención bastaría: {ruta}"
        );
    }

    #[test]
    fn a_game_the_index_ignores_keeps_what_it_had() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        database
            .open()
            .expect("abrir base")
            .execute(
                "INSERT INTO games(app_id, title, cover_url) VALUES (397080, 'Magicka 2: Spell Balance Beta', NULL)",
                [],
            )
            .expect("insertar juego sin arte");

        let resolved = parse_assets(INDEX_OK.as_bytes()).expect("leer índice");
        persist(&database, &resolved).expect("guardar");

        let cover: Option<String> = database
            .open()
            .expect("abrir base")
            .query_row(
                "SELECT cover_url FROM games WHERE app_id = 397080",
                [],
                |row| row.get(0),
            )
            .expect("leer carátula");
        assert!(
            cover.is_none(),
            "un juego sin arte publicada no puede recibir la de otro"
        );
    }

    #[test]
    fn the_refresh_interval_is_honoured() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);

        // Sin marca previa, toca.
        assert!(refresh_due(&database).expect("consultar marca"));

        let connection = database.open().expect("abrir base");
        let escribir = |valor: &str| {
            connection
                .execute(
                    "INSERT INTO app_settings(key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![LAST_REFRESH_KEY, valor],
                )
                .expect("escribir marca");
        };

        escribir(&chrono::Utc::now().to_rfc3339());
        assert!(!refresh_due(&database).expect("marca reciente"));

        escribir(&(chrono::Utc::now() - chrono::Duration::hours(13)).to_rfc3339());
        assert!(refresh_due(&database).expect("marca caducada"));

        // Una marca ilegible no bloquea el refresco.
        escribir("ayer por la tarde");
        assert!(refresh_due(&database).expect("marca ilegible"));
    }

    #[test]
    fn the_library_query_prefers_the_least_recently_updated() {
        let directory = TempDir::new().expect("crear directorio temporal");
        let database = temp_database(&directory);
        let connection = database.open().expect("abrir base");
        for (app_id, updated_at) in [
            (10_u32, "2026-08-18T10:00:00.000Z"),
            (20, "2020-01-01T00:00:00.000Z"),
        ] {
            connection
                .execute(
                    "INSERT INTO games(app_id, title, updated_at) VALUES (?1, 'Juego', ?2)",
                    params![app_id, updated_at],
                )
                .expect("insertar juego");
        }

        assert_eq!(library_app_ids(&database).expect("listar"), vec![20, 10]);
    }

    #[test]
    fn transport_errors_stop_talking_about_the_wishlist() {
        let limited = translate_error(AppError::new(
            "steam_wishlist_rate_limited",
            "Steam ha limitado temporalmente las peticiones.",
        ));
        assert_eq!(limited.code, RATE_LIMITED_CODE);

        let otro = translate_error(AppError::new(
            "steam_wishlist_content_type",
            "Steam devolvió un tipo de contenido inesperado al pedir la lista de deseados.",
        ));
        assert_eq!(otro.code, "steam_store_assets_unavailable");
        assert!(!otro.message.contains("deseados"));

        // El informe vacío es un valor útil, no un error.
        assert_eq!(ArtIndexReport::default().updated, 0);
    }
}
