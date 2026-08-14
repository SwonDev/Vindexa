use crate::db::{ImportedGame, ImportedInstallation};
use crate::error::{AppError, AppResult};
use chrono::{DateTime, Utc};
use std::collections::BTreeMap;
use std::time::SystemTime;

#[derive(Debug)]
pub struct LocalLibraryScan {
    pub steam_path: String,
    pub libraries_scanned: usize,
    pub games: Vec<ImportedGame>,
}

pub fn detect() -> Option<(String, usize)> {
    let steam = steamlocate::locate().ok()?;
    let count = steam
        .libraries()
        .ok()?
        .filter_map(Result::ok)
        .map(|library| library.app_ids().len())
        .sum();
    Some((steam.path().display().to_string(), count))
}

pub fn scan() -> AppResult<LocalLibraryScan> {
    let steam = steamlocate::locate().map_err(|error| {
        AppError::new(
            "steam_not_found",
            format!("No se encontró una instalación local de Steam: {error}"),
        )
    })?;
    let steam_path = steam.path().display().to_string();
    let libraries = steam.libraries().map_err(steamlocate_error)?;
    let mut games = BTreeMap::new();
    let mut libraries_scanned = 0;

    for library in libraries {
        let library = match library {
            Ok(library) => library,
            Err(_) => continue,
        };
        libraries_scanned += 1;
        let library_path = library.path().display().to_string();
        for app in library.apps().filter_map(Result::ok) {
            if app.app_id == 0 {
                continue;
            }
            let install_path = library.resolve_app_dir(&app);
            let title = app
                .name
                .as_deref()
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("Steam App {}", app.app_id));
            let last_updated_at = app.last_updated.map(system_time_to_rfc3339);
            games.insert(
                app.app_id,
                ImportedGame {
                    app_id: app.app_id,
                    title,
                    icon_url: None,
                    cover_url: Some(cover_url(app.app_id)),
                    header_url: Some(header_url(app.app_id)),
                    playtime_minutes: 0,
                    playtime_recent_minutes: 0,
                    last_played_at: None,
                    ownership_source: "local".to_string(),
                    family_availability: "not_applicable".to_string(),
                    installation: Some(ImportedInstallation {
                        library_path: library_path.clone(),
                        install_path: install_path.display().to_string(),
                        size_on_disk: app.size_on_disk,
                        build_id: app.build_id,
                        last_updated_at,
                    }),
                },
            );
        }
    }

    if libraries_scanned == 0 {
        return Err(AppError::new(
            "steam_library_unreadable",
            "Steam está instalado, pero no se pudo leer ninguna de sus bibliotecas.",
        ));
    }

    Ok(LocalLibraryScan {
        steam_path,
        libraries_scanned,
        games: games.into_values().collect(),
    })
}

pub(crate) fn cover_url(app_id: u32) -> String {
    format!(
        "https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/library_600x900.jpg"
    )
}

pub(crate) fn header_url(app_id: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/header.jpg")
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

fn steamlocate_error(error: steamlocate::Error) -> AppError {
    AppError::new(
        "steam_local_read",
        format!("No se pudo leer la biblioteca local de Steam: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::{cover_url, header_url};

    #[test]
    fn generated_art_urls_are_https_and_app_scoped() {
        assert_eq!(
            cover_url(570),
            "https://shared.steamstatic.com/store_item_assets/steam/apps/570/library_600x900.jpg"
        );
        assert_eq!(
            header_url(570),
            "https://shared.steamstatic.com/store_item_assets/steam/apps/570/header.jpg"
        );
    }
}
