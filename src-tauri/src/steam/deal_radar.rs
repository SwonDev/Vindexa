//! El radar de ofertas: traerlas, entenderlas y ordenarlas por lo que te gusta.
//!
//! # Los tres pasos, y por qué ese orden
//!
//! 1. **Traer.** Una petición a `featuredcategories`, sin sesión ni clave.
//! 2. **Entender.** Las rebajas llegan sin géneros ni estudio, que es justo lo
//!    que hace falta para saber si te interesan: se piden aparte, una vez por
//!    juego, y se guardan. Son unas decenas de peticiones al día, no miles.
//! 3. **Ordenar.** Con los rasgos ya guardados, se puntúan contra el modelo de
//!    gustos que sale de tu historial.
//!
//! Puntuar antes de entender daría la puntuación de un juego del que no se sabe
//! nada, que es cero disfrazado de dato.
//!
//! # Ritmo
//!
//! Las rebajas de Steam duran días y cambian a la vez para todo el mundo:
//! preguntar cada seis horas es más que suficiente y no castiga a la tienda.

use std::time::Duration;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::db::Database;
use crate::error::AppResult;
use crate::steam::deals::{self, DealSource};
use crate::steam::store_api;

const LAST_AUTO_KEY: &str = "deals.last_auto_refresh";
const AUTO_INTERVAL_HOURS: i64 = 6;

/// Cuántas fichas se piden por tanda para conocer los rasgos.
///
/// La respuesta de `featuredcategories` trae unas decenas de ofertas; este tope
/// evita que un día con muchas convierta la tanda en un barrido.
const FACETS_PER_RUN: u32 = 30;

/// Pausa entre fichas. La misma que usa la pasada de DRM, por el mismo motivo.
const REQUEST_INTERVAL: Duration = Duration::from_millis(1_500);

/// Qué dejó una tanda del radar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DealRadarReport {
    /// Ofertas que devolvió la tienda.
    pub received: u32,
    /// Ofertas nuevas.
    pub discovered: u32,
    /// Descartadas por ser ya tuyas o estar ya en deseados.
    pub already_known: u32,
    /// Fichas pedidas para conocer sus rasgos.
    pub described: u32,
    /// Ofertas puntuadas contra el modelo de gustos.
    pub scored: u32,
}

/// Trae, describe y puntúa.
pub async fn run(database: &Database) -> AppResult<DealRadarReport> {
    let ofertas = deals::fetch(&[DealSource::Specials, DealSource::TopSellers])
        .await
        .map_err(|failure| failure.error)?;
    let now = Utc::now();
    let sync = database.sync_store_deals(&ofertas, now)?;

    let mut report = DealRadarReport {
        received: sync.received,
        discovered: sync.discovered,
        already_known: sync.already_known,
        ..DealRadarReport::default()
    };

    // Los rasgos: una ficha por oferta nueva, con pausa. Un fallo de una ficha
    // deja esa oferta sin puntuar, no tumba la tanda.
    let pendientes = database.pending_deal_facets(FACETS_PER_RUN)?;
    for (indice, app_id) in pendientes.iter().enumerate() {
        if indice > 0 {
            sleep(REQUEST_INTERVAL).await;
        }
        if let Ok(Some(rasgos)) = store_api::fetch_facets(*app_id).await {
            let guardado = database.save_deal_facets(
                *app_id,
                &rasgos.genres,
                &rasgos.categories,
                rasgos.developer.as_deref(),
                rasgos.publisher.as_deref(),
                Utc::now(),
            );
            if guardado.is_ok() {
                report.described = report.described.saturating_add(1);
            }
        }
    }

    report.scored = database.score_store_deals()? as u32;
    Ok(report)
}

/// Corre si toca.
pub async fn run_if_due(database: &Database) -> AppResult<Option<DealRadarReport>> {
    if !is_due(database)? {
        return Ok(None);
    }
    let report = run(database).await?;
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
    fn un_informe_vacio_no_afirma_nada() {
        let report = DealRadarReport::default();
        assert_eq!(report.received, 0);
        assert_eq!(report.scored, 0);
    }
}
