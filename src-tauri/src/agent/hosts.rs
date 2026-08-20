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
    /// Cómo se le pide dar de alta un servidor MCP por tubería.
    ///
    /// **No hay una sintaxis común.** Suponer que la había dejó a Claude Code
    /// sin conectar con «unknown option '--command'» en cada arranque, porque
    /// se le mandaban los argumentos de Hermes.
    pub dialect: AddDialect,
}

/// Cómo espera cada agente que se le describa un servidor por tubería.
///
/// Comprobado con `--help` de cada uno el 19 de agosto de 2026:
///
/// ```text
/// hermes mcp add <nombre> --command <cmd> --env K=V --args <args…>
/// claude mcp add <nombre> -e K=V -- <cmd> <args…>
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddDialect {
    /// Opciones nombradas; `--args` tiene que ir al final.
    Hermes,
    /// Entorno con `-e` y el comando detrás de `--`.
    ClaudeCode,
}

/// Agentes soportados, en el orden en que se ofrecen.
pub const HOSTS: &[HostSpec] = &[
    HostSpec {
        id: "hermes",
        label: "Hermes",
        binary: "hermes",
        home_paths: &[".local/bin/hermes", ".hermes/bin/hermes"],
        dialect: AddDialect::Hermes,
    },
    HostSpec {
        id: "claude",
        label: "Claude Code",
        binary: "claude",
        home_paths: &[".local/bin/claude", ".claude/local/claude"],
        dialect: AddDialect::ClaudeCode,
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
                    "{binario} {}",
                    add_arguments(spec.dialect, &vindexa, "<testigo>").join(" ")
                ),
            }
        })
        .collect())
}

/// Argumentos del alta, en el dialecto de cada agente.
///
/// Vive en una sola función para que la vista previa que se enseña antes de
/// pulsar y lo que se ejecuta después sean literalmente lo mismo: una vista
/// previa que no coincide con el comando real es peor que no enseñarla.
pub(crate) fn add_arguments(dialect: AddDialect, vindexa: &str, token: &str) -> Vec<String> {
    let entorno = format!("VINDEXA_AGENT_TOKEN={token}");
    match dialect {
        // `--args` tiene que ir al final: todo lo que venga después lo toma como
        // argumento del proceso hijo.
        AddDialect::Hermes => vec![
            "mcp".into(),
            "add".into(),
            "vindexa".into(),
            "--command".into(),
            vindexa.into(),
            "--env".into(),
            entorno,
            "--args".into(),
            "mcp".into(),
        ],
        // El comando va detrás de `--`, y el entorno con `-e` antes.
        AddDialect::ClaudeCode => vec![
            "mcp".into(),
            "add".into(),
            "vindexa".into(),
            "-e".into(),
            entorno,
            "--".into(),
            vindexa.into(),
            "mcp".into(),
        ],
    }
}

/// Argumentos para retirar un alta anterior.
///
/// Los dos agentes usan `mcp remove <nombre>`; lo que cambia es el alta, no la
/// baja.
pub(crate) fn remove_arguments() -> Vec<String> {
    vec!["mcp".into(), "remove".into(), "vindexa".into()]
}

/// Perfiles del agente en los que hay que dar de alta.
///
/// Hermes puede tener varios perfiles aislados, cada uno con su configuración y
/// su propio registro de servidores MCP. El bot de alguien puede correr en uno
/// que no es el de por defecto —«vindexabot», por ejemplo— y dar de alta sólo
/// en el activo deja a ese bot con el testigo viejo: Vindexa cree que entregó
/// uno nuevo, revoca el anterior, y el bot contesta que el suyo ha caducado.
/// Pasó exactamente así, y se descubrió veinte horas después.
///
/// La lista sale de `profile list`, que imprime una tabla: se lee la primera
/// palabra de cada fila, saltando la cabecera y las líneas de separación. Si el
/// agente no entiende de perfiles, se devuelve la lista vacía y el alta se hace
/// una sola vez, como siempre.
fn profiles(binary: &PathBuf, spec: &HostSpec) -> Vec<String> {
    if spec.dialect != AddDialect::Hermes {
        return Vec::new();
    }
    let Ok(output) = Command::new(binary)
        .arg("profile")
        .arg("list")
        .stdin(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|linea| {
            // El activo lleva un rombo delante; la cabecera y los separadores,
            // guiones de dibujo.
            let limpia = linea.trim().trim_start_matches('◆').trim();
            let primera = limpia.split_whitespace().next()?;
            let valido = primera
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            (valido && primera != "Profile").then(|| primera.to_owned())
        })
        .collect()
}

/// Los mismos argumentos, dirigidos a un perfil concreto.
fn con_perfil(perfil: Option<&str>, resto: Vec<String>) -> Vec<String> {
    match perfil {
        Some(nombre) => {
            let mut args = vec!["-p".to_owned(), nombre.to_owned()];
            args.extend(resto);
            args
        }
        None => resto,
    }
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

    // Un alta por perfil. Sin perfiles, una sola sin `-p`, que es lo que había.
    let perfiles = profiles(&binary, spec);
    let destinos: Vec<Option<String>> = if perfiles.is_empty() {
        vec![None]
    } else {
        perfiles.into_iter().map(Some).collect()
    };

    let mut hechos: Vec<String> = Vec::new();
    let mut fallos: Vec<String> = Vec::new();

    for destino in &destinos {
        let perfil = destino.as_deref();
        match alta_en(&binary, spec, perfil, &vindexa, token) {
            Ok(()) => hechos.push(perfil.unwrap_or("por defecto").to_owned()),
            Err(error) => fallos.push(format!(
                "{}: {}",
                perfil.unwrap_or("por defecto"),
                error.message
            )),
        }
    }

    // Que uno de los perfiles se resista no puede pasar por un alta correcta:
    // el testigo anterior ya se ha revocado y ese perfil se ha quedado sin
    // enlace. Se dice cuál.
    if hechos.is_empty() || !fallos.is_empty() {
        return Err(AppError::new(
            "agent_host",
            format!(
                "{} no completó el alta en {}: {}",
                spec.label,
                if fallos.len() == 1 {
                    "un perfil"
                } else {
                    "varios perfiles"
                },
                fallos.join(" · ").chars().take(400).collect::<String>()
            ),
        ));
    }

    Ok(format!(
        "Vindexa quedó registrado en {} ({}). Ya puedes pedirle cosas por cualquiera de sus canales.",
        spec.label,
        if destinos.len() == 1 && destinos[0].is_none() {
            "perfil único".to_owned()
        } else {
            format!("perfiles: {})", hechos.join(", ")).replace("))", ")")
        }
    ))
}

/// Da de alta Vindexa en un destino concreto: un perfil, o el agente entero si
/// no tiene perfiles.
fn alta_en(
    binary: &PathBuf,
    spec: &HostSpec,
    perfil: Option<&str>,
    vindexa: &str,
    token: &str,
) -> AppResult<()> {
    // Primero se retira el alta anterior, si la hay.
    //
    // `mcp add` no sobrescribe una entrada que ya existe: se va sin escribir y
    // sin fallar. Como el agente seguía teniendo **un** servidor llamado
    // «vindexa» —el de antes, con el testigo viejo—, la comprobación de más
    // abajo lo daba por bueno, Vindexa revocaba el testigo anterior y el enlace
    // se rompía en silencio. Se descubrió veinte horas después, cuando el bot
    // contestó que su testigo había caducado.
    //
    // Retirando primero, lo que aparezca después sólo puede ser el alta nueva.
    // Si no había nada que retirar, el agente se queja y no pasa nada: por eso
    // el resultado se ignora a propósito.
    let _ = Command::new(binary)
        .args(con_perfil(perfil, remove_arguments()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    let mut child = Command::new(binary)
        .args(con_perfil(
            perfil,
            add_arguments(spec.dialect, vindexa, token),
        ))
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
                "rechazó el alta: {}",
                detalle.chars().take(200).collect::<String>()
            ),
        ));
    }
    // Salir con cero no basta: la primera versión de esto terminaba «bien»
    // dejando el alta a medias porque una pregunta interactiva se cancelaba
    // sola. Lo que cuenta es que el servidor aparezca en la lista del agente.
    if !is_registered(binary, perfil) {
        return Err(AppError::new(
            "agent_host",
            "aceptó el alta pero Vindexa no aparece en su lista de servidores.".to_owned(),
        ));
    }
    Ok(())
}

/// ¿Aparece Vindexa entre los servidores del agente?
fn is_registered(binary: &PathBuf, perfil: Option<&str>) -> bool {
    // `mcp list` es común a los dos agentes; lo que cambia es el alta. Y se
    // pregunta al mismo perfil en el que se acaba de dar de alta: preguntarle a
    // otro devuelve la lista de otro.
    let Ok(output) = Command::new(binary)
        .args(con_perfil(
            perfil,
            vec!["mcp".to_owned(), "list".to_owned()],
        ))
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

    /// Los argumentos dirigidos a un perfil llevan `-p` delante de todo.
    ///
    /// Detrás no vale: `--args` de Hermes se traga todo lo que venga después,
    /// así que un `-p` al final acabaría siendo un argumento del proceso hijo.
    #[test]
    fn el_perfil_va_delante_del_resto() {
        let sin = con_perfil(None, remove_arguments());
        assert_eq!(sin, vec!["mcp", "remove", "vindexa"]);

        let con = con_perfil(Some("vindexabot"), remove_arguments());
        assert_eq!(con, vec!["-p", "vindexabot", "mcp", "remove", "vindexa"]);

        let alta = con_perfil(
            Some("vindexabot"),
            add_arguments(AddDialect::Hermes, "/ruta/vindexa", "<testigo>"),
        );
        assert_eq!(&alta[0..2], &["-p", "vindexabot"]);
        assert_eq!(alta.last().map(String::as_str), Some("mcp"));
    }

    /// Rehacer un alta empieza por retirar la que había.
    ///
    /// `mcp add` no sobrescribe una entrada existente: termina con cero sin
    /// escribir nada. Como el agente seguía teniendo un servidor llamado
    /// «vindexa» —el de antes—, la comprobación de efecto lo daba por bueno y
    /// Vindexa revocaba el testigo que el agente sí tenía. El bot lo descubrió
    /// veinte horas más tarde diciendo que su testigo había caducado.
    #[test]
    fn la_baja_usa_el_mismo_nombre_que_el_alta() {
        let baja = remove_arguments();
        assert_eq!(baja, vec!["mcp", "remove", "vindexa"]);

        // El nombre tiene que ser el mismo en las dos, o la baja no encuentra
        // nada que quitar y el alta vuelve a chocar.
        for dialecto in [AddDialect::Hermes, AddDialect::ClaudeCode] {
            let alta = add_arguments(dialecto, "/ruta/vindexa", "<testigo>");
            let nombre_del_alta = alta
                .iter()
                .position(|arg| arg == "add")
                .and_then(|indice| alta.get(indice + 1))
                .expect("el alta nombra el servidor");
            assert_eq!(nombre_del_alta, &baja[2], "dialecto {dialecto:?}");
        }
    }

    #[test]
    fn el_comando_de_muestra_no_lleva_el_testigo() {
        // Se enseña antes de conectar, así que no puede filtrar el secreto: el
        // testigo sólo existe en el proceso hijo.
        let hosts = detect().expect("detectar");
        assert!(!hosts.is_empty());
        for host in &hosts {
            assert!(host.command_preview.contains("<testigo>"), "{host:?}");
            assert!(host.command_preview.contains("mcp add vindexa"), "{host:?}");
        }
    }

    /// Cada agente tiene su sintaxis, y suponer que era la misma dejó a Claude
    /// Code sin conectar en cada arranque con «unknown option '--command'».
    ///
    /// Las dos formas están tomadas del `--help` de cada uno:
    ///
    /// ```text
    /// hermes mcp add <nombre> --command <cmd> --env K=V --args <args…>
    /// claude mcp add <nombre> -e K=V -- <cmd> <args…>
    /// ```
    #[test]
    fn cada_agente_recibe_su_propia_sintaxis() {
        let hermes = add_arguments(AddDialect::Hermes, "/ruta/vindexa", "secreto");
        assert_eq!(
            hermes,
            [
                "mcp",
                "add",
                "vindexa",
                "--command",
                "/ruta/vindexa",
                "--env",
                "VINDEXA_AGENT_TOKEN=secreto",
                "--args",
                "mcp",
            ]
        );
        // `--args` va al final por exigencia del propio Hermes.
        assert_eq!(hermes[hermes.len() - 2], "--args");

        let claude = add_arguments(AddDialect::ClaudeCode, "/ruta/vindexa", "secreto");
        assert_eq!(
            claude,
            [
                "mcp",
                "add",
                "vindexa",
                "-e",
                "VINDEXA_AGENT_TOKEN=secreto",
                "--",
                "/ruta/vindexa",
                "mcp",
            ]
        );
        assert!(
            !claude.iter().any(|arg| arg == "--command"),
            "Claude Code no conoce esa opción: {claude:?}"
        );
    }

    #[test]
    fn la_vista_previa_es_el_comando_de_verdad_con_el_testigo_tapado() {
        // Una vista previa que no coincide con lo que se ejecuta es peor que no
        // enseñarla: se pide permiso para una cosa y se hace otra.
        for spec in HOSTS {
            let previa = add_arguments(spec.dialect, "/ruta/vindexa", "<testigo>");
            let real = add_arguments(spec.dialect, "/ruta/vindexa", "secreto");
            assert_eq!(previa.len(), real.len(), "{:?}", spec.id);
            for (izquierda, derecha) in previa.iter().zip(real.iter()) {
                if izquierda.contains("<testigo>") {
                    assert!(derecha.contains("secreto"));
                } else {
                    assert_eq!(izquierda, derecha, "{:?}", spec.id);
                }
            }
        }
    }

    #[test]
    fn un_agente_desconocido_se_rechaza() {
        let error = connect("agente-inventado", "x").expect_err("rechazar");
        assert_eq!(error.code, "validation");
    }
}
