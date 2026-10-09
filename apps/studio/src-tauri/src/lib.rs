//! Demido Studio's backend.
//!
//! `run` builds the app: it resolves where things live ([`paths`]), opens storage ([`db`],
//! [`settings`]), starts the long-lived services (local model runtime, skills watcher, market
//! data service, [`updater`]) and registers the [`commands`] the UI calls. Startup never blocks
//! on a service: one that fails is logged and the app still opens. The one thing that runs
//! before them is installing an update downloaded earlier, in automatic mode.

mod agent;
mod attachments;
mod commands;
mod db;
mod error;
mod llm;
mod market;
mod models;
mod paths;
mod providers;
mod runtime;
mod secrets;
mod settings;
mod shell;
mod skills;
mod state;
mod tools;
mod updater;

use std::sync::Arc;

use tauri::{Manager, RunEvent};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::agent::Agent;
use crate::db::Db;
use crate::market::MarketService;
use crate::models::ModelRegistry;
use crate::models::downloads::DownloadManager;
use crate::paths::AppPaths;
use crate::providers::ProviderStore;
use crate::runtime::LocalRuntime;
use crate::secrets::{HF_TOKEN, Secrets};
use crate::settings::SettingsStore;
use crate::skills::SkillRegistry;
use crate::state::AppState;
use crate::updater::Updater;

struct LogGuard(parking_lot::Mutex<Option<tracing_appender::non_blocking::WorkerGuard>>);

fn init_logging(paths: &AppPaths) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_env("DEMIDO_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,hyper=warn,reqwest=warn,tao=warn,wry=warn,notify=warn"));
    let appender = tracing_appender::rolling::daily(&paths.logs_dir, "demido.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer));
    let result = if cfg!(debug_assertions) {
        registry
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .try_init()
    } else {
        registry.try_init()
    };
    result.ok().map(|_| guard)
}

fn build_state(
    app: &tauri::AppHandle,
    paths: AppPaths,
    settings: Arc<SettingsStore>,
    launch: updater::Launch,
) -> anyhow::Result<Arc<AppState>> {
    let hardware = demido_hardware::detect();
    tracing::info!(
        data = %paths.data_dir.display(),
        install = ?paths.install_dir,
        gpu = ?hardware.primary_gpu().map(|g| &g.name),
        "starting Demido Studio {}",
        demido_core::brand::VERSION
    );

    // The UI shows avatars and workspace images through the asset protocol.
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_directory(&paths.data_dir, true);

    let http = reqwest::Client::builder()
        .user_agent(format!("DemidoStudio/{}", demido_core::brand::VERSION))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()?;
    let local_http = reqwest::Client::builder().no_proxy().build()?;

    let db = Arc::new(Db::open(&paths.db_file)?);
    if let Ok(n) = db.close_dangling_messages()
        && n > 0
    {
        tracing::info!("closed {n} messages left streaming by the previous session");
    }
    attachments::clear_staging(&paths, &db);
    // Attached files are read in a child process (this executable, see main.rs): a hostile file
    // that crashes or stalls its parser then costs only its own text.
    match std::env::current_exe() {
        Ok(exe) => demido_extract::isolate(
            exe,
            vec![demido_extract::CHILD_ARG.into()],
            std::time::Duration::from_secs(180),
            Some(runtime::job::adopt_std),
        ),
        Err(e) => tracing::warn!("attached files are read in-process: {e}"),
    }
    let secrets = Arc::new(Secrets::default());
    let providers = Arc::new(ProviderStore::load(
        paths.providers_file.clone(),
        secrets.clone(),
        http.clone(),
    ));
    let budget = AppState::memory_budget_gb(&hardware);
    let runtime_backend = paths
        .manifest
        .as_ref()
        .and_then(|m| m.runtime.as_ref())
        .map(|r| r.backend);
    let models = Arc::new(ModelRegistry::new(
        paths.clone(),
        settings.clone(),
        providers.clone(),
        budget,
        AppState::context_fits_gpu(&hardware, runtime_backend),
    ));
    models.rescan();

    let embedder = attachments::meaning::Embedder::new(
        db.clone(),
        models.clone(),
        paths.llama_server(),
        paths.logs_dir.clone(),
        attachments::meaning::for_this_computer(&hardware, runtime_backend),
    );
    let runtime = Arc::new(LocalRuntime::new(
        app.clone(),
        embedder.clone(),
        paths.llama_server(),
        paths.logs_dir.clone(),
    ));
    let skills = SkillRegistry::new(app.clone(), paths.skills_dir.clone(), paths.skills_state_file.clone());
    skills.seed_defaults(&paths.default_skills_dir());
    skills.rescan();
    skills.watch();

    let market = MarketService::new(
        app.clone(),
        paths.node(),
        paths.market_service(),
        paths.cache_dir.join("market"),
        paths.data_dir.join("pine"),
        paths.logs_dir.clone(),
        paths.tradingview_profile_dir.clone(),
        secrets.clone(),
    );

    let token_secrets = secrets.clone();
    let rescan_models = models.clone();
    let rescan_app = app.clone();
    let rescan_embedder = embedder.clone();
    let downloads = DownloadManager::new(
        app.clone(),
        paths.models_dir.clone(),
        Arc::new(move || token_secrets.get(HF_TOKEN)),
        Arc::new(move || {
            rescan_models.rescan();
            use tauri::Emitter;
            let _ = rescan_app.emit(models::CHANGED_EVENT, rescan_models.list());
            // It may have been the search model.
            rescan_embedder.wake();
        }),
    )?;

    let updater = Updater::new(app.clone(), http.clone(), settings.clone(), &paths, launch)?;

    Ok(Arc::new(AppState {
        app: app.clone(),
        paths,
        hardware,
        settings,
        db,
        secrets,
        providers,
        models,
        downloads,
        runtime,
        embedder,
        skills,
        market,
        mail,
        updater,
        agent: Agent::default(),
        http,
        local_http,
    }))
}

/// Background work after the window is up: preload the default model, restore the market
/// session and find the shell, so the first question does not pay for them, start checking for
/// updates, and find out what each model can do.
fn after_start(state: Arc<AppState>) {
    use tauri::Emitter;

    state.updater.spawn_scheduler();
    state.embedder.spawn();
    // Find the shell commands run in now, so neither the first command nor the Tools menu waits.
    tauri::async_runtime::spawn_blocking(|| {
        shell::detect();
    });
    let app = state.app.clone();
    tauri::async_runtime::spawn(state.models.clone().check_capabilities(move |list| {
        let _ = app.emit(models::CHANGED_EVENT, list);
    }));
    let catalog = state.clone();
    tauri::async_runtime::spawn(async move {
        if catalog.models.refresh_cloud_catalog(&catalog.http, false).await {
            let _ = catalog.app.emit(models::CHANGED_EVENT, catalog.models.list());
        }
    });
    tauri::async_runtime::spawn(async move {
        let market = state.market.clone();
        tauri::async_runtime::spawn(async move { market.warm_up().await });

        let settings = state.settings.get();
        if !settings.preload_default_model {
            return;
        }
        let Some(model) = state.models.default_model() else {
            return;
        };
        if model.source != models::ModelSource::Local || !state.runtime.available() {
            return;
        }
        match state.models.launch_spec(&model.id) {
            Ok(spec) => {
                if let Err(e) = state.runtime.ensure(&spec).await {
                    tracing::warn!("preloading {} failed: {e}", model.name);
                }
            }
            Err(e) => tracing::warn!("cannot preload {}: {e}", model.name),
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let paths = AppPaths::resolve()?;
            // Keeps the log writer flushing for the app's lifetime.
            app.manage(LogGuard(parking_lot::Mutex::new(init_logging(&paths))));
            if updater::setup_is_running() {
                // Setup is replacing the app's files right now and opens the app when it is done.
                tracing::info!("setup is installing or updating Demido Studio; not opening now");
                drop(app.state::<LogGuard>().0.lock().take());
                std::process::exit(0);
            }
            let settings = Arc::new(SettingsStore::load(paths.settings_file.clone()));
            let Some(launch) = updater::at_launch(&paths, &settings.get()) else {
                // The installer is starting and waits for this process to end; it opens the
                // updated app when it is done. Flush the log first: `exit` skips destructors.
                drop(app.state::<LogGuard>().0.lock().take());
                std::process::exit(0);
            };
            let state = build_state(app.handle(), paths, settings, launch)?;
            app.manage(state.clone());
            after_start(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app::app_info,
            commands::app::accept_disclaimer,
            commands::app::get_settings,
            commands::app::update_settings,
            commands::app::window_ready,
            commands::app::open_path,
            commands::app::reveal_path,
            commands::app::read_workspace_file,
            commands::app::list_tool_groups,
            commands::app::set_tool_group,
            commands::app::revoke_tool_permission,
            commands::chats::list_chats,
            commands::chats::get_messages,
            commands::chats::send_message,
            commands::chats::attach_file,
            commands::chats::attach_data,
            commands::chats::discard_attachment,
            commands::chats::regenerate,
            commands::chats::edit_message,
            commands::chats::stop_turn,
            commands::chats::running_turns,
            commands::chats::resolve_approval,
            commands::chats::stop_tool,
            commands::chats::rename_chat,
            commands::chats::pin_chat,
            commands::chats::delete_chat,
            commands::chats::get_trace,
            commands::chats::chat_traces,
            commands::chats::workspace_dir,
            commands::models::list_models,
            commands::models::rescan_models,
            commands::models::update_model,
            commands::models::set_models_enabled,
            commands::models::import_model_avatar,
            commands::models::set_default_model,
            commands::models::delete_model,
            commands::models::runtime_status,
            commands::models::runtime_logs,
            commands::models::load_model,
            commands::models::unload_model,
            commands::models::hf_search,
            commands::models::hf_repo_files,
            commands::models::recommended_models,
            commands::models::download_model,
            commands::models::list_downloads,
            commands::models::pause_download,
            commands::models::resume_download,
            commands::models::cancel_download,
            commands::models::clear_downloads,
            commands::models::has_hf_token,
            commands::models::set_hf_token,
            commands::models::model_folders,
            commands::models::suggested_model_folders,
            commands::models::add_model_folder,
            commands::models::remove_model_folder,
            commands::providers::list_providers,
            commands::providers::add_provider,
            commands::providers::update_provider,
            commands::providers::refresh_provider,
            commands::providers::remove_provider,
            commands::skills::list_skills,
            commands::skills::set_skill_enabled,
            commands::skills::delete_skill,
            commands::skills::read_skill_file,
            commands::skills::write_skill_file,
            commands::skills::create_skill,
            commands::skills::open_skills_folder,
            commands::market::market_status,
            commands::market::market_login,
            commands::market::market_logout,
            commands::market::market_search,
            commands::market::market_quote,
            commands::market::market_open_stream,
            commands::market::market_close_stream,
            commands::market::market_extend_stream,
            commands::market::market_indicator_catalog,
            commands::market::market_indicator_search,
            commands::market::market_indicator_layouts,
            commands::market::market_indicator_layout,
            commands::market::market_indicator_add,
            commands::market::market_indicator_remove,
            commands::market::market_pine_list,
            commands::market::market_pine_get,
            commands::market::market_pine_save,
            commands::market::market_pine_delete,
            commands::market::market_pine_check,
            commands::market::market_pine_publish,
            commands::market::market_pine_import,
            commands::market::market_bars_latest,
            commands::market::market_bars_older,
            commands::market::market_bars_freshen,
            commands::market::market_download_plan,
            commands::market::market_download_start,
            commands::market::market_download_pause,
            commands::market::market_download_resume,
            commands::market::market_download_cancel,
            commands::market::market_download_status,
            commands::market::market_download_list,
            commands::market::market_cache_summary,
            commands::market::market_cache_delete,
            commands::market::market_cache_recheck,
            commands::updates::update_status,
            commands::updates::check_for_updates,
            commands::updates::apply_update,
            commands::updates::cancel_update,
            commands::updates::set_update_preferences,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Demido Studio");

    app.run(|handle, event| {
        if let RunEvent::Exit = event
            && let Some(state) = handle.try_state::<Arc<AppState>>()
        {
            state.agent.stop_all();
            state.runtime.kill_now();
            state.embedder.kill_now();
            // The market service flushes its store and job records on `shutdown`; a download
            // left unflushed would redo its last second of work next time.
            let market = state.market.clone();
            tauri::async_runtime::block_on(market.shutdown(std::time::Duration::from_secs(2)));
            state.market.kill_now();
        }
    });
}
