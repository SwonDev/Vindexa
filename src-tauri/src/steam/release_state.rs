//! Por qué un juego no tiene precio: no se vende, o **aún no ha salido**.
//!
//! # El fallo que cierra
//!
//! La lista de deseados decía «no a la venta · gratuito, retirado o sin
//! publicar» en trescientos cincuenta y cuatro juegos que sólo estaban
//! esperando su fecha. Medido contra la tienda el 20/08/2026: de diez tomados
//! al azar entre los que no tenían precio, los diez venían con
//! `is_coming_soon`.
//!
//! La pantalla sí sabía decir «aún no ha salido», pero sólo para los que
//! estuvieran en `upcoming_releases`, que es la lista **curada** de candidatos
//! puntuados por el modelo de gustos —112 filas—, no un registro de qué se ha
//! publicado y qué no. De los 453 sin precio, sólo 99 caían ahí.
//!
//! # De dónde sale la respuesta
//!
//! Del mismo índice público y sin clave que resuelve el arte
//! —`IStoreBrowseService/GetItems`—, pidiéndole el bloque `release`. Va por
//! lotes, así que preguntar por los cuatrocientos cincuenta son tres
//! peticiones, no cuatrocientas cincuenta.
//!
//! # Lo que no hace
//!
//! No inventa fechas. `is_coming_soon` es lo único que se guarda: el «cuándo»
//! de un juego sin publicar lo lleva `upcoming_releases`, que además lo puntúa.
//! Y un juego que el índice no resuelve se queda como estaba: no saber por qué
//! no hay precio no es lo mismo que saber que no se vende.

use crate::db::Database;
use crate::db::pricing::{ABSENCE_COMING_SOON, ABSENCE_NO_PRICE};
use crate::error::{AppError, AppResult};
use chrono::Utc;
use serde::Deserialize;

/// Cuántos AppID caben en una pregunta. El mismo lote que usa el índice de arte.
const BATCH: usize = 200;

#[derive(Debug, Deserialize)]
struct ItemsEnvelope {
    response: ItemsResponse,
}

#[derive(Debug, Deserialize)]
struct ItemsResponse {
    #[serde(default)]
    store_items: Vec<StoreItem>,
}

#[derive(Debug, Deserialize)]
struct StoreItem {
    #[serde(default)]
    id: Option<u32>,
    #[serde(default)]
    appid: Option<u32>,
    #[serde(default)]
    success: i64,
    #[serde(default)]
    release: Option<Release>,
}

#[derive(Debug, Deserialize)]
struct Release {
    #[serde(default)]
    is_coming_soon: bool,
}

/// Qué dejó una pasada.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReleaseStateReport {
    /// Juegos por los que se preguntó.
    pub asked: usize,
    /// Juegos que el índice declara sin publicar.
    pub coming_soon: usize,
    /// Juegos publicados: su ausencia de precio es de otra clase.
    pub released: usize,
    /// Lotes que no contestaron. Sus juegos se quedan como estaban.
    pub failed_batches: usize,
}

/// Apunta, para toda la lista de deseados, cuáles están por salir.
///
/// Va por lotes: los 1.345 deseados de una lista real son siete peticiones. Con
/// esto anotado, la pasada que pide la ficha completa —la que trae géneros y
/// descripción para puntuar— puede ir directa a los que están por salir en vez
/// de rotar por todos y descartar a los publicados uno a uno.
pub async fn refresh_wishlist_release_state(database: &Database) -> AppResult<ReleaseStateReport> {
    let deseados = database.wishlist_app_ids()?;
    let mut report = ReleaseStateReport {
        asked: deseados.len(),
        ..ReleaseStateReport::default()
    };
    if deseados.is_empty() {
        return Ok(report);
    }
    for chunk in deseados.chunks(BATCH) {
        let estados = match resolve(chunk).await {
            Ok(estados) => estados,
            Err(_) => {
                report.failed_batches += 1;
                continue;
            }
        };
        for (_, coming) in &estados {
            if *coming {
                report.coming_soon += 1;
            } else {
                report.released += 1;
            }
        }
        database.record_wishlist_release_state(&estados)?;
    }
    Ok(report)
}

/// Pregunta al índice cuáles de estos juegos aún no se han publicado.
pub async fn resolve(app_ids: &[u32]) -> AppResult<Vec<(u32, bool)>> {
    let client = super::wishlist::wishlist_client()?;
    let mut salida = Vec::new();
    for chunk in app_ids.chunks(BATCH) {
        let bytes = super::wishlist::get_bytes(
            client,
            super::wishlist::STORE_ITEMS_ENDPOINT,
            &[("input_json", request_body(chunk))],
        )
        .await?;
        salida.extend(parse(&bytes)?);
    }
    Ok(salida)
}

/// Reclasifica las ausencias de precio que hoy dicen «no se vende».
///
/// Sólo mira las que están apuntadas como `no_price`: una que la tienda no
/// reconoce (`unavailable`) no es un juego por salir, y una con precio no es
/// una ausencia.
pub async fn refresh_absences(database: &Database) -> AppResult<ReleaseStateReport> {
    let pendientes = database.price_absences_to_classify()?;
    let mut report = ReleaseStateReport {
        asked: pendientes.len(),
        ..ReleaseStateReport::default()
    };
    if pendientes.is_empty() {
        return Ok(report);
    }

    for chunk in pendientes.chunks(BATCH) {
        let estados = match resolve(chunk).await {
            Ok(estados) => estados,
            Err(_) => {
                report.failed_batches += 1;
                continue;
            }
        };
        let ahora = Utc::now();
        let (por_salir, publicados): (Vec<_>, Vec<_>) =
            estados.into_iter().partition(|(_, coming)| *coming);
        report.coming_soon += por_salir.len();
        report.released += publicados.len();

        let por_salir: Vec<u32> = por_salir.into_iter().map(|(app_id, _)| app_id).collect();
        let publicados: Vec<u32> = publicados.into_iter().map(|(app_id, _)| app_id).collect();
        if !por_salir.is_empty() {
            database.record_price_absences(&por_salir, ABSENCE_COMING_SOON, ahora)?;
        }
        // Un publicado sin precio sigue siendo un publicado sin precio: se
        // reafirma su respuesta para que la fecha de consulta no envejezca.
        if !publicados.is_empty() {
            database.record_price_absences(&publicados, ABSENCE_NO_PRICE, ahora)?;
        }
    }
    Ok(report)
}

fn request_body(app_ids: &[u32]) -> String {
    let ids = app_ids
        .iter()
        .map(|app_id| format!("{{\"appid\":{app_id}}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"ids\":[{ids}],\"context\":{{\"language\":\"spanish\",\"country_code\":\"ES\"}},\
         \"data_request\":{{\"include_release\":true}}}}"
    )
}

/// Sólo los juegos que el índice resolvió. Un AppID ausente no se toca: no
/// saber por qué no hay precio no es saber que el juego no se vende.
fn parse(bytes: &[u8]) -> AppResult<Vec<(u32, bool)>> {
    let envelope: ItemsEnvelope = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_release_state_response",
            "La tienda de Steam devolvió un estado de publicación que Vindexa no pudo interpretar.",
        )
    })?;
    Ok(envelope
        .response
        .store_items
        .into_iter()
        .filter_map(|item| {
            let app_id = item.appid.or(item.id).filter(|app_id| *app_id > 0)?;
            if item.success != 1 {
                return None;
            }
            let release = item.release?;
            Some((app_id, release.is_coming_soon))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPUESTA: &str = r#"{"response":{"store_items":[
        {"id":4713940,"appid":4713940,"success":1,"visible":true,
         "release":{"steam_release_date":1790000000,"is_coming_soon":true}},
        {"id":620,"appid":620,"success":1,"visible":true,
         "release":{"steam_release_date":1303186800,"is_coming_soon":false}},
        {"id":397080,"appid":397080,"success":15,"visible":false}
    ]}}"#;

    #[test]
    fn distingue_lo_que_esta_por_salir_de_lo_que_ya_salio() {
        let estados = parse(RESPUESTA.as_bytes()).expect("leer respuesta");
        assert_eq!(estados.len(), 2, "el que la tienda no resuelve no se toca");
        assert_eq!(estados[0], (4_713_940, true));
        assert_eq!(estados[1], (620, false));
    }

    #[test]
    fn la_peticion_pide_el_bloque_de_publicacion() {
        let cuerpo = request_body(&[10, 20]);
        assert!(cuerpo.contains("\"include_release\":true"));
        assert!(cuerpo.contains("{\"appid\":10}"));
        assert!(cuerpo.contains("{\"appid\":20}"));
    }

    /// Contra la tienda de verdad.
    ///
    /// Apagada por defecto. Comprueba lo único que se rompe solo: que el índice
    /// sigue diciendo si un juego está por salir.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_la_tienda_el_estado
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_la_tienda_el_estado_de_publicacion_sigue_llegando() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        // Portal 2 salió en 2011; Monster Fantasy estaba sin publicar el
        // 20/08/2026 y es uno de los que la lista marcaba como «retirado».
        let estados = runtime
            .block_on(resolve(&[620, 4_713_940]))
            .expect("preguntar al índice");
        assert_eq!(
            estados.iter().find(|(id, _)| *id == 620).map(|(_, c)| *c),
            Some(false)
        );
        assert_eq!(
            estados
                .iter()
                .find(|(id, _)| *id == 4_713_940)
                .map(|(_, c)| *c),
            Some(true)
        );
    }
}
