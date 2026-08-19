//! Instalar el motor que falta.
//!
//! # Qué hace y qué no
//!
//! Detecta con qué gestor de paquetes cuenta esta máquina y arma la orden
//! exacta que instalaría llama.cpp. **Enseñarla es automático; ejecutarla, no.**
//!
//! No es una contradicción con que todo lo demás se configure solo. Conectar
//! Vindexa a un agente que ya está instalado toca la configuración de esa
//! aplicación y se deshace borrando una línea. Instalar un paquete toca el
//! sistema entero, tarda, puede pedir contraseña y deja cosas fuera de Vindexa.
//! Eso se hace cuando alguien lo pide, viendo antes qué se va a ejecutar.
//!
//! # Por qué no se descarga un binario suelto
//!
//! Bajar un ejecutable de internet y ponerlo a correr es exactamente lo que no
//! debe hacer una aplicación por su cuenta. El gestor de paquetes del sistema ya
//! resuelve la firma, la procedencia y las actualizaciones, y quien lo usa sabe
//! lo que tiene instalado.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Gestor de paquetes conocido, con lo que hay que decirle.
struct PackageManager {
    id: &'static str,
    binary: &'static str,
    /// Argumentos completos para instalar llama.cpp.
    args: &'static [&'static str],
}

#[cfg(target_os = "macos")]
const MANAGERS: &[PackageManager] = &[PackageManager {
    id: "homebrew",
    binary: "brew",
    args: &["install", "llama.cpp"],
}];

#[cfg(target_os = "windows")]
const MANAGERS: &[PackageManager] = &[
    PackageManager {
        id: "winget",
        binary: "winget",
        args: &["install", "--id", "ggml.llamacpp", "--silent"],
    },
    PackageManager {
        id: "scoop",
        binary: "scoop",
        args: &["install", "llama.cpp"],
    },
];

#[cfg(all(unix, not(target_os = "macos")))]
const MANAGERS: &[PackageManager] = &[
    PackageManager {
        id: "brew",
        binary: "brew",
        args: &["install", "llama.cpp"],
    },
    PackageManager {
        id: "pacman",
        binary: "pacman",
        args: &["-S", "--noconfirm", "llama.cpp"],
    },
];

/// Qué se puede hacer para tener llama.cpp en esta máquina.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    /// `true` cuando ya está instalado y no hay nada que hacer.
    pub already_installed: bool,
    /// Gestor encontrado. `None` significa que hay que instalarlo a mano.
    pub manager: Option<String>,
    /// La orden exacta, para poder leerla antes de ejecutarla.
    pub command: Option<String>,
}

const BIN_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];

fn find_binary(name: &str) -> Option<PathBuf> {
    for directory in BIN_DIRS {
        let candidate = PathBuf::from(directory).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
}

/// Qué haría falta, sin hacer nada todavía.
pub fn plan() -> InstallPlan {
    if super::runtimes()
        .iter()
        .any(|runtime| runtime.id == "llamacpp" && runtime.path.is_some())
    {
        return InstallPlan {
            already_installed: true,
            manager: None,
            command: None,
        };
    }
    for manager in MANAGERS {
        if find_binary(manager.binary).is_some() {
            return InstallPlan {
                already_installed: false,
                manager: Some(manager.id.to_owned()),
                command: Some(format!("{} {}", manager.binary, manager.args.join(" "))),
            };
        }
    }
    InstallPlan {
        already_installed: false,
        manager: None,
        command: None,
    }
}

/// Ejecuta la instalación. Sólo se llama cuando alguien lo pide.
///
/// Devuelve lo que dijo el gestor de paquetes, recortado: si algo sale mal, esa
/// salida es lo único que explica por qué.
pub fn run() -> AppResult<String> {
    let plan = plan();
    if plan.already_installed {
        return Ok("llama.cpp ya estaba instalado.".to_owned());
    }
    let manager = MANAGERS
        .iter()
        .find(|manager| Some(manager.id.to_owned()) == plan.manager)
        .ok_or_else(|| {
            AppError::new(
                "local_model_install",
                "No se ha encontrado ningún gestor de paquetes con el que instalar llama.cpp.",
            )
        })?;
    let binary = find_binary(manager.binary).ok_or_else(|| {
        AppError::new(
            "local_model_install",
            format!("{} ya no está donde estaba.", manager.binary),
        )
    })?;

    let output = Command::new(binary)
        .args(manager.args)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| {
            AppError::new(
                "local_model_install",
                format!("No se pudo ejecutar {}: {error}", manager.binary),
            )
        })?;
    if !output.status.success() {
        let detalle = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::new(
            "local_model_install",
            format!(
                "La instalación no terminó: {}",
                detalle.trim().chars().take(400).collect::<String>()
            ),
        ));
    }
    // Que el gestor diga que ha terminado bien no basta: lo que cuenta es que
    // el ejecutable esté donde se le va a buscar después.
    if find_binary("llama-server").is_none() {
        return Err(AppError::new(
            "local_model_install",
            "El gestor terminó sin errores pero llama-server no aparece en el sistema.",
        ));
    }
    Ok("llama.cpp instalado.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_plan_no_inventa_un_gestor_que_no_esta() {
        let plan = plan();
        if let Some(manager) = &plan.manager {
            assert!(
                MANAGERS.iter().any(|known| known.id == manager),
                "gestor desconocido: {manager}"
            );
            assert!(plan.command.is_some(), "un gestor sin orden no sirve");
        } else {
            assert!(plan.command.is_none());
        }
    }

    #[test]
    fn si_ya_esta_instalado_no_se_propone_nada() {
        let plan = plan();
        if plan.already_installed {
            assert!(plan.command.is_none());
            assert!(plan.manager.is_none());
        }
    }

    #[test]
    fn la_orden_que_se_ensena_es_la_que_se_ejecuta() {
        // Enseñar una cosa y ejecutar otra sería la peor manera posible de
        // pedir permiso.
        for manager in MANAGERS {
            let mostrada = format!("{} {}", manager.binary, manager.args.join(" "));
            assert!(mostrada.contains("llama"), "{mostrada}");
            assert!(!manager.args.is_empty());
        }
    }
}
