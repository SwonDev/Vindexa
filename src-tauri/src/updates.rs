//! Comprobación manual de versiones publicadas.
//!
//! # Qué hace y qué no
//!
//! Pregunta a GitHub cuál es la última versión publicada de Vindexa y la
//! compara con la que se está ejecutando. **No descarga nada y no instala
//! nada.** Instalar por su cuenta exigiría firmar los artefactos y llevar la
//! clave pública dentro de la aplicación; mientras eso no exista, lo honesto es
//! avisar y dejar que la persona decida.
//!
//! # Por qué la comparación es propia
//!
//! El proyecto avanza el tercer número hasta diez antes de subir el segundo
//! —0.1.9 → 0.1.10 → 0.2.0—, así que comparar como texto daría que `0.1.9` es
//! posterior a `0.1.10`. Se comparan los tres números por separado.

use crate::error::{AppError, AppResult};
use reqwest::{Client, StatusCode, header, redirect::Policy};
use serde::Deserialize;
use std::cmp::Ordering;
use std::time::Duration;

const RELEASES_ENDPOINT: &str = "https://api.github.com/repos/SwonDev/Vindexa/releases/latest";

/// Página donde se descargan los instaladores.
pub const RELEASES_PAGE: &str = "https://github.com/SwonDev/Vindexa/releases/latest";

/// Cuerpo máximo aceptado de la respuesta.
///
/// La ficha de una versión trae sus notas y la lista de artefactos: unas pocas
/// decenas de kilobytes. Este margen sobra.
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

#[derive(Debug, Deserialize)]
struct ReleasePayload {
    #[serde(default)]
    tag_name: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Última versión publicada, sin la `v` de la etiqueta.
///
/// `None` cuando todavía no hay ninguna. Un borrador o una versión preliminar
/// tampoco cuentan: no son lo que alguien debería instalar.
pub async fn latest_published_version() -> AppResult<Option<String>> {
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        // Sin redirecciones: la comprobación no tiene por qué acabar en otro
        // destino, y seguir una escondería un cambio de anfitrión.
        .redirect(Policy::none())
        .user_agent("Vindexa/0.1 (+https://github.com/SwonDev/Vindexa)")
        .build()
        .map_err(|_| unreachable_error())?;

    let mut response = client
        .get(RELEASES_ENDPOINT)
        .header(header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| unreachable_error())?;

    // Sin ninguna versión publicada, GitHub responde 404. No es un fallo.
    if response.status() == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(unreachable_error());
    }

    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unreachable_error())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(unreachable_error());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_release(&bytes)
}

fn parse_release(bytes: &[u8]) -> AppResult<Option<String>> {
    let payload: ReleasePayload = serde_json::from_slice(bytes).map_err(|_| unreachable_error())?;
    if payload.draft || payload.prerelease {
        return Ok(None);
    }
    Ok(payload
        .tag_name
        .as_deref()
        .map(str::trim)
        .and_then(|tag| tag.strip_prefix('v').or(Some(tag)))
        .map(str::to_owned)
        .filter(|version| !version.is_empty()))
}

/// Compara dos versiones `mayor.menor.parche`.
///
/// `None` cuando alguna no tiene esa forma: es preferible decir que no se ha
/// podido comparar a elegir una de las dos respuestas al azar.
pub fn compare(izquierda: &str, derecha: &str) -> Option<Ordering> {
    Some(numeros(izquierda)?.cmp(&numeros(derecha)?))
}

/// Los tres números de una versión, o `None` si no los tiene.
///
/// Se ignora cualquier sufijo tras un guion (`0.2.0-rc1`): compararlo bien
/// exigiría las reglas completas del versionado semántico, y aquí no hacen
/// falta porque las versiones preliminares ya se descartan antes.
fn numeros(version: &str) -> Option<[u32; 3]> {
    let base = version.trim().split('-').next()?;
    let mut partes = base.split('.');
    let mayor = partes.next()?.parse().ok()?;
    let menor = partes.next()?.parse().ok()?;
    let parche = partes.next()?.parse().ok()?;
    if partes.next().is_some() {
        return None;
    }
    Some([mayor, menor, parche])
}

fn unreachable_error() -> AppError {
    AppError::new(
        "update_check_unreachable",
        "No se ha podido consultar la página de versiones. Comprueba tu conexión y vuelve a intentarlo.",
    )
}

#[cfg(test)]
mod tests {
    use super::{compare, parse_release};
    use std::cmp::Ordering;

    #[test]
    fn el_tercer_numero_se_compara_como_numero_y_no_como_texto() {
        // Es el caso que rompe la comparación de textos, y justo el que usa el
        // proyecto: 0.1.9 → 0.1.10 → 0.2.0.
        assert_eq!(compare("0.1.9", "0.1.10"), Some(Ordering::Less));
        assert_eq!(compare("0.1.10", "0.1.9"), Some(Ordering::Greater));
        assert_eq!(compare("0.1.10", "0.2.0"), Some(Ordering::Less));
        assert_eq!(compare("0.1.0", "0.1.0"), Some(Ordering::Equal));
    }

    #[test]
    fn una_version_con_otra_forma_no_se_compara_a_la_ligera() {
        assert_eq!(compare("0.1", "0.1.0"), None);
        assert_eq!(compare("0.1.0.1", "0.1.0"), None);
        assert_eq!(compare("nightly", "0.1.0"), None);
        assert_eq!(compare("0.1.0", ""), None);
    }

    #[test]
    fn el_sufijo_preliminar_no_impide_comparar_los_numeros() {
        assert_eq!(compare("0.2.0-rc1", "0.1.10"), Some(Ordering::Greater));
    }

    #[test]
    fn lee_la_etiqueta_quitandole_la_uve() {
        let payload = br#"{"tag_name":"v0.1.4","draft":false,"prerelease":false}"#;
        assert_eq!(
            parse_release(payload).expect("interpretar"),
            Some("0.1.4".to_string())
        );
    }

    #[test]
    fn una_etiqueta_sin_uve_tambien_vale() {
        let payload = br#"{"tag_name":"0.1.4","draft":false,"prerelease":false}"#;
        assert_eq!(
            parse_release(payload).expect("interpretar"),
            Some("0.1.4".to_string())
        );
    }

    #[test]
    fn un_borrador_o_una_preliminar_no_cuentan_como_publicadas() {
        // Nadie debería instalar ninguna de las dos, así que ofrecerlas sería
        // empujar hacia algo que no está listo.
        let borrador = br#"{"tag_name":"v0.2.0","draft":true,"prerelease":false}"#;
        assert_eq!(parse_release(borrador).expect("interpretar"), None);
        let preliminar = br#"{"tag_name":"v0.2.0","draft":false,"prerelease":true}"#;
        assert_eq!(parse_release(preliminar).expect("interpretar"), None);
    }

    #[test]
    fn una_respuesta_sin_etiqueta_no_inventa_una_version() {
        assert_eq!(
            parse_release(br#"{"draft":false}"#).expect("interpretar"),
            None
        );
    }

    #[test]
    fn una_respuesta_ilegible_es_un_fallo_de_consulta() {
        let error = parse_release(b"no es json").expect_err("debe fallar");
        assert_eq!(error.code, "update_check_unreachable");
    }
}
