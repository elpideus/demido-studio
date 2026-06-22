mod agent;
mod commands;
mod db;
mod mcp;
mod providers;
mod secrets;
mod skills;
mod streaming;

use commands::AppState;
use mcp::McpManager;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let app_dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&app_dir).ok();
            std::fs::create_dir_all(app_dir.join("skills")).ok();
            let db_path = app_dir.join("demido.db");
            let conn = db::init(&db_path).expect("DB init failed");
            let secrets = secrets::Secrets::new(app_dir);
            let mut mcp = McpManager::new();
            if let Ok(servers) = crate::db::mcp_servers::list(&conn) {
                let _ = mcp.load_servers(servers);
            }
            let http_client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("Failed to build HTTP client");
            app.manage(AppState {
                conn: Mutex::new(conn),
                secrets,
                mcp: Mutex::new(mcp),
                active_cancel: Mutex::new(None),
                http_client,
                pending_permission: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_conversations,
            commands::create_conversation,
            commands::delete_conversation,
            commands::update_conversation_title,
            commands::list_messages,
            commands::list_providers,
            commands::upsert_provider,
            commands::delete_provider,
            commands::get_settings,
            commands::set_setting,
            commands::get_secret,
            commands::set_secret,
            commands::search_conversations,
            commands::list_models,
            commands::list_model_capabilities,
            commands::raw_provider_models_json,
            commands::get_model_reasoning,
            commands::send_message,
            commands::cancel_stream,
            commands::list_mcp_servers,
            commands::save_mcp_servers,
            commands::list_mcp_tools,
            commands::test_mcp_server,
            commands::test_provider,
            commands::list_model_overrides,
            commands::upsert_model_override,
            commands::batch_upsert_model_overrides,
            commands::delete_messages_after,
            commands::update_message_content,
            commands::delete_messages_from,
            commands::delete_message,
            commands::continue_generation,
            commands::set_agent_mode,
            commands::set_working_directory,
            commands::respond_to_permission,
            commands::export_conversation,
            commands::open_devtools,
            commands::fs_list_dir,
            commands::fs_read_file,
            commands::fs_read_file_base64,
            commands::save_file_base64,
            commands::copy_file_to_clipboard,
            commands::fs_walk,
            commands::fs_rename,
            commands::fs_delete,
            commands::fs_copy_dir,
            skills::list_skills,
            skills::delete_skill,
        ])
        .run(tauri::generate_context!())
        .expect("Tauri app failed");
}
