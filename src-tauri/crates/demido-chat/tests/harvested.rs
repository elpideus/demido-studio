//! Harvesting at a step boundary: the asynchronous path, and the rule that a
//! delegation is never silently lost.
//!
//! [#66](https://github.com/elpideus/demido-studio/issues/66). Above the
//! default parallelism `delegate_task` does not block: the call is answered at
//! once, the sub-agent runs beside the turn that asked for it, and what it
//! said arrives at a step boundary as a framed message derived from
//! `agent/returned` rather than as a second result for a call that already has
//! one.
//!
//! **Everything here is asserted on the log, and nothing holds a clock still.**
//! Determinism comes from ordering: a background answer appears after every
//! call of its step has been answered and nowhere else, and two of them appear
//! in the order their delegations were asked for. There is no scheduler seam,
//! no pause anybody tunes, and no sleep. The one thing the backend had to grow
//! is `Script::when`, which addresses a reply to whoever was asked a particular
//! question, so a sub-agent generating beside its parent is answered by who it
//! is rather than by when it got there.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::future::{ready, Ready};
use std::sync::Arc;
use std::time::Duration;

use demido_chat::{Asking, Chat, Decision, Model, Pool, Toolbox};
use demido_inference::scripted::{Script, Scripted, Step};
use demido_inference::{FinishReason, Request, Role, Supervisor, ToolCall};
use demido_settings::{Memory as SettingsMemory, Scope, Settings};
use demido_tools::{delegation, files, Registry, Workspace};
use demido_trace::{Body, Event, Journal, Memory, Replay};
use serde_json::json;

const SESSION: &str = "harvested";

/// A card with room for four slots on a model whose slot is cheap.
///
/// The pool is a VRAM budget rather than a preference
/// ([#65](https://github.com/elpideus/demido-studio/issues/65)), so a test that
/// wants a second slot has to say what the card can hold. These are the
/// development model's numbers from `docs/rules/done.md`, on a card that has
/// room.
const FREE: u64 = 5583 * demido_vram::MIB;
const PER_SLOT: u64 = 563 * demido_vram::MIB;

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

    /// A chat whose backend opened `slots` generation slots, the conversation's
    /// own among them.
    ///
    /// Autonomous throughout, because what is under test is the ordering rather
    /// than the matrix, and a person asked about the first delegation of a turn
    /// is [`tests/delegated.rs`]'s.
    fn chat(&self, slots: u64) -> Chat<Scripted, Memory> {
        self.set(demido_settings::id::TOOLS_MODE, &json!("autonomous"));
        self.set(demido_settings::id::PARALLEL_AGENTS, &json!(slots));
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
        .against(Pool::on_a_card_with(FREE, PER_SLOT))
    }

    fn set(&self, id: &str, value: &serde_json::Value) {
        self.settings.set(&Scope::chat(SESSION), id, value).unwrap();
    }

    fn events(&self) -> Vec<Event> {
        self.log.events().unwrap()
    }

    /// One block of the log, rebuilt into the message it contributed.
    ///
    /// The frame is recorded as a hash and what filled it, like every host
    /// wording on this log, so reading it is the same rebuild a replay does.
    fn block(&self, seq: u64) -> demido_inference::Message {
        Replay::of(&self.log).unwrap().block(seq).unwrap()
    }

    /// The wording one paragraph has here, as the chat reads it.
    fn paragraph(&self, id: &str) -> String {
        demido_prompts::Paragraphs::open(self.prompts.path())
            .get(id)
            .expect("a paragraph this build ships")
            .fill(&[])
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

fn allowing() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |_: Asking| ready(Decision::Allow)
}

/// Every `tool/call` on the log, in order: its position and the tool it named.
fn calls(events: &[Event]) -> Vec<(u64, String)> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            Body::Call { name, .. } => Some((event.seq, name.clone())),
            _ => None,
        })
        .collect()
}

/// Where the one call to `delegate_task` is.
fn delegation_at(events: &[Event]) -> u64 {
    let found: Vec<u64> = calls(events)
        .into_iter()
        .filter(|(_, name)| name == "delegate_task")
        .map(|(seq, _)| seq)
        .collect();
    assert_eq!(found.len(), 1, "one delegation: {found:?}");
    found[0]
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

/// Every `agent/returned` on the log, in order: where it is, which call it
/// answers, and where the child's own answer is.
fn folded(events: &[Event]) -> Vec<(u64, u64, u64)> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            Body::Returned { call, answer, .. } => Some((event.seq, *call, *answer)),
            _ => None,
        })
        .collect()
}

/// Where every answer to a call is: results and refusals alike.
fn answers(events: &[Event]) -> Vec<u64> {
    events
        .iter()
        .filter(|event| matches!(event.body, Body::Result { .. } | Body::Refusal { .. }))
        .map(|event| event.seq)
        .collect()
}

/// Where every generation of the **conversation** ended, in order.
///
/// What tells a fold-in at a boundary in the middle of a turn from one at the
/// end of it: a `turn/completion` after an `agent/returned` means the turn went
/// on to talk to the model again, so the answer was folded into a step rather
/// than into the ending.
///
/// Scoped to the main agent, because the sub-agents are generating on this same
/// log at the same time and their completions are their own steps rather than
/// the conversation's. Counting theirs would make this read the concurrency it
/// is supposed to be blind to.
fn completions(events: &[Event]) -> Vec<u64> {
    events
        .iter()
        .filter(|event| {
            event.agent == demido_trace::AgentId::main()
                && matches!(event.body, Body::Completion { .. })
        })
        .map(|event| event.seq)
        .collect()
}

/// A generation long enough for a sub-agent started before it to finish inside
/// it, ending in the call named.
///
/// **Not a clock.** Nothing here waits for a duration or reads one: the
/// conversation simply has more to say than the sub-agent does, which is the
/// ordinary case and the one where a boundary in the middle of a turn exists at
/// all. The assertions are on the log either way, and a run where the child had
/// not finished would fold it in at a later boundary rather than fail.
fn talks_then_calls(id: &str, name: &str, arguments: serde_json::Value) -> Vec<Step> {
    let mut steps: Vec<Step> = (0..12)
        .map(|at| Step::Say(format!("thinking about it, {at}. ")))
        .collect();
    steps.push(Step::Call(call(id, name, arguments)));
    steps
}

fn user_messages(request: &Request) -> Vec<&str> {
    request
        .messages
        .iter()
        .filter(|message| message.role == Role::User)
        .map(|message| message.content.as_str())
        .collect()
}

/// A script whose conversation delegates once and then answers, and whose
/// sub-agent answers the task it was addressed with.
fn one_delegation() -> Script {
    Script::serving("scripted")
        .then(delegates("call-1", "Find when the meeting is"))
        .then_say(&["The meeting is Thursday."])
        .when_say("Find when the meeting is", &["Thursday."])
}

/// Above parallelism 1 the call is answered immediately, and what answers it is
/// the acknowledgement rather than the sub-agent.
#[tokio::test]
async fn above_the_default_the_call_is_answered_at_once() {
    let rig = Rig::new(one_delegation());
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    let delegation = delegation_at(&events);
    let (text, failed) = result(&events, delegation).expect("the call was answered");
    assert!(!failed);
    assert_eq!(
        text,
        rig.paragraph(demido_prompts::id::AGENT_DELEGATED),
        "the result says the work has gone out, not what came back"
    );
    assert!(
        !text.contains("Thursday"),
        "the sub-agent's answer is not this call's result: {text}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(&event.body, Body::Result { call, .. } if *call == delegation))
            .count(),
        1,
        "one call, one result"
    );
}

/// The answer comes back as a message derived from `agent/returned`, and the
/// child's own answer is named by position rather than copied.
#[tokio::test]
async fn the_answer_arrives_as_a_framed_message() {
    let rig = Rig::new(one_delegation());
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    let delegation = delegation_at(&events);
    let folded = folded(&events);
    assert_eq!(folded.len(), 1, "one delegation, folded in once");
    let (at, call, answer) = folded[0];
    assert_eq!(call, delegation, "it answers the call that asked");

    // The position it names is the child's own completion, on the child's half
    // of the log, and the text is there rather than in the parent's event.
    let child = events
        .iter()
        .find(|event| event.seq == answer)
        .expect("the answer is an event");
    assert!(
        matches!(&child.body, Body::Completion { text, .. } if text == "Thursday."),
        "{:?}",
        child.body
    );
    assert_ne!(child.agent, demido_trace::AgentId::main());

    // And the block the parent's model was shown is the frame, filled.
    let frame = events
        .iter()
        .find(|event| event.seq > at && matches!(event.body, Body::Fragment { .. }))
        .expect("a frame after the fold-in");
    let message = rig.block(frame.seq);
    assert_eq!(message.role, Role::User, "a message, not a tool result");
    assert!(message.answers.is_none(), "it answers no call");
    assert!(message.content.contains("Thursday."), "{}", message.content);
    assert!(
        message.content.contains("Find when the meeting is"),
        "the task is quoted back so two sub-agents can be told apart: {}",
        message.content
    );
}

/// A background answer appears after every call of its step is answered, and
/// nowhere else.
///
/// The turn is shaped so the fold-in lands at a boundary **in the middle** of
/// it rather than at its ending: the sub-agent is asked for in the first step,
/// finishes while the conversation is talking in the second, and is written
/// after the second step's own call has been answered. A run that only ever
/// folded in at the end would pass a weaker version of this without ever
/// reaching the boundary the ticket is about.
#[tokio::test]
async fn a_background_answer_waits_for_the_rest_of_its_step() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Find when the meeting is"))
        .then(talks_then_calls(
            "call-2",
            "read_file",
            json!({ "path": "notes.txt" }),
        ))
        .then_say(&["The meeting is Thursday."])
        .when_say("Find when the meeting is", &["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, allowing())
        .await
        .unwrap();

    let events = rig.events();
    let folded = folded(&events);
    assert_eq!(folded.len(), 1);
    let (at, _, _) = folded[0];

    let answers = answers(&events);
    assert_eq!(answers.len(), 2, "the delegation and the read: {answers:?}");
    assert!(
        answers.iter().all(|answer| *answer < at),
        "the fold-in is after every answer of its step: {answers:?} then {at}"
    );
    // The half the ordering above cannot see on its own: this was a boundary
    // the turn carried on from, not the end of the turn.
    let completions = completions(&events);
    assert!(
        completions.iter().any(|completion| *completion > at),
        "the turn took another step with the answer in it: {completions:?} around {at}"
    );
    assert_eq!(
        completions.iter().filter(|at_| **at_ < at).count(),
        2,
        "and it is the second step's boundary rather than the first: {completions:?}"
    );
}

/// A run whose model stops asking for tools waits for the delegation, folds it
/// in, and gives the model the step it needs to use it.
#[tokio::test]
async fn a_run_that_stopped_asking_for_tools_waits_and_folds_in() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Find when the meeting is"))
        // The conversation gives up on tools with the sub-agent still out.
        .then_say(&["I will answer once I hear back."])
        .then_say(&["The meeting is Thursday."])
        .when_say("Find when the meeting is", &["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    let answer = chat
        .ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    assert_eq!(folded(&events).len(), 1, "nothing was silently lost");
    assert_eq!(
        answer.text, "The meeting is Thursday.",
        "the turn carried on with what came back"
    );
    let last = rig.script.requests().pop().expect("a last request");
    assert!(
        user_messages(&last)
            .iter()
            .any(|said| said.contains("Thursday.")),
        "the frame reached the model: {:?}",
        user_messages(&last)
    );
}

/// A run that exhausts its step ceiling still waits for the delegation and
/// records its answer. The ceiling ends the turn; it does not eat an answer.
#[tokio::test]
async fn the_step_ceiling_does_not_eat_an_answer() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Find when the meeting is"))
        .then(vec![Step::Call(call(
            "call-2",
            "read_file",
            json!({ "path": "notes.txt" }),
        ))])
        .when_say("Find when the meeting is", &["Thursday."]);
    let rig = Rig::new(script);
    rig.set(demido_settings::id::STEP_LIMIT, &json!(1));
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    let error = chat
        .ask("When is the meeting?", |_| {}, allowing())
        .await
        .expect_err("the turn ran out of steps");

    assert!(
        matches!(error, demido_chat::Error::StepLimit { steps: 1 }),
        "{error:?}"
    );
    let events = rig.events();
    assert_eq!(
        folded(&events).len(),
        1,
        "the sub-agent was waited for and written down anyway"
    );
}

/// Two sub-agents in flight fold in at their own step boundaries, in the order
/// their delegations are on the log.
///
/// The first is asked for in step one and finishes while the conversation talks
/// in step two; the second is asked for at the end of step two and finishes
/// while it talks in step three. So the two fold-ins are separated by a
/// generation, which is what *their own* boundaries means and what a pair
/// harvested together at the ending would not show.
#[tokio::test]
async fn two_sub_agents_fold_in_at_their_own_boundaries() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Find when the meeting is"))
        .then(talks_then_calls(
            "call-2",
            "delegate_task",
            json!({ "task": "Find who is coming" }),
        ))
        .then(talks_then_calls(
            "call-3",
            "read_file",
            json!({ "path": "notes.txt" }),
        ))
        .then_say(&["Thursday, and everyone is coming."])
        .when_say("Find when the meeting is", &["Thursday."])
        .when_say("Find who is coming", &["Everyone."]);
    let rig = Rig::new(script);
    let chat = rig.chat(3);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting, and who?", |_| {}, allowing())
        .await
        .unwrap();

    let events = rig.events();
    let asked: Vec<u64> = calls(&events)
        .into_iter()
        .filter(|(_, name)| name == "delegate_task")
        .map(|(seq, _)| seq)
        .collect();
    assert_eq!(asked.len(), 2);

    let folded = folded(&events);
    assert_eq!(folded.len(), 2, "both were folded in, each exactly once");
    assert_eq!(
        folded.iter().map(|(_, call, _)| *call).collect::<Vec<_>>(),
        asked,
        "in the order they were asked for, whatever order they finished in"
    );

    // Two boundaries rather than one: a generation stands between them, so
    // neither was harvested at the other's step.
    let between: Vec<u64> = completions(&events)
        .into_iter()
        .filter(|completion| *completion > folded[0].0 && *completion < folded[1].0)
        .collect();
    assert_eq!(
        between.len(),
        1,
        "one generation between the two fold-ins: {between:?} between {} and {}",
        folded[0].0,
        folded[1].0
    );

    // And each is after the call that asked for it was answered.
    for (at, call, _) in &folded {
        let answered = answers(&events)
            .into_iter()
            .find(|answer| answer > call)
            .expect("the call was answered");
        assert!(answered < *at, "a fold-in before its own step was answered");
    }
}

/// A delegation that finds no free slot blocks, and its answer is its own
/// result. The pool is a size rather than a switch.
///
/// Two delegations in one step at one spare slot: the first takes it and is
/// answered with the acknowledgement, and the second has nowhere to go and so
/// takes the path the default takes. The model is never told there is no room.
#[tokio::test]
async fn a_full_pool_sends_the_next_delegation_down_the_blocking_path() {
    let script = Script::serving("scripted")
        .then(vec![
            Step::Call(call(
                "call-1",
                "delegate_task",
                json!({ "task": "Find when the meeting is" }),
            )),
            Step::Call(call(
                "call-2",
                "delegate_task",
                json!({ "task": "Find who is coming" }),
            )),
        ])
        .then_say(&["Thursday, and everyone is coming."])
        .when_say("Find when the meeting is", &["Thursday."])
        .when_say("Find who is coming", &["Everyone."]);
    let rig = Rig::new(script);
    // Two slots is one sub-agent beside the conversation, and this step asks
    // for two.
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting, and who?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    let asked: Vec<u64> = calls(&events)
        .into_iter()
        .filter(|(_, name)| name == "delegate_task")
        .map(|(seq, _)| seq)
        .collect();
    assert_eq!(asked.len(), 2);

    assert_eq!(
        result(&events, asked[0]),
        Some((rig.paragraph(demido_prompts::id::AGENT_DELEGATED), false)),
        "the first took the slot and did not wait"
    );
    assert_eq!(
        result(&events, asked[1]),
        Some(("Everyone.".to_owned(), false)),
        "the second found none free and blocked, so its answer is its result"
    );
    assert_eq!(
        folded(&events)
            .iter()
            .map(|(_, call, _)| *call)
            .collect::<Vec<_>>(),
        vec![asked[0]],
        "and only the one that went to the pool is folded in"
    );
}

/// A stop does not lose the answer either. Every ending goes through the same
/// wait, and a person who stopped the turn is the likeliest of all to wonder
/// what the sub-agent had got to.
///
/// **The one test here that touches the clock, and it touches it to press the
/// button rather than to decide an order.** Pressing Stop is a moment in time,
/// so there is a sleep before it; what is asserted afterwards is still only the
/// log.
#[tokio::test]
async fn a_stop_still_writes_down_what_the_sub_agent_had_got_to() {
    let script = Script::serving("scripted")
        .then(delegates("call-1", "Find when the meeting is"))
        .then(vec![
            Step::Say("Still".into()),
            Step::Say(" going".into()),
            Step::Say(" and going".into()),
            Step::Say(" and going".into()),
            Step::Say(" and going".into()),
            Step::Say(" and going".into()),
        ])
        .when_say("Find when the meeting is", &["Thursday", ", probably."])
        .pausing(Duration::from_millis(100));
    let rig = Rig::new(script);
    let chat = Arc::new(rig.chat(2));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("When is the meeting?", |_| {}, nobody()).await })
    };
    // Long enough for the delegation to be out and the conversation to be
    // talking, which is where a background sub-agent is actually running.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(chat.stop());

    let answer = tokio::time::timeout(Duration::from_secs(5), asking)
        .await
        .expect("a stop does not wait for work, only for endings")
        .unwrap()
        .expect("a stop is not an error");

    assert_eq!(answer.reason, FinishReason::Cancelled);
    let events = rig.events();
    assert_eq!(
        folded(&events).len(),
        1,
        "the sub-agent's ending was folded in rather than dropped"
    );
    assert!(!chat.stop(), "and nothing is left running");
}

/// The frame is part of what this conversation was told, so the next message
/// carries it. A turn that dropped it would be a delegation the rest of the
/// conversation cannot see.
#[tokio::test]
async fn the_next_message_still_carries_what_the_sub_agent_said() {
    let rig = Rig::new(one_delegation());
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();
    chat.ask("Are you sure?", |_| {}, nobody()).await.unwrap();

    let last = rig.script.requests().pop().expect("a last request");
    assert!(
        user_messages(&last)
            .iter()
            .any(|said| said.contains("Thursday.")),
        "{:?}",
        user_messages(&last)
    );
}

/// At the default there is no pool, so the call blocks and the answer is its
/// own result. Nothing is folded in, because nothing was deferred.
#[tokio::test]
async fn the_default_is_still_the_blocking_path() {
    let rig = Rig::new(one_delegation());
    let chat = rig.chat(1);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let events = rig.events();
    let delegation = delegation_at(&events);
    assert_eq!(
        result(&events, delegation),
        Some(("Thursday.".to_owned(), false)),
        "what the sub-agent said is what the call returned"
    );
    assert!(folded(&events).is_empty(), "and nothing was folded in");
}

/// The row the person reads carries the sub-agent's own answer, never the
/// acknowledgement that answered the call while the work was out.
///
/// [#67](https://github.com/elpideus/demido-studio/issues/67) over this
/// ticket's path. At the default the call's result *is* what the child said, so
/// a transcript could take it and be right by accident; here the result is the
/// `agent.delegated` paragraph and the answer arrives later as a message, and a
/// row that took the result would show a person a paragraph Demido wrote where
/// the sub-agent's answer belongs.
#[tokio::test]
async fn the_transcript_draws_the_answer_and_not_the_acknowledgement() {
    let rig = Rig::new(one_delegation());
    let chat = rig.chat(2);
    chat.load(|_| {}).await;

    chat.ask("When is the meeting?", |_| {}, nobody())
        .await
        .unwrap();

    let transcript = chat.transcript().unwrap();
    let delegation = transcript
        .iter()
        .find_map(|moment| match moment {
            demido_chat::Moment::Delegated(delegation) => Some(delegation),
            _ => None,
        })
        .expect("a delegation is one exchange on this path too");
    assert_eq!(delegation.task, "Find when the meeting is");
    let answer = delegation.answer.as_ref().expect("it came back");
    assert_eq!(answer.text, "Thursday.", "what the sub-agent said");
    assert_ne!(
        answer.text,
        rig.paragraph(demido_prompts::id::AGENT_DELEGATED),
        "and not the paragraph that stood in for it while the work was out"
    );

    let events = rig.events();
    let (_, call, answered) = folded(&events)[0];
    assert_eq!(call, delegation.seq, "the row is the call that asked");
    assert_eq!(
        answer.seq, answered,
        "named by position, which is the same position `agent/returned` names"
    );
    assert!(
        !transcript
            .iter()
            .any(|moment| matches!(moment, demido_chat::Moment::Called(_))),
        "and the delegation is not also drawn as an ordinary call row"
    );
}
