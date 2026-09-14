//! A call, its result, and a turn sent more than once as it steps.
//!
//! `design/windows.md`: the event stream must be sufficient to rebuild an
//! assembly, not merely to describe one. A turn that uses a tool is sent once
//! per step, each time carrying what the step before it produced, so every one
//! of those sends has to rebuild: the tools it offered, the calls an answer
//! asked for, and what came back from each.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_inference::{FinishReason, Options, Role, ToolCall, Usage};
use demido_prompts::{id, Paragraphs, Tools};
use demido_trace::{Body, Decision, Journal, Layer, Memory, Replay, Sent, Session, Source};
use serde_json::json;

fn read_file_shape() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": { "path": { "type": "string" } },
        "required": ["path"],
        "additionalProperties": false,
    })
}

/// A first step: the read_file tool on offer, a question, and the request.
fn first_step(session: &Session<Memory>, tools: &Tools) -> Sent {
    let mut turn = session.begin();
    turn.user("What does notes.txt say?").unwrap();
    turn.offer(
        Layer::Registry,
        &[(tools.get("read_file").unwrap(), read_file_shape())],
    )
    .unwrap();
    turn.parameters("scripted", Options::default()).unwrap();
    turn.send().unwrap()
}

fn a_call() -> ToolCall {
    ToolCall {
        id: "call-1".into(),
        name: "read_file".into(),
        arguments: r#"{"path": "notes.txt"}"#.into(),
    }
}

fn usage() -> Usage {
    Usage {
        prompt_tokens: 7,
        completion_tokens: 3,
    }
}

#[test]
fn a_call_and_its_result_are_two_events_each_with_a_source_and_a_weight() {
    let prompts = tempfile::tempdir().unwrap();
    let session = Session::new("two-events", Memory::new());
    let sent = first_step(&session, &Tools::open(prompts.path()));

    let answer = session
        .completed(&sent, "", "", FinishReason::ToolCalls, usage())
        .unwrap();
    let call = session.called(sent.turn, answer, &a_call()).unwrap();
    let result = session
        .returned(sent.turn, call, "1: The meeting moved to Thursday.", false)
        .unwrap();

    let events = session.journal().events().unwrap();
    let find = |seq: u64| events.iter().find(|event| event.seq == seq).unwrap();

    let called = find(call);
    assert!(matches!(called.body, Body::Call { completion, .. } if completion == answer));
    assert_eq!(called.source, Source::Tool);
    assert!(called.weight.tokens > 0, "a call occupies context");

    let returned = find(result);
    assert!(matches!(returned.body, Body::Result { call: of, failed: false, .. } if of == call));
    assert_eq!(returned.source, Source::Tool);
    assert!(returned.weight.tokens > 0, "a result occupies context");
}

#[test]
fn every_step_of_a_turn_rebuilds_the_request_it_sent() {
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());
    let session = Session::new("steps", Memory::new());
    let first = first_step(&session, &tools);

    assert_eq!(first.request.tools.len(), 1);
    assert_eq!(
        first.request.tools[0].parameters["properties"]["path"]["description"],
        json!(tools.get("read_file").unwrap().parameter("path").unwrap()),
        "the offered tool carries its document's prose on its shape"
    );

    let answer = session
        .completed(&first, "Let me look.", "", FinishReason::ToolCalls, usage())
        .unwrap();
    let call = session.called(first.turn, answer, &a_call()).unwrap();
    let result = session
        .returned(first.turn, call, "1: The meeting moved to Thursday.", false)
        .unwrap();
    let second = session.step(&first, &[answer, result]).unwrap();

    assert_eq!(
        second.turn, first.turn,
        "a step is the same turn sent again"
    );
    let messages = &second.request.messages;
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[1].role, Role::Assistant);
    assert_eq!(messages[1].content, "Let me look.");
    assert_eq!(messages[1].calls, vec![a_call()]);
    assert_eq!(messages[2].role, Role::Tool);
    assert_eq!(messages[2].answers.as_deref(), Some("call-1"));
    assert_eq!(messages[2].content, "1: The meeting moved to Thursday.");
    assert_eq!(second.request.tools, first.request.tools);

    let replay = Replay::of(session.journal()).unwrap();
    assert_eq!(replay.request(first.seq).unwrap(), first.request);
    assert_eq!(replay.request(second.seq).unwrap(), second.request);
    assert_eq!(
        replay.assembly(first.turn).unwrap(),
        second.request,
        "a turn rebuilds as the last thing it sent"
    );
}

#[test]
fn a_refusal_is_rebuilt_from_the_wording_it_was_sent_in() {
    let prompts = tempfile::tempdir().unwrap();
    let paragraphs = Paragraphs::open(prompts.path());
    let session = Session::new("refusal", Memory::new());
    let first = first_step(&session, &Tools::open(prompts.path()));

    let answer = session
        .completed(&first, "", "", FinishReason::ToolCalls, usage())
        .unwrap();
    let call = session.called(first.turn, answer, &a_call()).unwrap();
    session.decided(first.turn, call, Decision::Deny).unwrap();
    let denied = paragraphs.get(id::TOOLS_DENIED).unwrap();
    let refusal = session
        .refused(
            first.turn,
            call,
            &denied,
            &[(demido_prompts::catalog::TOOL, "read_file")],
        )
        .unwrap();
    let second = session.step(&first, &[answer, refusal]).unwrap();

    let told = &second.request.messages[2];
    assert_eq!(told.role, Role::Tool);
    assert_eq!(told.answers.as_deref(), Some("call-1"));
    assert_eq!(
        told.content,
        denied.fill(&[(demido_prompts::catalog::TOOL, "read_file")])
    );

    let line = serde_json::to_value(
        session
            .journal()
            .events()
            .unwrap()
            .into_iter()
            .find(|event| event.seq == refusal)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(line["event"], "tool/refusal");
    assert!(
        line.get("text").is_none(),
        "a refusal is Demido's wording, recorded by hash and refilled"
    );

    // An edit made afterwards does not rewrite what the model was told.
    paragraphs
        .set(id::TOOLS_DENIED, "Declined: {{tool}}.")
        .unwrap();
    assert_eq!(
        Replay::of(session.journal())
            .unwrap()
            .request(second.seq)
            .unwrap(),
        second.request
    );
}

#[test]
fn a_decision_says_which_of_the_three_and_always_is_read_back_by_tool() {
    let prompts = tempfile::tempdir().unwrap();
    let session = Session::new("decisions", Memory::new());
    let first = first_step(&session, &Tools::open(prompts.path()));
    let answer = session
        .completed(&first, "", "", FinishReason::ToolCalls, usage())
        .unwrap();

    let mut said = Vec::new();
    for (index, decision) in [Decision::Allow, Decision::Deny, Decision::Always]
        .into_iter()
        .enumerate()
    {
        let call = session
            .called(
                first.turn,
                answer,
                &ToolCall {
                    id: format!("call-{index}"),
                    name: format!("tool_{index}"),
                    arguments: "{}".into(),
                },
            )
            .unwrap();
        session.decided(first.turn, call, decision).unwrap();
        said.push(decision);
    }

    let events = session.journal().events().unwrap();
    let kinds: Vec<serde_json::Value> = events
        .iter()
        .filter(|event| matches!(event.body, Body::Decided { .. }))
        .map(|event| serde_json::to_value(event).unwrap()["decision"].clone())
        .collect();
    assert_eq!(kinds, vec![json!("allow"), json!("deny"), json!("always")]);
    assert!(events
        .iter()
        .filter(|event| matches!(event.body, Body::Decided { .. }))
        .all(|event| event.source == Source::User));

    assert_eq!(
        Replay::of(session.journal()).unwrap().always(),
        vec!["tool_2".to_owned()],
        "only an always is a standing answer, and it is kept by tool name"
    );
}

#[test]
fn the_conversation_carries_calls_and_results_and_the_transcript_skips_an_answer_that_only_called()
{
    let prompts = tempfile::tempdir().unwrap();
    let session = Session::new("carried", Memory::new());
    let first = first_step(&session, &Tools::open(prompts.path()));
    let answer = session
        .completed(&first, "", "", FinishReason::ToolCalls, usage())
        .unwrap();
    let call = session.called(first.turn, answer, &a_call()).unwrap();
    let result = session.returned(first.turn, call, "1: hi", false).unwrap();
    let second = session.step(&first, &[answer, result]).unwrap();
    let finished = session
        .completed(&second, "It says hi.", "", FinishReason::Stop, usage())
        .unwrap();

    let replay = Replay::of(session.journal()).unwrap();
    let user = replay.history()[0].seq;
    assert_eq!(replay.conversation(), vec![user, answer, result, finished]);

    let transcript: Vec<String> = replay
        .history()
        .into_iter()
        .map(|exchange| exchange.text)
        .collect();
    assert_eq!(
        transcript,
        vec!["What does notes.txt say?", "It says hi."],
        "an answer that said nothing and only called is not a bubble"
    );
}

#[test]
fn a_resumed_session_sends_with_the_set_already_in_force() {
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());
    let log = Memory::new();

    let first = first_step(&Session::new("resumed", log.clone()), &tools);

    let again = Session::new("resumed", log.clone());
    again.resume().unwrap();
    let second = first_step(&again, &tools);

    let assemblies: Vec<Option<u64>> = log
        .events()
        .unwrap()
        .into_iter()
        .filter_map(|event| match event.body {
            Body::Assembly { tools, .. } => Some(tools),
            _ => None,
        })
        .collect();
    assert_eq!(assemblies.len(), 2);
    assert!(assemblies[0].is_some());
    assert_eq!(
        assemblies[0], assemblies[1],
        "an unchanged set after a restart is the set already recorded, not a new one"
    );
    assert_eq!(second.request.tools, first.request.tools);
}
