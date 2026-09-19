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

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use demido_chat::{Asking, Decision, Moment, Offering, Presence, Update};

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

/// A call waiting on the person.
///
/// An event rather than the return value of a command, for the same reason the
/// tokens are: the turn is already running when it happens, and nobody asked a
/// question this would be the answer to. What the window draws from it is a row
/// in the transcript rather than a dialog (`design/shell.md`), so it arrives on
/// the same channel the rest of the turn does.
const ASKING: &str = "chat://asking";

/// The calls waiting on somebody at the window, and the answers coming back.
///
/// The turn loop takes the approval as a **callback**, not a trait
/// ([#54](https://github.com/elpideus/demido-studio/issues/54)), and this is the
/// one real implementation of it: emit, then wait for a command to answer. The
/// rendezvous is a `oneshot` per call, because a decision is a question with one
/// answer and a channel that could deliver two would be a second approval
/// nobody gave.
///
/// It lives here rather than in [`Wiring`] on purpose. Every other thing in the
/// root is a subsystem the workspace could be built without Tauri around; this
/// is the window being the person, and it exists only because there is a window.
#[derive(Default)]
pub struct Approvals {
    /// Keyed by the call's position on the log, which is what a decision is
    /// recorded against. Calls are dispatched one at a time, so this holds at
    /// most one; keyed anyway, because "at most one" is the loop's property and
    /// not this map's.
    waiting: Mutex<HashMap<u64, oneshot::Sender<Decision>>>,
}

impl Approvals {
    /// Wait on the call at `seq`. The answer arrives through [`chat_decide`],
    /// or the receiver is dropped, which is what a stop does.
    fn waiting(&self, seq: u64) -> oneshot::Receiver<Decision> {
        let (tell, told) = oneshot::channel();
        self.waiting
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .insert(seq, tell);
        told
    }

    /// Take the answer to the call at `seq`, if anything is waiting for one.
    fn answer(&self, seq: u64, decision: Decision) -> bool {
        let sender = self
            .waiting
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .remove(&seq);
        // `send` fails when the turn stopped waiting, which a stop does. The
        // answer is dropped and nothing runs, which is the right outcome and
        // not an error worth reporting: the person answered a question that had
        // already been withdrawn.
        sender.is_some_and(|sender| sender.send(decision).is_ok())
    }

    /// Forget whatever is still waiting. Called when a turn ends, however it
    /// ended: a sender left behind is an answer with nowhere to go, and the row
    /// on screen is gone the moment the turn is over.
    fn settle(&self) {
        self.waiting
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clear();
    }
}

/// The transcript, which is a projection of the session log.
///
/// Asked once when the desk mounts. A chat is still there after a restart
/// because this is the same read either way, not because anything was restored.
#[tauri::command]
pub fn chat_transcript(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<Vec<Moment>> {
    Ok(wiring.chat.transcript()?)
}

/// What the tool picker draws: every group, with its tools, and the folder
/// they act in.
///
/// Only the shape. Which of them are on is the ladder's `tools.offered`, read
/// and written through the settings commands like any other value, so the
/// picker and a turn are looking at the same set.
#[tauri::command]
pub fn chat_tools(wiring: tauri::State<'_, Wiring>) -> Shelf {
    Shelf {
        groups: wiring.chat.groups(),
        workspace: wiring.chat.workspace().map(|root| shown(&root)),
    }
}

/// The tool picker's whole reading.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    groups: Vec<Offering>,
    /// `None` when the window has no folder, which is a backend handed no
    /// tools at all whatever the switches say
    /// ([#113](https://github.com/elpideus/demido-studio/issues/113)).
    workspace: Option<String>,
}

/// A path as a person reads it. The workspace root is canonical, which on
/// Windows is the verbatim `\\?\` form nobody types.
fn shown(path: &std::path::Path) -> String {
    let path = path.display().to_string();
    match path.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC") => rest.to_owned(),
        _ => path,
    }
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
    let outcome = wiring
        .chat
        .ask(
            &message,
            |update: Update| emit(&app, UPDATE, &update),
            // The person at the window, as a callback. The call goes out as an
            // event and the answer comes back through `chat_decide`; a stop
            // drops this future, which drops the receiver, and nothing is run.
            |asking: Asking| {
                let app = app.clone();
                async move {
                    // Registered before the event goes out: an answer that
                    // arrived between the two would be an answer nobody is
                    // waiting for.
                    let told = app.state::<Approvals>().waiting(asking.call);
                    emit(&app, ASKING, &asking);
                    // A window that went away without answering is a denial.
                    // Erring towards asking costs a click and erring the other
                    // way costs the thing (`docs/rules/tools.md`).
                    told.await.unwrap_or(Decision::Deny)
                }
            },
        )
        .await;

    // However the turn ended. A sender still in the map is an answer with
    // nowhere to go, and the row it belongs to is off the screen by now.
    app.state::<Approvals>().settle();
    outcome?;
    Ok(())
}

/// What the person answered about one call.
///
/// `call` is the call's position on the session log, which is what the row was
/// drawn from and what the decision is recorded against. A call nothing is
/// waiting on answers `false`: a stop withdraws the question, and a click that
/// lands a frame after one is a click on something that is no longer there.
#[tauri::command]
pub fn chat_decide(app: AppHandle, call: u64, decision: Decision) -> bool {
    app.state::<Approvals>().answer(call, decision)
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
