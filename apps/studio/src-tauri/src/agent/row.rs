//! The tool message a call writes to. The agent loop and the running tool share one handle, so
//! a progress display, a mid-run approval, a cancel and the final result all land on the same
//! row instead of overwriting each other's copies.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{Approval, ChatEvent};
use crate::db::{Message, MessageStatus};
use crate::error::CmdResult;
use crate::state::AppState;

/// The turn was stopped while a tool waited for the person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancelled;

#[derive(Clone)]
pub struct ToolRow {
    state: Arc<AppState>,
    row: Arc<Mutex<Message>>,
}

impl ToolRow {
    pub fn new(state: Arc<AppState>, row: Message) -> Self {
        Self {
            state,
            row: Arc::new(Mutex::new(row)),
        }
    }

    pub fn id(&self) -> String {
        self.row.lock().id.clone()
    }

    /// Applies `f`, then saves and shows the row. The lock is held throughout so two updates
    /// can never save out of order.
    pub fn update(&self, f: impl FnOnce(&mut Message)) -> CmdResult<()> {
        let mut row = self.row.lock();
        f(&mut row);
        self.state.db.save_message(&row)?;
        self.state.emit_chat(ChatEvent::Message {
            chat_id: row.chat_id.clone(),
            message: row.clone(),
        });
        Ok(())
    }

    /// Shows `display` while the tool is still running (a download's progress, say).
    pub fn set_display(&self, display: Value) {
        if let Err(e) = self.update(|m| set_display(m, display)) {
            tracing::warn!("could not save a tool's progress: {e}");
        }
    }

    /// Asks the person mid-run. `card` is what the approval shows (`tool_result.approval`);
    /// `None` is the plain "run this?" question asked before a tool starts.
    pub async fn request_approval(
        &self,
        card: Option<Value>,
        cancel: &CancellationToken,
    ) -> Result<Approval, Cancelled> {
        if let Err(e) = self.update(|m| begin_approval(m, card)) {
            tracing::warn!("could not save an approval request: {e}");
        }
        let decision = self.state.agent.wait_approval(&self.id(), cancel).await;
        if let Err(e) = self.update(end_approval) {
            tracing::warn!("could not save an approval answer: {e}");
        }
        decision
    }

    pub fn cancel(&self) -> CmdResult<()> {
        self.update(mark_cancelled)
    }

    pub fn finish(&self, ok: bool, content: String, display: Value, elapsed_ms: i64) -> CmdResult<()> {
        self.update(|m| finish(m, ok, content, display, elapsed_ms))
    }
}

fn result_object(m: &mut Message) -> &mut serde_json::Map<String, Value> {
    if !m.tool_result.as_ref().is_some_and(Value::is_object) {
        m.tool_result = Some(json!({}));
    }
    m.tool_result
        .as_mut()
        .and_then(Value::as_object_mut)
        .expect("tool_result is an object")
}

pub(crate) fn set_display(m: &mut Message, display: Value) {
    result_object(m).insert("display".into(), display);
}

pub(crate) fn begin_approval(m: &mut Message, card: Option<Value>) {
    m.status = MessageStatus::AwaitingApproval;
    if let Some(card) = card {
        result_object(m).insert("approval".into(), card);
    }
}

pub(crate) fn end_approval(m: &mut Message) {
    m.status = MessageStatus::Running;
    result_object(m).remove("approval");
}

/// Stopped by the person: only the status and what the model reads change, so a progress
/// display (a download that keeps going in the background) stays visible.
pub(crate) fn mark_cancelled(m: &mut Message) {
    m.status = MessageStatus::Cancelled;
    m.content = json!({"error": "Stopped by the user."}).to_string();
}

pub(crate) fn finish(m: &mut Message, ok: bool, content: String, display: Value, elapsed_ms: i64) {
    let (label, args) = {
        let r = result_object(m);
        (r.get("label").cloned(), r.get("args").cloned())
    };
    let label = label.unwrap_or_else(|| json!(m.tool_name));
    m.content = content;
    m.status = MessageStatus::Done;
    m.tool_result = Some(json!({
        "label": label,
        "args": args.unwrap_or(Value::Null),
        "ok": ok,
        "display": display,
        "durationMs": elapsed_ms,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Role;

    fn running_row() -> Message {
        let mut m = Message::new("chat", 3, Role::Tool, "");
        m.tool_name = Some("market_download".into());
        m.status = MessageStatus::Running;
        m.tool_result = Some(json!({"label": "Downloading EURUSD history", "args": {"symbol": "EURUSD"}}));
        m
    }

    #[test]
    fn cancel_keeps_the_progress_display() {
        let mut m = running_row();
        set_display(
            &mut m,
            json!({"kind": "download", "jobId": "j1", "plan": {"requests": 10}}),
        );
        mark_cancelled(&mut m);
        assert_eq!(m.status, MessageStatus::Cancelled);
        assert!(m.content.contains("Stopped by the user"));
        let r = m.tool_result.as_ref().unwrap();
        assert_eq!(r["display"]["jobId"], "j1");
        assert_eq!(r["label"], "Downloading EURUSD history");
    }

    #[test]
    fn approval_shows_its_card_then_goes_away() {
        let mut m = running_row();
        set_display(&mut m, json!({"kind": "download", "jobId": null}));
        begin_approval(&mut m, Some(json!({"kind": "download", "plan": {}, "minimal": null})));
        assert_eq!(m.status, MessageStatus::AwaitingApproval);
        assert_eq!(m.tool_result.as_ref().unwrap()["approval"]["kind"], "download");
        end_approval(&mut m);
        assert_eq!(m.status, MessageStatus::Running);
        let r = m.tool_result.as_ref().unwrap();
        assert!(r.get("approval").is_none());
        assert_eq!(r["display"]["kind"], "download");
    }

    #[test]
    fn a_plain_approval_adds_no_card() {
        let mut m = running_row();
        begin_approval(&mut m, None);
        assert_eq!(m.status, MessageStatus::AwaitingApproval);
        assert!(m.tool_result.as_ref().unwrap().get("approval").is_none());
    }

    #[test]
    fn finish_replaces_the_display_and_keeps_label_and_args() {
        let mut m = running_row();
        set_display(&mut m, json!({"kind": "download", "jobId": "j1"}));
        finish(&mut m, true, "{}".into(), json!({"kind": "candles"}), 42);
        assert_eq!(m.status, MessageStatus::Done);
        let r = m.tool_result.as_ref().unwrap();
        assert_eq!(r["display"]["kind"], "candles");
        assert_eq!(r["args"]["symbol"], "EURUSD");
        assert_eq!(r["durationMs"], 42);
        assert_eq!(r["ok"], true);
    }

    #[test]
    fn finish_without_a_label_falls_back_to_the_tool_name() {
        let mut m = running_row();
        m.tool_result = None;
        finish(&mut m, false, "{}".into(), json!({"error": "x"}), 0);
        assert_eq!(m.tool_result.as_ref().unwrap()["label"], "market_download");
    }
}
