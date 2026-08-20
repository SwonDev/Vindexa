//! Vindexa se conecta sola a los agentes que encuentra.
//!
//! # Por qué es automático
//!
//! Conectar una biblioteca a un agente por MCP consiste en copiar un comando
//! largo con un secreto dentro. Eso no es una decisión: es un trámite, y un
//! trámite que además hay que repetir cada vez que la aplicación cambia de sitio
//! o se actualiza. Aquí no hay botón: al arrancar, Vindexa mira qué agentes
//! compatibles hay, comprueba si el alta que dejó sigue siendo válida y la
//! rehace si algo se ha movido.
//!
//! # Qué comprueba en cada arranque
//!
//! | Situación | Qué hace |
//! |---|---|
//! | No hay ningún agente instalado | Nada. Ni siquiera emite un testigo. |
//! | Hay uno y nunca se conectó | Emite testigo y lo da de alta. |
//! | Está conectado y todo cuadra | Nada. Es el caso normal y no cuesta nada. |
//! | La aplicación cambió de ruta | Rehace el alta con la ruta nueva. |
//! | El testigo se revocó o desapareció | Emite otro y rehace el alta. |
//! | El alta nueva no se pudo escribir | No finge que sigue conectado: lo dice. |
//!
//! # Límites que se respetan
//!
//! - **Se puede apagar.** `agent_autolink` en las preferencias lo desactiva, y
//!   entonces esto no toca nada.
//! - **Un testigo por agente.** No se acumulan clientes muertos: al rehacer un
//!   alta se revoca el anterior.
//! - **Rehacer un alta empieza por retirar la que había.** `mcp add` no
//!   sobrescribe una entrada existente: se va sin escribir y sin fallar, y el
//!   agente se queda con el testigo viejo mientras Vindexa cree que entregó uno
//!   nuevo. Eso rompió el enlace de Hermes durante veinte horas.
//! - **Nunca en primer plano.** Corre en un hilo aparte; si el agente tarda o
//!   falla, la ventana ni se entera.
//! - **Deja rastro.** Lo que hizo se ve en Ajustes → Agentes, con su fecha.

use crate::agent::clients::{self, NewAgentClient};
use crate::agent::hosts;
use crate::agent::scope::ALL_SCOPES;
use crate::agent::token::TokenPolicy;
use crate::db::Database;
use crate::error::AppResult;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Clave en `app_settings` con lo que se dejó conectado.
const SETTING_KEY: &str = "agent_autolink_state";

/// Clave que apaga el automatismo.
const DISABLED_KEY: &str = "agent_autolink_disabled";

/// Qué quedó conectado en un agente concreto.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LinkRecord {
    pub host_id: String,
    /// Cliente del puente al que pertenece el testigo entregado.
    pub client_id: String,
    /// Ruta del ejecutable que se registró. Si cambia, el alta caducó.
    pub command: String,
    pub linked_at: String,
}

/// Lo que ve Ajustes → Agentes: qué hay instalado, qué está conectado y si el
/// automatismo sigue encendido.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutolinkStatus {
    pub disabled: bool,
    pub links: Vec<LinkRecord>,
    pub hosts: Vec<hosts::AgentHost>,
}

/// Resultado de una pasada, para poder contarlo en la interfaz y en las pruebas.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AutolinkReport {
    /// Agentes que se han dado de alta o se han reparado.
    pub linked: Vec<String>,
    /// Agentes que ya estaban bien y no se han tocado.
    pub unchanged: Vec<String>,
    /// Agentes encontrados que no se pudieron conectar, con el motivo.
    pub failed: Vec<String>,
}

fn read_state(connection: &Connection) -> Vec<LinkRecord> {
    connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [SETTING_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write_state(connection: &Connection, records: &[LinkRecord]) -> AppResult<()> {
    let raw = serde_json::to_string(records).unwrap_or_else(|_| "[]".to_owned());
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![SETTING_KEY, raw],
    )?;
    Ok(())
}

/// ¿Está el automatismo apagado a mano?
pub fn disabled(connection: &Connection) -> bool {
    connection
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            [DISABLED_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .is_some_and(|value| value == "1")
}

/// Enciende o apaga el automatismo.
pub fn set_disabled(connection: &Connection, value: bool) -> AppResult<()> {
    connection.execute(
        "INSERT INTO app_settings(key, value, updated_at)
         VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![DISABLED_KEY, if value { "1" } else { "0" }],
    )?;
    Ok(())
}

/// Lo que hay conectado ahora mismo, para enseñarlo en Ajustes.
pub fn state(connection: &Connection) -> Vec<LinkRecord> {
    read_state(connection)
}

/// ¿Sigue existiendo y activo el cliente al que se entregó el testigo?
fn client_alive(connection: &Connection, client_id: &str) -> bool {
    connection
        .query_row(
            "SELECT enabled FROM agent_clients WHERE id = ?1",
            [client_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .ok()
        .flatten()
        .is_some_and(|enabled| enabled != 0)
}

/// Rehace el enlace de un agente ahora, sin esperar al próximo arranque.
///
/// Hasta ahora, si un enlace se rompía sólo quedaba reiniciar Vindexa y confiar.
/// Cuando el alta fallaba en silencio —que es como se rompió el de Hermes—
/// reiniciar tampoco arreglaba nada: la pasada veía un enlace vigente y no lo
/// tocaba. Esto olvida lo apuntado para ese agente y vuelve a empezar, que es
/// lo que alguien quiere decir cuando pulsa «volver a conectar».
pub fn relink(database: &Database, host_id: &str) -> AppResult<AutolinkReport> {
    {
        let connection = database.open()?;
        let mut records = read_state(&connection);
        records.retain(|record| record.host_id != host_id);
        write_state(&connection, &records)?;
    }
    reconcile(database)
}

/// Da una pasada: conecta lo que falte, repara lo que se haya movido.
///
/// No devuelve error si un agente falla: se anota y se sigue con los demás. Que
/// Hermes esté a medio instalar no puede impedir que Vindexa arranque.
pub fn reconcile(database: &Database) -> AppResult<AutolinkReport> {
    let mut connection = database.open()?;
    if disabled(&connection) {
        return Ok(AutolinkReport::default());
    }
    let hosts = hosts::detect()?;
    let mut records = read_state(&connection);
    let mut report = AutolinkReport::default();

    for host in hosts {
        // Sin ejecutable no hay agente: ni se emite testigo ni se anota nada.
        if host.path.is_none() {
            records.retain(|record| record.host_id != host.id);
            continue;
        }
        let current = hosts::vindexa_command()?;
        let existing = records.iter().find(|record| record.host_id == host.id);
        // Un enlace sigue valiendo mientras su cliente esté vivo y el binario
        // que se registró siga ahí.
        //
        // Antes se exigía además que la ruta fuera **la de este proceso**, y eso
        // convertía cualquier copia en un secuestro: abrir una compilación de
        // desarrollo desde `target/` rehacía el alta apuntando a ella, y volver
        // a la instalada la rehacía otra vez. Dos rotaciones de testigo que
        // nadie pidió, en una función cuyo trabajo es justamente que el enlace
        // no se rompa.
        let vigente = existing.is_some_and(|record| {
            client_alive(&connection, &record.client_id)
                && (record.command == current || Path::new(&record.command).exists())
        });
        if vigente {
            report.unchanged.push(host.label.clone());
            continue;
        }

        // Se rehace. Si el nombre ya está ocupado por un alta anterior de este
        // mismo enlace —el archivo de estado se perdió, o se restauró una copia—
        // se retira primero: sin esto, el alta falla con «ya existe un cliente
        // con ese nombre», el error aborta la reconciliación entera y la
        // aplicación se queda sin agente para siempre, repitiendo el mismo fallo
        // en cada arranque. El testigo viejo ya no lo tiene nadie: si lo tuviera,
        // el enlace estaría vigente y no se estaría rehaciendo.
        if let Ok(Some(retirado)) = clients::revoke_by_name(&mut connection, &host.label) {
            eprintln!(
                "Vindexa: se retira el alta anterior de {} ({retirado}) para rehacer el enlace.",
                host.label
            );
        }
        let issued = clients::issue(
            &mut connection,
            &NewAgentClient {
                name: host.label.clone(),
                kind: if host.id == "hermes" {
                    "hermes".to_owned()
                } else {
                    "generic".to_owned()
                },
                scopes: ALL_SCOPES
                    .iter()
                    .map(|scope| scope.as_str().to_owned())
                    .collect(),
            },
            TokenPolicy::default(),
        )?;
        match hosts::connect(&host.id, &issued.token) {
            Ok(_) => {
                if let Some(previous) = existing.map(|record| record.client_id.clone()) {
                    let _ = clients::revoke(&mut connection, &previous);
                }
                records.retain(|record| record.host_id != host.id);
                records.push(LinkRecord {
                    host_id: host.id.clone(),
                    client_id: issued.client.id.clone(),
                    command: current,
                    linked_at: Utc::now().to_rfc3339(),
                });
                report.linked.push(host.label.clone());
            }
            Err(error) => {
                // El testigo emitido no llegó a entregarse: se revoca para no
                // dejar credenciales vivas que nadie tiene.
                let _ = clients::revoke(&mut connection, &issued.client.id);
                // Y el enlace anterior deja de figurar como bueno, porque ya no
                // lo es: el alta se retiró para rehacerla y no se rehízo. Sin
                // esto, Ajustes seguía enseñando un enlace vigente mientras el
                // agente contestaba que su testigo no vale, y la única forma de
                // enterarse era que alguien lo intentara y fallara.
                records.retain(|record| record.host_id != host.id);
                eprintln!(
                    "Vindexa: {} se quedó sin enlace ({}). Vuelve a conectarlo desde Ajustes → Agentes.",
                    host.label, error.message
                );
                report
                    .failed
                    .push(format!("{}: {}", host.label, error.message));
            }
        }
    }

    write_state(&connection, &records)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn base() -> (TempDir, Database) {
        // Una batería que corre en el ordenador de alguien no puede dar de alta
        // servidores en su agente de verdad. Con esto la detección contesta que
        // no hay ninguno y lo que se prueba es la lógica, no la máquina.
        //
        // SAFETY: se fija antes de que ningún hilo de la prueba lea el entorno.
        unsafe { std::env::set_var("VINDEXA_SIN_AGENTES", "1") };
        let directory = tempfile::tempdir().expect("directorio temporal");
        let database = Database::new(directory.path().join("vindexa.sqlite3"));
        database.initialize().expect("inicializar");
        (directory, database)
    }

    #[test]
    fn apagado_no_toca_nada() {
        let (_directory, database) = base();
        {
            let connection = database.open().expect("abrir");
            set_disabled(&connection, true).expect("apagar");
            assert!(disabled(&connection));
        }
        let report = reconcile(&database).expect("pasada");
        assert_eq!(report, AutolinkReport::default());
    }

    #[test]
    fn no_quedan_testigos_que_nadie_tenga() {
        // Un testigo emitido y no entregado es una credencial viva que nadie
        // usa: exactamente lo que no debe quedar por ahí. Si el alta falla —el
        // agente no está, o la rechaza— hay que revocarlo.
        //
        // La comprobación vale igual en una máquina con agentes y en una sin
        // ellos, que es lo que la hace útil en cualquier sitio donde corra.
        let (_directory, database) = base();
        let _ = reconcile(&database);
        let connection = database.open().expect("abrir");
        let clientes: i64 = connection
            .query_row("SELECT COUNT(*) FROM agent_clients", [], |row| row.get(0))
            .expect("contar clientes");
        let enlazados = read_state(&connection).len() as i64;
        assert_eq!(clientes, enlazados, "hay testigos sin dueño");
    }

    #[test]
    fn una_segunda_pasada_no_reconecta_lo_que_ya_estaba() {
        // Arrancar la aplicación diez veces no puede dejar diez clientes ni
        // rehacer un alta que ya funciona.
        let (_directory, database) = base();
        let primera = reconcile(&database).expect("primera pasada");
        let segunda = reconcile(&database).expect("segunda pasada");
        assert!(segunda.linked.is_empty(), "{segunda:?}");
        assert_eq!(segunda.unchanged.len(), primera.linked.len());
    }

    #[test]
    fn el_estado_se_guarda_y_se_lee() {
        let (_directory, database) = base();
        let connection = database.open().expect("abrir");
        let records = vec![LinkRecord {
            host_id: "hermes".into(),
            client_id: "cli-1".into(),
            command: "/Applications/Vindexa.app/Contents/MacOS/vindexa".into(),
            linked_at: "2026-08-19T15:00:00Z".into(),
        }];
        write_state(&connection, &records).expect("guardar");
        assert_eq!(read_state(&connection), records);
    }
}
