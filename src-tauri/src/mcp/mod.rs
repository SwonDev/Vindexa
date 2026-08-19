//! Vindexa como servidor MCP.
//!
//! # Para qué
//!
//! El puente de agentes ([`crate::agent`]) sabe hacer el trabajo —registrar una
//! sesión, cambiar un estado, crear una colección— con ámbitos, límite de
//! frecuencia, auditoría y deshacer. Lo que le faltaba era una puerta: una
//! forma de que un agente que ya vive en el ordenador de quien usa Vindexa
//! —Hermes, Claude, Codex, el que sea— pueda llamarlo.
//!
//! Esa puerta es el Model Context Protocol. El agente arranca este proceso, le
//! habla por la entrada estándar y recibe las respuestas por la salida.
//!
//! # Por qué así y no con un puerto
//!
//! Un socket en `127.0.0.1` está abierto para **cualquier** proceso local y
//! para cualquier página web capaz de hacerle una petición. Aquí no hay socket:
//! el transporte es la tubería que abre quien lanza el proceso, y sólo quien lo
//! lanza puede hablar con él. Además hace falta un testigo válido, que se emite
//! desde Ajustes y se puede revocar.
//!
//! # Cómo se conecta
//!
//! ```text
//! hermes mcp add vindexa \
//!   --command /Applications/Vindexa.app/Contents/MacOS/vindexa \
//!   --args mcp \
//!   --env VINDEXA_AGENT_TOKEN=<testigo emitido en Ajustes>
//! ```
//!
//! A partir de ahí, cualquier canal que ya tenga ese agente —Telegram,
//! WhatsApp, su propia ventana— sirve para conducir Vindexa hablando.
//!
//! # Qué no hace
//!
//! - No inventa juegos: si el nombre es ambiguo devuelve las opciones y espera.
//! - No aprueba lo que exige confirmación humana: eso ocurre dentro de Vindexa.
//! - No amplía sus permisos: los ámbitos son los del testigo.

mod tools;

use crate::agent::bridge::{self, AgentRequest, Requester};
use crate::agent::ratelimit::RateLimiter;
use crate::db::Database;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

/// Versión del protocolo que se anuncia en `initialize`.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// Variable de entorno con el testigo emitido en Ajustes.
const TOKEN_ENV: &str = "VINDEXA_AGENT_TOKEN";

/// Variable opcional para apuntar a otra base. Existe para poder probar contra
/// una copia sin tocar la biblioteca de verdad.
const DATABASE_ENV: &str = "VINDEXA_DATABASE";

/// Dónde guarda Vindexa su base, con el mismo criterio que usa la aplicación.
///
/// No se puede preguntar a Tauri —aquí no hay aplicación— así que se replica su
/// regla por plataforma. Si no coincidiera, este proceso miraría una base vacía
/// y diría que la biblioteca no tiene nada, que es la peor forma de fallar.
fn default_database_path() -> Option<PathBuf> {
    let identifier = "io.vindexa.desktop";
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/Application Support")
                .join(identifier)
                .join("vindexa.sqlite3"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        Some(
            PathBuf::from(appdata)
                .join(identifier)
                .join("vindexa.sqlite3"),
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            return Some(
                PathBuf::from(data)
                    .join(identifier)
                    .join("vindexa.sqlite3"),
            );
        }
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local/share")
                .join(identifier)
                .join("vindexa.sqlite3"),
        )
    }
}

pub(crate) fn database_path() -> Option<PathBuf> {
    std::env::var_os(DATABASE_ENV)
        .map(PathBuf::from)
        .or_else(default_database_path)
}

/// Arranca el servidor y no vuelve hasta que se cierra la entrada estándar.
///
/// Devuelve el código de salida del proceso: cero si terminó porque el agente
/// cerró la tubería, uno si no pudo ni empezar.
pub fn run() -> i32 {
    let Some(path) = database_path() else {
        eprintln!("Vindexa MCP: no se pudo determinar dónde vive la base de datos.");
        return 1;
    };
    if !path.exists() {
        eprintln!(
            "Vindexa MCP: no hay base de datos en {}. Abre Vindexa una vez antes de conectar un agente.",
            path.display()
        );
        return 1;
    }
    let database = Database::new(path);
    let limiter = RateLimiter::default();
    // El testigo se lee una vez, aquí. Más adentro se pasa como argumento: leer
    // el entorno en mitad de la lógica la vuelve imposible de probar sin que las
    // pruebas se pisen entre ellas.
    let token = std::env::var(TOKEN_ENV).ok();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(response) = handle_line(&database, &limiter, token.as_deref(), line) else {
            // Una notificación no lleva respuesta. Contestar a una sería
            // romper el protocolo.
            continue;
        };
        let Ok(encoded) = serde_json::to_string(&response) else {
            continue;
        };
        if writeln!(stdout, "{encoded}").is_err() || stdout.flush().is_err() {
            break;
        }
    }
    0
}

/// Procesa una línea. `None` significa «era una notificación, no contestes».
fn handle_line(
    database: &Database,
    limiter: &RateLimiter,
    token: Option<&str>,
    line: &str,
) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("JSON mal formado: {error}"),
            ));
        }
    };
    let method = message.get("method")?.as_str()?.to_owned();
    let id = message.get("id").cloned();
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    // Sin `id` es una notificación: se acepta en silencio.
    let id = id?;

    Some(match method.as_str() {
        "initialize" => success(id, initialize_result()),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, tools_list()),
        "tools/call" => match call_tool(database, limiter, token, &params) {
            Ok(value) => success(id, value),
            Err(message) => success(id, tool_error(&message)),
        },
        _ => error_response(id, -32601, &format!("Método no soportado: {method}")),
    })
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "vindexa",
            "version": env!("CARGO_PKG_VERSION")
        },
        "instructions": concat!(
            "Vindexa es la biblioteca de juegos local de quien te habla. ",
            "Consulta antes de escribir: los estados, las colecciones y los nombres exactos salen de «consultar». ",
            "No inventes datos que no te hayan dicho —un porcentaje de progreso que nadie mencionó, una fecha que no existe—. ",
            "Si un nombre de juego es ambiguo, la respuesta trae las opciones: pregunta cuál en vez de elegir tú. ",
            "Todo cambio deja un «undoToken» con el que puedes deshacerlo."
        )
    })
}

fn tools_list() -> Value {
    let mut list: Vec<Value> = tools::TOOLS
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": (tool.schema)()
            })
        })
        .collect();
    list.push(json!({
        "name": tools::UNDO_TOOL.name,
        "description": tools::UNDO_TOOL.description,
        "inputSchema": (tools::UNDO_TOOL.schema)()
    }));
    json!({ "tools": list })
}

/// Ejecuta una herramienta. El `Err` es un mensaje para el agente, no un fallo
/// del protocolo: por eso viaja como resultado con `isError`.
fn call_tool(
    database: &Database,
    limiter: &RateLimiter,
    token: Option<&str>,
    params: &Value,
) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "Falta el nombre de la herramienta.".to_owned())?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let token = token.filter(|value| !value.trim().is_empty()).ok_or_else(|| {
        format!(
            "Falta el testigo. Emite uno en Vindexa → Ajustes → Agentes y pásalo en {TOKEN_ENV}."
        )
    })?;
    let mut connection = database
        .open()
        .map_err(|error| format!("No se pudo abrir la biblioteca: {}", error.message))?;

    if name == tools::UNDO_TOOL.name {
        // Quien deshace es el propio cliente: el puente comprueba que el cambio
        // fuera suyo, así que un agente no puede deshacer lo de otro.
        let client = crate::agent::clients::authenticate(&connection, token)
            .map_err(|error| error.message.clone())?;
        // Se admite el identificador de la acción además del testigo: repetir
        // sesenta y cuatro caracteres al azar es justo lo que un modelo hace
        // mal, y el identificador ya viaja en la auditoría.
        let undo_token = match arguments.get("undoToken").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.to_owned(),
            _ => {
                let audit_id = arguments
                    .get("auditId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "Falta «undoToken» o «auditId».".to_owned())?;
                crate::agent::audit::undo_token_for_client(&connection, audit_id, &client.id)
                    .map_err(|error| error.message.clone())?
            }
        };
        let outcome = bridge::undo(&mut connection, &undo_token, &Requester::Client(client.id))
            .map_err(|error| error.message.clone())?;
        return Ok(tool_result(&outcome_to_json(&outcome)));
    }

    if !tools::TOOLS.iter().any(|tool| tool.name == name) {
        return Err(format!("No existe la herramienta «{name}»."));
    }

    // El nombre de la herramienta es el de la intención: basta con etiquetarla.
    let mut payload = arguments;
    let Some(object) = payload.as_object_mut() else {
        return Err("Los argumentos tienen que ser un objeto.".to_owned());
    };
    object.insert("intent".to_owned(), Value::String(name.to_owned()));

    let intent = serde_json::from_value(payload)
        .map_err(|error| format!("Los argumentos no encajan con la herramienta: {error}"))?;
    let request = AgentRequest {
        token: token.to_owned(),
        utterance: String::new(),
        intent,
    };
    let outcome = bridge::dispatch(&mut connection, limiter, &request)
        .map_err(|error| error.message.clone())?;
    Ok(tool_result(&outcome_to_json(&outcome)))
}

/// Traduce el resultado del puente a algo que un modelo pueda leer y encadenar.
fn outcome_to_json(outcome: &bridge::AgentOutcome) -> Value {
    serde_json::to_value(outcome).unwrap_or_else(|_| json!({ "outcome": "desconocido" }))
}

fn tool_result(value: &Value) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": false
    })
}

fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_catalogo_no_tiene_nombres_repetidos() {
        // Dos herramientas con el mismo nombre dejarían al modelo eligiendo a
        // ciegas cuál de las dos llama.
        let mut nombres: Vec<&str> = tools::TOOLS.iter().map(|tool| tool.name).collect();
        nombres.push(tools::UNDO_TOOL.name);
        let total = nombres.len();
        nombres.sort_unstable();
        nombres.dedup();
        assert_eq!(nombres.len(), total);
    }

    #[test]
    fn cada_herramienta_declara_un_esquema_de_objeto() {
        for tool in tools::TOOLS.iter().chain(std::iter::once(&tools::UNDO_TOOL)) {
            let schema = (tool.schema)();
            assert_eq!(
                schema.get("type").and_then(Value::as_str),
                Some("object"),
                "{} no declara un objeto",
                tool.name
            );
            assert!(
                !tool.description.is_empty(),
                "{} no explica qué hace",
                tool.name
            );
        }
    }

    #[test]
    fn los_nombres_son_los_de_las_intenciones() {
        // Si dejaran de coincidir, `tools/call` etiquetaría una intención que no
        // existe y todo fallaría en el borde más tonto posible.
        use crate::agent::intent::AgentIntent;
        for tool in tools::TOOLS {
            let payload = json!({ "intent": tool.name });
            let error = serde_json::from_value::<AgentIntent>(payload).unwrap_err();
            let mensaje = error.to_string();
            assert!(
                !mensaje.contains("unknown variant"),
                "«{}» no es una intención del puente: {mensaje}",
                tool.name
            );
        }
    }

    /// Construye un valor plausible para un trozo de esquema.
    ///
    /// No pretende cubrir JSON Schema entero: cubre lo que estos esquemas usan,
    /// que es lo justo para poder rellenar un ejemplo válido de cada
    /// herramienta.
    fn ejemplo(schema: &Value) -> Value {
        match schema.get("type").and_then(Value::as_str) {
            Some("string") => schema
                .get("enum")
                .and_then(Value::as_array)
                .and_then(|valores| valores.first().cloned())
                .unwrap_or_else(|| json!("x")),
            Some("integer") => schema.get("minimum").cloned().unwrap_or_else(|| json!(1)),
            Some("boolean") => json!(true),
            Some("array") => schema
                .get("items")
                .map(|items| json!([ejemplo(items)]))
                .unwrap_or_else(|| json!([])),
            Some("object") => {
                let mut objeto = serde_json::Map::new();
                let requeridos: Vec<String> = schema
                    .get("required")
                    .and_then(Value::as_array)
                    .map(|valores| {
                        valores
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if let Some(propiedades) = schema.get("properties").and_then(Value::as_object) {
                    for nombre in &requeridos {
                        if let Some(propiedad) = propiedades.get(nombre) {
                            objeto.insert(nombre.clone(), ejemplo(propiedad));
                        }
                    }
                    // Los selectores no declaran «required» porque admiten dos
                    // vías excluyentes; se rellena la inequívoca.
                    if requeridos.is_empty() && propiedades.contains_key("appId") {
                        objeto.insert("appId".to_owned(), json!(1));
                    }
                    if requeridos.is_empty() && propiedades.contains_key("id") {
                        objeto.insert("id".to_owned(), json!("x"));
                    }
                }
                Value::Object(objeto)
            }
            _ => Value::Null,
        }
    }

    #[test]
    fn el_esquema_de_cada_herramienta_produce_una_intencion_valida() {
        // Esta prueba existe por un fallo concreto: el esquema de «consultar»
        // llamaba «query» a un campo que el puente lee como «kind», así que el
        // agente construía una llamada impecable según el esquema y el puente
        // la rechazaba. Desde fuera parecía que el servidor estaba caído.
        use crate::agent::intent::AgentIntent;
        for tool in tools::TOOLS {
            let mut payload = ejemplo(&(tool.schema)());
            payload
                .as_object_mut()
                .expect("el esquema es un objeto")
                .insert("intent".to_owned(), json!(tool.name));
            serde_json::from_value::<AgentIntent>(payload.clone()).unwrap_or_else(|error| {
                panic!("«{}» no encaja con el puente: {error}\n{payload:#}", tool.name)
            });
        }
    }

    #[test]
    fn una_notificacion_no_lleva_respuesta() {
        let database = Database::new(PathBuf::from("/dev/null"));
        let limiter = RateLimiter::default();
        let respuesta = handle_line(
            &database,
            &limiter,
            None,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        assert!(respuesta.is_none());
    }

    /// Base real con migraciones, un juego y un cliente agente con todos los
    /// ámbitos. Es lo mínimo para comprobar que una llamada MCP llega hasta el
    /// puente y vuelve con un resultado de verdad.
    fn base_con_agente() -> (tempfile::TempDir, Database, String) {
        use crate::agent::clients::{self, NewAgentClient};
        use crate::agent::scope::ALL_SCOPES;
        use crate::agent::token::TokenPolicy;

        let directory = tempfile::tempdir().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar");
        let token = {
            let mut connection = database.open().expect("abrir");
            connection
                .execute(
                    "INSERT INTO games(app_id, title) VALUES (4242, 'DragonsWord Awakening')",
                    [],
                )
                .expect("insertar juego");
            connection
                .execute(
                    "INSERT INTO game_personal(app_id, status_id) VALUES (4242, 'unclassified')",
                    [],
                )
                .expect("insertar ficha");
            let emitido = clients::issue(
                &mut connection,
                &NewAgentClient {
                    name: "Hermes".into(),
                    kind: "hermes".into(),
                    scopes: ALL_SCOPES.iter().map(|scope| scope.as_str().to_owned()).collect(),
                },
                TokenPolicy::default(),
            )
            .expect("emitir testigo");
            emitido.token
        };
        (directory, database, token)
    }

    #[test]
    fn una_sesion_dictada_llega_hasta_la_biblioteca() {
        // Es el caso que motiva todo esto: «he estado dos horas con X y voy por
        // el 40 %» tiene que acabar en una fila de la base, no en un resumen.
        let (_directory, database, token) = base_con_agente();
        let limiter = RateLimiter::default();

        let respuesta = handle_line(
            &database,
            &limiter,
            Some(token.as_str()),
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"registrar_sesion","arguments":{"game":{"name":"DragonsWord Awakening"},"minutes":120,"progress":40}}}"#,
        )
        .expect("tools/call contesta");

        let texto = respuesta["result"]["content"][0]["text"]
            .as_str()
            .expect("resultado de texto");
        assert_eq!(respuesta["result"]["isError"], serde_json::json!(false), "{texto}");
        assert!(texto.contains("applied"), "{texto}");

        let connection = database.open().expect("abrir");
        // La sesión queda escrita con su progreso: eso es lo que se dictó.
        let (sesiones, progreso): (i64, Option<i64>) = connection
            .query_row(
                "SELECT COUNT(*), MAX(progress_after) FROM game_sessions WHERE app_id = 4242",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("leer sesiones");
        assert_eq!(sesiones, 1);
        assert_eq!(progreso, Some(40));
        let personal: i64 = connection
            .query_row(
                "SELECT progress FROM game_personal WHERE app_id = 4242",
                [],
                |row| row.get(0),
            )
            .expect("leer ficha");
        assert_eq!(personal, 40, "el progreso dictado llega a la ficha");
    }

    #[test]
    fn se_puede_deshacer_por_identificador_sin_repetir_el_testigo() {
        // Medido contra un modelo local: al repetir el testigo de sesenta y
        // cuatro caracteres se dejó ocho por el camino y el deshacer falló por
        // un motivo que no tenía nada que ver con lo que se le pedía. El
        // identificador de la acción es un UUID y ya viaja en la auditoría.
        let (_directory, database, token) = base_con_agente();
        let limiter = RateLimiter::default();

        let aplicado = handle_line(
            &database,
            &limiter,
            Some(token.as_str()),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"registrar_sesion","arguments":{"game":{"appId":4242},"minutes":30}}}"#,
        )
        .expect("registra");
        let texto = aplicado["result"]["content"][0]["text"].as_str().expect("texto");
        let cuerpo: Value = serde_json::from_str(texto).expect("json");
        let audit_id = cuerpo["auditId"].as_str().expect("identificador").to_owned();

        let deshecho = handle_line(
            &database,
            &limiter,
            Some(token.as_str()),
            &format!(
                r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"deshacer","arguments":{{"auditId":"{audit_id}"}}}}}}"#
            ),
        )
        .expect("deshace");
        let detalle = deshecho["result"]["content"][0]["text"].as_str().unwrap_or_default();
        assert_eq!(deshecho["result"]["isError"], json!(false), "{detalle}");

        let connection = database.open().expect("abrir");
        let sesiones: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM game_sessions WHERE app_id = 4242",
                [],
                |row| row.get(0),
            )
            .expect("contar");
        assert_eq!(sesiones, 0, "la sesión deshecha no puede seguir ahí");
    }

    #[test]
    fn sin_testigo_no_se_toca_nada() {
        let (_directory, database, _token) = base_con_agente();
        let limiter = RateLimiter::default();
        let respuesta = handle_line(
            &database,
            &limiter,
            None,
            r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"cambiar_estado","arguments":{"game":{"appId":4242},"statusId":"playing"}}}"#,
        )
        .expect("contesta");
        assert_eq!(respuesta["result"]["isError"], serde_json::json!(true));
    }

    #[test]
    fn initialize_anuncia_protocolo_y_herramientas() {
        let database = Database::new(PathBuf::from("/dev/null"));
        let limiter = RateLimiter::default();
        let respuesta = handle_line(
            &database,
            &limiter,
            None,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .expect("initialize contesta");
        assert_eq!(
            respuesta["result"]["protocolVersion"].as_str(),
            Some(PROTOCOL_VERSION)
        );

        let listado = handle_line(
            &database,
            &limiter,
            None,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
        )
        .expect("tools/list contesta");
        let herramientas = listado["result"]["tools"]
            .as_array()
            .expect("lista de herramientas");
        assert_eq!(herramientas.len(), tools::TOOLS.len() + 1);
    }
}
