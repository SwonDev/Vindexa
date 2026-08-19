//! De dónde salen los próximos lanzamientos que Vindexa puntúa.
//!
//! # El hueco que cierra este módulo
//!
//! El motor de gustos (`db::priority`) sabe aprender qué te gusta y puntuar
//! candidatos con ese modelo, y la pantalla sabe enseñarlos. Lo que faltaba era
//! quien trajera los candidatos: `upcoming_releases` no la rellenaba nadie, así
//! que la sección quedaba siempre vacía por muy bien que funcionara todo lo
//! demás.
//!
//! # De dónde se sacan, y por qué de ahí
//!
//! De **tu lista de deseados**. Es la única fuente honesta que hay a mano: son
//! juegos que ya has marcado tú, no una lista de novedades que a Vindexa se le
//! ocurra recomendar. Un juego de esa lista que la tienda declara sin publicar
//! es exactamente un próximo lanzamiento que te interesa.
//!
//! Que sea `coming_soon` lo dice la ficha oficial; no se deduce de que falte la
//! fecha, porque una fecha ausente también puede ser una fecha ilegible. Ver
//! [`crate::steam::store_api::ReleaseWindow`].
//!
//! # Qué no hace
//!
//! - **No descubre juegos nuevos.** No consulta listas de novedades ni de
//!   tendencias: si no está en tus deseados, no aparece.
//! - **No manda tus gustos a ninguna parte.** El modelo se aplica en local
//!   sobre lo ya descargado, como documenta `db::priority`.
//! - **No inventa una fecha.** Cuando la tienda sólo publica «Q4 2026», eso es
//!   lo que se guarda, marcado como no exacta.

use crate::db::Database;
use crate::db::priority::{ImportedUpcomingRelease, UpcomingImportSummary};
use crate::error::AppResult;
use crate::steam::store_api::{self, StoreBundleOutcome};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;

/// Cuántos deseados se revisan como mucho en cada pasada.
///
/// Cada uno cuesta una petición a la tienda. Con una lista de novecientos,
/// revisarlos todos de una vez son novecientas peticiones seguidas: Steam
/// respondería con un límite de frecuencia mucho antes. Se revisan por tandas y
/// se completan en pasadas sucesivas.
const MAX_PER_RUN: usize = 60;

/// Espera entre fichas. La sincronización de metadatos usa el mismo criterio:
/// ir despacio sale más barato que ir rápido y que te frenen.
const BETWEEN_REQUESTS: Duration = Duration::from_millis(350);

/// Tope de la descripción que acepta `db::priority::upsert_upcoming`.
///
/// Alguna ficha de Steam trae un resumen más largo. Se recorta aquí en vez de
/// ensanchar el límite de la base: es un resumen para reconocer el juego de un
/// vistazo, no el texto íntegro, y quien lo quiera entero tiene la ficha.
const MAX_DESCRIPTION_CHARS: usize = 2_000;

/// Origen que queda escrito en `upcoming_releases.source`.
///
/// Uno de los tres que admite `db::priority::UPCOMING_SOURCES`: el dato viene
/// de la ficha oficial de la tienda, aunque quien decide a qué juegos mirar sea
/// la lista de deseados.
const SOURCE: &str = "store";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingRefreshReport {
    /// Deseados revisados en esta pasada.
    pub checked: u32,
    /// De ellos, cuántos siguen sin publicarse.
    pub upcoming: u32,
    /// Cuántos entraron por primera vez.
    pub inserted: u32,
    /// Cuántos ya estaban y se han refrescado.
    pub updated: u32,
    /// Fichas que la tienda no devolvió. No es un error: hay juegos retirados.
    pub unavailable: u32,
    /// Quedan deseados por revisar en la próxima pasada.
    pub pending: u32,
}

/// Revisa una tanda de deseados y guarda los que aún no han salido.
///
/// Se empieza por los que hace más tiempo que no se miran, de modo que pasadas
/// sucesivas acaben cubriendo la lista entera sin repetir siempre los mismos.
pub async fn refresh_from_wishlist(database: &Database) -> AppResult<UpcomingRefreshReport> {
    let candidates = pending_candidates(database, MAX_PER_RUN)?;
    let pending_total = pending_count(database)?;

    let mut items: Vec<ImportedUpcomingRelease> = Vec::new();
    let mut checked = 0_u32;
    let mut unavailable = 0_u32;

    for (index, (app_id, fallback_title)) in candidates.iter().enumerate() {
        if index > 0 {
            sleep(BETWEEN_REQUESTS).await;
        }
        checked += 1;
        match store_api::fetch_bundle_with_retry_hint(*app_id).await {
            Ok(StoreBundleOutcome::Found(bundle)) => {
                if !bundle.release.coming_soon {
                    continue;
                }
                let metadata = &bundle.metadata;
                items.push(ImportedUpcomingRelease {
                    app_id: *app_id,
                    // El título sale del catálogo de deseados: la ficha de la
                    // tienda no devuelve el nombre en este paquete.
                    title: fallback_title.clone(),
                    capsule_url: metadata.capsule_url.clone(),
                    header_url: metadata.header_url.clone(),
                    release_date: bundle.release.label.clone(),
                    // La etiqueta sólo es una fecha exacta si la propia
                    // `metadata` supo interpretarla como tal.
                    release_date_is_exact: metadata.release_date.is_some(),
                    genres: metadata.genres.clone(),
                    categories: metadata.categories.clone(),
                    developer: metadata.developer.clone(),
                    publisher: metadata.publisher.clone(),
                    short_description: metadata.short_description.as_deref().map(recortar),
                    source: SOURCE.to_string(),
                });
            }
            Ok(StoreBundleOutcome::Unavailable) => unavailable += 1,
            // Un fallo de red corta la pasada, pero no tira lo ya reunido: se
            // guarda lo que hay y la siguiente pasada sigue donde ésta lo dejó.
            Err(_) => break,
        }
    }

    let upcoming = items.len() as u32;
    let summary: UpcomingImportSummary =
        crate::db::priority::upsert_upcoming(&mut database.open()?, &items)?;
    mark_checked(database, &candidates)?;

    Ok(UpcomingRefreshReport {
        checked,
        upcoming,
        inserted: summary.inserted,
        updated: summary.updated,
        unavailable,
        pending: pending_total.saturating_sub(checked),
    })
}

/// Recorta un resumen largo por el último espacio que quepa.
///
/// Cortar por el carácter exacto parte palabras y, si se cortara por bytes,
/// además rompería cualquier letra acentuada. Se corta por palabra y se marca
/// con puntos suspensivos, para que se vea que hay más.
fn recortar(texto: &str) -> String {
    if texto.chars().count() <= MAX_DESCRIPTION_CHARS {
        return texto.to_owned();
    }
    let recortado: String = texto.chars().take(MAX_DESCRIPTION_CHARS - 1).collect();
    let corte = recortado.rfind(' ').unwrap_or(recortado.len());
    format!("{}…", recortado[..corte].trim_end())
}

/// Deseados que llevan más tiempo sin revisarse.
fn pending_candidates(database: &Database, limit: usize) -> AppResult<Vec<(u32, String)>> {
    let connection = database.open()?;
    // Los que nunca se han mirado van primero —su marca es la cadena vacía— y
    // detrás los más antiguos. Así una lista larga se cubre entera en pasadas
    // sucesivas en vez de repetir siempre la cabeza.
    let mut statement = connection.prepare(
        "SELECT c.app_id, c.title
           FROM catalog_wishlist_entries w
           JOIN catalog_games c ON c.app_id = w.app_id
           LEFT JOIN upcoming_checks k ON k.app_id = c.app_id
          ORDER BY COALESCE(k.checked_at, '') ASC, c.app_id ASC
          LIMIT ?1",
    )?;
    let rows = statement.query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn pending_count(database: &Database) -> AppResult<u32> {
    let connection = database.open()?;
    Ok(
        connection.query_row("SELECT COUNT(*) FROM catalog_wishlist_entries", [], |row| {
            row.get(0)
        })?,
    )
}

/// Deja constancia de que estos ya se han mirado, aunque no fueran candidatos.
///
/// Sin esto, los que ya han salido volverían a encabezar la cola en cada pasada
/// y las siguientes nunca llegarían al resto de la lista.
fn mark_checked(database: &Database, candidates: &[(u32, String)]) -> AppResult<()> {
    if candidates.is_empty() {
        return Ok(());
    }
    let mut connection = database.open()?;
    let transaction = connection.transaction()?;
    {
        let mut remember = transaction.prepare(
            "INSERT INTO upcoming_checks(app_id, checked_at)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(app_id) DO UPDATE
                SET checked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )?;
        for (app_id, _) in candidates {
            remember.execute([app_id])?;
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn base() -> (TempDir, Database) {
        let directory = TempDir::new().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("migrar");
        (directory, database)
    }

    fn deseado(database: &Database, app_id: u32, title: &str) {
        let connection = database.open().expect("abrir");
        connection
            .execute(
                "INSERT INTO catalog_games(app_id, title) VALUES (?1, ?2)",
                rusqlite::params![app_id, title],
            )
            .expect("catálogo");
        connection
            .execute(
                "INSERT INTO catalog_wishlist_entries(app_id) VALUES (?1)",
                [app_id],
            )
            .expect("deseado");
    }

    #[test]
    fn la_cola_empieza_por_lo_que_nunca_se_ha_mirado() {
        let (_directory, database) = base();
        for (app_id, title) in [(10_u32, "Uno"), (20, "Dos"), (30, "Tres")] {
            deseado(&database, app_id, title);
        }
        // Dos ya revisados: deben quedar detrás del que no se ha mirado nunca.
        mark_checked(&database, &[(10, "Uno".into()), (30, "Tres".into())]).expect("marcar");

        let orden = pending_candidates(&database, 10).expect("cola");
        assert_eq!(orden.first().map(|(id, _)| *id), Some(20));
        assert_eq!(orden.len(), 3, "los revisados siguen en la lista, al final");
    }

    #[test]
    fn revisar_un_deseado_ya_publicado_tambien_cuenta() {
        // Es lo que impide que la cola se atasque: si sólo se anotaran los que
        // están por salir, los ya publicados volverían a encabezarla en cada
        // pasada y las siguientes nunca llegarían al resto de la lista.
        let (_directory, database) = base();
        deseado(&database, 40, "Ya salió");

        mark_checked(&database, &[(40, "Ya salió".into())]).expect("marcar");

        let anotado: i64 = database
            .open()
            .expect("abrir")
            .query_row(
                "SELECT COUNT(*) FROM upcoming_checks WHERE app_id = 40",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(anotado, 1);
    }

    #[test]
    fn un_resumen_largo_se_recorta_por_palabra_y_sin_romper_letras() {
        // Cortar por byte partiría una letra acentuada por la mitad; cortar por
        // carácter exacto parte la palabra. Se corta por palabra y se avisa con
        // puntos suspensivos de que hay más.
        let corto = "Un resumen corto.";
        assert_eq!(recortar(corto), corto);

        let largo = "áéíóú ".repeat(500);
        let recortado = recortar(&largo);
        assert!(recortado.chars().count() <= MAX_DESCRIPTION_CHARS);
        assert!(recortado.ends_with('…'));
        assert!(
            !recortado.contains("  "),
            "no queda un espacio suelto al final"
        );
        // Y sigue siendo texto legible, no bytes partidos.
        assert!(recortado.starts_with("áéíóú"));
    }

    #[test]
    fn una_lista_vacia_no_toca_nada() {
        let (_directory, database) = base();
        assert!(mark_checked(&database, &[]).is_ok());
        assert!(pending_candidates(&database, 10).expect("cola").is_empty());
    }
}
