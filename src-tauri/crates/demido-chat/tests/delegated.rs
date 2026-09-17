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

use demido_chat::{Asking, Chat, Decision, Model, Moment, Standing, Toolbox};
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

    /// The same chat, recording into `log` rather than straight into the rig's.
    ///
    /// One journal underneath either way: `Refusing` is the rig's own log with
    /// a sub-agent's writes turned away, so what a refused run did manage to
    /// record is still readable through `events`.
    fn chat_over<J: Journal + Clone + 'static>(&self, mode: &str, log: J) -> Chat<Scripted, J> {
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

    /// How deep this conversation may delegate, on its own tier of the ladder.
    ///
    /// A setter rather than a value handed to [`Rig::chat`], because the whole
    /// of #64 is that the number is read where a delegation is dispatched: a
    /// test that could only set it before a chat existed could not tell that
    /// apart from a number baked in at construction.
    fn set_depth(&self, depth: u64) {
        self.settings
            .set(
                &Scope::chat(SESSION),
                demido_settings::id::DELEGATION_DEPTH,
                &json!(depth),
            )
            .unwrap();
    }

    /// The wording one paragraph has here, as the chat reads it.
    fn paragraph(&self, id: &str) -> demido_prompts::Prompt {
        demido_prompts::Paragraphs::open(self.prompts.path())
            .get(id)
            .expect("a paragraph this build ships")
    }
}

/// A log that takes the conversation's events and refuses its sub-agents'.
///
/// A full disk arriving exactly when a child starts writing. Written here rather
/// than beside `Memory` because it is a fault this one test stages, and a
/// journal that fails on purpose is not a second implementation anybody would
/// wire (`docs/rules/tiles.md`).
#[derive(Clone)]
struct Refusing(Memory);

impl Journal for Refusing {
    fn append(&self, entry: demido_trace::Entry) -> demido_trace::Result<Event> {
        if entry.agent != AgentId::main() {
            return Err(demido_trace::Error::io(
                "writing a sub-agent's event",
                std::io::Error::other("there is no space left on the device"),
            ));
        }
        self.0.append(entry)
    }

    fn events(&self) -> demido_trace::Result<Vec<Event>> {
        self.0.events()
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

/// What one group of the registry came to in one assembly.
fn standing(assembly: &demido_chat::Assembly, group: &str) -> Standing {
    assembly
        .groups
        .iter()
        .find(|grouped| grouped.group == group)
        .unwrap_or_else(|| panic!("no {group} group: {:?}", assembly.groups))
        .standing
}

/// The depth of each agent a delegation opened, in the order they were opened.
fn depths(events: &[Event]) -> Vec<u32> {
    opened(events).iter().map(|(_, _, depth)| *depth).collect()
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

/// The one delegation the conversation's transcript draws, which is what every
/// scenario here asks about and is exactly one in all of them.
///
/// A helper rather than the same `find_map` six times, and it asserts the count
/// on the way past: a second delegation nobody expected would otherwise be
/// read as the first (`a_tool.rs`'s `only_call` is the same idea one row over).
fn only_delegation(transcript: &[Moment]) -> &demido_chat::Delegation {
    let drawn: Vec<&demido_chat::Delegation> = transcript
        .iter()
        .filter_map(|moment| match moment {
            Moment::Delegated(delegation) => Some(delegation),
            Moment::Said(_) | Moment::Called(_) => None,
        })
        .collect();
    assert_eq!(
        drawn.len(),
        1,
        "one delegation is drawn, once: {transcript:?}"
    );
    drawn[0]
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
        transcript
            .iter()
            .any(|moment| matches!(moment, Moment::Delegated(_))),
        "the delegation itself is, as the one exchange it was (#67)"
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

    // The conversation and its child are offered the whole set. The grandchild
    // is at the default depth's limit, so it is offered the same set less the
    // one tool it has no depth left for, which is #64's rule rather than a
    // second narrowing (`demido_permission::inherit`).
    //
    // The order is the chain's: the conversation asks, its child asks, the
    // grandchild answers, and then each of them takes its second step on the
    // way back up. Only the third generation is the grandchild's.
    let requests = rig.script.requests();
    assert_eq!(requests.len(), 5, "one generation per step of the chain");
    for (at, request) in requests.iter().enumerate() {
        let expected: &[&str] = match at {
            2 => &["read_file"],
            _ => &set,
        };
        assert_eq!(
            names(request),
            expected,
            "generation {at} was not offered what its agent's depth allows"
        );
    }
    // Three agents, each one level below the last, and the level is counted up
    // by the inheritance rule and by nothing else.
    let events = rig.events();
    let children = opened(&events);
    assert_eq!(
        depths(&events),
        [1, 2],
        "a chain two deep, each link one lower"
    );

    // The second delegation was opened **by the child**, not by the
    // conversation. This is the shape of "no sub-agent holds a handle on a
    // conversation it is not in": the tool holds no session at all, and what
    // answers it is whichever loop is running, which here is the child's.
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

/// Only the log failing stops a run.
///
/// The other half of the rule above it, and the one thing that is not a result:
/// a child whose events cannot be written is a child nothing can say happened,
/// so the turn that asked for it ends rather than carrying on over a record
/// that is not there.
#[tokio::test]
async fn a_log_that_will_not_take_the_childs_events_stops_the_run() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Read the notes"))
        .then_say(&["Thursday."])
        .then_say(&["Never reached."]);
    let rig = Rig::new(script);
    let log = Refusing(rig.log.clone());
    let chat = rig.chat_over("autonomous", log);
    chat.load(|_| {}).await;

    let error = chat
        .ask("When is the meeting?", |_| {}, nobody())
        .await
        .expect_err("a log that will not take a child's events ends the turn");

    assert!(
        matches!(error, demido_chat::Error::Journal(_)),
        "the failure is the log's, and it is not dressed up as a tool result: {error:?}"
    );
    let events = rig.events();
    assert_eq!(
        opened(&events).len(),
        1,
        "the child was opened, and then could not write"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.body, Body::Result { .. })),
        "nothing was answered as if the sub-agent had run"
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

/// At depth 1, `delegate_task` is absent from every child's payload.
///
/// v2 reached this by cloning a sub-agent's registry before `delegate_task` was
/// added to it, so the tool was never on offer at any setting. The same
/// behaviour, now the default value of a number being read
/// ([#64](https://github.com/elpideus/demido-studio/issues/64)): nothing about
/// the construction says it, and changing the setting changes it.
#[tokio::test]
async fn at_depth_one_the_tool_is_absent_from_every_childs_payload() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        .then_say(&["I did it myself."])
        .then_say(&["The sub-agent did it."]);
    let rig = Rig::new(script);
    rig.set_depth(1);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("Delegate as far as you can.", |_| {}, nobody())
        .await
        .unwrap();

    // The conversation was offered it, because the conversation is the one
    // agent with depth above it. Its child was not, which is the whole of the
    // rule at this setting.
    let requests = rig.script.requests();
    let [conversation, child, back_up] = requests.as_slice() else {
        panic!(
            "one delegation and the step after it: {} sent",
            requests.len()
        );
    };
    assert!(names(conversation).contains(&"delegate_task"));
    assert!(names(back_up).contains(&"delegate_task"));
    assert!(
        !names(child).contains(&"delegate_task"),
        "the child was shown a tool it has no depth for: {:?}",
        names(child)
    );
    let events = rig.events();
    assert_eq!(opened(&events).len(), 1, "one link, and no second");

    // And the monitor says which absence it is. A reader who finds no
    // Delegation group in a sub-agent's assembly must not be told the registry
    // dropped it or that they switched it off, because the control in the way
    // is the depth and neither of those would send them to it
    // (`docs/rules/tools.md`).
    let (child, _, _) = opened(&events)[0].clone();
    let theirs = wrote(&events, &child);
    let at = theirs[theirs.len() - 1].seq;
    let child = chat.assembly(at).unwrap().expect("the child's assembly");
    assert_eq!(standing(&child, "delegation"), Standing::PastTheDepth);

    let mine = wrote(&events, &AgentId::main());
    let ours = chat
        .assembly(mine[mine.len() - 1].seq)
        .unwrap()
        .expect("the conversation's assembly");
    assert_eq!(
        standing(&ours, "delegation"),
        Standing::Offered,
        "the conversation is above the limit and was offered the tool"
    );
}

/// At depth 3, the brief's own chain of three runs.
///
/// Brief B19: "depth 3 would mean Main chat/context delegates an agent we will
/// call Agent 1. Agent 1 needs another info so it delegates Agent 2. Agent 2
/// needs something else so it delegates Agent 3."
#[tokio::test]
async fn at_depth_three_a_three_link_chain_runs() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Agent 1, find out"))
        .then(delegates("call-2", "Agent 2, find out"))
        .then(delegates("call-3", "Agent 3, find out"))
        .then_say(&["Thursday."])
        .then_say(&["Agent 3 said Thursday."])
        .then_say(&["Agent 2 said Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    rig.set_depth(3);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    let answer = chat
        .ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    assert_eq!(answer.text, "The meeting is Thursday.");
    let events = rig.events();
    assert_eq!(depths(&events), [1, 2, 3], "three links, each one lower");

    // Agent 3 is the last link and knows it by what it was shown rather than by
    // a refusal it read: the fourth generation is the one with nothing to
    // delegate with.
    let requests = rig.script.requests();
    assert!(
        !names(&requests[3]).contains(&"delegate_task"),
        "agent 3 was shown a fourth link: {:?}",
        names(&requests[3])
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.body, Body::Refusal { .. })),
        "a chain that fits inside the depth refuses nothing"
    );
}

/// The depth is read where a delegation is dispatched, so a number changed
/// while a turn is running rules the next delegation of that same turn.
///
/// A ladder read at the top of a turn and carried down could not do this: the
/// child's set would be a reading taken before the change. The change is made
/// from the approval callback, which is the one place a test can stand between
/// a delegation being allowed and the child it opens being built.
#[tokio::test]
async fn the_depth_is_read_at_dispatch_and_not_carried_down() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        .then(delegates("call-2", "Ask somebody else again"))
        .then_say(&["Thursday."])
        .then_say(&["The sub-agent said Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    // One link is all this turn may open when it starts.
    rig.set_depth(1);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    // Cautious, so `delegate_task` asks, and the answer is where the person
    // changes the setting: after the first delegation was allowed, and before
    // the child it opens is built.
    let raised = Arc::new(Mutex::new(false));
    let approve = {
        let raised = raised.clone();
        let settings = rig.settings.clone();
        move |_: Asking| {
            let mut raised = raised.lock().unwrap();
            if !*raised {
                settings
                    .set(
                        &Scope::chat(SESSION),
                        demido_settings::id::DELEGATION_DEPTH,
                        &json!(2),
                    )
                    .unwrap();
                *raised = true;
            }
            ready(Decision::Allow)
        }
    };

    chat.ask("When is the meeting?", |_| {}, approve)
        .await
        .unwrap();

    assert!(*raised.lock().unwrap(), "the delegation was asked about");
    let events = rig.events();
    assert_eq!(
        depths(&events),
        [1, 2],
        "the depth raised mid turn did not reach the next delegation"
    );
    assert!(
        names(&rig.script.requests()[1]).contains(&"delegate_task"),
        "the child was built without the tool the raised depth gave it"
    );
}

/// A child at the limit that names the tool anyway is told why, in its own
/// context, and answers from what it has.
///
/// The words are not the picker's, because the reason is not the picker's: the
/// user did not turn this off, the chain reached its limit. And the refusal is
/// an event carrying its stated reason, which is what draws the monitor row.
#[tokio::test]
async fn a_child_at_the_limit_that_names_the_tool_is_told_why_in_its_own_context() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        // The child has no `delegate_task` and names it regardless. A small
        // model that has seen the tool once does exactly this.
        .then(delegates("call-2", "Ask somebody else again"))
        .then_say(&["Nobody else to ask, so: Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    rig.set_depth(1);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    let answer = chat
        .ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    // It answered rather than stalling, and the conversation got the answer.
    assert_eq!(answer.text, "The meeting is Thursday.");
    let events = rig.events();
    let children = opened(&events);
    let [(child, _, _)] = children.as_slice() else {
        panic!("the call at the limit opened something: {children:?}");
    };

    // In its own context: the refusal is on the child's half of the log rather
    // than the conversation's, so what reads it is the child's next request.
    let refusals: Vec<Event> = wrote(&events, child)
        .into_iter()
        .filter(|event| matches!(event.body, Body::Refusal { .. }))
        .collect();
    let [refusal] = refusals.as_slice() else {
        panic!("one refusal, on the child's own half of the log: {refusals:?}");
    };

    // Carrying its stated reason, by the hash of the paragraph that stated it,
    // the way every refusal does: that is the row the monitor draws.
    let depth = rig.paragraph(demido_prompts::id::TOOLS_DEPTH);
    let off = rig.paragraph(demido_prompts::id::TOOLS_OFF);
    let Body::Refusal { hash, values, .. } = &refusal.body else {
        unreachable!("filtered above")
    };
    assert_eq!(hash, &depth.hash, "the refusal names its own wording");
    assert_ne!(
        depth.hash, off.hash,
        "the wording at the limit is the picker's switched-off wording"
    );
    assert!(
        values
            .iter()
            .any(|filling| filling.value == "delegate_task"),
        "the refusal names the call that did not run: {values:?}"
    );

    // And it really is a different paragraph rather than the same sentence
    // under a second id: the picker's says the user turned it off, and at the
    // limit nobody did.
    assert_ne!(depth.text, off.text);
}

/// A delegation reads as one exchange in the conversation's transcript: the
/// task going out, and the sub-agent's answer coming back.
///
/// [#67](https://github.com/elpideus/demido-studio/issues/67). The child read a
/// file and answered from it; what the parent's chat shows is the task and the
/// answer, not the file read. A clean context should also be a clean
/// transcript.
#[tokio::test]
async fn a_delegation_is_one_exchange_in_the_transcript() {
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

    let transcript = chat.transcript().unwrap();
    let delegation = only_delegation(&transcript);
    assert_eq!(
        delegation.task, "Read notes.txt and say when the meeting is",
        "the task as it went out"
    );
    let answer = delegation.answer.as_ref().expect("and the answer back");
    assert_eq!(answer.text, "Thursday.");
    assert!(!answer.failed);
    assert_eq!(
        delegation.agent,
        opened(&rig.events())[0].0,
        "the row names the sub-agent that answered, as the monitor's column does"
    );

    // One exchange, and nothing of the machinery under it: no call row for the
    // delegation itself, and none of the child's own calls.
    assert_eq!(
        transcript
            .iter()
            .filter(|moment| matches!(moment, Moment::Called(_)))
            .count(),
        0,
        "the conversation called nothing itself: {transcript:?}"
    );
}

/// A sub-agent whose own turn ended badly is drawn as a failed answer rather
/// than as an answer, on the row the person is already reading.
#[tokio::test]
async fn a_sub_agent_that_ended_badly_reads_as_a_failure_on_the_row() {
    // A child that never stops asking for tools, so its run ends at the step
    // limit: the one ending that is a `turn/failure` rather than a completion.
    let mut script = Script::serving("scripted").then(delegates("call-1", "Read everything"));
    for index in 0..12 {
        script = script.when(
            "Read everything",
            vec![Step::Call(call(
                &format!("child-{index}"),
                "read_file",
                json!({ "path": "notes.txt" }),
            ))],
        );
    }
    let rig = Rig::new(script.then_say(&["The sub-agent gave up."]));
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("Read everything.", |_| {}, nobody())
        .await
        .unwrap();

    let transcript = chat.transcript().unwrap();
    let answer = only_delegation(&transcript)
        .answer
        .as_ref()
        .expect("a delegation that failed is still a delegation, and it ended");
    assert!(
        answer.failed,
        "the sub-agent's run ended in a failure, and the row says so: {answer:?}"
    );
    assert!(!answer.text.is_empty(), "and what it was");
}

/// A denied delegation is handed to the model as information, and it does the
/// work itself rather than stalling or asking for the same sub-agent again.
#[tokio::test]
async fn a_denied_delegation_is_reported_and_the_model_does_something_else() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Read the notes"))
        .then(vec![Step::Call(call(
            "call-2",
            "read_file",
            json!({ "path": "notes.txt" }),
        ))])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    let answer = chat
        .ask(
            "When is the meeting?",
            |_| {},
            move |asking: Asking| {
                seen.lock().unwrap().push(asking.tool.clone());
                ready(if asking.tool == "delegate_task" {
                    Decision::Deny
                } else {
                    Decision::Allow
                })
            },
        )
        .await
        .unwrap();

    let events = rig.events();
    assert!(
        opened(&events).is_empty(),
        "a delegation nobody allowed opens no sub-agent"
    );
    let refused = events
        .iter()
        .find_map(|event| match &event.body {
            Body::Refusal { call, .. } => Some(*call),
            _ => None,
        })
        .expect("the model is told, in a paragraph's wording");
    let outcome = chat
        .transcript()
        .unwrap()
        .into_iter()
        .find_map(|moment| match moment {
            Moment::Called(called) if called.seq == refused => called.outcome,
            _ => None,
        })
        .expect("and the row it is drawn on is an ordinary call row");
    assert!(
        matches!(outcome, demido_chat::Outcome::Refused { .. }),
        "declined, rather than a failure: {outcome:?}"
    );
    assert_eq!(
        asked.lock().unwrap().as_slice(),
        ["delegate_task"],
        "the one call the matrix asked about is the one the person answered"
    );
    assert!(
        events.iter().any(|event| matches!(
            &event.body,
            Body::Call { name, .. } if name == "read_file"
        )),
        "and the model then did the work itself"
    );
    assert_eq!(
        events
            .iter()
            .filter(
                |event| matches!(&event.body, Body::Call { name, .. } if name == "delegate_task")
            )
            .count(),
        1,
        "rather than asking for the same sub-agent again"
    );
    assert_eq!(answer.text, "The meeting is Thursday.");

    // The information itself: the declined paragraph, filled, in the request
    // the next step was sent with. Handing a denial to the model is the whole
    // criterion, and a run where the loop carried on without telling it would
    // look identical from the log's tool rows alone.
    let told = rig
        .paragraph(demido_prompts::id::TOOLS_DENIED)
        .fill(&[("tool", "delegate_task")]);
    let next = &rig.script.requests()[1];
    assert!(
        next.messages
            .iter()
            .any(|message| message.content.contains(told.trim())),
        "the model is handed the refusal it is meant to act on: {:?}",
        next.messages
    );
}

/// The floor holds in the mode that never asks about the delegation at all.
///
/// Autonomous is the case the criterion is really about: the delegation itself
/// runs unasked, so the only prompt in the whole turn is the destructive call
/// the sub-agent made, and a floor that lived in the parent's answer rather
/// than under every call would have nothing to hold here.
#[tokio::test]
async fn a_destructive_call_inside_a_child_asks_even_in_autonomous() {
    let script = Script::serving("scripted")
        .when("Tidy up.", delegates("call-1", "Delete the draft"))
        .when(
            "Delete the draft",
            vec![Step::Call(call(
                "call-2",
                "delete_file",
                json!({ "path": "draft.txt" }),
            ))],
        )
        .when_say("Delete the draft", &["Deleted."])
        .when_say("Tidy up.", &["The draft is gone."]);
    let rig = Rig::new(script);
    std::fs::write(
        rig.project.path().join("draft.txt"),
        "a draft
",
    )
    .unwrap();
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    let asked = Arc::new(Mutex::new(Vec::new()));
    chat.ask("Tidy up.", |_| {}, allowing(&asked))
        .await
        .unwrap();

    assert_eq!(
        asked.lock().unwrap().as_slice(),
        ["delete_file"],
        "autonomous asks about nothing else, and about this one every time"
    );
}

/// *Always for this tool* on the delegation does not reach a destructive call
/// inside the child.
///
/// The floor under every mode survives a level of indirection: a person who
/// said *always* to delegating did not thereby consent to whatever a sub-agent
/// deletes, and `docs/rules/tools.md` puts that floor in the loop rather than
/// in the window for exactly this reason.
#[tokio::test]
async fn always_on_a_delegation_leaves_a_destructive_call_inside_the_child_asking() {
    let script = Script::serving("scripted")
        .when("Tidy up.", delegates("call-1", "Delete the draft"))
        .when(
            "Delete the draft",
            vec![Step::Call(call(
                "call-2",
                "delete_file",
                json!({ "path": "draft.txt" }),
            ))],
        )
        .when_say("Delete the draft", &["Deleted."])
        .when_say("Tidy up.", &["The draft is gone."]);
    let rig = Rig::new(script);
    std::fs::write(rig.project.path().join("draft.txt"), "a draft\n").unwrap();
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let asked = Arc::new(Mutex::new(Vec::new()));
    let seen = asked.clone();
    chat.ask(
        "Tidy up.",
        |_| {},
        move |asking: Asking| {
            seen.lock().unwrap().push(asking.tool.clone());
            ready(if asking.tool == "delegate_task" {
                Decision::Always
            } else {
                Decision::Allow
            })
        },
    )
    .await
    .unwrap();

    assert_eq!(
        asked.lock().unwrap().as_slice(),
        ["delegate_task", "delete_file"],
        "the grant was about delegating, and the sub-agent's delete still asks"
    );
    assert!(
        !rig.project.path().join("draft.txt").exists(),
        "the person allowed it, so it ran"
    );
}

/// Nothing Demido puts in front of any agent in the chain describes the agent
/// mode or the delegation depth.
///
/// `docs/rules/tools.md`: the mode is a rule the loop keeps rather than a
/// paragraph the model is asked to respect, and the depth is a number read at
/// dispatch. A model told about either would negotiate with it, and the one at
/// the limit is told what is missing rather than how deep it is.
#[tokio::test]
async fn nothing_any_agent_is_sent_names_the_mode_or_the_depth() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Ask somebody else"))
        .then(delegates("call-2", "Read the notes"))
        .then_say(&["Thursday."])
        .then_say(&["The sub-agent said Thursday."])
        .then_say(&["The meeting is Thursday."]);
    let rig = Rig::new(script);
    // One level, so the grandchild's own delegation is refused at the limit and
    // the wording of that refusal is in the sweep below.
    rig.set_depth(1);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let asked = Arc::new(Mutex::new(Vec::new()));
    chat.ask("When is the meeting?", |_| {}, allowing(&asked))
        .await
        .unwrap();

    // Everything every agent was told **except** what answers one of its own
    // calls. A refusal is the other half of the ticket and is meant to say what
    // happened: a sub-agent at the limit is told the chain reached it, so that
    // it does the work itself rather than stalling. What is under test here is
    // the standing prose, which is where a rule the loop keeps would leak into
    // something the model could argue with.
    for request in rig.script.requests() {
        let prose = request
            .messages
            .iter()
            .filter(|message| message.answers.is_none())
            .map(|message| message.content.clone())
            // The tool documents as well as the messages: a tool's description
            // and its parameter prose are host prompt text the model reads
            // (`docs/rules/prompts.md`), and the depth is exactly the thing that
            // would be explained there if it were explained anywhere.
            .chain(
                request
                    .tools
                    .iter()
                    .map(|tool| format!("{} {}", tool.description, tool.parameters)),
            );
        for said in prose {
            let said = said.to_lowercase();
            for word in ["cautious", "balanced", "autonomous", "depth"] {
                assert!(
                    !said.contains(word),
                    "what the model may do it learns from what it is offered, never from prose: {said:?}"
                );
            }
        }
    }
}

/// The conversation's own context never carries its sub-agent's calls or their
/// results, and the next message does not either.
///
/// Asserted from the log, which is the criterion's own wording: the blocks the
/// conversation carries forward, and the requests the backend was handed.
#[tokio::test]
async fn the_childs_calls_are_in_no_request_the_conversation_sent() {
    let script = Script::serving("scripted")
        .when(
            "When is the meeting?",
            delegates("call-1", "Read notes.txt"),
        )
        .when(
            "Read notes.txt",
            vec![Step::Call(call(
                "call-2",
                "read_file",
                json!({ "path": "notes.txt" }),
            ))],
        )
        .when_say("Read notes.txt", &["Thursday."])
        .when_say("When is the meeting?", &["The meeting is Thursday."])
        .when_say("Thanks.", &["You are welcome."]);
    let rig = Rig::new(script);
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();
    // A second message, so what the conversation carries **forward** is
    // asserted and not only what it sent while the child was running.
    chat.ask("Thanks.", |_| {}, nobody()).await.unwrap();

    let events = rig.events();
    let (agent, _, _) = opened(&events)[0].clone();
    let childs: Vec<u64> = wrote(&events, &agent)
        .iter()
        .filter(|event| matches!(event.body, Body::Call { .. } | Body::Result { .. }))
        .map(|event| event.seq)
        .collect();
    assert!(!childs.is_empty(), "the child did call something");

    let carried = Replay::of(&rig.log).unwrap().conversation();
    for seq in &childs {
        assert!(
            !carried.contains(seq),
            "the conversation carries none of its sub-agent's calls: {seq}"
        );
    }

    // And at the seam: every request the conversation itself sent, which is
    // every request that carries the question the person typed.
    let conversations: Vec<Request> = rig
        .script
        .requests()
        .into_iter()
        .filter(|request| user_messages(request).contains(&"When is the meeting?"))
        .collect();
    assert!(
        conversations.len() >= 3,
        "two turns, one of them two steps: {}",
        conversations.len()
    );
    for request in conversations {
        assert!(
            !request
                .messages
                .iter()
                .any(|message| message.content.contains("The meeting moved to Thursday.")),
            "what the child read is in nobody's context but the child's: {:?}",
            request.messages
        );
    }
}
