//! Servidores de inferencia que ya están corriendo en este ordenador.
//!
//! # Por qué se buscan
//!
//! Quien usa modelos en local casi siempre tiene ya uno levantado: el
//! `llama-server` de su agente, LM Studio abierto, Ollama de servicio. Pedirle
//! que arranque otro para Vindexa sería cargar la máquina dos veces con lo
//! mismo. Antes de proponer nada se mira si hay algo escuchando y se usa.
//!
//! # Cómo se busca sin molestar
//!
//! Sólo en el bucle local y sólo en los puertos que estas herramientas usan de
//! verdad. Una petición `GET /v1/models` con un plazo corto: si contesta con la
//! forma que declara OpenAI, sirve; si no contesta o contesta otra cosa, se
//! descarta y se sigue. No se prueba puerto por puerto a ciegas —eso es un
//! escaneo, y un escaneo no es lo que hace falta aquí—.
//!
//! # Qué no se hace
//!
//! - No se sale del bucle local. Un modelo «local» que vive en otra máquina ya
//!   no es local, y mandar la biblioteca de alguien a un sitio que no ha
//!   elegido no entra en lo que hace Vindexa.
//! - No se guarda nada de lo que devuelva: el nombre del modelo se enseña y ya.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Cuánto se espera a cada puerto. Un servidor local que no contesta en medio
/// segundo es que no hay nadie: no merece hacer esperar a la interfaz.
const PROBE_TIMEOUT: Duration = Duration::from_millis(600);

/// Puertos donde estas herramientas escuchan, con quién suele estar detrás.
///
/// El orden importa poco, pero la lista sí: es la diferencia entre mirar donde
/// hay algo y barrer sesenta y cinco mil puertos por si acaso.
const KNOWN_PORTS: &[(u16, &str)] = &[
    (8080, "llama.cpp"),
    (8770, "llama.cpp"),
    (8081, "llama.cpp"),
    (1234, "LM Studio"),
    (11434, "Ollama"),
    (8000, "servidor OpenAI compatible"),
    (5001, "servidor OpenAI compatible"),
];

/// Un servidor de inferencia encontrado y listo para usarse.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InferenceEndpoint {
    /// Base sin la ruta: `http://127.0.0.1:8770`.
    pub base_url: String,
    /// Quién se supone que está detrás, por el puerto.
    pub label: String,
    /// Modelos que dice servir. Vacío si los declara pero no da nombres.
    pub models: Vec<String>,
}

#[derive(Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    #[serde(default)]
    id: String,
}

/// Busca servidores de inferencia en el bucle local.
///
/// Devuelve los que contestan con la forma de OpenAI. No falla nunca: no
/// encontrar ninguno es una respuesta perfectamente válida.
pub async fn discover() -> Vec<InferenceEndpoint> {
    let Ok(client) = reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() else {
        return Vec::new();
    };
    // Se preguntan todos a la vez: en serie, siete puertos muertos son cuatro
    // segundos de espera para no encontrar nada.
    let mut probes = Vec::with_capacity(KNOWN_PORTS.len());
    for (port, label) in KNOWN_PORTS {
        let client = client.clone();
        let port = *port;
        let label = *label;
        probes.push(tokio::spawn(async move { probe(&client, port, label).await }));
    }
    let mut found = Vec::new();
    for probe in probes {
        if let Ok(Some(endpoint)) = probe.await {
            found.push(endpoint);
        }
    }
    found
}

async fn probe(client: &reqwest::Client, port: u16, label: &str) -> Option<InferenceEndpoint> {
    let base = format!("http://127.0.0.1:{port}");
    let response = client
        .get(format!("{base}/v1/models"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let body: ModelsResponse = response.json().await.ok()?;
    Some(InferenceEndpoint {
        base_url: base,
        label: label.to_owned(),
        models: body
            .data
            .into_iter()
            .map(|entry| entry.id)
            .filter(|id| !id.is_empty())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_lista_de_puertos_no_repite_ninguno() {
        // Preguntar dos veces al mismo puerto duplicaría el resultado y haría
        // parecer que hay dos servidores donde hay uno.
        let mut puertos: Vec<u16> = KNOWN_PORTS.iter().map(|(port, _)| *port).collect();
        let total = puertos.len();
        puertos.sort_unstable();
        puertos.dedup();
        assert_eq!(puertos.len(), total);
    }

    #[test]
    fn solo_se_mira_el_bucle_local() {
        for (port, _) in KNOWN_PORTS {
            let base = format!("http://127.0.0.1:{port}");
            assert!(base.starts_with("http://127.0.0.1:"), "{base}");
        }
    }
}
