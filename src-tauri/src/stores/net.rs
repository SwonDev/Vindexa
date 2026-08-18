//! Capa de red compartida por los clientes de Epic y de GOG.
//!
//! Existe por dos motivos que no se pueden resolver en cada tienda por
//! separado:
//!
//! 1. **Los errores no pueden hablar de Steam.** `AppError` convierte
//!    `reqwest::Error` con un mensaje de Steam (ver `crate::error`), así que
//!    aquí **nunca** se usa `?` sobre un error de red: se traduce a mano con
//!    [`network_error`], que nombra la tienda correcta.
//! 2. **Una respuesta de terceros no puede crecer sin medida.** Todo cuerpo se
//!    lee por trozos contra un tope antes de intentar interpretarlo, de forma
//!    que un servidor que responda sin fin no se lleve la memoria del proceso.
//!
//! Ningún mensaje de error de este módulo incorpora la URL, la cabecera de
//! autorización ni el cuerpo de la petición: cualquiera de los tres puede
//! contener un testigo o un código de autorización.

use crate::error::{AppError, AppResult};
use crate::stores::ExternalStore;
use reqwest::{Client, Response, StatusCode};
use serde::de::DeserializeOwned;
use std::sync::OnceLock;
use std::time::Duration;

/// Tope de bytes que se aceptan de una sola respuesta. La página más grande de
/// las que se piden —la lista de recursos de Epic de una biblioteca amplia— se
/// queda muy por debajo.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Cliente compartido. Reutilizar la conexión importa: una sincronización pide
/// una ficha por juego, y abrir un TLS nuevo cada vez multiplicaría el tiempo.
static CLIENT: OnceLock<Client> = OnceLock::new();

pub fn client() -> AppResult<&'static Client> {
    if let Some(client) = CLIENT.get() {
        return Ok(client);
    }
    let built = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .https_only(true)
        .build()
        .map_err(|_| {
            AppError::new(
                "external_store_network",
                "No se pudo preparar la conexión segura con la tienda.",
            )
        })?;
    Ok(CLIENT.get_or_init(|| built))
}

/// Error de transporte: no hubo respuesta, o no se pudo leer.
pub fn network_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_network",
        format!(
            "No se pudo contactar con {}. Comprueba tu conexión y vuelve a intentarlo.",
            store.display_name()
        ),
    )
}

/// La tienda respondió que la sesión ya no vale. Es recuperable iniciando
/// sesión otra vez, y se dice así en vez de como un fallo genérico.
pub fn auth_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_auth",
        format!(
            "{} ha rechazado la sesión guardada. Vuelve a iniciar sesión.",
            store.display_name()
        ),
    )
}

/// La tienda respondió, pero con algo que no se puede usar.
pub fn response_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_response",
        format!(
            "{} devolvió una respuesta que Vindexa no ha podido interpretar.",
            store.display_name()
        ),
    )
}

/// Le pide a la tienda que espere: se ha alcanzado su límite de peticiones.
pub fn rate_limited_error(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_rate_limited",
        format!(
            "{} está limitando las peticiones ahora mismo. Espera un momento y vuelve a sincronizar.",
            store.display_name()
        ),
    )
}

/// Traduce el código de estado antes de leer el cuerpo.
///
/// Se separa de la lectura porque un 401 no necesita cuerpo para diagnosticarse
/// y porque el cuerpo de un error de OAuth puede repetir el código de
/// autorización que se acaba de enviar.
pub fn check_status(store: ExternalStore, status: StatusCode) -> AppResult<()> {
    match status {
        status if status.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(auth_error(store)),
        StatusCode::TOO_MANY_REQUESTS => Err(rate_limited_error(store)),
        _ => Err(response_error(store)),
    }
}

/// Lee el cuerpo por trozos hasta el tope y lo interpreta como JSON.
pub async fn read_json<T: DeserializeOwned>(
    store: ExternalStore,
    response: Response,
) -> AppResult<T> {
    let body = read_capped(store, response).await?;
    serde_json::from_slice(&body).map_err(|_| response_error(store))
}

async fn read_capped(store: ExternalStore, mut response: Response) -> AppResult<Vec<u8>> {
    // Si la tienda declara de antemano un tamaño imposible se rechaza sin leer
    // un solo byte.
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(response_error(store));
    }
    let mut buffer = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| network_error(store))? {
        if buffer.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(response_error(store));
        }
        buffer.extend_from_slice(&chunk);
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::{auth_error, check_status, network_error, rate_limited_error, response_error};
    use crate::stores::ExternalStore;
    use reqwest::StatusCode;

    #[test]
    fn network_messages_name_the_store_and_never_steam() {
        for store in ExternalStore::ALL {
            for error in [
                network_error(store),
                auth_error(store),
                response_error(store),
                rate_limited_error(store),
            ] {
                assert!(error.message.contains(store.display_name()));
                assert!(!error.message.contains("Steam"));
                assert!(error.code.starts_with("external_store_"));
            }
        }
    }

    #[test]
    fn a_rejected_session_is_told_apart_from_a_broken_response() {
        let store = ExternalStore::Epic;
        assert!(check_status(store, StatusCode::OK).is_ok());
        assert_eq!(
            check_status(store, StatusCode::UNAUTHORIZED)
                .unwrap_err()
                .code,
            "external_store_auth"
        );
        assert_eq!(
            check_status(store, StatusCode::FORBIDDEN).unwrap_err().code,
            "external_store_auth"
        );
        assert_eq!(
            check_status(store, StatusCode::TOO_MANY_REQUESTS)
                .unwrap_err()
                .code,
            "external_store_rate_limited"
        );
        assert_eq!(
            check_status(store, StatusCode::INTERNAL_SERVER_ERROR)
                .unwrap_err()
                .code,
            "external_store_response"
        );
    }
}
