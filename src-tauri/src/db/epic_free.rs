//! Los regalos de Epic, guardados y cruzados con lo que ya se tiene.
//!
//! El módulo `stores::epic_free` sabe hablar con Epic; éste sabe qué hacer con
//! lo que trae: guardarlo sin repetirlo, decir si el juego ya está en la
//! biblioteca y dejar un aviso **una sola vez** por regalo.
//!
//! # Por qué «ya lo tienes» se calcula aquí
//!
//! Epic no sabe qué tienes en Vindexa —ni debe saberlo—, y Vindexa guarda los
//! juegos de Epic con su título, no con el identificador de la tienda. El cruce
//! se hace por título normalizado con el mismo comparador que usa la
//! sincronización de tiendas, para que «Ghostrunner 2» y «Ghostrunner II» no
//! cuenten como juegos distintos.
//!
//! # Lo que no se sabe, no se dice
//!
//! Un regalo sin fecha de fin se guarda igual, pero la interfaz no dirá que
//! caduca pronto. Un cruce dudoso **no** marca el juego como poseído: es
//! preferible ofrecer un regalo que ya se tiene a esconder uno que falta.

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::stores::epic_free::{EpicFreeGame, FreeGameState};
use crate::stores::matching::normalize_title;

/// Un regalo tal y como lo enseña la interfaz: lo que dijo Epic más lo que
/// Vindexa sabe de tu biblioteca.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicFreeOffer {
    #[serde(flatten)]
    pub game: EpicFreeGame,
    /// Ya está en la biblioteca: no hay nada que reclamar.
    pub owned: bool,
    /// Horas hasta que se acabe, o `null` si Epic no publicó el fin.
    pub hours_left: Option<i64>,
    /// Se descartó a mano y no debe volver a molestar.
    pub dismissed: bool,
}

/// Resultado de guardar una tanda de regalos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpicFreeSyncReport {
    /// Regalos que Epic devolvió.
    pub received: u32,
    /// Regalos que Vindexa no había visto nunca.
    pub discovered: u32,
    /// Avisos creados: uno por regalo vigente, nuevo y no poseído.
    pub notified: u32,
    /// De los recibidos, cuántos ya están en la biblioteca.
    pub already_owned: u32,
}

/// Guarda lo que Epic ha dicho y deja aviso de lo que sea noticia.
///
/// Es idempotente: llamarla dos veces con la misma respuesta no crea un segundo
/// aviso ni duplica filas. Lo que sí hace es refrescar el estado de una oferta
/// que pasó de «anunciada» a «vigente», porque eso sí es noticia.
pub fn sync(
    connection: &mut Connection,
    games: &[EpicFreeGame],
    now: DateTime<Utc>,
) -> AppResult<EpicFreeSyncReport> {
    let owned_index = owned_titles(connection)?;
    let transaction = connection.transaction()?;
    let mut report = EpicFreeSyncReport {
        received: games.len() as u32,
        ..EpicFreeSyncReport::default()
    };
    let sello = now.to_rfc3339();

    for game in games {
        let owned = owned_index.contains(&normalize_title(&game.title));
        if owned {
            report.already_owned = report.already_owned.saturating_add(1);
        }

        let anterior: Option<(String, Option<String>)> = transaction
            .query_row(
                "SELECT state, notified_at FROM epic_free_offers WHERE offer_id = ?1",
                [&game.offer_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if anterior.is_none() {
            report.discovered = report.discovered.saturating_add(1);
        }

        transaction.execute(
            "INSERT INTO epic_free_offers(
                 offer_id, title, description, store_url, image_url, state,
                 starts_at, ends_at, original_price_cents, currency, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(offer_id) DO UPDATE SET
                 title = excluded.title,
                 description = excluded.description,
                 store_url = excluded.store_url,
                 image_url = excluded.image_url,
                 state = excluded.state,
                 starts_at = excluded.starts_at,
                 ends_at = excluded.ends_at,
                 original_price_cents = excluded.original_price_cents,
                 currency = excluded.currency,
                 updated_at = excluded.updated_at",
            params![
                game.offer_id,
                game.title,
                game.description,
                game.store_url,
                game.image_url,
                state_label(game.state),
                game.starts_at,
                game.ends_at,
                game.original_price_cents,
                game.currency,
                sello,
            ],
        )?;

        // Se avisa de lo que se puede reclamar hoy y aún no se tiene. Un regalo
        // anunciado para dentro de una semana no es una noticia accionable, y
        // uno que ya está en la biblioteca no es una noticia en absoluto.
        let ya_avisado = anterior
            .as_ref()
            .is_some_and(|(_, avisado)| avisado.is_some());
        if game.state == FreeGameState::Current && !owned && !ya_avisado {
            let clave = format!("epic_free:{}", game.offer_id);
            let cuerpo = match (game.original_price_cents, game.currency.as_deref()) {
                (Some(cents), Some(currency)) => format!(
                    "Gratis en Epic hasta que acabe la promoción. Fuera de ella cuesta {}.",
                    format_amount(cents, currency)
                ),
                _ => "Gratis en Epic hasta que acabe la promoción.".to_string(),
            };
            let insertadas = transaction.execute(
                "INSERT INTO notification_events(
                     id, kind, severity, title, body, occurred_at, dedupe_key)
                 VALUES (lower(hex(randomblob(16))), 'epic_free_game', 'info', ?1, ?2, ?3, ?4)
                 ON CONFLICT DO NOTHING",
                params![game.title, cuerpo, sello, clave],
            )?;
            if insertadas > 0 {
                report.notified = report.notified.saturating_add(1);
            }
            transaction.execute(
                "UPDATE epic_free_offers SET notified_at = ?1 WHERE offer_id = ?2",
                params![sello, game.offer_id],
            )?;
        }
    }

    transaction.commit()?;
    Ok(report)
}

/// Lo guardado, cruzado con la biblioteca y listo para enseñar.
///
/// Se devuelve también lo caducado que aún no se ha limpiado, con sus horas a
/// `null`: la interfaz decide si lo enseña, pero no se le oculta un dato.
pub fn list(connection: &Connection, now: DateTime<Utc>) -> AppResult<Vec<EpicFreeOffer>> {
    let owned_index = owned_titles(connection)?;
    let mut statement = connection.prepare(
        "SELECT offer_id, title, description, store_url, image_url, state,
                starts_at, ends_at, original_price_cents, currency, dismissed_at
           FROM epic_free_offers
          ORDER BY state = 'current' DESC, ends_at ASC, title ASC",
    )?;
    let filas = statement
        .query_map([], |row| {
            let state: String = row.get(5)?;
            Ok((
                EpicFreeGame {
                    offer_id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    store_url: row.get(3)?,
                    image_url: row.get(4)?,
                    state: if state == "current" {
                        FreeGameState::Current
                    } else {
                        FreeGameState::Upcoming
                    },
                    starts_at: row.get(6)?,
                    ends_at: row.get(7)?,
                    original_price_cents: row.get(8)?,
                    currency: row.get(9)?,
                },
                row.get::<_, Option<String>>(10)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(filas
        .into_iter()
        .map(|(game, dismissed_at)| EpicFreeOffer {
            owned: owned_index.contains(&normalize_title(&game.title)),
            hours_left: game.hours_left(now),
            dismissed: dismissed_at.is_some(),
            game,
        })
        .collect())
}

/// Descarta un regalo para que deje de aparecer y de avisar.
pub fn dismiss(connection: &Connection, offer_id: &str, now: DateTime<Utc>) -> AppResult<()> {
    connection.execute(
        "UPDATE epic_free_offers SET dismissed_at = ?1 WHERE offer_id = ?2",
        params![now.to_rfc3339(), offer_id],
    )?;
    Ok(())
}

/// Títulos normalizados de lo que ya está en la biblioteca.
///
/// Se miran **todos** los juegos, no sólo los de Epic: un regalo de Epic que ya
/// se tiene en Steam se sigue pudiendo reclamar, pero quien lo mira quiere saber
/// que ya lo ha jugado. Marcarlo como poseído es más útil que esconderlo.
fn owned_titles(connection: &Connection) -> AppResult<std::collections::HashSet<String>> {
    let mut statement = connection.prepare("SELECT title FROM games")?;
    let titulos = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(titulos.iter().map(|title| normalize_title(title)).collect())
}

fn state_label(state: FreeGameState) -> &'static str {
    match state {
        FreeGameState::Current => "current",
        FreeGameState::Upcoming => "upcoming",
    }
}

/// Importe legible. Las monedas de dos decimales cubren todo lo que Epic usa en
/// Europa; para el resto se enseña el número con su código y no se inventa un
/// símbolo.
fn format_amount(cents: i64, currency: &str) -> String {
    let unidades = cents / 100;
    let resto = (cents % 100).abs();
    match currency {
        "EUR" => format!("{unidades},{resto:02} €"),
        "USD" => format!("{unidades},{resto:02} $"),
        "GBP" => format!("£{unidades},{resto:02}"),
        otro => format!("{unidades},{resto:02} {otro}"),
    }
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
        seed_defaults(&mut connection).expect("sembrar");
        connection
    }

    fn regalo(offer_id: &str, title: &str, state: FreeGameState) -> EpicFreeGame {
        EpicFreeGame {
            offer_id: offer_id.to_string(),
            title: title.to_string(),
            description: String::new(),
            store_url: format!("https://store.epicgames.com/es-ES/p/{offer_id}"),
            image_url: None,
            state,
            starts_at: Some("2026-08-14T15:00:00Z".to_string()),
            ends_at: Some("2026-08-21T15:00:00Z".to_string()),
            original_price_cents: Some(2499),
            currency: Some("EUR".to_string()),
        }
    }

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, day, hour, 0, 0)
            .single()
            .expect("instante válido")
    }

    fn avisos(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare("SELECT dedupe_key FROM notification_events ORDER BY dedupe_key")
            .expect("preparar");
        statement
            .query_map([], |row| row.get::<_, Option<String>>(0))
            .expect("consultar")
            .filter_map(|clave| clave.ok().flatten())
            .collect()
    }

    #[test]
    fn un_regalo_vigente_deja_un_aviso_y_solo_uno() {
        let mut connection = database();
        let juegos = [regalo("of-1", "Caravan SandWitch", FreeGameState::Current)];

        let primero = sync(&mut connection, &juegos, at(19, 10)).expect("guardar");
        assert_eq!(primero.discovered, 1);
        assert_eq!(primero.notified, 1);

        // La segunda tanda trae lo mismo: ni fila nueva ni aviso nuevo.
        let segundo = sync(&mut connection, &juegos, at(19, 12)).expect("guardar otra vez");
        assert_eq!(segundo.discovered, 0);
        assert_eq!(segundo.notified, 0);
        assert_eq!(avisos(&connection), ["epic_free:of-1"]);
    }

    #[test]
    fn un_regalo_solo_anunciado_no_avisa_todavia() {
        // Avisar de algo que aún no se puede reclamar es ruido: cuando llegue su
        // turno, la oferta pasará a vigente y entonces sí.
        let mut connection = database();
        let juegos = [regalo("of-2", "Ghostrunner 2", FreeGameState::Upcoming)];
        let report = sync(&mut connection, &juegos, at(19, 10)).expect("guardar");
        assert_eq!(report.discovered, 1);
        assert_eq!(report.notified, 0);
        assert!(avisos(&connection).is_empty());
    }

    #[test]
    fn cuando_lo_anunciado_pasa_a_vigente_entonces_avisa() {
        let mut connection = database();
        sync(
            &mut connection,
            &[regalo("of-2", "Ghostrunner 2", FreeGameState::Upcoming)],
            at(19, 10),
        )
        .expect("guardar anunciado");
        let report = sync(
            &mut connection,
            &[regalo("of-2", "Ghostrunner 2", FreeGameState::Current)],
            at(21, 16),
        )
        .expect("guardar vigente");
        assert_eq!(report.discovered, 0);
        assert_eq!(report.notified, 1);
    }

    #[test]
    fn un_regalo_que_ya_esta_en_la_biblioteca_no_avisa_pero_se_enseña() {
        // No avisa porque no hay nada que reclamar; se enseña marcado porque
        // esconderlo dejaría a quien mira preguntándose si se lo ha perdido.
        let mut connection = database();
        connection
            .execute(
                "INSERT INTO games(app_id, title) VALUES (10, 'Caravan Sandwitch')",
                [],
            )
            .expect("insertar juego");

        let report = sync(
            &mut connection,
            &[regalo("of-1", "Caravan SandWitch", FreeGameState::Current)],
            at(19, 10),
        )
        .expect("guardar");
        assert_eq!(report.already_owned, 1);
        assert_eq!(report.notified, 0);

        let lista = list(&connection, at(19, 11)).expect("listar");
        assert_eq!(lista.len(), 1);
        assert!(lista[0].owned);
    }

    #[test]
    fn las_horas_que_faltan_salen_del_reloj_y_no_del_deseo() {
        let mut connection = database();
        sync(
            &mut connection,
            &[regalo("of-1", "Caravan SandWitch", FreeGameState::Current)],
            at(19, 10),
        )
        .expect("guardar");

        let lista = list(&connection, at(20, 15)).expect("listar");
        assert_eq!(lista[0].hours_left, Some(24));

        // Ya caducado: no se enseñan horas negativas.
        let tarde = list(&connection, at(25, 15)).expect("listar tarde");
        assert_eq!(tarde[0].hours_left, None);
    }

    #[test]
    fn lo_descartado_queda_marcado_sin_borrarse() {
        let mut connection = database();
        sync(
            &mut connection,
            &[regalo("of-1", "Caravan SandWitch", FreeGameState::Current)],
            at(19, 10),
        )
        .expect("guardar");
        dismiss(&connection, "of-1", at(19, 11)).expect("descartar");

        let lista = list(&connection, at(19, 12)).expect("listar");
        assert!(lista[0].dismissed, "sigue estando, pero descartado");
    }

    #[test]
    fn lo_vigente_va_antes_que_lo_anunciado() {
        let mut connection = database();
        sync(
            &mut connection,
            &[
                regalo("of-2", "Anunciado", FreeGameState::Upcoming),
                regalo("of-1", "Vigente", FreeGameState::Current),
            ],
            at(19, 10),
        )
        .expect("guardar");

        let lista = list(&connection, at(19, 11)).expect("listar");
        assert_eq!(lista[0].game.title, "Vigente");
        assert_eq!(lista[1].game.title, "Anunciado");
    }

    #[test]
    fn una_tanda_vacia_no_borra_lo_que_habia_ni_inventa_nada() {
        let mut connection = database();
        sync(
            &mut connection,
            &[regalo("of-1", "Caravan SandWitch", FreeGameState::Current)],
            at(19, 10),
        )
        .expect("guardar");

        let report = sync(&mut connection, &[], at(19, 12)).expect("tanda vacía");
        assert_eq!(report.received, 0);
        assert_eq!(report.discovered, 0);
        assert_eq!(list(&connection, at(19, 12)).expect("listar").len(), 1);
    }
}
