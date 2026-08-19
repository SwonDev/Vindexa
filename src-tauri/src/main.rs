// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `vindexa mcp` arranca el servidor MCP en vez de la ventana: es la puerta
    // por la que un agente local —Hermes, Claude, el que sea— conduce Vindexa
    // hablando, sin abrir ningún puerto. Va aquí y no en un binario aparte para
    // que el empaquetado lo lleve solo y no pueda quedarse desfasado respecto a
    // la aplicación con la que comparte base de datos.
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    match argumentos.first().map(String::as_str) {
        Some("mcp") => std::process::exit(vindexa_lib::run_mcp()),
        // `vindexa connect-agent hermes [ámbito…]` deja la biblioteca conectada
        // sin abrir la ventana. Sin ámbitos se conceden todos, que es lo que
        // hace falta para conducirla entera hablando.
        Some("connect-agent") => {
            let Some(host) = argumentos.get(1) else {
                eprintln!("Uso: vindexa connect-agent <hermes|claude> [ámbito…]");
                std::process::exit(2);
            };
            std::process::exit(vindexa_lib::run_connect_agent(host, &argumentos[2..]));
        }
        _ => vindexa_lib::run(),
    }
}
