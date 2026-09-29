//! The Updates tab and the activity bar's update button.

use super::St;
use crate::error::CmdResult;
use crate::settings::UpdateChannel;
use crate::updater::UpdateStatus;

#[tauri::command]
pub fn update_status(state: St<'_>) -> UpdateStatus {
    state.updater.status()
}

/// Checks now and answers once the check is done. A failure shows in the status (phase `error`)
/// rather than as a failed command, so the Updates tab shows it in place.
#[tauri::command]
pub async fn check_for_updates(state: St<'_>) -> CmdResult<UpdateStatus> {
    Ok(state.updater.check(true).await)
}

/// "Update" and "Restart and update": installs what is ready, or downloads what was found. A
/// download ends in "ready"; the UI then restarts to install it.
#[tauri::command]
pub async fn apply_update(state: St<'_>) -> CmdResult<UpdateStatus> {
    state.updater.apply().await
}

#[tauri::command]
pub fn cancel_update(state: St<'_>) -> UpdateStatus {
    state.updater.cancel()
}

#[tauri::command]
pub fn set_update_preferences(
    state: St<'_>,
    channel: Option<UpdateChannel>,
    auto: Option<bool>,
) -> CmdResult<UpdateStatus> {
    state.updater.set_preferences(channel, auto)
}
