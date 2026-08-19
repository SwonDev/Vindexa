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
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::steam::deals::StoreDeal;

/// Una oferta lista para enseñar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DealCandidate {
    pub app_id: u32,
    pub title: String,
    pub header_url: Option<String>,
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
pub fn sync(
    connection: &mut Connection,
    deals: &[StoreDeal],
    now: DateTime<Utc>,
) -> AppResult<DealSyncReport> {
    let mut report = DealSyncReport {
        received: deals.len() as u32,
        ..DealSyncReport::default()
    };
    let sello = now.to_rfc3339();
    let transaction = connection.transaction()?;

    for deal in deals {
        // Lo tuyo no es una oferta que descubrir. Se comprueba en las dos
        // listas: la biblioteca y los deseados, que viven en tablas distintas.
        let conocido: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM games WHERE app_id = ?1)
                 OR EXISTS(SELECT 1 FROM wishlist_entries WHERE app_id = ?1)
                 OR EXISTS(SELECT 1 FROM catalog_wishlist_entries WHERE app_id = ?1)",
            [deal.app_id],
            |row| row.get(0),
        )?;
        if conocido {
            report.already_known = report.already_known.saturating_add(1);
            continue;
        }

        let nuevo = transaction.execute(
            "INSERT INTO store_deals(
                 app_id, title, header_url, final_cents, initial_cents,
                 discount_percent, currency, source, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(app_id) DO UPDATE SET
                 title = excluded.title,
                 header_url = excluded.header_url,
                 final_cents = excluded.final_cents,
                 initial_cents = excluded.initial_cents,
                 discount_percent = excluded.discount_percent,
                 currency = excluded.currency,
                 source = excluded.source,
                 updated_at = excluded.updated_at",
            params![
                deal.app_id,
                deal.title,
                deal.header_url,
                deal.final_cents,
                deal.initial_cents,
                i64::from(deal.discount_percent),
                deal.currency,
                deal.source.as_str(),
                sello,
            ],
        )?;
        // `execute` devuelve 1 tanto al insertar como al actualizar; lo nuevo se
        // reconoce porque su `first_seen_at` es de esta misma tanda.
        if nuevo > 0 {
            let recien: bool = transaction.query_row(
                "SELECT first_seen_at >= ?2 FROM store_deals WHERE app_id = ?1",
                params![deal.app_id, sello],
                |row| row.get(0),
            )?;
            if recien {
                report.discovered = report.discovered.saturating_add(1);
            }
        }
    }

    // Lo que ya no está rebajado deja de estar: una oferta caducada que sigue en
    // pantalla es peor que ninguna oferta.
    let vigentes: Vec<u32> = deals.iter().map(|deal| deal.app_id).collect();
    if vigentes.is_empty() {
        transaction.execute("DELETE FROM store_deals", [])?;
    } else {
        let marcadores = vigentes
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("DELETE FROM store_deals WHERE app_id NOT IN ({marcadores})");
        let referencias: Vec<&dyn rusqlite::ToSql> = vigentes
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        transaction.execute(&sql, referencias.as_slice())?;
    }

    transaction.commit()?;
    Ok(report)
}

/// Ofertas a las que aún no se les han pedido los rasgos para puntuarlas.
pub fn pending_facets(connection: &Connection, limit: u32) -> AppResult<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT app_id FROM store_deals
          WHERE facets_fetched_at IS NULL AND dismissed_at IS NULL
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
          WHERE app_id = ?1",
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
        "SELECT app_id, title, header_url, final_cents, initial_cents,
                discount_percent, currency, source, match_score, match_reason
           FROM store_deals
          WHERE dismissed_at IS NULL
          ORDER BY match_score IS NULL ASC,
                   match_score DESC,
                   discount_percent DESC,
                   app_id ASC
          LIMIT ?1",
    )?;
    let filas = statement
        .query_map([limit], |row| {
            Ok(DealCandidate {
                app_id: row.get(0)?,
                title: row.get(1)?,
                header_url: row.get(2)?,
                final_cents: row.get(3)?,
                initial_cents: row.get(4)?,
                discount_percent: row.get::<_, i64>(5)?.clamp(0, 100) as u8,
                currency: row.get(6)?,
                source: row.get(7)?,
                match_score: row.get(8)?,
                match_reason: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(filas)
}

/// Descarta una oferta para que deje de aparecer.
pub fn dismiss(connection: &Connection, app_id: u32, now: DateTime<Utc>) -> AppResult<()> {
    connection.execute(
        "UPDATE store_deals SET dismissed_at = ?2 WHERE app_id = ?1",
        params![app_id, now.to_rfc3339()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;
    use crate::steam::deals::DealSource;
    use chrono::TimeZone;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        connection
    }

    fn oferta(app_id: u32, title: &str, discount: u8) -> StoreDeal {
        StoreDeal {
            app_id,
            title: title.to_string(),
            header_url: None,
            final_cents: 999,
            initial_cents: 1999,
            discount_percent: discount,
            currency: "EUR".to_string(),
            source: DealSource::Specials,
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
            &[oferta(10, "Ya lo tengo", 50), oferta(20, "Nuevo", 30)],
            at(10),
        )
        .expect("guardar");
        assert_eq!(report.already_known, 1);
        assert_eq!(report.discovered, 1);

        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].app_id, 20);
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

        let report = sync(&mut connection, &[oferta(30, "Deseado", 60)], at(10)).expect("guardar");
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
            &[oferta(20, "Primera", 30), oferta(21, "Segunda", 40)],
            at(10),
        )
        .expect("primera tanda");
        assert_eq!(list(&connection, 10).expect("listar").len(), 2);

        sync(&mut connection, &[oferta(21, "Segunda", 40)], at(12)).expect("segunda tanda");
        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].app_id, 21);
    }

    #[test]
    fn sin_ofertas_no_queda_nada_viejo_en_pantalla() {
        let mut connection = database();
        sync(&mut connection, &[oferta(20, "Una", 30)], at(10)).expect("guardar");
        sync(&mut connection, &[], at(12)).expect("tanda vacía");
        assert!(list(&connection, 10).expect("listar").is_empty());
    }

    #[test]
    fn lo_puntuado_va_delante_y_lo_no_puntuado_no_se_esconde() {
        let mut connection = database();
        sync(
            &mut connection,
            &[oferta(20, "Sin puntuar", 90), oferta(21, "Puntuado", 10)],
            at(10),
        )
        .expect("guardar");
        connection
            .execute(
                "UPDATE store_deals SET match_score = 72.5, match_reason = 'Coincide' WHERE app_id = 21",
                [],
            )
            .expect("puntuar");

        let lista = list(&connection, 10).expect("listar");
        assert_eq!(lista[0].app_id, 21, "lo puntuado va primero aunque rebaje menos");
        assert_eq!(lista[0].match_score, Some(72.5));
        assert_eq!(lista[1].app_id, 20);
        assert_eq!(lista[1].match_score, None, "sin puntuar no es cero");
    }

    #[test]
    fn se_piden_primero_los_rasgos_de_lo_mas_rebajado() {
        let mut connection = database();
        sync(
            &mut connection,
            &[oferta(20, "Poco", 10), oferta(21, "Mucho", 80)],
            at(10),
        )
        .expect("guardar");
        assert_eq!(pending_facets(&connection, 10).expect("pendientes"), [21, 20]);
    }

    #[test]
    fn con_los_rasgos_guardados_deja_de_estar_pendiente() {
        let mut connection = database();
        sync(&mut connection, &[oferta(20, "Una", 30)], at(10)).expect("guardar");
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
    fn lo_descartado_deja_de_aparecer() {
        let mut connection = database();
        sync(&mut connection, &[oferta(20, "Una", 30)], at(10)).expect("guardar");
        dismiss(&connection, 20, at(11)).expect("descartar");
        assert!(list(&connection, 10).expect("listar").is_empty());
    }
}
