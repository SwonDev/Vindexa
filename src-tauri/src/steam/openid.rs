use super::web_api::validate_steam_id;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Duration, Utc};
use reqwest::{Client, redirect::Policy};
use std::collections::{HashMap, HashSet};
use std::time::Duration as StdDuration;
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{Instant, timeout};
use url::Url;
use uuid::Uuid;

const OPENID_ENDPOINT: &str = "https://steamcommunity.com/openid/login";
const OPENID_NS: &str = "http://specs.openid.net/auth/2.0";
const IDENTIFIER_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";
const CALLBACK_TIMEOUT: StdDuration = StdDuration::from_secs(180);
const CONNECTION_TIMEOUT: StdDuration = StdDuration::from_secs(5);
const MAX_HTTP_REQUEST: usize = 32 * 1024;
const MAX_PROVIDER_RESPONSE: usize = 64 * 1024;
const MAX_CALLBACK_ATTEMPTS: usize = 64;

pub async fn authenticate<R: Runtime>(
    app: &AppHandle<R>,
    database: &Database,
) -> AppResult<String> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let state = Uuid::new_v4().simple().to_string();
    let realm = format!("http://127.0.0.1:{}/", address.port());
    let return_to = format!("{realm}steam/openid?state={state}");
    let authorization_url = build_authorization_url(&realm, &return_to)?;

    app.opener()
        .open_url(authorization_url.as_str(), None::<&str>)?;

    let deadline = Instant::now() + CALLBACK_TIMEOUT;
    for _ in 0..MAX_CALLBACK_ATTEMPTS {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        let accepted = timeout(remaining, listener.accept()).await;
        let (mut stream, peer) = match accepted {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => return Err(error.into()),
            Err(_) => break,
        };
        if !peer.ip().is_loopback() {
            write_browser_response(&mut stream, false).await;
            continue;
        }

        let target = match timeout(CONNECTION_TIMEOUT, read_request_target(&mut stream)).await {
            Ok(Ok(target)) => target,
            Ok(Err(_)) | Err(_) => {
                write_browser_response(&mut stream, false).await;
                continue;
            }
        };
        let callback = Url::parse(&format!("{realm}{}", target.trim_start_matches('/')))
            .map_err(|_| AppError::validation("Steam devolvió una URL de retorno no válida."))?;
        let fields: HashMap<String, String> = callback.query_pairs().into_owned().collect();
        if fields.get("state").map(String::as_str) != Some(state.as_str()) {
            write_browser_response(&mut stream, false).await;
            continue;
        }
        if fields.get("openid.mode").map(String::as_str) == Some("cancel") {
            write_browser_response(&mut stream, false).await;
            return Err(AppError::new(
                "openid_cancelled",
                "Se canceló el inicio de sesión en Steam.",
            ));
        }

        let result = async {
            validate_assertion_fields(&fields, &return_to)?;
            verify_with_provider(&fields).await?;

            let nonce = fields
                .get("openid.response_nonce")
                .ok_or_else(|| AppError::validation("Steam no devolvió el nonce de OpenID."))?;
            validate_nonce_timestamp(nonce)?;
            database.use_openid_nonce(nonce)?;

            let claimed_id = fields
                .get("openid.claimed_id")
                .ok_or_else(|| AppError::validation("Steam no devolvió una identidad OpenID."))?;
            let steam_id = parse_claimed_id(claimed_id)?;
            database.save_steam_identity(&steam_id)?;
            Ok(steam_id)
        }
        .await;

        write_browser_response(&mut stream, result.is_ok()).await;
        return result;
    }

    Err(AppError::new(
        "openid_timeout",
        "No se recibió la confirmación válida de Steam dentro del tiempo disponible.",
    ))
}

fn build_authorization_url(realm: &str, return_to: &str) -> AppResult<Url> {
    let mut url = Url::parse(OPENID_ENDPOINT)
        .map_err(|_| AppError::new("openid_configuration", "El endpoint de Steam no es válido."))?;
    url.query_pairs_mut()
        .append_pair("openid.ns", OPENID_NS)
        .append_pair("openid.mode", "checkid_setup")
        .append_pair("openid.return_to", return_to)
        .append_pair("openid.realm", realm)
        .append_pair("openid.identity", IDENTIFIER_SELECT)
        .append_pair("openid.claimed_id", IDENTIFIER_SELECT);
    Ok(url)
}

fn validate_assertion_fields(fields: &HashMap<String, String>, return_to: &str) -> AppResult<()> {
    let expected = [
        ("openid.ns", OPENID_NS),
        ("openid.mode", "id_res"),
        ("openid.op_endpoint", OPENID_ENDPOINT),
        ("openid.return_to", return_to),
    ];
    for (field, expected_value) in expected {
        if fields.get(field).map(String::as_str) != Some(expected_value) {
            return Err(AppError::new(
                "openid_assertion",
                "Steam devolvió una afirmación OpenID incompleta o no válida.",
            ));
        }
    }
    let identity = fields.get("openid.identity");
    let claimed_id = fields.get("openid.claimed_id");
    if identity.is_none() || identity != claimed_id {
        return Err(AppError::new(
            "openid_identity",
            "La identidad firmada por Steam no coincide con la identidad declarada.",
        ));
    }

    let signed: HashSet<&str> = fields
        .get("openid.signed")
        .map(String::as_str)
        .unwrap_or_default()
        .split(',')
        .collect();
    let required_signed = [
        "op_endpoint",
        "claimed_id",
        "identity",
        "return_to",
        "response_nonce",
        "assoc_handle",
    ];
    if required_signed.iter().any(|field| !signed.contains(field))
        || fields.get("openid.sig").is_none_or(String::is_empty)
    {
        return Err(AppError::new(
            "openid_signature",
            "La respuesta OpenID de Steam no contiene todos los campos firmados requeridos.",
        ));
    }
    Ok(())
}

async fn verify_with_provider(fields: &HashMap<String, String>) -> AppResult<()> {
    let mut form: Vec<(String, String)> = fields
        .iter()
        .filter(|(key, _)| key.starts_with("openid."))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    form.retain(|(key, _)| key != "openid.mode");
    form.push(("openid.mode".into(), "check_authentication".into()));

    let client = Client::builder()
        .connect_timeout(StdDuration::from_secs(10))
        .timeout(StdDuration::from_secs(20))
        .redirect(Policy::none())
        .user_agent("Vindexa/0.1 (+https://vindexa.app)")
        .build()
        .map_err(|_| provider_error())?;
    let mut response = client
        .post(OPENID_ENDPOINT)
        .form(&form)
        .send()
        .await
        .map_err(|_| provider_error())?;
    if !response.status().is_success() {
        return Err(provider_error());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROVIDER_RESPONSE as u64)
    {
        return Err(provider_error());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| provider_error())? {
        if bytes.len().saturating_add(chunk.len()) > MAX_PROVIDER_RESPONSE {
            return Err(provider_error());
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = std::str::from_utf8(&bytes).map_err(|_| provider_error())?;
    let valid = body.lines().any(|line| line.trim() == "is_valid:true");
    if !valid {
        return Err(AppError::new(
            "openid_rejected",
            "Steam no pudo validar la firma del inicio de sesión.",
        ));
    }
    Ok(())
}

async fn read_request_target(stream: &mut TcpStream) -> AppResult<String> {
    let mut bytes = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if bytes.len() > MAX_HTTP_REQUEST {
            return Err(AppError::validation(
                "La respuesta local de autenticación es demasiado grande.",
            ));
        }
    }
    let request = std::str::from_utf8(&bytes).map_err(|_| {
        AppError::validation("La respuesta local no está codificada correctamente.")
    })?;
    parse_request_target(request)
}

fn parse_request_target(request: &str) -> AppResult<String> {
    let first_line = request
        .split("\r\n")
        .next()
        .ok_or_else(|| AppError::validation("La respuesta local está vacía."))?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next();
    let target = parts.next();
    let protocol = parts.next();
    if method != Some("GET")
        || protocol.is_none_or(|value| !value.starts_with("HTTP/1."))
        || parts.next().is_some()
    {
        return Err(AppError::validation(
            "La respuesta local de autenticación no es una petición HTTP válida.",
        ));
    }
    let target = target.ok_or_else(|| AppError::validation("Falta la URL de retorno local."))?;
    if !target.starts_with("/steam/openid?") {
        return Err(AppError::validation(
            "La respuesta local no corresponde al callback de Steam.",
        ));
    }
    Ok(target.to_owned())
}

fn parse_claimed_id(value: &str) -> AppResult<String> {
    let steam_id = value
        .strip_prefix("https://steamcommunity.com/openid/id/")
        .ok_or_else(|| {
            AppError::new(
                "openid_claimed_id",
                "Steam devolvió un identificador OpenID con un origen inesperado.",
            )
        })?;
    validate_steam_id(steam_id)?;
    Ok(steam_id.to_owned())
}

fn validate_nonce_timestamp(nonce: &str) -> AppResult<()> {
    let timestamp = nonce.get(..20).ok_or_else(|| {
        AppError::new(
            "openid_nonce",
            "Steam devolvió un nonce OpenID con un formato no válido.",
        )
    })?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .map_err(|_| {
            AppError::new(
                "openid_nonce",
                "El nonce OpenID no contiene una fecha válida.",
            )
        })?
        .with_timezone(&Utc);
    let now = Utc::now();
    if timestamp < now - Duration::minutes(10) || timestamp > now + Duration::minutes(2) {
        return Err(AppError::new(
            "openid_nonce_expired",
            "La respuesta de Steam ha caducado. Inicia sesión de nuevo.",
        ));
    }
    Ok(())
}

async fn write_browser_response(stream: &mut TcpStream, success: bool) {
    let (title, message, accent) = if success {
        (
            "Steam conectado",
            "Vindexa ha verificado tu identidad. Ya puedes cerrar esta pestaña y volver a la aplicación.",
            "#66c0f4",
        )
    } else {
        (
            "No se pudo conectar Steam",
            "Vuelve a Vindexa para consultar el error y reintentar el inicio de sesión.",
            "#d85c5c",
        )
    };
    let body = format!(
        "<!doctype html><html lang=\"es\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>{title}</title><body style=\"margin:0;display:grid;place-items:center;min-height:100vh;background:#171d25;color:#eeeeef;font:16px system-ui\"><main style=\"max-width:520px;padding:40px;background:#22262d;border-top:3px solid {accent};box-shadow:0 20px 60px #0008\"><h1 style=\"margin-top:0\">{title}</h1><p style=\"color:#abb7b5;line-height:1.6\">{message}</p></main></body></html>"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn provider_error() -> AppError {
    AppError::new(
        "openid_provider",
        "No se pudo verificar el inicio de sesión directamente con Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_claimed_id, parse_request_target, validate_assertion_fields};
    use std::collections::HashMap;

    #[test]
    fn accepts_only_the_official_claimed_id_shape() {
        assert_eq!(
            parse_claimed_id("https://steamcommunity.com/openid/id/76561198000000000").unwrap(),
            "76561198000000000"
        );
        assert!(parse_claimed_id("https://example.com/76561198000000000").is_err());
        assert!(parse_claimed_id("https://steamcommunity.com/openid/id/not-a-number").is_err());
    }

    #[test]
    fn parses_only_the_expected_loopback_request_path() {
        let target = parse_request_target(
            "GET /steam/openid?state=abc&openid.mode=id_res HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        )
        .unwrap();
        assert!(target.starts_with("/steam/openid?"));
        assert!(parse_request_target("POST /steam/openid?x=1 HTTP/1.1\r\n\r\n").is_err());
        assert!(parse_request_target("GET /other?x=1 HTTP/1.1\r\n\r\n").is_err());
    }

    #[test]
    fn assertion_requires_identity_and_critical_signed_fields() {
        let return_to = "http://127.0.0.1:1234/steam/openid?state=abc";
        let claimed = "https://steamcommunity.com/openid/id/76561198000000000";
        let mut fields = HashMap::from([
            (
                "openid.ns".into(),
                "http://specs.openid.net/auth/2.0".into(),
            ),
            ("openid.mode".into(), "id_res".into()),
            (
                "openid.op_endpoint".into(),
                "https://steamcommunity.com/openid/login".into(),
            ),
            ("openid.return_to".into(), return_to.into()),
            ("openid.identity".into(), claimed.into()),
            ("openid.claimed_id".into(), claimed.into()),
            ("openid.sig".into(), "signed-value".into()),
            (
                "openid.signed".into(),
                "op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle".into(),
            ),
        ]);
        assert!(validate_assertion_fields(&fields, return_to).is_ok());
        fields.insert("openid.op_endpoint".into(), "https://example.com".into());
        assert!(validate_assertion_fields(&fields, return_to).is_err());
    }
}
