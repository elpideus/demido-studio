use tauri::Emitter;

use super::St;
use crate::error::{AppError, CmdResult};
use crate::models::CHANGED_EVENT;
use crate::providers::{ProviderKind, ProviderPatch, ProviderView};

fn view(state: &St<'_>, id: &str) -> CmdResult<ProviderView> {
    let _ = state.app.emit(CHANGED_EVENT, state.models.list());
    state
        .providers
        .views()
        .into_iter()
        .find(|p| p.config.id == id)
        .ok_or_else(|| AppError::msg("That provider no longer exists."))
}

#[tauri::command]
pub fn list_providers(state: St<'_>) -> Vec<ProviderView> {
    state.providers.views()
}

#[tauri::command]
pub async fn add_provider(
    state: St<'_>,
    kind: ProviderKind,
    name: Option<String>,
    api_key: String,
) -> CmdResult<ProviderView> {
    let config = state.providers.add(kind, name, api_key).await?;
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
    view(&state, &id)
}

#[tauri::command]
pub fn remove_provider(state: St<'_>, id: String) -> CmdResult<()> {
    state.providers.remove(&id)?;
    let _ = state.app.emit(CHANGED_EVENT, state.models.list());
    Ok(())
}
