//! Límite de frecuencia por cliente agente.
//!
//! La ventana es deslizante y vive en memoria del proceso. No se persiste a
//! propósito: escribir una fila por petición para contarlas convertiría el
//! propio límite en una vía de crecimiento de la base, y un reinicio de Vindexa
//! ya obliga al agente a volver a autenticarse.
//!
//! El reloj se inyecta como milisegundos desde época en cada llamada. Así el
//! límite es determinista en las pruebas y no depende de `Instant::now`.

use crate::error::{AppError, AppResult};
use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

/// Peticiones admitidas por ventana y cliente.
pub const DEFAULT_MAX_REQUESTS: usize = 30;
/// Duración de la ventana deslizante en milisegundos.
pub const DEFAULT_WINDOW_MILLIS: i64 = 60_000;
/// Tope de clientes vigilados a la vez. Evita que un atacante que rote
/// identificadores haga crecer el mapa sin control.
const MAX_TRACKED_CLIENTS: usize = 64;

/// Ventana deslizante compartida por todo el proceso.
#[derive(Debug)]
pub struct RateLimiter {
    max_requests: usize,
    window_millis: i64,
    seen: Mutex<HashMap<String, VecDeque<i64>>>,
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_REQUESTS, DEFAULT_WINDOW_MILLIS)
    }
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_millis: i64) -> Self {
        Self {
            max_requests: max_requests.max(1),
            window_millis: window_millis.max(1),
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Contabiliza una petición. Devuelve error si el cliente supera el límite.
    ///
    /// Una petición rechazada **no** se contabiliza: el agente que respeta el
    /// límite recupera su cupo en cuanto la ventana avanza, sin castigo extra.
    pub fn check(&self, client_id: &str, now_millis: i64) -> AppResult<()> {
        let mut seen = match self.seen.lock() {
            Ok(guard) => guard,
            // Un pánico previo dentro del candado no debe abrir la puerta.
            Err(poisoned) => poisoned.into_inner(),
        };

        if !seen.contains_key(client_id) && seen.len() >= MAX_TRACKED_CLIENTS {
            seen.retain(|_, hits| {
                hits.back()
                    .is_some_and(|last| now_millis - last < self.window_millis)
            });
            if seen.len() >= MAX_TRACKED_CLIENTS {
                return Err(rate_limited(self.max_requests));
            }
        }

        let hits = seen.entry(client_id.to_string()).or_default();
        while hits
            .front()
            .is_some_and(|first| now_millis - first >= self.window_millis)
        {
            hits.pop_front();
        }
        if hits.len() >= self.max_requests {
            return Err(rate_limited(self.max_requests));
        }
        hits.push_back(now_millis);
        Ok(())
    }

    /// Peticiones que aún caben en la ventana actual.
    pub fn remaining(&self, client_id: &str, now_millis: i64) -> usize {
        let seen = match self.seen.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let used = seen
            .get(client_id)
            .map(|hits| {
                hits.iter()
                    .filter(|hit| now_millis - **hit < self.window_millis)
                    .count()
            })
            .unwrap_or_default();
        self.max_requests.saturating_sub(used)
    }

    /// Olvida un cliente. Se usa al revocarlo.
    pub fn forget(&self, client_id: &str) {
        let mut seen = match self.seen.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        seen.remove(client_id);
    }
}

fn rate_limited(max_requests: usize) -> AppError {
    AppError::new(
        "agent_rate_limit",
        format!(
            "El cliente agente ha superado el límite de {max_requests} peticiones por minuto. \
             Espera antes de volver a intentarlo."
        ),
    )
}

static SHARED: OnceLock<RateLimiter> = OnceLock::new();

/// Limitador compartido por los comandos Tauri.
pub fn shared() -> &'static RateLimiter {
    SHARED.get_or_init(RateLimiter::default)
}
