//! Rebajas de GOG, traídas de su catálogo público.
//!
//! # Por qué GOG está aquí
//!
//! Vende **sin DRM por definición**, que es exactamente lo que esta biblioteca
//! mira. Y su catálogo público resuelve en una sola petición lo que en Steam
//! cuesta una ficha por juego: título, género, estudio, imagen, enlace y precio.
//!
//! # El extremo
//!
//! `catalog.gog.com/v1/catalog` admite país, idioma y moneda, no pide clave y
//! responde en JSON. Comprobado el 20 de agosto de 2026 con `countryCode=ES`:
//! doce productos rebajados con todo lo anterior.
//!
//! # Lo que se descarta
//!
//! Lo que no es un juego —paquetes, bandas sonoras, DLC sueltos— y lo que no
//! trae precio con moneda: un importe sin moneda no es un precio.

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{AppError, AppResult};

const CATALOG_ENDPOINT: &str = "https://catalog.gog.com/v1/catalog";

/// Anfitriones desde los que GOG sirve las carátulas de su catálogo.
///
/// # Por qué hay una lista y no basta con «https»
///
/// La política de contenido de la ventana sólo pinta imágenes de anfitriones
/// declarados. Una carátula de un anfitrión que no esté en `img-src` **no
/// falla**: se queda en blanco, y eso es indistinguible de un juego sin
/// carátula. Pasó con `gog-statics.com`, que es donde GOG sirve las del
/// catálogo mientras que la biblioteca usa `images.gog.com`.
///
/// Guardar una dirección que la ventana no va a pintar es guardar una promesa
/// que no se cumple, así que se descarta aquí. Y hay una prueba que compara
/// esta lista con la `img-src` de `tauri.conf.json`, porque las dos tienen que
/// decir lo mismo.
pub const IMAGE_HOSTS: [&str; 2] = ["gog.com", "gog-statics.com"];

/// La dirección de una carátula que la ventana sí puede pintar.
///
/// Se analiza entera y se mira el anfitrión: comparar el principio de la cadena
/// dejaría pasar `https://gog.com.attacker.tld/`.
fn imagen_pintable(raw: &str) -> Option<String> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    IMAGE_HOSTS
        .iter()
        .any(|base| host == *base || host.ends_with(&format!(".{base}")))
        .then(|| raw.to_string())
}

/// Cuántas se piden. La sección enseña unas pocas; pedir cien sería traer
/// noventa que nadie va a mirar.
const CATALOG_LIMIT: u32 = 24;

/// Una rebaja de GOG, ya normalizada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GogDeal {
    /// Identificador del producto en GOG. No es un AppID de Steam.
    pub product_id: String,
    pub title: String,
    pub store_url: String,
    pub image_url: Option<String>,
    pub final_cents: i64,
    pub initial_cents: i64,
    pub discount_percent: u8,
    pub currency: String,
    /// Rasgos para puntuar, que GOG entrega con la propia oferta.
    pub genres: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
}

/// Trae las rebajas vigentes del catálogo.
pub async fn fetch(country: &str, currency: &str) -> AppResult<Vec<GogDeal>> {
    let country = normalize_code(country, 2, "El país de la tienda")?;
    let currency = normalize_code(currency, 3, "La moneda")?;
    let client = super::net::client()?;
    let response = client
        .get(CATALOG_ENDPOINT)
        .query(&[
            ("limit", CATALOG_LIMIT.to_string()),
            ("order", "desc:discount".to_string()),
            ("discounted", "eq:true".to_string()),
            ("productType", "in:game".to_string()),
            ("page", "1".to_string()),
            ("countryCode", country),
            ("locale", "es-ES".to_string()),
            ("currencyCode", currency),
        ])
        .send()
        .await
        .map_err(|_| {
            AppError::new(
                "gog_deals_unreachable",
                "No se pudo preguntar a GOG por sus rebajas.",
            )
        })?;
    if !response.status().is_success() {
        return Err(AppError::new(
            "gog_deals_http",
            format!(
                "GOG respondió {} al pedir sus rebajas.",
                response.status().as_u16()
            ),
        ));
    }
    let bytes = response.bytes().await.map_err(|_| {
        AppError::new(
            "gog_deals_body",
            "La respuesta de GOG se cortó antes de terminar.",
        )
    })?;
    parse(&bytes)
}

fn normalize_code(value: &str, largo: usize, que: &str) -> AppResult<String> {
    let limpio = value.trim().to_uppercase();
    if limpio.len() != largo || !limpio.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::validation(format!(
            "{que} debe ser un código de {largo} letras."
        )));
    }
    Ok(limpio)
}

/// Analiza el catálogo. Separado de la red: es la parte que se rompe sola.
pub fn parse(bytes: &[u8]) -> AppResult<Vec<GogDeal>> {
    let raiz: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| {
        AppError::new(
            "gog_deals_invalid_json",
            "GOG devolvió una respuesta que no se puede leer.",
        )
    })?;
    let productos = raiz
        .get("products")
        .and_then(serde_json::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(productos.iter().filter_map(parse_product).collect())
}

fn parse_product(product: &serde_json::Value) -> Option<GogDeal> {
    // Sólo juegos: los paquetes y las bandas sonoras no son lo que se busca.
    if product
        .get("productType")
        .and_then(serde_json::Value::as_str)
        != Some("game")
    {
        return None;
    }
    let product_id = product
        .get("id")
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_i64().map(|id| id.to_string()))
        })
        .filter(|id| !id.trim().is_empty())?;
    let title = product
        .get("title")
        .and_then(serde_json::Value::as_str)?
        .trim()
        .to_string();
    if title.is_empty() {
        return None;
    }

    let price = product.get("price")?;
    let final_cents = money_cents(price.get("finalMoney"))?;
    // Sin precio base, el vigente hace de base: no hay descuento que inventar.
    let initial_cents = money_cents(price.get("baseMoney")).unwrap_or(final_cents);
    let currency = price
        .get("finalMoney")
        .and_then(|money| money.get("currency"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| value.len() == 3)?
        .to_uppercase();
    // La misma regla que en Steam: sin un precio base mayor que el vigente no
    // hay rebaja que comprobar, por mucho que el catálogo la haya devuelto en
    // la sección de rebajados.
    if initial_cents <= final_cents {
        return None;
    }
    // GOG publica el descuento como «-95%»; se lee el número y se acota.
    let discount_percent = price
        .get("discount")
        .and_then(serde_json::Value::as_str)
        .and_then(|texto| {
            texto
                .trim_start_matches('-')
                .trim_end_matches('%')
                .parse::<i64>()
                .ok()
        })
        .unwrap_or_else(|| {
            if initial_cents > 0 && initial_cents > final_cents {
                ((initial_cents - final_cents) * 100) / initial_cents
            } else {
                0
            }
        })
        .clamp(0, 100) as u8;

    let store_url = product
        .get("storeLink")
        .and_then(serde_json::Value::as_str)
        .filter(|url| url.starts_with("https://www.gog.com/"))
        .map(str::to_string)?;
    let image_url = product
        .get("coverHorizontal")
        .or_else(|| product.get("coverVertical"))
        .and_then(serde_json::Value::as_str)
        .and_then(imagen_pintable);

    let genres = product
        .get("genres")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("name").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let primero = |clave: &str| -> Option<String> {
        product
            .get(clave)
            .and_then(serde_json::Value::as_array)
            .and_then(|items| items.first())
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .filter(|value| !value.trim().is_empty())
    };

    Some(GogDeal {
        product_id,
        title,
        store_url,
        image_url,
        final_cents,
        initial_cents,
        discount_percent,
        currency,
        genres,
        developer: primero("developers"),
        publisher: primero("publishers"),
    })
}

/// Importe en la unidad mínima de la moneda.
///
/// GOG lo publica como texto decimal («1.49»). Se convierte multiplicando por
/// cien y redondeando, no truncando: truncar convertiría 1,49 € en 1,48 €.
fn money_cents(money: Option<&serde_json::Value>) -> Option<i64> {
    let bruto = money?.get("amount")?;
    let valor = bruto
        .as_f64()
        .or_else(|| bruto.as_str().and_then(|texto| texto.parse::<f64>().ok()))?;
    if !valor.is_finite() || valor < 0.0 {
        return None;
    }
    Some((valor * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Forma real del catálogo, reducida.
    fn respuesta() -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
          "products": [
            {
              "id": 1859396546,
              "title": "Leisure Suit Larry - Wet Dreams Don't Dry",
              "productType": "game",
              "storeLink": "https://www.gog.com/en/game/leisure_suit_larry_wet_dreams_dont_dry",
              "coverHorizontal": "https://images.gog-statics.com/abc.png",
              "price": {
                "finalMoney": { "amount": "1.49", "currency": "EUR" },
                "baseMoney": { "amount": "29.99", "currency": "EUR" },
                "discount": "-95%"
              },
              "genres": [ { "name": "Adventure" }, { "name": "Point-and-click" } ],
              "developers": ["CrazyBunch"],
              "publishers": ["Assemble Entertainment"]
            },
            {
              "id": 111,
              "title": "Una banda sonora",
              "productType": "pack",
              "storeLink": "https://www.gog.com/en/game/soundtrack",
              "price": { "finalMoney": { "amount": "1.00", "currency": "EUR" } }
            },
            {
              "id": 222,
              "title": "Sin moneda",
              "productType": "game",
              "storeLink": "https://www.gog.com/en/game/sin_moneda",
              "price": { "finalMoney": { "amount": "1.00" } }
            },
            {
              "id": 333,
              "title": "Con enlace ajeno",
              "productType": "game",
              "storeLink": "https://otro-sitio.example/game",
              "price": { "finalMoney": { "amount": "1.00", "currency": "EUR" } }
            }
          ]
        }))
        .expect("serializar")
    }

    #[test]
    fn lee_una_rebaja_con_todo_lo_que_hace_falta_para_puntuarla() {
        let ofertas = parse(&respuesta()).expect("analizar");
        assert_eq!(ofertas.len(), 1, "{ofertas:?}");
        let larry = &ofertas[0];
        assert_eq!(larry.product_id, "1859396546");
        assert_eq!(larry.final_cents, 149);
        assert_eq!(larry.initial_cents, 2999);
        assert_eq!(larry.discount_percent, 95);
        assert_eq!(larry.currency, "EUR");
        assert_eq!(larry.genres, ["Adventure", "Point-and-click"]);
        assert_eq!(larry.developer.as_deref(), Some("CrazyBunch"));
    }

    #[test]
    fn descarta_lo_que_no_es_un_juego_o_no_se_puede_valorar() {
        // Un paquete no es lo que se busca, un importe sin moneda no es un
        // precio, y un enlace fuera de GOG no se abre en su navegador.
        let ofertas = parse(&respuesta()).expect("analizar");
        assert!(
            !ofertas
                .iter()
                .any(|deal| deal.title.contains("banda sonora"))
        );
        assert!(!ofertas.iter().any(|deal| deal.title.contains("Sin moneda")));
        assert!(!ofertas.iter().any(|deal| deal.title.contains("ajeno")));
    }

    #[test]
    fn los_importes_se_redondean_en_vez_de_truncarse() {
        // Truncar convertiría 1,49 € en 1,48 €, y un precio equivocado por un
        // céntimo sigue siendo un precio equivocado.
        assert_eq!(
            money_cents(Some(&serde_json::json!({ "amount": "1.49" }))),
            Some(149)
        );
        assert_eq!(
            money_cents(Some(&serde_json::json!({ "amount": 19.99 }))),
            Some(1999)
        );
        assert_eq!(
            money_cents(Some(&serde_json::json!({ "amount": "-1" }))),
            None
        );
        assert_eq!(money_cents(None), None);
    }

    #[test]
    fn sin_porcentaje_publicado_se_calcula_del_precio() {
        let bytes = serde_json::to_vec(&serde_json::json!({
          "products": [ {
            "id": 9, "title": "Sin porcentaje", "productType": "game",
            "storeLink": "https://www.gog.com/en/game/x",
            "price": {
              "finalMoney": { "amount": "5.00", "currency": "EUR" },
              "baseMoney": { "amount": "20.00", "currency": "EUR" }
            }
          } ]
        }))
        .expect("serializar");
        assert_eq!(parse(&bytes).expect("analizar")[0].discount_percent, 75);
    }

    #[test]
    fn un_pais_o_una_moneda_inventados_se_rechazan_antes_de_salir_a_la_red() {
        assert!(normalize_code("España", 2, "El país").is_err());
        assert!(normalize_code("EU", 3, "La moneda").is_err());
        assert_eq!(normalize_code(" es ", 2, "El país").unwrap(), "ES");
    }

    /// Contra GOG de verdad. Apagada por defecto.
    ///
    /// ```text
    /// cargo test --manifest-path src-tauri/Cargo.toml -- --ignored contra_gog
    /// ```
    #[test]
    #[ignore = "sale a la red: se ejecuta a mano"]
    fn contra_gog_de_verdad_el_catalogo_sigue_teniendo_esta_forma() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let ofertas = runtime
            .block_on(fetch("ES", "EUR"))
            .expect("preguntar a GOG");
        assert!(!ofertas.is_empty(), "GOG siempre tiene rebajas");
        for oferta in &ofertas {
            assert!(!oferta.title.trim().is_empty());
            assert!(oferta.store_url.starts_with("https://www.gog.com/"));
            assert_eq!(oferta.currency, "EUR");
            assert!(oferta.initial_cents >= oferta.final_cents);
        }
    }

    /// La carátula que se guarda tiene que poder pintarse.
    ///
    /// GOG sirve las del catálogo desde `gog-statics.com`, que no estaba en la
    /// `img-src` de la ventana: la dirección se guardaba, la fila la pedía y el
    /// navegador la bloqueaba en silencio. Quedaba el hueco con las iniciales,
    /// que es exactamente lo que se enseña cuando **no hay** carátula.
    #[test]
    fn solo_se_guarda_una_caratula_que_la_ventana_puede_pintar() {
        assert_eq!(
            imagen_pintable("https://images.gog-statics.com/abc.png").as_deref(),
            Some("https://images.gog-statics.com/abc.png")
        );
        assert_eq!(
            imagen_pintable("https://images.gog.com/abc.png").as_deref(),
            Some("https://images.gog.com/abc.png")
        );
        // Un anfitrión que sólo termina pareciéndose no hereda el permiso, y sin
        // cifrar tampoco: comparar el principio de la cadena dejaba pasar las dos.
        assert_eq!(
            imagen_pintable("https://gog.com.attacker.tld/abc.png"),
            None
        );
        assert_eq!(imagen_pintable("https://evilgog-statics.com/abc.png"), None);
        assert_eq!(imagen_pintable("http://images.gog.com/abc.png"), None);
        assert_eq!(imagen_pintable("https://cdn.otra-tienda.com/abc.png"), None);
    }

    /// Y la lista de aquí tiene que coincidir con la de la ventana.
    ///
    /// Son dos archivos distintos —este y `tauri.conf.json`— y el desajuste no
    /// da error: da un hueco en blanco. Por eso se comparan.
    #[test]
    fn la_politica_de_contenido_permite_esos_mismos_anfitriones() {
        let configuracion = include_str!("../../tauri.conf.json");
        let raiz: serde_json::Value =
            serde_json::from_str(configuracion).expect("tauri.conf.json es JSON válido");
        let csp = raiz["app"]["security"]["csp"]
            .as_str()
            .expect("la configuración declara una política de contenido");
        for host in IMAGE_HOSTS {
            let comodin = format!("https://*.{host}");
            assert!(
                csp.contains(&comodin),
                "«{comodin}» falta en img-src: la carátula se guardaría y no se pintaría"
            );
        }
    }
}
