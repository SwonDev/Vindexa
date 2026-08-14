pub mod achievements;
pub mod actions;
pub mod family;
pub mod local;
pub mod metadata_enrichment;
pub mod news_api;
pub mod openid;
pub mod secrets;
pub mod store_api;
pub mod web_api;

pub use actions::{GameAction, open_game_action, request_uninstall, reveal_installation};
pub use local::{detect as detect_local_steam, scan as scan_local_library};
pub use openid::authenticate as authenticate_openid;
pub use secrets::{delete_api_key, has_api_key, save_api_key};
pub use web_api::{fetch_saved_account, mark_sync_failed};
