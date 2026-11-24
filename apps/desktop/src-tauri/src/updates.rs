//! The updater (DECISIONS F10 to F13; research note 35): Tauri's, checked against the project's release key
//! before anything is replaced.
//!
//! - **What it asks, and when:** the project's own releases on GitHub (`latest.json` of the newest release), a
//!   minute after the app starts and then once a day, while the core's `updates.check` is on. That request tells
//!   GitHub this computer's address and that it runs Demido; nothing else is sent.
//! - **What it installs:** nothing by itself. A newer version is offered in the tray and as a notice; the person
//!   installs it. Before anything is replaced the download is checked against the public half of the project's
//!   key, built into the app (minisign, as Tauri signs), and the version it was signed for must be the one
//!   announced, so that an older release cannot be passed off as a newer one (`requireSignedVersion`).
//! - **A build without the key** (`DEMIDO_UPDATE_KEY` at build time, the owner's to make) cannot update itself,
//!   and says so in the tray.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Url};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

use crate::{Shell, command, start_core, ui_folder};

/// The public half of the project's release key, given when the release is built.
pub const KEY: Option<&str> = option_env!("DEMIDO_UPDATE_KEY");
/// Where the newest release says what it is.
const AT: &str = "https://github.com/elpideus/demido-studio/releases/latest/download/latest.json";
const FIRST_LOOK_AFTER: Duration = Duration::from_secs(60);
const LOOK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

fn key() -> Option<&'static str> {
    KEY.map(str::trim).filter(|k| !k.is_empty())
}

/// Tauri's updater, when this build has the key.
pub fn plugin() -> Option<tauri::plugin::TauriPlugin<tauri::Wry, tauri_plugin_updater::Config>> {
    key().map(|k| tauri_plugin_updater::Builder::new().pubkey(k).build())
}

/// What the tray says about updates before one is offered.
pub fn tray_text() -> &'static str {
    if key().is_some() { "No update offered" } else { "This build cannot update itself" }
}

/// A development build's trial settings (core/dev/updates-trial.sh). A release reads none of them.
fn trial(name: &str) -> Option<String> {
    if cfg!(debug_assertions) { std::env::var(name).ok() } else { None }
}

fn said(text: &str) {
    if cfg!(debug_assertions) {
        eprintln!("demido-desktop: {text}");
    }
}

/// Whether the person lets Demido look: the core's `updates.check`, on unless turned off.
fn wanted(shell: &Shell) -> bool {
    command(&shell.core).args(["settings", "get", "updates.check", "--json"]).stderr(Stdio::null()).output().map_or(true, |o| String::from_utf8_lossy(&o.stdout).trim() != "false")
}

pub fn look_from_time_to_time(app: AppHandle, shell: Arc<Shell>) {
    if key().is_none() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let first = trial("DEMIDO_DESKTOP_UPDATE_AFTER").and_then(|s| s.parse().ok()).map_or(FIRST_LOOK_AFTER, Duration::from_secs);
        tokio::time::sleep(first).await;
        loop {
            if wanted(&shell) {
                look(&app, &shell).await;
            } else {
                said("updates.check is off: not looking");
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
    });
}

async fn look(app: &AppHandle, shell: &Arc<Shell>) {
    let at = trial("DEMIDO_DESKTOP_UPDATES_AT").unwrap_or_else(|| AT.to_string());
    let found = async {
        let url = Url::parse(&at).map_err(|e| e.to_string())?;
        app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?.build().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())
    }
    .await;
    match found {
        Ok(Some(update)) => offer(app, shell, update).await,
        Ok(None) => said("no newer version"),
        Err(e) => said(&format!("could not look for updates: {e}")),
    }
}

async fn offer(app: &AppHandle, shell: &Arc<Shell>, update: tauri_plugin_updater::Update) {
    let version = update.version.clone();
    said(&format!("version {version} is offered"));
    if trial("DEMIDO_DESKTOP_UPDATE_TRIAL").is_some() {
        // The trial fetches it and checks it as an install would, and replaces nothing.
        match update.download(|_, _| {}, || {}).await {
            Ok(bytes) => said(&format!("version {version}: signed with the key this build holds, for this version; {} bytes; not installed (a trial)", bytes.len())),
            Err(e) => said(&format!("version {version}: refused: {e}")),
        }
        return;
    }
    if let Some(item) = shell.update_item.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let _ = item.set_text(format!("Install Demido Studio {version} and restart"));
        let _ = item.set_enabled(true);
    }
    *shell.offered.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
    tell(app, &format!("Demido Studio {version} can be installed"), "From the tray: Install. It is checked against the project's key before anything is replaced.");
}

fn tell(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Installs the update offered, from the tray: downloaded, checked, the core stopped (it is inside the app and
/// is replaced with it), installed, and the app started again, which starts the new core.
pub fn install(app: AppHandle, shell: Arc<Shell>) {
    let Some(update) = shell.offered.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    tauri::async_runtime::spawn(async move {
        let version = update.version.clone();
        let bytes = match update.download(|_, _| {}, || {}).await {
            Ok(bytes) => bytes,
            Err(e) => return tell(&app, "The update was not installed", &format!("Demido Studio {version}: {e}. Nothing was replaced.")),
        };
        let _ = command(&shell.core).arg("stop").stdout(Stdio::null()).stderr(Stdio::null()).status();
        match update.install(bytes) {
            Ok(()) => app.restart(),
            Err(e) => {
                tell(&app, "The update was not installed", &format!("Demido Studio {version}: {e}. Demido is started again as it was."));
                let _ = start_core(&shell.core, ui_folder(&app));
            }
        }
    });
}
