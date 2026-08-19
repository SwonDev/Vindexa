//! Rebajas de la tienda, cruzadas con lo tuyo.
//!
//! # La diferencia con un escaparate
//!
//! La tienda ya sabe enseñar rebajas. Lo que no sabe es cuáles te interesan, y
//! eso es lo único que justifica traerlas aquí. Cada oferta se puntúa contra el
//! **mismo modelo de gustos** que ordena los próximos lanzamientos: el que sale
//! de tu historial, se calcula en tu ordenador y no viaja a ningún sitio.
//!
//! # Qué se descarta y por qué
//!
//! - Lo que ya está en la biblioteca: una rebaja de algo que tienes no es una
//!   oferta, es ruido.
//! - Lo que ya está en deseados: eso ya tiene su propia sección, con tu precio
//!   objetivo. Repetirlo sería enseñar la misma rebaja en dos sitios.
//! - Lo descartado a mano.
//!
//! # Lo que no se sabe se dice
//!
//! `match_score` nulo significa «todavía sin puntuar» —hacen falta los géneros,
//! y eso es una petición por juego—, no «no te interesa». La interfaz enseña la
//! diferencia en vez de fingir un cero.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Una oferta lista para guardar, venga de la tienda que venga.
///
/// Las dos tiendas se normalizan aquí porque a partir de este punto todo es
/// igual: se descarta lo que ya es tuyo, se puntúa contra el mismo modelo y se
/// enseña en la misma lista. Lo que cambia —cómo se piden, qué traen de más— se
/// queda en cada lector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingDeal {
    /// `steam` o `gog`.
    pub store: String,
    /// Identificador **en esa tienda**. Los catálogos no comparten numeración.
    pub external_id: String,
    /// AppID de Steam cuando lo hay: es lo que permite enseñar sus capturas.
    pub app_id: Option<u32>,
    pub title: String,
    pub store_url: String,
    pub image_url: Option<String>,
    pub final_cents: i64,
    pub initial_cents: i64,
    pub discount_percent: u8,
    pub currency: String,
    pub source: String,
    /// Rasgos, si la tienda los entrega con la oferta. Vacío significa «hay que
    /// pedirlos aparte», que es el caso de Steam.
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub facets_known: bool,
}

/// Una oferta lista para enseñar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DealCandidate {
    pub store: String,
    pub external_id: String,
    /// AppID de Steam cuando lo hay; `null` en GOG.
    pub app_id: Option<u32>,
    pub title: String,
    pub header_url: Option<String>,
    /// Adónde lleva «abrir en la tienda». Cada tienda tiene la suya.
    pub store_url: String,
    pub final_cents: i64,
    pub initial_cents: i64,
    pub discount_percent: u8,
    pub currency: String,
    pub source: String,
    /// Afinidad con tu historial, de 0 a 100. `null` es «aún sin puntuar».
    pub match_score: Option<f64>,
    /// Por qué esa puntuación, en las mismas palabras que el resto del radar.
    pub match_reason: String,
}

/// Qué dejó una tanda.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DealSyncReport {
    /// Ofertas que devolvió la tienda.
    pub received: u32,
    /// Ofertas nuevas para Vindexa.
    pub discovered: u32,
    /// Descartadas por estar ya en la biblioteca o en deseados.
    pub already_known: u32,
}

/// Guarda una tanda de ofertas, saltándose lo que ya es tuyo.
///
/// La tanda es **de una tienda**: lo que no venga en ella se borra sólo de esa
/// tienda. Si se borrara todo, traer las de GOG haría desaparecer las de Steam.
pub fn sync(
    connection: &mut Connection,
    store: &str,
    deals: &[IncomingDeal],
    now: DateTime<Utc>,
) -> AppResult<DealSyncReport> {
    let mut report = DealSyncReport {
        received: deals.len() as u32,
        ..DealSyncReport::default()
    };
    let sello = now.to_rfc3339();
    let transaction = connection.transaction()?;

    for deal in deals {
        // Lo tuyo no es una oferta que descubrir. Sólo se puede comprobar con
        // AppID de Steam, que es como Vindexa identifica lo que posees; una
        // oferta de GOG sin equivalencia se enseña igual, porque esconderla
        // sería esconder algo que quizá no tienes.
        if let Some(app_id) = deal.app_id {
            let conocido: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM games WHERE app_id = ?1)
                     OR EXISTS(SELECT 1 FROM wishlist_entries WHERE app_id = ?1)
                     OR EXISTS(SELECT 1 FROM catalog_wishlist_entries WHERE app_id = ?1)",
                [app_id],
                |row| row.get(0),
            )?;
            if conocido {
                report.already_known = report.already_known.saturating_add(1);
                continue;
            }
        }

        transaction.execute(
            "INSERT INTO store_deals(
                 store, external_id, app_id, title, store_url, image_url,
                 final_cents, initial_cents, discount_percent, currency, source,
                 genres_json, categories_json, developer, publisher,
                 facets_fetched_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
             ON CONFLICT(store, external_id) DO UPDATE SET
                 app_id = excluded.app_id,
                 title = excluded.title,
                 store_url = excluded.store_url,
                 image_url = excluded.image_url,
                 final_cents = excluded.final_cents,
                 initial_cents = excluded.initial_cents,
                 discount_percent = excluded.discount_percent,
                 currency = excluded.currency,
                 source = excluded.source,
                 -- Los rasgos sólo se pisan cuando la tanda los trae: en Steam
                 -- llegan después, y sobrescribirlos con vacío obligaría a
                 -- pedirlos otra vez en cada pasada.
                 genres_json = CASE WHEN ?16 IS NULL THEN genres_json ELSE excluded.genres_json END,
                 categories_json = CASE WHEN ?16 IS NULL THEN categories_json ELSE excluded.categories_json END,
                 developer = CASE WHEN ?16 IS NULL THEN developer ELSE excluded.developer END,
                 publisher = CASE WHEN ?16 IS NULL THEN publisher ELSE excluded.publisher END,
                 facets_fetched_at = COALESCE(excluded.facets_fetched_at, facets_fetched_at),
                 updated_at = excluded.updated_at",
            params![
                deal.store,
                deal.external_id,
                deal.app_id,
                deal.title,
                deal.store_url,
                deal.image_url,
                deal.final_cents,
                deal.initial_cents,
                i64::from(deal.discount_percent),
                deal.currency,
                deal.source,
                serde_json::to_string(&deal.genres).unwrap_or_else(|_| "[]".to_string()),
                serde_json::to_string(&deal.categories).unwrap_or_else(|_| "[]".to_string()),
                deal.developer,
                deal.publisher,
                deal.facets_known.then(|| sello.clone()),
                sello,
            ],
        )?;
        let recien: bool = transaction.query_row(
            "SELECT first_seen_at >= ?3 FROM store_deals WHERE store = ?1 AND external_id = ?2",
            params![deal.store, deal.external_id, sello],
            |row| row.get(0),
        )?;
        if recien {
            report.discovered = report.discovered.saturating_add(1);
        }
    }

    // Lo que ya no está rebajado deja de estar: una oferta caducada que sigue en
    // pantalla lleva a la tienda a pagar el precio completo creyendo que hay
    // descuento. Sólo se limpia la tienda de esta tanda.
    let vigentes: Vec<&String> = deals.iter().map(|deal| &deal.external_id).collect();
    if vigentes.is_empty() {
        transaction.execute("DELETE FROM store_deals WHERE store = ?1", [store])?;
    } else {
        let marcadores = vigentes.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
        let sql =
            format!("DELETE FROM store_deals WHERE store = ? AND external_id NOT IN ({marcadores})");
        let mut referencias: Vec<&dyn rusqlite::ToSql> = vec![&store];
        for id in &vigentes {
            referencias.push(*id as &dyn rusqlite::ToSql);
        }
        transaction.execute(&sql, referencias.as_slice())?;
    }

    transaction.commit()?;
    Ok(report)
}

/// Ofertas de Steam a las que aún no se les han pedido los rasgos.
///
/// Sólo de Steam: GOG los entrega con la propia oferta, así que nunca están
/// pendientes.
pub fn pending_facets(connection: &Connection, limit: u32) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM store_deals
          WHERE store = 'steam' AND app_id IS NOT NULL
            AND facets_fetched_at IS NULL AND dismissed_at IS NULL
          ORDER BY discount_percent DESC, app_id ASC
          LIMIT ?1",
    )?;
    let ids = statement
        .query_map([limit], |row| row.get::<_, u32>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

/// Guarda los rasgos de una oferta para poder puntuarla.
pub fn save_facets(
    connection: &Connection,
    app_id: u32,
    genres: &[String],
    categories: &[String],
    developer: Option<&str>,
    publisher: Option<&str>,
    now: DateTime<Utc>,
) -> AppResult<()> {
    connection.execute(
        "UPDATE store_deals
            SET genres_json = ?2,
                categories_json = ?3,
                developer = ?4,
                publisher = ?5,
                facets_fetched_at = ?6
          WHERE store = 'steam' AND app_id = ?1",
        params![
            app_id,
            serde_json::to_string(genres).unwrap_or_else(|_| "[]".to_string()),
            serde_json::to_string(categories).unwrap_or_else(|_| "[]".to_string()),
            developer,
            publisher,
            now.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Las ofertas que merecen enseñarse, mejor puntuadas primero.
///
/// `limit` acota cuántas se devuelven; el orden pone delante lo puntuado, y
/// dentro de eso, lo más rebajado. Lo que no tiene puntuación va después, no se
/// esconde: puede ser lo que todavía no se ha podido mirar.
pub fn list(connection: &Connection, limit: u32) -> AppResult<Vec<DealCandidate>> {
    let mut statement = connection.prepare(
        "SELECT store, external_id, app_id, title, image_url, store_url,
                final_cents, initial_cents, discount_percent, currency, source,
                match_score, match_reason
           FROM store_deals
          WHERE dismissed_at IS NULL
          ORDER BY match_score IS NULL ASC,
                   match_score DESC,
                   discount_percent DESC,
                   store ASC,
                   external_id ASC
          LIMIT ?1",
    )?;
    let filas = statement
        .query_map([limit], |row| {
            Ok(DealCandidate {
                store: row.get(0)?,
                external_id: row.get(1)?,
                app_id: row.get(2)?,
                title: row.get(3)?,
                header_url: row.get(4)?,
                store_url: row.get(5)?,
                final_cents: row.get(6)?,
                initial_cents: row.get(7)?,
                discount_percent: row.get::<_, i64>(8)?.clamp(0, 100) as u8,
                currency: row.get(9)?,
                source: row.get(10)?,
                match_score: row.get(11)?,
                match_reason: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(filas)
}

/// Descarta una oferta para que deje de aparecer.
/// A dónde lleva «abrir en la tienda» esa oferta, si sigue viva.
///
/// La dirección sale de la base, no del frente: así lo que se abre es lo que
/// Vindexa guardó al traer la oferta, y no una dirección compuesta en la
/// interfaz. Aun así, la ventana vuelve a comprobarla contra su lista de
/// destinos permitidos.
pub fn url_of(connection: &Connection, store: &str, external_id: &str) -> AppResult<String> {
    let url: Option<String> = connection
        .query_row(
            "SELECT store_url FROM store_deals WHERE store = ?1 AND external_id = ?2",
            params![store, external_id],
            |row| row.get(0),
        )
        .optional()?;
    url.ok_or_else(|| {
        crate::error::AppError::validation("Esa oferta ya no está entre las que hay guardadas.")
    })
}

pub fn dismiss(
    connection: &Connection,
    store: &str,
    external_id: &str,
    now: DateTime<Utc>,
) -> AppResult<()> {
    connection.execute(
        "UPDATE store_deals SET dismissed_at = ?3 WHERE store = ?1 AND external_id = ?2",
        params![store, external_id, now.to_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use chrono::TimeZone;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        connection
    }

    /// Una rebaja de Steam: identificada por AppID y **sin** rasgos, que es como
    /// llegan de verdad.
    fn steam(app_id: u32, title: &str, discount: u8) -> IncomingDeal {
        IncomingDeal {
            store: "steam".to_string(),
            external_id: app_id.to_string(),
            app_id: Some(app_id),
            title: title.to_string(),
            store_url: format!("https://store.steampowered.com/app/{app_id}/"),
            image_url: None,
            final_cents: 999,
            initial_cents: 1999,
            discount_percent: discount,
            currency: "EUR".to_string(),
            source: "specials".to_string(),
            genres: Vec::new(),
            categories: Vec::new(),
            developer: None,
            publisher: None,
            facets_known: false,
        }
    }

    /// Una rebaja de GOG: sin AppID y **con** rasgos.
    fn gog(product_id: &str, title: &str, discount: u8) -> IncomingDeal {
        IncomingDeal {
            store: "gog".to_string(),
            external_id: product_id.to_string(),
            app_id: None,
            title: title.to_string(),
            store_url: format!("https://www.gog.com/game/{product_id}"),
            image_url: None,
            final_cents: 999,
            initial_cents: 1999,
            discount_percent: discount,
            currency: "EUR".to_string(),
            source: "discounted".to_string(),
            genres: vec!["Rol".to_string()],
            categories: Vec::new(),
            developer: Some("Estudio".to_string()),
            publisher: None,
            facets_known: true,
        }
    }

    fn at(hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 19, hour, 0, 0)
            .single()
            .expect("instante válido")
    }

    #[test]
    fn una_rebaja_de_algo_que_ya_tienes_no_es_una_oferta() {
        let mut connection = database();
        connection
            .execute("INSERT INTO games(app_id, title) VALUES (10, 'Ya lo tengo')", [])
            .expect("insertar juego");

        let report = sync(
            &mut connection,
            "steam",
            &[steam(10, "Ya lo tengo", 50), steam(20, "Nuevo", 30)],
            at(10),
        )
        .expect("guardar");
        assert_eq!(report.already_known, 1);
        assert_eq!(report.discovered, 1);

        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].app_id, Some(20));
    }

    #[test]
    fn una_rebaja_de_algo_que_ya_deseas_tampoco_se_repite() {
        // Los deseados tienen su propia sección con tu precio objetivo; la misma
        // rebaja en dos sitios es la misma rebaja dos veces.
        let mut connection = database();
        connection
            .execute("INSERT INTO catalog_games(app_id, title) VALUES (30, 'Deseado')", [])
            .expect("insertar catálogo");
        connection
            .execute(
                "INSERT INTO catalog_wishlist_entries(app_id, bucket) VALUES (30, 'considering')",
                [],
            )
            .expect("insertar deseado");

        let report =
            sync(&mut connection, "steam", &[steam(30, "Deseado", 60)], at(10)).expect("guardar");
        assert_eq!(report.already_known, 1);
        assert!(list(&connection, 10).expect("listar").is_empty());
    }

    #[test]
    fn lo_que_deja_de_estar_rebajado_desaparece() {
        // Una oferta caducada que sigue en pantalla lleva a la tienda a pagar el
        // precio completo pensando que hay descuento.
        let mut connection = database();
        sync(
            &mut connection,
            "steam",
            &[steam(20, "Primera", 30), steam(21, "Segunda", 40)],
            at(10),
        )
        .expect("primera tanda");
        assert_eq!(list(&connection, 10).expect("listar").len(), 2);

        sync(&mut connection, "steam", &[steam(21, "Segunda", 40)], at(12))
            .expect("segunda tanda");
        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].app_id, Some(21));
    }

    #[test]
    fn sin_ofertas_no_queda_nada_viejo_en_pantalla() {
        let mut connection = database();
        sync(&mut connection, "steam", &[steam(20, "Una", 30)], at(10)).expect("guardar");
        sync(&mut connection, "steam", &[], at(12)).expect("tanda vacía");
        assert!(list(&connection, 10).expect("listar").is_empty());
    }

    #[test]
    fn una_tanda_de_una_tienda_no_borra_las_de_la_otra() {
        // Cada tienda se trae por su lado. Si limpiar la tanda de GOG barriera
        // toda la tabla, las rebajas de Steam desaparecerían cada seis horas y
        // volverían sólo en la vuelta siguiente.
        let mut connection = database();
        sync(&mut connection, "steam", &[steam(20, "De Steam", 30)], at(10)).expect("Steam");
        sync(&mut connection, "gog", &[gog("1207", "De GOG", 70)], at(10)).expect("GOG");
        assert_eq!(list(&connection, 10).expect("listar").len(), 2);

        // GOG deja de rebajar el suyo; el de Steam sigue rebajado.
        sync(&mut connection, "gog", &[], at(12)).expect("GOG vacío");
        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].store, "steam");
    }

    #[test]
    fn dos_tiendas_pueden_usar_el_mismo_numero_sin_pisarse() {
        // GOG numera sus productos a su manera: que un número coincida con un
        // AppID de Steam no es imposible, y sería otro juego distinto.
        let mut connection = database();
        sync(&mut connection, "steam", &[steam(1207, "El de Steam", 30)], at(10))
            .expect("Steam");
        sync(&mut connection, "gog", &[gog("1207", "El de GOG", 70)], at(10)).expect("GOG");

        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 2);
        let titulos: Vec<&str> = lista.iter().map(|deal| deal.title.as_str()).collect();
        assert!(titulos.contains(&"El de Steam"));
        assert!(titulos.contains(&"El de GOG"));
    }

    #[test]
    fn lo_puntuado_va_delante_y_lo_no_puntuado_no_se_esconde() {
        let mut connection = database();
        sync(
            &mut connection,
            "steam",
            &[steam(20, "Sin puntuar", 90), steam(21, "Puntuado", 10)],
            at(10),
        )
        .expect("guardar");
        connection
            .execute(
                "UPDATE store_deals SET match_score = 72.5, match_reason = 'Coincide'
                  WHERE store = 'steam' AND external_id = '21'",
                [],
            )
            .expect("puntuar");

        let lista = list(&connection, 10).expect("listar");
        assert_eq!(
            lista[0].app_id,
            Some(21),
            "lo puntuado va primero aunque rebaje menos"
        );
        assert_eq!(lista[0].match_score, Some(72.5));
        assert_eq!(lista[1].app_id, Some(20));
        assert_eq!(lista[1].match_score, None, "sin puntuar no es cero");
    }

    #[test]
    fn se_piden_primero_los_rasgos_de_lo_mas_rebajado() {
        let mut connection = database();
        sync(
            &mut connection,
            "steam",
            &[steam(20, "Poco", 10), steam(21, "Mucho", 80)],
            at(10),
        )
        .expect("guardar");
        assert_eq!(pending_facets(&connection, 10).expect("pendientes"), [21, 20]);
    }

    #[test]
    fn una_oferta_de_gog_no_se_queda_esperando_una_ficha_que_no_existe() {
        // Sus rasgos llegan con la propia oferta, y no hay ficha de Steam que
        // pedir: dejarla pendiente sería esperar para siempre.
        let mut connection = database();
        sync(&mut connection, "gog", &[gog("1207", "De GOG", 70)], at(10)).expect("guardar");
        assert!(pending_facets(&connection, 10).expect("pendientes").is_empty());

        let guardados: String = connection
            .query_row(
                "SELECT genres_json FROM store_deals WHERE store = 'gog'",
                [],
                |row| row.get(0),
            )
            .expect("leer géneros");
        assert!(guardados.contains("Rol"));
    }

    #[test]
    fn una_tanda_sin_rasgos_no_borra_los_que_ya_se_sabian() {
        // La ficha se pide una vez y vale para las seis horas siguientes; si la
        // tanda de Steam los machacara con vacíos, se volverían a pedir sin fin.
        let mut connection = database();
        sync(&mut connection, "steam", &[steam(20, "Una", 30)], at(10)).expect("guardar");
        save_facets(
            &connection,
            20,
            &["Acción".to_string()],
            &["Un jugador".to_string()],
            Some("Estudio"),
            Some("Editor"),
            at(11),
        )
        .expect("guardar rasgos");

        sync(&mut connection, "steam", &[steam(20, "Una", 45)], at(16)).expect("segunda tanda");
        assert!(pending_facets(&connection, 10).expect("pendientes").is_empty());
    }

    #[test]
    fn con_los_rasgos_guardados_deja_de_estar_pendiente() {
        let mut connection = database();
        sync(&mut connection, "steam", &[steam(20, "Una", 30)], at(10)).expect("guardar");
        save_facets(
            &connection,
            20,
            &["Acción".to_string()],
            &["Un jugador".to_string()],
            Some("Estudio"),
            Some("Editor"),
            at(11),
        )
        .expect("guardar rasgos");
        assert!(pending_facets(&connection, 10).expect("pendientes").is_empty());
    }

    #[test]
    fn lo_descartado_deja_de_aparecer_y_solo_en_su_tienda() {
        let mut connection = database();
        sync(&mut connection, "steam", &[steam(1207, "El de Steam", 30)], at(10))
            .expect("Steam");
        sync(&mut connection, "gog", &[gog("1207", "El de GOG", 70)], at(10)).expect("GOG");

        dismiss(&connection, "gog", "1207", at(11)).expect("descartar");
        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].store, "steam");
    }

    #[test]
    fn cada_oferta_lleva_su_propio_enlace_a_la_tienda() {
        // Abrir una rebaja de GOG en la tienda de Steam llevaría a otro sitio, o
        // a ninguno.
        let mut connection = database();
        sync(&mut connection, "gog", &[gog("1207", "De GOG", 70)], at(10)).expect("guardar");
        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista[0].store_url, "https://www.gog.com/game/1207");
    }
}
