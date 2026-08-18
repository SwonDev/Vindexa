//! Sesión de cuenta de una tienda externa: iniciarla, mantenerla, contarla y
//! cerrarla.
//!
//! Es la capa que la interfaz ve. Los dos clientes concretos
//! ([`super::epic_api`] y [`super::gog_api`]) hablan con su tienda; aquí se
//! decide **cuándo** hay que refrescar, **qué** se le cuenta a la persona
//! usuaria y **qué** no sale nunca de este módulo.
//!
//! # Lo que cruza hacia la interfaz
//!
//! [`ExternalStoreSession`] es todo lo que el frontend llega a ver de una
//! sesión: si la hay, a nombre de quién, cuándo caduca y **dónde está guardada**
//! —el servicio y la cuenta del llavero—, para que se pueda comprobar con
//! Acceso a Llaveros que tras cerrar sesión no queda nada. Ni el testigo de
//! acceso, ni el de refresco, ni el identificador de cuenta salen de aquí.
//!
//! # Cómo se obtiene el código de autorización
//!
//! Ni Epic ni GOG admiten un cliente público sin secreto, y ninguno de los dos
//! redirige a un esquema propio de aplicación. El código se recoge abriendo la
//! página de inicio de sesión de la tienda en el navegador del sistema y
//! pegando de vuelta lo que la tienda muestra al terminar. Es el mismo trato
//! que hace Legendary, y tiene una ventaja que conviene no perder: **Vindexa
//! nunca ve la contraseña**, sólo un código de un solo uso.

use crate::error::{AppError, AppResult};
use crate::stores::secrets::{self, StoredSession};
use crate::stores::{
    ExternalStore, ScanSource, ScanStatus, StoreOrigin, StoreScan, epic_api, gog_api,
};
use serde::{Deserialize, Serialize};

/// Lo que hay que hacer para iniciar sesión en una tienda.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreLoginPrompt {
    pub store: String,
    pub display_name: String,
    /// Página de la tienda que hay que abrir.
    pub url: String,
    /// Qué verá la persona usuaria y qué tiene que traerse de vuelta.
    pub instructions: String,
    /// Etiqueta del campo donde se pega el resultado.
    pub field_label: String,
}

/// Estado de la sesión de una tienda, **sin un solo dato secreto**.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalStoreSession {
    pub store: String,
    pub display_name: String,
    pub signed_in: bool,
    /// Nombre visible de la cuenta, cuando la tienda lo publica.
    pub account_name: Option<String>,
    /// Caducidad del testigo de acceso, en ISO 8601.
    pub expires_at: Option<String>,
    /// El testigo de acceso está caducado o a punto: la próxima operación lo
    /// renovará sola.
    pub needs_refresh: bool,
    /// El testigo de refresco también caducó: hay que volver a iniciar sesión.
    pub refresh_expired: bool,
    /// Dónde vive el testigo, para poder comprobarlo desde fuera de Vindexa.
    pub keychain_service: String,
    pub keychain_account: String,
    /// Página donde la persona usuaria puede revisar las sesiones de su cuenta
    /// en la propia tienda.
    pub account_sessions_url: Option<String>,
    /// Si el inicio de sesión puede completarse dentro de la ventana de
    /// Vindexa. Cuando es falso queda la vía manual, que funciona igual pero
    /// exige copiar el código a mano.
    pub supports_in_app_login: bool,
}

/// Resultado de cerrar sesión, dicho con todas las letras.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOutReport {
    pub store: String,
    pub display_name: String,
    /// Había algo guardado y se ha borrado.
    pub token_removed: bool,
    /// La tienda confirmó la revocación del testigo en su lado.
    pub remotely_revoked: bool,
    /// Comprobación posterior al borrado: el llavero ya no tiene la entrada.
    pub keychain_empty: bool,
    /// Página donde revisar las sesiones que la cuenta siga teniendo abiertas.
    pub account_sessions_url: Option<String>,
}

/// Página de cada tienda donde se revisan y revocan las sesiones de la cuenta.
fn account_sessions_url(store: ExternalStore) -> &'static str {
    match store {
        ExternalStore::Epic => "https://www.epicgames.com/account/personal",
        ExternalStore::Gog => gog_api::SESSIONS_URL,
    }
}

/// Qué abrir y qué traer de vuelta para iniciar sesión.
pub fn login_prompt(store: ExternalStore) -> StoreLoginPrompt {
    let (url, instructions, field_label) = match store {
        ExternalStore::Epic => (
            epic_api::login_url(),
            "Se abrirá la página de Epic en tu navegador. Inicia sesión con normalidad: Vindexa no ve tu contraseña. Al terminar, Epic mostrará un texto en formato JSON; cópialo entero y pégalo aquí, o pega sólo el valor de «authorizationCode».".to_string(),
            "Código de autorización de Epic".to_string(),
        ),
        ExternalStore::Gog => (
            gog_api::login_url(),
            "Se abrirá la página de GOG en tu navegador. Inicia sesión con normalidad: Vindexa no ve tu contraseña. Al terminar acabarás en una página en blanco; copia la dirección completa de la barra del navegador y pégala aquí.".to_string(),
            "Dirección a la que te ha llevado GOG".to_string(),
        ),
    };
    StoreLoginPrompt {
        store: store.as_str().to_string(),
        display_name: store.display_name().to_string(),
        url,
        instructions,
        field_label,
    }
}

/// Abre la página de inicio de sesión en el navegador del sistema y devuelve
/// las instrucciones.
///
/// La abre Rust, no la interfaz. Así el formulario de la tienda se rellena en
/// el navegador de siempre —con su gestor de contraseñas y su doble factor— y
/// la ventana de Vindexa no llega a verlo. Vindexa nunca recibe la contraseña:
/// sólo el código de un solo uso que la tienda entrega al final.
pub fn open_login_page<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: ExternalStore,
) -> AppResult<StoreLoginPrompt> {
    use tauri_plugin_opener::OpenerExt;

    let prompt = login_prompt(store);
    app.opener().open_url(prompt.url.as_str(), None::<&str>)?;
    Ok(prompt)
}

/// Inicia sesión de principio a fin dentro de Vindexa.
///
/// Abre la página de la tienda en el navegador integrado, espera a que la
/// persona se identifique, recoge el código de autorización de la página de
/// retorno y lo canjea. **No hay nada que copiar ni ningún JSON que leer.**
///
/// El código vive sólo en esta función: se recibe, se canjea y se descarta. No
/// se registra, no se devuelve y no aparece en ningún mensaje de error.
pub async fn sign_in<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    store: ExternalStore,
) -> AppResult<ExternalStoreSession> {
    let code = super::login_window::capture_authorization_code(app, store).await?;
    complete_login(store, &code).await
}

/// Canjea el código pegado y deja la sesión guardada en el llavero.
///
/// Es la vía de reserva de [`sign_in`]: sigue existiendo para quien prefiera
/// identificarse en su propio navegador, o si el integrado no estuviera
/// disponible.
pub async fn complete_login(store: ExternalStore, code: &str) -> AppResult<ExternalStoreSession> {
    let mut session = match store {
        ExternalStore::Epic => epic_api::exchange_code(code).await?,
        ExternalStore::Gog => gog_api::exchange_code(code).await?,
    };
    // GOG no devuelve el nombre de la cuenta al canjear el código; se pregunta
    // aparte y su fallo no invalida un inicio de sesión que sí funcionó.
    if store == ExternalStore::Gog && session.account_name.is_none() {
        session.account_name = gog_api::fetch_account_name(&session).await;
    }
    secrets::save(store, &session)?;
    Ok(describe(store, Some(&session)))
}

/// Estado de la sesión de una tienda. No toca la red.
pub fn session_snapshot(store: ExternalStore) -> AppResult<ExternalStoreSession> {
    let session = secrets::load(store)?;
    Ok(describe(store, session.as_ref()))
}

/// Estado de todas las tiendas conocidas, en el orden de [`ExternalStore::ALL`].
pub fn list_sessions() -> AppResult<Vec<ExternalStoreSession>> {
    ExternalStore::ALL.iter().copied().map(session_snapshot).collect()
}

/// Cierra la sesión: revoca en la tienda cuando se puede, borra el llavero
/// siempre, y comprueba después que no ha quedado nada.
///
/// El orden importa. Si la revocación remota fallara **después** de borrar el
/// testigo local, ya no habría con qué revocar; y si un fallo remoto impidiera
/// el borrado local, cerrar sesión sin conexión sería imposible. Por eso se
/// intenta revocar primero y se borra pase lo que pase.
pub async fn sign_out(store: ExternalStore) -> AppResult<SignOutReport> {
    let session = secrets::load(store)?;
    let token_removed = session.is_some();

    let remotely_revoked = match (store, session.as_ref()) {
        // GOG no publica un extremo de revocación; decir que se revocó sería
        // mentir. La tarjeta enseña dónde hacerlo a mano.
        (ExternalStore::Epic, Some(session)) => epic_api::revoke(session).await.is_ok(),
        _ => false,
    };

    secrets::delete(store)?;
    let keychain_empty = !secrets::has(store)?;

    Ok(SignOutReport {
        store: store.as_str().to_string(),
        display_name: store.display_name().to_string(),
        token_removed,
        remotely_revoked,
        keychain_empty,
        account_sessions_url: Some(account_sessions_url(store).to_string()),
    })
}

/// Devuelve una sesión utilizable, renovándola si hace falta.
///
/// El testigo renovado se guarda antes de usarse: si la sincronización se
/// interrumpe después, la próxima vez se arranca con el testigo bueno y no con
/// el que ya estaba caducado.
async fn active_session(store: ExternalStore) -> AppResult<StoredSession> {
    let session = secrets::load(store)?.ok_or_else(|| not_signed_in(store))?;
    let now = chrono::Utc::now().timestamp();
    if !session.needs_refresh(now) {
        return Ok(session);
    }
    if session.refresh_expired(now) {
        return Err(session_expired(store));
    }
    let renewed = match store {
        ExternalStore::Epic => epic_api::refresh(&session).await,
        ExternalStore::Gog => gog_api::refresh(&session).await,
    };
    match renewed {
        Ok(renewed) => {
            secrets::save(store, &renewed)?;
            Ok(renewed)
        }
        Err(error) if error.code == "external_store_auth" => Err(session_expired(store)),
        Err(error) => Err(error),
    }
}

/// Lee la biblioteca completa de la cuenta y la devuelve como un escaneo.
///
/// Reutiliza [`StoreScan`] a propósito: así la biblioteca que llega por la red
/// se guarda, se empareja con Steam y conserva las correcciones manuales
/// exactamente igual que la que se lee del disco, sin una segunda tubería de
/// persistencia que mantener.
pub async fn sync_library(store: ExternalStore) -> AppResult<StoreScan> {
    let session = match active_session(store).await {
        Ok(session) => session,
        // No haber iniciado sesión no es un fallo: es un estado que la tarjeta
        // sabe explicar, y que **no** debe borrar la biblioteca de ayer.
        Err(error) if error.code == "external_store_not_signed_in" => {
            return Ok(not_signed_in_scan(store, error));
        }
        Err(error) if error.code == "external_store_session_expired" => {
            return Ok(not_signed_in_scan(store, error));
        }
        Err(error) => return Err(error),
    };

    let games = match store {
        ExternalStore::Epic => epic_api::fetch_library(&session).await?,
        ExternalStore::Gog => gog_api::fetch_library(&session).await?,
    };

    let source = match store {
        ExternalStore::Epic => ScanSource::EpicAccountLibrary,
        ExternalStore::Gog => ScanSource::GogAccountLibrary,
    };

    let mut scan = StoreScan::empty(store, ScanStatus::Success);
    scan.origin = StoreOrigin::Account;
    scan.games = games;
    scan.note_source(source);
    Ok(scan)
}

/// El estado «no hay sesión» expresado como escaneo, para que la persistencia
/// registre el diagnóstico sin tocar ni un juego.
fn not_signed_in_scan(store: ExternalStore, error: AppError) -> StoreScan {
    let mut scan = StoreScan::empty(store, ScanStatus::Unavailable);
    scan.origin = StoreOrigin::Account;
    scan.error_code = Some(error.code);
    scan.error_message = Some(error.message);
    scan
}

fn not_signed_in(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_not_signed_in",
        format!(
            "No has iniciado sesión en {}. Inicia sesión para que Vindexa pueda leer tu biblioteca.",
            store.display_name()
        ),
    )
}

fn session_expired(store: ExternalStore) -> AppError {
    AppError::new(
        "external_store_session_expired",
        format!(
            "La sesión de {} ha caducado y no se ha podido renovar. Vuelve a iniciar sesión.",
            store.display_name()
        ),
    )
}

/// Traduce una sesión guardada al único retrato que sale de Rust.
fn describe(store: ExternalStore, session: Option<&StoredSession>) -> ExternalStoreSession {
    let now = chrono::Utc::now().timestamp();
    ExternalStoreSession {
        store: store.as_str().to_string(),
        display_name: store.display_name().to_string(),
        signed_in: session.is_some(),
        account_name: session.and_then(|session| session.account_name.clone()),
        expires_at: session.and_then(|session| iso_timestamp(session.expires_at)),
        needs_refresh: session.is_some_and(|session| session.needs_refresh(now)),
        refresh_expired: session.is_some_and(|session| session.refresh_expired(now)),
        keychain_service: secrets::keychain_service().to_string(),
        keychain_account: secrets::keychain_account(store).to_string(),
        account_sessions_url: Some(account_sessions_url(store).to_string()),
        supports_in_app_login: super::login_window::supports_in_app(store),
    }
}

fn iso_timestamp(seconds: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|moment| moment.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

#[cfg(test)]
mod tests {
    use super::{describe, login_prompt, not_signed_in, not_signed_in_scan};
    use crate::stores::secrets::StoredSession;
    use crate::stores::{ExternalStore, ScanStatus, StoreOrigin};

    fn session() -> StoredSession {
        StoredSession {
            access_token: "testigo-de-acceso".to_string(),
            refresh_token: "testigo-de-refresco".to_string(),
            expires_at: 4_000_000_000,
            refresh_expires_at: None,
            account_id: "identificador-privado".to_string(),
            account_name: Some("Fulanita".to_string()),
        }
    }

    #[test]
    fn the_portrait_that_reaches_the_interface_carries_no_secret() {
        let described = describe(ExternalStore::Epic, Some(&session()));
        let serialized = serde_json::to_string(&described).expect("serializar la sesión");

        assert!(described.signed_in);
        assert_eq!(described.account_name.as_deref(), Some("Fulanita"));
        assert!(!serialized.contains("testigo-de-acceso"));
        assert!(!serialized.contains("testigo-de-refresco"));
        assert!(!serialized.contains("identificador-privado"));
        // Sí viaja dónde mirar para comprobar que no queda nada.
        assert_eq!(described.keychain_service, "io.vindexa.desktop");
        assert!(!described.keychain_account.is_empty());
    }

    #[test]
    fn without_a_session_the_card_says_so_instead_of_pretending() {
        let described = describe(ExternalStore::Gog, None);
        assert!(!described.signed_in);
        assert_eq!(described.account_name, None);
        assert_eq!(described.expires_at, None);
        assert!(!described.needs_refresh);
        // El sitio donde se guarda el testigo se enseña aunque no haya ninguno:
        // es justo lo que hay que poder comprobar tras cerrar sesión.
        assert!(!described.keychain_account.is_empty());
    }

    #[test]
    fn every_store_knows_what_to_open_and_what_to_bring_back() {
        for store in ExternalStore::ALL {
            let prompt = login_prompt(store);
            assert_eq!(prompt.store, store.as_str());
            assert!(prompt.url.starts_with("https://"));
            assert!(!prompt.instructions.trim().is_empty());
            assert!(!prompt.field_label.trim().is_empty());
            // La promesa que sostiene todo el flujo: la contraseña no pasa por
            // aquí, y se dice.
            assert!(prompt.instructions.contains("contraseña"));
        }
    }

    #[test]
    fn not_being_signed_in_never_erases_yesterdays_library() {
        let store = ExternalStore::Epic;
        let scan = not_signed_in_scan(store, not_signed_in(store));
        // `unavailable` es justo el estado que `persist_scan` respeta sin tocar
        // ni un juego guardado.
        assert_eq!(scan.status, ScanStatus::Unavailable);
        assert_eq!(scan.origin, StoreOrigin::Account);
        assert!(scan.games.is_empty());
        assert_eq!(
            scan.error_code.as_deref(),
            Some("external_store_not_signed_in")
        );
        assert!(scan.error_message.is_some());
    }
}
