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
    let steam = steamlocate::locate().map_err(|_error| {
        AppError::new(
            "steam_not_found",
            "No se encontró una instalación local de Steam.",
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

/// Carátula vertical del juego, **derivada por convención**.
///
/// Es una semilla, no la verdad: el escaneo local es síncrono y no sale a la
/// red, así que la única URL que puede componer aquí es la canónica. Acierta
/// para el catálogo antiguo y falla con 404 para buena parte del moderno, que
/// guarda cada archivo bajo un hash de contenido o con otro nombre. Quien tiene
/// el nombre real es el índice de la tienda: [`super::store_assets`] reescribe
/// esta columna en cuanto puede consultarlo, y la convención se queda como red
/// de seguridad para cuando no hay red.
///
/// Se pide la variante `_2x` porque **`library_600x900.jpg` no mide 600×900:
/// mide 300×450**, comprobado contra la CDN. En una pantalla de densidad doble
/// una carátula de 208 px lógicos necesita 416 px reales, así que la de 300 se
/// ampliaba y salía borrosa. Cuando un juego no publica la grande, la escalera
/// de `art_cache` baja sola al siguiente peldaño.
pub(crate) fn cover_url(app_id: u32) -> String {
    format!(
        "https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/library_600x900_2x.jpg"
    )
}

/// Cabecera apaisada por convención, con la misma reserva que [`cover_url`]:
/// el nombre real puede llevar hash (`/apps/<id>/<sha1>/header.jpg`) o sufijo
/// de idioma (`header_spanish.jpg`), y eso sólo lo dice el índice de la tienda.
pub(crate) fn header_url(app_id: u32) -> String {
    format!("https://shared.steamstatic.com/store_item_assets/steam/apps/{app_id}/header.jpg")
}

fn system_time_to_rfc3339(value: SystemTime) -> String {
    DateTime::<Utc>::from(value).to_rfc3339()
}

fn steamlocate_error(_error: steamlocate::Error) -> AppError {
    AppError::new(
        "steam_local_read",
        "No se pudo leer la biblioteca local de Steam.",
    )
}

#[cfg(test)]
mod tests {
    use super::{cover_url, header_url, steamlocate_error};
    use std::io;
    use std::path::PathBuf;

    #[test]
    fn generated_art_urls_are_https_and_app_scoped() {
        assert_eq!(
            cover_url(570),
            "https://shared.steamstatic.com/store_item_assets/steam/apps/570/library_600x900_2x.jpg"
        );
        assert_eq!(
            header_url(570),
            "https://shared.steamstatic.com/store_item_assets/steam/apps/570/header.jpg"
        );
    }

    #[test]
    fn local_scan_errors_do_not_expose_filesystem_paths() {
        let error = steamlocate_error(steamlocate::Error::Io {
            inner: io::Error::new(io::ErrorKind::PermissionDenied, "token=fixture-secret"),
            path: PathBuf::from("/Users/example/Library/Application Support/Steam/private.vdf"),
        });

        assert_eq!(error.code, "steam_local_read");
        assert_eq!(
            error.message,
            "No se pudo leer la biblioteca local de Steam."
        );
        assert!(!error.message.contains("/Users/example"));
        assert!(!error.message.contains("fixture-secret"));
    }
}
