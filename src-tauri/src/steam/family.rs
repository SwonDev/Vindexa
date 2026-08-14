use crate::error::{AppError, AppResult};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

const STEAM_ID64_ACCOUNT_BASE: u64 = 76_561_197_960_265_728;
const MAX_LOCAL_CONFIG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FAMILY_MEMBERS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFamilyGroup {
    /// SteamID64 de los demás miembros. No se persisten nombres, rol ni nombre
    /// del grupo; los identificadores sólo sirven como diagnóstico transitorio.
    pub member_steam_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LocalFamilyEvidence {
    pub group: LocalFamilyGroup,
    /// AppIDs para los que el cliente mantiene caché de biblioteca en la cuenta
    /// enlazada. Es evidencia auxiliar y deliberadamente no se interpreta como
    /// un inventario completo por sí sola.
    pub cached_library_app_ids: HashSet<u32>,
}

pub fn detect_current(own_steam_id: &str) -> AppResult<Option<LocalFamilyEvidence>> {
    let steam = match steamlocate::locate() {
        Ok(steam) => steam,
        Err(_) => return Ok(None),
    };
    let Some(group) = detect(steam.path(), own_steam_id)? else {
        return Ok(None);
    };
    let mut cached_library_app_ids = cached_library_app_ids(steam.path(), own_steam_id)?;
    // Un manifiesto local es una señal incluso más fuerte que la caché del
    // cliente: el juego llegó a estar instalado en una biblioteca accesible.
    // Si una biblioteca está dañada simplemente se omite; nunca se inventa
    // disponibilidad a partir de una lectura fallida.
    if let Ok(libraries) = steam.libraries() {
        for library in libraries.filter_map(Result::ok) {
            cached_library_app_ids.extend(library.app_ids());
        }
    }
    Ok(Some(LocalFamilyEvidence {
        group,
        cached_library_app_ids,
    }))
}

/// Detecta únicamente el bloque `FamilyGroup` del caché local del cliente de
/// Steam. El formato VDF no es una API pública: si cambia, se devuelve ausencia
/// de diagnóstico y nunca se etiqueta un juego como compartido por inferencia.
pub fn detect(steam_path: &Path, own_steam_id: &str) -> AppResult<Option<LocalFamilyGroup>> {
    let own_steam_id = parse_steam_id(own_steam_id)?;
    let account_id = own_steam_id
        .checked_sub(STEAM_ID64_ACCOUNT_BASE)
        .filter(|value| *value <= u32::MAX as u64)
        .ok_or_else(|| AppError::validation("El SteamID64 no tiene un formato válido."))?;
    let config_path = steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("localconfig.vdf");
    let metadata = match fs::symlink_metadata(&config_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => {
            return Err(AppError::new(
                "steam_family_cache_read",
                "No se pudo comprobar el caché local de Steam Families.",
            ));
        }
    };
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_LOCAL_CONFIG_BYTES
    {
        return Err(AppError::new(
            "steam_family_cache_invalid",
            "El caché local de Steam Families no tiene un formato seguro.",
        ));
    }
    let contents = fs::read_to_string(config_path).map_err(|_| {
        AppError::new(
            "steam_family_cache_read",
            "No se pudo leer el caché local de Steam Families.",
        )
    })?;
    Ok(parse_family_group(&contents, own_steam_id))
}

fn parse_steam_id(value: &str) -> AppResult<u64> {
    if !(16..=20).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::validation(
            "El SteamID64 no tiene un formato válido.",
        ));
    }
    value
        .parse()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| AppError::validation("El SteamID64 no tiene un formato válido."))
}

fn cached_library_app_ids(steam_path: &Path, own_steam_id: &str) -> AppResult<HashSet<u32>> {
    let own_steam_id = parse_steam_id(own_steam_id)?;
    let account_id = own_steam_id
        .checked_sub(STEAM_ID64_ACCOUNT_BASE)
        .filter(|value| *value <= u32::MAX as u64)
        .ok_or_else(|| AppError::validation("El SteamID64 no tiene un formato válido."))?;
    let cache_path = steam_path
        .join("userdata")
        .join(account_id.to_string())
        .join("config")
        .join("librarycache");
    let entries = match fs::read_dir(cache_path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(_) => {
            return Err(AppError::new(
                "steam_family_cache_read",
                "No se pudo comprobar el índice local de la biblioteca de Steam.",
            ));
        }
    };
    let mut app_ids = HashSet::new();
    for entry in entries.take(20_000) {
        let Ok(entry) = entry else { continue };
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let Some(app_id) = path
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value != 0)
        else {
            continue;
        };
        app_ids.insert(app_id);
    }
    Ok(app_ids)
}

fn parse_family_group(contents: &str, own_steam_id: u64) -> Option<LocalFamilyGroup> {
    let mut waiting_for_block = false;
    let mut in_family_group = false;
    let mut depth = 0_i32;
    let mut members = Vec::new();

    for line in contents.lines() {
        let trimmed = line.trim();
        if !in_family_group {
            if waiting_for_block {
                if trimmed == "{" {
                    in_family_group = true;
                    depth = 1;
                } else if !trimmed.is_empty() {
                    waiting_for_block = false;
                }
                continue;
            }
            if trimmed == "\"FamilyGroup\"" {
                waiting_for_block = true;
            }
            continue;
        }

        if trimmed == "{" {
            depth += 1;
            continue;
        }
        if trimmed == "}" {
            depth -= 1;
            if depth == 0 {
                break;
            }
            continue;
        }
        let Some(account_id) = quoted_pair(trimmed)
            .filter(|(key, _)| *key == "accountid")
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .filter(|value| *value <= u32::MAX as u64)
        else {
            continue;
        };
        let Some(steam_id) = STEAM_ID64_ACCOUNT_BASE.checked_add(account_id) else {
            continue;
        };
        if steam_id != own_steam_id && !members.contains(&steam_id) {
            members.push(steam_id);
            if members.len() == MAX_FAMILY_MEMBERS - 1 {
                break;
            }
        }
    }

    (!members.is_empty()).then(|| LocalFamilyGroup {
        member_steam_ids: members.into_iter().map(|value| value.to_string()).collect(),
    })
}

fn quoted_pair(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split('"');
    let before = parts.next()?;
    let key = parts.next()?;
    let between = parts.next()?;
    let value = parts.next()?;
    let after = parts.next().unwrap_or_default();
    if !before.is_empty()
        || !between.chars().all(char::is_whitespace)
        || !after.chars().all(char::is_whitespace)
        || parts.next().is_some()
    {
        return None;
    }
    Some((key, value))
}

#[cfg(test)]
mod tests {
    use super::{
        STEAM_ID64_ACCOUNT_BASE, cached_library_app_ids, detect, parse_family_group, quoted_pair,
    };
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parses_only_family_member_account_ids_without_personal_labels() {
        let own_account = 121_123_806_u64;
        let contents = r#"
          "Unrelated"
          {
            "accountid" "999"
          }
          "FamilyGroup"
          {
            "version" "1"
            "name" "No debe salir del parser"
            "members"
            {
              "0" { "accountid" "999" }
              "1"
              {
                "accountid" "55623626"
              }
              "2"
              {
                "accountid" "121123806"
              }
            }
          }
          "After" { "accountid" "888" }
        "#;
        // Las formas inline se ignoran deliberadamente: el cliente actual
        // escribe las llaves y los pares en líneas independientes.
        let result = parse_family_group(contents, STEAM_ID64_ACCOUNT_BASE + own_account)
            .expect("detectar familia");
        assert_eq!(
            result.member_steam_ids,
            vec![(STEAM_ID64_ACCOUNT_BASE + 55_623_626).to_string()]
        );
    }

    #[test]
    fn returns_none_instead_of_inventing_members_from_other_blocks() {
        let contents = r#""Software" { "accountid" "123" }"#;
        assert!(parse_family_group(contents, STEAM_ID64_ACCOUNT_BASE + 1).is_none());
    }

    #[test]
    fn quoted_pairs_reject_trailing_or_embedded_tokens() {
        assert_eq!(
            quoted_pair("\"accountid\"  \"123\""),
            Some(("accountid", "123"))
        );
        assert!(quoted_pair("\"accountid\" \"123\" extra").is_none());
        assert!(quoted_pair("prefix \"accountid\" \"123\"").is_none());
    }

    #[test]
    fn detects_family_and_app_ids_without_reading_cache_contents() {
        let directory = TempDir::new().expect("crear temporal");
        let account_id = 121_123_806_u64;
        let config = directory
            .path()
            .join("userdata")
            .join(account_id.to_string())
            .join("config");
        let library_cache = config.join("librarycache");
        fs::create_dir_all(&library_cache).expect("crear caché");
        fs::write(
            config.join("localconfig.vdf"),
            "\"FamilyGroup\"\n{\n\"members\"\n{\n\"0\"\n{\n\"accountid\" \"55623626\"\n}\n}\n}\n",
        )
        .expect("guardar VDF");
        // El contenido no es JSON válido a propósito: sólo el nombre de archivo
        // se usa como señal de caché, evitando leer datos personales adicionales.
        fs::write(library_cache.join("620.json"), b"not parsed").expect("crear entrada");
        fs::write(library_cache.join("not-an-app.json"), b"ignored").expect("crear ruido");

        let steam_id = (STEAM_ID64_ACCOUNT_BASE + account_id).to_string();
        let family = detect(directory.path(), &steam_id)
            .expect("detectar")
            .expect("familia");
        assert_eq!(family.member_steam_ids.len(), 1);
        let ids = cached_library_app_ids(directory.path(), &steam_id).expect("leer índice");
        assert_eq!(ids, [620].into_iter().collect());
    }
}
