use tauri::Emitter;

use super::St;
use crate::claude_code;
use crate::error::{AppError, CmdResult};
use crate::models::CHANGED_EVENT;
use crate::providers::{ModelGroup, ProviderKind, ProviderPatch, ProviderUsage, ProviderView};

fn view(state: &St<'_>, id: &str) -> CmdResult<ProviderView> {
    let _ = state.app.emit(CHANGED_EVENT, state.models.list());
    state
        .providers
        .views()
        .into_iter()
        .find(|p| p.config.id == id)
        .ok_or_else(|| AppError::msg("That provider no longer exists."))
}

/// Reads models.dev in the background (what cloud models can do), then shows the result.
fn refresh_capabilities(state: &St<'_>) {
    let state = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        if state.models.refresh_cloud_catalog(&state.http, true).await {
            let _ = state.app.emit(CHANGED_EVENT, state.models.list());
        }
    });
}

#[tauri::command]
pub fn list_providers(state: St<'_>) -> Vec<ProviderView> {
    state.providers.views()
}

#[tauri::command]
pub async fn provider_usage(state: St<'_>) -> CmdResult<Vec<ProviderUsage>> {
    Ok(state.providers.usage().await)
}

#[tauri::command]
pub async fn add_provider(
    state: St<'_>,
    kind: ProviderKind,
    name: Option<String>,
    api_key: String,
    model_group: Option<ModelGroup>,
    claude_code_profile: Option<String>,
) -> CmdResult<ProviderView> {
    let config = state
        .providers
        .add(
            kind,
            name,
            api_key,
            model_group.unwrap_or_default(),
            claude_code_profile,
        )
        .await?;
    refresh_capabilities(&state);
    view(&state, &config.id)
}

#[tauri::command]
pub async fn update_provider(state: St<'_>, id: String, patch: ProviderPatch) -> CmdResult<ProviderView> {
    state.providers.update(&id, patch).await?;
    view(&state, &id)
}

#[tauri::command]
pub async fn refresh_provider(state: St<'_>, id: String) -> CmdResult<ProviderView> {
    state.providers.refresh_models(&id).await?;
    refresh_capabilities(&state);
    view(&state, &id)
}

#[tauri::command]
pub fn remove_provider(state: St<'_>, id: String) -> CmdResult<()> {
    let removed = state.providers.get(&id);
    state.providers.remove(&id)?;
    // A removed Claude subscription signs out and its profile's folder goes.
    if let Some(config) = removed.filter(|c| c.kind == ProviderKind::ClaudeCode) {
        let profile = config.claude_code_profile().to_string();
        tauri::async_runtime::spawn(async move { claude_code::forget(&profile).await });
    }
    let _ = state.app.emit(CHANGED_EVENT, state.models.list());
    Ok(())
}

/// Whether Claude Code is installed and a profile signed in, and how to install it on this
/// system. Without a profile, asks about the first one no provider uses, for a new subscription.
#[tauri::command]
pub async fn claude_code_status(state: St<'_>, profile: Option<String>) -> CmdResult<claude_code::Status> {
    let profile = profile.unwrap_or_else(|| state.providers.free_claude_code_profile());
    Ok(claude_code::status(claude_code::checked_profile(&profile)?).await)
}

/// Starts signing a profile in; the answer carries the page to open.
#[tauri::command]
pub async fn claude_code_login(profile: String, console: Option<bool>) -> CmdResult<claude_code::LoginState> {
    claude_code::login_start(&profile, console.unwrap_or(false)).await
}

/// Passes on the code the sign-in page showed.
#[tauri::command]
pub async fn claude_code_login_code(code: String) -> CmdResult<claude_code::LoginState> {
    claude_code::login_code(&code).await
}

#[tauri::command]
pub async fn claude_code_login_state() -> claude_code::LoginState {
    claude_code::login_state().await
}

#[tauri::command]
pub fn claude_code_login_cancel() {
    claude_code::login_cancel();
}

#[tauri::command]
pub async fn claude_code_logout(profile: String) -> CmdResult<claude_code::Account> {
    claude_code::logout(&profile).await
}
