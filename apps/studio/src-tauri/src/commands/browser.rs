//! The Browser window's commands: its tabs, its address bar, where it shows the active tab, and
//! the person's answer when the assistant asks them for a check or a sign-in.

use super::St;
use crate::error::{AppError, CmdResult};
use crate::web::browser::{self, AttentionInfo, Owner, Rect, TabInfo};

#[tauri::command]
pub fn browser_tabs(state: St<'_>) -> Vec<TabInfo> {
    state.web.browser.tabs()
}

#[tauri::command]
pub fn browser_attention(state: St<'_>) -> Option<AttentionInfo> {
    state.web.browser.attention()
}

/// Opens a tab of the person's at what they typed (an address or a search), or a blank one.
#[tauri::command]
pub async fn browser_new_tab(state: St<'_>, input: Option<String>) -> CmdResult<u64> {
    let engine = state.settings.get().browser_search_engine;
    let url = input.as_deref().and_then(|i| browser::address(i, engine));
    state
        .web
        .browser
        .open(Owner::User, url, true)
        .await
        .map_err(AppError::msg)
}

#[tauri::command]
pub fn browser_close_tab(state: St<'_>, id: u64) {
    state.web.browser.close(id);
}

#[tauri::command]
pub fn browser_activate(state: St<'_>, id: u64) {
    state.web.browser.activate(id);
}

/// Goes where the person typed in the address bar: an address, or a search.
#[tauri::command]
pub fn browser_navigate(state: St<'_>, id: u64, input: String) -> CmdResult<()> {
    let engine = state.settings.get().browser_search_engine;
    let Some(url) = browser::address(&input, engine) else {
        return Ok(());
    };
    state.web.browser.navigate(id, url).map_err(AppError::msg)
}

/// Back, forward, stop or reload.
#[tauri::command]
pub fn browser_history(state: St<'_>, id: u64, action: String) -> CmdResult<()> {
    let b = &state.web.browser;
    match action.as_str() {
        "reload" => b.reload(id),
        "back" | "forward" | "stop" => b.history(id, &action),
        other => Err(format!("Unknown browser action {other}.")),
    }
    .map_err(AppError::msg)
}

/// Where the Browser window shows its active tab, in the page's CSS pixels, or `null` when it
/// shows none (closed, minimized, covered by another window or a menu).
#[tauri::command]
pub fn browser_place(state: St<'_>, rect: Option<Rect>) {
    state.web.browser.place(rect);
}

/// The person's answer to the assistant's request: done, or go on without them.
#[tauri::command]
pub fn browser_answer(state: St<'_>, done: bool) {
    state.web.browser.answer(done);
}

/// Shows a file the browser downloaded in its folder.
#[tauri::command]
pub fn browser_reveal_download(app: tauri::AppHandle, state: St<'_>, path: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = std::path::PathBuf::from(path);
    if !state.web.browser.downloaded(&path) {
        return Err(AppError::msg("That is not a file the browser downloaded."));
    }
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| AppError::msg(e.to_string()))
}

/// Forgets the browser's cookies, sign-ins and cached files.
#[tauri::command]
pub async fn browser_clear_data(state: St<'_>) -> CmdResult<()> {
    state.web.browser.clear_data().await.map_err(AppError::msg)
}
