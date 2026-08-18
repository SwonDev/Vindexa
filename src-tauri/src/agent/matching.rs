//! Resolución de juegos a partir de un nombre en lenguaje natural.
//!
//! Es la pieza más delicada del puente. Un agente que modifica el juego
//! equivocado hace un daño silencioso y difícil de detectar; uno que pregunta
//! solo cuesta un mensaje más. Por eso el criterio es deliberadamente
//! conservador: solo se resuelve sin preguntar cuando la mejor coincidencia es
//! buena **y** además saca una distancia clara a la segunda.
//!
//! ## Cómo puntúa
//!
//! 1. **Normalización** ([`normalize`]): minúsculas, sin tildes ni diéresis,
//!    `ñ` a `n`, signos convertidos en separadores, espacios colapsados y
//!    números romanos sueltos (`ii`, `iv`, `x`…) convertidos a cifras. Así
//!    «Pokémon Rojo», «pokemon rojo» y «POKEMON, ROJO» son la misma cadena, y
//!    «Final Fantasy VII» encuentra a «Final Fantasy 7».
//! 2. **Similitud de caracteres**: distancia de edición con transposiciones,
//!    normalizada y medida
//!    también sobre las cadenas sin espacios. Eso captura los errores de
//!    tecleo y las palabras pegadas o separadas —«DragonsWord Awakening» frente
//!    a «Dragon's Word: Awakening»—.
//! 3. **Similitud de palabras**: cada palabra de la consulta busca su mejor
//!    pareja entre las del título; la cobertura resultante se penaliza
//!    suavemente cuando el título arrastra muchas palabras que la consulta no
//!    menciona. Esa penalización es la que hace que «portal» prefiera «Portal»
//!    antes que «Portal 2».
//! 4. **Refuerzo por prefijo o contención**, para consultas cortas que son el
//!    principio literal de un título.
//!
//! No hay dependencias nuevas: la distancia de edición está implementada aquí.

use serde::{Deserialize, Serialize};

/// Puntuación mínima para aceptar una coincidencia sin preguntar.
pub const ACCEPT_THRESHOLD: f64 = 0.86;
/// Puntuación mínima para ofrecer un título como candidato.
pub const CANDIDATE_THRESHOLD: f64 = 0.52;
/// Distancia mínima entre la primera y la segunda candidata para no preguntar.
pub const REQUIRED_MARGIN: f64 = 0.08;
/// Número máximo de candidatos devueltos al agente.
pub const MAX_CANDIDATES: usize = 5;
/// Longitud máxima considerada al calcular distancias de edición.
const MAX_COMPARED_CHARS: usize = 96;
/// Similitud mínima para considerar que dos palabras son la misma.
const WORD_MATCH_THRESHOLD: f64 = 0.62;

/// Un juego candidato con su puntuación.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameCandidate {
    pub app_id: u32,
    pub title: String,
    /// Puntuación entre 0 y 1, redondeada a tres decimales.
    pub score: f64,
}

/// Resultado de resolver un nombre.
#[derive(Debug, Clone, PartialEq)]
pub enum GameMatch {
    /// Coincidencia inequívoca.
    Resolved(GameCandidate),
    /// Varias opciones razonables: hay que preguntar.
    Ambiguous(Vec<GameCandidate>),
    /// Nada se parece lo suficiente.
    NotFound,
}

/// Entrada del índice sobre el que se busca.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameIndexEntry {
    pub app_id: u32,
    pub title: String,
}

/// Resuelve un nombre contra un catálogo de juegos.
pub fn resolve(query: &str, catalog: &[GameIndexEntry]) -> GameMatch {
    let normalized_query = normalize(query);
    if normalized_query.is_empty() {
        return GameMatch::NotFound;
    }

    let mut scored = catalog
        .iter()
        .map(|entry| {
            let score = score(&normalized_query, &normalize(&entry.title));
            GameCandidate {
                app_id: entry.app_id,
                title: entry.title.clone(),
                score: round(score),
            }
        })
        .filter(|candidate| candidate.score >= CANDIDATE_THRESHOLD)
        .collect::<Vec<_>>();

    // Orden determinista: puntuación descendente y, a igualdad, título y AppID.
    scored.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.app_id.cmp(&right.app_id))
    });

    let Some(best) = scored.first().cloned() else {
        return GameMatch::NotFound;
    };
    let runner_up = scored.get(1).map(|candidate| candidate.score);
    let margin = best.score - runner_up.unwrap_or(0.0);
    if best.score >= ACCEPT_THRESHOLD && margin >= REQUIRED_MARGIN {
        return GameMatch::Resolved(best);
    }
    scored.truncate(MAX_CANDIDATES);
    GameMatch::Ambiguous(scored)
}

/// Puntuación de similitud entre dos cadenas ya normalizadas.
pub fn score(query: &str, title: &str) -> f64 {
    if query.is_empty() || title.is_empty() {
        return 0.0;
    }
    if query == title {
        return 1.0;
    }

    let query_tight: String = query
        .chars()
        .filter(|character| *character != ' ')
        .collect();
    let title_tight: String = title
        .chars()
        .filter(|character| *character != ' ')
        .collect();
    if query_tight == title_tight {
        // Solo cambia el espaciado: «dragonsword» frente a «dragons word».
        return 0.99;
    }

    let character_similarity = ratio(query, title).max(ratio(&query_tight, &title_tight));
    let word_similarity = word_score(query, title);
    let mut total = 0.45 * character_similarity + 0.55 * word_similarity;

    if title.starts_with(query) || query.starts_with(title) {
        total = total.max(0.88);
    } else if query.chars().count() >= 4 && title.contains(query) {
        total = total.max(0.80);
    }
    total.clamp(0.0, 1.0)
}

/// Cobertura de las palabras de la consulta dentro del título.
fn word_score(query: &str, title: &str) -> f64 {
    let query_words = query
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let title_words = title
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if query_words.is_empty() || title_words.is_empty() {
        return 0.0;
    }

    let mut used = vec![false; title_words.len()];
    let mut coverage = 0.0;
    for query_word in &query_words {
        let mut best_ratio = 0.0;
        let mut best_index = None;
        for (index, title_word) in title_words.iter().enumerate() {
            if used[index] {
                continue;
            }
            let candidate = ratio(query_word, title_word);
            if candidate > best_ratio {
                best_ratio = candidate;
                best_index = Some(index);
            }
        }
        // Por debajo de este umbral dos palabras no son la misma palabra mal
        // escrita. Es holgado a propósito: las palabras cortas pagan caro cada
        // error («vally» frente a «valley» ya vale 0.83).
        if best_ratio >= WORD_MATCH_THRESHOLD
            && let Some(index) = best_index
        {
            used[index] = true;
            coverage += best_ratio;
        }
    }

    let coverage = coverage / query_words.len() as f64;
    let matched = used.iter().filter(|slot| **slot).count() as f64;
    // Penalización suave por las palabras del título que la consulta no cubre.
    let completeness = matched / title_words.len() as f64;
    coverage * (0.72 + 0.28 * completeness)
}

/// Similitud 0..1 derivada de la distancia de edición.
fn ratio(left: &str, right: &str) -> f64 {
    let left: Vec<char> = left.chars().take(MAX_COMPARED_CHARS).collect();
    let right: Vec<char> = right.chars().take(MAX_COMPARED_CHARS).collect();
    let longest = left.len().max(right.len());
    if longest == 0 {
        return 0.0;
    }
    let distance = edit_distance(&left, &right) as f64;
    1.0 - distance / longest as f64
}

/// Distancia de edición con transposición de caracteres adyacentes
/// (alineamiento óptimo de cadenas, la variante restringida de
/// Damerau-Levenshtein).
///
/// La transposición cuenta como **un** error, no como dos. Sin eso, «Knigth»
/// por «Knight» queda por debajo del umbral y el agente acabaría preguntando
/// por la errata de tecleo más común que existe.
fn edit_distance(left: &[char], right: &[char]) -> usize {
    let (rows, columns) = (left.len(), right.len());
    if rows == 0 {
        return columns;
    }
    if columns == 0 {
        return rows;
    }
    let mut two_back = vec![0usize; columns + 1];
    let mut previous: Vec<usize> = (0..=columns).collect();
    let mut current = vec![0usize; columns + 1];

    for row in 1..=rows {
        current[0] = row;
        for column in 1..=columns {
            let cost = usize::from(left[row - 1] != right[column - 1]);
            let mut best = (previous[column] + 1)
                .min(current[column - 1] + 1)
                .min(previous[column - 1] + cost);
            if row > 1
                && column > 1
                && left[row - 1] == right[column - 2]
                && left[row - 2] == right[column - 1]
            {
                best = best.min(two_back[column - 2] + 1);
            }
            current[column] = best;
        }
        std::mem::swap(&mut two_back, &mut previous);
        std::mem::swap(&mut previous, &mut current);
    }
    previous[columns]
}

/// Normaliza un título o una consulta para poder compararlos.
pub fn normalize(value: &str) -> String {
    let mut folded = String::with_capacity(value.len());
    for character in value.chars() {
        match fold(character) {
            Some(replacement) => folded.push_str(replacement),
            None => folded.push(' '),
        }
    }

    folded
        .split_whitespace()
        .map(roman_to_digits)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Traduce un carácter a su forma comparable, o `None` si actúa de separador.
fn fold(character: char) -> Option<&'static str> {
    // Tabla explícita en lugar de una normalización Unicode completa: cubre el
    // castellano y los idiomas de los títulos que aparecen en Steam sin añadir
    // la dependencia `unicode-normalization`.
    let lowered = character.to_lowercase().next().unwrap_or(character);
    Some(match lowered {
        'a'..='z' | '0'..='9' => return Some(single(lowered)),
        'á' | 'à' | 'ä' | 'â' | 'ã' | 'å' | 'ā' => "a",
        'é' | 'è' | 'ë' | 'ê' | 'ē' => "e",
        'í' | 'ì' | 'ï' | 'î' | 'ī' => "i",
        'ó' | 'ò' | 'ö' | 'ô' | 'õ' | 'ø' | 'ō' => "o",
        'ú' | 'ù' | 'ü' | 'û' | 'ū' => "u",
        'ñ' => "n",
        'ç' => "c",
        'ý' | 'ÿ' => "y",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        'ł' => "l",
        'š' => "s",
        'ž' => "z",
        '&' => " and ",
        _ => return None,
    })
}

/// Devuelve la letra ASCII como cadena estática sin asignar memoria.
fn single(character: char) -> &'static str {
    const ALPHABET: [&str; 36] = [
        "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f", "g", "h",
        "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    ];
    let index = match character {
        '0'..='9' => character as usize - '0' as usize,
        _ => 10 + (character as usize - 'a' as usize),
    };
    ALPHABET.get(index).copied().unwrap_or(" ")
}

/// Convierte una palabra que sea exactamente un número romano del 1 al 20.
fn roman_to_digits(word: &str) -> String {
    const ROMAN: [(&str, &str); 20] = [
        ("i", "1"),
        ("ii", "2"),
        ("iii", "3"),
        ("iv", "4"),
        ("v", "5"),
        ("vi", "6"),
        ("vii", "7"),
        ("viii", "8"),
        ("ix", "9"),
        ("x", "10"),
        ("xi", "11"),
        ("xii", "12"),
        ("xiii", "13"),
        ("xiv", "14"),
        ("xv", "15"),
        ("xvi", "16"),
        ("xvii", "17"),
        ("xviii", "18"),
        ("xix", "19"),
        ("xx", "20"),
    ];
    // «I» sola casi nunca es un número: se deja tal cual para no romper títulos
    // en inglés donde es un pronombre.
    if word == "i" {
        return word.to_string();
    }
    ROMAN
        .iter()
        .find(|(roman, _)| *roman == word)
        .map(|(_, digits)| (*digits).to_string())
        .unwrap_or_else(|| word.to_string())
}

fn round(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}
