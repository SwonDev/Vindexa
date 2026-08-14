use crate::error::{AppError, AppResult};

const SERVICE: &str = "io.vindexa.desktop";
const API_KEY_ACCOUNT: &str = "steam-web-api-key";

fn entry() -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, API_KEY_ACCOUNT).map_err(keyring_error)
}

pub fn save_api_key(value: &str) -> AppResult<()> {
    let value = value.trim();
    validate_api_key(value)?;
    entry()?.set_password(value).map_err(keyring_error)
}

pub fn load_api_key() -> AppResult<Option<String>> {
    match entry()?.get_password() {
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
    match entry()?.delete_credential() {
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
