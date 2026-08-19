//! El agente que vive dentro de Vindexa.
//!
//! # Para qué
//!
//! Si hay un agente instalado en el ordenador, Vindexa se conecta a él y se
//! conduce desde donde ese agente ya viva —su ventana, Telegram, lo que sea—.
//! Esto es la otra mitad: para quien no tiene ninguno, o para quien prefiere no
//! salir de la aplicación. Habla con un servidor de inferencia local, le da las
//! mismas herramientas que a cualquier otro agente y ejecuta lo que pida.
//!
//! # Qué comparte con los demás agentes
//!
//! Todo lo que importa. Las herramientas son las de [`crate::mcp::tools`], las
//! ejecuta [`crate::agent::bridge`] y por tanto pasan por los mismos ámbitos,
//! el mismo límite de frecuencia, la misma auditoría y el mismo deshacer. Ser
//! el agente de casa no le da un atajo: tiene su propio cliente y su testigo,
//! y se puede revocar como cualquiera.
//!
//! # Qué no hace
//!
//! - **No sale del ordenador.** Sólo habla con `127.0.0.1`. Un modelo que vive
//!   en otra máquina ya no es local, y la biblioteca de alguien no viaja a un
//!   sitio que no ha elegido.
//! - **No decide solo.** Lo que exige confirmación humana sigue esperando
//!   dentro de Vindexa; el agente no aprueba sus propias acciones.
//! - **No se queda callado.** Cada paso —lo que llamó y con qué— vuelve a la
//!   interfaz, porque un agente que ordena tu biblioteca sin contar qué hizo es
//!   exactamente lo que nadie quiere.

use crate::agent::bridge::{self, AgentRequest};
use crate::agent::clients::{self, NewAgentClient};
use crate::agent::ratelimit::RateLimiter;
use crate::agent::scope::ALL_SCOPES;
use crate::agent::token::TokenPolicy;
use crate::db::Database;
use crate::error::{AppError, AppResult};
use crate::mcp::tools;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

/// Servicio y cuenta con los que se guarda el testigo en el llavero.
const KEYCHAIN_SERVICE: &str = "io.vindexa.desktop";
const KEYCHAIN_ACCOUNT: &str = "vindagent-token";

/// Nombre del cliente que se da de alta en el puente.
const CLIENT_NAME: &str = "Vindagent";

/// Cuántas vueltas de herramientas se permiten en un turno.
///
/// Un modelo puede encadenar «consulta los estados» → «cambia el estado» → «lee
/// cómo quedó». Más de esto no es trabajo: es un bucle, y un bucle con acceso a
/// escritura hay que cortarlo.
const MAX_TOOL_ROUNDS: usize = 8;

/// Plazo de cada petición al modelo. Un modelo grande en una máquina cargada
/// tarda, pero no eternamente.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// Un mensaje de la conversación, tal y como la interfaz la guarda y la enseña.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// `user`, `assistant` o `tool`.
    pub role: String,
    #[serde(default)]
    pub content: String,
    /// Nombre de la herramienta, cuando el mensaje es el resultado de una.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
}

/// Lo que devuelve un turno.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatTurn {
    /// Respuesta final del agente.
    pub reply: String,
    /// Lo que hizo por el camino, en orden y en cristiano.
    pub steps: Vec<ChatStep>,
}

/// Una llamada a herramienta, con lo que pidió y lo que salió.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStep {
    pub tool: String,
    pub arguments: Value,
    pub result: String,
    pub failed: bool,
}

/// Instrucción de sistema. Es la misma promesa que se le hace a cualquier otro
/// agente, más lo que sólo aplica aquí dentro.
fn system_prompt() -> String {
    format!(
        "Eres el agente de Vindexa, la biblioteca de juegos local de quien te habla. \
Tu trabajo es ordenarla cuando te lo pidan: registrar partidas, cambiar estados, \
ajustar prioridades, crear colecciones, planificar.\n\n\
Reglas que no se negocian:\n\
- Consulta antes de escribir. Los estados, las colecciones y los nombres exactos salen de «consultar».\n\
- No inventes nada. Si no te han dicho el progreso, no lo pongas. Si no sabes la fecha, no la fijes.\n\
- Si un nombre de juego es ambiguo, la respuesta te da las opciones: pregunta cuál, no elijas tú.\n\
- Responde en español, breve y concreto, diciendo qué has hecho.\n\
- Hoy es {}.",
        chrono::Utc::now().format("%Y-%m-%d")
    )
}

/// Las herramientas, en el formato que espera un servidor compatible con OpenAI.
fn tool_definitions() -> Vec<Value> {
    tools::TOOLS
        .iter()
        .chain(std::iter::once(&tools::UNDO_TOOL))
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": (tool.schema)()
                }
            })
        })
        .collect()
}

/// Testigo del agente de casa, dado de alta la primera vez que hace falta.
///
/// Vive en el llavero del sistema y no en la base: es una credencial, y las
/// credenciales de esta aplicación viven donde el sistema sabe protegerlas.
fn ensure_token(database: &Database) -> AppResult<String> {
    if let Ok(token) = crate::keychain::get(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        let connection = database.open()?;
        // Un testigo guardado que ya no vale no sirve de nada: se comprueba
        // contra el puente antes de darlo por bueno.
        if clients::authenticate(&connection, &token).is_ok() {
            return Ok(token);
        }
    }
    let mut connection = database.open()?;
    let issued = clients::issue(
        &mut connection,
        &NewAgentClient {
            name: CLIENT_NAME.to_owned(),
            kind: "generic".to_owned(),
            scopes: ALL_SCOPES
                .iter()
                .map(|scope| scope.as_str().to_owned())
                .collect(),
        },
        TokenPolicy::default(),
    )?;
    crate::keychain::set(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &issued.token).map_err(|error| {
        AppError::new(
            "vindagent",
            format!("No se pudo guardar el testigo del agente: {error}"),
        )
    })?;
    Ok(issued.token)
}

/// Ejecuta una herramienta contra el puente y devuelve el texto que verá el
/// modelo.
fn run_tool(
    database: &Database,
    limiter: &RateLimiter,
    token: &str,
    name: &str,
    arguments: &Value,
) -> (String, bool) {
    let mut payload = arguments.clone();
    let Some(object) = payload.as_object_mut() else {
        return ("Los argumentos tienen que ser un objeto.".to_owned(), true);
    };

    if name == tools::UNDO_TOOL.name {
        let Ok(mut connection) = database.open() else {
            return ("No se pudo abrir la biblioteca.".to_owned(), true);
        };
        let Ok(client) = clients::authenticate(&connection, token) else {
            return ("El testigo del agente ya no vale.".to_owned(), true);
        };
        let undo_token = match object.get("undoToken").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.to_owned(),
            _ => {
                let Some(audit_id) = object.get("auditId").and_then(Value::as_str) else {
                    return ("Falta «undoToken» o «auditId».".to_owned(), true);
                };
                match crate::agent::audit::undo_token_for_client(&connection, audit_id, &client.id)
                {
                    Ok(value) => value,
                    Err(error) => return (error.message, true),
                }
            }
        };
        return match bridge::undo(
            &mut connection,
            &undo_token,
            &bridge::Requester::Client(client.id),
        ) {
            Ok(outcome) => (describe(&outcome), false),
            Err(error) => (error.message, true),
        };
    }

    object.insert("intent".to_owned(), Value::String(name.to_owned()));
    let intent = match serde_json::from_value(payload) {
        Ok(value) => value,
        Err(error) => return (format!("Los argumentos no encajan: {error}"), true),
    };
    let Ok(mut connection) = database.open() else {
        return ("No se pudo abrir la biblioteca.".to_owned(), true);
    };
    let request = AgentRequest {
        token: token.to_owned(),
        utterance: String::new(),
        intent,
    };
    match bridge::dispatch(&mut connection, limiter, &request) {
        Ok(outcome) => (describe(&outcome), false),
        Err(error) => (error.message, true),
    }
}

/// El resultado del puente, en texto, para que el modelo pueda encadenar.
fn describe(outcome: &bridge::AgentOutcome) -> String {
    serde_json::to_string(outcome).unwrap_or_else(|_| "{}".to_owned())
}

#[derive(Deserialize)]
struct Completion {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    #[serde(default)]
    message: AssistantMessage,
}

#[derive(Default, Deserialize)]
struct AssistantMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

#[derive(Clone, Deserialize)]
struct ToolCall {
    #[serde(default)]
    id: String,
    #[serde(default)]
    function: ToolFunction,
}

#[derive(Clone, Default, Deserialize)]
struct ToolFunction {
    #[serde(default)]
    name: String,
    #[serde(default)]
    arguments: String,
}

/// Comprueba que la dirección apunta de verdad a este ordenador.
///
/// # Por qué no basta con mirar cómo empieza
///
/// `http://127.0.0.1:8080@servidor.ajeno.tld/` empieza por `http://127.0.0.1:`
/// y sin embargo la petición viaja a `servidor.ajeno.tld`: lo de delante de la
/// arroba son credenciales, no un anfitrión. Comparar el principio de la cadena
/// es exactamente el error que convierte una frontera en un adorno. Aquí se
/// analiza la URL y se mira el anfitrión que se va a usar, no el texto.
///
/// Se rechaza además cualquier cosa con credenciales o con ruta: el destino se
/// compone luego añadiendo `/v1/chat/completions`, y una ruta previa serviría
/// para llevar la petición a otro sitio dentro del mismo servidor.
fn ensure_loopback(base_url: &str) -> AppResult<()> {
    let denegado = || {
        AppError::validation(
            "El agente de Vindexa sólo habla con un modelo que esté en este ordenador.",
        )
    };
    let url = url::Url::parse(base_url).map_err(|_| denegado())?;
    if url.scheme() != "http" {
        return Err(denegado());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(denegado());
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err(denegado());
    }
    match url.host() {
        Some(url::Host::Ipv4(address)) if address.is_loopback() => Ok(()),
        Some(url::Host::Ipv6(address)) if address.is_loopback() => Ok(()),
        Some(url::Host::Domain("localhost")) => Ok(()),
        _ => Err(denegado()),
    }
}

/// Un turno completo: se le pasa la conversación y se devuelve la respuesta,
/// habiendo ejecutado por el camino lo que el modelo haya pedido.
pub async fn chat(
    database: &Database,
    base_url: &str,
    model: &str,
    history: &[ChatMessage],
) -> AppResult<ChatTurn> {
    // Sólo el bucle local. No es una preferencia: es la frontera de esta
    // función, y comprobarla aquí evita que un ajuste mal escrito mande la
    // biblioteca de alguien a un servidor de internet.
    ensure_loopback(base_url)?;
    let token = ensure_token(database)?;
    let limiter = RateLimiter::default();
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| AppError::new("vindagent", format!("Cliente HTTP: {error}")))?;

    let mut messages: Vec<Value> = vec![json!({ "role": "system", "content": system_prompt() })];
    for message in history {
        messages.push(json!({ "role": message.role, "content": message.content }));
    }

    let mut steps = Vec::new();
    for _ in 0..MAX_TOOL_ROUNDS {
        let response = client
            .post(format!("{base_url}/v1/chat/completions"))
            .json(&json!({
                "model": model,
                "messages": messages,
                "tools": tool_definitions(),
                "tool_choice": "auto",
                "temperature": 0.3,
                "stream": false
            }))
            .send()
            .await
            .map_err(|error| {
                AppError::new(
                    "vindagent",
                    format!("El modelo local no contestó: {error}. ¿Sigue corriendo?"),
                )
            })?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AppError::new(
                "vindagent",
                format!(
                    "El modelo local respondió {status}: {}",
                    body.chars().take(300).collect::<String>()
                ),
            ));
        }
        let completion: Completion = serde_json::from_str(&body).map_err(|error| {
            AppError::new(
                "vindagent",
                format!("No se entendió la respuesta del modelo: {error}"),
            )
        })?;
        let Some(choice) = completion.choices.into_iter().next() else {
            return Err(AppError::new(
                "vindagent",
                "El modelo local devolvió una respuesta vacía.",
            ));
        };

        if choice.message.tool_calls.is_empty() {
            return Ok(ChatTurn {
                reply: choice.message.content.unwrap_or_default(),
                steps,
            });
        }

        // El mensaje del modelo con sus llamadas se conserva tal cual: sin él,
        // los resultados que vienen después no tienen a qué referirse.
        messages.push(json!({
            "role": "assistant",
            "content": choice.message.content.clone().unwrap_or_default(),
            "tool_calls": choice.message.tool_calls.iter().map(|call| json!({
                "id": call.id,
                "type": "function",
                "function": { "name": call.function.name, "arguments": call.function.arguments }
            })).collect::<Vec<_>>()
        }));

        for call in &choice.message.tool_calls {
            let arguments: Value =
                serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| json!({}));
            let (result, failed) = run_tool(
                database,
                &limiter,
                &token,
                &call.function.name,
                &arguments,
            );
            steps.push(ChatStep {
                tool: call.function.name.clone(),
                arguments,
                result: result.clone(),
                failed,
            });
            messages.push(json!({
                "role": "tool",
                "tool_call_id": call.id,
                "name": call.function.name,
                "content": result
            }));
        }
    }

    Err(AppError::new(
        "vindagent",
        "El agente se ha quedado dando vueltas sin llegar a una respuesta. Vuelve a intentarlo con una petición más concreta.",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_habla_con_nada_que_no_este_en_este_ordenador() {
        // Es la frontera de esta función: un modelo «local» en otra máquina ya
        // no es local, y la biblioteca de alguien no viaja a donde no ha dicho.
        for base in [
            "https://api.openai.com",
            "http://192.168.1.50:8080",
            "http://modelos.ejemplo.com",
            "http://10.0.0.5:8080",
            // Lo de delante de la arroba son credenciales, no un anfitrión:
            // esto empieza por «http://127.0.0.1:» y viaja a otro servidor.
            // Comparar el principio de la cadena convertía la frontera en un
            // adorno, y así entraba y salía la biblioteca de alguien.
            "http://127.0.0.1:8080@servidor.ajeno.tld/",
            "http://localhost:8080@servidor.ajeno.tld/",
            "http://127.0.0.1.servidor.ajeno.tld:8080",
            // Con ruta propia, el destino real lo decide quien la escribió.
            "http://127.0.0.1:8080/proxy",
            "http://127.0.0.1:8080/?destino=https://ajeno.tld",
            // Otro esquema es otra cosa: `file://` no es hablar con un modelo.
            "https://127.0.0.1:8080",
            "file:///etc/passwd",
        ] {
            let error = ensure_loopback(base).expect_err("rechazar");
            assert_eq!(error.code, "validation", "{base}");
        }
    }

    #[test]
    fn si_acepta_lo_que_de_verdad_es_local() {
        for base in [
            "http://127.0.0.1:8770",
            "http://127.0.0.1:8080/",
            "http://localhost:1234",
            "http://[::1]:8080",
        ] {
            ensure_loopback(base).unwrap_or_else(|error| panic!("{base}: {}", error.message));
        }
    }

    #[test]
    fn las_herramientas_son_las_mismas_que_ve_cualquier_agente() {
        // Si el agente de casa tuviera un catálogo propio, acabaría siendo otro
        // y las garantías dejarían de valer para él.
        let definiciones = tool_definitions();
        assert_eq!(definiciones.len(), tools::TOOLS.len() + 1);
        for definicion in &definiciones {
            assert_eq!(definicion["type"], "function");
            assert!(definicion["function"]["name"].is_string());
            assert!(definicion["function"]["parameters"]["type"] == "object");
        }
    }

    #[test]
    fn la_instruccion_de_sistema_prohibe_inventar() {
        let prompt = system_prompt();
        assert!(prompt.contains("No inventes"));
        assert!(prompt.contains("Consulta antes de escribir"));
    }
}
