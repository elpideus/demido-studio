use super::St;
use crate::agent::{Agent, Approval, ChatEvent, SendResult};
use crate::db::{Chat, Message, Trace};
use crate::error::CmdResult;

#[tauri::command]
pub fn list_chats(state: St<'_>) -> CmdResult<Vec<Chat>> {
    Ok(state.db.list_chats()?)
}

#[tauri::command]
pub fn get_messages(state: St<'_>, chat_id: String) -> CmdResult<Vec<Message>> {
    Ok(state.db.list_messages(&chat_id)?)
}

#[tauri::command]
pub fn send_message(state: St<'_>, chat_id: Option<String>, text: String, model_id: String) -> CmdResult<SendResult> {
    Agent::send(&state, chat_id, text, model_id)
}

#[tauri::command]
pub fn regenerate(state: St<'_>, chat_id: String, model_id: String) -> CmdResult<()> {
    Agent::regenerate(&state, &chat_id, model_id)
}

#[tauri::command]
pub fn edit_message(
    state: St<'_>,
    chat_id: String,
    message_id: String,
    text: String,
    model_id: String,
) -> CmdResult<Message> {
    Agent::edit(&state, &chat_id, &message_id, text, model_id)
}

#[tauri::command]
pub fn stop_turn(state: St<'_>, chat_id: String) {
    state.agent.stop(&chat_id);
}

#[tauri::command]
pub fn running_turns(state: St<'_>) -> Vec<String> {
    state.agent.running_chats()
}

#[tauri::command]
pub fn resolve_approval(state: St<'_>, message_id: String, decision: Approval) -> bool {
    state.agent.resolve_approval(&message_id, decision)
}

#[tauri::command]
pub fn stop_tool(state: St<'_>, message_id: String) -> bool {
    state.agent.stop_tool(&message_id)
}

#[tauri::command]
pub fn rename_chat(state: St<'_>, chat_id: String, title: String) -> CmdResult<Chat> {
    let title = title.trim();
    if !title.is_empty() {
        state.db.rename_chat(&chat_id, title)?;
    }
    let chat = state
        .db
        .get_chat(&chat_id)?
        .ok_or_else(|| crate::error::AppError::msg("That chat no longer exists."))?;
    state.emit_chat(ChatEvent::Chat { chat: chat.clone() });
    Ok(chat)
}

#[tauri::command]
pub fn pin_chat(state: St<'_>, chat_id: String, pinned: bool) -> CmdResult<Chat> {
    state.db.set_chat_pinned(&chat_id, pinned)?;
    let chat = state
        .db
        .get_chat(&chat_id)?
        .ok_or_else(|| crate::error::AppError::msg("That chat no longer exists."))?;
    state.emit_chat(ChatEvent::Chat { chat: chat.clone() });
    Ok(chat)
}

#[tauri::command]
pub fn delete_chat(state: St<'_>, chat_id: String) -> CmdResult<()> {
    state.agent.stop(&chat_id);
    state.db.delete_chat(&chat_id)?;
    let workspace = state.paths.workspace(&chat_id);
    if workspace.exists() {
        let _ = std::fs::remove_dir_all(workspace);
    }
    Ok(())
}

#[tauri::command]
pub fn get_trace(state: St<'_>, message_id: String) -> CmdResult<Option<Trace>> {
    Ok(state.db.trace_for_message(&message_id)?)
}

#[tauri::command]
pub fn chat_traces(state: St<'_>, chat_id: String) -> CmdResult<Vec<Trace>> {
    Ok(state.db.traces_for_chat(&chat_id)?)
}

#[tauri::command]
pub fn workspace_dir(state: St<'_>, chat_id: String) -> CmdResult<String> {
    let dir = state.paths.workspace(&chat_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir.to_string_lossy().into_owned())
}
