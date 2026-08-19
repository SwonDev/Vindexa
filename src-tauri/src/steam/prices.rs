//! Traer los precios de lo que se desea, y traerlos todos.
//!
//! # El fallo que motivó este módulo
//!
//! Los precios se pedían bien —en lotes de cien, con pausa entre lotes— pero se
//! guardaban mal: `record_observation` exigía que el juego estuviera en
//! `games`, y los deseados importados de Steam viven en `catalog_games` mientras
//! no se compren. El primero de ellos devolvía «no está en la biblioteca», el
//! error subía por el `?` y **abortaba el refresco entero**. Medido en la
//! instalación real: 5 precios sobre 1.410 deseados, y la pantalla diciendo
//! «1.405 juegos sin precio consultado» sin explicar por qué.
//!
//! De ahí las dos reglas de aquí:
//!
//! 1. Un fallo de un juego es un juego fallado, no una tanda perdida. Se cuenta
//!    y se sigue.
//! 2. Lo que no se ha podido saber se dice en el informe, con su motivo.
//!
//! # Por qué corre solo
//!
//! Un precio que hay que pedir a mano es un precio que nadie mira. La tanda
//! automática se reparte en el tiempo y respeta el ritmo de la tienda; el botón
//! de la pantalla sigue existiendo para cuando se quiere ahora.

use std::time::Duration;

use chrono::Utc;
use rusqlite::OptionalExtension;
use tokio::time::sleep;

use crate::db::pricing::PriceRefreshReport;
use crate::db::Database;
use crate::error::AppResult;
use crate::steam::store_api;

/// Cada cuánto se repasa solo. Los precios de Steam cambian por rebajas, que
/// duran días: mirar más a menudo sería pedir por pedir.
const AUTO_INTERVAL_HOURS: i64 = 6;
const LAST_AUTO_KEY: &str = "prices.last_auto_refresh";

/// Pausa entre lotes. La tienda admite ráfagas cortas y castiga las largas;
/// con cien juegos por lote, mil cuatrocientos deseados son quince peticiones.
const BATCH_INTERVAL: Duration = Duration::from_millis(750);

/// Pide y guarda los precios de una lista de juegos.
///
/// `limit` acota cuántos deseados se repasan; `0` significa «los que toquen»,
/// con el tope que impone la propia consulta.
pub async fn refresh(database: &Database, limit: u32) -> AppResult<PriceRefreshReport> {
    let now = Utc::now();
    let candidatos = database.stale_wishlist_price_targets(now, limit)?;
    let mut report = PriceRefreshReport::default();

    for (indice, lote) in candidatos.chunks(store_api::MAX_PRICE_BATCH).enumerate() {
        if indice > 0 {
            sleep(BATCH_INTERVAL).await;
        }
        match store_api::fetch_prices(lote).await {
            Ok(precios) => {
                // Los tres desenlaces del lote se cuentan por separado: un
                // precio desconocido no es un precio de cero, y un AppID que la
                // tienda no reconoce tampoco es un juego gratuito.
                report.without_price = report
                    .without_price
                    .saturating_add(precios.without_price.len() as u32)
                    .saturating_add(precios.unavailable.len() as u32);
                for observation in precios.prices {
                    match database.record_price_observation(&observation, Utc::now()) {
                        Ok(recorded) => {
                            report.observed = report.observed.saturating_add(1);
                            if recorded.changed {
                                report.changed = report.changed.saturating_add(1);
                            }
                            if recorded.alert.is_some_and(|alert| alert.created) {
                                report.alerts = report.alerts.saturating_add(1);
                            }
                        }
                        // Un juego que no se puede guardar es **un** juego
                        // perdido. Antes esto subía por el `?` y se llevaba por
                        // delante los mil cuatrocientos que venían detrás.
                        Err(_) => report.failed = report.failed.saturating_add(1),
                    }
                }
            }
            Err(failure) => {
                // Falla el lote entero, así que suman todos sus juegos: decir
                // que ha fallado uno cuando han quedado cien sin consultar
                // sería mentir sobre el alcance del problema.
                report.failed = report.failed.saturating_add(lote.len() as u32);
                // Steam ha pedido esperar: se espera. Insistir sólo empeora el
                // límite para el resto de los lotes.
                if let Some(delay) = failure.retry_after {
                    sleep(delay).await;
                }
            }
        }
    }
    Ok(report)
}

/// Repasa los precios si toca, sin que nadie lo pida.
///
/// Devuelve `None` cuando aún no toca, para que quien llama distinga «no había
/// nada que hacer» de «se hizo y no encontró nada».
pub async fn refresh_if_due(database: &Database) -> AppResult<Option<PriceRefreshReport>> {
    if !is_due(database)? {
        return Ok(None);
    }
    let report = refresh(database, 0).await?;
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
    let Some(last) = last else {
        return Ok(true);
    };
    // Un sello que no se entiende es un sello que no sirve: se vuelve a pasar.
    let Ok(moment) = chrono::DateTime::parse_from_rfc3339(&last) else {
        return Ok(true);
    };
    Ok(Utc::now().signed_duration_since(moment.with_timezone(&Utc))
        >= chrono::Duration::hours(AUTO_INTERVAL_HOURS))
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
    use super::*;

    #[test]
    fn el_informe_suma_lo_sabido_sin_contar_lo_fallado() {
        // «Resuelto» es sobre cuántos juegos se sabe algo: con precio o con la
        // certeza de que no lo tienen. Lo fallado queda fuera a propósito,
        // porque de esos no se sabe nada.
        let report = PriceRefreshReport {
            observed: 41,
            changed: 3,
            alerts: 1,
            without_price: 59,
            failed: 7,
        };
        assert_eq!(report.resolved(), 100);
    }

    #[test]
    fn un_informe_vacio_no_afirma_nada() {
        let report = PriceRefreshReport::default();
        assert_eq!(report.resolved(), 0);
        assert_eq!(report.failed, 0);
    }
}
