use tauri::Emitter;

use super::St;
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
) -> CmdResult<ProviderView> {
    let config = state
        .providers
        .add(kind, name, api_key, model_group.unwrap_or_default())
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
    state.providers.remove(&id)?;
    let _ = state.app.emit(CHANGED_EVENT, state.models.list());
    Ok(())
}
