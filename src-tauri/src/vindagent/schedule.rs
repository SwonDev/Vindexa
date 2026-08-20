//! Encargos que el agente repite solo.
//!
//! # Qué es un encargo
//!
//! Una frase y una cadencia: «cada domingo, sube a Backlog lo que lleve seis
//! meses sin tocar», «cada día, repasa qué he dejado a medias». Vindexa se la
//! manda al agente cuando toca, con las mismas herramientas y las mismas
//! barreras que si se la dijeras tú.
//!
//! # Por qué vive aquí y no en el agente
//!
//! Porque el encargo habla de **esta** biblioteca. Un programador de tareas
//! genérico sabe de horarios pero no sabe qué es un estado ni qué colecciones
//! tienes, y acabaría guardando la frase en otro sitio, con otro formato y sin
//! el rastro de lo que hizo. Aquí cada ejecución deja su fila en la auditoría
//! del puente, con su botón de deshacer, igual que cualquier otra orden.
//!
//! # Lo que no hace
//!
//! - **No corre si no hay modelo.** Sin un servidor de inferencia el encargo
//!   queda pendiente; no se inventa un resultado ni se marca como hecho.
//! - **No se salta la confirmación.** Lo que exige que una persona lo apruebe
//!   sigue esperando dentro de Vindexa.
//! - **No se acumula.** Si el ordenador estuvo apagado una semana, el encargo
//!   corre una vez, no siete.

use crate::db::Database;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Duration, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Cadencias admitidas. Cerradas a propósito: una expresión de cron es
/// exactamente el tipo de cosa que nadie recuerda escribir bien.
pub const CADENCES: [&str; 3] = ["diaria", "semanal", "mensual"];

fn cadence_duration(cadence: &str) -> Option<Duration> {
    match cadence {
        "diaria" => Some(Duration::days(1)),
        "semanal" => Some(Duration::weeks(1)),
        "mensual" => Some(Duration::days(30)),
        _ => None,
    }
}

/// Un encargo guardado.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    /// Lo que se le pide, en la misma frase que le dirías tú.
    pub instruction: String,
    pub cadence: String,
    pub enabled: bool,
    pub last_run_at: Option<String>,
    /// Resumen de lo último que hizo, o del error si falló.
    pub last_result: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveScheduledTask {
    #[serde(default)]
    pub id: Option<String>,
    pub instruction: String,
    pub cadence: String,
    #[serde(default = "yes")]
    pub enabled: bool,
}

fn yes() -> bool {
    true
}

const MAX_INSTRUCTION: usize = 500;

fn map(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get(0)?,
        instruction: row.get(1)?,
        cadence: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        last_run_at: row.get(4)?,
        last_result: row.get(5)?,
        created_at: row.get(6)?,
    })
}

const SELECT: &str =
    "SELECT id, instruction, cadence, enabled, last_run_at, last_result, created_at
                        FROM agent_tasks";

pub fn list(database: &Database) -> AppResult<Vec<ScheduledTask>> {
    let connection = database.open()?;
    let mut statement = connection.prepare(&format!("{SELECT} ORDER BY created_at"))?;
    let rows = statement.query_map([], map)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn save(database: &Database, input: &SaveScheduledTask) -> AppResult<ScheduledTask> {
    let instruction = input.instruction.trim();
    if instruction.is_empty() || instruction.chars().count() > MAX_INSTRUCTION {
        return Err(AppError::validation(format!(
            "El encargo tiene que decir algo y caber en {MAX_INSTRUCTION} caracteres."
        )));
    }
    if !CADENCES.contains(&input.cadence.as_str()) {
        return Err(AppError::validation(
            "La cadencia tiene que ser diaria, semanal o mensual.",
        ));
    }
    let connection = database.open()?;
    let id = input
        .id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    connection.execute(
        "INSERT INTO agent_tasks(id, instruction, cadence, enabled)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(id) DO UPDATE SET
            instruction = excluded.instruction,
            cadence = excluded.cadence,
            enabled = excluded.enabled",
        params![id, instruction, input.cadence, i64::from(input.enabled)],
    )?;
    connection
        .query_row(&format!("{SELECT} WHERE id = ?1"), [&id], map)
        .map_err(Into::into)
}

pub fn delete(database: &Database, id: &str) -> AppResult<()> {
    let connection = database.open()?;
    let removed = connection.execute("DELETE FROM agent_tasks WHERE id = ?1", [id])?;
    if removed == 0 {
        return Err(AppError::not_found("Ese encargo ya no existe."));
    }
    Ok(())
}

/// ¿Le toca a este encargo?
fn is_due(task: &ScheduledTask, now: DateTime<Utc>) -> bool {
    if !task.enabled {
        return false;
    }
    let Some(period) = cadence_duration(&task.cadence) else {
        return false;
    };
    let Some(last) = task.last_run_at.as_deref() else {
        // Nunca ha corrido: le toca. Un encargo que no arranca hasta la semana
        // que viene parece roto.
        return true;
    };
    let Ok(moment) = DateTime::parse_from_rfc3339(last) else {
        // Un sello ilegible no puede significar «ya está hecho».
        return true;
    };
    now.signed_duration_since(moment.with_timezone(&Utc)) >= period
}

fn record(database: &Database, id: &str, result: &str) -> AppResult<()> {
    let connection = database.open()?;
    connection.execute(
        "UPDATE agent_tasks
            SET last_run_at = ?2, last_result = ?3
          WHERE id = ?1",
        params![
            id,
            Utc::now().to_rfc3339(),
            result.chars().take(500).collect::<String>()
        ],
    )?;
    Ok(())
}

/// Resultado de una pasada, para poder contarlo.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleReport {
    pub ran: Vec<String>,
    pub failed: Vec<String>,
}

/// Corre los encargos que toquen.
///
/// Se marca la ejecución **también cuando falla**: reintentar cada treinta
/// segundos un encargo que no puede funcionar es la forma más rápida de llenar
/// la auditoría de ruido. Lo que falló se cuenta en `last_result`, se ve en
/// Ajustes y volverá a intentarse en su siguiente turno.
pub async fn run_due(
    database: &Database,
    base_url: &str,
    model: &str,
) -> AppResult<ScheduleReport> {
    let now = Utc::now();
    let mut report = ScheduleReport::default();
    for task in list(database)? {
        if !is_due(&task, now) {
            continue;
        }
        let history = vec![super::ChatMessage {
            role: "user".to_owned(),
            content: task.instruction.clone(),
            tool: None,
        }];
        match super::chat(database, base_url, model, &history).await {
            Ok(turn) => {
                let resumen = if turn.steps.is_empty() {
                    format!("Sin cambios. {}", turn.reply)
                } else {
                    format!(
                        "{} acciones: {}",
                        turn.steps.len(),
                        turn.steps
                            .iter()
                            .map(|step| step.tool.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                record(database, &task.id, &resumen)?;
                report.ran.push(task.instruction);
            }
            Err(error) => {
                record(database, &task.id, &format!("Falló: {}", error.message))?;
                report.failed.push(task.instruction);
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> (tempfile::TempDir, Database) {
        let directory = tempfile::tempdir().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar");
        (directory, database)
    }

    fn task(cadence: &str, last: Option<&str>, enabled: bool) -> ScheduledTask {
        ScheduledTask {
            id: "x".into(),
            instruction: "haz algo".into(),
            cadence: cadence.into(),
            enabled,
            last_run_at: last.map(str::to_owned),
            last_result: None,
            created_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn un_encargo_nuevo_arranca_enseguida() {
        // Uno que no corre hasta la semana que viene parece roto.
        assert!(is_due(&task("semanal", None, true), Utc::now()));
    }

    #[test]
    fn apagado_no_corre_nunca() {
        assert!(!is_due(&task("diaria", None, false), Utc::now()));
    }

    #[test]
    fn no_se_repite_antes_de_tiempo() {
        let hace_una_hora = (Utc::now() - Duration::hours(1)).to_rfc3339();
        assert!(!is_due(
            &task("diaria", Some(&hace_una_hora), true),
            Utc::now()
        ));
        let hace_dos_dias = (Utc::now() - Duration::days(2)).to_rfc3339();
        assert!(is_due(
            &task("diaria", Some(&hace_dos_dias), true),
            Utc::now()
        ));
    }

    #[test]
    fn el_ordenador_apagado_una_semana_no_acumula_siete_pasadas() {
        // Corre una vez, no una por día perdido: lo contrario sería castigar a
        // quien no enciende el ordenador.
        let hace_un_mes = (Utc::now() - Duration::days(30)).to_rfc3339();
        let encargo = task("diaria", Some(&hace_un_mes), true);
        assert!(is_due(&encargo, Utc::now()));
        // Tras correr, el sello es de ahora y ya no toca.
        let recien = task("diaria", Some(&Utc::now().to_rfc3339()), true);
        assert!(!is_due(&recien, Utc::now()));
    }

    #[test]
    fn un_sello_ilegible_se_trata_como_si_no_hubiera() {
        assert!(is_due(&task("diaria", Some("el martes"), true), Utc::now()));
    }

    #[test]
    fn una_cadencia_inventada_no_corre() {
        // No es lo mismo que apagado: es que no se sabe cuándo, y ante eso no
        // se ejecuta nada.
        assert!(!is_due(&task("cuando sea", None, true), Utc::now()));
    }

    #[test]
    fn se_guarda_se_lee_y_se_borra() {
        let (_directory, database) = base();
        let saved = save(
            &database,
            &SaveScheduledTask {
                id: None,
                instruction: "cada domingo revisa el backlog".into(),
                cadence: "semanal".into(),
                enabled: true,
            },
        )
        .expect("guardar");
        assert_eq!(list(&database).expect("listar").len(), 1);
        delete(&database, &saved.id).expect("borrar");
        assert!(list(&database).expect("listar").is_empty());
    }

    #[test]
    fn un_encargo_vacio_o_con_cadencia_rara_se_rechaza() {
        let (_directory, database) = base();
        for input in [
            SaveScheduledTask {
                id: None,
                instruction: "   ".into(),
                cadence: "diaria".into(),
                enabled: true,
            },
            SaveScheduledTask {
                id: None,
                instruction: "algo".into(),
                cadence: "cada rato".into(),
                enabled: true,
            },
        ] {
            assert_eq!(
                save(&database, &input).expect_err("rechazar").code,
                "validation"
            );
        }
    }
}
