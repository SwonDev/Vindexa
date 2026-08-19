use crate::error::{AppError, AppResult};

const SERVICE: &str = "io.vindexa.desktop";
const API_KEY_ACCOUNT: &str = "steam-web-api-key";
/// Testigo de sesión de Steam, el que autoriza los servicios de Familia.
///
/// Va en una entrada aparte de la Web API Key porque son credenciales
/// distintas con vidas distintas: la clave la escribe la persona y dura, el
/// testigo lo reparte la sesión y caduca. Revocar una no puede tirar la otra.
const SESSION_TOKEN_ACCOUNT: &str = "steam-session-token";

// Todo pasa por `crate::keychain`, que en las pruebas guarda en memoria: una
// prueba no puede pedirle la contraseña del llavero a quien esté delante.
use crate::keychain;

pub fn save_api_key(value: &str) -> AppResult<()> {
    let value = value.trim();
    validate_api_key(value)?;
    keychain::set(SERVICE, API_KEY_ACCOUNT, value).map_err(keyring_error)
}

pub fn load_api_key() -> AppResult<Option<String>> {
    match keychain::get(SERVICE, API_KEY_ACCOUNT) {
        Ok(value) => {
            validate_api_key(&value)?;
            Ok(Some(value))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    }
}

pub fn has_api_key() -> AppResult<bool> {
    load_api_key().map(|value| value.is_some())
}

pub fn delete_api_key() -> AppResult<()> {
    match keychain::delete(SERVICE, API_KEY_ACCOUNT) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    }
}

/// Guarda el testigo de sesión. Nunca se registra ni se enseña.
pub fn save_session_token(value: &str) -> AppResult<()> {
    let value = value.trim();
    crate::steam::family_session::validate_token(value)?;
    keychain::set(SERVICE, SESSION_TOKEN_ACCOUNT, value).map_err(keyring_error)
}

/// Recupera el testigo guardado, si lo hay.
///
/// Un testigo con forma inválida se trata como ausente en lugar de propagar un
/// error: lo que hay que hacer es volver a iniciar sesión, y una entrada
/// corrupta no puede dejar la función bloqueada para siempre.
pub fn load_session_token() -> AppResult<Option<String>> {
    match keychain::get(SERVICE, SESSION_TOKEN_ACCOUNT) {
        Ok(value) => Ok(crate::steam::family_session::validate_token(&value)
            .is_ok()
            .then_some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(keyring_error(error)),
    }
}

/// ¿Hay un testigo de sesión guardado?
///
/// No dice si sigue siendo válido: eso sólo lo sabe Steam, y comprobarlo
/// costaría una petición. La caducidad se descubre al usarlo y se cuenta
/// entonces.
pub fn has_session_token() -> AppResult<bool> {
    load_session_token().map(|value| value.is_some())
}

pub fn delete_session_token() -> AppResult<()> {
    match keychain::delete(SERVICE, SESSION_TOKEN_ACCOUNT) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    }
}

pub fn validate_api_key(value: &str) -> AppResult<()> {
    if value.len() != 32 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::validation(
            "La clave de Steam Web API debe contener exactamente 32 caracteres hexadecimales.",
        ));
    }
    Ok(())
}

fn keyring_error(_error: keyring::Error) -> AppError {
    AppError::new(
        "secure_storage",
        "No se pudo acceder al almacén seguro del sistema.",
    )
}

#[cfg(test)]
mod tests {
    use super::{keyring_error, validate_api_key};

    #[test]
    fn validates_exact_hex_key_without_persisting_it() {
        assert!(validate_api_key("0123456789abcdef0123456789ABCDEF").is_ok());
        assert!(validate_api_key("short").is_err());
        assert!(validate_api_key("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }

    #[test]
    fn secure_storage_errors_do_not_expose_platform_details() {
        let error = keyring_error(keyring::Error::Invalid(
            "account".to_string(),
            "secret=/Users/example/private.key".to_string(),
        ));

        assert_eq!(error.code, "secure_storage");
        assert_eq!(
            error.message,
            "No se pudo acceder al almacén seguro del sistema."
        );
        assert!(!error.message.contains("private.key"));
        assert!(!error.message.contains("secret"));
    }
}
