//! Esquema completo para las pruebas de integración.
//!
//! Antes cada prueba enumeraba a mano las migraciones que le interesaban con
//! `include_str!`. Funcionaba hasta que una migración nueva tocaba `games`: la
//! consulta pasaba a pedir una columna que la prueba no había creado y el fallo
//! aparecía en un sitio que no tenía nada que ver con el cambio.
//!
//! Aquí se aplican **todas** las migraciones, en el mismo orden que la
//! aplicación, leyéndolas del directorio en tiempo de ejecución. Una migración
//! nueva entra sola.
//!
//! Las pruebas que comprueban una migración concreta —`legacy_ownership`, por
//! ejemplo— siguen aplicando su subconjunto a propósito: ahí el esquema parcial
//! *es* el sujeto de la prueba.

use rusqlite::Connection;
use std::fs;
use std::path::PathBuf;

/// Devuelve las migraciones ordenadas por su número de prefijo.
fn migraciones() -> Vec<(String, String)> {
    let directorio = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let mut ficheros: Vec<PathBuf> = fs::read_dir(&directorio)
        .unwrap_or_else(|error| panic!("leer {}: {error}", directorio.display()))
        .filter_map(|entrada| entrada.ok().map(|entrada| entrada.path()))
        .filter(|ruta| ruta.extension().is_some_and(|extension| extension == "sql"))
        .collect();
    // El nombre empieza por el número con ceros a la izquierda, así que ordenar
    // el texto ya ordena por versión.
    ficheros.sort();
    assert!(
        !ficheros.is_empty(),
        "no se encontró ninguna migración en {}",
        directorio.display()
    );
    ficheros
        .into_iter()
        .map(|ruta| {
            let nombre = ruta
                .file_name()
                .expect("la ruta tiene nombre")
                .to_string_lossy()
                .into_owned();
            let sql = fs::read_to_string(&ruta)
                .unwrap_or_else(|error| panic!("leer {}: {error}", ruta.display()));
            (nombre, sql)
        })
        .collect()
}

/// Abre una base en memoria con el esquema actual completo.
///
/// Activa las claves foráneas igual que la aplicación: sin ellas una prueba
/// puede insertar una fila huérfana y dar por bueno un comportamiento que en
/// producción sería un error.
pub fn base_en_memoria() -> Connection {
    let connection = Connection::open_in_memory().expect("abrir SQLite temporal");
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .expect("activar claves foráneas");
    for (nombre, sql) in migraciones() {
        connection
            .execute_batch(&sql)
            .unwrap_or_else(|error| panic!("aplicar la migración {nombre}: {error}"));
    }
    connection
}
