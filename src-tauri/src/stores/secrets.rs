//! Dónde acaba el testigo de una tienda externa.
//!
//! En el llavero de macOS y en ningún otro sitio: ni en SQLite, ni en un
//! fichero de configuración, ni en un registro, ni en un mensaje de error, ni
//! en la interfaz. Es el mismo patrón que `crate::steam::secrets` aplica a la
//! clave de la Steam Web API, con una entrada por tienda.
//!
//! Que no haya migración que acompañe a este módulo es deliberado: la sesión
//! **no toca la base de datos**. Sobrevive a los reinicios porque el llavero es
//! persistente, no porque se haya copiado a ninguna tabla.
//!
//! [`StoredSession`] implementa `Debug` a mano. Un `dbg!` o un `{:?}` que se
//! cuele en un registro imprime cuántos caracteres tiene el testigo, nunca el
//! testigo.

use crate::error::{AppError, AppResult};
use crate::stores::ExternalStore;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};

/// Mismo servicio que la clave de Steam: Vindexa tiene una sola identidad en el
/// llavero y distingue sus secretos por el nombre de cuenta.
const SERVICE: &str = "io.vindexa.desktop";

/// Tope de caracteres de un testigo. Los `eg1` de Epic rondan el millar; el
/// límite está para que una respuesta anómala no escriba sin medida.
const MAX_TOKEN_CHARS: usize = 8_192;

/// Tope de caracteres del identificador y del nombre visible de la cuenta.
const MAX_ACCOUNT_CHARS: usize = 200;

/// Margen con el que se considera caducado un testigo que todavía no lo está.
/// Renovar dos minutos antes evita que una sincronización larga se quede a
/// medias con un testigo que expira mientras se pagina la biblioteca.
const EXPIRY_MARGIN_SECONDS: i64 = 120;

/// Nombre de cuenta con el que cada tienda guarda su sesión en el llavero.
///
/// Se expone porque la interfaz lo enseña: quien quiera comprobar con Acceso a
/// Llaveros que Vindexa ya no guarda nada necesita saber qué buscar. El nombre
/// no es un secreto; lo que protege el llavero es su contenido.
pub fn keychain_account(store: ExternalStore) -> &'static str {
    match store {
        ExternalStore::Epic => "epic-store-session",
        ExternalStore::Gog => "gog-store-session",
    }
}

/// Nombre del servicio bajo el que se agrupan las entradas de Vindexa.
pub fn keychain_service() -> &'static str {
    SERVICE
}

/// La sesión de una tienda, tal y como se guarda en el llavero.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredSession {
    pub access_token: String,
    pub refresh_token: String,
    /// Caducidad del testigo de acceso, en segundos desde la época.
    pub expires_at: i64,
    /// Caducidad del testigo de refresco, cuando la tienda la publica. Epic la
    /// devuelve; GOG no, y entonces queda en `None` en vez de inventarse.
    #[serde(default)]
    pub refresh_expires_at: Option<i64>,
    /// Identificador de la cuenta dentro de la tienda. GOG lo necesita porque
    /// su API de biblioteca lo lleva en la ruta. No se enseña en la interfaz.
    pub account_id: String,
    /// Nombre visible de la cuenta, cuando la tienda lo devuelve.
    #[serde(default)]
    pub account_name: Option<String>,
}

impl Debug for StoredSession {
    /// Nunca imprime un testigo. Ni entero, ni truncado, ni su prefijo: un
    /// prefijo de testigo sigue siendo material sensible en un informe de
    /// errores.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredSession")
            .field("access_token", &"<oculto>")
            .field("refresh_token", &"<oculto>")
            .field("expires_at", &self.expires_at)
            .field("refresh_expires_at", &self.refresh_expires_at)
            .field("account_id", &"<oculto>")
            .field("account_name", &self.account_name)
            .finish()
    }
}

impl StoredSession {
    /// Comprueba que la sesión es utilizable antes de guardarla o de usarla.
    ///
    /// Se valida tanto al escribir como al leer: el llavero es del sistema, no
    /// de Vindexa, y otro proceso podría haber dejado ahí cualquier cosa.
    pub fn validate(&self) -> AppResult<()> {
        let token_ok = |value: &str| !value.trim().is_empty() && value.chars().count() <= MAX_TOKEN_CHARS;
        if !token_ok(&self.access_token) || !token_ok(&self.refresh_token) {
            return Err(invalid_session());
        }
        if self.account_id.trim().is_empty() || self.account_id.chars().count() > MAX_ACCOUNT_CHARS {
            return Err(invalid_session());
        }
        if self
            .account_name
            .as_ref()
            .is_some_and(|name| name.chars().count() > MAX_ACCOUNT_CHARS)
        {
            return Err(invalid_session());
        }
        Ok(())
    }

    /// ¿Hay que renovar el testigo de acceso antes de usarlo?
    pub fn needs_refresh(&self, now: i64) -> bool {
        now + EXPIRY_MARGIN_SECONDS >= self.expires_at
    }

    /// ¿Ha caducado también el testigo de refresco? Cuando la tienda no publica
    /// su caducidad la respuesta honesta es «no se sabe», y no se sabe no es sí.
    pub fn refresh_expired(&self, now: i64) -> bool {
        self.refresh_expires_at.is_some_and(|limit| now >= limit)
    }
}

fn entry(store: ExternalStore) -> AppResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, keychain_account(store)).map_err(keyring_error)
}

/// Guarda la sesión en el llavero, sustituyendo la anterior si la había.
pub fn save(store: ExternalStore, session: &StoredSession) -> AppResult<()> {
    session.validate()?;
    let encoded = serde_json::to_string(session).map_err(|_| invalid_session())?;
    entry(store)?.set_password(&encoded).map_err(keyring_error)
}

/// Lee la sesión guardada. `None` significa «no hay sesión», que es un estado
/// normal, no un error.
///
/// Una entrada corrupta se trata como ausencia: se borra y se responde `None`.
/// Dejarla ahí condenaría a la tienda a fallar en cada arranque sin que nadie
/// pudiera arreglarlo salvo desde Acceso a Llaveros.
pub fn load(store: ExternalStore) -> AppResult<Option<StoredSession>> {
    let raw = match entry(store)?.get_password() {
        Ok(value) => value,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(error) => return Err(keyring_error(error)),
    };
    match serde_json::from_str::<StoredSession>(&raw) {
        Ok(session) if session.validate().is_ok() => Ok(Some(session)),
        _ => {
            delete(store)?;
            Ok(None)
        }
    }
}

/// ¿Queda algo guardado para esta tienda? Es lo que responde la comprobación
/// que la interfaz ofrece tras cerrar sesión.
pub fn has(store: ExternalStore) -> AppResult<bool> {
    match entry(store)?.get_password() {
        Ok(_) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(keyring_error(error)),
    }
}

/// Borra la sesión. Que no hubiera nada que borrar no es un fallo.
pub fn delete(store: ExternalStore) -> AppResult<()> {
    match entry(store)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(keyring_error(error)),
    }
}

fn invalid_session() -> AppError {
    AppError::new(
        "external_store_session",
        "La sesión guardada de esa tienda no es utilizable. Vuelve a iniciar sesión.",
    )
}

fn keyring_error(_error: keyring::Error) -> AppError {
    AppError::new(
        "secure_storage",
        "No se pudo acceder al almacén seguro del sistema.",
    )
}

#[cfg(test)]
mod tests {
    use super::{EXPIRY_MARGIN_SECONDS, MAX_TOKEN_CHARS, StoredSession, keychain_account, keyring_error};
    use crate::stores::ExternalStore;

    fn session() -> StoredSession {
        StoredSession {
            access_token: "testigo-de-acceso".to_string(),
            refresh_token: "testigo-de-refresco".to_string(),
            expires_at: 1_000,
            refresh_expires_at: Some(100_000),
            account_id: "cuenta-123".to_string(),
            account_name: Some("Persona usuaria".to_string()),
        }
    }

    #[test]
    fn each_store_keeps_its_session_in_its_own_keychain_entry() {
        assert_ne!(
            keychain_account(ExternalStore::Epic),
            keychain_account(ExternalStore::Gog)
        );
    }

    #[test]
    fn debugging_a_session_never_prints_a_token() {
        let printed = format!("{:?}", session());
        assert!(!printed.contains("testigo-de-acceso"));
        assert!(!printed.contains("testigo-de-refresco"));
        assert!(!printed.contains("cuenta-123"));
        // El nombre visible sí puede aparecer: no es un secreto y ayuda a
        // diagnosticar sin exponer nada.
        assert!(printed.contains("Persona usuaria"));
    }

    #[test]
    fn a_session_without_usable_tokens_is_refused() {
        assert!(session().validate().is_ok());

        let mut empty = session();
        empty.access_token = "   ".to_string();
        assert!(empty.validate().is_err());

        let mut oversized = session();
        oversized.refresh_token = "x".repeat(MAX_TOKEN_CHARS + 1);
        assert!(oversized.validate().is_err());

        let mut anonymous = session();
        anonymous.account_id = String::new();
        assert!(anonymous.validate().is_err());
    }

    #[test]
    fn a_token_is_renewed_before_it_actually_expires() {
        let session = session();
        assert!(!session.needs_refresh(1_000 - EXPIRY_MARGIN_SECONDS - 1));
        assert!(session.needs_refresh(1_000 - EXPIRY_MARGIN_SECONDS));
        assert!(session.needs_refresh(2_000));
    }

    #[test]
    fn an_unpublished_refresh_expiry_is_never_guessed_to_be_expired() {
        let mut session = session();
        session.refresh_expires_at = None;
        assert!(!session.refresh_expired(i64::MAX));

        session.refresh_expires_at = Some(500);
        assert!(session.refresh_expired(500));
        assert!(!session.refresh_expired(499));
    }

    #[test]
    fn secure_storage_errors_do_not_expose_platform_details() {
        let error = keyring_error(keyring::Error::Invalid(
            "cuenta".to_string(),
            "token=abcdef".to_string(),
        ));
        assert_eq!(error.code, "secure_storage");
        assert!(!error.message.contains("abcdef"));
        assert!(!error.message.contains("token"));
    }
}
