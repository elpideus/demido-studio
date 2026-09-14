//! `Scripted` against the contract suite.
//!
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): "Each
//! implementation's test file calls that function with itself. An
//! implementation that does not call it is not an implementation." A script is
//! what drives the turn loop with no card in the machine, so it is held to the
//! same promises `llama.cpp` is, and it needs no rig to be held to them.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use demido_inference::scripted::{Script, Scripted, Step};
use demido_inference::{contract, ToolCall};

/// A script that only ever answers.
#[tokio::test]
async fn a_script_that_answers_keeps_the_contract() {
    let script = Script::serving("scripted").then_say(&["Hel", "lo", "."]);
    contract::run::<Scripted>(script, "scripted").await;
}

/// The script the turn loop is driven by: a canned call, then a canned answer.
///
/// The call's reply opens with a sentence, the way a model usually announces a
/// call, which is also what gives the cancel cases something generated to keep.
#[tokio::test]
async fn a_script_that_calls_a_tool_and_then_answers_keeps_the_contract() {
    let script = Script::serving("scripted")
        .then(vec![
            Step::Say("Let me look.".into()),
            Step::Call(ToolCall {
                id: "call-1".into(),
                name: "read_file".into(),
                arguments: r#"{"path": "notes.txt"}"#.into(),
            }),
        ])
        .then_say(&["Thursday."]);
    contract::run::<Scripted>(script, "scripted").await;
}
