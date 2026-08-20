//! De dónde salen los próximos lanzamientos que Vindexa puntúa.
//!
//! # El hueco que cierra este módulo
//!
//! El motor de gustos (`db::priority`) sabe aprender qué te gusta y puntuar
//! candidatos con ese modelo, y la pantalla sabe enseñarlos. Lo que faltaba era
//! quien trajera los candidatos: `upcoming_releases` no la rellenaba nadie, así
//! que la sección quedaba siempre vacía por muy bien que funcionara todo lo
//! demás.
//!
//! # De dónde se sacan, y por qué de ahí
//!
//! De **tu lista de deseados**. Es la única fuente honesta que hay a mano: son
//! juegos que ya has marcado tú, no una lista de novedades que a Vindexa se le
//! ocurra recomendar. Un juego de esa lista que la tienda declara sin publicar
//! es exactamente un próximo lanzamiento que te interesa.
//!
//! Que sea `coming_soon` lo dice la ficha oficial; no se deduce de que falte la
//! fecha, porque una fecha ausente también puede ser una fecha ilegible. Ver
//! [`crate::steam::store_api::ReleaseWindow`].
//!
//! # La segunda fuente: lo que la tienda destaca
//!
//! Sólo con los deseados, la sección no descubre nada: enseña juegos que ya
//! habías marcado tú. Por eso mira también la sección **«próximamente» del
//! escaparate público** de Steam —la misma petición sin clave ni sesión que ya
//! hace el radar de ofertas—, y de ahí se queda con lo que **es un juego** (ni
//! demos ni DLC, que el escaparate mezcla), no tienes todavía, y la ficha
//! confirma como no publicado.
//!
//! Lo que llega por ahí se puntúa contra el mismo modelo local de gustos, así
//! que la lista sigue ordenándose por lo que a ti te encaja y no por lo que la
//! tienda quiera empujar. Y se distingue en pantalla de lo que sale de tus
//! deseados, porque no son la misma clase de aviso: uno es un recordatorio y el
//! otro un hallazgo.
//!
//! # Qué no hace
//!
//! - **No manda nada para descubrir.** La lista de «próximamente» es la misma
//!   para todo el mundo: pedirla no dice quién eres ni qué tienes.
//! - **No manda tus gustos a ninguna parte.** El modelo se aplica en local
//!   sobre lo ya descargado, como documenta `db::priority`.
//! - **No inventa una fecha.** Cuando la tienda sólo publica «Q4 2026», eso es
//!   lo que se guarda, marcado como no exacta.

use crate::db::Database;
use crate::db::priority::{ImportedUpcomingRelease, UpcomingImportSummary};
use crate::error::AppError;
use crate::error::AppResult;
use crate::steam::store_api::{self, StoreBundleOutcome};
use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

/// Cuántos deseados se revisan como mucho en cada pasada.
///
/// Cada uno cuesta una petición a la tienda. Con una lista de novecientos,
/// revisarlos todos de una vez son novecientas peticiones seguidas: Steam
/// respondería con un límite de frecuencia mucho antes. Se revisan por tandas y
/// se completan en pasadas sucesivas.
const MAX_PER_RUN: usize = 60;

/// Espera entre fichas. La sincronización de metadatos usa el mismo criterio:
/// ir despacio sale más barato que ir rápido y que te frenen.
const BETWEEN_REQUESTS: Duration = Duration::from_millis(350);

/// Tope de la descripción que acepta `db::priority::upsert_upcoming`.
///
/// Alguna ficha de Steam trae un resumen más largo. Se recorta aquí en vez de
/// ensanchar el límite de la base: es un resumen para reconocer el juego de un
/// vistazo, no el texto íntegro, y quien lo quiera entero tiene la ficha.
const MAX_DESCRIPTION_CHARS: usize = 2_000;

/// Origen que queda escrito en `upcoming_releases.source`.
///
/// Uno de los tres que admite `db::priority::UPCOMING_SOURCES`: el dato viene
/// de la ficha oficial de la tienda, aunque quien decide a qué juegos mirar sea
/// la lista de deseados.
const SOURCE: &str = "store";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingRefreshReport {
    /// Deseados revisados en esta pasada.
    pub checked: u32,
    /// De ellos, cuántos siguen sin publicarse.
    pub upcoming: u32,
    /// Cuántos entraron por primera vez.
    pub inserted: u32,
    /// Cuántos ya estaban y se han refrescado.
    pub updated: u32,
    /// Fichas que la tienda no devolvió. No es un error: hay juegos retirados.
    pub unavailable: u32,
    /// Quedan deseados por revisar en la próxima pasada.
    pub pending: u32,
}

/// Revisa una tanda de deseados y guarda los que aún no han salido.
///
/// Se empieza por los que hace más tiempo que no se miran, de modo que pasadas
/// sucesivas acaben cubriendo la lista entera sin repetir siempre los mismos.
pub async fn refresh_from_wishlist(database: &Database) -> AppResult<UpcomingRefreshReport> {
    let candidates = pending_candidates(database, MAX_PER_RUN)?;
    let pending_total = pending_count(database)?;

    let mut items: Vec<ImportedUpcomingRelease> = Vec::new();
    let mut checked = 0_u32;
    let mut unavailable = 0_u32;

    for (index, (app_id, fallback_title)) in candidates.iter().enumerate() {
        if index > 0 {
            sleep(BETWEEN_REQUESTS).await;
        }
        checked += 1;
        match store_api::fetch_bundle_with_retry_hint(*app_id).await {
            Ok(StoreBundleOutcome::Found(bundle)) => {
                if !bundle.release.coming_soon {
                    continue;
                }
                let metadata = &bundle.metadata;
                let dia = bundle
                    .release
                    .label
                    .as_deref()
                    .and_then(store_api::exact_release_day);
                items.push(ImportedUpcomingRelease {
                    app_id: *app_id,
                    // El título sale del catálogo de deseados: la ficha de la
                    // tienda no devuelve el nombre en este paquete.
                    title: fallback_title.clone(),
                    capsule_url: metadata.capsule_url.clone(),
                    header_url: metadata.header_url.clone(),
                    // «19 AGO 2026» es un día concreto y «Q4 2026» no lo es.
                    // La exactitud se decide leyendo la etiqueta, no mirando
                    // `metadata.release_date`: ese campo se deja vacío a
                    // propósito para todo lo que aún no ha salido, así que
                    // preguntarle daba «aproximada» siempre, incluso con el día
                    // delante.
                    release_date: dia.clone().or_else(|| bundle.release.label.clone()),
                    release_date_is_exact: dia.is_some(),
                    genres: metadata.genres.clone(),
                    categories: metadata.categories.clone(),
                    developer: metadata.developer.clone(),
                    publisher: metadata.publisher.clone(),
                    short_description: metadata.short_description.as_deref().map(recortar),
                    source: SOURCE.to_string(),
                });
            }
            Ok(StoreBundleOutcome::Unavailable) => unavailable += 1,
            // Un fallo de red corta la pasada, pero no tira lo ya reunido: se
            // guarda lo que hay y la siguiente pasada sigue donde ésta lo dejó.
            Err(_) => break,
        }
    }

    let upcoming = items.len() as u32;
    let summary: UpcomingImportSummary =
        crate::db::priority::upsert_upcoming(&mut database.open()?, &items)?;
    mark_checked(database, &candidates)?;

    Ok(UpcomingRefreshReport {
        checked,
        upcoming,
        inserted: summary.inserted,
        updated: summary.updated,
        unavailable,
        pending: pending_total.saturating_sub(checked),
    })
}

/// Recorta un resumen largo por el último espacio que quepa.
///
/// Cortar por el carácter exacto parte palabras y, si se cortara por bytes,
/// además rompería cualquier letra acentuada. Se corta por palabra y se marca
/// con puntos suspensivos, para que se vea que hay más.
fn recortar(texto: &str) -> String {
    if texto.chars().count() <= MAX_DESCRIPTION_CHARS {
        return texto.to_owned();
    }
    let recortado: String = texto.chars().take(MAX_DESCRIPTION_CHARS - 1).collect();
    let corte = recortado.rfind(' ').unwrap_or(recortado.len());
    format!("{}…", recortado[..corte].trim_end())
}

/// Deseados que llevan más tiempo sin revisarse.
fn pending_candidates(database: &Database, limit: usize) -> AppResult<Vec<(u32, String)>> {
    let connection = database.open()?;
    // Los que nunca se han mirado van primero —su marca es la cadena vacía— y
    // detrás los más antiguos. Así una lista larga se cubre entera en pasadas
    // sucesivas en vez de repetir siempre la cabeza.
    // Primero los que el índice dice que están por salir, después los que no
    // constan, y nunca los que ya salieron.
    //
    // Sin este orden, la cola rotaba por los 1.345 deseados pidiendo la ficha
    // de uno en uno, y siete de cada diez peticiones se gastaban en juegos ya
    // publicados que se descartaban al llegar: cubrir la lista entera eran
    // veintitrés pasadas. Con 422 por salir, ahora son siete.
    //
    // `NULL` es «no se sabe» y va en medio: un juego del que el índice no ha
    // contestado no se descarta, sólo espera su turno detrás de los que constan.
    let mut statement = connection.prepare(
        "SELECT c.app_id, c.title
           FROM catalog_wishlist_entries w
           JOIN catalog_games c ON c.app_id = w.app_id
           LEFT JOIN upcoming_checks k ON k.app_id = c.app_id
          WHERE k.coming_soon IS NULL OR k.coming_soon = 1
          ORDER BY (k.coming_soon = 1) DESC,
                   COALESCE(k.checked_at, '') ASC,
                   c.app_id ASC
          LIMIT ?1",
    )?;
    let rows = statement.query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Cuántos deseados quedan por mirar **en esta vuelta**.
///
/// Los mismos que la cola —los que están por salir y los que no constan— menos
/// los que ya se miraron hace poco. Sin ese «hace poco» el número no bajaba
/// nunca: se pulsaba «Recalcular», se revisaban sesenta y la frase seguía
/// diciendo «quedan 429», que es exactamente la clase de recuento que no
/// coincide con lo que acaba de pasar.
///
/// Pasadas doce horas todos vuelven a contar, porque una fecha de salida
/// mirada ayer ya no dice nada de hoy.
fn pending_count(database: &Database) -> AppResult<u32> {
    let connection = database.open()?;
    Ok(connection.query_row(
        "SELECT COUNT(*)
           FROM catalog_wishlist_entries w
           LEFT JOIN upcoming_checks k ON k.app_id = w.app_id
          WHERE (k.coming_soon IS NULL OR k.coming_soon = 1)
            AND (k.checked_at IS NULL
                 OR k.checked_at = ''
                 OR k.checked_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1))",
        [format!("-{AUTO_INTERVAL_HOURS} hours")],
        |row| row.get(0),
    )?)
}

/// Deja constancia de que estos ya se han mirado, aunque no fueran candidatos.
///
/// Sin esto, los que ya han salido volverían a encabezar la cola en cada pasada
/// y las siguientes nunca llegarían al resto de la lista.
fn mark_checked(database: &Database, candidates: &[(u32, String)]) -> AppResult<()> {
    if candidates.is_empty() {
        return Ok(());
    }
    let mut connection = database.open()?;
    let transaction = connection.transaction()?;
    {
        let mut remember = transaction.prepare(
            "INSERT INTO upcoming_checks(app_id, checked_at)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(app_id) DO UPDATE
                SET checked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        for (app_id, _) in candidates {
            remember.execute([app_id])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

/// Cuántos juegos del escaparate se miran como mucho en una pasada.
///
/// La sección «próximamente» devuelve una decena; el tope está para que un día
/// que devuelva muchos más no convierta la pasada en un barrido.
const MAX_SHOWCASE_PER_RUN: usize = 12;

/// Un juego tal y como lo publica el escaparate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShowcaseGame {
    pub app_id: u32,
    pub title: String,
}

/// Qué dejó la pasada del escaparate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowcaseReport {
    /// Juegos que destacaba la tienda.
    pub received: u32,
    /// Los que ya tenías o ya estaban en la lista: no se vuelven a preguntar.
    pub already_known: u32,
    /// Fichas pedidas para confirmar que siguen sin salir.
    pub checked: u32,
    /// Los que entraron como candidatos.
    pub inserted: u32,
    /// Los que ya estaban y se han refrescado.
    pub updated: u32,
    /// Descartados por no ser un juego —demos y DLC— o por haber salido ya.
    pub skipped: u32,
}

/// Lo que la tienda destaca como «próximamente».
pub async fn fetch_showcase() -> AppResult<Vec<ShowcaseGame>> {
    let client = crate::stores::net::client()?;
    let response = client
        .get(crate::steam::deals::FEATURED_ENDPOINT)
        .query(&[("cc", "ES"), ("l", "spanish")])
        .send()
        .await
        .map_err(|_| {
            AppError::new(
                "steam_showcase_unreachable",
                "No se pudo preguntar a la tienda por sus próximos lanzamientos.",
            )
        })?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "steam_showcase_http",
            format!(
                "La tienda respondió {} al pedir sus próximos lanzamientos.",
                response.status().as_u16()
            ),
        ));
    }
    let bytes = response.bytes().await.map_err(|_| {
        AppError::new(
            "steam_showcase_body",
            "La tienda cortó la respuesta de sus próximos lanzamientos.",
        )
    })?;
    parse_showcase(&bytes)
}

/// Lee la sección «próximamente» del escaparate.
///
/// Sólo se queda con el identificador y el nombre: lo demás —si es un juego, si
/// de verdad no ha salido— lo dice la ficha, y preguntarlo es el paso siguiente.
pub fn parse_showcase(bytes: &[u8]) -> AppResult<Vec<ShowcaseGame>> {
    let raiz: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_showcase_invalid_json",
            "La tienda devolvió una respuesta que no se puede leer.",
        )
    })?;
    let Some(items) = raiz
        .pointer("/coming_soon/items")
        .and_then(serde_json::Value::as_array)
    else {
        // Que no venga la sección no es un fallo: se sigue con lo que haya.
        return Ok(Vec::new());
    };
    let mut salida = Vec::new();
    let mut vistos = std::collections::BTreeSet::new();
    for item in items {
        let Some(app_id) = item
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
        else {
            continue;
        };
        let Some(title) = item
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if vistos.insert(app_id) {
            salida.push(ShowcaseGame {
                app_id,
                title: title.to_string(),
            });
        }
    }
    Ok(salida)
}

/// Trae los próximos lanzamientos que la tienda destaca y no son tuyos.
pub async fn refresh_from_showcase(database: &Database) -> AppResult<ShowcaseReport> {
    let destacados = fetch_showcase().await?;
    let mut report = ShowcaseReport {
        received: destacados.len() as u32,
        ..ShowcaseReport::default()
    };

    let pendientes: Vec<ShowcaseGame> = {
        let connection = database.open()?;
        let mut conocido = connection.prepare(
            "SELECT EXISTS(SELECT 1 FROM games WHERE app_id = ?1)
                 OR EXISTS(SELECT 1 FROM upcoming_releases WHERE app_id = ?1)",
        )?;
        let mut salida = Vec::new();
        for juego in destacados {
            let ya: bool = conocido.query_row([juego.app_id], |row| {
                row.get::<_, i64>(0).map(|value| value != 0)
            })?;
            if ya {
                report.already_known = report.already_known.saturating_add(1);
            } else if salida.len() < MAX_SHOWCASE_PER_RUN {
                salida.push(juego);
            }
        }
        salida
    };

    let mut items: Vec<ImportedUpcomingRelease> = Vec::new();
    for (indice, juego) in pendientes.iter().enumerate() {
        if indice > 0 {
            sleep(BETWEEN_REQUESTS).await;
        }
        report.checked = report.checked.saturating_add(1);
        match store_api::fetch_bundle_with_retry_hint(juego.app_id).await {
            Ok(StoreBundleOutcome::Found(bundle)) => {
                // El escaparate mezcla demos y DLC con los juegos, y ninguna de
                // las dos cosas es un lanzamiento que esperar. Un tipo que la
                // ficha no declara tampoco cuenta como juego: no se sabe.
                let es_juego = bundle.app_type.as_deref() == Some("game");
                if !es_juego || !bundle.release.coming_soon {
                    report.skipped = report.skipped.saturating_add(1);
                    continue;
                }
                let metadata = &bundle.metadata;
                let dia = bundle
                    .release
                    .label
                    .as_deref()
                    .and_then(store_api::exact_release_day);
                items.push(ImportedUpcomingRelease {
                    app_id: juego.app_id,
                    title: juego.title.clone(),
                    capsule_url: metadata.capsule_url.clone(),
                    header_url: metadata.header_url.clone(),
                    release_date: dia.clone().or_else(|| bundle.release.label.clone()),
                    release_date_is_exact: dia.is_some(),
                    genres: metadata.genres.clone(),
                    categories: metadata.categories.clone(),
                    developer: metadata.developer.clone(),
                    publisher: metadata.publisher.clone(),
                    short_description: metadata.short_description.as_deref().map(recortar),
                    source: SOURCE.to_string(),
                });
            }
            Ok(StoreBundleOutcome::Unavailable) => {
                report.skipped = report.skipped.saturating_add(1);
            }
            // Un fallo de red corta la pasada y guarda lo reunido.
            Err(_) => break,
        }
    }

    let summary: UpcomingImportSummary =
        crate::db::priority::upsert_upcoming(&mut database.open()?, &items)?;
    report.inserted = summary.inserted;
    report.updated = summary.updated;
    Ok(report)
}

/// Cuántos candidatos se vuelven a preguntar como mucho por pasada.
const MAX_REVISIT_PER_RUN: u32 = 20;

/// Qué dejó la revisión de los candidatos ya guardados.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisitReport {
    /// Candidatos a los que se volvió a preguntar.
    pub checked: u32,
    /// Los que ya han salido y dejan de figurar como próximos.
    pub retired: u32,
    /// Los que siguen sin salir y se han refrescado con lo que dice hoy la
    /// tienda: fecha, géneros y estudio.
    pub refreshed: u32,
}

/// Vuelve a preguntar por los candidatos ya guardados.
///
/// # Por qué existe
///
/// Nadie los revisaba. La pasada de deseados se salta los ya publicados pero no
/// borraba los que ya estaban, y un hallazgo del escaparate no está en deseados,
/// así que no se miraba nunca más: ni para retirarlo al salir, ni para
/// enterarse de que la fecha ha cambiado.
///
/// Un juego que sale **no** se descarta: descartar enseña al modelo que no te
/// interesa, y salir no dice nada de tu gusto. Se retira y ya.
pub async fn revisit_candidates(database: &Database) -> AppResult<RevisitReport> {
    let candidatos = crate::db::priority::upcoming_to_revisit(
        &database.open()?,
        Utc::now(),
        MAX_REVISIT_PER_RUN,
    )?;
    let mut report = RevisitReport::default();
    let mut publicados: Vec<u32> = Vec::new();
    let mut vigentes: Vec<ImportedUpcomingRelease> = Vec::new();
    let mut preguntados: Vec<(u32, String)> = Vec::new();

    for (indice, (app_id, titulo)) in candidatos.iter().enumerate() {
        if indice > 0 {
            sleep(BETWEEN_REQUESTS).await;
        }
        report.checked = report.checked.saturating_add(1);
        match store_api::fetch_bundle_with_retry_hint(*app_id).await {
            Ok(StoreBundleOutcome::Found(bundle)) => {
                preguntados.push((*app_id, String::new()));
                if !bundle.release.coming_soon {
                    publicados.push(*app_id);
                    continue;
                }
                let metadata = &bundle.metadata;
                let dia = bundle
                    .release
                    .label
                    .as_deref()
                    .and_then(store_api::exact_release_day);
                vigentes.push(ImportedUpcomingRelease {
                    app_id: *app_id,
                    // El título se conserva: la ficha no lo devuelve en este
                    // paquete y el que ya está guardado es el bueno.
                    title: titulo.clone(),
                    capsule_url: metadata.capsule_url.clone(),
                    header_url: metadata.header_url.clone(),
                    release_date: dia.clone().or_else(|| bundle.release.label.clone()),
                    release_date_is_exact: dia.is_some(),
                    genres: metadata.genres.clone(),
                    categories: metadata.categories.clone(),
                    developer: metadata.developer.clone(),
                    publisher: metadata.publisher.clone(),
                    short_description: metadata.short_description.as_deref().map(recortar),
                    source: SOURCE.to_string(),
                });
            }
            // Una ficha que la tienda ya no devuelve no prueba que haya salido:
            // también puede estar retirada. No se toca.
            Ok(StoreBundleOutcome::Unavailable) => {
                preguntados.push((*app_id, String::new()));
            }
            Err(_) => break,
        }
    }

    report.retired = crate::db::priority::retire_upcoming(&mut database.open()?, &publicados)?;
    if !vigentes.is_empty() {
        let resumen = crate::db::priority::upsert_upcoming(&mut database.open()?, &vigentes)?;
        report.refreshed = resumen.updated.saturating_add(resumen.inserted);
    }
    mark_checked(database, &preguntados)?;
    Ok(report)
}

/// Cada cuánto se repasa la lista de deseados por su cuenta.
///
/// Doce horas: los lanzamientos no cambian de fecha cada rato, y cada pasada
/// son sesenta peticiones a la tienda. Menos sería gastar red para nada; más,
/// enterarse tarde de que algo ya ha salido.
const AUTO_INTERVAL_HOURS: i64 = 12;

/// Cada cuánto se vuelve cuando **queda cola**.
///
/// Una pasada mira sesenta deseados. Con 429 por salir eso son ocho pasadas, y
/// a doce horas por pasada la lista tardaba cuatro días en cubrirse entera: la
/// sección enseñaba 112 candidatos de los 429 posibles y nada indicaba que
/// faltara nada. Mientras haya atrasados se vuelve cada media hora, que cubre
/// la lista en una mañana; cuando no queda ninguno se vuelve a las doce horas.
const CATCH_UP_MINUTES: i64 = 30;

/// Clave en `app_settings` con la última pasada automática.
const LAST_AUTO_KEY: &str = "upcoming_last_auto_refresh";

/// Repasa los próximos lanzamientos si toca, sin que nadie lo pida.
///
/// # Por qué automático
///
/// La sección existía entera —traer los deseados que aún no han salido,
/// aprender qué te gusta y puntuarlos— y sólo corría al pulsar un botón que
/// nadie sabía que había que pulsar. Medido sobre una biblioteca real con
/// novecientos deseados: cero lanzamientos guardados y cero pesos aprendidos.
/// Una recomendación que hay que pedir a mano no recomienda nada.
///
/// # Qué hace exactamente, y en qué orden
///
/// 1. Trae una tanda de deseados y guarda los que aún no han salido.
/// 2. Aprende del historial qué te gusta.
/// 3. Puntúa los candidatos contra ese modelo.
///
/// El orden importa: puntuar antes de aprender daría la puntuación del modelo
/// anterior. Nada de esto sale del ordenador salvo preguntar a la tienda por
/// fechas de salida, que es lo mismo que ya hace la sincronización.
pub async fn maintain_if_due(database: &Database) -> AppResult<Option<UpcomingRefreshReport>> {
    if !is_due(database)? {
        return Ok(None);
    }
    // Antes de pedir fichas una a una, saber a quién hay que pedírselas.
    //
    // El índice de la tienda contesta por lotes de doscientos: los 1.345
    // deseados son siete peticiones, y con eso apuntado la cola de fichas deja
    // de gastar siete de cada diez en juegos ya publicados. Si falla, la pasada
    // sigue con el orden que tuviera: es una mejora del reparto, no un paso
    // imprescindible.
    let _ = crate::steam::release_state::refresh_wishlist_release_state(database).await;
    let report = refresh_from_wishlist(database).await?;
    // Y lo que la tienda destaca, que es lo único de esta sección que puede
    // enseñar algo que no hubieras marcado tú. Si falla, la pasada sigue: los
    // deseados ya están traídos y sería absurdo tirarlos.
    let _ = refresh_from_showcase(database).await;
    // Y los ya guardados se vuelven a preguntar: los que han salido se retiran
    // y los que siguen esperando se refrescan con lo que dice hoy la tienda.
    let _ = revisit_candidates(database).await;
    // Aprender y puntuar son baratos y locales: se hacen aunque la tanda no
    // haya traído nada nuevo, porque el historial de juego sí ha cambiado.
    database.learn_taste()?;
    database.score_upcoming_releases()?;
    mark_done(database)?;
    Ok(Some(report))
}

fn is_due(database: &Database) -> AppResult<bool> {
    let connection = database.open()?;
    let last: Option<String> = connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [LAST_AUTO_KEY],
            |row| row.get(0),
        )
        .optional()?;
    drop(connection);
    let Some(last) = last else {
        return Ok(true);
    };
    // Un sello que no se entiende es un sello que no sirve: se vuelve a pasar.
    let Ok(moment) = chrono::DateTime::parse_from_rfc3339(&last) else {
        return Ok(true);
    };
    // Con cola atrasada se vuelve pronto; sin ella, a la cadencia de siempre.
    let espera = if pending_count(database)? > 0 {
        chrono::Duration::minutes(CATCH_UP_MINUTES)
    } else {
        chrono::Duration::hours(AUTO_INTERVAL_HOURS)
    };
    Ok(Utc::now().signed_duration_since(moment.with_timezone(&Utc)) >= espera)
}

fn mark_done(database: &Database) -> AppResult<()> {
    let connection = database.open()?;
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![LAST_AUTO_KEY, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// El escaparate mezcla juegos con demos y con DLC.
    ///
    /// Los tres llegan con el mismo `type: 0` en la lista, así que aquí no se
    /// puede distinguir: lo único que se lee es el identificador y el nombre, y
    /// quien decide si es un juego es la ficha, un paso más allá.
    #[test]
    fn lee_los_destacados_sin_inventarse_lo_que_no_dice_la_lista() {
        let bytes = serde_json::to_vec(&serde_json::json!({
          "coming_soon": { "items": [
            { "id": 2124360, "name": "HYPER PRIMATE", "type": 0 },
            { "id": 5063510, "name": "僵尸驾到 Demo", "type": 0 },
            { "id": 2124360, "name": "HYPER PRIMATE (repetido)", "type": 0 },
            { "id": 0, "name": "Sin identificador" },
            { "id": 42, "name": "   " }
          ] }
        }))
        .expect("serializar");

        let juegos = super::parse_showcase(&bytes).expect("analizar");
        assert_eq!(juegos.len(), 2, "{juegos:?}");
        assert_eq!(juegos[0].app_id, 2_124_360);
        assert_eq!(juegos[0].title, "HYPER PRIMATE");
        // El repetido no entra dos veces, y lo que no tiene identificador o
        // nombre no entra: sin ellos no hay nada que preguntar.
        assert_eq!(juegos[1].app_id, 5_063_510);
    }

    #[test]
    fn una_respuesta_sin_esa_seccion_no_es_un_fallo() {
        // Que la tienda deje de publicar «próximamente» no puede tumbar la
        // pasada: los deseados siguen siendo una fuente.
        assert!(super::parse_showcase(b"{}").expect("analizar").is_empty());
    }

    /// Contra la tienda de verdad. Apagada por defecto.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml --lib -- --ignored contra_la_tienda_el_escaparate
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_la_tienda_el_escaparate_sigue_teniendo_esta_forma() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let juegos = runtime
            .block_on(super::fetch_showcase())
            .expect("la tienda responde");
        assert!(!juegos.is_empty(), "la sección «próximamente» trae algo");
        for juego in &juegos {
            assert!(juego.app_id > 0);
            assert!(!juego.title.trim().is_empty());
        }
    }

    #[test]
    fn la_primera_vez_siempre_toca() {
        // Sin sello no hay nada aprendido: es justo el caso que dejaba la
        // sección vacía para siempre.
        let (_directory, database) = base_para_vencimiento();
        assert!(is_due(&database).expect("comprobar"));
    }

    fn base_para_vencimiento() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar");
        (directory, database)
    }

    #[test]
    fn recien_repasado_no_se_repite() {
        let (_directory, database) = base_para_vencimiento();
        mark_done(&database).expect("marcar");
        assert!(!is_due(&database).expect("comprobar"));
    }

    #[test]
    fn un_sello_ilegible_se_trata_como_si_no_hubiera() {
        // Un valor que no se entiende no puede significar «ya está hecho»:
        // sería quedarse sin repasar para siempre por un dato corrupto.
        let (_directory, database) = base_para_vencimiento();
        let connection = database.open().expect("abrir");
        connection
            .execute(
                "INSERT INTO app_settings(key, value) VALUES (?1, 'ayer por la tarde')",
                [LAST_AUTO_KEY],
            )
            .expect("escribir");
        drop(connection);
        assert!(is_due(&database).expect("comprobar"));
    }

    use super::*;
    use tempfile::TempDir;

    fn base() -> (TempDir, Database) {
        let directory = TempDir::new().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("migrar");
        (directory, database)
    }

    fn deseado(database: &Database, app_id: u32, title: &str) {
        let connection = database.open().expect("abrir");
        connection
            .execute(
                "INSERT INTO catalog_games(app_id, title) VALUES (?1, ?2)",
                rusqlite::params![app_id, title],
            )
            .expect("catálogo");
        connection
            .execute(
                "INSERT INTO catalog_wishlist_entries(app_id) VALUES (?1)",
                [app_id],
            )
            .expect("deseado");
    }

    #[test]
    fn la_cola_empieza_por_lo_que_nunca_se_ha_mirado() {
        let (_directory, database) = base();
        for (app_id, title) in [(10_u32, "Uno"), (20, "Dos"), (30, "Tres")] {
            deseado(&database, app_id, title);
        }
        // Dos ya revisados: deben quedar detrás del que no se ha mirado nunca.
        mark_checked(&database, &[(10, "Uno".into()), (30, "Tres".into())]).expect("marcar");

        let orden = pending_candidates(&database, 10).expect("cola");
        assert_eq!(orden.first().map(|(id, _)| *id), Some(20));
        assert_eq!(orden.len(), 3, "los revisados siguen en la lista, al final");
    }

    #[test]
    fn revisar_un_deseado_ya_publicado_tambien_cuenta() {
        // Es lo que impide que la cola se atasque: si sólo se anotaran los que
        // están por salir, los ya publicados volverían a encabezarla en cada
        // pasada y las siguientes nunca llegarían al resto de la lista.
        let (_directory, database) = base();
        deseado(&database, 40, "Ya salió");

        mark_checked(&database, &[(40, "Ya salió".into())]).expect("marcar");

        let anotado: i64 = database
            .open()
            .expect("abrir")
            .query_row(
                "SELECT COUNT(*) FROM upcoming_checks WHERE app_id = 40",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(anotado, 1);
    }

    #[test]
    fn un_resumen_largo_se_recorta_por_palabra_y_sin_romper_letras() {
        // Cortar por byte partiría una letra acentuada por la mitad; cortar por
        // carácter exacto parte la palabra. Se corta por palabra y se avisa con
        // puntos suspensivos de que hay más.
        let corto = "Un resumen corto.";
        assert_eq!(recortar(corto), corto);

        let largo = "áéíóú ".repeat(500);
        let recortado = recortar(&largo);
        assert!(recortado.chars().count() <= MAX_DESCRIPTION_CHARS);
        assert!(recortado.ends_with('…'));
        assert!(
            !recortado.contains("  "),
            "no queda un espacio suelto al final"
        );
        // Y sigue siendo texto legible, no bytes partidos.
        assert!(recortado.starts_with("áéíóú"));
    }

    /// La cola de fichas va a los que están por salir, no a la lista entera.
    ///
    /// Pedía la ficha de uno en uno rotando por los 1.345 deseados, y siete de
    /// cada diez peticiones se gastaban en juegos ya publicados que se
    /// descartaban al llegar: cubrir la lista eran veintitrés pasadas. Con el
    /// estado de publicación apuntado —que el índice contesta por lotes— la
    /// cola va directa. `NULL` es «no se sabe» y espera turno detrás; un juego
    /// publicado no se vuelve a preguntar.
    #[test]
    fn la_cola_pregunta_primero_por_los_que_estan_por_salir() {
        let (_directory, database) = base();
        for (app_id, title) in [
            (10_u32, "Ya publicado"),
            (20, "Sin saber"),
            (30, "Por salir"),
        ] {
            deseado(&database, app_id, title);
        }
        database
            .record_wishlist_release_state(&[(10, false), (30, true)])
            .expect("apuntar el estado");

        let cola = pending_candidates(&database, 10).expect("cola");
        let ids: Vec<u32> = cola.iter().map(|(app_id, _)| *app_id).collect();
        assert_eq!(
            ids,
            vec![30, 20],
            "primero el que consta por salir, después el que no consta, y el publicado nunca"
        );

        // Y el recuento de pendientes cuenta lo mismo que la cola: decir 3
        // cuando sólo se van a preguntar 2 es prometer trabajo que no existe.
        assert_eq!(pending_count(&database).expect("pendientes"), 2);
    }

    /// El recuento baja cuando se mira, y vuelve a subir cuando caduca.
    ///
    /// La frase que lo enseña dice «vuelve a recalcular para seguir». Si el
    /// número no bajaba nunca, esa frase mandaba a repetir un trabajo ya hecho
    /// y no había forma de saber cuándo se había terminado.
    #[test]
    fn los_ya_mirados_dejan_de_contar_hasta_que_caducan() {
        let (_directory, database) = base();
        for (app_id, title) in [(30_u32, "Por salir"), (40, "También por salir")] {
            deseado(&database, app_id, title);
        }
        database
            .record_wishlist_release_state(&[(30, true), (40, true)])
            .expect("apuntar el estado");
        assert_eq!(
            pending_count(&database).expect("pendientes"),
            2,
            "recién apuntados, ninguno se ha mirado todavía"
        );

        mark_checked(&database, &[(30, "Por salir".to_string())]).expect("marcar");
        assert_eq!(
            pending_count(&database).expect("pendientes"),
            1,
            "el que se acaba de mirar no vuelve a contar"
        );

        // Y pasado el ciclo vuelve a contar: una fecha mirada ayer no dice nada
        // de hoy.
        let connection = database.open().expect("abrir");
        connection
            .execute(
                "UPDATE upcoming_checks
                    SET checked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-13 hours')
                  WHERE app_id = 30",
                [],
            )
            .expect("envejecer la marca");
        drop(connection);
        assert_eq!(pending_count(&database).expect("pendientes"), 2);
    }

    #[test]
    fn una_lista_vacia_no_toca_nada() {
        let (_directory, database) = base();
        assert!(mark_checked(&database, &[]).is_ok());
        assert!(pending_candidates(&database, 10).expect("cola").is_empty());
    }
}
