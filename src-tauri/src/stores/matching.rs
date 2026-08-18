//! Emparejado conservador entre un juego de una tienda externa y uno de la
//! biblioteca de Steam.
//!
//! # Principio rector
//!
//! **Un emparejado falso es peor que ninguno.** Ver «Fallout 3» de GOG unido por
//! error a «Fallout 4» de Steam destruye la confianza en toda la biblioteca;
//! verlo sin emparejar sólo obliga a un clic. Por eso el algoritmo prefiere
//! siempre el `NULL`.
//!
//! # El algoritmo, en orden
//!
//! 1. **Normalización** ([`normalize_title`]): minúsculas, sin diacríticos, sin
//!    signos, `&` → `and`, numerales romanos → dígitos, y se retiran los
//!    sufijos de edición conocidos («Game of the Year Edition», «Definitive
//!    Edition», «Remastered», «Complete Edition»…). Dos ediciones del mismo
//!    juego colapsan al mismo texto.
//!
//! 2. **Guarda numérica dura** ([`discriminant_numbers`]): se extraen todos los
//!    tokens numéricos —incluidos los años entre paréntesis y los numerales
//!    romanos ya convertidos— de ambos títulos. Si los dos conjuntos no son
//!    idénticos, **se rechaza sin puntuar**. Ésta es la regla que separa
//!    «Fallout 3» de «Fallout 4», «DOOM (1993)» de «DOOM (2016)» y «DOOM II» de
//!    «DOOM»: la similitud textual entre esos pares es altísima y ninguna
//!    métrica de cadenas los distinguiría por sí sola.
//!
//! 3. **Coincidencia exacta tras normalizar** → confianza [`EXACT_CONFIDENCE`].
//!    Es el camino real dominante: «Fallout 3» y «Fallout 3: Game of the Year
//!    Edition» acaban siendo la misma cadena.
//!
//! 4. **Puntuación combinada** para el resto: `0,6 · Jaro-Winkler + 0,4 ·
//!    Jaccard de tokens`. Jaro-Winkler capta erratas y variaciones de escritura;
//!    el Jaccard castiga que a un título le sobren o le falten palabras enteras,
//!    que es justo el fallo que Jaro-Winkler comete solo («Fallout» contra
//!    «Fallout Shelter» le parecen un 0,89).
//!
//! 5. **Umbral** [`MATCH_THRESHOLD`] = 0,92, y además **desempate obligatorio**:
//!    si dos candidatos de Steam quedan a menos de [`AMBIGUITY_MARGIN`] el uno
//!    del otro, no se empareja ninguno. Un empate no es una respuesta.
//!
//! Ninguna de estas reglas usa dependencias externas: Jaro-Winkler está
//! implementado aquí.

use std::collections::BTreeSet;

/// Confianza que se registra cuando los títulos normalizados coinciden
/// exactamente. Se queda deliberadamente por debajo de 1,0 porque **1,0 está
/// reservado a las decisiones tomadas por una persona** (ver `stores::db`).
pub const EXACT_CONFIDENCE: f64 = 0.99;

/// Confianza mínima para escribir un `matched_app_id`.
///
/// 0,92 es alto a propósito. Con la fórmula combinada, superarlo exige que los
/// dos títulos compartan prácticamente el mismo conjunto de palabras y difieran
/// como mucho en detalles de escritura. Los pares del mismo juego llegan casi
/// siempre por la vía exacta (paso 3), así que subir el listón del camino difuso
/// no pierde emparejados legítimos y sí evita los falsos.
pub const MATCH_THRESHOLD: f64 = 0.92;

/// Distancia mínima que debe separar al mejor candidato del segundo. Si dos
/// juegos de Steam puntúan casi igual, la respuesta honesta es «no se sabe».
pub const AMBIGUITY_MARGIN: f64 = 0.03;

/// Peso de la similitud de cadena frente a la de conjunto de palabras.
const STRING_WEIGHT: f64 = 0.6;
const TOKEN_WEIGHT: f64 = 0.4;

/// Longitud máxima del prefijo común que premia Jaro-Winkler, según la
/// definición original de Winkler.
const WINKLER_PREFIX_LIMIT: usize = 4;
/// Factor de escala del premio de prefijo, también el valor clásico.
const WINKLER_SCALE: f64 = 0.1;
/// Jaro mínimo por debajo del cual Winkler no aplica premio de prefijo.
const WINKLER_BOOST_THRESHOLD: f64 = 0.7;

/// Sufijos de edición que no distinguen un producto de otro. Se comparan sobre
/// el título ya normalizado y sólo se retiran cuando aparecen al final.
///
/// El orden importa: los más largos primero, para que «game of the year
/// edition» se retire entero antes de que «edition» se lleve sólo la última
/// palabra.
const EDITION_SUFFIXES: &[&str] = &[
    "game of the year edition",
    "game of the year",
    "goty edition",
    "complete collection",
    "definitive edition",
    "anniversary edition",
    "collectors edition",
    "collector s edition",
    "commemorative edition",
    "champions edition",
    "legendary edition",
    "remastered edition",
    "enhanced edition",
    "complete edition",
    "ultimate edition",
    "director s cut",
    "directors cut",
    "special edition",
    "premium edition",
    "standard edition",
    "digital edition",
    "deluxe edition",
    "gold edition",
    "royal edition",
    "final cut",
    "the final cut",
    "redux",
    "remastered",
    "remaster",
    "goty",
    "hd edition",
    "edition",
];

/// Prefijos que las tiendas anteponen y que no forman parte del nombre.
const LEADING_ARTICLES: &[&str] = &["the ", "a ", "an "];

/// Un candidato de la biblioteca de Steam.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchCandidate {
    pub app_id: u32,
    pub title: String,
}

/// Decisión de emparejado, con la puntuación que la justifica.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchDecision {
    pub app_id: u32,
    pub confidence: f64,
}

/// Índice precalculado de la biblioteca de Steam, para no volver a normalizar
/// cada título por cada juego externo.
#[derive(Debug, Clone, Default)]
pub struct SteamTitleIndex {
    entries: Vec<IndexedTitle>,
}

#[derive(Debug, Clone)]
struct IndexedTitle {
    app_id: u32,
    normalized: String,
    tokens: BTreeSet<String>,
    numbers: BTreeSet<String>,
}

impl SteamTitleIndex {
    pub fn build(candidates: &[MatchCandidate]) -> Self {
        let entries = candidates
            .iter()
            .filter_map(|candidate| {
                let normalized = normalize_title(&candidate.title);
                if normalized.is_empty() {
                    return None;
                }
                Some(IndexedTitle {
                    app_id: candidate.app_id,
                    tokens: token_set(&normalized),
                    numbers: discriminant_numbers(&normalized),
                    normalized,
                })
            })
            .collect();
        Self { entries }
    }

    /// Busca el mejor emparejado para un título externo.
    ///
    /// Devuelve `None` cuando no hay ningún candidato por encima del umbral o
    /// cuando los dos mejores están demasiado cerca para decidir.
    pub fn best_match(&self, external_title: &str) -> Option<MatchDecision> {
        let normalized = normalize_title(external_title);
        if normalized.is_empty() {
            return None;
        }
        let numbers = discriminant_numbers(&normalized);
        let tokens = token_set(&normalized);

        let mut best: Option<MatchDecision> = None;
        let mut runner_up = 0.0_f64;

        for entry in &self.entries {
            // Guarda numérica dura: sin esto, «Fallout 3» y «Fallout 4»
            // puntuarían por encima de cualquier umbral razonable.
            if entry.numbers != numbers {
                continue;
            }
            let confidence = if entry.normalized == normalized {
                EXACT_CONFIDENCE
            } else {
                let string_similarity = jaro_winkler(&normalized, &entry.normalized);
                let token_similarity = jaccard(&tokens, &entry.tokens);
                STRING_WEIGHT * string_similarity + TOKEN_WEIGHT * token_similarity
            };
            match best {
                Some(current) if confidence <= current.confidence => {
                    if confidence > runner_up {
                        runner_up = confidence;
                    }
                }
                Some(current) => {
                    runner_up = runner_up.max(current.confidence);
                    best = Some(MatchDecision {
                        app_id: entry.app_id,
                        confidence,
                    });
                }
                None => {
                    best = Some(MatchDecision {
                        app_id: entry.app_id,
                        confidence,
                    });
                }
            }
        }

        let best = best?;
        if best.confidence < MATCH_THRESHOLD {
            return None;
        }
        // Un empate técnico entre dos juegos distintos de Steam no se resuelve
        // eligiendo uno: se deja sin emparejar.
        if best.confidence - runner_up < AMBIGUITY_MARGIN {
            return None;
        }
        Some(best)
    }
}

// ---------------------------------------------------------------------------
// Normalización
// ---------------------------------------------------------------------------

/// Reduce un título comercial a su forma comparable.
pub fn normalize_title(value: &str) -> String {
    let folded = fold_and_simplify(value);
    let with_digits = romans_to_digits(&folded);
    strip_edition_suffixes(&strip_leading_article(&with_digits))
}

/// Minúsculas, sin diacríticos, sin signos y con los espacios colapsados.
///
/// Todo lo que no sea alfanumérico ASCII tras el plegado actúa como separador,
/// **salvo el apóstrofo**, que une: «Assassin's Creed» debe dar `assassins
/// creed` y no `assassin s creed`, porque las tiendas escriben ese apóstrofo de
/// tres formas distintas (`'`, `’`, `´`) para el mismo juego.
fn fold_and_simplify(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut pending_space = false;
    for character in value.chars() {
        if matches!(character, '\'' | '\u{2019}' | '\u{02BC}' | '´' | '`') {
            continue;
        }
        // Una tabla vacía significa «no hay plegado»: el carácter original
        // decide por sí mismo si es alfanumérico o separador.
        let folded = fold_character(character);
        let mut push = |folded_character: char| {
            if folded_character.is_ascii_alphanumeric() {
                if pending_space && !output.is_empty() {
                    output.push(' ');
                }
                pending_space = false;
                output.push(folded_character.to_ascii_lowercase());
            } else {
                pending_space = true;
            }
        };
        if folded.is_empty() {
            push(character);
        } else {
            for folded_character in folded.chars() {
                push(folded_character);
            }
        }
    }
    output
}

/// Tabla de plegado de los diacríticos que aparecen de verdad en títulos de
/// videojuegos. No se añade una dependencia de Unicode por esto: la lista está
/// acotada y es explícita.
///
/// Devuelve la cadena de reemplazo, o la cadena vacía cuando el carácter no
/// necesita plegado y debe usarse tal cual.
fn fold_character(character: char) -> &'static str {
    match character {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "A",
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => "c",
        'Ç' | 'Ć' | 'Č' | 'Ĉ' | 'Ċ' => "C",
        'ď' | 'đ' => "d",
        'Ď' | 'Đ' => "D",
        'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'É' | 'È' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => "E",
        'ģ' | 'ğ' | 'ĝ' | 'ġ' => "g",
        'Ģ' | 'Ğ' | 'Ĝ' | 'Ġ' => "G",
        'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'Í' | 'Ì' | 'Î' | 'Ï' | 'Ī' | 'Ĭ' | 'Į' | 'İ' => "I",
        'ĺ' | 'ļ' | 'ľ' | 'ł' => "l",
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ł' => "L",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' => "N",
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'ō' | 'ŏ' | 'ő' | 'ø' => "o",
        'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Ō' | 'Ŏ' | 'Ő' | 'Ø' => "O",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'Ŕ' | 'Ŗ' | 'Ř' => "R",
        'ś' | 'ş' | 'š' | 'ŝ' => "s",
        'Ś' | 'Ş' | 'Š' | 'Ŝ' => "S",
        'ţ' | 'ť' | 'ŧ' => "t",
        'Ţ' | 'Ť' | 'Ŧ' => "T",
        'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'Ú' | 'Ù' | 'Û' | 'Ü' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => "U",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'Ý' | 'Ÿ' | 'Ŷ' => "Y",
        'ź' | 'ż' | 'ž' => "z",
        'Ź' | 'Ż' | 'Ž' => "Z",
        'ß' => "ss",
        'æ' => "ae",
        'Æ' => "AE",
        'œ' => "oe",
        'Œ' => "OE",
        'þ' => "th",
        'Þ' => "TH",
        'ð' => "d",
        'Ð' => "D",
        // `&` y `+` unen palabras igual que «and»; sin esto «Rick & Morty» y
        // «Rick and Morty» no se parecerían.
        '&' => " and ",
        '+' => " plus ",
        // Los signos tipográficos que las tiendas usan como separador ya caen
        // por la vía general (no son alfanuméricos), así que no hace falta
        // enumerarlos.
        _ => "",
    }
}

/// Convierte numerales romanos a dígitos para que «Final Fantasy VII» y «Final
/// Fantasy 7» comparen igual.
///
/// La conversión sólo se aplica a numerales de dos o más letras, o a uno de una
/// sola letra cuando es la **última** palabra del título. Así «I Am Setsuna» no
/// se convierte en «1 am setsuna» pero «Final Fantasy V» sí llega a «final
/// fantasy 5». La regla se aplica idénticamente a los dos lados de la
/// comparación, así que aunque acierte de más nunca desalinea el emparejado.
fn romans_to_digits(value: &str) -> String {
    let tokens = value.split(' ').collect::<Vec<_>>();
    let last_index = tokens.len().saturating_sub(1);
    tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            let eligible = token.len() >= 2 || index == last_index;
            if !eligible {
                return (*token).to_string();
            }
            match roman_value(token) {
                Some(value) => value.to_string(),
                None => (*token).to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Valor de un numeral romano en minúsculas, acotado a 1..=39 (más allá no hay
/// secuelas y el riesgo de falso positivo crece).
fn roman_value(token: &str) -> Option<u32> {
    if token.is_empty() || token.len() > 8 {
        return None;
    }
    let mut total = 0_u32;
    let mut previous = 0_u32;
    for character in token.chars().rev() {
        let value = match character {
            'i' => 1,
            'v' => 5,
            'x' => 10,
            _ => return None,
        };
        if value < previous {
            total = total.checked_sub(value)?;
        } else {
            total = total.checked_add(value)?;
            previous = value;
        }
    }
    (1..=39).contains(&total).then_some(total)
}

fn strip_leading_article(value: &str) -> String {
    for article in LEADING_ARTICLES {
        if let Some(rest) = value.strip_prefix(article) {
            return rest.to_string();
        }
    }
    value.to_string()
}

/// Retira, de forma repetida, los sufijos de edición conocidos. Repetida porque
/// las tiendas apilan («Definitive Edition Remastered»).
fn strip_edition_suffixes(value: &str) -> String {
    let mut current = value.to_string();
    let mut changed = true;
    while changed {
        changed = false;
        for suffix in EDITION_SUFFIXES {
            let Some(rest) = current.strip_suffix(suffix) else {
                continue;
            };
            let rest = rest.trim_end();
            // Nunca se vacía el título: «Redux» a secas es un juego real.
            if rest.is_empty() || rest == current {
                continue;
            }
            current = rest.to_string();
            changed = true;
            break;
        }
    }
    current
}

/// Conjunto de tokens numéricos que **distinguen** productos: números de
/// secuela, años de reedición y numerales romanos ya convertidos.
///
/// Se guarda como texto para que «03» y «3» no se confundan con el mismo dato
/// por accidente de tipos; el valor se compara tal cual aparece normalizado.
pub fn discriminant_numbers(normalized_title: &str) -> BTreeSet<String> {
    normalized_title
        .split(' ')
        .filter(|token| !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit()))
        .map(|token| token.trim_start_matches('0').to_string())
        .map(|token| {
            if token.is_empty() {
                "0".to_string()
            } else {
                token
            }
        })
        .collect()
}

fn token_set(normalized_title: &str) -> BTreeSet<String> {
    normalized_title
        .split(' ')
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn jaccard(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    if left.is_empty() && right.is_empty() {
        return 1.0;
    }
    let intersection = left.intersection(right).count() as f64;
    let union = left.union(right).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

// ---------------------------------------------------------------------------
// Jaro-Winkler
// ---------------------------------------------------------------------------

/// Similitud de Jaro-Winkler en el rango 0..=1.
///
/// Implementación propia sobre `Vec<char>` para que los títulos con acentos o
/// caracteres multibyte no se corten por la mitad. No se añade ninguna
/// dependencia por esto.
pub fn jaro_winkler(left: &str, right: &str) -> f64 {
    let jaro = jaro(left, right);
    if jaro < WINKLER_BOOST_THRESHOLD {
        return jaro;
    }
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    let prefix = left_chars
        .iter()
        .zip(right_chars.iter())
        .take(WINKLER_PREFIX_LIMIT)
        .take_while(|(a, b)| a == b)
        .count();
    (jaro + prefix as f64 * WINKLER_SCALE * (1.0 - jaro)).min(1.0)
}

/// Similitud de Jaro en el rango 0..=1.
pub fn jaro(left: &str, right: &str) -> f64 {
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    if left_chars.is_empty() && right_chars.is_empty() {
        return 1.0;
    }
    if left_chars.is_empty() || right_chars.is_empty() {
        return 0.0;
    }
    if left_chars == right_chars {
        return 1.0;
    }

    // Ventana de coincidencia clásica: ⌊max(|s1|,|s2|)/2⌋ − 1.
    let window = (left_chars.len().max(right_chars.len()) / 2).saturating_sub(1);
    let mut left_matched = vec![false; left_chars.len()];
    let mut right_matched = vec![false; right_chars.len()];
    let mut matches = 0_usize;

    for (index, character) in left_chars.iter().enumerate() {
        let start = index.saturating_sub(window);
        let end = (index + window + 1).min(right_chars.len());
        for candidate in start..end {
            if right_matched[candidate] || right_chars[candidate] != *character {
                continue;
            }
            left_matched[index] = true;
            right_matched[candidate] = true;
            matches += 1;
            break;
        }
    }

    if matches == 0 {
        return 0.0;
    }

    let mut transpositions = 0_usize;
    let mut right_cursor = 0_usize;
    for index in 0..left_chars.len() {
        if !left_matched[index] {
            continue;
        }
        while !right_matched[right_cursor] {
            right_cursor += 1;
        }
        if left_chars[index] != right_chars[right_cursor] {
            transpositions += 1;
        }
        right_cursor += 1;
    }

    let matches = matches as f64;
    let transpositions = (transpositions / 2) as f64;
    (matches / left_chars.len() as f64
        + matches / right_chars.len() as f64
        + (matches - transpositions) / matches)
        / 3.0
}

#[cfg(test)]
mod tests {
    use super::{
        EXACT_CONFIDENCE, MATCH_THRESHOLD, MatchCandidate, SteamTitleIndex, discriminant_numbers,
        jaro, jaro_winkler, normalize_title,
    };

    fn index(candidates: &[(u32, &str)]) -> SteamTitleIndex {
        let candidates = candidates
            .iter()
            .map(|(app_id, title)| MatchCandidate {
                app_id: *app_id,
                title: (*title).to_string(),
            })
            .collect::<Vec<_>>();
        SteamTitleIndex::build(&candidates)
    }

    #[test]
    fn jaro_matches_the_reference_values_of_the_literature() {
        // Valores clásicos del artículo de Winkler, con tolerancia de 1e-4.
        assert!((jaro("martha", "marhta") - 0.944_444).abs() < 1e-4);
        assert!((jaro_winkler("martha", "marhta") - 0.961_111).abs() < 1e-4);
        assert!((jaro("dixon", "dicksonx") - 0.766_667).abs() < 1e-4);
        assert!((jaro_winkler("dixon", "dicksonx") - 0.813_333).abs() < 1e-4);
        assert!((jaro_winkler("dwayne", "duane") - 0.840_000).abs() < 1e-4);
        assert_eq!(jaro_winkler("", ""), 1.0);
        assert_eq!(jaro_winkler("algo", ""), 0.0);
        assert_eq!(jaro_winkler("igual", "igual"), 1.0);
    }

    #[test]
    fn normalization_collapses_editions_articles_accents_and_roman_numerals() {
        assert_eq!(
            normalize_title("Fallout 3: Game of the Year Edition"),
            "fallout 3"
        );
        assert_eq!(
            normalize_title("The Witcher 3: Wild Hunt — Complete Edition"),
            "witcher 3 wild hunt"
        );
        assert_eq!(
            normalize_title("The Witcher 3: Wild Hunt"),
            "witcher 3 wild hunt"
        );
        assert_eq!(
            normalize_title("Divinity: Original Sin 2 - Definitive Edition"),
            "divinity original sin 2"
        );
        assert_eq!(normalize_title("Final Fantasy VII"), "final fantasy 7");
        assert_eq!(
            normalize_title("Sid Meier's Civilization VI"),
            "sid meiers civilization 6"
        );
        assert_eq!(normalize_title("Pokémon Café ReMix"), "pokemon cafe remix");
        assert_eq!(normalize_title("Rick & Morty"), "rick and morty");
        // «I» inicial no es un numeral: sigue siendo la palabra.
        assert_eq!(normalize_title("I Am Setsuna"), "i am setsuna");
        // Un título que sólo es un sufijo de edición no se vacía nunca.
        assert_eq!(normalize_title("Redux"), "redux");
        assert_eq!(normalize_title("   "), "");
    }

    #[test]
    fn goty_and_base_editions_are_the_same_product() {
        let steam = index(&[(22370, "Fallout 3"), (377160, "Fallout 4")]);
        let decision = steam
            .best_match("Fallout 3: Game of the Year Edition")
            .expect("emparejar la GOTY con la base");
        assert_eq!(decision.app_id, 22370);
        assert_eq!(decision.confidence, EXACT_CONFIDENCE);
        // Y la confianza automática nunca alcanza 1,0: ese valor está reservado
        // a una decisión humana.
        assert!(decision.confidence < 1.0);
    }

    #[test]
    fn the_complete_edition_of_the_witcher_finds_its_base_game() {
        let steam = index(&[
            (292030, "The Witcher 3: Wild Hunt"),
            (20900, "The Witcher: Enhanced Edition Director's Cut"),
            (20920, "The Witcher 2: Assassins of Kings Enhanced Edition"),
        ]);
        let decision = steam
            .best_match("The Witcher 3: Wild Hunt — Complete Edition")
            .expect("emparejar la Complete Edition");
        assert_eq!(decision.app_id, 292030);

        // Y la primera entrega, con sus dos sufijos apilados, va a su propio
        // juego y no al 2 ni al 3.
        let first = steam
            .best_match("The Witcher: Enhanced Edition")
            .expect("emparejar la primera entrega");
        assert_eq!(first.app_id, 20900);
    }

    #[test]
    fn sequels_are_never_matched_to_each_other() {
        let steam = index(&[
            (22370, "Fallout 3"),
            (377160, "Fallout 4"),
            (22300, "Fallout"),
            (588430, "Fallout Shelter"),
        ]);
        // Jaro-Winkler cree que «fallout 3» y «fallout 4» son un 0,93: la guarda
        // numérica es lo único que los separa.
        assert!(jaro_winkler("fallout 3", "fallout 4") > MATCH_THRESHOLD);
        let decision = steam.best_match("Fallout 3").expect("emparejar Fallout 3");
        assert_eq!(decision.app_id, 22370);

        // Un juego que sólo existe fuera de Steam no inventa emparejado.
        assert!(
            steam
                .best_match("Fallout Tactics: Brotherhood of Steel")
                .is_none()
        );
        // Y el spin-off nunca se pega al juego base.
        let shelter = steam
            .best_match("Fallout Shelter")
            .expect("emparejar el spin-off");
        assert_eq!(shelter.app_id, 588430);
    }

    #[test]
    fn doom_1993_and_doom_2016_are_never_confused() {
        let steam = index(&[
            (2280, "DOOM (1993)"),
            (379720, "DOOM"),
            (2300, "DOOM II"),
            (782330, "DOOM Eternal"),
        ]);
        let classic = steam
            .best_match("DOOM (1993)")
            .expect("emparejar el DOOM clásico");
        assert_eq!(classic.app_id, 2280);

        let modern = steam.best_match("DOOM").expect("emparejar el DOOM de 2016");
        assert_eq!(modern.app_id, 379720);

        let second = steam.best_match("DOOM II").expect("emparejar DOOM II");
        assert_eq!(second.app_id, 2300);

        // Un año distinto rompe el emparejado aunque el nombre sea idéntico.
        assert!(steam.best_match("DOOM (2016)").is_none());
    }

    #[test]
    fn unrelated_titles_are_left_unmatched() {
        let steam = index(&[
            (367520, "Hollow Knight"),
            (1145360, "Hades"),
            (413150, "Stardew Valley"),
            (620, "Portal 2"),
        ]);
        for external in [
            "Hollow Knight: Silksong",
            "Hades II",
            "Stardew Valley Expanded",
            "Portal",
            "Celeste",
            "",
            "   ",
        ] {
            assert!(
                steam.best_match(external).is_none(),
                "no debería emparejar «{external}»"
            );
        }
    }

    #[test]
    fn a_technical_tie_between_two_steam_games_is_refused() {
        // Dos juegos de Steam con el mismo título normalizado: elegir uno sería
        // adivinar, así que no se empareja ninguno.
        let steam = index(&[(1, "Rush"), (2, "Rush")]);
        assert!(steam.best_match("Rush").is_none());

        // Con un único candidato el mismo título sí se empareja: lo que se
        // rechaza es el empate, no la coincidencia.
        let unico = index(&[(1, "Rush")]);
        assert_eq!(unico.best_match("Rush").expect("emparejar").app_id, 1);
    }

    #[test]
    fn roman_and_arabic_spellings_of_the_same_sequel_meet() {
        let steam = index(&[(377840, "FINAL FANTASY IX"), (39140, "FINAL FANTASY VII")]);
        let decision = steam
            .best_match("Final Fantasy 7")
            .expect("emparejar la grafía arábiga con la romana");
        assert_eq!(decision.app_id, 39140);
    }

    #[test]
    fn discriminant_numbers_ignore_leading_zeros_but_keep_years() {
        // Sólo cuentan los tokens que son enteramente numéricos. En «f1 2020»
        // el «1» va pegado a la letra y forma parte de la palabra: eso es
        // correcto y basta para separar «F1 2020» de «F1 2021».
        let expect = |value: &str, expected: &[&str]| {
            assert_eq!(
                discriminant_numbers(value),
                expected.iter().map(ToString::to_string).collect(),
                "números discriminantes de «{value}»"
            );
        };
        expect("f1 2020", &["2020"]);
        expect("doom 03", &["3"]);
        expect("final fantasy 7", &["7"]);
        expect("hollow knight", &[]);

        let racing = index(&[(1, "F1 2020"), (2, "F1 2021")]);
        assert_eq!(
            racing
                .best_match("F1 2020")
                .expect("emparejar el año exacto")
                .app_id,
            1
        );
        assert!(racing.best_match("F1 2019").is_none());
    }

    #[test]
    fn an_empty_steam_library_matches_nothing() {
        let steam = SteamTitleIndex::default();
        assert!(steam.best_match("Hollow Knight").is_none());
    }
}
