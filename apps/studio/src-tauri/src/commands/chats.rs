use super::St;
use crate::agent::{Agent, Approval, ChatEvent, SendResult};
use crate::attachments;
use crate::db::{Attachment, Chat, Message, Trace};
use crate::error::CmdResult;

#[tauri::command]
pub fn list_chats(state: St<'_>) -> CmdResult<Vec<Chat>> {
    Ok(state.db.list_chats()?)
}

#[tauri::command]
pub fn get_messages(state: St<'_>, chat_id: String) -> CmdResult<Vec<Message>> {
    let mut messages = state.db.list_messages(&chat_id)?;
    attachments::resolve_messages(&state.paths, &mut messages);
    Ok(messages)
}

#[tauri::command]
pub fn send_message(
    state: St<'_>,
    chat_id: Option<String>,
    text: String,
    model_id: String,
    attachment_ids: Option<Vec<String>>,
) -> CmdResult<SendResult> {
    Agent::send(&state, chat_id, text, model_id, attachment_ids.unwrap_or_default())
}

/// Adds a file from disk to the composer: copies it aside and reads it.
#[tauri::command]
pub async fn attach_file(state: St<'_>, path: String) -> CmdResult<Attachment> {
    attachments::stage_file(&state, std::path::PathBuf::from(path)).await
}

/// Adds bytes from the clipboard to the composer. The body is the file; the `x-name` header its
/// URI-encoded name.
#[tauri::command]
pub async fn attach_data(state: St<'_>, request: tauri::ipc::Request<'_>) -> CmdResult<Attachment> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        crate::bail_msg!("The pasted file arrived empty.");
    };
    if bytes.len() as u64 > attachments::MAX_FILE_BYTES {
        crate::bail_msg!("The pasted file is larger than 100 MB, the most a file can be.");
    }
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| "Pasted file".into());
    attachments::stage_bytes(&state, name, bytes.clone()).await
}

/// Removes a file from the composer before it was sent.
#[tauri::command]
pub fn discard_attachment(state: St<'_>, id: String) -> CmdResult<()> {
    attachments::discard(&state, &id)
}

/// Decodes `%XX` escapes (what `encodeURIComponent` produces).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3])
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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
