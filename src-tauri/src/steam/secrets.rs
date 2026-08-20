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

/// Lo último que dijo el llavero sobre la clave, mientras dure el proceso.
///
/// # Por qué se recuerda
///
/// macOS pregunta por la contraseña del llavero **una vez por cada lectura**
/// mientras la aplicación no esté en la lista de acceso de esa entrada, y una
/// aplicación firmada ad hoc nunca lo está: su firma cambia con cada
/// compilación. El enriquecimiento de fichas, los logros y la sincronización
/// leen la clave por su cuenta, así que un arranque normal encadenaba varios
/// diálogos idénticos.
///
/// Recordar la respuesta durante la vida del proceso deja **una** pregunta.
/// No es un almacén: no se escribe en ningún sitio, muere con la aplicación y
/// se invalida en cuanto la clave cambia o se borra.
static CACHED_API_KEY: std::sync::RwLock<Option<Option<String>>> = std::sync::RwLock::new(None);

/// Olvida lo recordado para que la próxima consulta vuelva a preguntar.
///
/// La llaman los actos explícitos de quien usa la aplicación: si denegó el
/// acceso sin querer, pulsar «verificar» o «sincronizar» tiene que poder volver
/// a intentarlo sin reiniciar.
pub fn forget_cached_api_key() {
    if let Ok(mut cache) = CACHED_API_KEY.write() {
        *cache = None;
    }
}

pub fn save_api_key(value: &str) -> AppResult<()> {
    let value = value.trim();
    validate_api_key(value)?;
    let result = keychain::set(SERVICE, API_KEY_ACCOUNT, value).map_err(keyring_error);
    // Se olvida siempre, también si falló: lo que hay en el llavero después de
    // un intento fallido no se sabe, y un recuerdo que no se sabe si es cierto
    // es peor que no tener ninguno.
    forget_cached_api_key();
    result
}

/// La clave **sólo si ya está recordada**, sin tocar el llavero.
///
/// La diferencia con [`load_api_key`] es quién puede provocar un diálogo del
/// sistema. Un acto explícito —verificar la clave, sincronizar— tiene derecho a
/// pedir la contraseña: quien lo pulsó está delante. Una tarea de fondo no: el
/// diálogo aparece solo, en mitad de otra cosa, y si no hay nadie se queda ahí
/// esperando o se deniega. Pasó tal cual, y quien usa esto lo describió mejor
/// que ningún registro: «volvió a pedir las contraseñas y yo no estaba».
///
/// Devuelve `None` cuando no hay nada recordado —ni la clave ni una negativa—,
/// que es la señal de «todavía no toca preguntar por esto».
pub fn cached_api_key() -> Option<String> {
    CACHED_API_KEY
        .read()
        .ok()
        .and_then(|cache| cache.clone())
        .flatten()
}

pub fn load_api_key() -> AppResult<Option<String>> {
    if let Ok(cache) = CACHED_API_KEY.read()
        && let Some(recordado) = cache.as_ref()
    {
        return Ok(recordado.clone());
    }
    let leido = match keychain::get(SERVICE, API_KEY_ACCOUNT) {
        Ok(value) => {
            validate_api_key(&value)?;
            Some(value)
        }
        Err(keyring::Error::NoEntry) => None,
        // Una negativa también es una respuesta: sin recordarla, cada tarea de
        // fondo vuelve a abrir el mismo diálogo a los pocos segundos. Los actos
        // explícitos —verificar la clave, sincronizar— llaman antes a
        // `forget_cached_api_key`, así que volver a intentarlo siempre es
        // posible sin reiniciar.
        Err(error) => {
            let fallo = keyring_error(error);
            if let Ok(mut cache) = CACHED_API_KEY.write() {
                *cache = Some(None);
            }
            return Err(fallo);
        }
    };
    if let Ok(mut cache) = CACHED_API_KEY.write() {
        *cache = Some(leido.clone());
    }
    Ok(leido)
}

pub fn has_api_key() -> AppResult<bool> {
    load_api_key().map(|value| value.is_some())
}

pub fn delete_api_key() -> AppResult<()> {
    let result = match keychain::delete(SERVICE, API_KEY_ACCOUNT) {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    };
    forget_cached_api_key();
    result
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
    use super::{
        API_KEY_ACCOUNT, SERVICE, delete_api_key, forget_cached_api_key, keychain, keyring_error,
        load_api_key, save_api_key, validate_api_key,
    };

    /// Las pruebas que tocan la clave comparten el recuerdo del proceso y el
    /// llavero de pruebas, que también es único. En paralelo se pisan, así que
    /// van de una en una.
    static EN_SERIE: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    /// La clave se lee una vez por sesión, no una vez por consulta.
    ///
    /// Cada lectura del llavero es un diálogo pidiendo la contraseña mientras la
    /// aplicación no esté en la lista de acceso de esa entrada, y una firmada ad
    /// hoc nunca lo está: su firma cambia con cada compilación. El
    /// enriquecimiento de fichas, los logros y la sincronización leen la clave
    /// por su cuenta, así que un arranque encadenaba varios diálogos idénticos.
    ///
    /// Las dos mitades van en la misma prueba a propósito: comparten el recuerdo
    /// y el llavero de pruebas, y separadas se pisarían al correr en paralelo.
    #[test]
    fn la_clave_se_recuerda_durante_la_sesion_y_se_olvida_al_cambiar() {
        let _turno = EN_SERIE.lock().unwrap_or_else(|error| error.into_inner());
        // El llavero de pruebas guarda en memoria: nada toca el del sistema.
        forget_cached_api_key();
        let clave = "0123456789ABCDEF0123456789ABCDEF";
        save_api_key(clave).expect("guardar");
        assert_eq!(load_api_key().expect("leer"), Some(clave.to_string()));

        // Se borra por detrás, sin pasar por `delete_api_key`: si la respuesta no
        // estuviera recordada, esta lectura devolvería `None` y habría un
        // diálogo más por cada consulta.
        keychain::delete(SERVICE, API_KEY_ACCOUNT).expect("borrar a mano");
        assert_eq!(
            load_api_key().expect("leer otra vez"),
            Some(clave.to_string()),
            "la respuesta recordada sigue valiendo durante la sesión"
        );

        // Y un recuerdo que sobrevive a un cambio es una mentira con fecha.
        let segunda = "FEDCBA9876543210FEDCBA9876543210";
        save_api_key(segunda).expect("guardar otra");
        assert_eq!(load_api_key().expect("leer"), Some(segunda.to_string()));

        delete_api_key().expect("borrar");
        assert_eq!(load_api_key().expect("leer tras borrar"), None);
    }

    /// Una negativa del llavero también se recuerda, y un acto explícito la
    /// olvida.
    ///
    /// Sin recordarla, cada tarea de fondo volvía a abrir el mismo diálogo a los
    /// pocos segundos. Recordándola para siempre, una negativa por error dejaría
    /// Steam apagado hasta reiniciar.
    #[test]
    fn una_negativa_se_recuerda_hasta_que_alguien_lo_pide_de_nuevo() {
        let _turno = EN_SERIE.lock().unwrap_or_else(|error| error.into_inner());
        forget_cached_api_key();
        // El llavero de pruebas no puede denegar, así que se comprueba lo que sí
        // depende de este módulo: que olvidar deja el siguiente acceso limpio.
        let clave = "0123456789ABCDEF0123456789ABCDEF";
        save_api_key(clave).expect("guardar");
        assert_eq!(load_api_key().expect("leer"), Some(clave.to_string()));

        keychain::delete(SERVICE, API_KEY_ACCOUNT).expect("borrar a mano");
        forget_cached_api_key();
        assert_eq!(
            load_api_key().expect("leer tras olvidar"),
            None,
            "olvidar obliga a volver a preguntar"
        );
    }
}
