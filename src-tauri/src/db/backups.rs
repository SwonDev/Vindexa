//! Copias automáticas de la base local.
//!
//! # Por qué existen
//!
//! Todo lo que hace valiosa esta base es **irrepetible**: las notas, los
//! checkpoints, los estados, las colecciones y el modelo de gustos no están en
//! ningún servidor y no se pueden volver a descargar. El catálogo sí; el resto
//! no. Había exportación manual, que es lo mismo que no tener copias: nadie
//! pulsa un botón todos los días.
//!
//! # Qué hace, y qué no
//!
//! Una copia al día, en un directorio propio junto a la base, con las tres más
//! recientes guardadas y las demás borradas. Usa la misma exportación que el
//! botón manual, así que la copia se **valida** después de escribirla: una copia
//! que no se puede abrir es peor que no tener ninguna, porque parece una copia.
//!
//! No sube nada a ningún sitio, no cifra —la base tampoco lo está, y decir que
//! sí sería mentir— y no toca las copias que hayas hecho tú a mano en otra
//! carpeta.
//!
//! # Lo que falla se dice
//!
//! Un fallo se guarda con su fecha y la pantalla de Datos lo enseña. Una copia
//! que dejó de hacerse en silencio es indistinguible de una que nunca hizo
//! falta, y sólo se descubre el día que se necesita.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

/// Cuántas copias se conservan. Tres cubren el error de ayer y el de anteayer
/// sin convertir el disco en un archivo histórico.
pub const KEEP: usize = 3;

/// Cada cuánto se hace una. Una biblioteca no cambia tanto en un día como para
/// justificar más, y menos deja demasiado tiempo sin red de seguridad.
pub const INTERVAL_HOURS: i64 = 24;

/// Directorio donde viven, junto a la base y no dentro de ella.
const DIR_NAME: &str = "copias";

/// Prefijo de las copias automáticas. Distingue las que hace Vindexa de las que
/// hayas guardado tú ahí: al limpiar sólo se borran las suyas.
const PREFIX: &str = "vindexa-auto-";

const LAST_KEY: &str = "backups.last_auto";
const LAST_ERROR_KEY: &str = "backups.last_error";

/// Lo que la pantalla de Datos necesita para contar la verdad.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    /// Dónde están, para poder abrirlas o copiarlas fuera.
    pub directory: String,
    /// Cuántas copias automáticas hay ahora mismo.
    pub kept: u32,
    /// Lo que ocupan entre todas.
    pub total_bytes: u64,
    /// Cuándo se hizo la última. `null` es «todavía ninguna».
    pub last_at: Option<String>,
    /// Qué falló la última vez, si falló. Una copia que dejó de hacerse en
    /// silencio sólo se descubre el día que se necesita.
    pub last_error: Option<String>,
}

/// Qué dejó una copia automática.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOutcome {
    pub path: String,
    pub bytes: u64,
    /// Copias antiguas borradas al hacer sitio.
    pub pruned: u32,
}

/// Dónde se guardan las copias de esta base.
pub fn directory(database_path: &Path) -> PathBuf {
    database_path
        .parent()
        .map(|parent| parent.join(DIR_NAME))
        .unwrap_or_else(|| PathBuf::from(DIR_NAME))
}

/// El nombre que le toca a la copia de este momento.
pub fn file_name(now: DateTime<Utc>) -> String {
    format!("{PREFIX}{}.sqlite3", now.format("%Y%m%d-%H%M%S"))
}

/// ¿Toca hacer una?
pub fn is_due(connection: &Connection, now: DateTime<Utc>) -> AppResult<bool> {
    let last: Option<String> = connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [LAST_KEY],
            |row| row.get(0),
        )
        .optional()?;
    Ok(should_run(last.as_deref(), now))
}

/// La decisión, separada para poder comprobarla sin base ni reloj.
pub fn should_run(last: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(last) = last else {
        return true;
    };
    // Un sello ilegible es un sello que no sirve: se vuelve a copiar.
    let Ok(moment) = DateTime::parse_from_rfc3339(last) else {
        return true;
    };
    now.signed_duration_since(moment.with_timezone(&Utc)) >= chrono::Duration::hours(INTERVAL_HOURS)
}

/// Las copias automáticas que hay ahora, de la más nueva a la más vieja.
pub fn existing(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut copias: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(PREFIX) && name.ends_with(".sqlite3"))
        })
        .collect();
    // El nombre lleva la fecha en formato ordenable, así que ordenar por nombre
    // ordena por antigüedad sin preguntarle al sistema de archivos —cuya marca
    // de tiempo se pierde al copiar la carpeta a otro disco—.
    copias.sort();
    copias.reverse();
    copias
}

/// Borra las que sobran y devuelve cuántas se fueron.
pub fn prune(directory: &Path, keep: usize) -> u32 {
    let mut borradas = 0_u32;
    for path in existing(directory).into_iter().skip(keep) {
        if fs::remove_file(&path).is_ok() {
            borradas = borradas.saturating_add(1);
        }
    }
    borradas
}

/// Lo que hay, para poder enseñarlo.
pub fn status(connection: &Connection, database_path: &Path) -> AppResult<BackupStatus> {
    let directorio = directory(database_path);
    let copias = existing(&directorio);
    let total_bytes = copias
        .iter()
        .filter_map(|path| fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();
    let leer = |clave: &str| -> AppResult<Option<String>> {
        Ok(connection
            .query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                [clave],
                |row| row.get(0),
            )
            .optional()?)
    };
    Ok(BackupStatus {
        directory: directorio.to_string_lossy().into_owned(),
        kept: copias.len() as u32,
        total_bytes,
        last_at: leer(LAST_KEY)?,
        last_error: leer(LAST_ERROR_KEY)?,
    })
}

/// Deja constancia de que se hizo, y borra el fallo anterior.
pub fn mark_done(connection: &Connection, now: DateTime<Utc>) -> AppResult<()> {
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![LAST_KEY, now.to_rfc3339_opts(SecondsFormat::Millis, true)],
    )?;
    connection.execute("DELETE FROM app_settings WHERE key = ?1", [LAST_ERROR_KEY])?;
    Ok(())
}

/// Deja constancia de que **no** se pudo, con lo que pasó.
pub fn mark_failed(connection: &Connection, message: &str) -> AppResult<()> {
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![LAST_ERROR_KEY, message],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, day, hour, 0, 0)
            .single()
            .expect("instante válido")
    }

    #[test]
    fn la_primera_vez_siempre_toca_y_despues_una_al_dia() {
        assert!(should_run(None, at(20, 12)), "sin copias, toca");
        assert!(
            !should_run(Some("2026-08-20T06:00:00.000Z"), at(20, 12)),
            "seis horas después no toca todavía"
        );
        assert!(
            should_run(Some("2026-08-19T06:00:00.000Z"), at(20, 12)),
            "pasado un día sí"
        );
        assert!(
            should_run(Some("no es una fecha"), at(20, 12)),
            "un sello ilegible no bloquea la copia"
        );
    }

    #[test]
    fn se_conservan_las_tres_ultimas_y_no_se_toca_lo_ajeno() {
        let directorio = tempfile::tempdir().expect("directorio temporal");
        let ruta = directorio.path();
        for nombre in [
            "vindexa-auto-20260817-030000.sqlite3",
            "vindexa-auto-20260818-030000.sqlite3",
            "vindexa-auto-20260819-030000.sqlite3",
            "vindexa-auto-20260820-030000.sqlite3",
            // Lo que no hizo Vindexa no se borra: puede ser una copia tuya.
            "vindexa-backup-20260101-000000.sqlite3",
            "notas.txt",
        ] {
            fs::write(ruta.join(nombre), b"x").expect("crear archivo");
        }

        assert_eq!(existing(ruta).len(), 4);
        assert_eq!(prune(ruta, KEEP), 1);

        let quedan: Vec<String> = existing(ruta)
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
            })
            .collect();
        assert_eq!(
            quedan,
            vec![
                "vindexa-auto-20260820-030000.sqlite3".to_string(),
                "vindexa-auto-20260819-030000.sqlite3".to_string(),
                "vindexa-auto-20260818-030000.sqlite3".to_string(),
            ],
            "se van las más viejas, no las más nuevas"
        );
        assert!(ruta.join("vindexa-backup-20260101-000000.sqlite3").exists());
        assert!(ruta.join("notas.txt").exists());
    }

    /// Una copia que no se puede abrir es peor que no tener ninguna.
    ///
    /// Parece una copia, ocupa sitio y da tranquilidad, y sólo se descubre el
    /// día que hace falta. Por eso esta prueba no comprueba que exista el
    /// archivo: lo abre y lee dentro un dato que se escribió antes.
    #[test]
    fn la_copia_automatica_se_hace_y_se_puede_abrir() {
        let directorio = tempfile::tempdir().expect("directorio temporal");
        let database = crate::db::Database::new(directorio.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar");
        {
            let connection = database.open().expect("abrir");
            connection
                .execute(
                    "INSERT INTO games(app_id, title) VALUES (10, 'Lo que no se puede volver a bajar')",
                    [],
                )
                .expect("sembrar");
        }

        let hecha = database
            .auto_backup_if_due(at(20, 3))
            .expect("copiar")
            .expect("la primera vez toca");
        assert!(hecha.bytes > 0);
        assert_eq!(hecha.pruned, 0);

        // Se abre de verdad y trae el dato dentro.
        let copia = rusqlite::Connection::open(&hecha.path).expect("abrir la copia");
        let titulo: String = copia
            .query_row("SELECT title FROM games WHERE app_id = 10", [], |row| {
                row.get(0)
            })
            .expect("leer dentro de la copia");
        assert_eq!(titulo, "Lo que no se puede volver a bajar");

        // Y no se repite hasta que pase un día.
        assert!(
            database
                .auto_backup_if_due(at(20, 9))
                .expect("copiar")
                .is_none(),
            "seis horas después no toca"
        );

        let estado = database.backup_status().expect("estado");
        assert_eq!(estado.kept, 1);
        assert!(estado.total_bytes > 0);
        assert!(estado.last_at.is_some());
        assert_eq!(estado.last_error, None);
    }

    #[test]
    fn el_nombre_lleva_la_fecha_en_un_orden_que_se_puede_ordenar() {
        // Ordenar por nombre tiene que ordenar por antigüedad: la marca de
        // tiempo del sistema de archivos se pierde al copiar la carpeta.
        let antes = file_name(at(19, 3));
        let despues = file_name(at(20, 3));
        assert!(antes < despues, "{antes} < {despues}");
        assert!(despues.starts_with(PREFIX) && despues.ends_with(".sqlite3"));
    }
}
