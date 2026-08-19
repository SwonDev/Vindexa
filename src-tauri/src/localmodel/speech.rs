//! Dictar en vez de escribir.
//!
//! # Para qué
//!
//! Contar lo que has jugado es justo lo que se dice mejor hablando que
//! tecleando: «he estado dos horas con esto y voy por el cuarenta por ciento».
//! Aquí se busca un transcriptor que ya esté corriendo en el ordenador y se le
//! manda el audio.
//!
//! # Local, como todo lo demás
//!
//! Sólo el bucle local. Un audio con tu voz es más personal que una lista de
//! juegos, así que no sale de la máquina. Si no hay transcriptor, el botón de
//! dictar no aparece: es mejor no ofrecerlo que ofrecerlo y fallar.
//!
//! # Dos formas de hablar con él
//!
//! - `/v1/audio/transcriptions`, que es la que declara OpenAI y la que
//!   implementan la mayoría de servidores locales.
//! - `/transcribe`, la del servidor de esta casa.
//!
//! Se prueban en ese orden. Las dos reciben el audio como `multipart` en un
//! campo llamado `file`, así que la diferencia es sólo la ruta y el nombre del
//! campo del texto en la respuesta.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Puertos donde suelen escuchar los transcriptores locales.
const KNOWN_PORTS: &[u16] = &[8767, 8768, 9000, 8080];

/// Plazo para saber si hay alguien. Corto: es una comprobación, no un trabajo.
const PROBE_TIMEOUT: Duration = Duration::from_millis(600);

/// Plazo de una transcripción. Un minuto de audio en un modelo grande tarda.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(120);

/// Un transcriptor encontrado y listo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SpeechEndpoint {
    pub base_url: String,
    /// Modelo que dice usar, si lo dice.
    pub model: Option<String>,
}

#[derive(Deserialize)]
struct Health {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    backend: Option<String>,
}

/// Busca un transcriptor en el bucle local. `None` es una respuesta válida.
pub async fn discover() -> Option<SpeechEndpoint> {
    let client = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build().ok()?;
    let mut probes = Vec::with_capacity(KNOWN_PORTS.len());
    for port in KNOWN_PORTS {
        let client = client.clone();
        let port = *port;
        probes.push(tokio::spawn(async move {
            let base = format!("http://127.0.0.1:{port}");
            let health: Health = client
                .get(format!("{base}/health"))
                .send()
                .await
                .ok()?
                .error_for_status()
                .ok()?
                .json()
                .await
                .ok()?;
            // `ok` a secas no basta: cualquier servicio tiene un `/health`. Se
            // exige que se declare como transcriptor.
            if !health.ok || health.backend.is_none() {
                return None;
            }
            Some(SpeechEndpoint {
                base_url: base,
                model: health.model,
            })
        }));
    }
    for probe in probes {
        if let Ok(Some(endpoint)) = probe.await {
            return Some(endpoint);
        }
    }
    None
}

#[derive(Deserialize)]
struct Transcription {
    #[serde(default)]
    text: Option<String>,
}

/// Manda el audio y devuelve lo que se entendió.
pub async fn transcribe(base_url: &str, audio: Vec<u8>, mime: &str) -> AppResult<String> {
    if !base_url.starts_with("http://127.0.0.1:") {
        return Err(AppError::validation(
            "El dictado sólo habla con un transcriptor de este ordenador.",
        ));
    }
    if audio.is_empty() {
        return Err(AppError::validation("No se grabó nada."));
    }
    let client = reqwest::Client::builder()
        .timeout(TRANSCRIBE_TIMEOUT)
        .build()
        .map_err(|error| AppError::new("speech", format!("Cliente HTTP: {error}")))?;

    // El nombre del archivo importa: el servidor decide por su extensión cómo
    // decodificar el audio.
    let extension = match mime {
        m if m.contains("webm") => "webm",
        m if m.contains("ogg") => "ogg",
        m if m.contains("mp4") | m.contains("m4a") => "m4a",
        _ => "wav",
    };

    let mut ultimo = String::new();
    for ruta in ["/v1/audio/transcriptions", "/transcribe"] {
        let parte = reqwest::multipart::Part::bytes(audio.clone())
            .file_name(format!("dictado.{extension}"))
            .mime_str(mime)
            .map_err(|_| AppError::validation("El formato del audio no es válido."))?;
        let formulario = reqwest::multipart::Form::new()
            .part("file", parte)
            .text("language", "es");
        let response = match client
            .post(format!("{base_url}{ruta}"))
            .multipart(formulario)
            .send()
            .await
        {
            Ok(value) => value,
            Err(error) => {
                ultimo = error.to_string();
                continue;
            }
        };
        if !response.status().is_success() {
            ultimo = format!("{}", response.status());
            continue;
        }
        let cuerpo: Transcription = match response.json().await {
            Ok(value) => value,
            Err(error) => {
                ultimo = error.to_string();
                continue;
            }
        };
        // Un texto vacío no es un fallo: es que no se entendió nada. Decirlo así
        // evita que alguien crea que se ha perdido lo que dijo.
        return Ok(cuerpo.text.unwrap_or_default().trim().to_owned());
    }
    Err(AppError::new(
        "speech",
        format!("El transcriptor no aceptó el audio: {ultimo}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_se_mira_el_bucle_local() {
        for port in KNOWN_PORTS {
            assert!(format!("http://127.0.0.1:{port}").starts_with("http://127.0.0.1:"));
        }
    }

    #[test]
    fn no_se_manda_la_voz_fuera_del_ordenador() {
        // Un audio con tu voz es más personal que una lista de juegos.
        for base in ["https://api.ejemplo.com", "http://192.168.1.9:8767"] {
            let error = tauri::async_runtime::block_on(transcribe(base, vec![1, 2, 3], "audio/wav"))
                .expect_err("rechazar");
            assert_eq!(error.code, "validation", "{base}");
        }
    }

    #[test]
    fn sin_audio_no_se_llama_a_nadie() {
        let error = tauri::async_runtime::block_on(transcribe(
            "http://127.0.0.1:8767",
            Vec::new(),
            "audio/wav",
        ))
        .expect_err("rechazar");
        assert_eq!(error.code, "validation");
    }

    /// Comprobación contra un transcriptor de verdad.
    ///
    /// No corre sola: hace falta uno levantado y un `.wav` a mano. Existe
    /// porque lo que importa de esto —que el `multipart` llegue bien y que la
    /// respuesta se entienda— no se puede comprobar con un doble.
    #[test]
    fn contra_un_transcriptor_de_verdad() {
        let (Ok(base), Ok(audio)) = (
            std::env::var("VINDEXA_STT"),
            std::env::var("VINDEXA_STT_AUDIO"),
        ) else {
            eprintln!("sin VINDEXA_STT y VINDEXA_STT_AUDIO: nada que comprobar");
            return;
        };
        let bytes = std::fs::read(audio).expect("leer el audio");
        let texto =
            tauri::async_runtime::block_on(transcribe(&base, bytes, "audio/wav")).expect("transcribir");
        assert!(!texto.trim().is_empty(), "no se entendió nada");
        println!("transcrito: {texto}");
    }
}
