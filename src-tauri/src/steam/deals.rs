//! Las rebajas de la tienda, traídas sin sesión y sin clave.
//!
//! # De dónde salen
//!
//! `store.steampowered.com/api/featuredcategories` es el mismo extremo que
//! alimenta la portada de la tienda. No pide clave, responde en JSON y admite
//! país e idioma. Comprobado el 19 de agosto de 2026 con `cc=ES`: diez rebajas
//! con precio, descuento y carátula ancha.
//!
//! # Lo que trae y lo que no
//!
//! Trae identificador, título, imagen, precio final, precio de referencia,
//! descuento y moneda. **No** trae géneros, categorías ni estudio, que es
//! precisamente lo que hace falta para saber si una oferta te interesa. Eso se
//! pide aparte, una vez por juego, y se guarda.
//!
//! # Por qué no se analiza el HTML de la búsqueda
//!
//! `store.steampowered.com/search/results?json=1` devuelve diez mil resultados,
//! pero en HTML: analizar la maquetación de una tienda es construir algo que se
//! rompe cuando ellos cambian una clase de CSS. Este extremo devuelve datos.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::steam::store_api::{STORE_COUNTRY, StoreMetadataFailure};

/// El escaparate público de la tienda. Sin clave y sin sesión.
///
/// Lo comparten el radar de ofertas y los próximos lanzamientos: es la misma
/// respuesta, y cada uno lee la sección que le toca.
pub(crate) const FEATURED_ENDPOINT: &str = "https://store.steampowered.com/api/featuredcategories";

/// De qué escaparate viene una oferta.
///
/// No se mezclan: una rebaja del 80 % y un superventas a precio completo son
/// cosas distintas, y presentarlas juntas convertiría la sección en un
/// escaparate en vez de en una recomendación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DealSource {
    Specials,
    TopSellers,
    NewReleases,
}

impl DealSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Specials => "specials",
            Self::TopSellers => "top_sellers",
            Self::NewReleases => "new_releases",
        }
    }

    /// Clave de la sección dentro de la respuesta.
    fn key(self) -> &'static str {
        match self {
            Self::Specials => "specials",
            Self::TopSellers => "top_sellers",
            Self::NewReleases => "new_releases",
        }
    }
}

/// Una oferta tal y como la publica la tienda.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreDeal {
    pub app_id: u32,
    pub title: String,
    pub header_url: Option<String>,
    pub final_cents: i64,
    pub initial_cents: i64,
    pub discount_percent: u8,
    pub currency: String,
    pub source: DealSource,
}

/// Trae las rebajas vigentes.
pub async fn fetch(sources: &[DealSource]) -> Result<Vec<StoreDeal>, StoreMetadataFailure> {
    let client = crate::stores::net::client().map_err(StoreMetadataFailure::from)?;
    let response = client
        .get(FEATURED_ENDPOINT)
        .query(&[("cc", STORE_COUNTRY), ("l", "spanish")])
        .send()
        .await
        .map_err(|_| {
            StoreMetadataFailure::from(AppError::new(
                "steam_deals_unreachable",
                "No se pudo preguntar a la tienda por las rebajas.",
            ))
        })?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "steam_deals_http",
            format!(
                "La tienda respondió {} al pedir las rebajas.",
                response.status().as_u16()
            ),
        )
        .into());
    }
    let bytes = response.bytes().await.map_err(|_| {
        StoreMetadataFailure::from(AppError::new(
            "steam_deals_body",
            "La respuesta de la tienda se cortó antes de terminar.",
        ))
    })?;
    parse(&bytes, sources).map_err(StoreMetadataFailure::from)
}

/// Analiza la respuesta. Separado de la red: es la parte que se rompe sola.
pub fn parse(bytes: &[u8], sources: &[DealSource]) -> AppResult<Vec<StoreDeal>> {
    let raiz: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "steam_deals_invalid_json",
            "La tienda devolvió una respuesta que no se puede leer.",
        )
    })?;

    let mut salida = Vec::new();
    for source in sources {
        let items = raiz
            .pointer(&format!("/{}/items", source.key()))
            .and_then(serde_json::Value::as_array);
        let Some(items) = items else { continue };
        for item in items {
            let Some(deal) = parse_item(item, *source) else {
                continue;
            };
            salida.push(deal);
        }
    }
    // Un juego puede salir en dos escaparates; se queda la entrada de mayor
    // descuento, que es la que motiva mirarlo.
    salida.sort_by(|left, right| {
        left.app_id
            .cmp(&right.app_id)
            .then(right.discount_percent.cmp(&left.discount_percent))
    });
    salida.dedup_by_key(|deal| deal.app_id);
    Ok(salida)
}

fn parse_item(item: &serde_json::Value, source: DealSource) -> Option<StoreDeal> {
    let app_id = item.get("id").and_then(serde_json::Value::as_u64)?;
    let app_id = u32::try_from(app_id).ok().filter(|value| *value > 0)?;
    let title = item
        .get("name")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }
    let final_cents = item
        .get("final_price")
        .and_then(serde_json::Value::as_i64)
        .filter(|cents| *cents >= 0)?;
    // Sin precio de referencia, el vigente hace de referencia: no hay descuento
    // que inventar.
    let initial_cents = item
        .get("original_price")
        .and_then(serde_json::Value::as_i64)
        .filter(|cents| *cents >= final_cents)
        .unwrap_or(final_cents);
    let currency = item
        .get("currency")
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() == 3)?
        .to_uppercase();
    // Un juego a precio completo no es una oferta. El escaparate de más
    // vendidos mezcla las dos cosas, y enseñar un 49,99 € sin rebaja bajo el
    // título «Ofertas para ti» es una promesa que la fila no cumple. Sin un
    // precio de referencia **mayor** tampoco: «−25 %» junto a dos importes
    // iguales es un descuento que nadie puede comprobar.
    if initial_cents <= final_cents {
        return None;
    }
    let discount_percent = item
        .get("discount_percent")
        .and_then(serde_json::Value::as_i64)
        .filter(|percent| *percent > 0)
        // A veces la rebaja llega sólo en los importes; se calcula con ellos
        // en vez de enseñar un cero al lado de dos precios distintos.
        .unwrap_or(((initial_cents - final_cents) * 100) / initial_cents)
        .clamp(0, 100) as u8;

    let header_url = item
        .get("header_image")
        .or_else(|| item.get("large_capsule_image"))
        .or_else(|| item.get("small_capsule_image"))
        .and_then(serde_json::Value::as_str)
        // Sólo `https`: la imagen se pinta dentro de la ventana de la aplicación.
        .filter(|url| url.starts_with("https://"))
        .map(str::to_string);

    Some(StoreDeal {
        app_id,
        title,
        header_url,
        final_cents,
        initial_cents,
        discount_percent,
        currency,
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forma real de la respuesta, reducida.
    fn respuesta() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
          "specials": { "items": [
            { "id": 1771300, "name": "Kingdom Come: Deliverance II",
              "discount_percent": 60, "original_price": 5999, "final_price": 2399,
              "currency": "EUR",
              "header_image": "https://cdn.steam/1771300/header.jpg" },
            { "id": 2358720, "name": "Black Myth: Wukong",
              "discount_percent": 30, "original_price": 5999, "final_price": 4199,
              "currency": "EUR",
              "large_capsule_image": "https://cdn.steam/2358720/capsule.jpg" },
            { "id": 0, "name": "Sin identificador", "final_price": 100, "currency": "EUR" },
            { "id": 55, "name": "Sin moneda", "final_price": 100 }
          ] },
          "top_sellers": { "items": [
            { "id": 730, "name": "Counter-Strike 2", "discount_percent": 20,
              "original_price": 1000, "final_price": 800, "currency": "EUR",
              "header_image": "http://cdn.inseguro/730/header.jpg" },
            { "id": 892970, "name": "Valheim a precio completo",
              "discount_percent": 0, "final_price": 1999, "currency": "EUR" },
            { "id": 1771300, "name": "Kingdom Come: Deliverance II",
              "discount_percent": 0, "final_price": 5999, "currency": "EUR" }
          ] }
        }))
        .expect("serializar")
    }

    #[test]
    fn lee_las_rebajas_con_su_precio_y_su_descuento() {
        let ofertas = parse(&respuesta(), &[DealSource::Specials]).expect("analizar");
        let kingdom = ofertas
            .iter()
            .find(|deal| deal.app_id == 1771300)
            .expect("está");
        assert_eq!(kingdom.final_cents, 2399);
        assert_eq!(kingdom.initial_cents, 5999);
        assert_eq!(kingdom.discount_percent, 60);
        assert_eq!(kingdom.currency, "EUR");
    }

    #[test]
    fn descarta_lo_que_no_se_puede_identificar_o_valorar() {
        // Sin AppID no hay juego, y sin moneda un importe no es un precio.
        let ofertas = parse(&respuesta(), &[DealSource::Specials]).expect("analizar");
        assert_eq!(ofertas.len(), 2, "{ofertas:?}");
    }

    #[test]
    fn una_imagen_sin_cifrar_no_se_usa() {
        let ofertas = parse(&respuesta(), &[DealSource::TopSellers]).expect("analizar");
        let cs = ofertas
            .iter()
            .find(|deal| deal.app_id == 730)
            .expect("está");
        assert_eq!(cs.header_url, None);
    }

    #[test]
    fn lo_que_no_esta_rebajado_no_es_una_oferta() {
        // El escaparate de más vendidos mezcla rebajas con precios completos.
        // Un 49,99 € sin descuento bajo el título «Ofertas para ti» es una
        // promesa que la fila no cumple.
        let ofertas = parse(&respuesta(), &[DealSource::TopSellers]).expect("analizar");
        assert!(
            !ofertas.iter().any(|deal| deal.app_id == 892_970),
            "un precio completo no entra: {ofertas:?}"
        );
        assert!(
            ofertas
                .iter()
                .all(|deal| deal.discount_percent > 0 || deal.initial_cents > deal.final_cents)
        );
    }

    #[test]
    fn un_juego_en_dos_escaparates_se_queda_con_el_mayor_descuento() {
        let ofertas = parse(
            &respuesta(),
            &[DealSource::Specials, DealSource::TopSellers],
        )
        .expect("analizar");
        let repetido: Vec<_> = ofertas
            .iter()
            .filter(|deal| deal.app_id == 1771300)
            .collect();
        assert_eq!(repetido.len(), 1);
        assert_eq!(repetido[0].discount_percent, 60);
    }

    #[test]
    fn sin_precio_de_referencia_no_hay_rebaja_que_ensenar() {
        // Antes se dejaba pasar con «0 %» y el precio repetido a los dos lados.
        // Una fila así ocupa sitio en una sección de ofertas sin ser una.
        let bytes = serde_json::to_vec(&serde_json::json!({
          "specials": { "items": [
            { "id": 42, "name": "Sin referencia", "final_price": 999, "currency": "EUR" }
          ] }
        }))
        .expect("serializar");
        let ofertas = parse(&bytes, &[DealSource::Specials]).expect("analizar");
        assert!(ofertas.is_empty(), "{ofertas:?}");
    }

    #[test]
    fn una_rebaja_que_solo_viene_en_los_importes_se_calcula() {
        let bytes = serde_json::to_vec(&serde_json::json!({
          "specials": { "items": [
            { "id": 42, "name": "Sin porcentaje", "original_price": 2000,
              "final_price": 1500, "currency": "EUR" }
          ] }
        }))
        .expect("serializar");
        let ofertas = parse(&bytes, &[DealSource::Specials]).expect("analizar");
        assert_eq!(ofertas[0].discount_percent, 25);
    }

    #[test]
    fn una_seccion_que_no_viene_no_es_un_fallo() {
        let ofertas = parse(b"{}", &[DealSource::Specials]).expect("analizar");
        assert!(ofertas.is_empty());
    }

    /// Contra la tienda de verdad. Apagada por defecto.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_la_tienda_las_rebajas
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_la_tienda_las_rebajas_siguen_teniendo_esta_forma() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let ofertas = runtime
            .block_on(fetch(&[DealSource::Specials]))
            .expect("preguntar a la tienda");
        assert!(!ofertas.is_empty(), "la tienda siempre tiene rebajas");
        for oferta in &ofertas {
            assert!(oferta.app_id > 0);
            assert!(!oferta.title.trim().is_empty());
            assert_eq!(oferta.currency.len(), 3);
            assert!(oferta.initial_cents >= oferta.final_cents);
        }
    }
}
