//! Los juegos que Epic regala cada semana.
//!
//! # Por qué está aquí
//!
//! Epic regala un juego (a veces dos) cada jueves, y la promoción caduca. Quien
//! no se acuerda de pasar por la tienda, lo pierde. Vindexa ya sabe lo que se
//! tiene, así que puede decir dos cosas que la propia tienda no dice: **qué hay
//! gratis ahora** y **si ya lo tienes**.
//!
//! # Cómo se consigue, sin iniciar sesión
//!
//! `store-site-backend-static.ak.epicgames.com/freeGamesPromotions` es el mismo
//! extremo público que alimenta la portada de la tienda: no pide clave ni
//! sesión, responde en JSON y admite país e idioma. Comprobado el 19 de agosto
//! de 2026 con `country=ES`: once elementos, uno gratis en ese momento y siete
//! anunciados.
//!
//! # Qué no hace
//!
//! **No reclama el juego por ti.** Reclamar exige la sesión de Epic y pasar por
//! su flujo de compra; automatizarlo sería suplantar a quien usa esto en una
//! operación de cuenta. Lo que hace es llevarte a la página exacta en el
//! navegador integrado, donde ya estás identificado, a un clic de «Obtener».
//!
//! # Qué significa cada fecha
//!
//! Una promoción tiene principio y fin. Sin fin declarado no se dice «caduca
//! pronto»: se dice que no se sabe. Un juego sin ventana de promoción no es un
//! juego gratis, es un juego que aparece en la respuesta por otros motivos, y
//! se descarta.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Extremo público de las promociones. No admite clave y no la necesita.
const PROMOTIONS_ENDPOINT: &str =
    "https://store-site-backend-static.ak.epicgames.com/freeGamesPromotions";

/// Tope de respuesta. La real ronda los 30 KiB; esto es un cortafuegos.
const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// En qué momento de su promoción está un juego.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FreeGameState {
    /// Gratis ahora mismo.
    Current,
    /// Anunciado: será gratis más adelante.
    Upcoming,
    /// Su ventana ya pasó.
    ///
    /// La tienda no devuelve esto: sale de comparar la ventana guardada con el
    /// reloj, y existe para no llamar «anunciado» a lo que ya terminó ni
    /// ofrecer reclamar algo que hoy cuesta dinero. En la tabla no se guarda
    /// nunca —ahí sólo hay dos estados—, así que el `CHECK` de la migración
    /// sigue valiendo tal cual.
    Expired,
}

/// Un juego regalado por Epic, tal y como Vindexa lo enseña.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicFreeGame {
    /// Identificador estable de Epic. Es la clave con la que se recuerda si ya
    /// se avisó de esta promoción.
    pub offer_id: String,
    pub title: String,
    pub description: String,
    /// Dirección de la ficha en la tienda, ya montada para el idioma pedido.
    pub store_url: String,
    /// Imagen ancha si la hay; `None` si Epic no la publica.
    pub image_url: Option<String>,
    pub state: FreeGameState,
    /// Principio y fin de la ventana en que es gratis.
    pub starts_at: Option<String>,
    pub ends_at: Option<String>,
    /// Lo que costaría fuera de la promoción, en la unidad mínima de la moneda.
    pub original_price_cents: Option<i64>,
    pub currency: Option<String>,
}

impl EpicFreeGame {
    /// Horas que faltan para que se acabe la promoción, si se sabe.
    ///
    /// `None` es «no se sabe», y se dice así: una promoción sin fecha de fin no
    /// se convierte en «caduca hoy» por comodidad de la interfaz.
    pub fn hours_left(&self, now: DateTime<Utc>) -> Option<i64> {
        let ends = self.ends_at.as_deref()?;
        let moment = DateTime::parse_from_rfc3339(ends).ok()?.with_timezone(&Utc);
        let left = moment.signed_duration_since(now).num_hours();
        (left >= 0).then_some(left)
    }
}

/// Trae las promociones vigentes y anunciadas.
///
/// `country` es el código ISO de dos letras del país de la tienda y `locale` el
/// idioma en el que se quieren los textos.
pub async fn fetch(country: &str, locale: &str) -> AppResult<Vec<EpicFreeGame>> {
    let country = normalize_country(country)?;
    let client = super::net::client()?;
    let response = client
        .get(PROMOTIONS_ENDPOINT)
        .query(&[
            ("locale", locale),
            ("country", country.as_str()),
            ("allowCountries", country.as_str()),
        ])
        .send()
        .await
        .map_err(|_| {
            AppError::new(
                "epic_free_unreachable",
                "No se pudo preguntar a Epic por los juegos gratis de esta semana.",
            )
        })?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "epic_free_http",
            format!(
                "Epic respondió {} al preguntar por los juegos gratis.",
                response.status().as_u16()
            ),
        ));
    }
    let bytes = response.bytes().await.map_err(|_| {
        AppError::new(
            "epic_free_body",
            "La respuesta de Epic se cortó antes de terminar.",
        )
    })?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AppError::new(
            "epic_free_too_large",
            "La respuesta de Epic es mayor de lo razonable y no se ha analizado.",
        ));
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| {
        AppError::new(
            "epic_free_invalid_json",
            "La respuesta de Epic no es un JSON que se pueda leer.",
        )
    })?;
    Ok(parse(&value, locale))
}

fn normalize_country(country: &str) -> AppResult<String> {
    let trimmed = country.trim().to_uppercase();
    if trimmed.len() != 2 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::validation(
            "El país de la tienda debe ser un código de dos letras.",
        ));
    }
    Ok(trimmed)
}

/// Convierte la respuesta de Epic en la lista que Vindexa enseña.
///
/// Vive separada de la red para poder comprobarla con una respuesta guardada:
/// el formato de Epic es la parte que se rompe sola, no la petición.
pub fn parse(value: &serde_json::Value, locale: &str) -> Vec<EpicFreeGame> {
    let elements = value
        .pointer("/data/Catalog/searchStore/elements")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut juegos = Vec::new();
    for element in elements {
        let Some(juego) = parse_element(element, locale) else {
            continue;
        };
        juegos.push(juego);
    }
    // Primero lo que ya está gratis, y dentro de cada grupo lo que antes acaba:
    // el orden de la respuesta de Epic no significa nada.
    juegos.sort_by(|left, right| {
        (left.state != FreeGameState::Current)
            .cmp(&(right.state != FreeGameState::Current))
            .then_with(|| left.ends_at.cmp(&right.ends_at))
            .then_with(|| left.title.cmp(&right.title))
    });
    juegos
}

fn parse_element(element: &serde_json::Value, locale: &str) -> Option<EpicFreeGame> {
    let title = element.get("title")?.as_str()?.trim().to_string();
    if title.is_empty() {
        return None;
    }
    let offer_id = element
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(title.as_str())
        .to_string();

    let (state, starts_at, ends_at) = promotion_window(element)?;

    // El precio original sirve para decir cuánto se ahorra. Sin él no se
    // inventa una cifra: se calla.
    let total = element.pointer("/price/totalPrice");
    let original_price_cents = total
        .and_then(|price| price.get("originalPrice"))
        .and_then(serde_json::Value::as_i64)
        .filter(|cents| *cents > 0);
    let currency = total
        .and_then(|price| price.get("currencyCode"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    let description = element
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string();

    Some(EpicFreeGame {
        offer_id,
        store_url: store_url(element, locale),
        title,
        description,
        image_url: image_url(element),
        state,
        starts_at,
        ends_at,
        original_price_cents,
        currency,
    })
}

/// Ventana de promoción: en curso o anunciada.
///
/// Devuelve `None` cuando el elemento no tiene ninguna, que es como Epic marca
/// lo que aparece en la respuesta sin ser un regalo.
fn promotion_window(
    element: &serde_json::Value,
) -> Option<(FreeGameState, Option<String>, Option<String>)> {
    let promotions = element.get("promotions")?;
    for (clave, state) in [
        ("promotionalOffers", FreeGameState::Current),
        ("upcomingPromotionalOffers", FreeGameState::Upcoming),
    ] {
        let grupos = promotions.get(clave).and_then(serde_json::Value::as_array);
        let Some(grupos) = grupos else { continue };
        for grupo in grupos {
            let ofertas = grupo
                .get("promotionalOffers")
                .and_then(serde_json::Value::as_array);
            let Some(ofertas) = ofertas else { continue };
            for oferta in ofertas {
                // Sólo cuenta el descuento total: un 20 % no es un regalo.
                let porcentaje = oferta
                    .pointer("/discountSetting/discountPercentage")
                    .and_then(serde_json::Value::as_i64);
                if porcentaje.is_some_and(|valor| valor != 0) {
                    continue;
                }
                let inicio = oferta
                    .get("startDate")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let fin = oferta
                    .get("endDate")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                return Some((state, inicio, fin));
            }
        }
    }
    None
}

/// Dirección de la ficha. Epic publica el fragmento en varios sitios según el
/// tipo de producto; se usa el primero que exista y, si no hay ninguno, se
/// lleva a la búsqueda de la tienda en vez de inventar una URL rota.
fn store_url(element: &serde_json::Value, locale: &str) -> String {
    let slug = element
        .pointer("/catalogNs/mappings")
        .and_then(serde_json::Value::as_array)
        .and_then(|mappings| mappings.first())
        .and_then(|mapping| mapping.get("pageSlug"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            element
                .get("offerMappings")
                .and_then(serde_json::Value::as_array)
                .and_then(|mappings| mappings.first())
                .and_then(|mapping| mapping.get("pageSlug"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            element
                .get("productSlug")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| element.get("urlSlug").and_then(serde_json::Value::as_str))
        .map(str::trim)
        .filter(|slug| !slug.is_empty() && !slug.contains(['/', '?', '#', ' ']));

    let locale = locale_path(locale);
    match slug {
        Some(slug) => format!("https://store.epicgames.com/{locale}/p/{slug}"),
        None => {
            let titulo = element
                .get("title")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            format!(
                "https://store.epicgames.com/{locale}/browse?q={}",
                urlencoding_lite(titulo)
            )
        }
    }
}

/// Epic usa `es-ES` en la ruta; un idioma que no cuadre se degrada a inglés en
/// vez de producir una ruta inválida.
fn locale_path(locale: &str) -> String {
    let limpio = locale.trim();
    if limpio.len() >= 2
        && limpio
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        limpio.to_string()
    } else {
        "en-US".to_string()
    }
}

/// Codificación mínima para el término de búsqueda. No se usa una dependencia
/// nueva para tres caracteres.
fn urlencoding_lite(value: &str) -> String {
    let mut salida = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                salida.push(*byte as char);
            }
            b' ' => salida.push('+'),
            other => salida.push_str(&format!("%{other:02X}")),
        }
    }
    salida
}

/// Imagen ancha, si la hay. Se prefiere el formato apaisado porque el bloque de
/// la interfaz es una franja, no una carátula.
fn image_url(element: &serde_json::Value) -> Option<String> {
    let imagenes = element
        .get("keyImages")
        .and_then(serde_json::Value::as_array)?;
    let preferencias = [
        "OfferImageWide",
        "DieselStoreFrontWide",
        "VaultClosed",
        "OfferImageTall",
        "Thumbnail",
    ];
    for preferida in preferencias {
        for imagen in imagenes {
            // Sólo `https`: una imagen por texto plano en una ventana de
            // aplicación es una fuga de tráfico y un aviso del sistema.
            if imagen.get("type").and_then(serde_json::Value::as_str) == Some(preferida)
                && let Some(url) = imagen.get("url").and_then(serde_json::Value::as_str)
                && url.starts_with("https://")
            {
                return Some(url.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Respuesta reducida con la forma real de Epic, tomada del extremo público
    /// el 19 de agosto de 2026. Ninguna prueba de este módulo toca la red.
    fn respuesta() -> serde_json::Value {
        serde_json::json!({
          "data": { "Catalog": { "searchStore": { "elements": [
            {
              "title": "Caravan SandWitch",
              "id": "oferta-1",
              "description": "Vuelve a casa.",
              "price": { "totalPrice": { "originalPrice": 2499, "discountPrice": 0, "currencyCode": "EUR" } },
              "keyImages": [
                { "type": "Thumbnail", "url": "https://cdn.epic/thumb.jpg" },
                { "type": "OfferImageWide", "url": "https://cdn.epic/wide.jpg" }
              ],
              "catalogNs": { "mappings": [ { "pageSlug": "caravan-sandwitch-05ff58" } ] },
              "promotions": {
                "promotionalOffers": [ { "promotionalOffers": [
                  { "startDate": "2026-08-14T15:00:00.000Z", "endDate": "2026-08-21T15:00:00.000Z",
                    "discountSetting": { "discountType": "PERCENTAGE", "discountPercentage": 0 } }
                ] } ],
                "upcomingPromotionalOffers": []
              }
            },
            {
              "title": "Ghostrunner 2",
              "id": "oferta-2",
              "description": "Correr por las paredes.",
              "price": { "totalPrice": { "originalPrice": 3999, "discountPrice": 3999, "currencyCode": "EUR" } },
              "keyImages": [],
              "catalogNs": { "mappings": [ { "pageSlug": "ghostrunner-2" } ] },
              "promotions": {
                "promotionalOffers": [],
                "upcomingPromotionalOffers": [ { "promotionalOffers": [
                  { "startDate": "2026-08-21T15:00:00.000Z", "endDate": "2026-08-28T15:00:00.000Z",
                    "discountSetting": { "discountType": "PERCENTAGE", "discountPercentage": 0 } }
                ] } ]
              }
            },
            {
              "title": "Un juego con 20 % de descuento",
              "id": "oferta-3",
              "price": { "totalPrice": { "originalPrice": 1999, "discountPrice": 1599, "currencyCode": "EUR" } },
              "promotions": {
                "promotionalOffers": [ { "promotionalOffers": [
                  { "startDate": "2026-08-14T15:00:00.000Z", "endDate": "2026-08-21T15:00:00.000Z",
                    "discountSetting": { "discountType": "PERCENTAGE", "discountPercentage": 80 } }
                ] } ]
              }
            },
            {
              "title": "Un juego sin promoción ninguna",
              "id": "oferta-4",
              "price": { "totalPrice": { "originalPrice": 1999, "discountPrice": 1999, "currencyCode": "EUR" } },
              "promotions": null
            }
          ] } } }
        })
    }

    #[test]
    fn solo_entran_los_regalos_de_verdad() {
        // Un 80 % de descuento no es un regalo, y un elemento sin ventana de
        // promoción tampoco: los dos aparecen en la respuesta de Epic.
        let juegos = parse(&respuesta(), "es-ES");
        let titulos: Vec<&str> = juegos.iter().map(|juego| juego.title.as_str()).collect();
        assert_eq!(titulos, ["Caravan SandWitch", "Ghostrunner 2"]);
    }

    #[test]
    fn lo_que_ya_esta_gratis_va_primero() {
        let juegos = parse(&respuesta(), "es-ES");
        assert_eq!(juegos[0].state, FreeGameState::Current);
        assert_eq!(juegos[1].state, FreeGameState::Upcoming);
    }

    #[test]
    fn la_direccion_lleva_a_la_ficha_en_el_idioma_pedido() {
        let juegos = parse(&respuesta(), "es-ES");
        assert_eq!(
            juegos[0].store_url,
            "https://store.epicgames.com/es-ES/p/caravan-sandwitch-05ff58"
        );
    }

    #[test]
    fn sin_ficha_conocida_se_lleva_a_la_busqueda_en_vez_de_a_una_url_rota() {
        let elemento = serde_json::json!({
          "title": "Juego Sin Slug",
          "id": "x",
          "promotions": { "promotionalOffers": [ { "promotionalOffers": [
            { "endDate": "2026-08-21T15:00:00.000Z",
              "discountSetting": { "discountPercentage": 0 } }
          ] } ] }
        });
        let valor = serde_json::json!({ "data": { "Catalog": { "searchStore": { "elements": [elemento] } } } });
        let juegos = parse(&valor, "es-ES");
        assert_eq!(
            juegos[0].store_url,
            "https://store.epicgames.com/es-ES/browse?q=Juego+Sin+Slug"
        );
    }

    #[test]
    fn se_prefiere_la_imagen_apaisada_y_solo_por_https() {
        let juegos = parse(&respuesta(), "es-ES");
        assert_eq!(
            juegos[0].image_url.as_deref(),
            Some("https://cdn.epic/wide.jpg")
        );
    }

    #[test]
    fn una_imagen_sin_cifrar_no_se_usa() {
        let elemento = serde_json::json!({
          "title": "Con imagen insegura",
          "id": "y",
          "keyImages": [ { "type": "OfferImageWide", "url": "http://cdn.epic/wide.jpg" } ],
          "promotions": { "promotionalOffers": [ { "promotionalOffers": [
            { "endDate": "2026-08-21T15:00:00.000Z", "discountSetting": { "discountPercentage": 0 } }
          ] } ] }
        });
        let valor = serde_json::json!({ "data": { "Catalog": { "searchStore": { "elements": [elemento] } } } });
        assert_eq!(parse(&valor, "es-ES")[0].image_url, None);
    }

    #[test]
    fn las_horas_que_faltan_se_calculan_y_se_callan_si_no_se_saben() {
        let juegos = parse(&respuesta(), "es-ES");
        let ahora = Utc
            .with_ymd_and_hms(2026, 8, 19, 15, 0, 0)
            .single()
            .unwrap();
        assert_eq!(juegos[0].hours_left(ahora), Some(48));

        let sin_fin = EpicFreeGame {
            ends_at: None,
            ..juegos[0].clone()
        };
        assert_eq!(sin_fin.hours_left(ahora), None);
    }

    #[test]
    fn una_promocion_ya_terminada_no_dice_que_quedan_horas_negativas() {
        let juegos = parse(&respuesta(), "es-ES");
        let tarde = Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).single().unwrap();
        assert_eq!(juegos[0].hours_left(tarde), None);
    }

    #[test]
    fn un_pais_inventado_se_rechaza_antes_de_salir_a_la_red() {
        assert!(normalize_country("España").is_err());
        assert_eq!(normalize_country(" es ").unwrap(), "ES");
    }

    /// Contra Epic de verdad.
    ///
    /// Apagada por defecto —una prueba no sale a la red sin permiso— y
    /// encendida a mano cuando hace falta comprobar que el formato de Epic
    /// sigue siendo el que este módulo entiende:
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_epic
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_epic_de_verdad_el_formato_sigue_siendo_este() {
        // Sin `#[tokio::test]`: este proyecto no incluye las macros de tokio, y
        // añadir una dependencia para una prueba manual sería peor negocio que
        // levantar el runtime aquí mismo.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let juegos = runtime
            .block_on(fetch("ES", "es-ES"))
            .expect("preguntar a Epic");
        assert!(
            !juegos.is_empty(),
            "Epic siempre tiene algo anunciado o vigente"
        );
        for juego in &juegos {
            assert!(!juego.title.trim().is_empty());
            assert!(
                juego.store_url.starts_with("https://store.epicgames.com/"),
                "{}",
                juego.store_url
            );
            assert!(juego.ends_at.is_some(), "{} sin fecha de fin", juego.title);
        }
    }

    #[test]
    fn una_respuesta_vacia_no_inventa_regalos() {
        assert!(parse(&serde_json::json!({}), "es-ES").is_empty());
    }
}
