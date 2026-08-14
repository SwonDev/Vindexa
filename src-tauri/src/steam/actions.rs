use crate::db::Database;
use crate::error::{AppError, AppResult};
use rusqlite::OptionalExtension;
use std::path::PathBuf;
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameAction {
    Launch,
    Install,
    Uninstall,
    #[cfg(test)]
    Store,
}

impl GameAction {
    #[cfg(test)]
    pub fn parse(value: &str) -> AppResult<Self> {
        match value {
            "launch" => Ok(Self::Launch),
            "install" => Ok(Self::Install),
            "uninstall" => Ok(Self::Uninstall),
            "store" => Ok(Self::Store),
            _ => Err(AppError::validation("La acción de Steam no es válida.")),
        }
    }
}

pub fn open_game_action<R: Runtime>(
    app: &AppHandle<R>,
    app_id: u32,
    action: GameAction,
) -> AppResult<()> {
    validate_app_id(app_id)?;
    let url = game_action_url(app_id, action)?;
    app.opener().open_url(url, None::<&str>)?;
    Ok(())
}

pub fn request_uninstall<R: Runtime>(
    app: &AppHandle<R>,
    database: &Database,
    app_id: u32,
) -> AppResult<()> {
    validate_app_id(app_id)?;
    let installed = database.open()?.query_row(
        "SELECT EXISTS(SELECT 1 FROM game_installations WHERE app_id = ?1 AND is_primary = 1)",
        [app_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !installed {
        return Err(AppError::not_found(
            "Steam no registra una instalación local para este juego.",
        ));
    }
    open_game_action(app, app_id, GameAction::Uninstall)
}

fn game_action_url(app_id: u32, action: GameAction) -> AppResult<String> {
    validate_app_id(app_id)?;
    Ok(match action {
        GameAction::Launch => format!("steam://run/{app_id}"),
        GameAction::Install => format!("steam://install/{app_id}"),
        GameAction::Uninstall => format!("steam://uninstall/{app_id}"),
        #[cfg(test)]
        GameAction::Store => format!("https://store.steampowered.com/app/{app_id}"),
    })
}

pub fn reveal_installation<R: Runtime>(
    app: &AppHandle<R>,
    database: &Database,
    app_id: u32,
) -> AppResult<()> {
    validate_app_id(app_id)?;
    let path: Option<String> = database
        .open()?
        .query_row(
            "SELECT install_path FROM game_installations WHERE app_id = ?1 AND is_primary = 1",
            [app_id],
            |row| row.get(0),
        )
        .optional()?;
    let path = path.ok_or_else(|| {
        AppError::not_found("No hay una instalación local registrada para este juego.")
    })?;
    let canonical = PathBuf::from(path).canonicalize().map_err(|_| {
        AppError::not_found("La carpeta de instalación ya no existe en este equipo.")
    })?;
    if !canonical.is_dir() {
        return Err(AppError::not_found(
            "La ruta registrada no es una carpeta de instalación válida.",
        ));
    }
    if !is_current_steam_installation(&canonical) {
        return Err(AppError::validation(
            "La carpeta registrada no pertenece a una biblioteca local de Steam detectada.",
        ));
    }
    app.opener().reveal_item_in_dir(&canonical)?;
    Ok(())
}

fn is_current_steam_installation(path: &std::path::Path) -> bool {
    steamlocate::locate_all().is_ok_and(|installations| {
        installations.into_iter().any(|steam| {
            steam.libraries().is_ok_and(|libraries| {
                libraries.filter_map(Result::ok).any(|library| {
                    library
                        .path()
                        .join("steamapps")
                        .join("common")
                        .canonicalize()
                        .is_ok_and(|root| path.starts_with(root))
                })
            })
        })
    })
}

fn validate_app_id(app_id: u32) -> AppResult<()> {
    if app_id == 0 {
        return Err(AppError::validation(
            "El identificador de Steam no es válido.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GameAction, game_action_url};

    #[test]
    fn actions_are_an_explicit_allowlist() {
        assert_eq!(GameAction::parse("launch").unwrap(), GameAction::Launch);
        assert_eq!(GameAction::parse("install").unwrap(), GameAction::Install);
        assert_eq!(
            GameAction::parse("uninstall").unwrap(),
            GameAction::Uninstall
        );
        assert_eq!(GameAction::parse("store").unwrap(), GameAction::Store);
        assert!(GameAction::parse("steam://evil").is_err());
    }

    #[test]
    fn uninstall_only_builds_the_steam_protocol_for_a_valid_app() {
        assert_eq!(
            game_action_url(620, GameAction::Uninstall).unwrap(),
            "steam://uninstall/620"
        );
        assert!(game_action_url(0, GameAction::Uninstall).is_err());
    }
}
