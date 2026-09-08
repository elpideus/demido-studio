//! The commands the desk calls, and the events it listens to.
//!
//! Everything here is a thin call onto [`demido_chat::Chat`]. The one decision
//! this file makes is which half of a conversation crosses the boundary as a
//! **call** and which as an **event**, and it is not arbitrary: a call is a
//! question with one answer, and an event is something that keeps happening.
//!
//! So the transcript, the presence and the stop are commands, and the tokens
//! are events. A turn that streamed its answer back as the return value of
//! `chat_send` would deliver the whole thing at once, which is a spinner with
//! extra steps.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use demido_chat::{Presence, Said, Update};

use crate::wiring::Wiring;

/// Tokens, as they are generated.
///
/// One name for one kind of thing. The window subscribes once and appends,
/// which is what lets it hold a growing answer in a ref rather than in state
/// (`web/src/chat/stream.ts`).
const UPDATE: &str = "chat://update";

/// What the composer may offer, as it changes.
///
/// Emitted as well as returned by [`chat_load`], because loading a model is
/// minutes and the states in the middle are the whole point: a window told only
/// the outcome shows an idle composer for all of them.
const PRESENCE: &str = "chat://presence";

/// The transcript, which is a projection of the session log.
///
/// Asked once when the desk mounts. A chat is still there after a restart
/// because this is the same read either way, not because anything was restored.
#[tauri::command]
pub fn chat_transcript(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<Vec<Said>> {
    Ok(wiring.chat.history()?)
}

/// What the composer should say about the model right now.
#[tauri::command]
pub fn chat_presence(wiring: tauri::State<'_, Wiring>) -> Presence {
    wiring.chat.presence()
}

/// Start the model, telling the window every state it passes through.
///
/// Never fails, and says so in its type: a backend that will not start is a
/// [`Presence::Failed`] the desk carries on around, and startup never blocks
/// (`AGENTS.md`). The state is reached through the handle rather than taken as
/// an argument because Tauri requires an async command that borrows its state
/// to return a `Result`, and a function that cannot fail returning one is a
/// caller writing an error branch that can never run.
#[tauri::command]
pub async fn chat_load(app: AppHandle) -> Presence {
    let wiring = app.state::<Wiring>();
    wiring
        .chat
        .load(|presence| emit(&app, PRESENCE, presence))
        .await
}

/// Send a message, and stream the answer back as events.
///
/// Returns when the turn is over. What it returns is deliberately nothing: the
/// answer arrived over [`UPDATE`] as it was generated, and handing the caller a
/// second copy of it is how a window ends up with two records of one turn.
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    wiring: tauri::State<'_, Wiring>,
    message: String,
) -> demido_core::Result<()> {
    wiring
        .chat
        .ask(&message, |update: Update| emit(&app, UPDATE, &update))
        .await?;
    Ok(())
}

/// Call off the generation in flight. `false` when there was none.
///
/// The partial answer and the stop are recorded by the turn loop, on the same
/// path a finished answer takes, so this command has nothing to record.
#[tauri::command]
pub fn chat_stop(wiring: tauri::State<'_, Wiring>) -> bool {
    wiring.chat.stop()
}

/// Tell the window, and carry on if it is not listening.
///
/// A failed emit is the webview being gone, which is a window that is closing.
/// There is nothing to do about it and nothing to stop for: the turn is still
/// being recorded, and the log is what the next launch reads.
fn emit(app: &AppHandle, name: &str, payload: impl Serialize + Clone) {
    if let Err(error) = app.emit(name, payload) {
        tracing::warn!(%error, name, "the window was not told");
    }
}
