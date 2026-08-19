mod agent;
mod art_cache;
mod browser;
mod commands;
mod db;
mod error;
mod keychain;
mod models;
mod steam;
mod store_window;
mod stores;
mod updates;

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
            // Presupuesto en disco de la caché de arte. Subir la calidad del
            // arte multiplica lo que ocupa, así que el techo se fija antes de
            // servir la primera imagen.
            if let Ok(connection) = database.open()
                && let Ok(preferences) = db::organization::load_preferences(&connection)
            {
                art_cache::set_max_cache_bytes(u64::from(preferences.art_cache_mib) * 1024 * 1024);
            }
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
            commands::get_rich_game_metadata,
            commands::get_drm_state_counts,
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
            commands::set_game_collections,
            commands::list_sync_runs,
            commands::refresh_steam_art,
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
            commands::list_game_dlc,
            commands::refresh_game_dlc,
            commands::set_dlc_owned,
            commands::set_dlc_hidden,
            commands::set_dlc_installed,
            commands::get_dlc_summary,
            commands::list_notification_rules,
            commands::save_notification_rule,
            commands::delete_notification_rule,
            commands::get_notification_inbox,
            commands::mark_notification_read,
            commands::mark_all_notifications_read,
            commands::dismiss_notification,
            commands::dismiss_all_notifications,
            commands::refresh_notification_events,
            commands::recompute_priorities,
            commands::explain_priority,
            commands::set_priority_lock,
            commands::list_priority_ranking,
            commands::learn_taste,
            commands::record_taste_feedback,
            commands::score_upcoming_releases,
            commands::list_upcoming_releases,
            commands::dismiss_upcoming_release,
            commands::list_wishlist_prices,
            commands::get_game_prices,
            commands::get_game_price_history,
            commands::forget_game_prices,
            commands::refresh_wishlist_prices,
            commands::archive_games,
            commands::unarchive_games,
            commands::list_archived_games,
            commands::count_archived_games,
            commands::is_game_archived,
            commands::list_saved_views,
            commands::save_saved_view,
            commands::delete_saved_view,
            commands::reorder_saved_views,
            commands::mark_saved_view_used,
            commands::list_curated_lists,
            commands::save_curated_list,
            commands::delete_curated_list,
            commands::reorder_curated_lists,
            commands::get_curated_list_detail,
            commands::add_curated_game,
            commands::update_curated_item,
            commands::remove_curated_game,
            commands::move_curated_item,
            commands::reorder_curated_items,
            commands::get_wishlist_overview,
            commands::save_wishlist_entry,
            commands::remove_wishlist_entry,
            commands::move_wishlist_entry,
            commands::reorder_wishlist_bucket,
            commands::import_steam_wishlist,
            commands::import_steam_wishlist_from_browser,
            commands::steam_family_session_status,
            commands::link_steam_family_session,
            commands::unlink_steam_family_session,
            commands::sync_steam_family_catalog,
            commands::list_game_videos,
            commands::save_game_video,
            commands::delete_game_video,
            commands::reorder_game_videos,
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
            commands::maintain_art_cache,
            commands::save_preferences,
            commands::check_for_updates,
            commands::agent_dispatch,
            commands::agent_confirm,
            commands::agent_undo,
            commands::agent_undo_as_client,
            commands::issue_agent_client,
            commands::rotate_agent_token,
            commands::set_agent_client_scopes,
            commands::set_agent_client_enabled,
            commands::revoke_agent_client,
            commands::list_agent_clients,
            commands::list_agent_audit,
            commands::detect_external_stores,
            commands::list_external_store_accounts,
            commands::scan_external_store,
            commands::scan_external_stores,
            commands::list_external_games,
            commands::set_external_game_match,
            commands::clear_external_game_match,
            commands::link_external_store,
            commands::unlink_external_store,
            commands::rematch_external_stores,
            commands::launch_external_game,
            commands::list_external_store_sessions,
            commands::begin_external_store_login,
            commands::sign_in_external_store,
            commands::complete_external_store_login,
            commands::sign_out_external_store,
            commands::sync_external_store_library,
            commands::itch_session_state,
            commands::save_itch_api_key,
            commands::import_itch_library,
            commands::sign_out_itch,
            commands::forget_itch_library,
        ])
        .run(tauri::generate_context!())
        .expect("Vindexa no pudo iniciar el runtime de escritorio");
}
