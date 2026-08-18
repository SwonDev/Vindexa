//! Precio observado, historial y aviso de rebaja (migración 029).
//!
//! # Este módulo no dice «el precio»
//!
//! Dice «el precio que Vindexa observó, en esta moneda, este día». Toda la
//! superficie pública arrastra esa distinción a propósito:
//!
//! - Ningún importe existe sin `currency`. Un número sin moneda no es un
//!   precio, y el esquema lo hace imposible: la moneda forma parte de la clave.
//! - Ningún importe viaja sin `observed_at`. La interfaz tiene que poder decir
//!   cuándo se miró y que puede haber cambiado, y [`freshness_of`] le da la
//!   palabra exacta para hacerlo.
//! - `lowest_cents` es el mínimo **visto por Vindexa**, no el mínimo histórico
//!   real del juego. Se recalcula desde `game_price_history` en la misma
//!   transacción que inserta la observación, así que siempre hay una fila que
//!   lo demuestra.
//! - Un objetivo en euros y un precio en dólares **no se comparan**. Cuando no
//!   coinciden, [`WishlistPriceStatus::comparable`] sale a `false` y no hay
//!   diferencia calculada: el hueco se declara en vez de rellenarse.
//!
//! # De dónde salen los precios
//!
//! De la misma respuesta de `appdetails` que ya consume
//! [`crate::steam::store_api`], en su bloque `price_overview`. Este módulo no
//! abre conexiones ni conoce `reqwest`: recibe el JSON ya descargado en
//! [`parse_store_price_overview`] y persiste el resultado. Esa frontera es lo
//! que permite probar el modelo entero sin red.
//!
//! # El aviso de rebaja se engancha a `notifications`
//!
//! No hay un segundo sistema de avisos. [`evaluate_alert`] escribe en
//! `notification_events` —la misma bandeja, la misma `dedupe_key`, el mismo
//! índice único parcial de la migración 023— con la clave
//! `price_drop:{app_id}:{moneda}:{importe}`.
//!
//! Esa clave lleva el importe y **no** la fecha, igual que `dlc_release`. La
//! consecuencia es deliberada: un mismo precio no vuelve a avisar nunca, y
//! cualquier precio más bajo sí. Es la elección que evita que una consulta
//! diaria repita el mismo aviso cada mañana.

use crate::error::{AppError, AppResult};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Importe máximo admitido, en la unidad mínima de la moneda. Coincide con el
/// tope del precio objetivo de deseados para que un objetivo y un precio
/// observado quepan siempre en la misma comparación.
pub const MAX_PRICE_CENTS: i64 = 100_000_000;
/// Procedencias admitidas por el `CHECK` de la migración 029.
pub const PRICE_SOURCES: [&str; 2] = ["steam_store", "manual"];
/// Horas durante las que una observación se considera vigente.
pub const FRESH_HOURS: i64 = 24;
/// Horas a partir de las cuales una observación se considera caducada.
pub const STALE_HOURS: i64 = 24 * 7;
/// Puntos que devuelve como máximo una serie histórica.
pub const MAX_HISTORY_POINTS: u32 = 400;
/// Juegos que un solo barrido de actualización puede pedir a la tienda.
pub const MAX_REFRESH_BATCH: u32 = 200;

/// Vigencia de una observación. Es una palabra, no un booleano, porque la
/// interfaz necesita distinguir «recién mirado» de «hace días» sin inventarse
/// el umbral por su cuenta.
pub const FRESHNESS_FRESH: &str = "fresh";
pub const FRESHNESS_AGING: &str = "aging";
pub const FRESHNESS_STALE: &str = "stale";
/// Sello ilegible o fecha futura: no se sabe cuándo se miró, y se dice.
pub const FRESHNESS_UNKNOWN: &str = "unknown";

// ---------------------------------------------------------------------------
// Modelos
// ---------------------------------------------------------------------------

/// Último precio conocido de un juego en una moneda concreta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamePrice {
    pub app_id: u32,
    pub currency: String,
    /// País con el que se consultó. Sin él la moneda no es reproducible.
    pub country_code: Option<String>,
    pub final_cents: i64,
    /// Precio de referencia sin descuento, tal y como lo publica la tienda.
    pub initial_cents: i64,
    pub discount_percent: u8,
    /// Mínimo observado por Vindexa, respaldado siempre por una fila del
    /// historial. No es el mínimo histórico real del juego.
    pub lowest_cents: i64,
    pub lowest_observed_at: String,
    /// Cuándo se vio por primera vez el importe vigente.
    pub changed_at: String,
    /// Cuándo se confirmó por última vez.
    pub observed_at: String,
    pub source: String,
    /// Palabra de vigencia calculada contra el instante de la consulta.
    pub freshness: String,
    /// Minutos transcurridos desde la observación. `None` cuando el sello no
    /// se puede interpretar.
    pub age_minutes: Option<i64>,
}

/// Un punto de la serie histórica.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricePoint {
    pub observed_at: String,
    pub final_cents: i64,
    pub initial_cents: i64,
    pub discount_percent: u8,
    pub source: String,
}

/// Serie completa de un juego en una moneda. Se sirve con la moneda dentro
/// para que la gráfica no pueda pintarse sin etiquetarla.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceHistory {
    pub app_id: u32,
    pub currency: String,
    pub points: Vec<PricePoint>,
}

/// Observación que entra al sistema. `observed_at` lo pone quien llama para
/// que las pruebas sean deterministas y para que una respuesta cacheada pueda
/// conservar el instante real en el que se descargó.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceObservation {
    pub app_id: u32,
    pub currency: String,
    #[serde(default)]
    pub country_code: Option<String>,
    pub final_cents: i64,
    pub initial_cents: i64,
    #[serde(default)]
    pub discount_percent: u8,
    #[serde(default)]
    pub source: String,
}

/// Resultado de registrar una observación.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedPrice {
    pub price: GamePrice,
    /// `true` sólo si el importe cambió respecto a la observación anterior.
    /// Una consulta que confirma el mismo precio actualiza la frescura pero no
    /// añade un punto al historial.
    pub changed: bool,
    /// Aviso generado, si el precio bajó del objetivo anotado en deseados.
    pub alert: Option<PriceAlert>,
}

/// Aviso de rebaja ya materializado en la bandeja de `notifications`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceAlert {
    pub app_id: u32,
    pub currency: String,
    pub final_cents: i64,
    pub target_cents: i64,
    pub dedupe_key: String,
    /// `false` cuando ese mismo aviso ya estaba en la bandeja.
    pub created: bool,
}

/// Situación de precio de una entrada de deseados.
///
/// Es el modelo que convierte «precio objetivo 29,99 €» en un plan: dice si hay
/// precio, si se puede comparar y cuánto falta. Cuando no se puede comparar, lo
/// dice en vez de calcular una diferencia inventada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WishlistPriceStatus {
    pub app_id: u32,
    pub target_cents: Option<i64>,
    pub target_currency: Option<String>,
    /// Precio mostrado: el de la moneda del objetivo si lo hay; si no hay
    /// objetivo y sólo se ha observado una moneda, esa. Con varias monedas y
    /// sin objetivo no se elige ninguna: elegir sería inventar.
    pub price: Option<GamePrice>,
    /// Monedas observadas que no son la del precio mostrado.
    pub other_currencies: Vec<String>,
    /// Sólo `true` con objetivo y precio en la **misma** moneda.
    pub comparable: bool,
    /// `final - objetivo`. Negativo significa por debajo del objetivo.
    pub difference_cents: Option<i64>,
    pub meets_target: bool,
}

/// Recuento de lo que hizo un barrido de precios.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceRefreshReport {
    pub observed: u32,
    pub changed: u32,
    pub alerts: u32,
    /// Juegos que la tienda no devolvió con bloque de precio. No se cuentan
    /// como cero: se cuentan como desconocidos.
    pub without_price: u32,
    pub failed: u32,
}

// ---------------------------------------------------------------------------
// Vigencia
// ---------------------------------------------------------------------------

/// Palabra de vigencia y edad en minutos de una observación.
///
/// Una fecha futura devuelve [`FRESHNESS_UNKNOWN`] en lugar de una edad
/// negativa: si el reloj no cuadra, lo honesto es no afirmar nada.
pub fn freshness_of(observed_at: &str, now: DateTime<Utc>) -> (String, Option<i64>) {
    let Ok(parsed) = DateTime::parse_from_rfc3339(observed_at) else {
        return (FRESHNESS_UNKNOWN.to_string(), None);
    };
    let minutes = (now - parsed.with_timezone(&Utc)).num_minutes();
    if minutes < 0 {
        return (FRESHNESS_UNKNOWN.to_string(), None);
    }
    let word = if minutes < FRESH_HOURS * 60 {
        FRESHNESS_FRESH
    } else if minutes < STALE_HOURS * 60 {
        FRESHNESS_AGING
    } else {
        FRESHNESS_STALE
    };
    (word.to_string(), Some(minutes))
}

// ---------------------------------------------------------------------------
// Análisis de la respuesta de la tienda
// ---------------------------------------------------------------------------

/// Convierte el bloque `price_overview` de `appdetails` en una observación.
///
/// Vive aquí y no en `steam::store_api` por dos razones: el formato del importe
/// es una decisión del modelo de precios, y así se puede comprobar con un JSON
/// fijo sin tocar la red.
///
/// Devuelve `Ok(None)` cuando la ficha no trae precio —juego gratuito, aún sin
/// publicar, o retirado de la tienda—. Eso no es un fallo y no debe confundirse
/// con un precio de cero.
///
/// **Sobre las unidades:** `initial` y `final` llegan como enteros en la unidad
/// mínima que aplica la propia tienda, y se guardan tal cual. Vindexa no los
/// reescala: hacerlo exigiría saber cuántos decimales usa cada moneda en la
/// respuesta de Steam, dato que su API no documenta.
pub fn parse_store_price_overview(
    app_id: u32,
    country_code: Option<&str>,
    value: &serde_json::Value,
) -> AppResult<Option<PriceObservation>> {
    let Some(block) = value.as_object() else {
        return Ok(None);
    };
    let Some(currency) = block.get("currency").and_then(serde_json::Value::as_str) else {
        return Ok(None);
    };
    let Some(final_cents) = block.get("final").and_then(json_i64) else {
        return Ok(None);
    };
    // Sin `initial` la tienda está diciendo que no hay precio de referencia
    // distinto del vigente; no es motivo para descartar la observación.
    let initial_cents = block
        .get("initial")
        .and_then(json_i64)
        .unwrap_or(final_cents);
    let discount = block
        .get("discount_percent")
        .and_then(json_i64)
        .unwrap_or(0)
        .clamp(0, 100) as u8;

    let observation = PriceObservation {
        app_id,
        currency: currency.to_string(),
        country_code: country_code.map(str::to_string),
        final_cents,
        initial_cents,
        discount_percent: discount,
        source: "steam_store".to_string(),
    };
    // Una ficha con un importe absurdo se descarta entera: es preferible no
    // saber el precio a persistir una cifra que la interfaz enseñaría.
    match validate(&observation) {
        Ok(_) => Ok(Some(observation)),
        Err(_) => Ok(None),
    }
}

fn json_i64(value: &serde_json::Value) -> Option<i64> {
    value.as_i64().or_else(|| {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<i64>().ok())
    })
}

// ---------------------------------------------------------------------------
// Escritura
// ---------------------------------------------------------------------------

/// Normaliza y comprueba una observación. Devuelve moneda, país y procedencia
/// ya en su forma canónica.
fn validate(observation: &PriceObservation) -> AppResult<(String, Option<String>, String)> {
    if observation.app_id == 0 {
        return Err(AppError::validation("El juego indicado no es válido."));
    }
    let currency = observation.currency.trim().to_uppercase();
    if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(AppError::validation(
            "Un precio sin moneda ISO 4217 no es un precio.",
        ));
    }
    let country = match observation.country_code.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(raw) => {
            let code = raw.to_uppercase();
            if code.len() != 2 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(AppError::validation(
                    "El país de la consulta de precio no es válido.",
                ));
            }
            Some(code)
        }
    };
    for amount in [observation.final_cents, observation.initial_cents] {
        if !(0..=MAX_PRICE_CENTS).contains(&amount) {
            return Err(AppError::validation(
                "El importe observado está fuera del rango admitido.",
            ));
        }
    }
    if observation.discount_percent > 100 {
        return Err(AppError::validation("El descuento observado no es válido."));
    }
    let source = if observation.source.trim().is_empty() {
        "steam_store".to_string()
    } else {
        observation.source.trim().to_string()
    };
    if !PRICE_SOURCES.contains(&source.as_str()) {
        return Err(AppError::validation(
            "La procedencia del precio no está entre las admitidas.",
        ));
    }
    Ok((currency, country, source))
}

/// Registra una observación y, si procede, genera el aviso de rebaja.
///
/// Todo ocurre en una transacción: el historial, el estado vigente, el recálculo
/// del mínimo y el aviso entran juntos o no entra ninguno. Un aviso sin la fila
/// de precio que lo justifica sería exactamente el dato inventado que este
/// producto se niega a producir.
pub fn record_observation(
    connection: &mut Connection,
    observation: &PriceObservation,
    now: DateTime<Utc>,
) -> AppResult<RecordedPrice> {
    let (currency, country, source) = validate(observation)?;
    ensure_game_exists(connection, observation.app_id)?;
    let observed_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);

    let transaction = connection.transaction()?;

    let previous: Option<(i64, i64, i64, String)> = transaction
        .query_row(
            "SELECT final_cents, initial_cents, discount_percent, changed_at
               FROM game_prices
              WHERE app_id = ?1 AND currency = ?2",
            params![observation.app_id, currency],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;

    let discount = i64::from(observation.discount_percent);
    let changed = match &previous {
        None => true,
        Some((final_cents, initial_cents, previous_discount, _)) => {
            *final_cents != observation.final_cents
                || *initial_cents != observation.initial_cents
                || *previous_discount != discount
        }
    };
    let changed_at = match (&previous, changed) {
        (Some((_, _, _, previous_changed_at)), false) => previous_changed_at.clone(),
        _ => observed_at.clone(),
    };

    if changed {
        // `ON CONFLICT` cubre el caso de dos observaciones con el mismo sello,
        // que sólo ocurre con un reloj detenido o en una prueba.
        transaction.execute(
            "INSERT INTO game_price_history(
                 app_id, currency, observed_at, final_cents, initial_cents,
                 discount_percent, source
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(app_id, currency, observed_at) DO UPDATE SET
                 final_cents = excluded.final_cents,
                 initial_cents = excluded.initial_cents,
                 discount_percent = excluded.discount_percent,
                 source = excluded.source",
            params![
                observation.app_id,
                currency,
                observed_at,
                observation.final_cents,
                observation.initial_cents,
                discount,
                source
            ],
        )?;
    }

    // El mínimo se lee del historial en vez de acumularse en memoria: así lo
    // que Vindexa afirma como mínimo siempre tiene una fila que lo respalda.
    let (lowest_cents, lowest_observed_at): (i64, String) = transaction.query_row(
        "SELECT final_cents, observed_at
           FROM game_price_history
          WHERE app_id = ?1 AND currency = ?2
          ORDER BY final_cents ASC, observed_at ASC
          LIMIT 1",
        params![observation.app_id, currency],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    transaction.execute(
        "INSERT INTO game_prices(
             app_id, currency, country_code, final_cents, initial_cents,
             discount_percent, lowest_cents, lowest_observed_at, changed_at,
             observed_at, source
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(app_id, currency) DO UPDATE SET
             country_code = excluded.country_code,
             final_cents = excluded.final_cents,
             initial_cents = excluded.initial_cents,
             discount_percent = excluded.discount_percent,
             lowest_cents = excluded.lowest_cents,
             lowest_observed_at = excluded.lowest_observed_at,
             changed_at = excluded.changed_at,
             observed_at = excluded.observed_at,
             source = excluded.source",
        params![
            observation.app_id,
            currency,
            country,
            observation.final_cents,
            observation.initial_cents,
            discount,
            lowest_cents,
            lowest_observed_at,
            changed_at,
            observed_at,
            source
        ],
    )?;

    let alert = evaluate_alert(&transaction, observation.app_id, now)?;
    transaction.commit()?;

    let price = price_for(connection, observation.app_id, &currency, now)?
        .ok_or_else(|| AppError::not_found("El precio registrado ya no está disponible."))?;
    Ok(RecordedPrice {
        price,
        changed,
        alert,
    })
}

/// Compara el último precio conocido con el objetivo anotado en deseados y, si
/// está por debajo, deja el aviso en la bandeja de `notifications`.
///
/// Es idempotente: la `dedupe_key` y el índice único parcial de la migración
/// 023 garantizan que el mismo precio no genere dos avisos. Devuelve `None`
/// cuando no hay objetivo, cuando no hay precio en esa moneda, o cuando el
/// precio no ha bajado del objetivo. **La moneda tiene que coincidir**: un
/// objetivo en euros no se compara nunca con un precio en dólares.
pub fn evaluate_alert(
    connection: &Connection,
    app_id: u32,
    now: DateTime<Utc>,
) -> AppResult<Option<PriceAlert>> {
    let target: Option<(Option<i64>, Option<String>)> = connection
        .query_row(
            "SELECT target_price_cents, currency FROM wishlist_entries WHERE app_id = ?1
              UNION ALL
             SELECT target_price_cents, currency FROM catalog_wishlist_entries WHERE app_id = ?1
             LIMIT 1",
            [app_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((Some(target_cents), Some(target_currency))) = target else {
        return Ok(None);
    };
    let currency = target_currency.trim().to_uppercase();
    if currency.len() != 3 {
        return Ok(None);
    }

    let observed: Option<(i64, String)> = connection
        .query_row(
            "SELECT final_cents, observed_at
               FROM game_prices
              WHERE app_id = ?1 AND currency = ?2",
            params![app_id, currency],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((final_cents, observed_at)) = observed else {
        return Ok(None);
    };
    if final_cents > target_cents {
        return Ok(None);
    }

    let title: String = connection
        .query_row(
            "SELECT title FROM games WHERE app_id = ?1",
            [app_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found("El juego ya no está en la biblioteca."))?;

    let dedupe_key = format!("price_drop:{app_id}:{currency}:{final_cents}");
    let body = format!(
        "Ahora {} frente a tu objetivo de {}. Precio observado el {}; puede haber cambiado desde entonces.",
        format_amount(final_cents, &currency),
        format_amount(target_cents, &currency),
        observed_at
    );
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO notification_events(
             id, rule_id, app_id, kind, severity, title, body, occurred_at, dedupe_key
         ) VALUES (?1, NULL, ?2, 'price_drop', 'success', ?3, ?4, ?5, ?6)",
        params![
            Uuid::new_v4().to_string(),
            app_id,
            format!("Bajada de precio: {title}"),
            body,
            now.to_rfc3339_opts(SecondsFormat::Millis, true),
            dedupe_key
        ],
    )?;

    Ok(Some(PriceAlert {
        app_id,
        currency,
        final_cents,
        target_cents,
        dedupe_key,
        created: inserted == 1,
    }))
}

/// Importe legible en castellano: coma decimal y el código de moneda detrás.
/// No se elige símbolo aquí; el símbolo es cosa de la interfaz, que sí conoce
/// la configuración regional de quien lee.
fn format_amount(cents: i64, currency: &str) -> String {
    format!("{},{:02} {currency}", cents / 100, cents % 100)
}

fn ensure_game_exists(connection: &Connection, app_id: u32) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation("El juego indicado no es válido."));
    }
    connection
        .query_row(
            "SELECT 1 FROM games WHERE app_id = ?1",
            [app_id],
            |_| Ok(()),
        )
        .optional()?
        .ok_or_else(|| AppError::not_found(format!("El juego {app_id} no está en la biblioteca.")))
}

/// Borra todo lo observado de un juego. Sirve para cuando la persona quita el
/// juego de deseados y no quiere conservar el rastro de precios.
pub fn forget_prices(connection: &mut Connection, app_id: u32) -> AppResult<()> {
    let transaction = connection.transaction()?;
    transaction.execute("DELETE FROM game_price_history WHERE app_id = ?1", [app_id])?;
    transaction.execute("DELETE FROM game_prices WHERE app_id = ?1", [app_id])?;
    transaction.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Lectura
// ---------------------------------------------------------------------------

const PRICE_COLUMNS: &str = "app_id, currency, country_code, final_cents, initial_cents,
     discount_percent, lowest_cents, lowest_observed_at, changed_at, observed_at, source";

fn map_price(row: &rusqlite::Row<'_>, now: DateTime<Utc>) -> rusqlite::Result<GamePrice> {
    let observed_at: String = row.get(9)?;
    let (freshness, age_minutes) = freshness_of(&observed_at, now);
    Ok(GamePrice {
        app_id: row.get(0)?,
        currency: row.get(1)?,
        country_code: row.get(2)?,
        final_cents: row.get(3)?,
        initial_cents: row.get(4)?,
        discount_percent: row.get::<_, i64>(5)?.clamp(0, 100) as u8,
        lowest_cents: row.get(6)?,
        lowest_observed_at: row.get(7)?,
        changed_at: row.get(8)?,
        observed_at,
        source: row.get(10)?,
        freshness,
        age_minutes,
    })
}

/// Todos los precios conocidos de un juego, una fila por moneda observada.
pub fn prices_for_game(
    connection: &Connection,
    app_id: u32,
    now: DateTime<Utc>,
) -> AppResult<Vec<GamePrice>> {
    let mut statement = connection.prepare(&format!(
        "SELECT {PRICE_COLUMNS} FROM game_prices WHERE app_id = ?1 ORDER BY currency ASC"
    ))?;
    let prices = statement
        .query_map([app_id], |row| map_price(row, now))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(prices)
}

fn price_for(
    connection: &Connection,
    app_id: u32,
    currency: &str,
    now: DateTime<Utc>,
) -> AppResult<Option<GamePrice>> {
    connection
        .query_row(
            &format!("SELECT {PRICE_COLUMNS} FROM game_prices WHERE app_id = ?1 AND currency = ?2"),
            params![app_id, currency],
            |row| map_price(row, now),
        )
        .optional()
        .map_err(Into::into)
}

/// Serie histórica de un juego en una moneda, de la observación más antigua a
/// la más reciente. Con `limit` se recortan los puntos más viejos, nunca los
/// recientes: una gráfica truncada por el final mentiría sobre el precio de hoy.
pub fn price_history(
    connection: &Connection,
    app_id: u32,
    currency: &str,
    limit: u32,
) -> AppResult<PriceHistory> {
    let currency = currency.trim().to_uppercase();
    if currency.len() != 3 {
        return Err(AppError::validation(
            "La serie de precios necesita una moneda ISO 4217.",
        ));
    }
    let limit = if limit == 0 {
        MAX_HISTORY_POINTS
    } else {
        limit.min(MAX_HISTORY_POINTS)
    };
    let mut statement = connection.prepare(
        "SELECT observed_at, final_cents, initial_cents, discount_percent, source
           FROM game_price_history
          WHERE app_id = ?1 AND currency = ?2
          ORDER BY observed_at DESC
          LIMIT ?3",
    )?;
    let mut points = statement
        .query_map(params![app_id, currency, limit], |row| {
            Ok(PricePoint {
                observed_at: row.get(0)?,
                final_cents: row.get(1)?,
                initial_cents: row.get(2)?,
                discount_percent: row.get::<_, i64>(3)?.clamp(0, 100) as u8,
                source: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    points.reverse();
    Ok(PriceHistory {
        app_id,
        currency,
        points,
    })
}

/// Situación de precio de cada entrada de deseados.
///
/// Devuelve una fila por entrada, también para las que no tienen ningún precio
/// observado: un hueco declarado vale más que una lista incompleta que parece
/// completa.
pub fn wishlist_price_statuses(
    connection: &Connection,
    now: DateTime<Utc>,
) -> AppResult<Vec<WishlistPriceStatus>> {
    let mut entries_statement = connection.prepare(
        "SELECT app_id, target_price_cents, currency FROM wishlist_entries
          UNION ALL
         SELECT app_id, target_price_cents, currency FROM catalog_wishlist_entries
          ORDER BY app_id ASC",
    )?;
    let entries = entries_statement
        .query_map([], |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(entries_statement);

    let mut statuses = Vec::with_capacity(entries.len());
    for (app_id, target_cents, target_currency) in entries {
        let observed = prices_for_game(connection, app_id, now)?;
        let normalized_target_currency = target_currency
            .as_deref()
            .map(|value| value.trim().to_uppercase())
            .filter(|value| value.len() == 3);

        // Con objetivo se enseña la moneda del objetivo. Sin objetivo sólo se
        // enseña un precio si no hay ambigüedad: con varias monedas observadas
        // y sin criterio, elegir una sería inventarse la preferencia.
        let chosen = match normalized_target_currency.as_deref() {
            Some(currency) => observed.iter().find(|price| price.currency == currency),
            None if observed.len() == 1 => observed.first(),
            None => None,
        };
        let other_currencies = observed
            .iter()
            .filter(|price| Some(price.currency.as_str()) != chosen.map(|p| p.currency.as_str()))
            .map(|price| price.currency.clone())
            .collect::<Vec<_>>();

        let comparable = matches!((target_cents, chosen), (Some(_), Some(_)));
        let difference_cents = match (target_cents, chosen) {
            (Some(target), Some(price)) => Some(price.final_cents - target),
            _ => None,
        };
        statuses.push(WishlistPriceStatus {
            app_id,
            target_cents,
            target_currency: normalized_target_currency,
            price: chosen.cloned(),
            other_currencies,
            comparable,
            difference_cents,
            meets_target: difference_cents.is_some_and(|difference| difference <= 0),
        });
    }
    Ok(statuses)
}

/// Juegos de deseados a los que toca volver a preguntar el precio: primero los
/// que nunca se han consultado, después los de observación más antigua.
///
/// Devolver esta lista es todo lo que este módulo hace respecto a la red: quien
/// llama decide cuándo y con qué ritmo pedirle nada a la tienda.
pub fn stale_wishlist_app_ids(
    connection: &Connection,
    now: DateTime<Utc>,
    limit: u32,
) -> AppResult<Vec<u32>> {
    let limit = if limit == 0 {
        MAX_REFRESH_BATCH
    } else {
        limit.min(MAX_REFRESH_BATCH)
    };
    let cutoff =
        (now - chrono::Duration::hours(FRESH_HOURS)).to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut statement = connection.prepare(
        "SELECT w.app_id,
                (SELECT MAX(p.observed_at) FROM game_prices p WHERE p.app_id = w.app_id) AS seen
           FROM (SELECT app_id FROM wishlist_entries
                  UNION ALL
                 SELECT app_id FROM catalog_wishlist_entries) w
          WHERE seen IS NULL OR seen < ?1
          ORDER BY seen IS NOT NULL ASC, seen ASC, w.app_id ASC
          LIMIT ?2",
    )?;
    let ids = statement
        .query_map(params![cutoff, limit], |row| row.get::<_, u32>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrations, seed_defaults};
    use chrono::TimeZone;

    fn database() -> Connection {
        let mut connection = Connection::open_in_memory().expect("abrir SQLite en memoria");
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .expect("activar claves foráneas");
        migrations::migrate(&mut connection).expect("migrar");
        seed_defaults(&mut connection).expect("sembrar valores por defecto");
        connection
    }

    fn game(connection: &Connection, app_id: u32, title: &str) {
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (?1, ?2)",
                params![app_id, title],
            )
            .expect("insertar juego");
        connection
            .execute(
                "INSERT INTO game_personal(app_id, status_id) VALUES (?1, 'backlog')",
                [app_id],
            )
            .expect("insertar ficha personal");
    }

    fn wish(connection: &Connection, app_id: u32, target: Option<i64>, currency: Option<&str>) {
        connection
            .execute(
                "INSERT INTO wishlist_entries(app_id, bucket, target_price_cents, currency)
                 VALUES (?1, 'waiting_sale', ?2, ?3)",
                params![app_id, target, currency],
            )
            .expect("insertar deseado");
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, day, hour, 0, 0)
            .single()
            .expect("instante válido")
    }

    fn observation(app_id: u32, currency: &str, final_cents: i64) -> PriceObservation {
        PriceObservation {
            app_id,
            currency: currency.to_string(),
            country_code: Some("ES".to_string()),
            final_cents,
            initial_cents: 5999,
            discount_percent: 0,
            source: String::new(),
        }
    }

    fn events(connection: &Connection) -> Vec<(String, String)> {
        let mut statement = connection
            .prepare("SELECT kind, dedupe_key FROM notification_events ORDER BY dedupe_key ASC")
            .expect("preparar");
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("consultar")
            .collect::<Result<Vec<_>, _>>()
            .expect("recoger")
    }

    // --- Sin precio conocido ------------------------------------------------

    #[test]
    fn un_juego_sin_observaciones_no_tiene_precio_ni_inventa_cero() {
        let mut connection = database();
        game(&connection, 10, "Sin precio");
        wish(&connection, 10, Some(2999), Some("EUR"));

        let statuses = wishlist_price_statuses(&connection, at(1, 12)).expect("estados");
        assert_eq!(statuses.len(), 1);
        let status = &statuses[0];
        assert!(status.price.is_none());
        assert!(!status.comparable);
        assert!(status.difference_cents.is_none());
        assert!(!status.meets_target);
        // Con objetivo pero sin precio no hay nada que comparar y no hay aviso.
        assert!(
            evaluate_alert(&connection, 10, at(1, 12))
                .expect("evaluar")
                .is_none()
        );
        assert!(events(&connection).is_empty());

        // Y sigue apareciendo en la lista de lo que hay que consultar.
        let pending = stale_wishlist_app_ids(&connection, at(1, 12), 0).expect("pendientes");
        assert_eq!(pending, vec![10]);
        let _ = &mut connection;
    }

    #[test]
    fn una_ficha_sin_bloque_de_precio_no_produce_observacion() {
        // Juego gratuito o aún sin publicar: `appdetails` omite `price_overview`.
        let ausente = serde_json::json!(null);
        assert!(
            parse_store_price_overview(10, Some("ES"), &ausente)
                .expect("analizar")
                .is_none()
        );
        // Bloque presente pero sin moneda: tampoco es un precio.
        let sin_moneda = serde_json::json!({ "final": 1999 });
        assert!(
            parse_store_price_overview(10, Some("ES"), &sin_moneda)
                .expect("analizar")
                .is_none()
        );
    }

    #[test]
    fn analiza_el_bloque_real_de_appdetails() {
        // Forma fija tomada de una respuesta de `appdetails`; ninguna red.
        let bloque = serde_json::json!({
            "currency": "EUR",
            "initial": 5999,
            "final": 2999,
            "discount_percent": 50,
            "initial_formatted": "59,99€",
            "final_formatted": "29,99€"
        });
        let observation = parse_store_price_overview(10, Some("es"), &bloque)
            .expect("analizar")
            .expect("hay precio");
        assert_eq!(observation.currency, "EUR");
        assert_eq!(observation.initial_cents, 5999);
        assert_eq!(observation.final_cents, 2999);
        assert_eq!(observation.discount_percent, 50);
        assert_eq!(observation.country_code.as_deref(), Some("es"));
    }

    // --- Precio en otra moneda ----------------------------------------------

    #[test]
    fn un_objetivo_en_euros_no_se_compara_con_un_precio_en_dolares() {
        let mut connection = database();
        game(&connection, 10, "Otra moneda");
        wish(&connection, 10, Some(2999), Some("EUR"));

        let mut usd = observation(10, "USD", 1999);
        usd.country_code = Some("US".to_string());
        let recorded = record_observation(&mut connection, &usd, at(1, 12)).expect("registrar");
        // El precio en dólares está por debajo de 29,99, pero el objetivo es en
        // euros: no hay comparación posible y no hay aviso.
        assert!(recorded.alert.is_none());
        assert!(events(&connection).is_empty());

        let statuses = wishlist_price_statuses(&connection, at(1, 12)).expect("estados");
        let status = &statuses[0];
        assert!(status.price.is_none());
        assert!(!status.comparable);
        assert_eq!(status.other_currencies, vec!["USD".to_string()]);
    }

    #[test]
    fn las_monedas_conviven_sin_pisarse() {
        let mut connection = database();
        game(&connection, 10, "Dos monedas");
        record_observation(&mut connection, &observation(10, "EUR", 2999), at(1, 12))
            .expect("registrar euros");
        record_observation(&mut connection, &observation(10, "USD", 3499), at(1, 12))
            .expect("registrar dólares");

        let prices = prices_for_game(&connection, 10, at(1, 12)).expect("precios");
        assert_eq!(prices.len(), 2);
        assert_eq!(prices[0].currency, "EUR");
        assert_eq!(prices[0].final_cents, 2999);
        assert_eq!(prices[1].currency, "USD");
        assert_eq!(prices[1].final_cents, 3499);
    }

    #[test]
    fn un_importe_sin_moneda_valida_se_rechaza() {
        let mut connection = database();
        game(&connection, 10, "Moneda rara");
        let mut invalid = observation(10, "EUROS", 1999);
        invalid.discount_percent = 0;
        let error =
            record_observation(&mut connection, &invalid, at(1, 12)).expect_err("debe rechazar");
        assert!(error.to_string().contains("moneda"));
    }

    // --- El precio baja del objetivo ----------------------------------------

    #[test]
    fn un_precio_por_debajo_del_objetivo_deja_un_aviso_en_la_bandeja() {
        let mut connection = database();
        game(&connection, 10, "Rebajado");
        wish(&connection, 10, Some(2999), Some("EUR"));

        let recorded =
            record_observation(&mut connection, &observation(10, "EUR", 1999), at(1, 12))
                .expect("registrar");
        let alert = recorded.alert.expect("hay aviso");
        assert!(alert.created);
        assert_eq!(alert.final_cents, 1999);
        assert_eq!(alert.target_cents, 2999);
        assert_eq!(alert.dedupe_key, "price_drop:10:EUR:1999");

        let stored = events(&connection);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, "price_drop");

        // El cuerpo del aviso dice cuándo se observó: nunca afirma «es el
        // precio», sino «este precio se vio este día».
        let body: String = connection
            .query_row("SELECT body FROM notification_events", [], |row| row.get(0))
            .expect("leer cuerpo");
        assert!(body.contains("19,99 EUR"));
        assert!(body.contains("29,99 EUR"));
        assert!(body.contains("puede haber cambiado"));
    }

    #[test]
    fn el_mismo_precio_no_avisa_dos_veces_pero_uno_mas_bajo_si() {
        let mut connection = database();
        game(&connection, 10, "Rebajado");
        wish(&connection, 10, Some(2999), Some("EUR"));

        record_observation(&mut connection, &observation(10, "EUR", 1999), at(1, 12))
            .expect("primera");
        // Segunda consulta con el mismo importe: la `dedupe_key` lo frena.
        let repeated =
            record_observation(&mut connection, &observation(10, "EUR", 1999), at(2, 12))
                .expect("segunda");
        let alert = repeated.alert.expect("evalúa");
        assert!(!alert.created);
        assert_eq!(events(&connection).len(), 1);

        // Otro escalón más abajo sí merece un aviso nuevo.
        record_observation(&mut connection, &observation(10, "EUR", 1499), at(3, 12))
            .expect("tercera");
        assert_eq!(events(&connection).len(), 2);
    }

    #[test]
    fn sin_objetivo_no_hay_aviso_por_barato_que_este() {
        let mut connection = database();
        game(&connection, 10, "Sin objetivo");
        wish(&connection, 10, None, None);
        let recorded = record_observation(&mut connection, &observation(10, "EUR", 99), at(1, 12))
            .expect("registrar");
        assert!(recorded.alert.is_none());
        assert!(events(&connection).is_empty());

        // Sin objetivo y con una sola moneda observada sí se enseña el precio.
        let statuses = wishlist_price_statuses(&connection, at(1, 12)).expect("estados");
        assert_eq!(statuses[0].price.as_ref().expect("precio").final_cents, 99);
        assert!(!statuses[0].comparable);
    }

    // --- El precio sube -----------------------------------------------------

    #[test]
    fn un_precio_que_sube_no_avisa_y_conserva_el_minimo_visto() {
        let mut connection = database();
        game(&connection, 10, "Vuelve a subir");
        wish(&connection, 10, Some(2999), Some("EUR"));

        record_observation(&mut connection, &observation(10, "EUR", 1999), at(1, 12))
            .expect("rebaja");
        let subida = record_observation(&mut connection, &observation(10, "EUR", 5999), at(2, 12))
            .expect("subida");

        assert!(subida.changed);
        assert!(subida.alert.is_none());
        assert_eq!(subida.price.final_cents, 5999);
        // El mínimo sigue siendo el que se vio, con su fecha.
        assert_eq!(subida.price.lowest_cents, 1999);
        assert!(subida.price.lowest_observed_at.starts_with("2026-03-01"));
        // Y sigue habiendo un único aviso: el de la rebaja anterior.
        assert_eq!(events(&connection).len(), 1);

        let status = &wishlist_price_statuses(&connection, at(2, 12)).expect("estados")[0];
        assert!(status.comparable);
        assert_eq!(status.difference_cents, Some(3000));
        assert!(!status.meets_target);
    }

    #[test]
    fn confirmar_el_mismo_precio_refresca_la_fecha_sin_ensuciar_el_historial() {
        let mut connection = database();
        game(&connection, 10, "Estable");
        record_observation(&mut connection, &observation(10, "EUR", 2999), at(1, 12))
            .expect("primera");
        let repeated =
            record_observation(&mut connection, &observation(10, "EUR", 2999), at(5, 12))
                .expect("segunda");

        assert!(!repeated.changed);
        // La frescura avanza…
        assert!(repeated.price.observed_at.starts_with("2026-03-05"));
        // …pero la fecha del cambio se conserva.
        assert!(repeated.price.changed_at.starts_with("2026-03-01"));

        let history = price_history(&connection, 10, "EUR", 0).expect("historial");
        assert_eq!(history.points.len(), 1);
    }

    #[test]
    fn el_historial_dibuja_la_evolucion_en_orden() {
        let mut connection = database();
        game(&connection, 10, "Serie");
        for (day, cents) in [(1, 5999), (2, 2999), (3, 1999), (4, 4999)] {
            record_observation(&mut connection, &observation(10, "EUR", cents), at(day, 12))
                .expect("registrar");
        }
        let history = price_history(&connection, 10, "EUR", 0).expect("historial");
        assert_eq!(history.currency, "EUR");
        assert_eq!(
            history
                .points
                .iter()
                .map(|point| point.final_cents)
                .collect::<Vec<_>>(),
            vec![5999, 2999, 1999, 4999]
        );
        // Recortar deja los puntos recientes, nunca al revés.
        let recortado = price_history(&connection, 10, "EUR", 2).expect("historial corto");
        assert_eq!(
            recortado
                .points
                .iter()
                .map(|point| point.final_cents)
                .collect::<Vec<_>>(),
            vec![1999, 4999]
        );
    }

    // --- Observación caducada ------------------------------------------------

    #[test]
    fn una_observacion_vieja_se_marca_caducada_en_lugar_de_borrarse() {
        let mut connection = database();
        game(&connection, 10, "Caducado");
        record_observation(&mut connection, &observation(10, "EUR", 2999), at(1, 12))
            .expect("registrar");

        // Mismo día: vigente.
        let fresco = prices_for_game(&connection, 10, at(1, 18)).expect("precios");
        assert_eq!(fresco[0].freshness, FRESHNESS_FRESH);
        assert_eq!(fresco[0].age_minutes, Some(360));

        // Tres días después: envejecido pero utilizable.
        let envejecido = prices_for_game(&connection, 10, at(4, 12)).expect("precios");
        assert_eq!(envejecido[0].freshness, FRESHNESS_AGING);

        // Pasada la semana: caducado. El dato sigue ahí, con su fecha.
        let caducado = prices_for_game(&connection, 10, at(20, 12)).expect("precios");
        assert_eq!(caducado[0].freshness, FRESHNESS_STALE);
        assert_eq!(caducado[0].final_cents, 2999);
        assert!(caducado[0].observed_at.starts_with("2026-03-01"));
    }

    #[test]
    fn un_sello_ilegible_o_futuro_no_finge_una_edad() {
        let (word, age) = freshness_of("no soy una fecha", at(1, 12));
        assert_eq!(word, FRESHNESS_UNKNOWN);
        assert!(age.is_none());

        let (word, age) = freshness_of("2026-03-10T12:00:00.000Z", at(1, 12));
        assert_eq!(word, FRESHNESS_UNKNOWN);
        assert!(age.is_none());
    }

    #[test]
    fn el_barrido_prioriza_lo_nunca_visto_y_luego_lo_mas_rancio() {
        let mut connection = database();
        for (app_id, title) in [(10, "Nunca visto"), (20, "Rancio"), (30, "Reciente")] {
            game(&connection, app_id, title);
            wish(&connection, app_id, None, None);
        }
        record_observation(&mut connection, &observation(20, "EUR", 999), at(1, 12))
            .expect("rancio");
        record_observation(&mut connection, &observation(30, "EUR", 999), at(10, 12))
            .expect("reciente");

        // A las 12:00 del día 10: el 30 acaba de mirarse y no entra.
        let pending = stale_wishlist_app_ids(&connection, at(10, 12), 0).expect("pendientes");
        assert_eq!(pending, vec![10, 20]);
    }

    #[test]
    fn olvidar_los_precios_de_un_juego_no_toca_los_de_otro() {
        let mut connection = database();
        game(&connection, 10, "Uno");
        game(&connection, 20, "Otro");
        record_observation(&mut connection, &observation(10, "EUR", 999), at(1, 12)).expect("uno");
        record_observation(&mut connection, &observation(20, "EUR", 1999), at(1, 12))
            .expect("otro");

        forget_prices(&mut connection, 10).expect("olvidar");
        assert!(
            prices_for_game(&connection, 10, at(1, 12))
                .expect("precios")
                .is_empty()
        );
        assert_eq!(
            prices_for_game(&connection, 20, at(1, 12))
                .expect("precios")
                .len(),
            1
        );
    }

    #[test]
    fn un_juego_que_no_esta_en_la_biblioteca_no_admite_precio() {
        let mut connection = database();
        let error = record_observation(&mut connection, &observation(999, "EUR", 999), at(1, 12))
            .expect_err("debe rechazar");
        assert!(error.to_string().contains("no está en la biblioteca"));
    }
}
