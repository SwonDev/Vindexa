use crate::error::{AppError, AppResult};
use tauri::{
    AppHandle, Manager, Runtime, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    webview::NewWindowResponse,
};

const STORE_HOST: &str = "store.steampowered.com";
const STORE_WINDOW_LABEL: &str = "steam-store";
const STORE_BLOCKER_ID: &str = "io.vindexa.store-protection.v1";
const STORE_BLOCKER_RULES: &str = r##"[
  {
    "trigger": {
      "url-filter": ".*",
      "if-domain": [
        "*doubleclick.net",
        "*google-analytics.com",
        "*googletagmanager.com",
        "*scorecardresearch.com"
      ]
    },
    "action": { "type": "block" }
  },
  {
    "trigger": {
      "url-filter": ".*",
      "if-domain": ["store.steampowered.com"]
    },
    "action": {
      "type": "css-display-none",
      "selector": "#cookiePrefPopup, .cookiepreferences_popup"
    }
  }
]"##;

pub async fn open<R: Runtime>(app: &AppHandle<R>, app_id: u32) -> AppResult<()> {
    let url = store_url(app_id)?;
    if let Some(window) = app.get_webview_window(STORE_WINDOW_LABEL) {
        if cfg!(any(target_os = "macos", target_os = "linux"))
            && window.url().map_err(store_window_error)?.as_str() == "about:blank"
        {
            return Err(store_protection_error());
        }
        window
            .navigate(url)
            .and_then(|_| window.unminimize())
            .and_then(|_| window.show())
            .and_then(|_| window.set_focus())
            .map_err(store_window_error)?;
        return Ok(());
    }

    let initial_url = if cfg!(any(target_os = "macos", target_os = "linux")) {
        Url::parse("about:blank").expect("about:blank siempre es una URL válida")
    } else {
        url.clone()
    };
    let window =
        WebviewWindowBuilder::new(app, STORE_WINDOW_LABEL, WebviewUrl::External(initial_url))
            .title("Tienda oficial de Steam · sesión privada — Vindexa")
            .inner_size(1280.0, 820.0)
            .min_inner_size(960.0, 680.0)
            .center()
            .resizable(true)
            .decorations(true)
            .incognito(true)
            .devtools(false)
            .zoom_hotkeys_enabled(false)
            .general_autofill_enabled(false)
            .on_navigation(is_allowed_store_navigation)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .on_download(|_, _| false)
            .build()
            .map_err(store_window_error)?;

    install_native_content_blocker(&window).await?;
    if cfg!(any(target_os = "macos", target_os = "linux"))
        && let Err(error) = window.navigate(url)
    {
        let _ = window.close();
        return Err(store_window_error(error));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
async fn install_native_content_blocker<R: Runtime>(window: &WebviewWindow<R>) -> AppResult<()> {
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore, WKUserContentController};
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;
    use tokio::time::{Duration, timeout};

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
            let identifier = NSString::from_str(STORE_BLOCKER_ID);
            let encoded_rules = NSString::from_str(STORE_BLOCKER_RULES);
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
        let _ = window.close();
        return Err(store_protection_error());
    }

    match timeout(Duration::from_secs(5), receiver).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(()))) | Ok(Err(_)) | Err(_) => {
            let _ = window.close();
            Err(store_protection_error())
        }
    }
}

fn store_protection_error() -> AppError {
    AppError::new(
        "store_protection",
        "No se pudo activar la protección de contenido; la tienda no se abrió.",
    )
}

#[cfg(target_os = "linux")]
struct LinuxFilterInstallState {
    manager: webkit2gtk::glib::WeakRef<webkit2gtk::UserContentManager>,
    sender: std::sync::Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Result<(), ()>>>>>,
}

#[cfg(target_os = "linux")]
fn send_linux_filter_result(
    sender: &std::sync::Arc<std::sync::Mutex<Option<tokio::sync::oneshot::Sender<Result<(), ()>>>>>,
    result: Result<(), ()>,
) {
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
    use tokio::time::{Duration, timeout};
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
        let identifier = CString::new(STORE_BLOCKER_ID).map_err(|_| store_protection_error())?;
        Ok::<_, AppError>((store_path, identifier))
    })();
    let (store_path, identifier) = match native_inputs {
        Ok(inputs) => inputs,
        Err(error) => {
            let _ = window.close();
            return Err(error);
        }
    };

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
            let rules = glib::Bytes::from_static(STORE_BLOCKER_RULES.as_bytes());
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
        let _ = window.close();
        return Err(store_protection_error());
    }

    match timeout(Duration::from_secs(5), receiver).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(()))) | Ok(Err(_)) | Err(_) => {
            let _ = window.close();
            Err(store_protection_error())
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
async fn install_native_content_blocker<R: Runtime>(_window: &WebviewWindow<R>) -> AppResult<()> {
    Ok(())
}

fn store_window_error(_error: tauri::Error) -> AppError {
    AppError::new(
        "store_window",
        "No se pudo abrir la tienda protegida de Steam.",
    )
}

pub fn store_url(app_id: u32) -> AppResult<Url> {
    if app_id == 0 {
        return Err(AppError::validation(
            "El identificador de Steam no es válido.",
        ));
    }
    Url::parse(&format!("https://{STORE_HOST}/app/{app_id}"))
        .map_err(|_| AppError::new("store_url", "No se pudo preparar la tienda oficial."))
}

pub fn is_allowed_store_navigation(url: &Url) -> bool {
    url.as_str() == "about:blank" || (url.scheme() == "https" && url.host_str() == Some(STORE_HOST))
}

#[cfg(test)]
pub fn is_blocked_tracking_host(host: Option<&str>) -> bool {
    let Some(host) = host.map(|host| host.trim_end_matches('.').to_ascii_lowercase()) else {
        return false;
    };
    const BLOCKED: &[&str] = &[
        "doubleclick.net",
        "google-analytics.com",
        "googletagmanager.com",
        "scorecardresearch.com",
    ];
    BLOCKED
        .iter()
        .any(|blocked| host == *blocked || host.ends_with(&format!(".{blocked}")))
}

#[cfg(test)]
mod tests {
    use super::{
        STORE_BLOCKER_ID, STORE_BLOCKER_RULES, is_allowed_store_navigation,
        is_blocked_tracking_host, store_url,
    };
    use serde_json::Value;
    use tauri::Url;

    #[test]
    fn store_urls_are_exact_https_urls_for_a_valid_app() {
        assert_eq!(
            store_url(620).unwrap().as_str(),
            "https://store.steampowered.com/app/620"
        );
        assert!(store_url(0).is_err());
    }

    #[test]
    fn top_level_navigation_is_limited_to_the_official_store_host() {
        assert!(is_allowed_store_navigation(
            &Url::parse("about:blank").unwrap()
        ));
        assert!(is_allowed_store_navigation(
            &Url::parse("https://store.steampowered.com/app/620").unwrap()
        ));
        assert!(!is_allowed_store_navigation(
            &Url::parse("http://store.steampowered.com/app/620").unwrap()
        ));
        assert!(!is_allowed_store_navigation(
            &Url::parse("https://store.steampowered.com.evil.example/app/620").unwrap()
        ));
        assert!(!is_allowed_store_navigation(
            &Url::parse("https://steamcommunity.com/login").unwrap()
        ));
    }

    #[test]
    fn tracker_matching_is_boundary_safe_and_keeps_steam_assets() {
        assert!(is_blocked_tracking_host(Some("www.google-analytics.com")));
        assert!(is_blocked_tracking_host(Some("DOUBLECLICK.NET")));
        assert!(!is_blocked_tracking_host(Some(
            "google-analytics.com.safe.example"
        )));
        assert!(!is_blocked_tracking_host(Some(
            "cdn.akamai.steamstatic.com"
        )));
        assert!(!is_blocked_tracking_host(None));
    }

    #[test]
    fn native_blocker_rules_are_versioned_valid_json_with_fail_closed_actions() {
        assert_eq!(STORE_BLOCKER_ID, "io.vindexa.store-protection.v1");
        let rules: Value = serde_json::from_str(STORE_BLOCKER_RULES).unwrap();
        let rules = rules.as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["action"]["type"], "block");
        assert_eq!(rules[1]["action"]["type"], "css-display-none");
        assert_eq!(
            rules[0]["trigger"]["if-domain"],
            serde_json::json!([
                "*doubleclick.net",
                "*google-analytics.com",
                "*googletagmanager.com",
                "*scorecardresearch.com"
            ])
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_filter_callback_matches_the_gio_abi() {
        let _: webkit2gtk::gio::ffi::GAsyncReadyCallback =
            Some(super::finish_linux_content_filter_install);
    }
}
