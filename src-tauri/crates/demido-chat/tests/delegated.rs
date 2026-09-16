//! The child session: what a delegation actually opens, and what a stop reaches.
//!
//! [#63](https://github.com/elpideus/demido-studio/issues/63). A delegated task
//! is a **child session, not an interleaved one**: it runs the whole agent loop
//! again, in a fresh session sharing the parent's store, and its record is kept.
//! So everything here is asserted on one log, with the agent as the scope, and
//! on what the backend was handed rather than on what the loop meant to send.
//!
//! The whole file is driven by `demido_inference::scripted`, which passes the
//! same `Backend` contract `llama.cpp` does, so none of it needs a card. The
//! slice's model gate is [#69](https://github.com/elpideus/demido-studio/issues/69).

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::future::{ready, Ready};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use demido_chat::{Asking, Chat, Decision, Model, Moment, Toolbox};
use demido_inference::scripted::{Script, Scripted, Step};
use demido_inference::{FinishReason, Request, Role, Supervisor, ToolCall};
use demido_settings::{Memory as SettingsMemory, Scope, Settings};
use demido_tools::{delegation, files, Registry, Workspace};
use demido_trace::{AgentId, Body, Event, Journal, Memory, Replay};
use serde_json::json;

const SESSION: &str = "delegated";

/// A project with a file in it, one log, the script every agent in the chain
/// answers from, and the ladder.
struct Rig {
    project: tempfile::TempDir,
    prompts: tempfile::TempDir,
    log: Memory,
    script: Script,
    settings: Arc<Settings>,
}

impl Rig {
    fn new(script: Script) -> Self {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(
            project.path().join("notes.txt"),
            "The meeting moved to Thursday.\n",
        )
        .unwrap();
        Self {
            project,
            prompts: tempfile::tempdir().unwrap(),
            log: Memory::new(),
            script,
            settings: Arc::new(Settings::open(SettingsMemory::new())),
        }
    }

    /// A chat over the Files and Delegation groups, in the mode named on its own
    /// tier of the ladder.
    ///
    /// The Delegation group is wired the way the composition root wires it: both
    /// ends of the pair made together, the tool's end into the registry and the
    /// loop's end into the conversation. Nothing here stands in for a sub-agent,
    /// which is the point of this file.
    fn chat(&self, mode: &str) -> Chat<Scripted, Memory> {
        self.settings
            .set(
                &Scope::chat(SESSION),
                demido_settings::id::TOOLS_MODE,
                &json!(mode),
            )
            .unwrap();
        let (delegating, delegations) = demido_chat::delegations();
        let registry = Registry::open(Some(Workspace::open(self.project.path()).unwrap()))
            .with_group(files())
            .with_group(delegation(delegating));
        let log = self.log.clone();
        Chat::new(
            SESSION,
            move || Ok(log.clone()),
            Arc::new(Supervisor::new()),
            Some(Model {
                config: self.script.clone(),
                id: "scripted".into(),
            }),
            self.settings.clone(),
            Toolbox::open(registry, self.prompts.path()),
            delegations,
        )
    }

    fn events(&self) -> Vec<Event> {
        self.log.events().unwrap()
    }
}

fn call(id: &str, name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: arguments.to_string(),
    }
}

/// A generation that asks for one delegation and says nothing else.
fn delegates(id: &str, task: &str) -> Vec<Step> {
    vec![Step::Call(call(
        id,
        "delegate_task",
        json!({ "task": task }),
    ))]
}

fn nobody() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |asking: Asking| panic!("nobody should have been asked about {}", asking.tool)
}

/// Somebody at the window who allows everything, and remembers what they were
/// shown.
fn allowing(seen: &Arc<Mutex<Vec<String>>>) -> impl FnMut(Asking) -> Ready<Decision> + Send {
    let seen = seen.clone();
    move |asking: Asking| {
        seen.lock().unwrap().push(asking.tool);
        ready(Decision::Allow)
    }
}

/// Every event one agent wrote, in order.
fn wrote(events: &[Event], agent: &AgentId) -> Vec<Event> {
    events
        .iter()
        .filter(|event| &event.agent == agent)
        .cloned()
        .collect()
}

/// The agents a delegation opened, in the order they were opened: the agent, the
/// call that opened it, and its depth.
fn opened(events: &[Event]) -> Vec<(AgentId, u64, u32)> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            Body::Delegated { call, agent, depth } => Some((agent.clone(), *call, *depth)),
            _ => None,
        })
        .collect()
}

/// What came back from the call at `call`, if anything did.
fn result(events: &[Event], call: u64) -> Option<(String, bool)> {
    events.iter().find_map(|event| match &event.body {
        Body::Result {
            call: at,
            text,
            failed,
        } if *at == call => Some((text.clone(), *failed)),
        _ => None,
    })
}

fn user_messages(request: &Request) -> Vec<&str> {
    request
        .messages
        .iter()
        .filter(|message| message.role == Role::User)
        .map(|message| message.content.as_str())
        .collect()
}

fn names(request: &Request) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

/// A delegation opens a child session over the conversation's own store, with
/// the parent pointing at it, and the child's events carry the child's agent.
#[tokio::test]
async fn a_delegation_opens_a_child_session_sharing_the_parents_store() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Read the notes"))
        .then_say(&["Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    let children = opened(&events);
    let [(agent, call, depth)] = children.as_slice() else {
        panic!("one delegation opened one child: {children:?}");
    };
    assert_eq!(*depth, 1, "one level under the conversation");
    assert_eq!(agent, &AgentId::delegated(*call), "named after its call");

    // One store, and the child is a scope on it rather than a log of its own.
    let child = wrote(&events, agent);
    assert!(!child.is_empty(), "the child recorded its run");
    assert!(
        child
            .iter()
            .all(|event| event.session.to_string() == SESSION),
        "the child records into the conversation's session"
    );
    let last_child = child[child.len() - 1].seq;
    assert!(
        events
            .iter()
            .any(|event| event.agent == AgentId::main() && event.seq > last_child),
        "the conversation went on recording after its child, on the same log"
    );

    // The parent points at it, which is what the monitor's left column reads.
    let agents = Replay::of(&rig.log).unwrap().agents();
    assert_eq!(agents.len(), 2);
    assert_eq!(agents[0].agent, AgentId::main());
    assert_eq!(agents[1].agent, *agent);
    assert_eq!(agents[1].parent, Some(AgentId::main()));
    assert_eq!(agents[1].call, Some(*call));
}

/// The child runs the whole loop: its own assembly, its own call, its own result
/// and its own completion, all durable on the log.
///
/// And the context it runs in is clean. Clean is not the same as hidden: the
/// conversation does not carry the child's messages into its next request, and
/// the record is all there.
#[tokio::test]
async fn the_childs_context_is_clean_and_its_transcript_is_durable() {
    let script = Script::serving("scripted")
        .then(delegates(
            "call-1",
            "Read notes.txt and say when the meeting is",
        ))
        .then(vec![Step::Call(call(
            "call-2",
            "read_file",
            json!({ "path": "notes.txt" }),
        ))])
        .then_say(&["Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    let (agent, _, _) = opened(&events)[0].clone();

    // The child's own half of the log, whole: it composed a turn, sent it,
    // called a tool, got a result and completed.
    let child = wrote(&events, &agent);
    assert!(child.iter().any(|event| matches!(
        &event.body,
        Body::Message { role: Role::User, text } if text.contains("Read notes.txt")
    )));
    assert!(child
        .iter()
        .any(|event| matches!(&event.body, Body::Assembly { .. })));
    assert!(child.iter().any(|event| matches!(
        &event.body,
        Body::Call { name, .. } if name == "read_file"
    )));
    assert!(child.iter().any(|event| matches!(
        &event.body,
        Body::Result { text, failed: false, .. } if text.contains("Thursday")
    )));
    assert!(child
        .iter()
        .any(|event| matches!(&event.body, Body::Completion { .. })));

    // Nothing the conversation said reached the child, and nothing the child
    // said reached the conversation.
    let requests = rig.script.requests();
    assert_eq!(
        user_messages(&requests[1]),
        ["Read notes.txt and say when the meeting is"],
        "a sub-agent starts in a separate clean context"
    );
    let last = requests.last().unwrap();
    assert!(
        !last
            .messages
            .iter()
            .any(|message| message.content.contains("Read notes.txt")),
        "the conversation does not carry its sub-agent's messages: {:?}",
        last.messages
    );

    // The transcript is the conversation's, and the record is everybody's.
    let transcript = chat.transcript().unwrap();
    assert!(
        !transcript.iter().any(|moment| matches!(
            moment,
            Moment::Called(called) if called.name == "read_file"
        )),
        "the child's call is not drawn in the conversation's transcript"
    );
    assert!(
        transcript.iter().any(|moment| matches!(
            moment,
            Moment::Called(called) if called.name == "delegate_task"
        )),
        "the delegation itself is"
    );
}

/// At the default parallelism the call blocks, and the answer is the tool's own
/// result: one record of one answer, and no `agent/returned` beside it.
#[tokio::test]
async fn the_call_blocks_and_the_answer_is_the_tools_own_result() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Read the notes"))
        .then_say(&["Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    let answer = chat
        .ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    assert_eq!(answer.text, "The meeting is Thursday.");
    let events = rig.events();
    let delegation = events
        .iter()
        .find(|event| matches!(&event.body, Body::Call { name, .. } if name == "delegate_task"))
        .expect("the delegation is a call like any other")
        .seq;
    assert_eq!(
        result(&events, delegation),
        Some(("Thursday.".to_owned(), false)),
        "what the sub-agent said is what the call returned"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.body, Body::Returned { .. })),
        "a delegation that blocked is not folded in a second time"
    );
}

/// The inheritance function is called on the way into every child at every
/// depth: a set the picker narrowed reaches the grandchild too, and a
/// sub-agent's own delegation is opened by the sub-agent's session.
#[tokio::test]
async fn what_the_parent_offers_is_the_ceiling_at_every_depth() {
    let set = ["read_file", "delegate_task"];
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        .then(delegates("call-2", "Read the notes"))
        .then_say(&["Thursday."])
        .then_say(&["The sub-agent said Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    rig.settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::TOOLS_OFFERED,
            &json!(set),
        )
        .unwrap();
    // Nothing below may add to that set, and the rule holds at both depths.
    assert!(set.contains(&"delegate_task"));
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    for (at, request) in rig.script.requests().iter().enumerate() {
        assert_eq!(
            names(request),
            set,
            "generation {at} was offered the conversation's set and nothing else"
        );
    }
    // Three agents, each one level below the last, and the depth is decremented
    // by the inheritance rule and by nothing else.
    let events = rig.events();
    let children = opened(&events);
    let depths: Vec<u32> = children.iter().map(|(_, _, depth)| *depth).collect();
    assert_eq!(depths, [1, 2], "a chain two deep, each link one lower");

    // The second delegation was opened **by the child**, not by the
    // conversation: a tool bound to a session is rebound to the child before the
    // child runs, so no sub-agent holds a handle on a conversation it is not in.
    let deeper = events
        .iter()
        .find(|event| matches!(&event.body, Body::Delegated { depth: 2, .. }))
        .expect("a second link");
    assert_eq!(
        deeper.agent, children[0].0,
        "the grandchild was opened by the child's own session"
    );
    assert_ne!(deeper.agent, AgentId::main());
}

/// A sub-agent is ruled on by the same matrix and the same person, call by call.
#[tokio::test]
async fn a_childs_calls_are_ruled_on_by_the_same_person() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Write the plan"))
        .then(vec![Step::Call(call(
            "call-2",
            "write_file",
            json!({ "path": "plan.txt", "content": "Thursday." }),
        ))])
        .then_say(&["Written."])
        .then_say(&["The plan is written."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let asked = Arc::new(Mutex::new(Vec::new()));
    chat.ask("Write the plan.", |_| {}, allowing(&asked))
        .await
        .unwrap();

    assert_eq!(
        asked.lock().unwrap().as_slice(),
        ["delegate_task", "write_file"],
        "the delegation is asked about, and so is what the sub-agent then does"
    );
    assert!(rig.project.path().join("plan.txt").exists());
}

/// A tool failure inside a child is a result the child answers with, and the
/// turn that asked for it carries on.
#[tokio::test]
async fn a_tool_failure_inside_a_child_is_a_result_it_answers_with() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Read the minutes"))
        .then(vec![Step::Call(call(
            "call-2",
            "read_file",
            json!({ "path": "minutes.txt" }),
        ))])
        .then_say(&["There are no minutes."])
        .then_say(&["The sub-agent found no minutes."]);
    let rig = Rig::new(script);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    let answer = chat
        .ask("What do the minutes say?", |_| {}, nobody())
        .await
        .expect("a failed call inside a child is not a failed turn");

    assert_eq!(answer.text, "The sub-agent found no minutes.");
    let events = rig.events();
    let (agent, _, _) = opened(&events)[0].clone();
    let failed = wrote(&events, &agent)
        .into_iter()
        .find(|event| matches!(event.body, Body::Result { failed: true, .. }))
        .expect("the child's own failed result");
    assert!(
        matches!(&failed.body, Body::Result { text, .. } if !text.is_empty()),
        "the child is told what went wrong"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.body, Body::Failure { .. })),
        "a tool failure is not a run ending"
    );
    let delegation = events
        .iter()
        .find(|event| matches!(&event.body, Body::Call { name, .. } if name == "delegate_task"))
        .unwrap()
        .seq;
    assert_eq!(
        result(&events, delegation),
        Some(("There are no minutes.".to_owned(), false)),
        "the child answered with what it had"
    );
}

/// A Stop on the parent leaves nothing generating, at depth 2.
///
/// The parent's token is the child's and the grandchild's, so a stop reaches
/// whichever of them is talking to the model. What it must not leave behind is a
/// sub-agent generating against a card nobody is waiting for.
#[tokio::test]
async fn a_stop_on_the_parent_leaves_nothing_generating_at_depth_two() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        .then(delegates("call-2", "Read the notes"))
        .then(vec![
            Step::Say("Still".into()),
            Step::Say(" going".into()),
            Step::Say(" and going".into()),
            Step::Say(" and going".into()),
        ])
        .then_say(&["Never reached."])
        .pausing(Duration::from_millis(100));
    let rig = Rig::new(script);
    let chat = Arc::new(rig.chat("autonomous"));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("When is the meeting?", |_| {}, nobody()).await })
    };
    // Long enough for the grandchild to be the one generating.
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        opened(&rig.events()).len(),
        2,
        "the stop lands while the chain is two deep"
    );
    assert!(chat.stop());

    let answer = tokio::time::timeout(Duration::from_secs(5), asking)
        .await
        .expect("a stop does not wait for the chain")
        .unwrap()
        .expect("a stop is not an error");
    assert_eq!(answer.reason, FinishReason::Cancelled);
    assert!(!chat.stop(), "nothing is left running");

    // The agent that was generating recorded the stop, on the same path a
    // finished answer takes, and nothing above it generated again.
    let events = rig.events();
    let agents = Replay::of(&rig.log).unwrap().agents();
    assert_eq!(agents.len(), 3, "the chain really was two deep");
    let deepest = &agents[2].agent;
    let stopped = wrote(&events, deepest)
        .into_iter()
        .rev()
        .find_map(|event| match event.body {
            Body::Completion { reason, .. } => Some(reason),
            _ => None,
        })
        .expect("the sub-agent at the bottom of the chain was generating");
    assert_eq!(
        stopped,
        FinishReason::Cancelled,
        "the stop reached the bottom of the chain"
    );

    // Every delegation in flight is answered as stopped rather than left with a
    // result, so the next message can carry the calls.
    let delegations: Vec<u64> = events
        .iter()
        .filter(|event| matches!(&event.body, Body::Call { name, .. } if name == "delegate_task"))
        .map(|event| event.seq)
        .collect();
    assert_eq!(delegations.len(), 2);
    for call in &delegations {
        assert_eq!(result(&events, *call), None, "call {call} ran to a result");
        assert!(
            events
                .iter()
                .any(|event| matches!(&event.body, Body::Refusal { call: at, .. } if at == call)),
            "call {call} was not answered at all"
        );
    }

    // Three generations, one per agent, and no fourth. The script has another
    // reply waiting and nothing ever asks for it.
    let sent = rig.script.requests().len();
    assert_eq!(sent, 3, "one generation per agent and no more");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        rig.script.requests().len(),
        sent,
        "nothing was sent after the turn ended"
    );
}

/// A chain cannot run past the depth it was given: the delegation at the limit
/// is a failed result the sub-agent answers from what it has.
///
/// The **absence** of the tool at the limit, with its own wording and its own
/// monitor row, is
/// [#64](https://github.com/elpideus/demido-studio/issues/64). What is owed here
/// is only that the chain ends.
#[tokio::test]
async fn the_chain_ends_at_the_depth_it_was_given() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        .then(delegates("call-2", "Ask somebody else again"))
        .then(delegates("call-3", "And again"))
        .then_say(&["I could not delegate further."])
        .then_say(&["The sub-agent could not delegate further."])
        .then_say(&["Done."]);
    let rig = Rig::new(script);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("Delegate as far as you can.", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    assert_eq!(
        opened(&events).len(),
        2,
        "two children, and no third: {:?}",
        opened(&events)
    );
    let refused = events
        .iter()
        .filter(|event| matches!(&event.body, Body::Call { name, .. } if name == "delegate_task"))
        .filter_map(|event| result(&events, event.seq))
        .filter(|(_, failed)| *failed)
        .count();
    assert_eq!(refused, 1, "the delegation at the limit is a failed result");
}
