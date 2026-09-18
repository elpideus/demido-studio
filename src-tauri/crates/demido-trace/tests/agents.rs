//! Every event names the agent it belongs to, and a sub-agent is a scope on
//! one log rather than a log of its own.
//!
//! `docs/rules/done.md`'s S4 spec puts this in the first commit of the slice
//! rather than in the one that needs it: "the monitor's scope is a query rather
//! than a retrofit", and a field added after the fact means rewriting the slice
//! that wrote the events without it. So the assertions here are about the main
//! session as much as about a child: an agent field that only children carry is
//! a field the monitor has to apologise for.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use demido_inference::{FinishReason, Options, Request, Role, ToolCall, Usage};
use demido_prompts::{Document, Tools};
use demido_trace::{AgentId, Body, Journal, Memory, Replay, Session, Source};

/// Each tool as a turn offers it: its document, and a shape for the prose.
fn documents(tools: &Tools, names: &[&str]) -> Vec<(Document, serde_json::Value)> {
    names
        .iter()
        .map(|name| {
            (
                tools.get(name).expect("a host tool"),
                serde_json::json!({ "type": "object" }),
            )
        })
        .collect()
}

/// A turn that says something, is sent, and comes back answered: the smallest
/// exchange that writes one of most kinds of event.
fn exchange(session: &Session<impl Journal>, said: &str, answered: &str) -> u64 {
    let mut turn = session.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.user(said).unwrap();
    let sent = turn.send().unwrap();
    session
        .completed(
            &sent,
            answered,
            "",
            // An answer that said nothing asked for a call instead, which is
            // what an exchange that is about to delegate really looks like.
            if answered.is_empty() {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            },
            Usage {
                prompt_tokens: 9,
                completion_tokens: 2,
            },
        )
        .unwrap();
    sent.seq
}

#[test]
fn every_event_of_the_main_session_names_the_main_agent() {
    let session = Session::new("named", Memory::new());
    let call = ToolCall {
        id: "call-1".into(),
        name: "read_file".into(),
        arguments: r#"{"path":"plan.txt"}"#.into(),
    };

    let seq = exchange(&session, "what is in plan.txt?", "");
    let called = session.called(1, seq, &call).unwrap();
    session
        .decided(1, called, demido_trace::Decision::Allow)
        .unwrap();
    session.returned(1, called, "a plan", false).unwrap();
    session
        .failed(1, "unavailable", "the backend went away")
        .unwrap();

    let events = session.journal().events().unwrap();
    assert!(
        events.len() > 5,
        "the exchange wrote something to assert about"
    );
    for event in &events {
        assert_eq!(
            event.agent,
            AgentId::main(),
            "event {} ({:?}) does not name the main session",
            event.seq,
            event.body
        );
    }

    let line = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(
        line["agent"], "main",
        "the agent is on the line, where the raw JSON tab reads it"
    );
}

#[test]
fn a_delegation_records_the_call_that_opened_it_and_the_depth() {
    let journal = Memory::new();
    let session = Session::new("delegated", journal.clone());
    let seq = exchange(&session, "read every file and summarise", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"summarise the tree"}"#.into(),
            },
        )
        .unwrap();

    let child = session.delegate(1, call).unwrap();

    let events = journal.events().unwrap();
    let opened = events
        .iter()
        .find(|event| matches!(event.body, Body::Delegated { .. }))
        .expect("a delegation is an event");
    assert_eq!(
        opened.agent,
        AgentId::main(),
        "the parent records that it delegated; the child's own events are the child's"
    );
    assert_eq!(
        opened.body,
        Body::Delegated {
            call,
            agent: child.agent().clone(),
            depth: 1,
        }
    );
    assert_eq!(
        child.depth(),
        1,
        "the child of the main session is depth one"
    );

    // The chain: the child asks for one of its own, which is the case v2 made
    // unreachable by construction and the depth limit is what stops here (#64).
    let childs_own = exchange(&child, "summarise the tree", "");
    let onward = child
        .called(
            1,
            childs_own,
            &ToolCall {
                id: "call-2".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"read one file"}"#.into(),
            },
        )
        .unwrap();
    let deeper = child.delegate(1, onward).unwrap();
    assert_eq!(deeper.depth(), 2, "depth is the indent, and it counts up");
    assert_ne!(deeper.agent(), child.agent(), "two children are two agents");
}

#[test]
fn a_synchronous_delegation_writes_no_returned_event() {
    // `agent/returned` is written **only** where a background answer is folded
    // in at a step boundary (#66). At the default parallelism the answer is the
    // tool's own result, and a returned event there would be a second record of
    // one answer.
    let journal = Memory::new();
    let session = Session::new("synchronous", journal.clone());
    let seq = exchange(&session, "delegate this", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();

    let child = session.delegate(1, call).unwrap();
    let answered = exchange(&child, "summarise the tree", "three files, one plan");
    session
        .returned(1, call, "three files, one plan", false)
        .unwrap();

    assert!(
        !journal
            .events()
            .unwrap()
            .iter()
            .any(|event| matches!(event.body, Body::Returned { .. })),
        "a delegation answered as a tool result is not folded in"
    );

    // And the background path, which is the one that writes it.
    let completion = journal
        .events()
        .unwrap()
        .iter()
        .rev()
        .find(|event| {
            event.agent == *child.agent() && matches!(event.body, Body::Completion { .. })
        })
        .expect("the child answered")
        .seq;
    assert!(completion > answered);
    let folded = session
        .folded_in(1, call, child.agent(), completion)
        .unwrap();
    let event = journal
        .events()
        .unwrap()
        .into_iter()
        .find(|event| event.seq == folded)
        .unwrap();
    assert_eq!(event.agent, AgentId::main(), "it is folded into the parent");
    assert_eq!(
        event.body,
        Body::Returned {
            call,
            agent: child.agent().clone(),
            answer: completion,
        },
        "the answer is named by position rather than copied out of the child"
    );
}

#[test]
fn a_child_writes_its_own_offered_set_even_when_it_is_its_parents() {
    // The case `docs/rules/tools.md`'s per-tool hash was written for: a
    // sub-agent's set changes per sub-agent while the wording does not. The
    // parent deduplicates an unchanged set against its own last one, and a
    // child has no last one, so the set it was given is in the log as its own
    // fact rather than as an inference from the agent above it.
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());
    let offered = documents(&tools, &["read_file", "list_directory"]);

    let journal = Memory::new();
    let session = Session::new("its-own-set", journal.clone());
    let mut turn = session.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.offer(demido_trace::Layer::Chat, &offered).unwrap();
    turn.user("delegate this").unwrap();
    let sent = turn.send().unwrap();
    let call = session
        .called(
            1,
            sent.seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();

    let child = session.delegate(1, call).unwrap();
    let mut turn = child.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.offer(demido_trace::Layer::Chat, &offered).unwrap();
    turn.user("summarise the tree").unwrap();
    turn.send().unwrap();

    let events = journal.events().unwrap();
    let sets: Vec<&demido_trace::Event> = events
        .iter()
        .filter(|event| matches!(event.body, Body::Offered { .. }))
        .collect();
    assert_eq!(sets.len(), 2, "the child records the set it was given");
    assert_eq!(sets[0].agent, AgentId::main());
    assert_eq!(sets[1].agent, *child.agent());

    let documents: Vec<&demido_trace::Event> = events
        .iter()
        .filter(|event| matches!(event.body, Body::ToolVersion { .. }))
        .collect();
    assert_eq!(
        documents.len(),
        4,
        "each agent writes the wording its own set names: two tools, twice"
    );
    assert!(documents[2..]
        .iter()
        .all(|event| event.agent == *child.agent()));
}

#[test]
fn a_childs_assembly_rebuilds_from_its_own_events() {
    let prompts = tempfile::tempdir().unwrap();
    let tools = Tools::open(prompts.path());
    let parents = documents(&tools, &["read_file", "write_file", "run_command"]);
    let childs = documents(&tools, &["read_file"]);

    let journal = Memory::new();
    let session = Session::new("rebuilt", journal.clone());
    let mut turn = session.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.offer(demido_trace::Layer::Chat, &parents).unwrap();
    turn.user("read every file and summarise").unwrap();
    let sent = turn.send().unwrap();
    let call = session
        .called(
            1,
            sent.seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();

    let child = session.delegate(1, call).unwrap();
    let mut turn = child.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.offer(demido_trace::Layer::Chat, &childs).unwrap();
    turn.user("summarise the tree").unwrap();
    let childs_own: Request = turn.send().unwrap().request;

    // The parent goes on recording after the child was opened, so what the
    // rebuild has to find is the child's own version events rather than
    // whatever wording sits nearest on a shared log.
    exchange(&session, "and while you are at it", "");

    let replay = Replay::of(&journal).unwrap();
    let rebuilt = replay
        .request(childs_own_assembly(&replay, child.agent()))
        .unwrap();
    // Serialised rather than compared as values, so this is the bytes that go
    // on the wire rather than two Rust values a `PartialEq` found equal. What
    // the offline suite cannot do is render both through a real chat template,
    // which is `a_real_model.rs`'s job and the same claim one layer out.
    assert_eq!(
        serde_json::to_string(&rebuilt).unwrap(),
        serde_json::to_string(&childs_own).unwrap(),
        "the child's assembly rebuilds byte for byte out of the child's own events"
    );
}

/// The assembly the agent recorded, on a log that holds more than one agent's.
fn childs_own_assembly(replay: &Replay, agent: &AgentId) -> u64 {
    replay
        .events()
        .iter()
        .rev()
        .find(|event| event.agent == *agent && matches!(event.body, Body::Assembly { .. }))
        .expect("the child sent something")
        .seq
}

#[test]
fn a_scope_is_a_filter_over_one_stream() {
    let journal = Memory::new();
    let session = Session::new("scoped", journal.clone());
    let seq = exchange(&session, "delegate this", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();
    let child = session.delegate(1, call).unwrap();
    exchange(&child, "summarise the tree", "three files, one plan");
    session
        .returned(1, call, "three files, one plan", false)
        .unwrap();

    let replay = Replay::of(&journal).unwrap();
    let said: Vec<String> = replay
        .history()
        .into_iter()
        .map(|exchange| exchange.text)
        .collect();
    assert_eq!(
        said,
        vec!["delegate this".to_owned()],
        "the parent's transcript is the parent's: a clean context is also a clean transcript"
    );

    let childs = replay.clone().scoped_to(child.agent().clone());
    let said: Vec<String> = childs
        .history()
        .into_iter()
        .map(|exchange| exchange.text)
        .collect();
    assert_eq!(
        said,
        vec![
            "summarise the tree".to_owned(),
            "three files, one plan".to_owned()
        ],
        "and selecting the agent reads its own"
    );

    assert_eq!(
        replay.events().len(),
        childs.events().len(),
        "one stream and two scopes: the monitor's stream stays whole, and what          the scope changes is the projections over it"
    );
    assert!(
        childs
            .events()
            .iter()
            .any(|event| event.agent == AgentId::main()),
        "a scope is a filter over one stream rather than a second stream"
    );

    let ledger = childs.ledger();
    assert_eq!(
        ledger.get(&Source::User).map(|tally| tally.events),
        Some(1),
        "the ledger is filtered where the ledger exists"
    );
}

#[test]
fn the_agents_are_the_monitors_left_column() {
    let journal = Memory::new();
    let session = Session::new("column", journal.clone());
    let seq = exchange(&session, "delegate twice", "");
    let first = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();
    let second = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-2".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();

    let one = session.delegate(1, first).unwrap();
    let onward = one
        .called(
            1,
            exchange(&one, "read one file", ""),
            &ToolCall {
                id: "call-3".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();
    let deeper = one.delegate(1, onward).unwrap();
    let two = session.delegate(1, second).unwrap();

    let agents = Replay::of(&journal).unwrap().agents();
    assert_eq!(
        agents
            .iter()
            .map(|agent| (agent.agent.clone(), agent.depth, agent.call))
            .collect::<Vec<_>>(),
        vec![
            (AgentId::main(), 0, None),
            (one.agent().clone(), 1, Some(first)),
            (deeper.agent().clone(), 2, Some(onward)),
            (two.agent().clone(), 1, Some(second)),
        ],
        "the main session first, then every agent in the order it was opened"
    );
    assert_eq!(agents[0].parent, None, "nothing opened the main session");
    assert_eq!(agents[2].parent.as_ref(), Some(one.agent()));
}

/// What the slot strip counts: an agent is generating from the moment its
/// request is sent until an answer or a failure comes back
/// ([#68](https://github.com/elpideus/demido-studio/issues/68)). A parent
/// waiting on a child it blocked for is not generating, because it holds no
/// slot while it waits, which is why this is read off each agent's own requests
/// rather than off which agents are open.
#[test]
fn an_agent_generates_between_a_request_and_its_answer() {
    let journal = Memory::new();
    let session = Session::new("generating", journal.clone());
    let seq = exchange(&session, "delegate this", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();
    let child = session.delegate(1, call).unwrap();

    let generating = |journal: &Memory| -> Vec<(AgentId, bool)> {
        Replay::of(journal)
            .unwrap()
            .agents()
            .into_iter()
            .map(|agent| (agent.agent, agent.generating))
            .collect()
    };
    assert_eq!(
        generating(&journal),
        vec![(AgentId::main(), false), (child.agent().clone(), false)],
        "the parent's answer asked for a call and came back, and the child has sent nothing"
    );

    let mut turn = child.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.user("summarise the tree").unwrap();
    turn.send().unwrap();
    assert_eq!(
        generating(&journal),
        vec![(AgentId::main(), false), (child.agent().clone(), true)],
        "a request sent and not answered is a slot in use, and it is the child's"
    );

    child
        .failed(1, "stopped", "the person pressed Stop")
        .unwrap();
    assert_eq!(
        generating(&journal),
        vec![(AgentId::main(), false), (child.agent().clone(), false)],
        "a failure ends a generation the way an answer does"
    );
}

#[test]
fn a_session_resumes_over_its_own_events_and_not_its_childrens() {
    // Both halves of what `resume` restores are per agent. A parent that took
    // its turn number from a sub-agent's log would number its next exchange
    // after somebody else's, and a parent that took a sub-agent's written
    // wordings for its own would stop writing paragraphs its own assemblies
    // name, which is a rebuild that fails on a log that looks complete.
    let prompts = tempfile::tempdir().unwrap();
    let paragraph = demido_prompts::Paragraphs::open(prompts.path())
        .get(demido_prompts::id::CONTEXT_TREE)
        .unwrap();

    let journal = Memory::new();
    let session = Session::new("resumed", journal.clone());
    let seq = exchange(&session, "delegate this", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();

    let child = session.delegate(1, call).unwrap();
    for _ in 0..3 {
        let mut turn = child.begin();
        turn.parameters("a-model", Options::default()).unwrap();
        turn.fragment(
            Source::System,
            Role::System,
            &paragraph,
            &[("root", "S:/work"), ("tree", "src/")],
        )
        .unwrap();
        turn.send().unwrap();
    }
    assert_eq!(
        child.begin().number(),
        4,
        "a sub-agent's exchanges are its own, counted from one"
    );

    let again = Session::new("resumed", journal.clone());
    again.resume().unwrap();
    assert_eq!(
        again.begin().number(),
        2,
        "the parent carries on after its own last turn, not after its child's"
    );

    let mut turn = again.begin();
    turn.parameters("a-model", Options::default()).unwrap();
    turn.fragment(
        Source::System,
        Role::System,
        &paragraph,
        &[("root", "S:/work"), ("tree", "src/")],
    )
    .unwrap();
    turn.send().unwrap();
    assert_eq!(
        journal
            .events()
            .unwrap()
            .iter()
            .filter(|event| matches!(event.body, Body::Version { .. })
                && event.agent == AgentId::main())
            .count(),
        1,
        "and it writes the wording its own assembly names, which its child's copy does not cover"
    );
}

#[test]
fn a_refusal_names_the_paragraph_that_stated_the_reason() {
    // The depth refusal is one of these (#64): a call that did not run, and the
    // wording that says why. What makes the monitor's row a projection rather
    // than a rendering decision is that the reason is readable from the log,
    // both as the text it put in the model's context and as the paragraph it
    // came from.
    let prompts = tempfile::tempdir().unwrap();
    let paragraph = demido_prompts::Paragraphs::open(prompts.path())
        .get(demido_prompts::id::TOOLS_OFF)
        .unwrap();

    let journal = Memory::new();
    let session = Session::new("refused", journal.clone());
    let seq = exchange(&session, "delegate this", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: "{}".into(),
            },
        )
        .unwrap();
    session
        .refused(1, call, &paragraph, &[("tool", "delegate_task")])
        .unwrap();

    let replay = Replay::of(&journal).unwrap();
    let moments = replay.transcript().unwrap();
    let called = moments
        .iter()
        .find_map(|moment| match moment {
            demido_trace::Moment::Called(called) => Some(called),
            demido_trace::Moment::Said(_) | demido_trace::Moment::Delegated(_) => None,
        })
        .expect("the call is a row");
    match called.outcome.as_ref().expect("it was answered") {
        demido_trace::Outcome::Refused { id, text } => {
            assert_eq!(id, demido_prompts::id::TOOLS_OFF, "which refusal it was");
            assert!(
                text.contains("delegate_task"),
                "and the reason as the model was given it"
            );
        }
        other => panic!("a refusal, not {other:?}"),
    }
}

/// A delegation is one moment in the parent's transcript: the task out, and
/// the sub-agent's answer back.
///
/// [#67](https://github.com/elpideus/demido-studio/issues/67). "A clean context
/// should also be a clean transcript": what the parent's chat shows is the task
/// going out and the answer coming back, and the child's own calls are not in
/// it. They are on the log, under the child's agent, which is the whole
/// difference between clean and hidden.
#[test]
fn a_delegation_is_one_moment_carrying_the_task_and_the_answer() {
    let journal = Memory::new();
    let session = Session::new("one-moment", journal.clone());
    let seq = exchange(&session, "When is the meeting?", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"Read notes.txt and say when the meeting is"}"#.into(),
            },
        )
        .unwrap();

    // The child runs the whole loop again: it is given the task, it reads a
    // file, and it answers.
    let child = session.delegate(1, call).unwrap();
    let asked = exchange(&child, "Read notes.txt and say when the meeting is", "");
    let childs_call = child
        .called(
            1,
            asked,
            &ToolCall {
                id: "call-2".into(),
                name: "read_file".into(),
                arguments: r#"{"path":"notes.txt"}"#.into(),
            },
        )
        .unwrap();
    child
        .returned(1, childs_call, "The meeting moved to Thursday.", false)
        .unwrap();
    exchange(&child, "and now?", "Thursday.");
    // The blocking path: what the child said is the call's own result.
    session.returned(1, call, "Thursday.", false).unwrap();

    let moments = Replay::of(&journal).unwrap().transcript().unwrap();
    let delegation = moments
        .iter()
        .find_map(|moment| match moment {
            demido_trace::Moment::Delegated(delegation) => Some(delegation),
            _ => None,
        })
        .expect("a delegation is a moment of its own");
    assert_eq!(delegation.seq, call, "the row is the call that asked");
    assert_eq!(delegation.agent, *child.agent());
    assert_eq!(
        delegation.task, "Read notes.txt and say when the meeting is",
        "the task as the child was given it, off the child's own log"
    );
    let answer = delegation.answer.as_ref().expect("the sub-agent answered");
    assert_eq!(answer.text, "Thursday.");
    assert!(!answer.failed);

    assert!(
        !moments.iter().any(|moment| matches!(
            moment,
            demido_trace::Moment::Called(called) if called.name == "delegate_task"
        )),
        "and it is not also an ordinary call row: one exchange, not two"
    );
    assert!(
        !moments.iter().any(|moment| matches!(
            moment,
            demido_trace::Moment::Called(called) if called.name == "read_file"
        )),
        "the child's own calls are not in the parent's transcript"
    );
}

/// A background delegation reads the same way, and the acknowledgement that
/// answered its call is not mistaken for the answer.
#[test]
fn a_folded_in_answer_is_the_one_the_sub_agent_gave() {
    let journal = Memory::new();
    let session = Session::new("folded", journal.clone());
    let seq = exchange(&session, "Summarise the tree.", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"Summarise the tree"}"#.into(),
            },
        )
        .unwrap();
    let child = session.delegate(1, call).unwrap();
    // Above the default the call is answered at once, with the paragraph that
    // says the work has gone out.
    session
        .returned(1, call, "The task has gone to a sub-agent.", false)
        .unwrap();
    exchange(&child, "Summarise the tree", "Three files, one plan.");
    let completion = journal
        .events()
        .unwrap()
        .iter()
        .rev()
        .find(|event| {
            event.agent == *child.agent() && matches!(event.body, Body::Completion { .. })
        })
        .expect("the child answered")
        .seq;
    session
        .folded_in(1, call, child.agent(), completion)
        .unwrap();

    let moments = Replay::of(&journal).unwrap().transcript().unwrap();
    let delegation = moments
        .iter()
        .find_map(|moment| match moment {
            demido_trace::Moment::Delegated(delegation) => Some(delegation),
            _ => None,
        })
        .expect("a delegation is a moment of its own whichever path it took");
    let answer = delegation.answer.as_ref().expect("it came back");
    assert_eq!(
        answer.text, "Three files, one plan.",
        "the answer is the sub-agent's own, never the acknowledgement that stood in for it"
    );
    assert_eq!(
        answer.seq, completion,
        "named by position, as the log names it"
    );
}

/// A delegation whose sub-agent is still working has no answer yet, and a row
/// that claimed one would be claiming the first thing the child happened to
/// say.
#[test]
fn a_delegation_still_running_has_no_answer_yet() {
    let journal = Memory::new();
    let session = Session::new("running", journal.clone());
    let seq = exchange(&session, "Summarise the tree.", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"Summarise the tree"}"#.into(),
            },
        )
        .unwrap();
    let child = session.delegate(1, call).unwrap();
    // A step of the child's own: it asked for a tool and has not answered.
    exchange(&child, "Summarise the tree", "");

    let moments = Replay::of(&journal).unwrap().transcript().unwrap();
    let delegation = moments
        .iter()
        .find_map(|moment| match moment {
            demido_trace::Moment::Delegated(delegation) => Some(delegation),
            _ => None,
        })
        .expect("the delegation is drawn from the moment it is opened");
    assert!(
        delegation.answer.is_none(),
        "a step of the child's own is not an answer to the parent"
    );
}

/// A sub-agent whose run ended badly answers with what went wrong, drawn as a
/// failure rather than as an answer.
#[test]
fn a_sub_agent_that_ended_badly_is_a_failed_answer() {
    let journal = Memory::new();
    let session = Session::new("ended-badly", journal.clone());
    let seq = exchange(&session, "Summarise the tree.", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"Summarise the tree"}"#.into(),
            },
        )
        .unwrap();
    let child = session.delegate(1, call).unwrap();
    exchange(&child, "Summarise the tree", "");
    child
        .failed(1, "step-limit", "the turn used every step it was allowed")
        .unwrap();
    session
        .returned(1, call, "the turn used every step it was allowed", true)
        .unwrap();

    let moments = Replay::of(&journal).unwrap().transcript().unwrap();
    let delegation = moments
        .iter()
        .find_map(|moment| match moment {
            demido_trace::Moment::Delegated(delegation) => Some(delegation),
            _ => None,
        })
        .expect("a delegation that failed is still a delegation");
    let answer = delegation.answer.as_ref().expect("it ended");
    assert!(
        answer.failed,
        "the ending is the failure the child recorded"
    );
    assert!(answer.text.contains("every step"));
}

/// A delegation nobody allowed is an ordinary call row with a stated reason:
/// no child was opened, so there is no exchange to draw.
#[test]
fn a_delegation_that_opened_no_child_is_an_ordinary_call_row() {
    let prompts = tempfile::tempdir().unwrap();
    let paragraph = demido_prompts::Paragraphs::open(prompts.path())
        .get(demido_prompts::id::TOOLS_DENIED)
        .unwrap();

    let journal = Memory::new();
    let session = Session::new("denied", journal.clone());
    let seq = exchange(&session, "Summarise the tree.", "");
    let call = session
        .called(
            1,
            seq,
            &ToolCall {
                id: "call-1".into(),
                name: "delegate_task".into(),
                arguments: r#"{"task":"Summarise the tree"}"#.into(),
            },
        )
        .unwrap();
    session
        .refused(1, call, &paragraph, &[("tool", "delegate_task")])
        .unwrap();

    let moments = Replay::of(&journal).unwrap().transcript().unwrap();
    assert!(
        !moments
            .iter()
            .any(|moment| matches!(moment, demido_trace::Moment::Delegated(_))),
        "nothing was delegated, so there is no delegation to draw"
    );
    assert!(
        moments.iter().any(|moment| matches!(
            moment,
            demido_trace::Moment::Called(called) if called.name == "delegate_task"
        )),
        "the call is a row with the reason it did not run"
    );
}
