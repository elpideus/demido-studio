//! The Tauri application: the only place in the workspace that knows Tauri
//! exists.
//!
//! Everything durable is Rust's (`docs/stack.md`), and everything Rust owns is
//! a crate under `crates/`. This package is the ceiling those tiles sit in: it
//! assembles the composition root, registers the commands the window may call,
//! and opens the window.

pub mod chat;
/// Only a debug build loads `devUrl`, so only a debug build has a dev server to
/// want. Compiled out of a release rather than merely unused there: a shipped
/// build carries neither this check nor the CDP relaxation below, and a module
/// left in to warn about itself is a warning every release build prints.
#[cfg(debug_assertions)]
mod dev_server;
pub mod wiring;

/// The port `scripts/drive.mjs` connects to. Debug builds only, and only when
/// the dev command merged `tauri.drive.conf.json`.
#[cfg(debug_assertions)]
const CDP_PORT: u16 = 9222;

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            boot_report,
            read_layout,
            remember_layout,
            chat::chat_transcript,
            chat::chat_presence,
            chat::chat_load,
            chat::chat_send,
            chat::chat_stop
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
