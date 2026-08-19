mod agent;
mod localmodel;
mod mcp;
mod vindagent;
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

/// Conecta Vindexa a un agente local desde la terminal.
///
/// Hace lo mismo que el botón de Ajustes → Agentes: emite un testigo con los
/// ámbitos indicados y da de alta el servidor MCP en ese agente. Existe además
/// como orden porque hay quien prepara su máquina sin abrir ventanas, y porque
/// una instalación desatendida no puede depender de un clic.
///
/// El testigo no se imprime: viaja directo al proceso del agente.
pub fn run_connect_agent(host_id: &str, scopes: &[String]) -> i32 {
    let Some(path) = mcp::database_path() else {
        eprintln!("Vindexa: no se pudo determinar dónde vive la base de datos.");
        return 1;
    };
    let database = db::Database::new(path);
    let scopes: Vec<String> = if scopes.is_empty() {
        agent::scope::ALL_SCOPES
            .iter()
            .map(|scope| scope.as_str().to_owned())
            .collect()
    } else {
        scopes.to_vec()
    };
    let input = agent::NewAgentClient {
        name: host_id.to_owned(),
        kind: if host_id == "hermes" { "hermes" } else { "generic" }.to_owned(),
        scopes,
    };
    let issued = match database
        .open()
        .and_then(|mut connection| {
            agent::clients::issue(&mut connection, &input, agent::TokenPolicy::default())
        }) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Vindexa: no se pudo emitir el testigo: {}", error.message);
            return 1;
        }
    };
    match agent::hosts::connect(host_id, &issued.token) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(error) => {
            eprintln!("Vindexa: {}", error.message);
            1
        }
    }
}

/// Arranca Vindexa como servidor MCP en vez de como ventana.
///
/// Es la puerta por la que un agente local conduce la biblioteca hablando. Vive
/// en el mismo binario a propósito: así el empaquetado lo lleva solo y nunca se
/// queda desfasado respecto a la aplicación con la que comparte base de datos.
pub fn run_mcp() -> i32 {
    mcp::run()
}

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
            // Techo en disco de la caché de arte. Lo normal es que no haya
            // ninguno; se lee antes de servir la primera imagen por si quien usa
            // la aplicación fijó uno a mano.
            if let Ok(connection) = database.open()
                && let Ok(preferences) = db::organization::load_preferences(&connection)
            {
                art_cache::set_max_cache_bytes(u64::from(preferences.art_cache_mib) * 1024 * 1024);
            }
            // Vindexa se conecta sola a los agentes que encuentre: detecta, da
            // de alta y repara el enlace si la aplicación ha cambiado de sitio.
            // En un hilo aparte porque hablar con un agente puede tardar y la
            // ventana no puede esperar a eso para aparecer.
            {
                let database = database.clone();
                std::thread::spawn(move || {
                    // Al arrancar, la base está ocupada con las migraciones y la
                    // recuperación. Esto no tiene ninguna prisa: espera a que
                    // pase el temporal y reintenta un par de veces antes de
                    // rendirse, porque «la base estaba ocupada» no es un motivo
                    // para quedarse sin agente hasta el siguiente arranque.
                    for espera in [4_u64, 15, 45] {
                        std::thread::sleep(std::time::Duration::from_secs(espera));
                        match agent::autolink::reconcile(&database) {
                            Ok(report) => {
                                for label in &report.linked {
                                    eprintln!("Vindexa: conectada a {label}.");
                                }
                                for failure in &report.failed {
                                    eprintln!("Vindexa: no se pudo conectar a {failure}");
                                }
                                return;
                            }
                            Err(error) => {
                                eprintln!(
                                    "Vindexa: el enlace con agentes no pudo completarse ({}); se reintentará.",
                                    error.message
                                );
                            }
                        }
                    }
                });
            }

            // Los próximos lanzamientos se repasan solos cada doce horas.
            // Estaba todo escrito y sólo corría al pulsar un botón que nadie
            // sabía que había que pulsar: medido sobre una biblioteca real con
            // novecientos deseados, cero lanzamientos guardados y cero pesos
            // aprendidos. Una recomendación que hay que pedir a mano no
            // recomienda nada.
            {
                let database = database.clone();
                tauri::async_runtime::spawn(async move {
                    // Se espera a que la ventana esté servida: esto tarda
                    // sesenta peticiones a la tienda y no puede competir con la
                    // primera pintada.
                    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    match steam::upcoming::maintain_if_due(&database).await {
                        Ok(Some(report)) => {
                            eprintln!(
                                "Vindexa: próximos lanzamientos al día ({} revisados, {} por revisar).",
                                report.checked, report.pending
                            );
                        }
                        Ok(None) => {}
                        Err(error) => {
                            eprintln!(
                                "Vindexa: no se pudieron repasar los lanzamientos: {}",
                                error.message
                            );
                        }
                    }
                });
            }

            // Los encargos del agente. No hace nada mientras no haya ninguno
            // guardado, que es el caso de una instalación recién hecha: sin
            // encargos, este bucle sólo comprueba una lista vacía.
            {
                let database = database.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
                        let pendientes = vindagent::schedule::list(&database)
                            .map(|tasks| tasks.iter().any(|task| task.enabled))
                            .unwrap_or(false);
                        if !pendientes {
                            continue;
                        }
                        // Sin modelo no se inventa un resultado: el encargo
                        // queda pendiente para cuando lo haya.
                        let Some((base_url, model)) =
                            vindagent::current_target(&database).await
                        else {
                            continue;
                        };
                        match vindagent::schedule::run_due(&database, &base_url, &model).await {
                            Ok(report) if !report.ran.is_empty() || !report.failed.is_empty() => {
                                eprintln!(
                                    "Vindexa: encargos del agente ({} hechos, {} fallidos).",
                                    report.ran.len(),
                                    report.failed.len()
                                );
                            }
                            Ok(_) => {}
                            Err(error) => {
                                eprintln!(
                                    "Vindexa: los encargos no pudieron correr: {}",
                                    error.message
                                );
                            }
                        }
                    }
                });
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

            // La ventana abre ocupando el espacio disponible.
            //
            // El ajuste `maximized` de la configuración no basta en macOS: allí
            // «maximizar» es el *zoom* del sistema, que lleva la ventana al
            // tamaño que la aplicación declara como preferido y no al que cabe
            // en la pantalla. Pedirlo explícitamente al arrancar sí respeta el
            // área útil —sin tapar la barra de menús ni el Dock— y en Windows y
            // en Linux hace lo mismo que allí significa maximizar.
            //
            // Se ignora el fallo a propósito: una ventana que no ha podido
            // maximizarse sigue siendo una ventana perfectamente usable, y no
            // es motivo para no arrancar.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.maximize();
            }
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
            commands::set_collection_appearance,
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
            commands::refresh_upcoming_releases,
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
            commands::open_store_browser,
            commands::reveal_installation,
            commands::cache_game_art,
            commands::clear_art_cache,
            commands::list_artwork_targets,
            commands::get_art_cache_usage,
            commands::maintain_art_cache,
            commands::save_preferences,
            commands::check_for_updates,
            commands::agent_dispatch,
            commands::agent_confirm,
            commands::agent_undo,
            commands::agent_undo_as_client,
            commands::issue_agent_client,
            commands::detect_agent_hosts,
            commands::local_model_survey,
            commands::vindagent_chat,
            commands::speech_endpoint,
            commands::transcribe_dictation,
            commands::vindagent_config,
            commands::list_agent_tasks,
            commands::save_agent_task,
            commands::delete_agent_task,
            commands::save_vindagent_config,
            commands::suggest_local_models,
            commands::local_model_install_plan,
            commands::install_local_runtime,
            commands::agent_autolink_state,
            commands::set_agent_autolink_disabled,
            commands::connect_agent_host,
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
