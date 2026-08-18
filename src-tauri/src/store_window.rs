//! Ventanas del navegador integrado de tiendas.
//!
//! Este módulo es la parte que toca Tauri: crea la ventana endurecida, instala
//! el bloqueador nativo, inyecta la barra y ejecuta sus órdenes. Toda la
//! lógica de decisión vive en [`crate::browser`], que se puede probar sin
//! arrancar un webview.
//!
//! ## Endurecimiento de cada ventana
//!
//! | Ajuste | Valor | Por qué |
//! | --- | --- | --- |
//! | Almacén de datos | uno por tienda | Sesión aislada de la aplicación y de las demás tiendas. |
//! | `devtools` | solo en depuración | En release no hay inspector sobre contenido remoto. |
//! | `on_navigation` | [`crate::browser::policy::evaluate`] | Allowlist de host y esquema. |
//! | `on_new_window` | `Deny` | Nada de ventanas emergentes; el destino válido se abre aquí. |
//! | `on_download` | `false` | Ninguna descarga escribe en el disco. |
//! | `general_autofill_enabled` | `false` | El autorrelleno del sistema no ve estos formularios. |
//! | `zoom_hotkeys_enabled` | `false` | El zoom lo controla la aplicación, no el webview. |
//! | `browser_extensions_enabled` | `false` | Ninguna extensión se inyecta en la tienda. |
//! | `enable_clipboard_access` | no se activa | La página no lee el portapapeles por su cuenta. |
//! | Capabilities Tauri | ninguna | Sin `invoke`; la ACL rechaza todo origen remoto. |

#[cfg(any(target_os = "macos", target_os = "linux"))]
use crate::browser::blocker;
use crate::browser::chrome::messages;
use crate::browser::policy::{ControlCommand, NavigationVerdict, RejectionReason};
use crate::browser::session::WindowState;
use crate::browser::stores::StoreProfile;
use crate::browser::{chrome, policy, prefs, session, stores};
use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use std::time::Duration;
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

/// Host de la tienda de Steam, usado para construir la ficha de un AppID.
const STORE_HOST: &str = "store.steampowered.com";

/// Margen máximo antes de avisar de que una carga no ha terminado.
const LOAD_TIMEOUT: Duration = Duration::from_secs(25);

/// Abre la ficha de un juego en el navegador integrado.
///
/// Firma estable consumida por `commands::open_store`.
pub async fn open<R: Runtime>(app: &AppHandle<R>, app_id: u32) -> AppResult<()> {
    let url = store_url(app_id)?;
    open_in_store(app, stores::default_store(), url).await
}

/// Abre la portada de una tienda del catálogo.
///
/// Pendiente de exponerse como comando: el diff para `commands.rs` y `lib.rs`
/// viaja en el informe, porque esos archivos pertenecen a otro agente.
pub async fn open_store_home<R: Runtime>(app: &AppHandle<R>, store_id: &str) -> AppResult<()> {
    let store = stores::store_by_id(store_id).ok_or_else(|| {
        AppError::validation("Esa tienda no está disponible en el navegador integrado.")
    })?;
    open_in_store(app, store, store.home_url()).await
}

/// Abre o reutiliza la ventana de `store` y la lleva a `url`.
async fn open_in_store<R: Runtime>(
    app: &AppHandle<R>,
    store: &'static StoreProfile,
    url: Url,
) -> AppResult<()> {
    if !matches!(policy::evaluate(store, &url), NavigationVerdict::Allowed(_)) {
        return Err(AppError::validation(
            "Ese destino no pertenece a las tiendas permitidas.",
        ));
    }

    let label = store.window_label();
    if let Some(window) = app.get_webview_window(&label) {
        // Una ventana sin estado registrado quedó a medio abrir: su protección
        // no está confirmada, así que no se reutiliza.
        if !session::is_registered(&label) {
            return Err(store_protection_error());
        }
        window
            .navigate(url)
            .and_then(|()| window.unminimize())
            .and_then(|()| window.show())
            .and_then(|()| window.set_focus())
            .map_err(store_window_error)?;
        return Ok(());
    }

    create_window(app, store, url).await
}

/// Crea la ventana endurecida de una tienda.
async fn create_window<R: Runtime>(
    app: &AppHandle<R>,
    store: &'static StoreProfile,
    target: Url,
) -> AppResult<()> {
    let label = store.window_label();
    let token = policy::new_control_token();
    let config_dir = browser_config_dir(app);
    let zoom = config_dir
        .as_ref()
        .map(|dir| prefs::load(dir).get(store.id))
        .unwrap_or(session::DEFAULT_ZOOM);

    // En WebKit la ventana arranca en blanco: primero se compila el filtro de
    // contenido y solo después se navega a la tienda.
    let deferred = cfg!(any(target_os = "macos", target_os = "linux"));
    let initial = if deferred {
        Url::parse("about:blank").expect("about:blank siempre es una URL válida")
    } else {
        target.clone()
    };

    session::register(
        &label,
        WindowState::new(store, token.clone(), target.clone(), zoom),
    );

    let window = match build_hardened_window(app, store, &label, &token, initial) {
        Ok(window) => window,
        Err(error) => {
            session::forget(&label);
            return Err(store_window_error(error));
        }
    };

    let closing_label = label.clone();
    window.on_window_event(move |event| {
        // El historial vive solo en memoria y muere con la ventana.
        if matches!(event, WindowEvent::Destroyed) {
            session::forget(&closing_label);
        }
    });

    install_native_content_blocker(&window).await?;
    session::with_window(&label, |state| state.blocker_active = true);

    let _ = window.set_zoom(zoom);

    if deferred && let Err(error) = window.navigate(target) {
        let _ = window.close();
        session::forget(&label);
        return Err(store_window_error(error));
    }

    push_state(app, &label);
    Ok(())
}

/// Construye la ventana endurecida.
///
/// Es deliberadamente **síncrona**: el constructor de Tauri no es `Send`, así
/// que no puede seguir vivo a través de un `await`. Manteniéndolo en su propia
/// función, el futuro de creación de ventana sí es `Send` y puede enviarse al
/// runtime asíncrono desde cualquier manejador nativo.
fn build_hardened_window<R: Runtime>(
    app: &AppHandle<R>,
    store: &'static StoreProfile,
    label: &str,
    token: &str,
    initial: Url,
) -> Result<WebviewWindow<R>, tauri::Error> {
    let nav_app = app.clone();
    let nav_label = label.to_string();
    let popup_app = app.clone();
    let popup_label = label.to_string();
    let download_app = app.clone();
    let download_label = label.to_string();
    let load_app = app.clone();
    let load_label = label.to_string();

    let mut builder = WebviewWindowBuilder::new(app, label, WebviewUrl::External(initial))
        .title(store.window_title())
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 640.0)
        .center()
        .resizable(true)
        .decorations(true)
        .devtools(devtools_enabled())
        .browser_extensions_enabled(false)
        .zoom_hotkeys_enabled(false)
        .general_autofill_enabled(false)
        .disable_drag_drop_handler()
        .initialization_script(chrome::initialization_script(token, store))
        .on_navigation(move |url| decide_navigation(&nav_app, store, &nav_label, url))
        .on_new_window(move |url, _features| deny_new_window(&popup_app, store, &popup_label, url))
        .on_download(move |_webview, event| {
            if matches!(event, DownloadEvent::Requested { .. }) {
                notify(&download_app, &download_label, messages::DOWNLOAD_BLOCKED);
            }
            false
        })
        .on_page_load(move |_window, payload| {
            let finished = matches!(payload.event(), PageLoadEvent::Finished);
            session::with_window(&load_label, |state| state.loading = !finished);
            push_state(&load_app, &load_label);
        });

    if let Some(user_agent) = store.user_agent {
        builder = builder.user_agent(user_agent);
    }

    builder = with_isolated_session(builder, store);

    builder.build()
}

/// Aísla el almacenamiento de la ventana y decide si sobrevive al cierre.
///
/// La sesión de la tienda es la sesión de la persona usuaria: sin ella no se
/// puede iniciar sesión ni añadir nada a la lista de deseados desde la propia
/// tienda. WebKit guarda cookies y almacenamiento por identificador de almacén,
/// así que cada tienda recibe el suyo: persiste entre aperturas y ninguna
/// tienda ve las cookies de otra ni las de la ventana principal. La barra
/// conserva el botón de vaciar sesión para borrarlo a voluntad.
///
/// Donde WebKit no ofrece almacenes con identificador —macOS anterior al 14 y
/// el resto de plataformas— se vuelve al modo incógnito: aísla igual, pero la
/// sesión muere al cerrar la ventana.
fn with_isolated_session<'a, R: Runtime>(
    builder: WebviewWindowBuilder<'a, R, AppHandle<R>>,
    store: &'static StoreProfile,
) -> WebviewWindowBuilder<'a, R, AppHandle<R>> {
    #[cfg(target_os = "macos")]
    if objc2::available!(macos = 14.0)
        && let Some(identifier) = store.session_store_id()
    {
        return builder.data_store_identifier(identifier);
    }
    // Los almacenes con identificador son exclusivos de WebKit en Apple.
    #[cfg(not(target_os = "macos"))]
    let _ = store;
    builder.incognito(true)
}

/// Decide si una navegación de nivel superior puede continuar.
fn decide_navigation<R: Runtime>(
    app: &AppHandle<R>,
    store: &'static StoreProfile,
    label: &str,
    url: &Url,
) -> bool {
    match policy::evaluate(store, url) {
        NavigationVerdict::Bootstrap => true,
        NavigationVerdict::Allowed(_) => {
            if let Some(generation) =
                session::with_window(label, |state| state.note_navigation(url))
            {
                watch_load(app.clone(), label.to_string(), generation);
            }
            push_state(app, label);
            true
        }
        NavigationVerdict::Control(command) => {
            let app = app.clone();
            let label = label.to_string();
            tauri::async_runtime::spawn(async move {
                run_control(app, store, label, command).await;
            });
            false
        }
        NavigationVerdict::Rejected(reason) => {
            notify(app, label, &describe_rejection(reason, url));
            false
        }
    }
}

/// Deniega toda ventana emergente y reconduce el destino válido a esta ventana.
fn deny_new_window<R: Runtime>(
    app: &AppHandle<R>,
    store: &'static StoreProfile,
    label: &str,
    url: Url,
) -> NewWindowResponse<R> {
    match policy::evaluate(store, &url) {
        // Un `target="_blank"` legítimo se convierte en una navegación normal
        // de esta misma ventana, que vuelve a pasar por la política.
        NavigationVerdict::Allowed(_) => {
            let app = app.clone();
            let label = label.to_string();
            tauri::async_runtime::spawn(async move {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.navigate(url);
                }
            });
        }
        NavigationVerdict::Rejected(reason) => notify(app, label, reason.message()),
        _ => notify(app, label, messages::POPUP_BLOCKED),
    }
    NewWindowResponse::Deny
}

/// Ejecuta una orden de la barra.
async fn run_control<R: Runtime>(
    app: AppHandle<R>,
    store: &'static StoreProfile,
    label: String,
    command: ControlCommand,
) {
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };

    match command {
        ControlCommand::Back => {
            if let Some(Some(target)) = session::with_window(&label, |state| state.go_back()) {
                let _ = window.navigate(target);
            }
        }
        ControlCommand::Forward => {
            if let Some(Some(target)) = session::with_window(&label, |state| state.go_forward()) {
                let _ = window.navigate(target);
            }
        }
        ControlCommand::Reload => {
            // `History::push` descarta destinos repetidos, así que recargar no
            // ensucia el historial.
            let _ = window.reload();
        }
        ControlCommand::Stop => {
            let _ = window.eval(chrome::STOP_SCRIPT);
            session::with_window(&label, |state| state.loading = false);
            push_state(&app, &label);
        }
        ControlCommand::Home => {
            let _ = window.navigate(store.home_url());
        }
        ControlCommand::Query(text) => match policy::resolve_query(store, &text) {
            Ok((target, url)) if target.id == store.id => {
                let _ = window.navigate(url);
            }
            Ok((target, url)) => {
                // Boxeado a propósito: rompe la recursión de tipos entre este
                // futuro y el de creación de ventana.
                let _ = Box::pin(open_in_store(&app, target, url)).await;
                notify(&app, &label, messages::OPENED_IN_OTHER_STORE);
            }
            Err(reason) => notify(&app, &label, reason.message()),
        },
        ControlCommand::SwitchStore(target) => {
            if target.id == store.id {
                let _ = window.set_focus();
            } else {
                let _ = Box::pin(open_in_store(&app, target, target.home_url())).await;
            }
        }
        ControlCommand::ZoomIn | ControlCommand::ZoomOut | ControlCommand::ZoomReset => {
            apply_zoom(&app, &window, store, &label, command);
        }
        ControlCommand::Close => {
            let _ = window.close();
        }
        ControlCommand::ClearSession => {
            let _ = window.clear_all_browsing_data();
            notify(&app, &label, messages::SESSION_CLEARED);
        }
        ControlCommand::AddToWishlist => {
            // La ventana remota no toca la base de datos: la orden sólo pide a
            // Vindexa que mire la URL —la que esta misma política ya aceptó— y
            // guarde el deseo. No se evalúa ningún guion en la página.
            let (database, maintenance) = {
                let state = app.state::<crate::commands::AppState>();
                (state.database.clone(), state.maintenance.clone())
            };
            let message = crate::steam::wishlist_session::add_current_page_to_wishlist(
                &label,
                database,
                maintenance,
            )
            .await;
            notify(&app, &label, &message);
        }
    }
}

/// Aplica y recuerda el zoom de la tienda.
fn apply_zoom<R: Runtime>(
    app: &AppHandle<R>,
    window: &WebviewWindow<R>,
    store: &'static StoreProfile,
    label: &str,
    command: ControlCommand,
) {
    let Some(zoom) = session::with_window(label, |state| {
        state.zoom = match command {
            ControlCommand::ZoomIn => session::zoom_in(state.zoom),
            ControlCommand::ZoomOut => session::zoom_out(state.zoom),
            _ => session::DEFAULT_ZOOM,
        };
        state.zoom
    }) else {
        return;
    };
    let _ = window.set_zoom(zoom);
    if let Some(dir) = browser_config_dir(app) {
        prefs::remember_zoom(&dir, store.id, zoom);
    }
    push_state(app, label);
}

/// Deja un aviso en español en la barra y lo empuja a la ventana.
/// Aviso de rechazo con el destino incluido.
///
/// Sin el anfitrión, «ese destino está fuera de las tiendas permitidas» no dice
/// cuál, y no hay forma de saber qué habría que autorizar. Sólo se enseña el
/// anfitrión: la ruta y la consulta pueden llevar datos de la sesión.
fn describe_rejection(reason: RejectionReason, url: &Url) -> String {
    match url.host_str() {
        Some(host) if matches!(reason, RejectionReason::HostNotAllowed) => {
            format!("{} Destino: {host}.", reason.message())
        }
        _ => reason.message().to_string(),
    }
}

fn notify<R: Runtime>(app: &AppHandle<R>, label: &str, message: &str) {
    session::with_window(label, |state| {
        state.notice = Some(message.to_string());
        state.loading = false;
    });
    push_state(app, label);
}

/// Empuja el estado actual a la barra de la ventana.
fn push_state<R: Runtime>(app: &AppHandle<R>, label: &str) {
    let Some(state) = session::snapshot(label) else {
        return;
    };
    let Some(token) = session::with_window(label, |window| window.token.clone()) else {
        return;
    };
    let script = chrome::state_script(&token, &state);
    let app = app.clone();
    let label = label.to_string();
    // Siempre fuera del manejador nativo: `eval` viaja al hilo principal y no
    // debe ejecutarse en medio de una decisión de navegación.
    tauri::async_runtime::spawn(async move {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.eval(script);
        }
    });
}

/// Vigila que una carga concreta termine; si no, deja un aviso recuperable.
fn watch_load<R: Runtime>(app: AppHandle<R>, label: String, generation: u64) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(LOAD_TIMEOUT).await;
        let stalled = session::with_window(&label, |state| {
            state.load_generation == generation && state.loading
        })
        .unwrap_or(false);
        if stalled {
            notify(&app, &label, messages::LOAD_TIMEOUT);
        }
    });
}

/// ¿Debe la ventana remota exponer el inspector?
///
/// Solo en compilaciones de depuración. En release no hay DevTools sobre
/// contenido de terceros.
fn devtools_enabled() -> bool {
    cfg!(debug_assertions)
}

/// Directorio donde se recuerda el zoom por tienda.
fn browser_config_dir<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("browser"))
}

#[cfg(target_os = "macos")]
async fn install_native_content_blocker<R: Runtime>(window: &WebviewWindow<R>) -> AppResult<()> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore, WKUserContentController};
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    use tokio::time::timeout;

    let encoded_rules_source = blocker::content_rule_list_json();
    let (sender, receiver) = oneshot::channel::<Result<(), ()>>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    if window
        .with_webview(move |webview| unsafe {
            let Some(main_thread) = MainThreadMarker::new() else {
                if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
                    let _ = sender.send(Err(()));
                }
                return;
            };
            let Some(store) = WKContentRuleListStore::defaultStore(main_thread) else {
                if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
                    let _ = sender.send(Err(()));
                }
                return;
            };
            let controller = webview.controller().cast::<WKUserContentController>();
            let identifier = NSString::from_str(blocker::BLOCKER_ID);
            let encoded_rules = NSString::from_str(&encoded_rules_source);
            let sender = sender.clone();
            let completion =
                RcBlock::new(move |rule: *mut WKContentRuleList, error: *mut NSError| {
                    let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take())
                    else {
                        return;
                    };
                    if error.is_null() && !rule.is_null() {
                        (&*controller).addContentRuleList(&*rule);
                        let _ = sender.send(Ok(()));
                    } else {
                        let _ = sender.send(Err(()));
                    }
                });
            store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                Some(&identifier),
                Some(&encoded_rules),
                Some(&completion),
            );
        })
        .is_err()
    {
        close_after_protection_failure(window);
        return Err(store_protection_error());
    }

    match timeout(Duration::from_secs(8), receiver).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(()))) | Ok(Err(_)) | Err(_) => {
            close_after_protection_failure(window);
            Err(store_protection_error())
        }
    }
}

/// Cierra la ventana y olvida su estado cuando la protección no se pudo activar.
///
/// Sólo hay a qué reaccionar donde existe un bloqueador nativo que pueda fallar.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn close_after_protection_failure<R: Runtime>(window: &WebviewWindow<R>) {
    let label = window.label().to_string();
    let _ = window.close();
    session::forget(&label);
}

fn store_protection_error() -> AppError {
    AppError::new("store_protection", messages::BLOCKER_UNAVAILABLE)
}

/// Aviso de que la instalación del filtro terminó.
///
/// `webkit2gtk` avisa por una llamada de vuelta de C, que puede ejecutarse una
/// sola vez o ninguna. De ahí el envoltorio: el `Option` permite consumir el
/// emisor, y el `Mutex` cruzar la frontera con C sin exigir `&mut`.
#[cfg(target_os = "linux")]
type LinuxFilterSender = std::sync::Arc<std::sync::Mutex<Option<LinuxFilterChannel>>>;

#[cfg(target_os = "linux")]
type LinuxFilterChannel = tokio::sync::oneshot::Sender<Result<(), ()>>;

#[cfg(target_os = "linux")]
struct LinuxFilterInstallState {
    manager: webkit2gtk::glib::WeakRef<webkit2gtk::UserContentManager>,
    sender: LinuxFilterSender,
}

#[cfg(target_os = "linux")]
fn send_linux_filter_result(sender: &LinuxFilterSender, result: Result<(), ()>) {
    if let Some(sender) = sender.lock().ok().and_then(|mut sender| sender.take()) {
        let _ = sender.send(result);
    }
}

#[cfg(target_os = "linux")]
unsafe extern "C" fn finish_linux_content_filter_install(
    source_object: *mut webkit2gtk::glib::gobject_ffi::GObject,
    result: *mut webkit2gtk::gio::ffi::GAsyncResult,
    user_data: webkit2gtk::glib::ffi::gpointer,
) {
    use webkit2gtk::glib;
    use webkit2gtk::glib::translate::{ToGlibPtr, from_glib_full};

    if user_data.is_null() {
        return;
    }
    let state = unsafe { Box::from_raw(user_data.cast::<LinuxFilterInstallState>()) };
    if source_object.is_null() || result.is_null() {
        send_linux_filter_result(&state.sender, Err(()));
        return;
    }

    let mut error = std::ptr::null_mut();
    let filter = unsafe {
        webkit2gtk::ffi::webkit_user_content_filter_store_save_finish(
            source_object.cast(),
            result,
            &mut error,
        )
    };
    let native_error = if error.is_null() {
        None
    } else {
        Some(unsafe { from_glib_full::<_, glib::Error>(error) })
    };

    let installed = if filter.is_null() || native_error.is_some() {
        false
    } else if let Some(manager) = state.manager.upgrade() {
        unsafe {
            webkit2gtk::ffi::webkit_user_content_manager_add_filter(
                manager.to_glib_none().0,
                filter,
            );
        }
        true
    } else {
        false
    };

    if !filter.is_null() {
        unsafe { webkit2gtk::ffi::webkit_user_content_filter_unref(filter) };
    }
    send_linux_filter_result(&state.sender, installed.then_some(()).ok_or(()));
}

#[cfg(target_os = "linux")]
async fn install_native_content_blocker<R: Runtime>(window: &WebviewWindow<R>) -> AppResult<()> {
    use std::ffi::CString;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    use tokio::time::timeout;
    use webkit2gtk::WebViewExt;
    use webkit2gtk::glib;
    use webkit2gtk::glib::prelude::ObjectExt;
    use webkit2gtk::glib::translate::ToGlibPtr;

    let native_inputs = (|| {
        let store_directory = window
            .app_handle()
            .path()
            .app_cache_dir()
            .map_err(|_| store_protection_error())?
            .join("content-filters");
        std::fs::create_dir_all(&store_directory).map_err(|_| store_protection_error())?;
        let store_path = CString::new(store_directory.to_string_lossy().as_bytes())
            .map_err(|_| store_protection_error())?;
        let identifier = CString::new(blocker::BLOCKER_ID).map_err(|_| store_protection_error())?;
        Ok::<_, AppError>((store_path, identifier))
    })();
    let (store_path, identifier) = match native_inputs {
        Ok(inputs) => inputs,
        Err(error) => {
            close_after_protection_failure(window);
            return Err(error);
        }
    };

    let encoded_rules = blocker::content_rule_list_json();
    let (sender, receiver) = oneshot::channel::<Result<(), ()>>();
    let sender = Arc::new(Mutex::new(Some(sender)));
    let callback_sender = sender.clone();
    if window
        .with_webview(move |webview| {
            let view = webview.inner();
            let Some(manager) = view.user_content_manager() else {
                send_linux_filter_result(&callback_sender, Err(()));
                return;
            };
            let rules = glib::Bytes::from_owned(encoded_rules.into_bytes());
            let store = unsafe {
                webkit2gtk::ffi::webkit_user_content_filter_store_new(store_path.as_ptr())
            };
            if store.is_null() {
                send_linux_filter_result(&callback_sender, Err(()));
                return;
            }

            let state = Box::new(LinuxFilterInstallState {
                manager: manager.downgrade(),
                sender: callback_sender.clone(),
            });
            unsafe {
                webkit2gtk::ffi::webkit_user_content_filter_store_save(
                    store,
                    identifier.as_ptr(),
                    rules.to_glib_none().0,
                    std::ptr::null_mut(),
                    Some(finish_linux_content_filter_install),
                    Box::into_raw(state).cast(),
                );
                // GTask conserva una referencia a `store` hasta terminar el callback.
                glib::gobject_ffi::g_object_unref(store.cast());
            }
        })
        .is_err()
    {
        close_after_protection_failure(window);
        return Err(store_protection_error());
    }

    match timeout(Duration::from_secs(8), receiver).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(()))) | Ok(Err(_)) | Err(_) => {
            close_after_protection_failure(window);
            Err(store_protection_error())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn install_native_content_blocker<R: Runtime>(_window: &WebviewWindow<R>) -> AppResult<()> {
    // Windows (WebView2) no ofrece una lista de contenido nativa equivalente.
    // El aislamiento por host, la denegación de descargas y de ventanas
    // emergentes y la ausencia de IPC siguen siendo fronteras independientes.
    Ok(())
}

fn store_window_error(_error: tauri::Error) -> AppError {
    AppError::new(
        "store_window",
        "No se pudo abrir el navegador integrado de tiendas.",
    )
}

/// URL de la ficha de un juego en la tienda oficial de Steam.
pub fn store_url(app_id: u32) -> AppResult<Url> {
    if app_id == 0 {
        return Err(AppError::validation(
            "El identificador de Steam no es válido.",
        ));
    }
    Url::parse(&format!("https://{STORE_HOST}/app/{app_id}"))
        .map_err(|_| AppError::new("store_url", "No se pudo preparar la tienda oficial."))
}

/// ¿Puede la ventana de Steam navegar a esta URL?
///
/// Se conserva como atajo legible sobre [`policy::evaluate`] y como red de
/// seguridad de las pruebas heredadas de la versión anterior.
#[allow(
    dead_code,
    reason = "atajo legible sobre policy::evaluate, usado en pruebas"
)]
pub fn is_allowed_store_navigation(url: &Url) -> bool {
    matches!(
        policy::evaluate(stores::default_store(), url),
        NavigationVerdict::Bootstrap | NavigationVerdict::Allowed(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("URL de prueba válida")
    }

    #[test]
    fn store_urls_are_exact_https_urls_for_a_valid_app() {
        assert_eq!(
            store_url(620).unwrap().as_str(),
            "https://store.steampowered.com/app/620"
        );
        assert!(store_url(0).is_err());
        assert_eq!(store_url(0).unwrap_err().code, "validation");
    }

    #[test]
    fn the_steam_window_keeps_its_historical_navigation_guarantees() {
        assert!(is_allowed_store_navigation(&url("about:blank")));
        assert!(is_allowed_store_navigation(&url(
            "https://store.steampowered.com/app/620"
        )));
        assert!(!is_allowed_store_navigation(&url(
            "http://store.steampowered.com/app/620"
        )));
        assert!(!is_allowed_store_navigation(&url(
            "https://store.steampowered.com.evil.example/app/620"
        )));
        assert!(!is_allowed_store_navigation(&url("https://www.gog.com/")));
        // La comunidad sí forma parte del perfil de Steam desde la versión 2.
        assert!(is_allowed_store_navigation(&url(
            "https://steamcommunity.com/login"
        )));
    }

    #[test]
    fn the_url_of_a_game_belongs_to_the_default_store_window() {
        let target = store_url(440).unwrap();
        let store = stores::store_for_url(&target).expect("la ficha pertenece a una tienda");
        assert_eq!(store.id, stores::DEFAULT_STORE_ID);
        assert_eq!(store.window_label(), "vindexa-store-steam");
        assert!(store.window_title().contains("navegador protegido"));
    }

    #[test]
    fn protection_failures_report_a_stable_code_and_a_spanish_message() {
        let error = store_protection_error();
        assert_eq!(error.code, "store_protection");
        assert_eq!(error.message, messages::BLOCKER_UNAVAILABLE);
        assert!(!error.message.contains('/'));
    }

    #[test]
    fn devtools_are_only_available_in_debug_builds() {
        // En un binario de release el inspector no existe sobre contenido
        // remoto; en depuración sí, para poder diagnosticar la barra.
        assert_eq!(devtools_enabled(), cfg!(debug_assertions));
        #[cfg(not(debug_assertions))]
        assert!(!devtools_enabled());
    }

    #[test]
    fn no_capability_ever_reaches_a_store_browser_window() {
        // Prueba de configuración del aislamiento IPC: ninguna capability puede
        // conceder permisos a las ventanas del navegador ni declarar un origen
        // remoto. Con esto, la comprobación de ACL de Tauri (`resolve_access`
        // con `Origin::Remote`) rechaza cualquier `invoke` desde la tienda.
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("capabilities");
        let mut reviewed = 0_usize;
        for entry in std::fs::read_dir(&directory).expect("capabilities/ existe") {
            let path = entry.expect("entrada legible").path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let raw = std::fs::read_to_string(&path).expect("capability legible");
            let capability: serde_json::Value =
                serde_json::from_str(&raw).expect("capability con JSON válido");
            reviewed += 1;

            assert!(
                capability
                    .get("remote")
                    .is_none_or(serde_json::Value::is_null),
                "ninguna capability puede abrirse a orígenes remotos: {}",
                path.display()
            );

            let windows: Vec<&str> = capability["windows"]
                .as_array()
                .map(|values| values.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let reaches_browser = windows.iter().any(|pattern| {
                *pattern == "*"
                    || pattern.starts_with("vindexa-store")
                    || pattern.trim_end_matches('*') == "vindexa-"
            });
            if reaches_browser {
                let permissions = capability["permissions"]
                    .as_array()
                    .expect("toda capability declara permisos");
                assert!(
                    permissions.is_empty(),
                    "la capability {} alcanza el navegador y concede permisos",
                    path.display()
                );
            }

            // La ventana principal nunca comparte capability con el navegador.
            if windows.contains(&"main") {
                assert!(
                    !reaches_browser,
                    "la capability {} mezcla la ventana principal con el navegador",
                    path.display()
                );
            }
        }
        assert!(reviewed >= 2, "se revisaron las capabilities del proyecto");
    }

    #[test]
    fn the_application_never_freezes_prototypes_from_the_tauri_configuration() {
        // `app.security.freezePrototype` inyecta `Object.freeze(Object.prototype)`
        // en *todos* los webviews, también en el del navegador de tiendas. Allí
        // no protege nada —esa ventana no tiene IPC— y rompe el paquete de la
        // tienda: con él activo, Steam no llegaba a construir su reproductor ni
        // sus capturas. El endurecimiento vive en `src/main.tsx`, donde sólo
        // alcanza a la ventana propia de la aplicación.
        let ruta = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json");
        let raw = std::fs::read_to_string(&ruta).expect("tauri.conf.json legible");
        let configuracion: serde_json::Value =
            serde_json::from_str(&raw).expect("tauri.conf.json con JSON válido");
        assert_eq!(
            configuracion["app"]["security"]["freezePrototype"],
            serde_json::Value::Bool(false),
            "activar freezePrototype rompe el contenido de las tiendas"
        );
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_blocker_installed_in_the_window_is_the_versioned_one() {
        let compiled = blocker::content_rule_list_json();
        assert!(compiled.starts_with('['));
        assert!(compiled.contains("ignore-previous-rules"));
        assert_eq!(blocker::BLOCKER_ID, "io.vindexa.browser-protection.v2");
    }
}
