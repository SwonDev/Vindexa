//! Modelos de lenguaje que ya viven en este ordenador.
//!
//! # Para qué
//!
//! Vindexa puede conducirse hablando. Si hay un agente instalado —Hermes, por
//! ejemplo— se conecta a él y ya está ([`crate::agent::autolink`]). Si no lo
//! hay, la alternativa no puede ser «instálate algo y vuelve»: hay que mirar qué
//! tiene la máquina, qué le cabe y proponer lo que le va a funcionar.
//!
//! Este módulo es la parte que mira. No descarga, no arranca nada y no manda
//! nada a ninguna parte: sólo contesta a tres preguntas.
//!
//! | Pregunta | Función |
//! |---|---|
//! | ¿Con qué se puede ejecutar un modelo aquí? | [`runtimes`] |
//! | ¿Qué modelos hay ya descargados? | [`scan_models`] |
//! | ¿Qué le cabe a esta máquina? | [`hardware`] |
//! | ¿Hay ya algo sirviendo un modelo? | [`endpoints::discover`] |
//! | ¿Y si no hay nada, qué me descargo? | [`catalog::suggest`] |
//! | ¿Y cómo instalo el motor que falta? | [`install::plan`] |
//!
//! # Por qué no se rastrea el disco entero
//!
//! Un disco de dos teras tarda minutos en recorrerse y encuentra sobre todo
//! ruido. Se miran los sitios donde estas cosas se guardan de verdad —la caché
//! de Hugging Face, las carpetas de LM Studio y de Ollama, `~/AI`, `~/Models`—
//! con la profundidad justa y un tope de archivos. Encontrar rápido lo que hay
//! vale más que encontrar todo lo que podría haber.

pub mod catalog;
pub mod endpoints;
pub mod install;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hasta dónde se baja dentro de cada carpeta conocida.
///
/// Cuatro niveles cubren `hub/models--autor--nombre/snapshots/<hash>/x.gguf`,
/// que es la ruta más profunda que usan estas herramientas.
const MAX_DEPTH: usize = 5;

/// Tope de archivos inspeccionados por carpeta raíz. Una caché de Hugging Face
/// puede tener decenas de miles de ficheros sueltos.
const MAX_ENTRIES_PER_ROOT: usize = 20_000;

/// Un modelo por debajo de esto no es un modelo: es un fragmento, un índice o
/// un proyector suelto.
const MIN_MODEL_BYTES: u64 = 64 * 1024 * 1024;

/// Motor capaz de ejecutar un modelo en local.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub id: String,
    pub label: String,
    /// Ruta del ejecutable. `None` significa que no está instalado.
    pub path: Option<String>,
    /// Qué formatos sabe cargar, para poder casarlo con lo que hay descargado.
    pub formats: Vec<String>,
}

/// Un modelo encontrado en el disco.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    /// Nombre legible: el del archivo o el de la carpeta que lo contiene.
    pub name: String,
    pub path: String,
    /// `gguf` o `mlx`.
    pub format: String,
    pub size_bytes: u64,
    /// De dónde salió: la carpeta conocida en la que apareció.
    pub source: String,
}

/// Todo lo que hace falta saber para decidir con qué hablar con Vindexa.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModelSurvey {
    pub runtimes: Vec<Runtime>,
    pub models: Vec<LocalModel>,
    pub hardware: Hardware,
    /// Servidores de inferencia que ya están corriendo. Si hay uno, no hace
    /// falta arrancar nada: se usa ese.
    pub endpoints: Vec<endpoints::InferenceEndpoint>,
}

/// Lo que la máquina puede permitirse.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    /// Memoria total en bytes. `None` cuando el sistema no lo dice: eso es «no
    /// se sabe», y con eso no se recomienda nada.
    pub total_memory_bytes: Option<u64>,
    pub cpu_cores: Option<usize>,
    pub architecture: String,
    /// Memoria que se considera razonable dedicar a un modelo, en bytes.
    ///
    /// Es la mitad de la total: el resto lo necesitan el sistema, el navegador
    /// y la propia Vindexa. Un modelo que ocupa toda la memoria no va lento, va
    /// a tirones y acaba tirando cosas.
    pub usable_model_bytes: Option<u64>,
}

/// Motores conocidos y dónde suelen estar.
const RUNTIMES: &[(&str, &str, &[&str], &[&str])] = &[
    (
        "llamacpp",
        "llama.cpp",
        &["llama-server"],
        &["gguf"],
    ),
    ("mlx", "MLX", &["mlx_lm.server"], &["mlx"]),
    ("ollama", "Ollama", &["ollama"], &["gguf"]),
    ("lmstudio", "LM Studio", &["lms"], &["gguf", "mlx"]),
];

/// Directorios habituales de los ejecutables, además del `PATH`.
const BIN_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/usr/bin",
    ".local/bin",
    ".cargo/bin",
];

fn find_executable(names: &[&str]) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    for name in names {
        for directory in BIN_DIRS {
            let candidate = if directory.starts_with('/') {
                PathBuf::from(directory).join(name)
            } else {
                let Some(home) = home.as_ref() else { continue };
                home.join(directory).join(name)
            };
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if let Some(path) = std::env::var_os("PATH")
            && let Some(found) = std::env::split_paths(&path)
                .map(|directory| directory.join(name))
                .find(|candidate| candidate.is_file())
        {
            return Some(found);
        }
    }
    None
}

/// Con qué se puede ejecutar un modelo en esta máquina.
pub fn runtimes() -> Vec<Runtime> {
    RUNTIMES
        .iter()
        .map(|(id, label, binaries, formats)| Runtime {
            id: (*id).to_owned(),
            label: (*label).to_owned(),
            path: find_executable(binaries).map(|path| path.to_string_lossy().into_owned()),
            formats: formats.iter().map(|format| (*format).to_owned()).collect(),
        })
        .collect()
}

/// Carpetas donde estas herramientas guardan los modelos, con su nombre.
fn model_roots() -> Vec<(String, PathBuf)> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    [
        ("Hugging Face", ".cache/huggingface/hub"),
        ("LM Studio", ".cache/lm-studio/models"),
        ("LM Studio", ".lmstudio/models"),
        ("Ollama", ".ollama/models"),
        ("Carpeta AI", "AI/models"),
        ("Carpeta AI", "AI"),
        ("Modelos", "Models"),
        ("Modelos", "models"),
    ]
    .iter()
    .map(|(label, relative)| ((*label).to_owned(), home.join(relative)))
    .filter(|(_, path)| path.is_dir())
    .collect()
}

/// ¿Es este archivo un modelo utilizable?
fn model_from_file(path: &Path, source: &str) -> Option<LocalModel> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    let format = match extension.as_str() {
        "gguf" => "gguf",
        "safetensors" => "mlx",
        _ => return None,
    };
    let size = path.metadata().ok()?.len();
    if size < MIN_MODEL_BYTES {
        return None;
    }
    let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    // Un proyector multimodal o un fragmento suelto no se pueden cargar por su
    // cuenta: enseñarlos como modelos sólo sirve para que alguien elija mal.
    if file_name.starts_with("mmproj") {
        return None;
    }
    // Los MLX vienen partidos en varios ficheros dentro de una carpeta: el
    // modelo es la carpeta, así que se nombra por ella y se cuenta una vez.
    let name = if format == "mlx" {
        path.parent()?.file_name()?.to_string_lossy().into_owned()
    } else {
        path.file_stem()?.to_string_lossy().into_owned()
    };
    Some(LocalModel {
        name,
        path: path.to_string_lossy().into_owned(),
        format: format.to_owned(),
        size_bytes: size,
        source: source.to_owned(),
    })
}

/// Recorre una carpeta con profundidad y número de archivos acotados.
fn walk(root: &Path, source: &str, found: &mut Vec<LocalModel>) {
    let mut pending = vec![(root.to_path_buf(), 0_usize)];
    let mut inspected = 0_usize;
    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DEPTH || inspected >= MAX_ENTRIES_PER_ROOT {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            inspected += 1;
            if inspected >= MAX_ENTRIES_PER_ROOT {
                break;
            }
            let path = entry.path();
            // Los enlaces simbólicos se siguen sólo hacia archivos: la caché de
            // Hugging Face los usa, pero seguirlos hacia carpetas puede dar
            // vueltas en círculo.
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                pending.push((path, depth + 1));
            } else if let Some(model) = model_from_file(&path, source) {
                found.push(model);
            }
        }
    }
}

/// Modelos ya descargados, sin repetir y de mayor a menor.
pub fn scan_models() -> Vec<LocalModel> {
    let mut found = Vec::new();
    for (label, root) in model_roots() {
        walk(&root, &label, &mut found);
    }
    // Un MLX aporta un archivo por fragmento: se queda el mayor de cada
    // carpeta, que es lo que representa el modelo entero.
    found.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(right.size_bytes.cmp(&left.size_bytes))
    });
    found.dedup_by(|left, right| left.name == right.name && left.format == right.format);
    found.sort_by_key(|model| std::cmp::Reverse(model.size_bytes));
    found
}

/// Memoria total del sistema, preguntada al sistema.
fn total_memory() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()?;
        String::from_utf8_lossy(&output.stdout).trim().parse().ok()
    }
    #[cfg(target_os = "linux")]
    {
        let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
        let line = contents.lines().find(|line| line.starts_with("MemTotal:"))?;
        let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kilobytes * 1024)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

/// Qué máquina es esta.
pub fn hardware() -> Hardware {
    let total = total_memory();
    Hardware {
        total_memory_bytes: total,
        cpu_cores: std::thread::available_parallelism()
            .ok()
            .map(std::num::NonZeroUsize::get),
        architecture: std::env::consts::ARCH.to_owned(),
        usable_model_bytes: total.map(|bytes| bytes / 2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_motores_se_describen_aunque_no_esten() {
        // La interfaz tiene que poder decir «no tienes esto» con su nombre, así
        // que la lista es siempre completa y lo que cambia es la ruta.
        let encontrados = runtimes();
        assert_eq!(encontrados.len(), RUNTIMES.len());
        for runtime in &encontrados {
            assert!(!runtime.label.is_empty());
            assert!(!runtime.formats.is_empty());
        }
    }

    #[test]
    fn un_fragmento_pequeno_no_es_un_modelo() {
        let directory = tempfile::tempdir().expect("directorio temporal");
        let path = directory.path().join("diminuto.gguf");
        std::fs::write(&path, b"no soy un modelo").expect("escribir");
        assert_eq!(model_from_file(&path, "prueba"), None);
    }

    #[test]
    fn un_proyector_multimodal_no_se_ofrece_como_modelo() {
        // Se distribuye junto al modelo de verdad y no se puede cargar solo:
        // ofrecerlo sería invitar a elegir mal.
        let directory = tempfile::tempdir().expect("directorio temporal");
        let path = directory.path().join("mmproj-loquesea-f16.gguf");
        std::fs::write(&path, vec![0_u8; (MIN_MODEL_BYTES + 1) as usize]).expect("escribir");
        assert_eq!(model_from_file(&path, "prueba"), None);
    }

    #[test]
    fn un_gguf_grande_si_lo_es() {
        let directory = tempfile::tempdir().expect("directorio temporal");
        let path = directory.path().join("Qwen3-8B-Q4_K_M.gguf");
        std::fs::write(&path, vec![0_u8; (MIN_MODEL_BYTES + 1) as usize]).expect("escribir");
        let modelo = model_from_file(&path, "prueba").expect("es un modelo");
        assert_eq!(modelo.name, "Qwen3-8B-Q4_K_M");
        assert_eq!(modelo.format, "gguf");
    }

    #[test]
    fn un_mlx_se_nombra_por_su_carpeta() {
        // Viene partido en varios ficheros: el modelo es la carpeta.
        let directory = tempfile::tempdir().expect("directorio temporal");
        let carpeta = directory.path().join("Qwen3-8B-MLX-4bit");
        std::fs::create_dir_all(&carpeta).expect("crear");
        let path = carpeta.join("model-00001-of-00002.safetensors");
        std::fs::write(&path, vec![0_u8; (MIN_MODEL_BYTES + 1) as usize]).expect("escribir");
        let modelo = model_from_file(&path, "prueba").expect("es un modelo");
        assert_eq!(modelo.name, "Qwen3-8B-MLX-4bit");
        assert_eq!(modelo.format, "mlx");
    }

    #[test]
    fn la_memoria_utilizable_nunca_se_inventa() {
        // Sin dato del sistema no se recomienda nada: decir «te caben 8 GB» sin
        // saber cuánta memoria hay es exactamente el error que no se comete.
        let maquina = hardware();
        match maquina.total_memory_bytes {
            Some(total) => assert_eq!(maquina.usable_model_bytes, Some(total / 2)),
            None => assert_eq!(maquina.usable_model_bytes, None),
        }
    }
}
