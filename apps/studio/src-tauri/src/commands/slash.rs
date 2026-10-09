use super::St;
use crate::error::CmdResult;
use crate::slash::{self, Outcome, SlashCommand};

#[tauri::command]
pub fn list_slash_commands(state: St<'_>) -> Vec<SlashCommand> {
    slash::list(&state.skills)
}

/// Runs a command typed in the composer (`/compact`, `/autocompact 12k`, a skill's command).
#[tauri::command]
pub fn run_slash_command(
    state: St<'_>,
    chat_id: Option<String>,
    text: String,
    model_id: String,
    attachment_ids: Option<Vec<String>>,
) -> CmdResult<Outcome> {
    slash::run(&state, chat_id, &text, model_id, attachment_ids.unwrap_or_default())
}
