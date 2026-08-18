//! Emisión y verificación de los tokens de los clientes agente.
//!
//! El token en claro **nunca** se persiste. `agent_clients.token_hash` guarda
//! una cadena autodescriptiva con el algoritmo, el número de iteraciones, la
//! sal y el resumen derivado, con el mismo espíritu que el formato PHC:
//!
//! ```text
//! pbkdf2-sha256$120000$<sal en hexadecimal>$<resumen en hexadecimal>
//! ```
//!
//! El token que recibe el agente tiene la forma `vdx_<id de cliente>_<secreto>`.
//! El identificador viaja en claro a propósito: permite localizar una sola fila
//! y evitar un recorrido de toda la tabla comparando resúmenes. El
//! identificador no es un secreto; el secreto son los 32 bytes finales.

use crate::agent::crypto::{
    SHA256_OUTPUT_BYTES, constant_time_eq, from_hex, pbkdf2_hmac_sha256, random_bytes, to_hex,
};
use crate::error::{AppError, AppResult};
use uuid::Uuid;

/// Prefijo obligatorio de todo token emitido por Vindexa.
pub const TOKEN_PREFIX: &str = "vdx";
/// Bytes de entropía del secreto. 256 bits de un CSPRNG del sistema.
pub const SECRET_BYTES: usize = 32;
/// Bytes de sal por cliente.
pub const SALT_BYTES: usize = 16;
/// Iteraciones PBKDF2 en producción.
///
/// El secreto tiene 256 bits de entropía real, así que el KDF es defensa en
/// profundidad frente a una copia robada de `vindexa.sqlite3`, no la barrera
/// principal. 120 000 iteraciones mantienen la verificación por debajo de la
/// décima de segundo en un portátil moderno compilado en modo release.
pub const DEFAULT_ITERATIONS: u32 = 120_000;
/// Iteraciones mínimas aceptadas al leer un `token_hash` existente.
const MIN_ITERATIONS: u32 = 1_000;
/// Iteraciones máximas aceptadas. Evita que una fila manipulada convierta una
/// verificación en una denegación de servicio.
const MAX_ITERATIONS: u32 = 5_000_000;

const ALGORITHM: &str = "pbkdf2-sha256";

/// Coste de derivación aplicado al emitir un token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenPolicy {
    pub iterations: u32,
}

impl Default for TokenPolicy {
    fn default() -> Self {
        Self {
            iterations: DEFAULT_ITERATIONS,
        }
    }
}

impl TokenPolicy {
    /// Coste reducido para las pruebas. No debe usarse en producción: existe
    /// para que la batería de tests no dependa del rendimiento del KDF.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            iterations: MIN_ITERATIONS,
        }
    }
}

/// Token recién emitido: identificador del cliente, secreto en claro y el
/// resumen que se persiste.
#[derive(Debug, Clone)]
pub struct MintedToken {
    pub client_id: String,
    pub plaintext: String,
    pub hash: String,
}

/// Genera un identificador de cliente y su token.
pub fn mint(policy: TokenPolicy) -> MintedToken {
    let client_id = Uuid::new_v4().to_string();
    mint_for_client(&client_id, policy)
}

/// Genera un token nuevo para un cliente ya existente (rotación).
pub fn mint_for_client(client_id: &str, policy: TokenPolicy) -> MintedToken {
    let secret = random_bytes(SECRET_BYTES);
    let secret_hex = to_hex(&secret);
    let hash = derive(&secret_hex, policy.iterations);
    MintedToken {
        client_id: client_id.to_string(),
        plaintext: format!("{TOKEN_PREFIX}_{client_id}_{secret_hex}"),
        hash,
    }
}

/// Deriva el resumen persistible de un secreto con una sal nueva.
fn derive(secret_hex: &str, iterations: u32) -> String {
    let salt = random_bytes(SALT_BYTES);
    let mut derived = [0u8; SHA256_OUTPUT_BYTES];
    pbkdf2_hmac_sha256(secret_hex.as_bytes(), &salt, iterations, &mut derived);
    format!(
        "{ALGORITHM}${iterations}${}${}",
        to_hex(&salt),
        to_hex(&derived)
    )
}

/// Partes útiles de un token presentado por un agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedToken {
    pub client_id: String,
    pub secret_hex: String,
}

/// Descompone `vdx_<id>_<secreto>` sin consultar la base de datos.
pub fn parse(token: &str) -> AppResult<ParsedToken> {
    let token = token.trim();
    if token.len() > 512 {
        return Err(invalid_token());
    }
    let mut parts = token.splitn(3, '_');
    let prefix = parts.next().unwrap_or_default();
    let client_id = parts.next().unwrap_or_default();
    let secret_hex = parts.next().unwrap_or_default();
    if prefix != TOKEN_PREFIX || client_id.is_empty() || secret_hex.is_empty() {
        return Err(invalid_token());
    }
    if Uuid::parse_str(client_id).is_err() {
        return Err(invalid_token());
    }
    if secret_hex.len() != SECRET_BYTES * 2 || from_hex(secret_hex).is_none() {
        return Err(invalid_token());
    }
    Ok(ParsedToken {
        client_id: client_id.to_string(),
        secret_hex: secret_hex.to_string(),
    })
}

/// Comprueba un secreto contra el resumen almacenado.
///
/// Devuelve `false` ante cualquier resumen malformado en lugar de propagar un
/// error: una fila corrupta no debe distinguirse de un token equivocado.
pub fn verify(secret_hex: &str, stored: &str) -> bool {
    let mut fields = stored.split('$');
    let algorithm = fields.next().unwrap_or_default();
    let iterations = fields.next().unwrap_or_default();
    let salt = fields.next().unwrap_or_default();
    let expected = fields.next().unwrap_or_default();
    if fields.next().is_some() || algorithm != ALGORITHM {
        return false;
    }
    let Ok(iterations) = iterations.parse::<u32>() else {
        return false;
    };
    if !(MIN_ITERATIONS..=MAX_ITERATIONS).contains(&iterations) {
        return false;
    }
    let (Some(salt), Some(expected)) = (from_hex(salt), from_hex(expected)) else {
        return false;
    };
    if salt.len() != SALT_BYTES || expected.len() != SHA256_OUTPUT_BYTES {
        return false;
    }
    let mut derived = [0u8; SHA256_OUTPUT_BYTES];
    pbkdf2_hmac_sha256(secret_hex.as_bytes(), &salt, iterations, &mut derived);
    constant_time_eq(&derived, &expected)
}

/// Token opaco de un solo uso para deshacer una acción aplicada.
pub fn mint_undo_token() -> String {
    to_hex(&random_bytes(SECRET_BYTES))
}

pub(crate) fn invalid_token() -> AppError {
    AppError::new(
        "agent_token",
        "El token del agente no es válido o ha sido revocado.",
    )
}
