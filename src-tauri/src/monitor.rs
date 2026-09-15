//! The two questions the session monitor asks, and nothing else.
//!
//! Both are **calls**, by the rule `src/chat.rs` sets out: a call is a question
//! with one answer, and an event is something that keeps happening. The log is
//! read when the panel opens and again whenever a turn records something, which
//! the window already learns from `chat://update`. A monitor fed its own event
//! channel would be a second live copy of the session, and one copy is the
//! whole product claim.

use demido_chat::Assembly;
use demido_trace::Event;

use crate::wiring::Wiring;

/// The session log, whole, in order.
///
/// Every line, with nothing folded and nothing left out: the stream groups it
/// into turns and the detail pane's last tab is the raw JSON of one line, so
/// what crosses the boundary is the record rather than a summary of it
/// (`design/windows.md`).
#[tauri::command]
pub fn monitor_log(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<Vec<Event>> {
    Ok(wiring.chat.log()?)
}

/// The assembly as it stood at one event, and what became of each tool group
/// in it.
///
/// `at` is a position on the log, which is what a row in the stream was drawn
/// from. Nothing comes back for a moment before the first assembly was
/// composed, which is a real state and not an error: the first thing on a log
/// is the paragraph that went into a prompt that had not been sent yet.
#[tauri::command]
pub fn monitor_assembly(
    wiring: tauri::State<'_, Wiring>,
    at: u64,
) -> demido_core::Result<Option<Assembly>> {
    Ok(wiring.chat.assembly(at)?)
}
