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
    /// El llavero no dejó leer la entrada: se denegó el permiso, está bloqueado
    /// o el sistema falló.
    ///
    /// **No es lo mismo que no tener sesión**, y confundirlos fue un fallo real:
    /// la tarjeta decía «sin sesión iniciada» y ofrecía volver a entrar cuando
    /// la sesión estaba guardada y lo único que faltaba era el permiso. Con
    /// esto, la tarjeta dice lo que pasó y qué hacer.
    pub unreadable: bool,
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
    forget_remembered_session(store);
    Ok(describe(store, Some(&session)))
}

/// Lo que se recuerda de cada tienda entre lecturas del llavero.
///
/// # Por qué se recuerda
///
/// macOS pregunta por la contraseña del llavero cada vez que lee una entrada
/// mientras la aplicación no esté en su lista de acceso, y abrir Ajustes →
/// Tiendas lee **las dos**. Sin recordar nada, cada visita a la pantalla eran
/// dos diálogos idénticos.
///
/// Lo que se guarda **no es el testigo**: son los tres datos que la tarjeta
/// enseña —nombre visible y caducidades—, y de ellos se recalcula el estado en
/// cada consulta, así que un testigo que caduca mientras la aplicación está
/// abierta se sigue viendo caducado. Muere con el proceso y se olvida en cuanto
/// alguien inicia o cierra sesión.
type Portrait = Option<(Option<String>, i64, Option<i64>)>;

/// Lo que se recuerda de una tienda: su retrato, o que el llavero no dejó leer.
///
/// La negativa también se recuerda. Sin eso, con el permiso denegado cada
/// refresco de la pantalla volvía a abrir dos diálogos de contraseña, que es
/// justo lo que se estaba intentando evitar. Se olvida al iniciar sesión, al
/// cerrarla y cuando se pulsa «reintentar», que es lo que hace falta para que
/// una negativa por error no obligue a reiniciar.
#[derive(Clone)]
enum Recuerdo {
    Sesion(Portrait),
    Ilegible,
}

static RECORDADO: std::sync::RwLock<Option<Vec<(ExternalStore, Recuerdo)>>> =
    std::sync::RwLock::new(None);

fn recordar(store: ExternalStore, recuerdo: Recuerdo) {
    if let Ok(mut cache) = RECORDADO.write() {
        let entradas = cache.get_or_insert_with(Vec::new);
        entradas.retain(|(conocida, _)| *conocida != store);
        entradas.push((store, recuerdo));
    }
}

fn recordado(store: ExternalStore) -> Option<Recuerdo> {
    let cache = RECORDADO.read().ok()?;
    let entradas = cache.as_ref()?;
    entradas
        .iter()
        .find(|(conocida, _)| *conocida == store)
        .map(|(_, recuerdo)| recuerdo.clone())
}

/// Olvida lo recordado de una tienda: su sesión acaba de cambiar.
pub fn forget_remembered_session(store: ExternalStore) {
    if let Ok(mut cache) = RECORDADO.write()
        && let Some(entradas) = cache.as_mut()
    {
        entradas.retain(|(conocida, _)| *conocida != store);
    }
}

/// Estado de la sesión de una tienda. No toca la red.
///
/// Nunca falla: un llavero que no deja leer es **un estado**, no un error, y
/// enseñarlo como error dejaba la pantalla sin las dos tarjetas.
pub fn session_snapshot(store: ExternalStore) -> AppResult<ExternalStoreSession> {
    match recordado(store) {
        Some(Recuerdo::Sesion(portrait)) => return Ok(from_portrait(store, portrait)),
        Some(Recuerdo::Ilegible) => return Ok(unreadable(store)),
        None => {}
    }
    let session = match secrets::load(store) {
        Ok(session) => session,
        Err(_) => {
            recordar(store, Recuerdo::Ilegible);
            return Ok(unreadable(store));
        }
    };
    recordar(
        store,
        Recuerdo::Sesion(session.as_ref().map(|session| {
            (
                session.account_name.clone(),
                session.expires_at,
                session.refresh_expires_at,
            )
        })),
    );
    Ok(describe(store, session.as_ref()))
}

/// Reconstruye la tarjeta desde lo recordado, recalculando las caducidades
/// contra el reloj de ahora y no contra el de la lectura.
fn from_portrait(store: ExternalStore, portrait: Portrait) -> ExternalStoreSession {
    let Some((account_name, expires_at, refresh_expires_at)) = portrait else {
        return describe(store, None);
    };
    let now = chrono::Utc::now().timestamp();
    ExternalStoreSession {
        signed_in: true,
        account_name,
        expires_at: iso_timestamp(expires_at),
        needs_refresh: now + secrets::EXPIRY_MARGIN_SECONDS >= expires_at,
        refresh_expired: refresh_expires_at.is_some_and(|limit| now >= limit),
        ..describe(store, None)
    }
}

/// Estado de todas las tiendas conocidas, en el orden de [`ExternalStore::ALL`].
///
/// Un llavero que no deja leer **una** tienda no puede borrar del mapa a la
/// otra: cada tarjeta cuenta lo suyo, y la que no se pudo leer lo dice en vez
/// de aparecer como si no hubiera sesión.
pub fn list_sessions() -> AppResult<Vec<ExternalStoreSession>> {
    ExternalStore::ALL
        .iter()
        .copied()
        .map(session_snapshot)
        .collect()
}

/// Lo que se enseña cuando el llavero no deja leer la entrada de una tienda.
fn unreadable(store: ExternalStore) -> ExternalStoreSession {
    ExternalStoreSession {
        unreadable: true,
        ..describe(store, None)
    }
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
    forget_remembered_session(store);
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
            // La caducidad recordada acaba de quedarse vieja.
            forget_remembered_session(store);
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
        unreadable: false,
    }
}

fn iso_timestamp(seconds: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(seconds, 0)
        .map(|moment| moment.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        describe, from_portrait, login_prompt, not_signed_in, not_signed_in_scan, unreadable,
    };
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

    /// «No se pudo leer» y «no hay sesión» no son lo mismo.
    ///
    /// Con el llavero denegando el permiso, la tarjeta decía «sin sesión
    /// iniciada» y ofrecía volver a entrar. La sesión estaba guardada: lo que
    /// faltaba era el permiso, y volver a entrar no lo arregla.
    #[test]
    fn un_llavero_que_no_deja_leer_no_se_confunde_con_no_tener_sesion() {
        let sin_sesion = describe(ExternalStore::Gog, None);
        assert!(!sin_sesion.unreadable);

        let ilegible = unreadable(ExternalStore::Gog);
        assert!(ilegible.unreadable);
        assert!(!ilegible.signed_in);
        // Y sigue diciendo dónde mirar: es lo que permite comprobarlo a mano.
        assert_eq!(ilegible.keychain_service, "io.vindexa.desktop");
        assert!(!ilegible.keychain_account.is_empty());
    }

    /// Lo recordado no puede congelar una caducidad.
    ///
    /// El diálogo del llavero se evita recordando tres datos, no el testigo. Si
    /// además se recordara el veredicto ya calculado, un testigo que caduca con
    /// la aplicación abierta seguiría figurando como vigente hasta reiniciar.
    #[test]
    fn lo_recordado_recalcula_la_caducidad_contra_el_reloj_de_ahora() {
        let ahora = chrono::Utc::now().timestamp();

        let vigente = from_portrait(ExternalStore::Epic, Some((None, ahora + 3_600, None)));
        assert!(vigente.signed_in);
        assert!(!vigente.needs_refresh);

        let a_punto = from_portrait(ExternalStore::Epic, Some((None, ahora + 30, None)));
        assert!(a_punto.needs_refresh, "dentro del margen ya toca renovar");

        let caducado = from_portrait(
            ExternalStore::Epic,
            Some((Some("Fulanita".to_string()), ahora - 10, Some(ahora - 5))),
        );
        assert!(caducado.needs_refresh);
        assert!(caducado.refresh_expired);
        assert_eq!(caducado.account_name.as_deref(), Some("Fulanita"));

        // Y recordar «aquí no había sesión» sigue siendo no tener sesión.
        let sin_nada = from_portrait(ExternalStore::Gog, None);
        assert!(!sin_nada.signed_in);
        assert!(!sin_nada.unreadable);
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
