//! Construcción de la acción de lanzamiento de un juego de tienda externa.
//!
//! # Esto es una frontera de inyección
//!
//! El identificador de un juego externo llega de un fichero JSON escrito por
//! otro programa. Interpolarlo directamente en una URL de protocolo convertiría
//! un manifiesto manipulado en una orden arbitraria para el cliente de Epic o de
//! GOG. Por eso **el identificador se valida contra una lista blanca de
//! caracteres antes de construir nada**, igual que `crate::steam::actions` exige
//! que el AppID sea un entero positivo antes de formar `steam://run/<id>`.
//!
//! La validación es de allowlist, no de denylist: se enumera lo que se acepta y
//! todo lo demás se rechaza. Un `%2F`, un `?`, un espacio o una comilla no
//! «se escapan»: se rechaza la entrada entera.

use crate::error::{AppError, AppResult};
use crate::stores::ExternalStore;
use crate::stores::paths;
use std::path::Path;
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;

/// Longitud máxima de un identificador externo. Los `AppName` de Epic rondan los
/// 32 caracteres y los `productId` de GOG son numéricos de 10 dígitos; 128 deja
/// margen sin abrir la puerta a una URL desmesurada.
pub const MAX_EXTERNAL_ID_CHARS: usize = 128;

/// Acción que la interfaz puede pedir sobre un juego externo. Allowlist cerrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalGameAction {
    Launch,
}

impl ExternalGameAction {
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "launch" => Ok(Self::Launch),
            _ => Err(AppError::validation(
                "La acción sobre el juego externo no es válida.",
            )),
        }
    }
}

/// Cómo se va a arrancar el juego.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchTarget {
    /// URL de protocolo registrada por el cliente oficial.
    ProtocolUrl(String),
    /// Ruta absoluta a un ejecutable ya validado y contenido en su instalación.
    Executable(String),
}

/// Valida un identificador de Epic (`AppName`).
///
/// Epic usa identificadores alfanuméricos ASCII; algunos títulos añaden `_`,
/// `-` o `.`. Nada más entra.
pub fn validate_epic_app_name(value: &str) -> AppResult<&str> {
    if value.is_empty() || value.chars().count() > MAX_EXTERNAL_ID_CHARS {
        return Err(invalid_identifier());
    }
    let accepted = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
    if !accepted {
        return Err(invalid_identifier());
    }
    Ok(value)
}

/// Valida un identificador de GOG (`productId`): sólo dígitos, sin ceros a la
/// izquierda con significado y nunca cero.
pub fn validate_gog_product_id(value: &str) -> AppResult<&str> {
    if value.is_empty() || value.len() > 20 {
        return Err(invalid_identifier());
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_identifier());
    }
    if value.bytes().all(|byte| byte == b'0') {
        return Err(invalid_identifier());
    }
    Ok(value)
}

/// Valida el identificador según la tienda a la que pertenece.
pub fn validate_external_id(store: ExternalStore, value: &str) -> AppResult<&str> {
    match store {
        ExternalStore::Epic => validate_epic_app_name(value),
        ExternalStore::Gog => validate_gog_product_id(value),
    }
}

/// Construye la URL de protocolo oficial de la tienda.
///
/// * Epic: `com.epicgames.launcher://apps/<AppName>?action=launch&silent=true`
///   es el enlace que el propio Epic Games Launcher registra y publica en sus
///   accesos directos.
/// * GOG: `goggalaxy://openGameView/<productId>` abre la ficha del juego dentro
///   de Galaxy. **No es un lanzamiento directo**: Galaxy no expone un esquema
///   público para eso (su cliente usa `GalaxyClient.exe /command=runGame`, que
///   es un ejecutable, no un protocolo). Cuando hay un ejecutable validado se
///   prefiere [`LaunchTarget::Executable`]; esta URL es el respaldo honesto.
pub fn protocol_url(store: ExternalStore, external_id: &str) -> AppResult<String> {
    let external_id = validate_external_id(store, external_id)?;
    Ok(match store {
        ExternalStore::Epic => {
            format!("com.epicgames.launcher://apps/{external_id}?action=launch&silent=true")
        }
        ExternalStore::Gog => format!("goggalaxy://openGameView/{external_id}"),
    })
}

/// Decide cómo arrancar un juego externo.
///
/// Si hay un ejecutable registrado, se revalida **en el momento del
/// lanzamiento** (no basta con que lo estuviera al escanear: el juego puede
/// haberse desinstalado o la ruta puede haber sido reemplazada por un enlace).
/// Si no supera la validación, se cae con elegancia a la URL de protocolo, que
/// deja la decisión en manos del cliente oficial.
pub fn resolve_launch_target(
    store: ExternalStore,
    external_id: &str,
    install_path: Option<&str>,
    launch_target: Option<&str>,
) -> AppResult<LaunchTarget> {
    let external_id = validate_external_id(store, external_id)?;

    if let (Some(install_path), Some(launch_target)) = (install_path, launch_target) {
        let install_directory = Path::new(install_path);
        if paths::is_real_directory(install_directory)
            && let Some(executable) = paths::resolve_executable_within(install_directory, launch_target)
        {
            return Ok(LaunchTarget::Executable(paths::display_path(&executable)));
        }
    }

    Ok(LaunchTarget::ProtocolUrl(protocol_url(store, external_id)?))
}

/// Entrega la acción al sistema operativo.
///
/// Se separa de [`resolve_launch_target`] para que toda la lógica de decisión
/// sea comprobable sin un `AppHandle`, igual que en `crate::steam::actions`. El
/// `LaunchTarget` sólo puede haber salido de esa función, que ya validó el
/// identificador y contuvo el ejecutable dentro de su instalación.
///
/// Vindexa entrega la acción y **no conoce el resultado**: si el cliente de la
/// tienda decide pedir una actualización, mostrar un aviso o no hacer nada, eso
/// ya no depende de aquí.
pub fn open_external_game<R: Runtime>(
    app: &AppHandle<R>,
    target: &LaunchTarget,
) -> AppResult<()> {
    match target {
        LaunchTarget::ProtocolUrl(url) => app.opener().open_url(url.as_str(), None::<&str>)?,
        LaunchTarget::Executable(path) => app.opener().open_path(path.as_str(), None::<&str>)?,
    }
    Ok(())
}

fn invalid_identifier() -> AppError {
    // El mensaje no repite el identificador rechazado: si viniera de un
    // manifiesto manipulado, eso lo colaría en la interfaz.
    AppError::validation("El identificador del juego externo no es válido.")
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalGameAction, LaunchTarget, protocol_url, resolve_launch_target,
        validate_epic_app_name, validate_external_id, validate_gog_product_id,
    };
    use crate::stores::ExternalStore;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn the_action_is_an_explicit_allowlist() {
        assert_eq!(
            ExternalGameAction::parse("launch").unwrap(),
            ExternalGameAction::Launch
        );
        for invented in ["uninstall", "run", "", "launch ", "LAUNCH"] {
            assert!(ExternalGameAction::parse(invented).is_err());
        }
    }

    #[test]
    fn epic_identifiers_reject_every_command_injection_shape() {
        assert!(validate_epic_app_name("Fortnite").is_ok());
        assert!(validate_epic_app_name("0a1b2c3d4e5f6789").is_ok());
        assert!(validate_epic_app_name("Snapdragon_Egret").is_ok());
        assert!(validate_epic_app_name("Some-App.Name").is_ok());

        for malicious in [
            "",
            " ",
            "app name",
            "app/../../etc/passwd",
            "app?action=uninstall",
            "app&action=uninstall",
            "app#fragment",
            "app%2Faction",
            "app\"; rm -rf /",
            "app'\u{0000}",
            "app\nnewline",
            "app\r\nSet-Cookie: x",
            "com.epicgames.launcher://apps/otro",
            "app\\windows\\path",
            "aplicación",
            "app|pipe",
            "app;semicolon",
            "app$(whoami)",
            "app`whoami`",
        ] {
            let error =
                validate_epic_app_name(malicious).expect_err("rechazar identificador malicioso");
            assert_eq!(error.code, "validation");
            // El mensaje jamás repite lo que le entró.
            assert!(!error.message.contains(malicious.trim()) || malicious.trim().is_empty());
        }

        let too_long = "a".repeat(200);
        assert!(validate_epic_app_name(&too_long).is_err());
    }

    #[test]
    fn gog_identifiers_are_non_zero_digits_only() {
        assert!(validate_gog_product_id("1207658924").is_ok());
        assert!(validate_gog_product_id("1").is_ok());
        for malicious in [
            "",
            "0",
            "000",
            "12a",
            "-1",
            "12 34",
            "1207658924/../..",
            "1207658924?x=1",
            "1207658924'; DROP TABLE external_games; --",
            "١٢٣",
            "99999999999999999999999",
        ] {
            assert!(
                validate_gog_product_id(malicious).is_err(),
                "debería rechazar «{malicious}»"
            );
        }
    }

    #[test]
    fn protocol_urls_match_the_official_schemes_and_never_carry_raw_input() {
        assert_eq!(
            protocol_url(ExternalStore::Epic, "Fortnite").unwrap(),
            "com.epicgames.launcher://apps/Fortnite?action=launch&silent=true"
        );
        assert_eq!(
            protocol_url(ExternalStore::Gog, "1207658924").unwrap(),
            "goggalaxy://openGameView/1207658924"
        );

        // La validación se ejecuta ANTES de construir la URL, así que ningún
        // parámetro extra puede colarse por el identificador.
        assert!(protocol_url(ExternalStore::Epic, "Fortnite&action=uninstall").is_err());
        assert!(protocol_url(ExternalStore::Gog, "1207658924/../otro").is_err());
        assert!(validate_external_id(ExternalStore::Gog, "NoSoyUnNumero").is_err());
    }

    #[test]
    fn a_registered_executable_is_revalidated_at_launch_time() {
        let directory = TempDir::new().expect("crear temporal");
        let install = directory.path().join("Juego");
        fs::create_dir_all(&install).expect("crear instalación");
        let executable = install.join("Juego.exe");
        fs::write(&executable, "binario").expect("escribir ejecutable");

        let target = resolve_launch_target(
            ExternalStore::Gog,
            "1207658924",
            install.to_str(),
            Some("Juego.exe"),
        )
        .expect("resolver el objetivo");
        assert!(matches!(target, LaunchTarget::Executable(_)));

        // Si el ejecutable se borra, no se lanza una ruta fantasma: se cae al
        // protocolo oficial y decide el cliente.
        fs::remove_file(&executable).expect("desinstalar el ejecutable");
        let fallback = resolve_launch_target(
            ExternalStore::Gog,
            "1207658924",
            install.to_str(),
            Some("Juego.exe"),
        )
        .expect("resolver el respaldo");
        assert_eq!(
            fallback,
            LaunchTarget::ProtocolUrl("goggalaxy://openGameView/1207658924".to_string())
        );
    }

    #[test]
    fn an_executable_outside_the_installation_never_becomes_a_launch_target() {
        let directory = TempDir::new().expect("crear temporal");
        let install = directory.path().join("Juego");
        fs::create_dir_all(&install).expect("crear instalación");
        let outside = directory.path().join("malicioso.sh");
        fs::write(&outside, "#!/bin/sh\nrm -rf /").expect("escribir binario externo");

        let target = resolve_launch_target(
            ExternalStore::Epic,
            "Fortnite",
            install.to_str(),
            Some("../malicioso.sh"),
        )
        .expect("resolver el objetivo");
        assert_eq!(
            target,
            LaunchTarget::ProtocolUrl(
                "com.epicgames.launcher://apps/Fortnite?action=launch&silent=true".to_string()
            )
        );

        // Ni siquiera con la ruta absoluta al binario externo.
        let absolute = resolve_launch_target(
            ExternalStore::Epic,
            "Fortnite",
            install.to_str(),
            outside.to_str(),
        )
        .expect("resolver el objetivo absoluto");
        assert!(matches!(absolute, LaunchTarget::ProtocolUrl(_)));
    }

    #[test]
    fn a_malicious_identifier_is_refused_even_before_looking_at_the_disk() {
        let error = resolve_launch_target(ExternalStore::Epic, "app?action=uninstall", None, None)
            .expect_err("rechazar antes de tocar el disco");
        assert_eq!(error.code, "validation");
    }
}
