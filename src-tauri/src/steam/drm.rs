//! Clasificación DRM-Free a partir de señales oficiales de la tienda de Steam.
//!
//! # Qué es y qué no es esta marca
//!
//! `drm_state` resume **lo que la ficha pública declara**, no una certificación:
//! `store.steampowered.com/api/appdetails` publica `drm_notice`,
//! `ext_user_account_notice` y `legal_notice`, y de ahí —y solo de ahí— sale la
//! clasificación. Vindexa no inspecciona binarios ni deduce nada del precio, del
//! género o de la reputación del estudio. Cuando la señal no alcanza, el estado
//! es `unknown`: **nunca se adivina**.
//!
//! # Requisito de producto: es un dato de ficha, no un adorno de carátula
//!
//! Esta marca **no debe aparecer nunca sobre las carátulas** de la biblioteca.
//! Es un dato de ficha, se muestra dentro del detalle del juego y siempre
//! acompañado de su evidencia (`drm_evidence_json`), para que la persona usuaria
//! pueda ver exactamente qué frase oficial la motivó. No diseñes insignias,
//! superposiciones ni bordes sobre la portada a partir de este campo.
//!
//! # Reglas
//!
//! 1. `third_party_drm` si `drm_notice` o `ext_user_account_notice` nombran un
//!    DRM o un lanzador de terceros, o si una categoría lo exige. `legal_notice`
//!    solo cuenta cuando nombra una tecnología anti-tamper concreta (Denuvo,
//!    SecuROM…): el aviso legal está lleno de marcas registradas y mencionar
//!    «Ubisoft» o «Rockstar» ahí no demuestra ningún requisito.
//! 2. `steam_drm` si esos mismos avisos nombran Steamworks DRM o el cliente de
//!    Steam.
//! 3. `unknown` si hay un aviso que no reconocemos. Un aviso anticheat (VAC,
//!    BattlEye, Easy Anti-Cheat) **no es DRM** y por eso no clasifica: deja el
//!    estado en `unknown` con la evidencia del aviso literal.
//! 4. `drm_free` solo si la respuesta de la tienda estaba completa y no traía
//!    ningún aviso de DRM ni de cuenta externa. Sin respuesta completa, `unknown`.

use crate::db::rich_metadata::{DrmAssessment, DrmEvidence, DrmState, MAX_DRM_EVIDENCE};

/// Longitud máxima del aviso literal que se copia como evidencia.
const MAX_EVIDENCE_CHARS: usize = 240;

/// Nombres de los campos oficiales, en la forma `camelCase` que consume la
/// interfaz.
const SOURCE_DRM_NOTICE: &str = "drmNotice";
const SOURCE_EXT_ACCOUNT: &str = "extUserAccountNotice";
const SOURCE_LEGAL_NOTICE: &str = "legalNotice";
const SOURCE_CATEGORIES: &str = "categories";
const SOURCE_STORE: &str = "storeAppdetails";

/// Señales oficiales de una única respuesta de `appdetails`.
#[derive(Debug, Clone, Copy, Default)]
pub struct DrmSignals<'a> {
    pub drm_notice: Option<&'a str>,
    pub ext_user_account_notice: Option<&'a str>,
    pub legal_notice: Option<&'a str>,
    pub categories: &'a [String],
    /// Se registra por completitud del conjunto oficial de señales. **No**
    /// influye en la clasificación: que un juego sea gratuito no dice nada
    /// sobre su DRM y deducirlo sería adivinar. Existe para que quien llame
    /// entregue el conjunto completo de señales y para que el test
    /// `being_free_to_play_never_changes_the_classification` pueda demostrar
    /// que el precio nunca entra en la decisión.
    #[allow(dead_code)]
    pub is_free: bool,
    /// `true` solo si la tienda respondió `success` con datos. La ausencia de
    /// avisos únicamente es evidencia cuando la respuesta estaba completa.
    pub store_response_complete: bool,
}

/// Frase reconocida y su etiqueta canónica para la evidencia.
struct DrmPattern {
    /// Frase normalizada (minúsculas, sin puntuación, tokens separados por un
    /// único espacio) que se busca con frontera de palabra.
    needle: &'static str,
    label: &'static str,
}

/// DRM y lanzadores de terceros. La coincidencia es por frontera de palabra:
/// «origin» no coincide con «original soundtrack».
const THIRD_PARTY_PATTERNS: &[DrmPattern] = &[
    DrmPattern { needle: "denuvo", label: "Denuvo Anti-Tamper" },
    DrmPattern { needle: "securom", label: "SecuROM" },
    DrmPattern { needle: "starforce", label: "StarForce" },
    DrmPattern { needle: "safedisc", label: "SafeDisc" },
    DrmPattern { needle: "tages", label: "Tagès" },
    DrmPattern { needle: "arxan", label: "Arxan" },
    DrmPattern { needle: "themida", label: "Themida" },
    DrmPattern { needle: "vmprotect", label: "VMProtect" },
    DrmPattern { needle: "gameguard", label: "nProtect GameGuard" },
    DrmPattern { needle: "nprotect", label: "nProtect GameGuard" },
    DrmPattern { needle: "ea app", label: "EA app" },
    DrmPattern { needle: "ea desktop", label: "EA app" },
    DrmPattern { needle: "origin", label: "EA (Origin)" },
    DrmPattern { needle: "electronic arts account", label: "Cuenta de Electronic Arts" },
    DrmPattern { needle: "cuenta de electronic arts", label: "Cuenta de Electronic Arts" },
    DrmPattern { needle: "ubisoft connect", label: "Ubisoft Connect" },
    DrmPattern { needle: "uplay", label: "Uplay" },
    DrmPattern { needle: "ubisoft account", label: "Cuenta de Ubisoft" },
    DrmPattern { needle: "cuenta de ubisoft", label: "Cuenta de Ubisoft" },
    DrmPattern { needle: "rockstar games launcher", label: "Rockstar Games Launcher" },
    DrmPattern { needle: "rockstar games social club", label: "Rockstar Social Club" },
    DrmPattern { needle: "social club", label: "Rockstar Social Club" },
    DrmPattern { needle: "battle net", label: "Battle.net" },
    DrmPattern { needle: "blizzard account", label: "Cuenta de Blizzard" },
    DrmPattern { needle: "cuenta de blizzard", label: "Cuenta de Blizzard" },
    DrmPattern { needle: "2k launcher", label: "2K Launcher" },
    DrmPattern { needle: "2k account", label: "Cuenta 2K" },
    DrmPattern { needle: "epic online services", label: "Epic Online Services" },
    DrmPattern { needle: "epic games account", label: "Cuenta de Epic Games" },
    DrmPattern { needle: "epic games launcher", label: "Epic Games Launcher" },
    DrmPattern { needle: "cuenta de epic games", label: "Cuenta de Epic Games" },
    DrmPattern { needle: "bethesda net", label: "Bethesda.net" },
    DrmPattern { needle: "bethesda account", label: "Cuenta de Bethesda" },
    DrmPattern { needle: "paradox account", label: "Cuenta de Paradox" },
    DrmPattern { needle: "cuenta de paradox", label: "Cuenta de Paradox" },
    DrmPattern { needle: "gog galaxy", label: "GOG Galaxy" },
    DrmPattern { needle: "microsoft account", label: "Cuenta de Microsoft" },
    DrmPattern { needle: "cuenta de microsoft", label: "Cuenta de Microsoft" },
    DrmPattern { needle: "xbox live", label: "Xbox Live" },
    DrmPattern { needle: "square enix account", label: "Cuenta de Square Enix" },
    DrmPattern { needle: "third party drm", label: "DRM de terceros" },
    DrmPattern { needle: "drm de terceros", label: "DRM de terceros" },
];

/// Subconjunto admitido en `legal_notice`: solo tecnologías anti-tamper con
/// nombre propio. Los nombres de editoras y lanzadores quedan fuera a propósito
/// porque el aviso legal es, casi siempre, una lista de marcas registradas.
const LEGAL_NOTICE_PATTERNS: &[DrmPattern] = &[
    DrmPattern { needle: "denuvo", label: "Denuvo Anti-Tamper" },
    DrmPattern { needle: "securom", label: "SecuROM" },
    DrmPattern { needle: "starforce", label: "StarForce" },
    DrmPattern { needle: "safedisc", label: "SafeDisc" },
    DrmPattern { needle: "arxan", label: "Arxan" },
    DrmPattern { needle: "themida", label: "Themida" },
    DrmPattern { needle: "vmprotect", label: "VMProtect" },
    DrmPattern { needle: "gameguard", label: "nProtect GameGuard" },
    DrmPattern { needle: "nprotect", label: "nProtect GameGuard" },
];

/// Señales del propio Steam.
const STEAM_PATTERNS: &[DrmPattern] = &[
    DrmPattern { needle: "steamworks drm", label: "Steamworks DRM" },
    DrmPattern { needle: "steamworks", label: "Steamworks DRM" },
    DrmPattern { needle: "steam drm", label: "Steam DRM" },
    DrmPattern { needle: "steam client", label: "Cliente de Steam" },
    DrmPattern { needle: "cliente de steam", label: "Cliente de Steam" },
    DrmPattern { needle: "requires steam", label: "Requiere Steam" },
    DrmPattern { needle: "requiere steam", label: "Requiere Steam" },
];

/// Clasifica el estado de DRM a partir de las señales oficiales.
///
/// Es una función pura: mismas señales, mismo resultado, sin red ni reloj.
pub fn classify(signals: &DrmSignals<'_>) -> DrmAssessment {
    let drm_notice = clean(signals.drm_notice);
    let ext_notice = clean(signals.ext_user_account_notice);
    let legal_notice = clean(signals.legal_notice);

    let mut third_party = Vec::new();
    let mut steam = Vec::new();

    for (source, text) in [
        (SOURCE_DRM_NOTICE, drm_notice.as_deref()),
        (SOURCE_EXT_ACCOUNT, ext_notice.as_deref()),
    ] {
        let Some(text) = text else { continue };
        let normalized = normalize(text);
        collect(&normalized, THIRD_PARTY_PATTERNS, source, &mut third_party);
        collect(&normalized, STEAM_PATTERNS, source, &mut steam);
    }

    for category in signals.categories {
        let normalized = normalize(category);
        collect(
            &normalized,
            THIRD_PARTY_PATTERNS,
            SOURCE_CATEGORIES,
            &mut third_party,
        );
    }

    if let Some(legal_notice) = legal_notice.as_deref() {
        let normalized = normalize(legal_notice);
        collect(
            &normalized,
            LEGAL_NOTICE_PATTERNS,
            SOURCE_LEGAL_NOTICE,
            &mut third_party,
        );
    }

    if !third_party.is_empty() {
        let mut evidence = third_party;
        evidence.extend(steam);
        return assessment(DrmState::ThirdPartyDrm, evidence);
    }
    if !steam.is_empty() {
        return assessment(DrmState::SteamDrm, steam);
    }

    // Hay aviso pero no lo reconocemos: se conserva literal como evidencia y el
    // estado permanece `unknown`. Interpretarlo sería adivinar.
    let mut unrecognized = Vec::new();
    for (source, text) in [
        (SOURCE_DRM_NOTICE, drm_notice.as_deref()),
        (SOURCE_EXT_ACCOUNT, ext_notice.as_deref()),
    ] {
        if let Some(text) = text {
            unrecognized.push(DrmEvidence::new(source, truncate(text)));
        }
    }
    if !unrecognized.is_empty() {
        return assessment(DrmState::Unknown, unrecognized);
    }

    if signals.store_response_complete {
        return assessment(
            DrmState::DrmFree,
            vec![DrmEvidence::new(
                SOURCE_STORE,
                "La ficha oficial no declara drm_notice ni ext_user_account_notice.",
            )],
        );
    }
    DrmAssessment::default()
}

fn assessment(state: DrmState, mut evidence: Vec<DrmEvidence>) -> DrmAssessment {
    evidence.truncate(MAX_DRM_EVIDENCE);
    DrmAssessment { state, evidence }
}

fn collect(
    normalized: &str,
    patterns: &[DrmPattern],
    source: &str,
    output: &mut Vec<DrmEvidence>,
) {
    for pattern in patterns {
        if !contains_phrase(normalized, pattern.needle) {
            continue;
        }
        if output
            .iter()
            .any(|evidence| evidence.source == source && evidence.matched == pattern.label)
        {
            continue;
        }
        output.push(DrmEvidence::new(source, pattern.label));
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn truncate(value: &str) -> String {
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= MAX_EVIDENCE_CHARS {
        return normalized;
    }
    let mut trimmed: String = normalized.chars().take(MAX_EVIDENCE_CHARS - 1).collect();
    trimmed.push('…');
    trimmed
}

/// Minúsculas, sin acentos ni puntuación, con un único espacio entre tokens y
/// rodeada de espacios. Así `contains_phrase` compara con frontera de palabra.
fn normalize(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len() + 2);
    normalized.push(' ');
    let mut last_was_space = true;
    for character in value.chars() {
        let folded = fold_accent(character);
        if folded.is_alphanumeric() {
            for lowered in folded.to_lowercase() {
                normalized.push(lowered);
            }
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }
    if !last_was_space {
        normalized.push(' ');
    }
    normalized
}

fn fold_accent(character: char) -> char {
    match character {
        'á' | 'à' | 'ä' | 'â' | 'Á' | 'À' | 'Ä' | 'Â' => 'a',
        'é' | 'è' | 'ë' | 'ê' | 'É' | 'È' | 'Ë' | 'Ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' | 'Í' | 'Ì' | 'Ï' | 'Î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'Ó' | 'Ò' | 'Ö' | 'Ô' => 'o',
        'ú' | 'ù' | 'ü' | 'û' | 'Ú' | 'Ù' | 'Ü' | 'Û' => 'u',
        'ñ' | 'Ñ' => 'n',
        'ç' | 'Ç' => 'c',
        other => other,
    }
}

fn contains_phrase(normalized: &str, needle: &str) -> bool {
    normalized.contains(&format!(" {needle} "))
}

#[cfg(test)]
mod tests {
    use super::{DrmSignals, classify};
    use crate::db::rich_metadata::{DrmAssessment, DrmState};

    fn signals<'a>(drm: Option<&'a str>, ext: Option<&'a str>, legal: Option<&'a str>) -> DrmSignals<'a> {
        DrmSignals {
            drm_notice: drm,
            ext_user_account_notice: ext,
            legal_notice: legal,
            categories: &[],
            is_free: false,
            store_response_complete: true,
        }
    }

    fn labels(assessment: &DrmAssessment) -> Vec<&str> {
        assessment
            .evidence
            .iter()
            .map(|evidence| evidence.matched.as_str())
            .collect()
    }

    #[test]
    fn denuvo_in_the_drm_notice_is_third_party() {
        let result = classify(&signals(
            Some("Denuvo Anti-tamper. 5 machine activations per 24 hours."),
            None,
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert_eq!(result.evidence[0].source, "drmNotice");
        assert_eq!(result.evidence[0].matched, "Denuvo Anti-Tamper");
    }

    #[test]
    fn the_ea_app_account_requirement_is_third_party() {
        let result = classify(&signals(
            None,
            Some("Requires a EA account and the EA app to play."),
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert_eq!(result.evidence[0].source, "extUserAccountNotice");
        assert!(labels(&result).contains(&"EA app"));
    }

    #[test]
    fn ubisoft_connect_is_third_party() {
        let result = classify(&signals(
            None,
            Some("Se requiere una cuenta de Ubisoft y Ubisoft Connect."),
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"Ubisoft Connect"));
    }

    #[test]
    fn the_rockstar_launcher_and_social_club_are_third_party() {
        let result = classify(&signals(
            Some("Rockstar Games Launcher"),
            Some("Rockstar Games Social Club"),
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"Rockstar Games Launcher"));
    }

    #[test]
    fn battle_net_is_third_party_even_written_with_a_dot() {
        let result = classify(&signals(
            None,
            Some("Requires a Battle.net account."),
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"Battle.net"));
    }

    #[test]
    fn the_2k_launcher_is_third_party() {
        let result = classify(&signals(Some("2K Launcher"), None, None));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"2K Launcher"));
    }

    #[test]
    fn epic_online_services_as_a_requirement_is_third_party() {
        let result = classify(&signals(
            None,
            Some("This product requires an Epic Games account and Epic Online Services."),
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"Epic Online Services"));
    }

    #[test]
    fn nprotect_gameguard_is_third_party() {
        let result = classify(&signals(Some("nProtect GameGuard"), None, None));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"nProtect GameGuard"));
    }

    #[test]
    fn steamworks_drm_is_classified_as_steam_not_as_third_party() {
        let result = classify(&signals(Some("Steamworks DRM"), None, None));
        assert_eq!(result.state, DrmState::SteamDrm);
        assert_eq!(result.evidence[0].matched, "Steamworks DRM");
    }

    #[test]
    fn a_third_party_drm_wins_over_a_simultaneous_steam_signal() {
        let result = classify(&signals(
            Some("Steamworks DRM y Denuvo Anti-tamper"),
            None,
            None,
        ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(labels(&result).contains(&"Denuvo Anti-Tamper"));
        assert!(labels(&result).contains(&"Steamworks DRM"));
    }

    #[test]
    fn a_legal_notice_with_only_trademarks_is_not_third_party_drm() {
        let result = classify(&signals(
            None,
            None,
            Some(
                "© 2020 Ubisoft Entertainment. All Rights Reserved. Ubisoft, Ubi.com and the \
                 Ubisoft logo are trademarks of Ubisoft Entertainment in the U.S. and/or other \
                 countries. Rockstar Games and the Rockstar Games logo are trademarks of \
                 Take-Two Interactive.",
            ),
        ));
        assert_eq!(result.state, DrmState::DrmFree);
        assert_eq!(result.evidence[0].source, "storeAppdetails");
    }

    #[test]
    fn a_legal_notice_naming_an_anti_tamper_technology_is_third_party() {
        let result = classify(&signals(
            None,
            None,
            Some("This game is protected by Denuvo Anti-Tamper technology."),
            ));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert_eq!(result.evidence[0].source, "legalNotice");
    }

    #[test]
    fn anti_cheat_notices_are_never_drm_and_stay_unknown() {
        for notice in [
            "This game uses Valve Anti-Cheat (VAC).",
            "Este producto utiliza BattlEye.",
            "Easy Anti-Cheat is required to play online.",
        ] {
            let result = classify(&signals(Some(notice), None, None));
            assert_eq!(
                result.state,
                DrmState::Unknown,
                "un aviso anticheat no clasifica como DRM: {notice}"
            );
            assert_eq!(result.evidence[0].source, "drmNotice");
        }
    }

    #[test]
    fn a_word_that_merely_contains_a_needle_never_matches() {
        let categories = [
            "Banda sonora original".to_string(),
            "Incluye la banda sonora originalmente publicada".to_string(),
        ];
        let result = classify(&DrmSignals {
            categories: &categories,
            ..signals(None, None, None)
        });
        assert_eq!(result.state, DrmState::DrmFree);
        assert!(!labels(&result).contains(&"EA (Origin)"));
    }

    #[test]
    fn a_complete_response_without_notices_is_the_only_drm_free_case() {
        let result = classify(&signals(None, None, None));
        assert_eq!(result.state, DrmState::DrmFree);
        assert_eq!(result.evidence.len(), 1);
    }

    #[test]
    fn an_incomplete_response_never_claims_drm_free() {
        let result = classify(&DrmSignals {
            store_response_complete: false,
            ..DrmSignals::default()
        });
        assert_eq!(result.state, DrmState::Unknown);
        assert!(result.evidence.is_empty());
    }

    #[test]
    fn an_unrecognized_notice_keeps_the_literal_signal_and_stays_unknown() {
        let result = classify(&signals(
            Some("Este juego requiere una conexión permanente a un servicio no identificado."),
            None,
            None,
        ));
        assert_eq!(result.state, DrmState::Unknown);
        assert_eq!(result.evidence[0].source, "drmNotice");
        assert!(result.evidence[0].matched.starts_with("Este juego requiere"));
    }

    #[test]
    fn being_free_to_play_never_changes_the_classification() {
        let paid = DrmSignals {
            is_free: false,
            ..signals(None, None, None)
        };
        let free = DrmSignals {
            is_free: true,
            ..signals(None, None, None)
        };
        assert_eq!(classify(&paid), classify(&free));
    }

    #[test]
    fn ordinary_steam_categories_never_trigger_a_third_party_match() {
        let categories = [
            "Un jugador".to_string(),
            "Multijugador".to_string(),
            "Anti-Cheat de Valve habilitado".to_string(),
            "Steam Cloud".to_string(),
            "Compatible con Steam Workshop".to_string(),
        ];
        let result = classify(&DrmSignals {
            categories: &categories,
            ..signals(None, None, None)
        });
        assert_eq!(result.state, DrmState::DrmFree);
    }

    #[test]
    fn a_category_that_demands_a_third_party_launcher_is_third_party() {
        let categories = ["Requiere Ubisoft Connect".to_string()];
        let result = classify(&DrmSignals {
            categories: &categories,
            ..signals(None, None, None)
        });
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert_eq!(result.evidence[0].source, "categories");
    }

    #[test]
    fn empty_or_blank_notices_are_treated_as_absent() {
        let result = classify(&signals(Some("   "), Some(""), Some("\n\t")));
        assert_eq!(result.state, DrmState::DrmFree);
    }

    #[test]
    fn evidence_never_grows_beyond_the_documented_cap() {
        let notice = "Denuvo, SecuROM, StarForce, SafeDisc, Arxan, Themida, VMProtect, \
                      nProtect GameGuard, Uplay, Ubisoft Connect, Battle.net, 2K Launcher";
        let result = classify(&signals(Some(notice), None, None));
        assert_eq!(result.state, DrmState::ThirdPartyDrm);
        assert!(result.evidence.len() <= 8);
    }

    #[test]
    fn a_long_unrecognized_notice_is_bounded_before_becoming_evidence() {
        let notice = "aviso ".repeat(200);
        let result = classify(&signals(Some(&notice), None, None));
        assert_eq!(result.state, DrmState::Unknown);
        assert!(result.evidence[0].matched.chars().count() <= 240);
    }

    #[test]
    fn the_serialized_evidence_uses_the_source_and_match_keys() {
        let result = classify(&signals(Some("Denuvo Anti-tamper"), None, None));
        let json = serde_json::to_string(&result.evidence).expect("serializar evidencias");
        assert_eq!(
            json,
            r#"[{"source":"drmNotice","match":"Denuvo Anti-Tamper"}]"#
        );
    }
}
