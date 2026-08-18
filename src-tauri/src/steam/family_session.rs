//! Testigo de sesión de Steam para los servicios de Familia.
//!
//! # Por qué hace falta otra credencial
//!
//! El catálogo de una Familia de Steam **no se puede leer con una Web API Key**.
//! La vía que había —preguntar `IPlayerService/GetOwnedGames` por cada miembro—
//! sólo funciona si cada uno tiene su biblioteca pública para esa clave, y la
//! mayoría no la tiene. Por eso el cliente de Steam enseña miles de juegos
//! prestados y Vindexa se quedaba en los propios.
//!
//! Los servicios `IFamilyGroupsService`, que son los que el propio cliente usa,
//! se autentican con un **testigo de sesión**: el mismo que la web de Steam
//! reparte a sus páginas cuando has iniciado sesión. Se obtiene de la sesión que
//! ya vive en el navegador integrado.
//!
//! # Cómo se obtiene
//!
//! Se lleva la ventana de Steam a `pointssummary/ajaxgetasyncconfig`, que la
//! propia web consulta para configurar su cliente, y se lee el documento. La
//! respuesta es JSON plano, así que basta con leer el texto del cuerpo: no hace
//! falta ejecutar nada de la página ni tocar sus cookies.
//!
//! Se reutiliza el recorrido de [`crate::steam::wishlist_session`] —abrir,
//! esperar la carga correcta y evaluar un guion acotado— porque es exactamente
//! el mismo problema con otro destino.
//!
//! # Lo que **no** se hace
//!
//! No se leen cookies, ni `localStorage`, ni ningún otro dato de la sesión. Sólo
//! el testigo. Y el testigo **no se registra ni se muestra**, ni siquiera
//! truncado: con él se puede hablar en nombre de la cuenta.

use crate::error::{AppError, AppResult};
use crate::steam::wishlist_session::{evaluate_json, open_steam_page, wait_for_steam_page};
use tauri::{AppHandle, Runtime, Url};

/// Página que reparte la configuración del cliente web, con el testigo dentro.
const TOKEN_URL: &str = "https://store.steampowered.com/pointssummary/ajaxgetasyncconfig";

const STORE_HOST: &str = "store.steampowered.com";
const TOKEN_PATH: &str = "/pointssummary/ajaxgetasyncconfig";

/// Cuerpo máximo que se acepta de esa página.
///
/// La respuesta son unos cientos de bytes. Este margen sobra y evita interpretar
/// un documento entero si Steam decide devolver otra cosa.
const MAX_PAYLOAD_BYTES: usize = 64 * 1024;

/// Longitud máxima admitida para el testigo.
///
/// Es un JWT: tres segmentos separados por puntos. Mil caracteres dejan holgura
/// de sobra y ponen un tope a lo que se guarda en el llavero.
const MAX_TOKEN_CHARS: usize = 1_000;

/// Lee el cuerpo de la página, que es JSON plano.
///
/// `innerText` primero porque WebKit envuelve el JSON en un `<pre>` y devuelve
/// lo que se ve, sin los adornos del visor; `textContent` como respaldo por si
/// el motor lo presenta de otra forma.
///
/// El guion vuelve a comprobar el destino antes de tocar el documento: entre el
/// sondeo y la evaluación la página pudo cambiar, y es la segunda de las tres
/// comprobaciones que se hacen sobre dónde se está leyendo.
///
/// Devuelve `SIN_CUERPO` cuando el documento no tiene nada legible. Sin ese
/// aviso, un cuerpo vacío se confundiría con «no has iniciado sesión», que es
/// un diagnóstico distinto y manda a arreglar algo que no está roto.
const READ_TOKEN_SCRIPT: &str = r#"
(function () {
  if (location.host !== "store.steampowered.com") return "";
  if (location.pathname !== "/pointssummary/ajaxgetasyncconfig") return "";
  var cuerpo = document.body;
  if (!cuerpo) return "SIN_CUERPO";
  var texto = cuerpo.innerText || cuerpo.textContent || "";
  if (!texto.trim()) return "SIN_CUERPO";
  return texto.slice(0, 65536);
})()
"#;

/// Marca que el guion devuelve cuando la página no tiene cuerpo legible.
const SIN_CUERPO: &str = "SIN_CUERPO";

/// URL de la página del testigo.
pub fn token_page_url() -> Url {
    Url::parse(TOKEN_URL).expect("la URL del testigo de Steam es válida")
}

/// ¿Es esta la página del testigo?
///
/// Se comprueba anfitrión **y** ruta: la ventana podría haber acabado en
/// cualquier otro sitio del mismo dominio.
pub fn is_token_page(url: &Url) -> bool {
    url.host_str() == Some(STORE_HOST) && url.path() == TOKEN_PATH
}

/// Testigo válido extraído de la respuesta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Construye un testigo a partir de un valor guardado, validando su forma.
    ///
    /// Existe para que quien lo recupera del llavero no tenga que fabricar una
    /// respuesta de Steam falsa sólo para volver a pasar por el análisis.
    pub fn parse(raw: &str) -> AppResult<Self> {
        let raw = raw.trim();
        validate_token(raw)?;
        Ok(Self(raw.to_owned()))
    }

    /// El testigo en claro, para construir la petición. No se registra ni se
    /// enseña: quien lo obtiene lo usa y lo suelta.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Interpreta la respuesta de la página de configuración.
///
/// Steam responde con un objeto que tiene el testigo bajo `data.webapi_token`.
/// Cuando no hay sesión, el campo llega vacío o no llega: eso **no** es un error
/// de formato, es «no has iniciado sesión», y se distingue porque cada caso
/// lleva a una frase distinta.
pub fn parse_token_payload(raw: &str) -> AppResult<SessionToken> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(not_signed_in());
    }
    if raw == SIN_CUERPO {
        return Err(AppError::new(
            "steam_family_empty_page",
            "Steam devolvió una página vacía en el navegador integrado. Recárgala y vuelve a intentarlo.",
        ));
    }
    if raw.len() > MAX_PAYLOAD_BYTES {
        return Err(AppError::new(
            "steam_family_token_payload",
            "La respuesta de Steam es más larga de lo que Vindexa acepta leer.",
        ));
    }
    let valor: serde_json::Value = serde_json::from_str(raw).map_err(|_| {
        // Con la sesión cerrada, Steam devuelve la página de inicio de sesión en
        // HTML en lugar de JSON. Es el caso más común, así que se cuenta como
        // «no has iniciado sesión» y no como respuesta rota.
        not_signed_in()
    })?;
    let token = valor
        .get("data")
        .and_then(|data| data.get("webapi_token"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .ok_or_else(not_signed_in)?;
    SessionToken::parse(token)
}

/// Comprueba la forma del testigo antes de guardarlo o usarlo.
///
/// No se valida la firma —eso lo hace Steam— pero sí que tenga la forma de un
/// JWT y ningún carácter fuera del alfabeto de base64url. Un valor con espacios
/// o saltos de línea acabaría en una cabecera HTTP mal formada.
pub fn validate_token(token: &str) -> AppResult<()> {
    if token.is_empty() || token.len() > MAX_TOKEN_CHARS {
        return Err(token_shape_error());
    }
    let segmentos: Vec<&str> = token.split('.').collect();
    if segmentos.len() != 3 || segmentos.iter().any(|segmento| segmento.is_empty()) {
        return Err(token_shape_error());
    }
    let alfabeto_valido = token
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if !alfabeto_valido {
        return Err(token_shape_error());
    }
    Ok(())
}

/// Pide a la sesión abierta en el navegador integrado su testigo de Steam.
pub async fn read_session_token<R: Runtime>(app: &AppHandle<R>) -> AppResult<SessionToken> {
    let (window, generacion_anterior) = open_steam_page(app, token_page_url()).await?;
    let label = window.label().to_string();
    wait_for_steam_page(
        &label,
        generacion_anterior,
        is_token_page,
        "Steam no terminó de responder en el navegador integrado. Comprueba esa ventana y vuelve a intentarlo.",
    )
    .await?;
    let raw = evaluate_json(&window, READ_TOKEN_SCRIPT).await?;
    parse_token_payload(&raw)
}

fn not_signed_in() -> AppError {
    AppError::new(
        "steam_family_signed_out",
        "No hay una sesión de Steam abierta en el navegador integrado. Inicia sesión allí y vuelve a intentarlo.",
    )
}

fn token_shape_error() -> AppError {
    AppError::new(
        "steam_family_token_shape",
        "Steam devolvió un testigo de sesión con una forma que Vindexa no reconoce.",
    )
}

#[cfg(test)]
mod tests {
    use super::{is_token_page, parse_token_payload, token_page_url, validate_token};
    use tauri::Url;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    // Un testigo con la forma real: tres segmentos de base64url. El contenido es
    // inventado a propósito; ninguna prueba de este archivo usa uno de verdad.
    const TESTIGO: &str = "eyJ0eXAiOiJKV1QifQ.eyJzdWIiOiIwIn0.ZmlybWFfZGVfcHJ1ZWJh";

    #[test]
    fn extrae_el_testigo_de_la_respuesta_de_steam() {
        let payload = format!(r#"{{"success":1,"data":{{"webapi_token":"{TESTIGO}"}}}}"#);
        let token = parse_token_payload(&payload).expect("interpretar respuesta");
        assert_eq!(token.as_str(), TESTIGO);
    }

    #[test]
    fn una_respuesta_sin_testigo_es_sesion_cerrada_no_error_de_formato() {
        // La diferencia importa: una dice «inicia sesión» y la otra «algo se ha
        // roto». Confundirlas manda a la persona a buscar un problema que no
        // existe.
        let error = parse_token_payload(r#"{"success":1,"data":{}}"#).expect_err("debe fallar");
        assert_eq!(error.code, "steam_family_signed_out");

        let vacio = parse_token_payload(r#"{"success":1,"data":{"webapi_token":""}}"#)
            .expect_err("debe fallar");
        assert_eq!(vacio.code, "steam_family_signed_out");
    }

    #[test]
    fn una_pagina_sin_cuerpo_no_se_confunde_con_sesion_cerrada() {
        // Son dos problemas distintos: uno se arregla iniciando sesión y el otro
        // recargando. Decir el equivocado manda a arreglar algo que no falla.
        let error = parse_token_payload("SIN_CUERPO").expect_err("debe fallar");
        assert_eq!(error.code, "steam_family_empty_page");
    }

    #[test]
    fn la_pagina_de_inicio_de_sesion_en_html_cuenta_como_sesion_cerrada() {
        // Con la sesión cerrada Steam no devuelve JSON, devuelve su página de
        // login. Es el caso más frecuente y no puede presentarse como una
        // respuesta rota.
        let error = parse_token_payload("<!DOCTYPE html><html><body>Sign in</body></html>")
            .expect_err("debe fallar");
        assert_eq!(error.code, "steam_family_signed_out");
        assert_eq!(
            parse_token_payload("   ").expect_err("debe fallar").code,
            "steam_family_signed_out"
        );
    }

    #[test]
    fn rechaza_un_testigo_con_forma_que_no_es_la_suya() {
        for candidato in [
            "",
            "sin-puntos",
            "dos.segmentos",
            "tres..vacio",
            "con espacio.en.medio",
            "salto\n.de.linea",
        ] {
            assert!(
                validate_token(candidato).is_err(),
                "«{candidato}» no debería aceptarse"
            );
        }
        assert!(validate_token(TESTIGO).is_ok());
    }

    #[test]
    fn el_constructor_valida_y_recorta_los_espacios() {
        let token = super::SessionToken::parse(&format!("  {TESTIGO}  ")).expect("construir");
        assert_eq!(token.as_str(), TESTIGO);
        assert!(super::SessionToken::parse("no.es").is_err());
    }

    #[test]
    fn rechaza_un_testigo_desmesurado() {
        let largo = format!(
            "{}.{}.{}",
            "a".repeat(400),
            "b".repeat(400),
            "c".repeat(400)
        );
        assert!(validate_token(&largo).is_err());
    }

    #[test]
    fn la_pagina_del_testigo_se_reconoce_por_anfitrion_y_ruta() {
        assert!(is_token_page(&token_page_url()));
        assert!(is_token_page(&url(
            "https://store.steampowered.com/pointssummary/ajaxgetasyncconfig?l=spanish"
        )));
        // Mismo dominio, otra página: no vale.
        assert!(!is_token_page(&url(
            "https://store.steampowered.com/app/620"
        )));
        // Misma ruta, otro dominio: tampoco.
        assert!(!is_token_page(&url(
            "https://store.steampowered.com.example.com/pointssummary/ajaxgetasyncconfig"
        )));
    }
}
