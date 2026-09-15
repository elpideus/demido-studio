//! The turn loop with tools in it, against a scripted backend.
//!
//! A canned call, then a canned answer, and everything between the two is the
//! loop's: dispatch, the matrix, the person asked, the call run, the result in
//! the next request, the step limit, and what a stop leaves behind. The script
//! is `demido_inference::scripted`, which passes the same contract suite
//! `llama.cpp` does, so none of this needs a card.
//!
//! What a real model *chooses* to do with a denial or a planted file is the live
//! suite's, on [#59](https://github.com/elpideus/demido-studio/issues/59).

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::collections::VecDeque;
use std::future::{ready, Ready};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use demido_chat::{Asking, Called, Chat, Decision, Model, Moment, Outcome, Toolbox};
use demido_inference::scripted::{Script, Scripted, Step};
use demido_inference::{FinishReason, Role, Supervisor, ToolCall};
use demido_settings::{Memory as SettingsMemory, Scope, Settings};
use demido_tools::{files, shell, Ability, Registry, Workspace};
use demido_trace::{Body, Event, Journal, Memory, Replay, Source};
use serde_json::json;

const SESSION: &str = "a-tool";

/// A project with a file in it, a prompts directory nobody has edited, a log,
/// the script, and the ladder.
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

    /// A chat over the Files and Shell groups, in the mode named on its own
    /// tier of the ladder.
    fn chat(&self, mode: &str) -> Chat<Scripted, Memory> {
        self.settings
            .set(
                &Scope::chat(SESSION),
                demido_settings::id::TOOLS_MODE,
                &json!(mode),
            )
            .unwrap();
        let workspace = Workspace::open(self.project.path()).unwrap();
        let registry = Registry::open(Some(workspace))
            .with_group(files())
            .with_group(shell());
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
        )
    }

    fn path(&self, name: &str) -> std::path::PathBuf {
        self.project.path().join(name)
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

/// Somebody at the window, answering in the order given, and remembering what
/// they were shown.
#[derive(Clone, Default)]
struct Person {
    shown: Arc<Mutex<Vec<Asking>>>,
    answers: Arc<Mutex<VecDeque<Decision>>>,
}

impl Person {
    fn answering(decisions: &[Decision]) -> Self {
        Self {
            shown: Arc::default(),
            answers: Arc::new(Mutex::new(decisions.iter().copied().collect())),
        }
    }

    fn approve(&self) -> impl FnMut(Asking) -> Ready<Decision> + Send {
        let person = self.clone();
        move |asking| {
            person.shown.lock().unwrap().push(asking);
            let answer = person.answers.lock().unwrap().pop_front();
            ready(answer.unwrap_or_else(|| panic!("asked more often than the test expected")))
        }
    }

    fn asked(&self) -> Vec<Asking> {
        self.shown.lock().unwrap().clone()
    }
}

/// Nobody is at the window, and nobody should need to be.
fn nobody() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |asking: Asking| panic!("nobody should have been asked about {}", asking.tool)
}

fn count(events: &[Event], kind: fn(&Body) -> bool) -> usize {
    events.iter().filter(|event| kind(&event.body)).count()
}

fn results(body: &Body) -> bool {
    matches!(body, Body::Result { .. })
}

fn refusals(body: &Body) -> bool {
    matches!(body, Body::Refusal { .. })
}

/// The product's sentence for this slice: the model asks for a file, the file
/// is read, and the answer comes from what was in it.
#[tokio::test]
async fn a_call_is_dispatched_run_and_its_result_is_in_the_next_request() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"path": "notes.txt"}"#)
        .then_say(&["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    // Cautious, and reading is still Allow: nobody is asked.
    let answer = chat
        .ask("When is the meeting?", |_| {}, nobody())
        .await
        .expect("an answer");
    assert_eq!(answer.text, "Thursday.");
    assert_eq!(answer.reason, FinishReason::Stop);

    let sent = rig.script.requests();
    assert_eq!(sent.len(), 2, "one step to call, one to answer");
    let offered: Vec<&str> = sent[0]
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect();
    assert!(
        offered.contains(&"read_file") && offered.contains(&"run_command"),
        "the Files and Shell groups are in the payload: {offered:?}"
    );

    let second = &sent[1].messages;
    let asked = &second[second.len() - 2];
    assert_eq!(asked.role, Role::Assistant);
    assert_eq!(
        asked.calls,
        vec![ToolCall {
            id: "call-1".into(),
            name: "read_file".into(),
            arguments: r#"{"path": "notes.txt"}"#.into(),
        }]
    );
    let result = &second[second.len() - 1];
    assert_eq!(result.role, Role::Tool);
    assert_eq!(result.answers.as_deref(), Some("call-1"));
    assert!(
        result.content.contains("The meeting moved to Thursday."),
        "the file's content is what the model reads next: {}",
        result.content
    );

    let transcript = chat.transcript().unwrap();
    let said: Vec<String> = transcript
        .iter()
        .filter_map(|moment| match moment {
            Moment::Said(said) => Some(said.text.clone()),
            Moment::Called(_) => None,
        })
        .collect();
    assert_eq!(
        said,
        vec!["When is the meeting?", "Thursday."],
        "an answer that only called is not a bubble of its own"
    );

    // #55: the call and its result are in the transcript, at the point in the
    // turn where they happened, which here is between the question and the
    // answer that came out of them.
    assert!(
        matches!(
            &transcript[1],
            Moment::Called(called)
                if called.name == "read_file"
                    && called.arguments.contains("notes.txt")
                    && matches!(
                        &called.outcome,
                        Some(Outcome::Returned { text, failed: false })
                            if text.contains("The meeting moved to Thursday.")
                    )
        ),
        "{:?}",
        transcript[1]
    );
}

/// A failed call reads as failed in the transcript, so a broken tool and a
/// model paraphrasing one do not look alike.
#[tokio::test]
async fn a_failed_call_reads_as_a_failure_in_the_transcript() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"path": "missing.txt"}"#)
        .then_say(&["It is not there."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask("Read it.", |_| {}, nobody()).await.unwrap();

    let transcript = chat.transcript().unwrap();
    let called = only_call(&transcript);
    assert!(
        matches!(called.outcome, Some(Outcome::Returned { failed: true, .. })),
        "{:?}",
        called.outcome
    );
}

/// A declined call is answered in the transcript too, and it is not a failed
/// tool: nothing was attempted.
#[tokio::test]
async fn a_declined_call_reads_as_declined_rather_than_as_a_failure() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "plan.txt", "content": "x"}"#,
        )
        .then_say(&["Then I will not."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask(
        "Write it.",
        |_| {},
        Person::answering(&[Decision::Deny]).approve(),
    )
    .await
    .unwrap();

    let transcript = chat.transcript().unwrap();
    let called = only_call(&transcript);
    assert!(
        matches!(&called.outcome, Some(Outcome::Refused { text }) if text.contains("declined")),
        "{:?}",
        called.outcome
    );
}

/// *Always for this tool* is written to the ladder's **chat** tier and to no
/// other. #55's own line: one answer about one conversation is not consent for
/// every conversation the person ever opens.
#[tokio::test]
async fn always_for_this_tool_writes_the_chat_tier_and_never_the_global_one() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "a.txt", "content": "a"}"#,
        )
        .then_say(&["Written."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    chat.ask(
        "Write a.",
        |_| {},
        Person::answering(&[Decision::Always]).approve(),
    )
    .await
    .unwrap();

    assert_eq!(
        rig.settings
            .resolve(&demido_settings::Ladder::for_chat(SESSION))
            .always(),
        vec!["write_file".to_owned()]
    );
    assert_eq!(
        rig.settings
            .resolve(&demido_settings::Ladder::for_chat("another"))
            .always(),
        Vec::<String>::new(),
        "another conversation was never asked and never answered"
    );
    assert_eq!(
        rig.settings
            .resolve(&demido_settings::Ladder::global())
            .always(),
        Vec::<String>::new(),
        "and the global tier is not where a transcript may write"
    );
}

/// The floor, held in the loop rather than in the window: a window that sent
/// *always* about a destructive call gets the call allowed and nothing
/// remembered, because the matrix asks about the next one too.
#[tokio::test]
async fn always_on_a_destructive_call_is_not_remembered_however_it_arrives() {
    let script = Script::serving("scripted")
        .then_call("call-1", "delete_file", r#"{"path": "notes.txt"}"#)
        .then_say(&["Gone."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    chat.ask(
        "Delete it.",
        |_| {},
        Person::answering(&[Decision::Always]).approve(),
    )
    .await
    .unwrap();

    assert!(!rig.path("notes.txt").exists(), "the call still ran");
    assert_eq!(
        rig.settings
            .resolve(&demido_settings::Ladder::for_chat(SESSION))
            .always(),
        Vec::<String>::new(),
        "nothing destructive is ever covered by an always"
    );
}

/// The one call in a transcript. Panics with what is there when there is not
/// exactly one, which is what a reader of a failed test wants.
fn only_call(transcript: &[Moment]) -> &Called {
    let calls: Vec<&Called> = transcript
        .iter()
        .filter_map(|moment| match moment {
            Moment::Called(called) => Some(called),
            Moment::Said(_) => None,
        })
        .collect();
    match calls.as_slice() {
        [one] => one,
        other => panic!("expected one call in the transcript, found {}", other.len()),
    }
}

/// A call and its result are two events, each with a source and a weight.
#[tokio::test]
async fn a_call_and_its_result_are_separate_events_each_carrying_source_and_weight() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"path": "notes.txt"}"#)
        .then_say(&["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask("When?", |_| {}, nobody()).await.unwrap();

    let events = rig.events();
    let called = events
        .iter()
        .find(|event| matches!(event.body, Body::Call { .. }))
        .expect("the call is on the log");
    let returned = events
        .iter()
        .find(|event| matches!(event.body, Body::Result { .. }))
        .expect("the result is on the log");

    assert_ne!(called.seq, returned.seq);
    assert!(
        matches!(returned.body, Body::Result { call, failed: false, .. } if call == called.seq)
    );
    for event in [called, returned] {
        assert_eq!(event.source, Source::Tool);
        assert!(event.weight.tokens > 0, "{:?} weighs nothing", event.body);
    }
}

/// Every request the loop sent, on every step, rebuilds from the log, tools
/// included.
#[tokio::test]
async fn every_request_the_loop_sent_rebuilds_from_the_log() {
    let script = Script::serving("scripted")
        .then(vec![
            Step::Say("Let me look.".into()),
            Step::Call(call("call-1", "read_file", json!({"path": "notes.txt"}))),
        ])
        .then_call(
            "call-2",
            "write_file",
            r#"{"path": "a.txt", "content": "x"}"#,
        )
        .then_say(&["Done."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask(
        "Go.",
        |_| {},
        Person::answering(&[Decision::Deny]).approve(),
    )
    .await
    .unwrap();

    let assemblies: Vec<u64> = rig
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::Assembly { .. }))
        .map(|event| event.seq)
        .collect();
    let sent = rig.script.requests();
    assert_eq!(assemblies.len(), sent.len());
    assert_eq!(sent.len(), 3);

    let replay = Replay::of(&rig.log).unwrap();
    for (seq, request) in assemblies.iter().zip(&sent) {
        assert_eq!(
            &replay.request(*seq).unwrap(),
            request,
            "the log rebuilt a different request from the one step {seq} sent"
        );
    }
}

/// Cautious asks about a write, shows the person the thing that will run, and
/// runs it once they allow it.
#[tokio::test]
async fn a_call_the_matrix_asks_about_runs_once_the_person_allows_it() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "plan.txt", "content": "ship it"}"#,
        )
        .then_say(&["Written."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let person = Person::answering(&[Decision::Allow]);
    chat.ask("Write the plan.", |_| {}, person.approve())
        .await
        .unwrap();

    let asked = person.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].tool, "write_file");
    assert_eq!(asked[0].ability, Ability::Write);
    assert!(!asked[0].destructive);
    assert_eq!(
        asked[0].arguments,
        json!({"path": "plan.txt", "content": "ship it"}),
        "the person decides about the arguments that will actually run"
    );
    assert_eq!(
        std::fs::read_to_string(rig.path("plan.txt")).unwrap(),
        "ship it"
    );
    assert_eq!(count(&rig.events(), results), 1);
}

/// The row the mode picked is the row that runs: Balanced writes without asking.
#[tokio::test]
async fn a_call_the_mode_permits_runs_without_asking() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "plan.txt", "content": "x"}"#,
        )
        .then_say(&["Written."]);
    let rig = Rig::new(script);
    let chat = rig.chat("balanced");
    chat.load(|_| {}).await;

    chat.ask("Write it.", |_| {}, nobody()).await.unwrap();
    assert!(rig.path("plan.txt").exists());
}

/// A denial is information: the call does not run, the model is told in the
/// place a result would go, and the turn goes on to an answer.
#[tokio::test]
async fn a_denial_is_handed_to_the_model_and_nothing_runs() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "plan.txt", "content": "x"}"#,
        )
        .then_say(&["I will not write it, then."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let answer = chat
        .ask(
            "Write it.",
            |_| {},
            Person::answering(&[Decision::Deny]).approve(),
        )
        .await
        .expect("a denial is not an error");
    assert_eq!(answer.text, "I will not write it, then.");
    assert!(!rig.path("plan.txt").exists(), "a denied call does not run");

    let sent = rig.script.requests();
    let told = sent[1].messages.last().unwrap();
    assert_eq!(told.role, Role::Tool);
    assert_eq!(told.answers.as_deref(), Some("call-1"));
    assert!(
        told.content.contains("declined") && told.content.contains("write_file"),
        "the model is told which call the person declined: {}",
        told.content
    );

    let events = rig.events();
    assert_eq!(count(&events, results), 0, "nothing was attempted");
    let refusal = events
        .iter()
        .find(|event| refusals(&event.body))
        .expect("the denial is on the log as what the model was told");
    assert_eq!(refusal.source, Source::System);
}

/// A refusal is a decision rather than a loop: the identical call straight
/// after a denial is not run, the person is not asked it again, and the second
/// refusal does not read like the first.
///
/// The last of those is [#59](https://github.com/elpideus/demido-studio/issues/59)'s
/// finding, measured rather than guessed at: told the same sentence twice, the
/// development model made the identical write three times in one turn.
#[tokio::test]
async fn the_identical_call_after_a_denial_is_neither_run_nor_asked_again() {
    let same = r#"{"path": "plan.txt", "content": "x"}"#;
    let script = Script::serving("scripted")
        .then_call("call-1", "write_file", same)
        .then_call("call-2", "write_file", same)
        .then_say(&["Understood."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let person = Person::answering(&[Decision::Deny]);
    chat.ask("Write it.", |_| {}, person.approve())
        .await
        .unwrap();

    assert_eq!(person.asked().len(), 1, "one decision, not one per retry");
    assert!(!rig.path("plan.txt").exists());

    let sent = rig.script.requests();
    assert_eq!(sent.len(), 3);
    let first = sent[1].messages.last().unwrap();
    let told = sent[2].messages.last().unwrap();
    assert_eq!(told.answers.as_deref(), Some("call-2"));
    assert!(
        told.content.contains("already made this exact call"),
        "the second refusal repeats the first: {}",
        told.content
    );
    assert_ne!(
        told.content, first.content,
        "a model that ignored a sentence once is handed the same sentence again"
    );
    // The first denial leaves the tools alone, and the repeat takes them away.
    assert!(
        !sent[0].tools.is_empty(),
        "the turn started with something to withhold"
    );
    assert!(
        !sent[1].tools.is_empty(),
        "one denial took the tools away, and that is the step the model is \
         supposed to be choosing something else in"
    );
    assert!(
        sent[2].tools.is_empty(),
        "the repeat left the tools in: {:?}",
        sent[2]
            .tools
            .iter()
            .map(|tool| &tool.name)
            .collect::<Vec<_>>()
    );

    let events = rig.events();
    assert_eq!(count(&events, results), 0);
    assert_eq!(count(&events, refusals), 2);
}

/// A step that ran something keeps its tools, which is every ordinary turn.
///
/// The guard beside the one above: a rule that took the tools away whenever a
/// step ended would end every turn that used one.
#[tokio::test]
async fn a_step_that_ran_something_keeps_the_tools_the_turn_started_with() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"path": "notes.txt"}"#)
        .then_call("call-2", "read_file", r#"{"path": "notes.txt"}"#)
        .then_say(&["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask("When?", |_| {}, nobody()).await.unwrap();

    let sent = rig.script.requests();
    assert_eq!(sent.len(), 3);
    for (step, request) in sent.iter().enumerate() {
        assert_eq!(
            request.tools.len(),
            6,
            "step {step} of a turn whose calls all ran lost its tools"
        );
    }
}

/// A call whose arguments did not fit the schema is not a refusal: there is
/// something to fix, and fixing it means calling again.
#[tokio::test]
async fn a_call_that_did_not_parse_keeps_the_tools_so_the_model_can_correct_it() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"pathe": "notes.txt"}"#)
        .then_call("call-2", "read_file", r#"{"path": "notes.txt"}"#)
        .then_say(&["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask("When?", |_| {}, nobody()).await.unwrap();

    let sent = rig.script.requests();
    assert!(
        !sent[1].tools.is_empty(),
        "a misspelt argument took the tools away, so the correction had nothing \
         to be made with"
    );
    assert!(
        rig.events()
            .iter()
            .any(|event| matches!(&event.body, Body::Result { failed: true, .. })),
        "the parse failure is on the log as a failed result"
    );
}

/// A tool switched off is not a reason to take the others away.
///
/// The picker's whole point is saying "files but no shell", which no mode can
/// express. A turn that answered a call to the switched-off group by removing
/// the group the person left on would be undoing their choice for them.
#[tokio::test]
async fn a_call_to_a_switched_off_tool_leaves_the_groups_that_are_on() {
    let script = Script::serving("scripted")
        .then_call("call-1", "run_command", r#"{"command": "echo hi"}"#)
        .then_say(&["It is off, then."]);
    let rig = Rig::new(script);
    rig.settings
        .set(
            &Scope::chat(SESSION),
            demido_settings::id::TOOLS_OFFERED,
            &json!(["read_file"]),
        )
        .unwrap();
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;
    chat.ask("Run it.", |_| {}, nobody()).await.unwrap();

    let sent = rig.script.requests();
    assert_eq!(names(&sent[0]), ["read_file"]);
    assert_eq!(
        names(&sent[1]),
        ["read_file"],
        "the group the person left on was taken away because of the one they          turned off"
    );
}

fn names(request: &demido_inference::Request) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

/// *Always for this tool* is asked once, covers the tool's later calls in this
/// chat, and still holds on the next message.
#[tokio::test]
async fn always_for_this_tool_is_not_asked_again_for_that_tool() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "a.txt", "content": "a"}"#,
        )
        .then_call(
            "call-2",
            "write_file",
            r#"{"path": "b.txt", "content": "b"}"#,
        )
        .then_say(&["Both."])
        .then_call(
            "call-3",
            "write_file",
            r#"{"path": "c.txt", "content": "c"}"#,
        )
        .then_say(&["And c."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let person = Person::answering(&[Decision::Always]);
    chat.ask("Write a and b.", |_| {}, person.approve())
        .await
        .unwrap();
    chat.ask("Now c.", |_| {}, person.approve()).await.unwrap();

    assert_eq!(person.asked().len(), 1);
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(rig.path(name).exists(), "{name} was not written");
    }
}

/// The floor: *always* does not cover a destructive call, even in the mode that
/// approves everything else.
#[tokio::test]
async fn always_does_not_cover_a_destructive_call_even_in_autonomous() {
    let script = Script::serving("scripted")
        .then_call("call-1", "delete_file", r#"{"path": "notes.txt"}"#)
        .then_call("call-2", "delete_file", r#"{"path": "other.txt"}"#)
        .then_say(&["Gone."]);
    let rig = Rig::new(script);
    std::fs::write(rig.path("other.txt"), "x").unwrap();
    let chat = rig.chat("autonomous");
    chat.load(|_| {}).await;

    let person = Person::answering(&[Decision::Always, Decision::Allow]);
    chat.ask("Delete both.", |_| {}, person.approve())
        .await
        .unwrap();

    let asked = person.asked();
    assert_eq!(asked.len(), 2, "a destructive call asks every time");
    assert!(asked.iter().all(|asking| asking.destructive));
}

/// The log says which of the three the person answered.
#[tokio::test]
async fn an_approval_a_denial_and_an_always_are_three_distinct_events() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "a.txt", "content": "a"}"#,
        )
        .then_call(
            "call-2",
            "write_file",
            r#"{"path": "b.txt", "content": "b"}"#,
        )
        .then_call(
            "call-3",
            "write_file",
            r#"{"path": "c.txt", "content": "c"}"#,
        )
        .then_say(&["Done."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;

    let person = Person::answering(&[Decision::Allow, Decision::Deny, Decision::Always]);
    chat.ask("Write three.", |_| {}, person.approve())
        .await
        .unwrap();

    let decisions: Vec<serde_json::Value> = rig
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::Decided { .. }))
        .map(|event| serde_json::to_value(event).unwrap()["decision"].clone())
        .collect();
    assert_eq!(
        decisions,
        vec![json!("allow"), json!("deny"), json!("always")]
    );
}

/// A tool that failed reads as failed on the log, and the model is still told.
#[tokio::test]
async fn a_failed_call_is_recorded_failed_and_answered() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"path": "missing.txt"}"#)
        .then_call("call-2", "read_files", r#"{"path": "notes.txt"}"#)
        .then_say(&["Could not."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask("Read it.", |_| {}, nobody()).await.unwrap();

    let failed: Vec<bool> = rig
        .events()
        .iter()
        .filter_map(|event| match &event.body {
            Body::Result { failed, .. } => Some(*failed),
            _ => None,
        })
        .collect();
    assert_eq!(
        failed,
        vec![true, true],
        "a missing file and a guessed tool name"
    );
    let told = rig.script.requests()[2].messages.last().unwrap().clone();
    assert!(told.content.contains("read_file"), "{}", told.content);
}

/// The step limit ends a runaway loop, and it is the same limit in every mode.
#[tokio::test]
async fn the_step_limit_ends_a_runaway_loop_whatever_the_mode() {
    for mode in demido_permission::Mode::names() {
        let script = Script::serving("scripted").then_call(
            "call-1",
            "read_file",
            r#"{"path": "notes.txt"}"#,
        );
        let rig = Rig::new(script);
        rig.settings
            .set(&Scope::Global, demido_settings::id::STEP_LIMIT, &json!(2))
            .unwrap();
        let chat = rig.chat(mode);
        chat.load(|_| {}).await;

        let ended = chat.ask("Loop.", |_| {}, nobody()).await;
        assert!(
            matches!(ended, Err(demido_chat::Error::StepLimit { steps: 2 })),
            "{mode}: {ended:?}"
        );
        assert_eq!(
            rig.script.requests().len(),
            3,
            "{mode}: two steps run, and the third asks for a call that is refused"
        );

        let events = rig.events();
        assert_eq!(count(&events, results), 2, "{mode}");
        assert_eq!(
            count(&events, refusals),
            1,
            "{mode}: the call past the limit"
        );
        assert!(
            events.iter().any(|event| matches!(
                &event.body,
                Body::Failure { kind, .. } if kind == "step-limit"
            )),
            "{mode}: the limit is on the log as the reason the turn ended"
        );
    }
}

/// A stop while a call is still being generated keeps what was said and the
/// call that arrived, and runs nothing.
#[tokio::test]
async fn a_stop_mid_generation_records_the_partial_state_and_runs_nothing() {
    let script = Script::serving("scripted")
        .then(vec![
            Step::Say("Writing now.".into()),
            Step::Call(call(
                "call-1",
                "write_file",
                json!({"path": "plan.txt", "content": "x"}),
            )),
            Step::Say(" and more".into()),
            Step::Say(" and more".into()),
        ])
        .pausing(Duration::from_millis(100));
    let rig = Rig::new(script);
    let chat = Arc::new(rig.chat("autonomous"));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("Write it.", |_| {}, nobody()).await })
    };
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(chat.stop());

    let answer = asking.await.unwrap().expect("a stop is not an error");
    assert_eq!(answer.reason, FinishReason::Cancelled);
    assert_eq!(answer.text, "Writing now.");

    let events = rig.events();
    assert_eq!(count(&events, |body| matches!(body, Body::Call { .. })), 1);
    assert_eq!(count(&events, results), 0);
    assert_eq!(
        count(&events, refusals),
        1,
        "the call that arrived is answered, so the next message can carry it"
    );
    assert!(!rig.path("plan.txt").exists());
    assert_eq!(rig.script.requests().len(), 1);
    assert!(!chat.stop(), "nothing is left running");
}

/// A stop while the person is still deciding runs nothing and records no
/// decision nobody made.
#[tokio::test]
async fn a_stop_while_waiting_for_the_person_runs_nothing() {
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "write_file",
            r#"{"path": "plan.txt", "content": "x"}"#,
        )
        .then_say(&["Never reached."]);
    let rig = Rig::new(script);
    let chat = Arc::new(rig.chat("cautious"));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move {
            chat.ask(
                "Write it.",
                |_| {},
                |_asking: Asking| std::future::pending::<Decision>(),
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(chat.stop());

    let answer = tokio::time::timeout(Duration::from_secs(2), asking)
        .await
        .expect("a stop does not wait for the person")
        .unwrap()
        .unwrap();
    assert_eq!(answer.reason, FinishReason::Cancelled);

    let events = rig.events();
    assert_eq!(
        count(&events, |body| matches!(body, Body::Decided { .. })),
        0
    );
    assert_eq!(count(&events, results), 0);
    assert_eq!(count(&events, refusals), 1);
    assert!(!rig.path("plan.txt").exists());
    assert_eq!(rig.script.requests().len(), 1);
}

/// A stop while a command runs kills it, and whatever it would have done next
/// never happens.
#[cfg(windows)]
#[tokio::test]
async fn a_stop_while_a_command_runs_leaves_nothing_running() {
    let command = json!({
        "command": "ping -n 4 127.0.0.1 >nul & echo late> marker.txt"
    });
    let script = Script::serving("scripted")
        .then_call("call-1", "run_command", &command.to_string())
        .then_say(&["Never reached."]);
    let rig = Rig::new(script);
    let chat = Arc::new(rig.chat("autonomous"));
    chat.load(|_| {}).await;

    let asking = {
        let chat = chat.clone();
        tokio::spawn(async move { chat.ask("Run it.", |_| {}, nobody()).await })
    };
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(chat.stop());

    let answer = tokio::time::timeout(Duration::from_secs(2), asking)
        .await
        .expect("a stop does not wait for the command")
        .unwrap()
        .unwrap();
    assert_eq!(answer.reason, FinishReason::Cancelled);

    // Longer than the command would have taken to reach its second half.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert!(
        !marker(rig.project.path()).exists(),
        "the command outlived the stop"
    );
    let events = rig.events();
    assert_eq!(count(&events, results), 0);
    assert_eq!(count(&events, refusals), 1);
}

#[cfg(windows)]
fn marker(project: &std::path::Path) -> std::path::PathBuf {
    project.join("marker.txt")
}

/// The next message carries the calls and what came back, so the model does
/// not have to call again for what it already has.
#[tokio::test]
async fn the_next_message_carries_the_calls_and_their_results() {
    let script = Script::serving("scripted")
        .then_call("call-1", "read_file", r#"{"path": "notes.txt"}"#)
        .then_say(&["Thursday."]);
    let rig = Rig::new(script);
    let chat = rig.chat("cautious");
    chat.load(|_| {}).await;
    chat.ask("When?", |_| {}, nobody()).await.unwrap();
    chat.ask("Which room?", |_| {}, nobody()).await.unwrap();

    let sent = rig.script.requests();
    let third = &sent[2].messages;
    let roles: Vec<Role> = third.iter().map(|message| message.role).collect();
    assert_eq!(
        roles,
        vec![
            Role::User,
            Role::Assistant,
            Role::Tool,
            Role::Assistant,
            Role::User
        ]
    );
    assert_eq!(third[1].calls.len(), 1);
    assert_eq!(third[2].answers.as_deref(), Some("call-1"));

    let replay = Replay::of(&rig.log).unwrap();
    assert_eq!(replay.assembly(2).unwrap(), sent[2]);
}
