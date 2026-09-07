//! The Tauri application: the only place in the workspace that knows Tauri
//! exists.
//!
//! Everything durable is Rust's (`docs/stack.md`), and everything Rust owns is
//! a crate under `crates/`. This package is the ceiling those tiles sit in: it
//! assembles the composition root, registers the commands the window may call,
//! and opens the window.

mod dev_server;
pub mod wiring;

/// The port `scripts/drive.mjs` connects to. Debug builds only, and only when
/// the dev command merged `tauri.drive.conf.json`.
#[cfg(debug_assertions)]
const CDP_PORT: u16 = 9222;

use serde::Serialize;
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

/// The first call the window makes, and the one the driver makes to prove the
/// IPC channel is real rather than merely present.
#[tauri::command]
fn boot_report(app: tauri::AppHandle) -> BootReport {
    BootReport {
        version: demido_core::VERSION,
        driven: app.config().app.with_global_tauri,
    }
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

    let wiring = Wiring::assemble()?;

    tauri::Builder::default()
        .manage(wiring)
        .invoke_handler(tauri::generate_handler![boot_report])
        .run(context)
        .map_err(|error| demido_core::Error::unavailable("the application window", error))
}
