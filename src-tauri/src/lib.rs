//! The Tauri application: the only place in the workspace that knows Tauri
//! exists.
//!
//! Everything durable is Rust's (`docs/stack.md`), and everything Rust owns is
//! a crate under `crates/`. This package is the ceiling those tiles sit in: it
//! assembles the composition root, registers the commands the window may call,
//! and opens the window.

pub mod boot;
pub mod chat;
/// Only a debug build loads `devUrl`, so only a debug build has a dev server to
/// want. Compiled out of a release rather than merely unused there: a shipped
/// build carries neither this check nor the CDP relaxation below, and a module
/// left in to warn about itself is a warning every release build prints.
#[cfg(debug_assertions)]
mod dev_server;
pub mod settings;
pub mod wiring;

/// The port `scripts/drive.mjs` connects to. Debug builds only, and only when
/// the dev command merged `tauri.drive.conf.json`.
#[cfg(debug_assertions)]
const CDP_PORT: u16 = 9222;

use std::sync::atomic::{AtomicBool, Ordering};

use demido_shell::Shell;
use serde::Serialize;
use tauri::Manager;
use wiring::Wiring;

/// What the window is told at boot.
///
/// The version comes from the crate manifest rather than from a constant typed
/// out again, so the badge in the corner, the installer and the tag cannot come
/// to disagree (`docs/rules/versioning.md`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootReport {
    version: &'static str,
    /// True when the window is being driven for evidence, which is the only
    /// build where `withGlobalTauri` is set. See `docs/rules/done.md`.
    driven: bool,
}

/// What `scripts/drive.mjs` calls to prove the IPC channel is real rather than
/// merely present.
///
/// The window itself no longer calls it: the desk asks for its layout instead,
/// which is a real question with a real answer. This stays because the window
/// gate's driver asserts on it (`docs/rules/done.md`), and a channel proof that
/// depends on whatever the current screen happens to ask for is a proof that
/// breaks every time the screen changes.
#[tauri::command]
fn boot_report(app: tauri::AppHandle) -> BootReport {
    BootReport {
        version: demido_core::VERSION,
        driven: app.config().app.with_global_tauri,
    }
}

/// Start the sequence the splash draws, at the splash's word.
///
/// The window asks rather than being told, because the events are the whole
/// point: a sequence begun before the splash was listening would paint its
/// first two stages into nothing, and a splash that misses a stage is a splash
/// reporting a boot that did not happen. Called once; a second call is a
/// reload of the same window and is refused rather than run twice.
#[tauri::command]
fn boot_begin(app: tauri::AppHandle) {
    begin(&app);
}

/// The one place the sequence starts, whoever asks: the splash when it is
/// listening, the watchdog when the splash never speaks.
///
/// The guard is what makes those two callers safe to have. A boot that ran
/// twice would open the desk twice and close a splash that is already gone.
fn begin(app: &tauri::AppHandle) {
    if app.state::<Started>().0.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move { boot::run(handle).await });
}

/// Whether the sequence has been started. See [`begin`].
#[derive(Debug, Default)]
struct Started(AtomicBool);

/// How long the splash is given to start listening before the sequence runs
/// without it.
///
/// Startup never blocks (`AGENTS.md`), and that has to hold for the splash
/// itself: a window whose webview fails to load would otherwise be an app that
/// never opens its desk. Long enough for a cold WebView2 to boot on a slow
/// machine, short enough that nobody sits in front of a frozen splash.
const SPLASH_SPEAKS_WITHIN: std::time::Duration = std::time::Duration::from_secs(8);

/// The desk as it was left.
///
/// Answered from Rust rather than from the webview's storage, because a layout
/// belongs to a profile and a profile is a Windows user
/// (`docs/rules/profiles.md`). A layout this build cannot read is not reported
/// here: it has already been discarded, and what comes back is the default
/// desk. See
/// [`docs/decisions/0010-the-desk-remembers-itself.md`](../../docs/decisions/0010-the-desk-remembers-itself.md).
#[tauri::command]
fn read_layout(wiring: tauri::State<'_, Wiring>) -> Shell {
    wiring.desk.read().unwrap_or_default()
}

/// The desk as it looks now.
///
/// Called on every change the window makes, including the ones in the middle of
/// a drag. Which of them becomes a file is the debouncer's decision and not the
/// window's, so nothing in the frontend has to know how often it may speak.
#[tauri::command]
fn remember_layout(wiring: tauri::State<'_, Wiring>, shell: Shell) {
    wiring.desk.remember(shell);
}

/// Build the application and run it.
///
/// # Errors
///
/// Only for a failure that leaves nothing worth opening a window for. A
/// subsystem that cannot start is reported and skipped: startup never blocks
/// (`AGENTS.md`).
pub fn run() -> demido_core::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let context = tauri::generate_context!();

    // A debug build loads `devUrl`, so without the frontend dev server it
    // serves a blank page under the right window title and nothing says why.
    // `docs/rules/done.md` names that as the second trap of the window gate.
    // Checked before the window exists, so the failure is a sentence rather
    // than an empty rectangle.
    #[cfg(debug_assertions)]
    dev_server::require(context.config())?;

    // The window gate's driver reaches the webview over CDP, which WebView2
    // only opens when it is asked to on the command line. The ask lives here
    // rather than in `tauri.conf.json` or in a capability, and it is compiled
    // out of anything shipped, so a release build has neither this code nor a
    // permission to abuse. See `docs/rules/done.md`.
    //
    // The port opens on every debug build, and `withGlobalTauri` does not,
    // because those are two different switches and coupling them would hide
    // the exact fault this is built around: the internals object present, the
    // public handle missing, and a healthy app inspecting as a broken one.
    // Under `pnpm dev` the driver can connect and say precisely that. Under
    // `pnpm dev:drive`, which merges `tauri.drive.conf.json`, both are there.
    #[cfg(debug_assertions)]
    {
        std::env::set_var(
            "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
            format!("--remote-debugging-port={CDP_PORT}"),
        );
        tracing::info!(
            port = CDP_PORT,
            global_tauri = context.config().app.with_global_tauri,
            "debug build: the webview is debuggable"
        );
    }

    tauri::Builder::default()
        // Assembled here rather than before the builder, because the one thing
        // the composition root needs is the profile directory and Tauri is what
        // resolves it. It is still one place naming one implementation per
        // trait, which is what `docs/rules/tiles.md` asks for.
        .setup(|app| {
            let profile = app
                .path()
                .app_local_data_dir()
                .map_err(|error| demido_core::Error::unavailable("the profile directory", error))?;
            app.manage(Wiring::assemble(&profile)?);
            app.manage(boot::Failures::default());
            app.manage(Started::default());

            // The splash is expected to ask for the sequence itself, and this
            // is what happens when it cannot. See `SPLASH_SPEAKS_WITHIN`.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(SPLASH_SPEAKS_WITHIN).await;
                if !handle.state::<Started>().0.load(Ordering::SeqCst) {
                    tracing::warn!("the splash never asked for the sequence; starting it anyway");
                    begin(&handle);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot_report,
            boot_begin,
            boot::boot_stages,
            boot::boot_failures,
            read_layout,
            remember_layout,
            chat::chat_transcript,
            chat::chat_presence,
            chat::chat_load,
            chat::chat_send,
            chat::chat_stop,
            settings::settings_rows,
            settings::settings_set,
            settings::settings_clear
        ])
        .build(context)
        .map_err(|error| demido_core::Error::unavailable("the application window", error))?
        .run(|app, event| {
            // Give the card back on the way out.
            //
            // A `llama.cpp` server is killed when the handle to it is dropped,
            // and a process that exits does not drop anything: without this the
            // window closes and several gigabytes stay resident in VRAM with
            // nothing on screen left that could stop them. The one place that
            // is true of is here, because this is the only place that knows the
            // application is ending rather than a window closing.
            if matches!(event, tauri::RunEvent::Exit) {
                tracing::info!("giving the card back");
                tauri::async_runtime::block_on(app.state::<Wiring>().chat.shutdown());
            }
        });

    Ok(())
}
