//! Escaneo de punta a punta: del manifiesto en disco a la fila persistida.
//!
//! Los escáneres leen el disco, así que cada caso construye su propio
//! directorio temporal con manifiestos de mentira. **Ninguna prueba toca rutas
//! reales de la persona usuaria ni sale a la red**: se usan siempre los
//! `scan_sources` explícitos, nunca `detect_sources`, que sí mira el equipo.

use crate::stores::db::{ExternalGameRequest, list, list_accounts, persist_scan};
use crate::stores::test_support::{insert_steam_game, migrated_database};
use crate::stores::{ExternalStore, ScanStatus, epic, gog};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("crear carpeta");
    }
    fs::write(path, contents).expect("escribir fichero");
}

/// Crea una carpeta de instalación con su ejecutable, como la dejaría el
/// instalador real.
fn install_directory(root: &Path, name: &str, executable: &str) -> PathBuf {
    let directory = root.join(name);
    fs::create_dir_all(&directory).expect("crear instalación");
    fs::write(directory.join(executable), "binario").expect("escribir ejecutable");
    directory
}

fn epic_manifest(manifests: &Path, file: &str, app_name: &str, title: &str, install: &Path) {
    write(
        &manifests.join(file),
        &format!(
            r#"{{"FormatVersion": 0, "AppName": {app_name:?}, "DisplayName": {title:?},
                 "InstallLocation": {install:?}, "InstallSize": 2048,
                 "LaunchExecutable": "juego.exe", "bIsIncompleteInstall": false}}"#,
            install = install.to_str().expect("ruta utf-8")
        ),
    );
}

/// Escribe un `galaxy-2.0.db` con el esquema que Vindexa sabe leer y ninguna
/// fila, como el que deja un Galaxy recién instalado.
fn empty_galaxy_database(root: &Path) -> PathBuf {
    let storage = root.join("Galaxy").join("storage");
    fs::create_dir_all(&storage).expect("crear carpeta de Galaxy");
    let path = storage.join("galaxy-2.0.db");
    let connection = rusqlite::Connection::open(&path).expect("crear base de Galaxy");
    connection
        .execute_batch(
            "CREATE TABLE InstalledBaseProducts(productId INTEGER, installationPath TEXT);
             CREATE TABLE LimitedDetails(productId INTEGER, title TEXT, images TEXT);",
        )
        .expect("crear esquema de Galaxy");
    drop(connection);
    path
}

fn gog_info(install: &Path, product_id: &str, title: &str) {
    write(
        &install.join(format!("goggame-{product_id}.info")),
        &format!(
            r#"{{"gameId": {product_id:?}, "rootGameId": {product_id:?}, "name": {title:?},
                 "playTasks": [{{"type": "FileTask", "path": "juego.exe",
                                 "isPrimary": true, "category": "game"}}]}}"#
        ),
    );
}

#[test]
fn without_any_store_installed_nothing_is_persisted_and_the_reason_is_recorded() {
    let (_directory, mut connection) = migrated_database();

    for (scan, expected_code) in [
        (
            epic::scan_sources(&epic::EpicSources::default()).expect("escanear Epic ausente"),
            "epic_client_not_found",
        ),
        (
            gog::scan_sources(&gog::GogSources::default()).expect("escanear GOG ausente"),
            "gog_client_not_found",
        ),
    ] {
        assert_eq!(scan.status, ScanStatus::Unavailable);
        let report = persist_scan(&mut connection, &scan).expect("persistir ausencia");
        assert_eq!(report.status, "unavailable");
        assert_eq!(report.discovered, 0);
        assert_eq!(report.error_code.as_deref(), Some(expected_code));
        // El motivo viaja hasta la interfaz: una lista vacía sin explicación es
        // indistinguible de «no tienes juegos».
        assert!(report.error_message.is_some());
    }

    let page = list(&connection, &ExternalGameRequest::default()).expect("listar");
    assert_eq!(page.total, 0);

    let accounts = list_accounts(&connection).expect("listar cuentas");
    assert_eq!(accounts.len(), 2);
    for account in accounts {
        assert!(!account.linked);
        assert_eq!(account.game_count, 0);
        assert_eq!(account.last_scan_status.as_deref(), Some("unavailable"));
        assert!(account.last_scan_error_message.is_some());
    }
}

#[test]
fn an_installed_store_without_games_is_an_empty_library_not_a_failure() {
    let (_directory, mut connection) = migrated_database();
    let root = TempDir::new().expect("crear temporal");

    // Epic instalado: la carpeta de manifiestos existe y está vacía.
    let manifests = root.path().join("Manifests");
    fs::create_dir_all(&manifests).expect("crear carpeta de manifiestos");
    let epic_scan = epic::scan_sources(&epic::EpicSources {
        manifest_directories: vec![manifests],
        ..Default::default()
    })
    .expect("escanear Epic vacío");
    let report = persist_scan(&mut connection, &epic_scan).expect("persistir Epic vacío");
    assert_eq!(report.status, "success");
    assert_eq!(report.discovered, 0);
    assert_eq!(report.error_code, None);
    assert!(report.detected_root.is_some());

    // GOG instalado: Galaxy recién instalado tiene su esquema y ninguna fila.
    // Es la única evidencia de «GOG está aquí», porque una carpeta `GOG Games`
    // vacía no la deja el cliente: la puede haber creado cualquiera.
    let galaxy = empty_galaxy_database(root.path());
    let gog_scan = gog::scan_sources(&gog::GogSources {
        galaxy_databases: vec![galaxy],
        ..Default::default()
    })
    .expect("escanear GOG vacío");
    let report = persist_scan(&mut connection, &gog_scan).expect("persistir GOG vacío");
    assert_eq!(report.status, "success");
    assert_eq!(report.discovered, 0);
    assert_eq!(report.error_code, None);
    assert!(report.detected_root.is_some());
}

#[test]
fn a_corrupt_manifest_is_counted_and_never_stops_its_neighbours() {
    let (_directory, mut connection) = migrated_database();
    let root = TempDir::new().expect("crear temporal");

    let manifests = root.path().join("Manifests");
    let install = install_directory(root.path(), "Juego Sano", "juego.exe");
    epic_manifest(&manifests, "sano.item", "JuegoSano", "Juego Sano", &install);
    write(&manifests.join("roto.item"), "{ esto no es json");
    write(&manifests.join("vacio.item"), "");

    let scan = epic::scan_sources(&epic::EpicSources {
        manifest_directories: vec![manifests],
        ..Default::default()
    })
    .expect("escanear manifiestos mixtos");
    let report = persist_scan(&mut connection, &scan).expect("persistir");

    assert_eq!(report.status, "success");
    assert_eq!(report.discovered, 1);
    assert_eq!(report.skipped, 2);

    let page = list(&connection, &ExternalGameRequest::default()).expect("listar");
    assert_eq!(page.total, 1);
    assert_eq!(page.items[0].external_id, "JuegoSano");
    assert!(page.items[0].installed);

    // Un `.info` de GOG corrupto se comporta igual: se cuenta y el vecino sano
    // sobrevive.
    let games_root = root.path().join("GOG Games");
    let sane = install_directory(&games_root, "Juego GOG", "juego.exe");
    gog_info(&sane, "1207658924", "Juego de GOG");
    let broken = install_directory(&games_root, "Juego Roto", "juego.exe");
    write(&broken.join("goggame-1207658925.info"), "{ tampoco es json");

    let scan = gog::scan_sources(&gog::GogSources {
        install_roots: vec![games_root],
        ..Default::default()
    })
    .expect("escanear GOG mixto");
    let report = persist_scan(&mut connection, &scan).expect("persistir GOG");
    assert_eq!(report.discovered, 1);
    assert_eq!(report.skipped, 1);
}

#[test]
fn rescanning_the_same_disk_neither_duplicates_rows_nor_moves_the_discovery_date() {
    let (_directory, mut connection) = migrated_database();
    insert_steam_game(&connection, 22370, "Fallout 3");
    let root = TempDir::new().expect("crear temporal");

    let manifests = root.path().join("Manifests");
    let install = install_directory(root.path(), "Fallout 3", "juego.exe");
    epic_manifest(&manifests, "fo3.item", "Fallout3", "Fallout 3", &install);
    let sources = epic::EpicSources {
        manifest_directories: vec![manifests],
        ..Default::default()
    };

    let first = persist_scan(
        &mut connection,
        &epic::scan_sources(&sources).expect("primer escaneo"),
    )
    .expect("persistir el primero");
    assert_eq!(first.discovered, 1);
    assert_eq!(first.matched, 1);
    let before = list(&connection, &ExternalGameRequest::default()).expect("listar");

    // Tres pasadas más sobre exactamente el mismo disco.
    for _ in 0..3 {
        let report = persist_scan(
            &mut connection,
            &epic::scan_sources(&sources).expect("reescaneo"),
        )
        .expect("persistir el reescaneo");
        assert_eq!(report.discovered, 1);
        assert_eq!(report.expired, 0);
    }

    let after = list(&connection, &ExternalGameRequest::default()).expect("listar de nuevo");
    assert_eq!(after.total, 1);
    assert_eq!(before.total, after.total);
    // La fecha de descubrimiento es la del primer hallazgo: reescanear no
    // convierte un juego viejo en un juego nuevo.
    assert_eq!(after.items[0].discovered_at, before.items[0].discovered_at);
    assert_eq!(after.items[0].matched_app_id, Some(22370));

    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM external_games", [], |row| row.get(0))
        .expect("contar filas");
    assert_eq!(rows, 1);

    let accounts = list_accounts(&connection).expect("listar cuentas");
    let epic_account = accounts
        .iter()
        .find(|account| account.store == ExternalStore::Epic.as_str())
        .expect("cuenta de Epic");
    assert_eq!(epic_account.game_count, 1);
}
