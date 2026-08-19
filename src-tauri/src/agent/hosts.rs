//! Agentes que viven en este ordenador y saben hablar MCP.
//!
//! # Qué resuelve
//!
//! Vindexa ya expone sus herramientas por MCP ([`crate::mcp`]). Falta la parte
//! aburrida: que quien usa la aplicación no tenga que copiar a mano un comando
//! largo con un testigo dentro. Aquí se busca el agente donde suele estar, se
//! arma su comando de alta y se ejecuta cuando la persona lo pide.
//!
//! # Qué no hace
//!
//! - **No busca por todo el disco.** Mira los sitios donde estos programas se
//!   instalan y el `PATH`. Rastrear el disco entero por si acaso sería lento y
//!   además desproporcionado.
//! - **No conecta nada solo.** Detectar es leer; dar de alta es un botón que
//!   pulsa una persona, y el comando exacto se enseña antes de ejecutarlo.
//! - **No inventa el testigo.** Lo emite el puente con los ámbitos que se hayan
//!   marcado, y se puede revocar desde la misma pantalla.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Un agente compatible que puede estar instalado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSpec {
    /// Identificador estable que viaja a la interfaz.
    pub id: &'static str,
    /// Nombre con el que se le conoce.
    pub label: &'static str,
    /// Ejecutable que hay que encontrar.
    pub binary: &'static str,
    /// Rutas donde suele instalarse, relativas al hogar.
    pub home_paths: &'static [&'static str],
    /// Qué se le manda para dar de alta un servidor MCP por tubería.
    pub add_args: &'static [&'static str],
}

/// Agentes soportados, en el orden en que se ofrecen.
pub const HOSTS: &[HostSpec] = &[
    HostSpec {
        id: "hermes",
        label: "Hermes",
        binary: "hermes",
        home_paths: &[".local/bin/hermes", ".hermes/bin/hermes"],
        add_args: &["mcp", "add"],
    },
    HostSpec {
        id: "claude",
        label: "Claude Code",
        binary: "claude",
        home_paths: &[".local/bin/claude", ".claude/local/claude"],
        add_args: &["mcp", "add"],
    },
];

/// Estado de un agente en este ordenador.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHost {
    pub id: String,
    pub label: String,
    /// Ruta del ejecutable encontrado. `None` significa que no está instalado.
    pub path: Option<String>,
    /// Comando que se ejecutaría, ya montado y **sin el testigo dentro**.
    ///
    /// Se enseña antes de pulsar nada: conectar un agente a la biblioteca es una
    /// decisión, y una decisión se toma viendo lo que va a pasar.
    pub command_preview: String,
}

/// Apaga la detección por completo.
///
/// Existe para las pruebas: una batería que corre en el ordenador de alguien no
/// puede acabar dando de alta servidores en su agente de verdad. Con esto,
/// `detect` contesta que no hay ninguno y el automatismo no tiene nada que
/// hacer.
const DISABLE_ENV: &str = "VINDEXA_SIN_AGENTES";

fn detection_disabled() -> bool {
    std::env::var_os(DISABLE_ENV).is_some()
}

/// Busca un ejecutable en las rutas habituales y en el `PATH`.
fn find_binary(spec: &HostSpec) -> Option<PathBuf> {
    if detection_disabled() {
        return None;
    }
    if let Some(home) = std::env::var_os("HOME") {
        for relative in spec.home_paths {
            let candidate = PathBuf::from(&home).join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(spec.binary))
        .find(|candidate| candidate.is_file())
}

/// Ruta del propio ejecutable de Vindexa, que es lo que el agente tendrá que
/// lanzar. Se resuelve en tiempo de ejecución para que valga igual dentro del
/// paquete instalado que durante el desarrollo.
pub(crate) fn vindexa_command() -> AppResult<String> {
    let path = std::env::current_exe().map_err(|error| {
        AppError::new(
            "agent_host",
            format!("No se pudo localizar el ejecutable de Vindexa: {error}"),
        )
    })?;
    Ok(path.to_string_lossy().into_owned())
}

/// Qué agentes hay, con el comando que se usaría para conectarlos.
pub fn detect() -> AppResult<Vec<AgentHost>> {
    let vindexa = vindexa_command()?;
    Ok(HOSTS
        .iter()
        .map(|spec| {
            let path = find_binary(spec);
            let binario = path
                .as_ref()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| spec.binary.to_owned());
            AgentHost {
                id: spec.id.to_owned(),
                label: spec.label.to_owned(),
                path: path.map(|value| value.to_string_lossy().into_owned()),
                command_preview: format!(
                    "{binario} {} vindexa --command {vindexa} --args mcp --env VINDEXA_AGENT_TOKEN=<testigo>",
                    spec.add_args.join(" ")
                ),
            }
        })
        .collect())
}

/// Da de alta Vindexa como servidor MCP del agente indicado.
///
/// El testigo viaja como variable de entorno del proceso hijo y acaba guardado
/// en la configuración de ese agente, que es donde vive el resto de sus
/// credenciales. No se escribe en ningún registro de Vindexa ni se devuelve.
pub fn connect(host_id: &str, token: &str) -> AppResult<String> {
    let spec = HOSTS
        .iter()
        .find(|spec| spec.id == host_id)
        .ok_or_else(|| AppError::validation("Ese agente no está soportado."))?;
    let binary = find_binary(spec).ok_or_else(|| {
        AppError::new(
            "agent_host_missing",
            format!("No se encontró {} en este ordenador.", spec.label),
        )
    })?;
    let vindexa = vindexa_command()?;

    // `--args` va al final por exigencia del propio agente: todo lo que venga
    // después lo toma como argumento del proceso hijo.
    let mut child = Command::new(&binary)
        .args(spec.add_args)
        .arg("vindexa")
        .arg("--command")
        .arg(&vindexa)
        .arg("--env")
        .arg(format!("VINDEXA_AGENT_TOKEN={token}"))
        .arg("--args")
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AppError::new(
                "agent_host",
                format!("No se pudo ejecutar {}: {error}", spec.label),
            )
        })?;

    // Tras descubrir las herramientas, el agente pregunta si las habilita
    // todas. Sin nadie al otro lado la pregunta se cancela y el alta se pierde
    // en silencio —así fallaba la primera vez—. Se contesta que sí: las
    // herramientas son las que Vindexa acaba de ofrecer, y lo que cada una
    // puede tocar ya lo limita el testigo, no esta respuesta.
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"y\n");
    }
    let output = child.wait_with_output().map_err(|error| {
        AppError::new(
            "agent_host",
            format!("{} no terminó el alta: {error}", spec.label),
        )
    })?;

    if !output.status.success() {
        // El error del agente se devuelve tal cual, recortado: es lo único que
        // explica por qué no ha entrado.
        let detalle = String::from_utf8_lossy(&output.stderr);
        let detalle = detalle.trim();
        let detalle = if detalle.is_empty() {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        } else {
            detalle.to_owned()
        };
        return Err(AppError::new(
            "agent_host",
            format!(
                "{} rechazó el alta: {}",
                spec.label,
                detalle.chars().take(400).collect::<String>()
            ),
        ));
    }
    // Salir con cero no basta: la primera versión de esto terminaba «bien»
    // dejando el alta a medias porque una pregunta interactiva se cancelaba
    // sola. Lo que cuenta es que el servidor aparezca en la lista del agente.
    if !is_registered(&binary, spec) {
        return Err(AppError::new(
            "agent_host",
            format!(
                "{} aceptó el alta pero Vindexa no aparece en su lista de servidores.",
                spec.label
            ),
        ));
    }

    Ok(format!(
        "Vindexa quedó registrado en {}. Ya puedes pedirle cosas por cualquiera de sus canales.",
        spec.label
    ))
}

/// ¿Aparece Vindexa entre los servidores del agente?
fn is_registered(binary: &PathBuf, spec: &HostSpec) -> bool {
    let Ok(output) = Command::new(binary)
        .arg(spec.add_args[0])
        .arg("list")
        .stdin(Stdio::null())
        .output()
    else {
        return false;
    };
    let texto = String::from_utf8_lossy(&output.stdout).to_lowercase();
    texto.contains("vindexa")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_comando_de_muestra_no_lleva_el_testigo() {
        // Se enseña antes de conectar, así que no puede filtrar el secreto: el
        // testigo sólo existe en el proceso hijo.
        let hosts = detect().expect("detectar");
        assert!(!hosts.is_empty());
        for host in &hosts {
            assert!(host.command_preview.contains("<testigo>"), "{host:?}");
            assert!(host.command_preview.contains("--args mcp"), "{host:?}");
        }
    }

    #[test]
    fn un_agente_desconocido_se_rechaza() {
        let error = connect("agente-inventado", "x").expect_err("rechazar");
        assert_eq!(error.code, "validation");
    }
}
