//! Puente de agentes externos (migración 026).
//!
//! Permite que un agente conversacional —Hermes, en el caso de esta casa, o
//! cualquier otro cliente local— conduzca Vindexa con frases en lenguaje
//! natural, sin abrir un solo puerto de red y sin que ninguna modificación
//! ocurra en silencio.
//!
//! ## Capas
//!
//! | Módulo | Responsabilidad |
//! |---|---|
//! | [`crypto`] | SHA-256, HMAC-SHA256 y PBKDF2 propios, por ausencia de dependencias |
//! | [`token`] | Emisión, análisis y verificación de tokens con sal |
//! | [`scope`] | Conjunto cerrado de ámbitos de autorización |
//! | [`clients`] | Alta, rotación, revocación y autenticación de clientes |
//! | [`ratelimit`] | Ventana deslizante por cliente |
//! | [`matching`] | Resolución tolerante de juegos por nombre |
//! | [`intent`] | Catálogo cerrado de intenciones y sus esquemas |
//! | [`receipt`] | Recibos de deshacer con detección de caducidad |
//! | [`audit`] | Registro auditable en `agent_audit_log` |
//! | [`executor`] | Resolución y ejecución transaccional |
//! | [`bridge`] | API pública que consumen los comandos Tauri |
//!
//! ## Invariantes
//!
//! 1. El token en claro solo existe en el momento de emitirlo. La base guarda
//!    `pbkdf2-sha256$<iteraciones>$<sal>$<resumen>`.
//! 2. El ámbito se comprueba **antes** de resolver nombres y antes de escribir.
//! 3. Cada petición deja exactamente una fila en `agent_audit_log`.
//! 4. Un cambio y su fila de auditoría se escriben en la misma transacción.
//! 5. Toda escritura devuelve un `undo_token` de un solo uso.
//! 6. Ante un nombre ambiguo se pregunta; nunca se adivina.
//! 7. La confirmación humana ocurre en Vindexa, no en el agente.

pub mod audit;
pub mod bridge;
pub mod clients;
pub mod crypto;
pub mod executor;
pub mod intent;
pub mod matching;
pub mod ratelimit;
pub mod receipt;
pub mod scope;
pub mod token;

#[cfg(test)]
mod tests;

// Superficie pública del puente. `AffectedGame`, `AgentScope` y `ScopeSet`
// forman parte del contrato aunque hoy solo los nombren las pruebas: quien
// integre el puente los necesita para tipar sus llamadas.
#[allow(unused_imports, reason = "contrato público del módulo")]
pub use audit::AffectedGame;
pub use audit::AgentAuditEntry;
pub use bridge::{AgentOutcome, AgentRequest, Requester};
pub use clients::{AgentClientSummary, IssuedAgentClient, NewAgentClient};
#[allow(unused_imports, reason = "contrato público del módulo")]
pub use scope::{AgentScope, ScopeSet};
pub use token::TokenPolicy;
