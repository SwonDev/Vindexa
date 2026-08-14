mod art_cache;
mod commands;
mod db;
mod error;
mod models;
mod steam;
mod store_window;

use commands::AppState;
use db::Database;
use std::fs;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Manager;
use tokio::sync::{Mutex, RwLock};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?;
            fs::create_dir_all(&data_dir)?;
            fs::create_dir_all(&cache_dir)?;

            let database = Database::new(data_dir.join("vindexa.sqlite3"));
            let startup_recovery = db::recovery::StartupRecovery::prepare(database.clone());
            let metadata_enrichment =
                Arc::new(steam::metadata_enrichment::MetadataEnrichmentCoordinator::default());
            app.manage(AppState {
                database,
                startup_recovery: Arc::new(StdMutex::new(startup_recovery)),
                cache_dir,
                maintenance: Arc::new(RwLock::new(())),
                steam_login_lock: Arc::new(Mutex::new(())),
                steam_sync_lock: Arc::new(Mutex::new(())),
                metadata_enrichment,
                achievements_lock: Arc::new(Mutex::new(())),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_database_recovery_status,
            commands::select_database_recovery_backup,
            commands::refresh_database_recovery_backups,
            commands::restore_database_recovery_backup,
            commands::create_clean_database_after_recovery,
            commands::export_quarantined_database,
            commands::bootstrap,
            commands::list_games,
            commands::get_library_filter_options,
            commands::get_game_detail,
            commands::list_tags,
            commands::save_tag,
            commands::delete_tag,
            commands::set_game_tags,
            commands::list_game_sessions,
            commands::save_game_session,
            commands::delete_game_session,
            commands::save_personal_dates,
            commands::refresh_game_metadata,
            commands::start_metadata_enrichment,
            commands::metadata_enrichment_status,
            commands::refresh_game_achievements,
            commands::update_game,
            commands::bulk_update_status,
            commands::apply_library_drop,
            commands::undo_library_drop,
            commands::save_collection,
            commands::preview_smart_collection,
            commands::delete_collection,
            commands::list_smart_rules,
            commands::reorder_collections,
            commands::set_collection_games,
            commands::set_game_collections,
            commands::get_planner_overview,
            commands::move_planner_item,
            commands::move_planner_queue_item,
            commands::save_planner_item,
            commands::save_planner_capacity,
            commands::remove_planner_item,
            commands::save_status,
            commands::delete_status,
            commands::reorder_statuses,
            commands::save_planner_column,
            commands::delete_planner_column,
            commands::reorder_planner_columns,
            commands::import_local_steam,
            commands::start_steam_login,
            commands::save_steam_api_key,
            commands::delete_steam_api_key,
            commands::verify_saved_steam_api_key,
            commands::sync_steam_library,
            commands::list_family_catalog,
            commands::get_family_catalog_game,
            commands::unlink_steam,
            commands::recommend_game,
            commands::get_discovery_snapshot,
            commands::refresh_discovery_news,
            commands::save_reminder,
            commands::complete_reminder,
            commands::snooze_reminder,
            commands::dismiss_recommendation,
            commands::restore_recommendation,
            commands::get_database_diagnostics,
            commands::export_backup,
            commands::import_backup,
            commands::launch_game,
            commands::install_game,
            commands::uninstall_game,
            commands::open_store,
            commands::reveal_installation,
            commands::cache_game_art,
            commands::clear_art_cache,
            commands::save_preferences,
            commands::check_for_updates,
        ])
        .run(tauri::generate_context!())
        .expect("Vindexa no pudo iniciar el runtime de escritorio");
}
