//! El radar de ofertas: traerlas, entenderlas y ordenarlas por lo que te gusta.
//!
//! # Dos tiendas, un solo criterio
//!
//! Se pregunta a **Steam** y a **GOG**. GOG está aquí por dos motivos: vende sin
//! DRM por definición —que es justo lo que esta biblioteca mira— y su catálogo
//! entrega género y estudio con la propia oferta, así que sus rebajas se pueden
//! puntuar en el acto sin una petición por juego.
//!
//! Cada tienda se trae por su lado y se guarda por su lado: si una no responde,
//! la otra entra igual y lo que faltó **se dice** en el informe, en vez de dejar
//! una lista corta con pinta de completa.
//!
//! # Los tres pasos, y por qué ese orden
//!
//! 1. **Traer.** Una petición por tienda, sin sesión ni clave.
//! 2. **Entender.** Las rebajas de Steam llegan sin géneros ni estudio, que es
//!    justo lo que hace falta para saber si te interesan: se piden aparte, una
//!    vez por juego, y se guardan. Las de GOG ya vienen con ellos.
//! 3. **Ordenar.** Con los rasgos ya guardados, se puntúan contra el modelo de
//!    gustos que sale de tu historial.
//!
//! Puntuar antes de entender daría la puntuación de un juego del que no se sabe
//! nada, que es cero disfrazado de dato.
//!
//! # Ritmo
//!
//! Las rebajas duran días y cambian a la vez para todo el mundo: preguntar cada
//! seis horas es más que suficiente y no castiga a ninguna de las dos tiendas.

use std::time::Duration;

use chrono::Utc;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::db::Database;
use crate::db::deals::IncomingDeal;
use crate::error::AppResult;
use crate::steam::deals::{self, DealSource, StoreDeal};
use crate::steam::store_api;
use crate::stores::gog_deals::{self, GogDeal};

const LAST_AUTO_KEY: &str = "deals.last_auto_refresh";
const AUTO_INTERVAL_HOURS: i64 = 6;

/// País y moneda con los que se piden los precios. Los mismos que usa el resto
/// de la aplicación para Steam: enseñar una rebaja en otra moneda que el precio
/// de al lado sería comparar dos cosas distintas.
const COUNTRY: &str = "ES";
const CURRENCY: &str = "EUR";

/// Cuántas fichas se piden por tanda para conocer los rasgos.
///
/// La respuesta de `featuredcategories` trae unas decenas de ofertas; este tope
/// evita que un día con muchas convierta la tanda en un barrido.
const FACETS_PER_RUN: u32 = 30;

/// Pausa entre fichas. La misma que usa la pasada de DRM, por el mismo motivo.
const REQUEST_INTERVAL: Duration = Duration::from_millis(1_500);

/// Qué dejó una tanda del radar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DealRadarReport {
    /// Ofertas que devolvieron las tiendas.
    pub received: u32,
    /// Ofertas nuevas.
    pub discovered: u32,
    /// Descartadas por ser ya tuyas o estar ya en deseados.
    pub already_known: u32,
    /// Fichas pedidas para conocer sus rasgos.
    pub described: u32,
    /// Ofertas puntuadas contra el modelo de gustos.
    pub scored: u32,
    /// Tiendas a las que no se pudo preguntar en esta tanda.
    ///
    /// Sin esta lista, una tienda caída se ve igual que una tienda sin rebajas:
    /// lo que quedó fuera se dice, no se calla.
    pub unavailable: Vec<String>,
}

/// Traduce una rebaja de Steam al modelo común.
///
/// El AppID viaja por partida doble: como identificador dentro de Steam y como
/// `app_id`, que es lo que permite cruzarla con tu biblioteca y enseñar sus
/// capturas. En GOG sólo existe lo primero.
fn from_steam(deal: &StoreDeal) -> IncomingDeal {
    IncomingDeal {
        store: "steam".to_string(),
        external_id: deal.app_id.to_string(),
        app_id: Some(deal.app_id),
        title: deal.title.clone(),
        store_url: format!("https://store.steampowered.com/app/{}/", deal.app_id),
        image_url: deal.header_url.clone(),
        final_cents: deal.final_cents,
        initial_cents: deal.initial_cents,
        discount_percent: deal.discount_percent,
        currency: deal.currency.clone(),
        source: deal.source.as_str().to_string(),
        genres: Vec::new(),
        categories: Vec::new(),
        developer: None,
        publisher: None,
        // Steam no manda los rasgos con la rebaja: hay que pedir la ficha.
        facets_known: false,
    }
}

/// Traduce una rebaja de GOG al modelo común.
fn from_gog(deal: &GogDeal) -> IncomingDeal {
    IncomingDeal {
        store: "gog".to_string(),
        external_id: deal.product_id.clone(),
        // GOG no publica el AppID de Steam del mismo juego, y adivinarlo por el
        // título sería inventarse una equivalencia.
        app_id: None,
        title: deal.title.clone(),
        store_url: deal.store_url.clone(),
        image_url: deal.image_url.clone(),
        final_cents: deal.final_cents,
        initial_cents: deal.initial_cents,
        discount_percent: deal.discount_percent,
        currency: deal.currency.clone(),
        source: "discounted".to_string(),
        genres: deal.genres.clone(),
        // El catálogo no trae categorías al estilo de Steam; se deja vacío en
        // vez de rellenarlo con los géneros otra vez.
        categories: Vec::new(),
        developer: deal.developer.clone(),
        publisher: deal.publisher.clone(),
        // Los rasgos llegan con la oferta: no hace falta pedir nada más.
        facets_known: true,
    }
}

/// Trae, describe y puntúa.
pub async fn run(database: &Database) -> AppResult<DealRadarReport> {
    let mut report = DealRadarReport::default();
    let now = Utc::now();

    // Steam. Un fallo suyo no impide traer las de GOG.
    match deals::fetch(&[DealSource::Specials, DealSource::TopSellers]).await {
        Ok(ofertas) => {
            let entrantes: Vec<IncomingDeal> = ofertas.iter().map(from_steam).collect();
            let sync = database.sync_store_deals("steam", &entrantes, now)?;
            acumular(&mut report, &sync);
        }
        Err(_) => report.unavailable.push("steam".to_string()),
    }

    // GOG.
    match gog_deals::fetch(COUNTRY, CURRENCY).await {
        Ok(ofertas) => {
            let entrantes: Vec<IncomingDeal> = ofertas.iter().map(from_gog).collect();
            let sync = database.sync_store_deals("gog", &entrantes, now)?;
            acumular(&mut report, &sync);
        }
        Err(_) => report.unavailable.push("gog".to_string()),
    }

    // Los rasgos que faltan: una ficha por oferta de Steam, con pausa. Un fallo
    // de una ficha deja esa oferta sin puntuar, no tumba la tanda.
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

fn acumular(report: &mut DealRadarReport, sync: &crate::db::deals::DealSyncReport) {
    report.received = report.received.saturating_add(sync.received);
    report.discovered = report.discovered.saturating_add(sync.discovered);
    report.already_known = report.already_known.saturating_add(sync.already_known);
}

/// Corre ahora, lo pida el reloj o no.
///
/// Es lo que hay detrás del botón de la pantalla: una rebaja que se acaba no
/// espera seis horas, y tras un cambio en cómo se leen las ofertas hace falta
/// poder volver a preguntarlo sin reiniciar.
pub async fn refresh_now(database: &Database) -> AppResult<DealRadarReport> {
    let report = run(database).await?;
    mark_done(database)?;
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
    let guardadas: i64 =
        connection.query_row("SELECT COUNT(*) FROM store_deals", [], |row| row.get(0))?;
    Ok(should_run(last.as_deref(), guardadas, Utc::now()))
}

/// Si toca preguntar, dado lo que se sabe.
///
/// La lista vacía manda sobre el reloj: una tabla sin ofertas y un sello
/// reciente es lo que deja la aplicación después de una migración que la
/// rehace, y esperar seis horas dejaría la sección vacía sin que nada lo
/// explique.
fn should_run(last: Option<&str>, stored: i64, now: chrono::DateTime<Utc>) -> bool {
    if stored == 0 {
        return true;
    }
    let Some(last) = last else {
        return true;
    };
    let Ok(moment) = chrono::DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    now.signed_duration_since(moment.with_timezone(&Utc))
        >= chrono::Duration::hours(AUTO_INTERVAL_HOURS)
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
    use chrono::TimeZone;

    #[test]
    fn un_informe_vacio_no_afirma_nada() {
        let report = DealRadarReport::default();
        assert_eq!(report.received, 0);
        assert_eq!(report.scored, 0);
        assert!(report.unavailable.is_empty());
    }

    #[test]
    fn una_rebaja_de_steam_conserva_su_appid_por_partida_doble() {
        let entrante = from_steam(&StoreDeal {
            app_id: 379_720,
            title: "DOOM".to_string(),
            header_url: Some("https://cdn.example/doom.jpg".to_string()),
            final_cents: 499,
            initial_cents: 1999,
            discount_percent: 75,
            currency: "EUR".to_string(),
            source: DealSource::Specials,
        });

        assert_eq!(entrante.store, "steam");
        assert_eq!(entrante.external_id, "379720");
        assert_eq!(entrante.app_id, Some(379_720));
        assert_eq!(
            entrante.store_url,
            "https://store.steampowered.com/app/379720/"
        );
        // Sin rasgos: la rebaja de Steam llega desnuda y hay que pedir la ficha.
        assert!(!entrante.facets_known);
        assert!(entrante.genres.is_empty());
    }

    #[test]
    fn una_rebaja_de_gog_no_finge_un_appid_de_steam() {
        let entrante = from_gog(&GogDeal {
            product_id: "1207658930".to_string(),
            title: "The Witcher 3".to_string(),
            store_url: "https://www.gog.com/game/the_witcher_3".to_string(),
            image_url: Some("https://images.gog.com/w3.jpg".to_string()),
            final_cents: 999,
            initial_cents: 2999,
            discount_percent: 66,
            currency: "EUR".to_string(),
            genres: vec!["Rol".to_string()],
            developer: Some("CD PROJEKT RED".to_string()),
            publisher: Some("CD PROJEKT".to_string()),
        });

        assert_eq!(entrante.store, "gog");
        assert_eq!(entrante.external_id, "1207658930");
        // Adivinar el AppID por el título sería inventarse una equivalencia.
        assert_eq!(entrante.app_id, None);
        assert_eq!(entrante.store_url, "https://www.gog.com/game/the_witcher_3");
        // Los rasgos vienen con la oferta: se puede puntuar sin pedir nada más.
        assert!(entrante.facets_known);
        assert_eq!(entrante.genres, vec!["Rol".to_string()]);
    }

    #[test]
    fn una_lista_vacia_se_rellena_sin_esperar_al_reloj() {
        // Migración 046: la tabla se rehace y queda vacía, pero el sello de la
        // última tanda sigue puesto. Sin esta regla, la sección se quedaría sin
        // nada durante seis horas y nada lo diría.
        let ahora = Utc
            .with_ymd_and_hms(2026, 8, 20, 12, 0, 0)
            .single()
            .expect("instante válido");
        let hace_un_rato = "2026-08-20T11:30:00Z";

        assert!(
            should_run(Some(hace_un_rato), 0, ahora),
            "sin ofertas se pregunta"
        );
        assert!(
            !should_run(Some(hace_un_rato), 40, ahora),
            "con ofertas recientes se respeta el ritmo"
        );
        assert!(
            should_run(Some("2026-08-20T05:00:00Z"), 40, ahora),
            "pasadas las seis horas se vuelve a preguntar"
        );
        assert!(
            should_run(None, 40, ahora),
            "sin sello nunca se ha preguntado"
        );
        assert!(
            should_run(Some("no es una fecha"), 40, ahora),
            "un sello ilegible no bloquea la tanda"
        );
    }

    #[test]
    fn lo_de_las_dos_tiendas_se_suma_en_un_solo_informe() {
        let mut report = DealRadarReport::default();
        acumular(
            &mut report,
            &crate::db::deals::DealSyncReport {
                received: 30,
                discovered: 12,
                already_known: 18,
            },
        );
        acumular(
            &mut report,
            &crate::db::deals::DealSyncReport {
                received: 40,
                discovered: 25,
                already_known: 15,
            },
        );

        assert_eq!(report.received, 70);
        assert_eq!(report.discovered, 37);
        assert_eq!(report.already_known, 33);
    }
}
