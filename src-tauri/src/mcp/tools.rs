//! Catálogo de herramientas que ve el agente.
//!
//! Cada herramienta es una intención del puente con un esquema JSON que el
//! modelo lee para saber cómo llamarla. Las descripciones están escritas para
//! que las lea una máquina que tiene que decidir: dicen qué hace la herramienta,
//! qué **no** hace y cuándo conviene otra.
//!
//! El nombre de cada herramienta es el de su intención, así que traducir una
//! llamada MCP a una petición del puente es añadir `"intent": "<nombre>"` a los
//! argumentos. No hay una segunda tabla que mantener sincronizada.

use serde_json::{Value, json};

/// Una herramienta tal y como la anuncia MCP.
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// Esquema de los argumentos, ya en JSON Schema.
    pub schema: fn() -> Value,
}

/// Selector de juego: por AppID o por nombre, nunca por los dos.
fn game_selector() -> Value {
    json!({
        "type": "object",
        "description": "El juego. Manda «appId» si lo sabes, «name» si no, o los dos: con los dos manda el AppID y el nombre sirve para confirmar que hablan del mismo juego. Si el nombre es ambiguo, la respuesta trae las opciones para que preguntes cuál.",
        "properties": {
            "appId": { "type": "integer", "minimum": 1 },
            "name": { "type": "string", "maxLength": 200 }
        },
        "additionalProperties": false
    })
}

fn entity_selector(what: &'static str) -> Value {
    json!({
        "type": "object",
        "description": format!("{what}. Por «id» o por nombre exacto, no por los dos."),
        "properties": {
            "id": { "type": "string", "maxLength": 128 },
            "name": { "type": "string", "maxLength": 80 }
        },
        "additionalProperties": false
    })
}

fn games_array() -> Value {
    json!({
        "type": "array",
        "description": "Juegos a los que afecta. Más de cinco exige que una persona lo confirme dentro de Vindexa.",
        "items": game_selector(),
        "maxItems": 200
    })
}

pub const TOOLS: &[Tool] = &[
    Tool {
        name: "consultar",
        description: "Lee la biblioteca sin cambiar nada. Úsala siempre antes de modificar algo: para saber qué estados existen, cómo se llama exactamente un juego o qué colecciones hay.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "object",
                        "description": "Qué se consulta. El tipo va en «kind».",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": ["biblioteca", "juego", "estados", "colecciones", "listas", "planificador", "avisos", "sesiones", "auditoria"],
                                "description": "biblioteca: busca por texto. juego: ficha completa. estados/colecciones/listas/planificador/avisos: catálogos. sesiones: últimas partidas de un juego. auditoria: lo que tú mismo has hecho."
                            },
                            "text": { "type": "string", "description": "Texto de búsqueda para «biblioteca»." },
                            "statusId": { "type": "string" },
                            "limit": { "type": "integer", "minimum": 1, "maximum": 200 },
                            "game": game_selector()
                        },
                        "required": ["kind"]
                    }
                },
                "required": ["query"]
            })
        },
    },
    Tool {
        name: "registrar_sesion",
        description: "Apunta una partida: cuánto se ha jugado y, si se sabe, por qué porcentaje de la historia va. Es lo que hay que llamar ante «he estado dos horas con X y voy por el 40 %».",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "game": game_selector(),
                    "minutes": { "type": "integer", "minimum": 1, "maximum": 1440, "description": "Duración en minutos. Dos horas son 120." },
                    "startedAt": { "type": "string", "description": "Cuándo empezó, en ISO 8601. Si no se dice, se toma ahora." },
                    "progress": { "type": "integer", "minimum": 0, "maximum": 100, "description": "Porcentaje de la historia. Omítelo si no lo ha dicho: no lo inventes." },
                    "note": { "type": "string", "maxLength": 2000 }
                },
                "required": ["game", "minutes"]
            })
        },
    },
    Tool {
        name: "marcar_terminado",
        description: "Marca un juego como terminado. Con «keepPlayable» en true conserva su estado actual —para quien lo ha acabado pero va a seguir jugando— y entonces suele acompañarse de bajarle la prioridad.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "game": game_selector(),
                    "completedOn": { "type": "string", "description": "Fecha en la que se terminó (AAAA-MM-DD)." },
                    "keepPlayable": { "type": "boolean", "description": "true conserva el estado actual; false lo mueve a «Completado»." },
                    "priority": { "type": "integer", "minimum": 0, "maximum": 5, "description": "Prioridad nueva. 0 es la más baja." }
                },
                "required": ["game"]
            })
        },
    },
    Tool {
        name: "cambiar_estado",
        description: "Mueve un juego a otro estado. Consulta antes «estados» para usar un identificador que exista: no te lo inventes.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "game": game_selector(), "statusId": { "type": "string" } },
                "required": ["game", "statusId"]
            })
        },
    },
    Tool {
        name: "ajustar_prioridad",
        description: "Cambia la prioridad de un juego. «priority» fija un valor de 0 a 5; «delta» lo mueve. Exactamente uno de los dos.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "game": game_selector(),
                    "priority": { "type": "integer", "minimum": 0, "maximum": 5 },
                    "delta": { "type": "integer", "minimum": -5, "maximum": 5 }
                },
                "required": ["game"]
            })
        },
    },
    Tool {
        name: "valorar",
        description: "Pone nota a un juego, de 1 a 10. Sin «rating» borra la valoración.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "game": game_selector(), "rating": { "type": "integer", "minimum": 1, "maximum": 10 } },
                "required": ["game"]
            })
        },
    },
    Tool {
        name: "anotar",
        description: "Escribe una nota personal en la ficha de un juego. Con «append» se añade al final de lo que ya había en vez de sustituirlo.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "game": game_selector(),
                    "note": { "type": "string", "maxLength": 4000 },
                    "append": { "type": "boolean" }
                },
                "required": ["game", "note"]
            })
        },
    },
    Tool {
        name: "fijar_proxima_accion",
        description: "Deja apuntado por dónde seguir la próxima vez. Sin «action» se borra lo anterior.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "game": game_selector(), "action": { "type": "string", "maxLength": 280 } },
                "required": ["game"]
            })
        },
    },
    Tool {
        name: "fijar_checkpoint",
        description: "Anota el punto exacto de la partida: capítulo, zona o misión. Sin «checkpoint» se borra.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "game": game_selector(), "checkpoint": { "type": "string", "maxLength": 280 } },
                "required": ["game"]
            })
        },
    },
    Tool {
        name: "fijar",
        description: "Fija o deja de fijar un juego arriba del todo.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "game": game_selector(), "pinned": { "type": "boolean" } },
                "required": ["game", "pinned"]
            })
        },
    },
    Tool {
        name: "seguir",
        description: "Activa o desactiva el seguimiento de un juego, que es lo que hace que aparezca en la pantalla de Seguimiento.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "game": game_selector(), "tracking": { "type": "boolean" } },
                "required": ["game", "tracking"]
            })
        },
    },
    Tool {
        name: "crear_coleccion",
        description: "Crea una colección manual, opcionalmente con juegos dentro. No crea colecciones inteligentes: sus reglas las decide una persona en Vindexa.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "maxLength": 80 },
                    "description": { "type": "string", "maxLength": 1000 },
                    "color": { "type": "string", "description": "Color en hexadecimal, por ejemplo #5CAAC1." },
                    "icon": { "type": "string", "description": "Nombre del icono: folder, star, heart, trophy, rocket, sword…" },
                    "games": games_array()
                },
                "required": ["name"]
            })
        },
    },
    Tool {
        name: "añadir_a_coleccion",
        description: "Mete juegos en una colección manual que ya existe.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "collection": entity_selector("La colección"), "games": games_array() },
                "required": ["collection", "games"]
            })
        },
    },
    Tool {
        name: "quitar_de_coleccion",
        description: "Saca juegos de una colección. No borra el juego de la biblioteca.",
        schema: || {
            json!({
                "type": "object",
                "properties": { "collection": entity_selector("La colección"), "games": games_array() },
                "required": ["collection", "games"]
            })
        },
    },
    Tool {
        name: "crear_lista_curada",
        description: "Crea una lista curada en Deseados.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "maxLength": 80 },
                    "description": { "type": "string", "maxLength": 1000 },
                    "accent": { "type": "string" },
                    "icon": { "type": "string" },
                    "pinned": { "type": "boolean" }
                },
                "required": ["name"]
            })
        },
    },
    Tool {
        name: "añadir_a_lista",
        description: "Añade juegos a una lista curada de Deseados.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "list": entity_selector("La lista"),
                    "games": games_array(),
                    "note": { "type": "string", "maxLength": 280 },
                    "highlight": { "type": "boolean" }
                },
                "required": ["list", "games"]
            })
        },
    },
    Tool {
        name: "planificar",
        description: "Coloca un juego en una columna del planificador. Consulta antes «planificador» para saber qué columnas hay.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "game": game_selector(),
                    "columnId": { "type": "string" },
                    "targetDate": { "type": "string", "description": "Fecha objetivo (AAAA-MM-DD)." },
                    "estimatedMinutes": { "type": "integer", "minimum": 1 }
                },
                "required": ["game", "columnId"]
            })
        },
    },
    Tool {
        name: "programar_aviso",
        description: "Programa un recordatorio sobre un juego para una fecha y hora concretas.",
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "game": game_selector(),
                    "dueAt": { "type": "string", "description": "Cuándo avisar, en ISO 8601." },
                    "note": { "type": "string", "maxLength": 280 }
                },
                "required": ["game", "dueAt"]
            })
        },
    },
];

/// Herramienta aparte: no es una intención, sino el reverso de todas.
pub const UNDO_TOOL: Tool = Tool {
    name: "deshacer",
    description: "Deshace un cambio que tú mismo hiciste. Pasa el «auditId» que devolvió esa llamada —o el que aparece en «consultar» con kind=auditoria—; el «undoToken» también vale, pero el identificador es más corto y más difícil de copiar mal. Sólo puedes deshacer lo tuyo.",
    schema: || {
        json!({
            "type": "object",
            "properties": {
                "auditId": { "type": "string", "description": "Identificador de la acción a deshacer. Es lo más cómodo." },
                "undoToken": { "type": "string", "description": "Alternativa: el token que devolvió la llamada." }
            }
        })
    },
};
