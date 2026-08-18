//! Barra del navegador integrado.
//!
//! La barra se inyecta como *user script* en cada documento de nivel superior
//! de la ventana de tienda y vive en un `shadow root` cerrado para que el CSS
//! del sitio no la deforme.
//!
//! > **No es una frontera de seguridad.** La barra comparte contexto con el
//! > documento remoto, así que se asume que la página podría emitir cualquiera
//! > de sus órdenes. Ninguna concede privilegios: todas vuelven a pasar por
//! > `policy::evaluate` en Rust. Las fronteras reales son la política de
//! > navegación nativa, la ausencia de IPC, el bloqueo de descargas y el de
//! > ventanas emergentes.

use crate::browser::policy;
use crate::browser::session::{ChromeState, ZOOM_STEPS};
use crate::browser::stores::{HostRule, StoreProfile};
use serde::Serialize;

/// Altura de la barra en píxeles CSS.
pub const BAR_HEIGHT: u32 = 46;

/// Nombre de la propiedad global que expone la barra, con el token incrustado.
fn api_name(token: &str) -> String {
    format!("__vindexaBrowser_{token}")
}

#[derive(Serialize)]
struct HostConfig {
    host: &'static str,
    sub: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChromeConfig<'a> {
    api: String,
    control: String,
    store_id: &'a str,
    store_name: &'a str,
    bar_height: u32,
    zoom_steps: &'a [f64],
    hosts: Vec<HostConfig>,
    strings: Strings,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Strings {
    back: &'static str,
    forward: &'static str,
    reload: &'static str,
    stop: &'static str,
    home: &'static str,
    address_label: &'static str,
    address_placeholder: &'static str,
    go: &'static str,
    secure: &'static str,
    insecure: &'static str,
    zoom_in: &'static str,
    zoom_out: &'static str,
    zoom_reset: &'static str,
    find: &'static str,
    find_placeholder: &'static str,
    find_previous: &'static str,
    find_next: &'static str,
    find_close: &'static str,
    find_unsupported: &'static str,
    store_label: &'static str,
    add_to_wishlist: &'static str,
    clear_session: &'static str,
    close: &'static str,
    retry: &'static str,
    dismiss: &'static str,
    loading: &'static str,
    shortcuts: &'static str,
}

const STRINGS: Strings = Strings {
    back: "Atrás",
    forward: "Adelante",
    reload: "Recargar",
    stop: "Detener",
    home: "Inicio de la tienda",
    address_label: "Dirección o búsqueda en la tienda",
    address_placeholder: "Busca en la tienda o escribe una dirección permitida",
    go: "Ir",
    secure: "Conexión cifrada (HTTPS) verificada por el sistema",
    insecure: "Conexión sin verificar",
    zoom_in: "Aumentar el zoom",
    zoom_out: "Reducir el zoom",
    zoom_reset: "Restablecer el zoom al 100 %",
    find: "Buscar en la página",
    find_placeholder: "Buscar en esta página",
    find_previous: "Coincidencia anterior",
    find_next: "Coincidencia siguiente",
    find_close: "Cerrar la búsqueda",
    find_unsupported: "Este sistema no permite buscar dentro de la página.",
    store_label: "Tienda",
    add_to_wishlist: "Añadir este juego a tus deseados de Vindexa",
    clear_session: "Vaciar cookies y almacenamiento de esta ventana",
    close: "Cerrar la ventana",
    retry: "Reintentar",
    dismiss: "Descartar",
    loading: "Cargando…",
    shortcuts: "Atajos: Cmd/Ctrl+L dirección · Cmd/Ctrl+F buscar · Cmd/Ctrl+R recargar · Alt+Flechas historial · Cmd/Ctrl+0 zoom",
};

/// Mensajes de estado de red, en español y sin rutas ni cabeceras.
pub mod messages {
    /// La carga no terminó dentro del margen previsto.
    pub const LOAD_TIMEOUT: &str =
        "La página no ha terminado de cargarse. Comprueba tu conexión y vuelve a intentarlo.";
    /// El bloqueador nativo no pudo instalarse.
    pub const BLOCKER_UNAVAILABLE: &str =
        "No se ha podido activar el bloqueo de anuncios y rastreo, así que la tienda no se ha abierto.";
    /// Se vació la sesión de la ventana.
    pub const SESSION_CLEARED: &str =
        "Se han borrado las cookies y el almacenamiento de esta ventana.";
    /// La ventana abrió una tienda distinta en su propia ventana.
    pub const OPENED_IN_OTHER_STORE: &str =
        "Ese destino pertenece a otra tienda y se ha abierto en su propia ventana.";
    /// Una ventana emergente apuntaba fuera de las tiendas permitidas.
    pub const POPUP_BLOCKED: &str =
        "Se ha bloqueado una ventana emergente hacia un destino que no pertenece a las tiendas permitidas.";
    /// Una descarga fue denegada.
    pub const DOWNLOAD_BLOCKED: &str =
        "Las descargas están desactivadas en el navegador integrado. Usa el cliente de Steam o tu navegador habitual.";
}

/// Guion de inicialización que se inyecta en cada documento de la ventana.
pub fn initialization_script(token: &str, store: &'static StoreProfile) -> String {
    let config = ChromeConfig {
        api: api_name(token),
        control: policy::control_origin(token),
        store_id: store.id,
        store_name: store.name,
        bar_height: BAR_HEIGHT,
        zoom_steps: ZOOM_STEPS,
        hosts: store
            .hosts
            .iter()
            .map(|rule| HostConfig {
                host: rule.domain(),
                sub: matches!(rule, HostRule::WithSubdomains(_)),
            })
            .collect(),
        strings: STRINGS,
    };
    let serialized = serde_json::to_string(&config)
        .expect("la configuración de la barra siempre serializa");
    CHROME_SCRIPT.replace("__VINDEXA_CONFIG__", &serialized)
}

/// Guion que empuja un estado nuevo a la barra ya inyectada.
pub fn state_script(token: &str, state: &ChromeState) -> String {
    let serialized =
        serde_json::to_string(state).expect("el estado de la barra siempre serializa");
    format!(
        "(function(){{var api=window[{name}];if(api&&api.apply){{api.apply({state});}}}})();",
        name = serde_json::to_string(&api_name(token))
            .expect("el nombre de la API siempre serializa"),
        state = serialized
    )
}

/// Guion que detiene la carga en curso sin tocar el historial.
pub const STOP_SCRIPT: &str = "try{window.stop();}catch(e){}";

const CHROME_SCRIPT: &str = r#"
(function () {
  'use strict';
  var CONFIG = __VINDEXA_CONFIG__;
  var S = CONFIG.strings;

  function normalizeHost(value) {
    return String(value || '').toLowerCase().replace(/\.+$/, '');
  }

  function hostAllowed(host) {
    host = normalizeHost(host);
    if (!host) { return false; }
    for (var i = 0; i < CONFIG.hosts.length; i++) {
      var rule = CONFIG.hosts[i];
      if (rule.sub) {
        if (host === rule.host || host.slice(-(rule.host.length + 1)) === '.' + rule.host) {
          return true;
        }
      } else if (host === rule.host) {
        return true;
      }
    }
    return false;
  }

  var blank = location.href === 'about:blank';
  if (!blank && (location.protocol !== 'https:' || !hostAllowed(location.hostname))) {
    return;
  }
  if (window[CONFIG.api]) { return; }

  var state = {
    url: location.href,
    host: normalizeHost(location.hostname),
    path: location.pathname + location.search,
    secure: location.protocol === 'https:',
    canGoBack: false,
    canGoForward: false,
    loading: true,
    zoom: 1,
    zoomPercent: 100,
    notice: null,
    storeId: CONFIG.storeId,
    storeName: CONFIG.storeName,
    stores: []
  };

  function send(action, params) {
    var target = CONFIG.control + '/' + action;
    if (params) {
      var parts = [];
      for (var key in params) {
        if (Object.prototype.hasOwnProperty.call(params, key)) {
          parts.push(encodeURIComponent(key) + '=' + encodeURIComponent(params[key]));
        }
      }
      if (parts.length) { target += '?' + parts.join('&'); }
    }
    try { window.location.href = target; } catch (e) { /* la orden se pierde, nada más */ }
  }

  function el(tag, attrs, text) {
    var node = document.createElement(tag);
    if (attrs) {
      for (var key in attrs) {
        if (Object.prototype.hasOwnProperty.call(attrs, key)) {
          node.setAttribute(key, attrs[key]);
        }
      }
    }
    if (text !== undefined && text !== null) { node.textContent = text; }
    return node;
  }

  function button(label, glyph, action) {
    var node = el('button', { type: 'button', 'aria-label': label, title: label }, glyph);
    node.addEventListener('click', function (event) {
      event.preventDefault();
      action();
    });
    return node;
  }

  var host = el('div');
  host.setAttribute('data-vindexa-browser-chrome', '');
  host.style.cssText = 'all:initial;display:block;position:fixed;top:0;left:0;width:100%;z-index:2147483647;';
  var root = host.attachShadow ? host.attachShadow({ mode: 'closed' }) : null;
  if (!root) { return; }

  var style = el('style');
  style.textContent = [
    ':host{display:block;}',
    '*{box-sizing:border-box;font-family:system-ui,-apple-system,"Segoe UI",sans-serif;}',
    '.bar{display:flex;align-items:center;gap:6px;height:' + CONFIG.barHeight + 'px;padding:0 10px;',
    'background:#10141c;color:#e6e9ef;border-bottom:1px solid #262c38;font-size:13px;}',
    'button{appearance:none;border:0;border-radius:6px;background:transparent;color:#e6e9ef;',
    'height:30px;min-width:30px;padding:0 8px;font-size:14px;line-height:1;cursor:pointer;}',
    'button:hover:not(:disabled){background:#222938;}',
    'button:focus-visible{outline:2px solid #6ea8fe;outline-offset:1px;}',
    'button:disabled{opacity:.35;cursor:default;}',
    '.address{display:flex;align-items:center;gap:6px;flex:1;min-width:120px;height:30px;',
    'padding:0 10px;border-radius:8px;background:#1a2030;border:1px solid #2b3345;}',
    '.address:focus-within{border-color:#6ea8fe;}',
    '.lock{font-size:12px;flex:0 0 auto;}',
    '.lock.ok{color:#5fd39a;} .lock.bad{color:#ff9d7a;}',
    'input{flex:1;min-width:0;border:0;outline:0;background:transparent;color:#e6e9ef;font-size:13px;}',
    '.host{font-weight:600;color:#ffffff;flex:0 0 auto;max-width:45%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}',
    'select{height:30px;border-radius:6px;background:#1a2030;color:#e6e9ef;border:1px solid #2b3345;font-size:12px;padding:0 6px;}',
    '.zoom{display:flex;align-items:center;gap:2px;}',
    '.zoom span{min-width:44px;text-align:center;font-variant-numeric:tabular-nums;font-size:12px;}',
    '.find{display:none;align-items:center;gap:6px;padding:6px 10px;background:#141926;border-bottom:1px solid #262c38;}',
    '.find.open{display:flex;}',
    '.find input{background:#1a2030;border:1px solid #2b3345;border-radius:6px;height:28px;padding:0 8px;max-width:280px;}',
    '.find .miss{color:#ff9d7a;font-size:12px;}',
    '.notice{display:none;align-items:center;gap:10px;padding:8px 12px;background:#2a1f16;',
    'color:#ffd8b8;border-bottom:1px solid #4a3b28;font-size:12px;}',
    '.notice.open{display:flex;}',
    '.notice p{margin:0;flex:1;}',
    '.notice button{border:1px solid #6a4f33;height:26px;font-size:12px;}',
    '.progress{height:2px;background:transparent;}',
    '.progress.on{background:linear-gradient(90deg,#6ea8fe,#5fd39a);animation:vx 1.1s linear infinite;}',
    '@keyframes vx{0%{opacity:.35;}50%{opacity:1;}100%{opacity:.35;}}',
    '@media (prefers-reduced-motion: reduce){.progress.on{animation:none;opacity:.8;}}'
  ].join('');
  root.appendChild(style);

  var bar = el('div', { class: 'bar', role: 'toolbar', 'aria-label': 'Navegador integrado de Vindexa' });
  var backBtn = button(S.back, '‹', function () { send('back'); });
  var forwardBtn = button(S.forward, '›', function () { send('forward'); });
  var reloadBtn = button(S.reload, '⟳', function () { send(state.loading ? 'stop' : 'reload'); });
  var homeBtn = button(S.home, '⌂', function () { send('home'); });

  var address = el('div', { class: 'address' });
  var lock = el('span', { class: 'lock ok', title: S.secure }, '🔒');
  var hostLabel = el('span', { class: 'host' }, '');
  var input = el('input', {
    type: 'text',
    spellcheck: 'false',
    autocomplete: 'off',
    autocapitalize: 'off',
    'aria-label': S.addressLabel,
    placeholder: S.addressPlaceholder
  });
  var editing = false;
  input.addEventListener('focus', function () {
    editing = true;
    input.value = state.url;
    input.select();
  });
  input.addEventListener('blur', function () { editing = false; render(); });
  input.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') {
      event.preventDefault();
      var value = input.value.trim();
      if (value) { send('go', { q: value }); }
    } else if (event.key === 'Escape') {
      event.preventDefault();
      input.blur();
    }
    event.stopPropagation();
  });
  address.appendChild(lock);
  address.appendChild(hostLabel);
  address.appendChild(input);

  var zoomBox = el('div', { class: 'zoom' });
  var zoomOutBtn = button(S.zoomOut, '−', function () { send('zoom-out'); });
  var zoomLabel = el('span', { role: 'status' }, '100 %');
  zoomLabel.title = S.zoomReset;
  var zoomInBtn = button(S.zoomIn, '+', function () { send('zoom-in'); });
  zoomLabel.addEventListener('click', function () { send('zoom-reset'); });
  zoomBox.appendChild(zoomOutBtn);
  zoomBox.appendChild(zoomLabel);
  zoomBox.appendChild(zoomInBtn);

  var findBtn = button(S.find, '⌕', function () { toggleFind(true); });
  var storeSelect = el('select', { 'aria-label': S.storeLabel });
  storeSelect.addEventListener('change', function () {
    if (storeSelect.value && storeSelect.value !== state.storeId) {
      send('store', { id: storeSelect.value });
    }
  });
  var wishlistBtn = button(S.addToWishlist, '♥', function () { send('wishlist-add'); });
  var clearBtn = button(S.clearSession, '⌫', function () { send('clear-session'); });
  var closeBtn = button(S.close, '✕', function () { send('close'); });

  bar.appendChild(backBtn);
  bar.appendChild(forwardBtn);
  bar.appendChild(reloadBtn);
  bar.appendChild(homeBtn);
  bar.appendChild(address);
  bar.appendChild(zoomBox);
  bar.appendChild(findBtn);
  bar.appendChild(wishlistBtn);
  bar.appendChild(storeSelect);
  bar.appendChild(clearBtn);
  bar.appendChild(closeBtn);
  bar.title = S.shortcuts;
  root.appendChild(bar);

  var progress = el('div', { class: 'progress' });
  root.appendChild(progress);

  var findPanel = el('div', { class: 'find', role: 'search' });
  var findInput = el('input', { type: 'text', 'aria-label': S.findPlaceholder, placeholder: S.findPlaceholder });
  var findMiss = el('span', { class: 'miss' }, '');
  var findPrev = button(S.findPrevious, '↑', function () { runFind(true); });
  var findNext = button(S.findNext, '↓', function () { runFind(false); });
  var findClose = button(S.findClose, '✕', function () { toggleFind(false); });
  findPanel.appendChild(findInput);
  findPanel.appendChild(findPrev);
  findPanel.appendChild(findNext);
  findPanel.appendChild(findMiss);
  findPanel.appendChild(findClose);
  root.appendChild(findPanel);

  var notice = el('div', { class: 'notice', role: 'status' });
  var noticeText = el('p', null, '');
  var noticeRetry = button(S.retry, S.retry, function () { send('reload'); });
  var noticeDismiss = button(S.dismiss, S.dismiss, function () {
    state.notice = null;
    render();
  });
  notice.appendChild(noticeText);
  notice.appendChild(noticeRetry);
  notice.appendChild(noticeDismiss);
  root.appendChild(notice);

  function runFind(backwards) {
    var query = findInput.value;
    if (!query) { findMiss.textContent = ''; return; }
    if (typeof window.find !== 'function') {
      findMiss.textContent = S.findUnsupported;
      return;
    }
    var found = false;
    try { found = window.find(query, false, !!backwards, true, false, false, false); } catch (e) { found = false; }
    findMiss.textContent = found ? '' : '0/0';
  }

  function toggleFind(open) {
    if (open) {
      findPanel.classList.add('open');
      findInput.focus();
      findInput.select();
    } else {
      findPanel.classList.remove('open');
      findMiss.textContent = '';
    }
    layout();
  }

  findInput.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') {
      event.preventDefault();
      runFind(event.shiftKey);
    } else if (event.key === 'Escape') {
      event.preventDefault();
      toggleFind(false);
    }
    event.stopPropagation();
  });

  function layout() {
    var zoom = state.zoom > 0 ? state.zoom : 1;
    // La ventana aplica el zoom a todo el documento, incluida esta barra:
    // se compensa a la inversa para que conserve su tamaño real.
    host.style.transform = 'scale(' + (1 / zoom) + ')';
    host.style.transformOrigin = 'top left';
    host.style.width = (100 * zoom) + '%';
    var height = host.offsetHeight || CONFIG.barHeight;
    var reserved = Math.ceil(height / zoom);
    try {
      document.documentElement.style.setProperty('padding-top', reserved + 'px', 'important');
    } catch (e) { /* documento sin estilos editables */ }
  }

  function render() {
    backBtn.disabled = !state.canGoBack;
    forwardBtn.disabled = !state.canGoForward;
    reloadBtn.textContent = state.loading ? '✕' : '⟳';
    reloadBtn.setAttribute('aria-label', state.loading ? S.stop : S.reload);
    reloadBtn.title = state.loading ? S.stop : S.reload;
    progress.className = state.loading ? 'progress on' : 'progress';

    lock.className = state.secure ? 'lock ok' : 'lock bad';
    lock.textContent = state.secure ? '🔒' : '⚠';
    lock.title = state.secure ? S.secure : S.insecure;
    hostLabel.textContent = state.host || '';
    if (!editing) { input.value = state.path || ''; }

    zoomLabel.textContent = state.zoomPercent + ' %';

    if (storeSelect.childElementCount !== state.stores.length) {
      storeSelect.textContent = '';
      for (var i = 0; i < state.stores.length; i++) {
        var option = el('option', { value: state.stores[i].id }, state.stores[i].name);
        storeSelect.appendChild(option);
      }
    }
    storeSelect.value = state.storeId;

    if (state.notice) {
      noticeText.textContent = state.notice;
      notice.classList.add('open');
    } else {
      notice.classList.remove('open');
    }
    layout();
  }

  function attach() {
    var parent = document.documentElement;
    if (parent && host.parentNode !== parent) { parent.appendChild(host); }
  }

  var api = {};
  api.apply = function (next) {
    if (!next || typeof next !== 'object') { return; }
    for (var key in next) {
      if (Object.prototype.hasOwnProperty.call(next, key)) { state[key] = next[key]; }
    }
    attach();
    render();
  };
  try {
    Object.defineProperty(window, CONFIG.api, { value: api, writable: false, configurable: false });
  } catch (e) {
    window[CONFIG.api] = api;
  }

  document.addEventListener('keydown', function (event) {
    var meta = event.metaKey || event.ctrlKey;
    var key = event.key ? event.key.toLowerCase() : '';
    if (meta && key === 'l') { event.preventDefault(); input.focus(); return; }
    if (meta && key === 'f') { event.preventDefault(); toggleFind(true); return; }
    if (meta && key === 'r') { event.preventDefault(); send('reload'); return; }
    if (meta && (key === '+' || key === '=')) { event.preventDefault(); send('zoom-in'); return; }
    if (meta && key === '-') { event.preventDefault(); send('zoom-out'); return; }
    if (meta && key === '0') { event.preventDefault(); send('zoom-reset'); return; }
    if (meta && key === 'w') { event.preventDefault(); send('close'); return; }
    if (event.altKey && key === 'arrowleft') { event.preventDefault(); send('back'); return; }
    if (event.altKey && key === 'arrowright') { event.preventDefault(); send('forward'); return; }
    if (key === 'escape') {
      if (findPanel.classList.contains('open')) { toggleFind(false); }
      else if (state.loading) { send('stop'); }
    }
  }, true);

  if (document.documentElement) { attach(); }
  document.addEventListener('DOMContentLoaded', function () { attach(); render(); });
  var guard = new MutationObserver(function () { attach(); });
  try {
    guard.observe(document.documentElement, { childList: true });
  } catch (e) { /* documento aún sin raíz observable */ }
  window.addEventListener('resize', layout);
  render();

})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::session::WindowState;
    use crate::browser::stores;
    use tauri::Url;

    fn steam() -> &'static StoreProfile {
        stores::store_by_id("steam").unwrap()
    }

    #[test]
    fn the_script_carries_the_token_and_never_leaves_placeholders() {
        let script = initialization_script("deadbeef", steam());
        assert!(!script.contains("__VINDEXA_CONFIG__"));
        assert!(script.contains("__vindexaBrowser_deadbeef"));
        assert!(script.contains("vindexa-browser://deadbeef"));
        assert!(script.contains("\"storeId\":\"steam\""));
    }

    #[test]
    fn the_script_refuses_to_render_outside_the_allowed_hosts() {
        let script = initialization_script("abc", steam());
        assert!(script.contains("location.protocol !== 'https:'"));
        assert!(script.contains("hostAllowed(location.hostname)"));
        // La lista de hosts embebida es exactamente la del perfil.
        for rule in steam().hosts {
            assert!(
                script.contains(rule.domain()),
                "{} debe viajar en la configuración",
                rule.domain()
            );
        }
        assert!(!script.contains("www.gog.com"));
    }

    #[test]
    fn the_script_never_touches_the_tauri_ipc_surface() {
        let script = initialization_script("abc", steam());
        for forbidden in [
            "__TAURI_INTERNALS__",
            "__TAURI__",
            "invoke(",
            "postMessage(",
            "convertFileSrc",
            "asset://",
        ] {
            assert!(
                !script.contains(forbidden),
                "la barra no puede usar {forbidden}"
            );
        }
    }

    #[test]
    fn the_state_script_is_scoped_to_its_own_window_token() {
        let state = WindowState::new(
            steam(),
            "tok1".into(),
            Url::parse("https://store.steampowered.com/").unwrap(),
            1.0,
        );
        let script = state_script("tok1", &state.snapshot());
        assert!(script.contains("__vindexaBrowser_tok1"));
        assert!(!script.contains("__vindexaBrowser_tok2"));
        assert!(script.contains("\"storeId\":\"steam\""));
        assert!(script.starts_with("(function()"));
        assert!(script.ends_with("})();"));
    }

    #[test]
    fn a_hostile_notice_cannot_break_out_of_the_state_script() {
        let mut state = WindowState::new(
            steam(),
            "tok".into(),
            Url::parse("https://store.steampowered.com/").unwrap(),
            1.0,
        );
        state.notice = Some("</script><script>alert(1)</script>\"; alert(2); //".into());
        let script = state_script("tok", &state.snapshot());
        // `serde_json` escapa las comillas, así que el guion sigue siendo una
        // única expresión y el texto viaja como dato.
        assert!(
            script.contains(r#"\"; alert(2)"#),
            "las comillas del aviso viajan escapadas"
        );
        assert_eq!(script.matches("api.apply(").count(), 1);
        let payload = script
            .split_once("api.apply(")
            .and_then(|(_, rest)| rest.rsplit_once(");}})();"))
            .map(|(json, _)| json)
            .expect("el estado viaja como JSON");
        let parsed: serde_json::Value = serde_json::from_str(payload).expect("JSON válido");
        assert!(parsed["notice"].as_str().unwrap().contains("alert(1)"));
    }

    #[test]
    fn every_visible_string_is_spanish_and_accented() {
        let script = initialization_script("abc", steam());
        for expected in [
            "Atrás",
            "Recargar",
            "Detener",
            "Inicio de la tienda",
            "Buscar en la página",
            "Conexión cifrada",
            "Restablecer el zoom",
            "Cerrar la ventana",
        ] {
            assert!(script.contains(expected), "falta la cadena «{expected}»");
        }
    }

    /// Todas las claves `PREFIJO.clave` que usa el guion.
    fn referenced_keys(script: &str, prefix: &str) -> Vec<String> {
        let mut keys = Vec::new();
        for fragment in script.split(prefix).skip(1) {
            let key: String = fragment
                .chars()
                .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
                .collect();
            if !key.is_empty() && !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys
    }

    #[test]
    fn the_script_only_reads_configuration_keys_that_rust_really_sends() {
        // No se puede ejecutar el webview desde aquí, así que esta prueba cubre
        // el fallo más probable: que la barra lea una clave que Rust nunca
        // manda y se quede con `undefined` en un botón o una etiqueta.
        let script = initialization_script("abc", steam());
        let payload = script
            .split_once("var CONFIG = ")
            .and_then(|(_, rest)| rest.split_once(";\n"))
            .map(|(json, _)| json)
            .expect("la configuración viaja como JSON literal");
        let config: serde_json::Value =
            serde_json::from_str(payload).expect("configuración con JSON válido");

        for key in referenced_keys(&script, "CONFIG.") {
            assert!(
                config.get(&key).is_some(),
                "el guion lee CONFIG.{key} pero Rust no lo envía"
            );
        }
        let strings = config["strings"]
            .as_object()
            .expect("la barra recibe sus cadenas");
        for key in referenced_keys(&script, "S.") {
            assert!(
                strings.contains_key(&key),
                "el guion lee S.{key} pero no existe en las cadenas"
            );
        }
        assert!(strings.len() >= 20, "la barra está traducida por completo");
    }

    #[test]
    fn the_bar_only_asks_for_control_actions_that_rust_understands() {
        use crate::browser::policy::{self, NavigationVerdict};

        let script = initialization_script("abc", steam());
        let mut checked = 0_usize;
        for fragment in script.split("send('").skip(1) {
            let action: String = fragment
                .chars()
                .take_while(|character| *character != '\'')
                .collect();
            let url = tauri::Url::parse(&format!("vindexa-browser://abc/{action}?q=x&id=gog"))
                .expect("orden de control con URL válida");
            assert!(
                matches!(policy::evaluate(steam(), &url), NavigationVerdict::Control(_)),
                "la barra envía «{action}» y la política no la reconoce"
            );
            checked += 1;
        }
        assert!(checked >= 8, "se comprobaron las órdenes de la barra");
    }

    #[test]
    fn network_messages_stay_free_of_local_details() {
        for message in [
            messages::LOAD_TIMEOUT,
            messages::BLOCKER_UNAVAILABLE,
            messages::SESSION_CLEARED,
            messages::OPENED_IN_OTHER_STORE,
            messages::POPUP_BLOCKED,
            messages::DOWNLOAD_BLOCKED,
        ] {
            assert!(message.ends_with('.'));
            assert!(!message.contains('/'));
            assert!(!message.to_lowercase().contains("error code"));
            assert!(!message.contains("Users"));
        }
    }
}
