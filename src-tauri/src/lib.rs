mod agent;
mod caps;
mod caveman;
pub mod cli;
mod commands;
mod db;
mod google_apis;
mod local;
mod mcp;
mod prompt;
mod providers;
mod reset;
mod secrets;
mod skills;
mod sources;
mod streaming;
mod vars;
mod web;

use commands::AppState;
use mcp::McpManager;
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(debug_assertions)]
    {
        builder = builder.plugin(tauri_plugin_mcp::init());
    }
    let builder = builder
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build());

    builder.setup(|app| {
            let app_dir = app.path().app_data_dir().expect("app data dir");
            std::fs::create_dir_all(&app_dir).ok();
            std::fs::create_dir_all(app_dir.join("skills")).ok();
            // Before the DB is opened or secrets are read: a reset scheduled last run.
            reset::apply_pending(&app_dir);
            let db_path = app_dir.join("demido.db");
            let conn = db::init(&db_path).expect("DB init failed");
            // Pre-file installs keep their prompt in the settings table; move it to the .md once.
            prompt::migrate_from_db(app.handle(), &conn);
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
                conn: std::sync::Arc::new(Mutex::new(conn)),
                secrets,
                mcp: Mutex::new(mcp),
                active_cancel: Mutex::new(None),
                http_client,
                pending_permission: Mutex::new(None),
                // Populated by the frontend's first `sync_skill_mcp_servers` — the enabled set
                // lives in prefs.json, so at startup the backend genuinely does not know it yet.
                enabled_skills: Mutex::new(vec![]),
                local_engine: local::engine::LocalEngine::default(),
                searxng_engine: local::searxng::SearxngEngine::default(),
            });
            // Detect any models already present in the (default or configured) folder.
            let handle = app.handle().clone();
            if let Some(state) = handle.try_state::<AppState>() {
                local::commands::scan_on_startup(&handle, &state);
            }
            local::searxng::start_on_startup(&handle);
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
            commands::get_setting,
            commands::set_setting,
            commands::reset_app_data,
            commands::get_secret,
            commands::set_secret,
            commands::search_conversations,
            commands::list_models,
            commands::list_model_capabilities,
            commands::lookup_model_caps,
            commands::set_model_caps_override,
            commands::raw_provider_models_json,
            commands::get_model_reasoning,
            commands::send_message,
            commands::cancel_stream,
            commands::fetch_link_previews,
            commands::list_mcp_servers,
            commands::save_mcp_servers,
            commands::sync_skill_mcp_servers,
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
            commands::set_caveman_level,
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
            prompt::get_system_prompt,
            prompt::set_system_prompt,
            prompt::get_system_prompt_path,
            prompt::list_prompt_vars,
            skills::list_skills,
            skills::delete_skill,
            skills::read_skill_files,
            skills::read_skill_command,
            skills::write_skill_file,
            local::commands::hf_list_quants,
            local::commands::download_local_model,
            local::commands::list_local_models,
            local::commands::delete_local_model,
            local::commands::local_runtime_ready,
            local::commands::install_local_runtime,
            local::commands::list_runtime_variants,
            local::commands::install_runtime_variant,
            local::commands::hf_trending_models,
            local::commands::hf_search_models,
            local::commands::hf_model_card,
            local::commands::get_models_dirs,
            local::commands::set_models_dirs,
            local::commands::scan_local_models,
            local::commands::local_running_model,
            local::commands::stop_local_engine,
            local::commands::preload_local_model,
            local::commands::setup_needed,
            local::commands::complete_setup,
            local::commands::python_available_version,
            local::commands::python_ready,
            local::commands::python_status,
            local::commands::install_python,
            local::commands::uninstall_python,
            local::commands::install_searxng,
            local::commands::uninstall_searxng,
            local::commands::searxng_status,
            local::commands::start_searxng,
            local::commands::stop_searxng,
            local::commands::graphify_status,
            local::commands::graphify_set_auto_build,
            local::commands::install_graphify,
            local::commands::uninstall_graphify,
            local::commands::build_graphify,
            local::commands::query_graphify,
            local::commands::graphify_graph_html,
            local::commands::graphify_get_positions,
            local::commands::graphify_set_positions,
            commands::list_accounts,
            commands::delete_account,
            commands::update_account_services,
            commands::has_google_credentials,
            commands::set_google_credentials,
            commands::initiate_google_oauth,
            commands::fetch_emails,
            commands::get_email_body,
            commands::trash_email,
            commands::set_email_read,
            commands::fetch_calendar_events,
            commands::create_calendar_event,
            commands::update_calendar_event,
            commands::fetch_contacts,
            commands::update_contact,
        ])
        .build(tauri::generate_context!())
        .expect("Tauri app failed")
        .run(|app, event| {
            // Kill the enclosed llama-server so it never outlives the app.
            if let tauri::RunEvent::Exit = event {
                if let Some(state) = app.try_state::<AppState>() {
                    state.local_engine.stop();
                    state.searxng_engine.stop();
                }
            }
        });
}
