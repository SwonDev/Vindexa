//! Primitivas criptográficas mínimas para el puente de agentes.
//!
//! ## Por qué están escritas aquí
//!
//! `src-tauri/Cargo.toml` no incluye ninguna dependencia de hash o de derivación
//! de claves: no hay `sha2`, `hmac`, `pbkdf2`, `argon2` ni `ring`. `keyring`
//! guarda secretos en el almacén del sistema, pero no expone un KDF, y SQLite
//! empaquetado no trae funciones de hash. Como esta fase no puede añadir
//! dependencias, aquí viven implementaciones directas de SHA-256, HMAC-SHA256 y
//! PBKDF2-HMAC-SHA256 según FIPS 180-4, RFC 2104 y RFC 8018.
//!
//! ## Límites declarados
//!
//! - **Es criptografía escrita a mano.** Está verificada contra los vectores
//!   oficiales (FIPS 180-4, RFC 4231, RFC 7914) en `agent::tests`, pero no ha
//!   sido auditada por terceros ni endurecida frente a ataques de canal lateral
//!   más allá de la comparación en tiempo constante de [`constant_time_eq`].
//! - **PBKDF2 no es memory-hard.** Argon2id sería preferible frente a un ataque
//!   por GPU. Aquí el atacante tendría que enfrentarse además a un secreto de
//!   256 bits de entropía real, no a una contraseña humana, así que el coste de
//!   fuerza bruta lo domina la entropía y no el KDF.
//! - La recomendación para una fase posterior es sustituir este módulo por las
//!   crates `argon2` y `subtle`. El informe de integración recoge el diff.

/// Constantes de ronda de SHA-256 (FIPS 180-4, §4.2.2).
const ROUND_CONSTANTS: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Estado inicial de SHA-256 (FIPS 180-4, §5.3.3).
const INITIAL_STATE: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Tamaño de bloque de SHA-256 en bytes. HMAC lo necesita para el relleno.
pub const SHA256_BLOCK_BYTES: usize = 64;
/// Tamaño del resumen de SHA-256 en bytes.
pub const SHA256_OUTPUT_BYTES: usize = 32;

/// Estado incremental de SHA-256.
#[derive(Debug, Clone)]
pub struct Sha256 {
    state: [u32; 8],
    block: [u8; SHA256_BLOCK_BYTES],
    filled: usize,
    total_bits: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self {
            state: INITIAL_STATE,
            block: [0; SHA256_BLOCK_BYTES],
            filled: 0,
            total_bits: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_bits = self.total_bits.wrapping_add((data.len() as u64) * 8);
        if self.filled > 0 {
            let missing = SHA256_BLOCK_BYTES - self.filled;
            let take = missing.min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == SHA256_BLOCK_BYTES {
                let block = self.block;
                self.compress(&block);
                self.filled = 0;
            }
        }
        while data.len() >= SHA256_BLOCK_BYTES {
            let (head, rest) = data.split_at(SHA256_BLOCK_BYTES);
            let mut block = [0u8; SHA256_BLOCK_BYTES];
            block.copy_from_slice(head);
            self.compress(&block);
            data = rest;
        }
        if !data.is_empty() {
            self.block[..data.len()].copy_from_slice(data);
            self.filled = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; SHA256_OUTPUT_BYTES] {
        let total_bits = self.total_bits;
        // Relleno: un bit a uno, ceros y la longitud en bits big-endian.
        self.update_without_length(&[0x80]);
        while self.filled != 56 {
            self.update_without_length(&[0x00]);
        }
        let mut tail = [0u8; 8];
        tail.copy_from_slice(&total_bits.to_be_bytes());
        self.update_without_length(&tail);

        let mut digest = [0u8; SHA256_OUTPUT_BYTES];
        for (index, word) in self.state.iter().enumerate() {
            digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    fn update_without_length(&mut self, data: &[u8]) {
        for byte in data {
            self.block[self.filled] = *byte;
            self.filled += 1;
            if self.filled == SHA256_BLOCK_BYTES {
                let block = self.block;
                self.compress(&block);
                self.filled = 0;
            }
        }
    }

    fn compress(&mut self, block: &[u8; SHA256_BLOCK_BYTES]) {
        let mut schedule = [0u32; 64];
        for index in 0..16 {
            schedule[index] = u32::from_be_bytes([
                block[index * 4],
                block[index * 4 + 1],
                block[index * 4 + 2],
                block[index * 4 + 3],
            ]);
        }
        for index in 16..64 {
            let previous = schedule[index - 15];
            let ahead = schedule[index - 2];
            let sigma0 = previous.rotate_right(7) ^ previous.rotate_right(18) ^ (previous >> 3);
            let sigma1 = ahead.rotate_right(17) ^ ahead.rotate_right(19) ^ (ahead >> 10);
            schedule[index] = schedule[index - 16]
                .wrapping_add(sigma0)
                .wrapping_add(schedule[index - 7])
                .wrapping_add(sigma1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for index in 0..64 {
            let big_sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(big_sigma1)
                .wrapping_add(choose)
                .wrapping_add(ROUND_CONSTANTS[index])
                .wrapping_add(schedule[index]);
            let big_sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = big_sigma0.wrapping_add(majority);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        let round = [a, b, c, d, e, f, g, h];
        for (slot, value) in self.state.iter_mut().zip(round) {
            *slot = slot.wrapping_add(value);
        }
    }
}

/// Resumen SHA-256 de un mensaje completo.
pub fn sha256(message: &[u8]) -> [u8; SHA256_OUTPUT_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(message);
    hasher.finish()
}

/// HMAC-SHA256 según RFC 2104.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; SHA256_OUTPUT_BYTES] {
    let mut normalized = [0u8; SHA256_BLOCK_BYTES];
    if key.len() > SHA256_BLOCK_BYTES {
        normalized[..SHA256_OUTPUT_BYTES].copy_from_slice(&sha256(key));
    } else {
        normalized[..key.len()].copy_from_slice(key);
    }

    let mut inner_pad = [0x36u8; SHA256_BLOCK_BYTES];
    let mut outer_pad = [0x5cu8; SHA256_BLOCK_BYTES];
    for index in 0..SHA256_BLOCK_BYTES {
        inner_pad[index] ^= normalized[index];
        outer_pad[index] ^= normalized[index];
    }

    let mut inner = Sha256::new();
    inner.update(&inner_pad);
    inner.update(message);
    let inner_digest = inner.finish();

    let mut outer = Sha256::new();
    outer.update(&outer_pad);
    outer.update(&inner_digest);
    outer.finish()
}

/// PBKDF2-HMAC-SHA256 según RFC 8018, §5.2.
///
/// `iterations` debe ser mayor que cero; `output` recibe la clave derivada con
/// la longitud que ya tenga el segmento.
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8]) {
    debug_assert!(iterations > 0, "PBKDF2 exige al menos una iteración");
    let iterations = iterations.max(1);
    let mut block_index: u32 = 1;
    let mut written = 0usize;

    while written < output.len() {
        let mut salted = Vec::with_capacity(salt.len() + 4);
        salted.extend_from_slice(salt);
        salted.extend_from_slice(&block_index.to_be_bytes());

        let mut previous = hmac_sha256(password, &salted);
        let mut accumulator = previous;
        for _ in 1..iterations {
            previous = hmac_sha256(password, &previous);
            for (slot, value) in accumulator.iter_mut().zip(previous) {
                *slot ^= value;
            }
        }

        let remaining = output.len() - written;
        let take = remaining.min(SHA256_OUTPUT_BYTES);
        output[written..written + take].copy_from_slice(&accumulator[..take]);
        written += take;
        block_index += 1;
    }
}

/// Comparación en tiempo constante respecto al contenido.
///
/// La longitud sí se filtra, igual que en `subtle`: los resúmenes que compara
/// esta aplicación tienen siempre el mismo tamaño.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Bytes aleatorios de calidad criptográfica.
///
/// `uuid` v4 obtiene su aleatoriedad de `getrandom`, que consulta el CSPRNG del
/// sistema operativo (`getentropy` en macOS, `getrandom(2)` en Linux,
/// `BCryptGenRandom` en Windows). Es la única fuente de entropía ya presente en
/// el árbol de dependencias fijado en `Cargo.lock`.
pub fn random_bytes(length: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(length);
    while output.len() < length {
        let chunk = *uuid::Uuid::new_v4().as_bytes();
        let remaining = length - output.len();
        output.extend_from_slice(&chunk[..remaining.min(chunk.len())]);
    }
    output
}

/// Codifica en hexadecimal minúscula.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        text.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    text
}

/// Decodifica hexadecimal. Devuelve `None` ante cualquier entrada malformada.
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let raw = text.as_bytes();
    let mut output = Vec::with_capacity(raw.len() / 2);
    // `as_chunks` entrega parejas ya formadas —`[u8; 2]`—, así que no hay que
    // indexar dentro de cada una. El resto sobra siempre: la longitud se
    // comprobó múltiplo de dos ahí arriba.
    let (parejas, _resto) = raw.as_chunks::<2>();
    for [alto, bajo] in parejas {
        let high = (*alto as char).to_digit(16)?;
        let low = (*bajo as char).to_digit(16)?;
        output.push(((high << 4) | low) as u8);
    }
    Some(output)
}
