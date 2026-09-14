//! What reaches the payload: the offered set and the mode, off the ladder.
//!
//! `docs/rules/tools.md` keeps two axes apart. **Offered** is the picker's, a
//! set on the ladder that an override replaces; a tool switched off is absent
//! from what the backend receives, and a model naming it anyway is told the
//! user turned it off. **Permitted** is the mode's, a name on the same ladder,
//! with the chat as the last word
//! ([`docs/decisions/0007-a-chat-outranks-its-character.md`](../../../../docs/decisions/0007-a-chat-outranks-its-character.md)).
//!
//! Asserted against what the scripted backend was handed, never against what
//! the loop meant to send.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::future::{ready, Ready};
use std::sync::{Arc, Mutex};

use demido_chat::{Asking, Chat, Decision, Model, Toolbox};
use demido_inference::scripted::{Script, Scripted};
use demido_inference::{Request, Role, Supervisor};
use demido_settings::{id, Memory as SettingsMemory, Scope, Settings};
use demido_tools::{files, shell, Registry, Workspace};
use demido_trace::{Body, Journal, Layer, Memory};
use serde_json::json;

const SESSION: &str = "offered";

struct Rig {
    project: tempfile::TempDir,
    prompts: tempfile::TempDir,
    settings: Arc<Settings>,
}

impl Rig {
    fn new() -> Self {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("notes.txt"), "Thursday.\n").unwrap();
        Self {
            project,
            prompts: tempfile::tempdir().unwrap(),
            settings: Arc::new(Settings::open(SettingsMemory::new())),
        }
    }

    /// A conversation called `id`, over its own log, answering from `script`.
    async fn chat(&self, id: &str, script: &Script, log: &Memory) -> Chat<Scripted, Memory> {
        let registry = Registry::open(Some(Workspace::open(self.project.path()).unwrap()))
            .with_group(files())
            .with_group(shell());
        let log = log.clone();
        let chat = Chat::new(
            id,
            move || Ok(log.clone()),
            Arc::new(Supervisor::new()),
            Some(Model {
                config: script.clone(),
                id: "scripted".into(),
            }),
            self.settings.clone(),
            Toolbox::open(registry, self.prompts.path()),
        );
        chat.load(|_| {}).await;
        chat
    }

    fn set(&self, scope: &Scope, id: &str, value: serde_json::Value) {
        self.settings.set(scope, id, &value).unwrap();
    }
}

fn names(request: &Request) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

fn nobody() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |asking: Asking| panic!("nobody should have been asked about {}", asking.tool)
}

/// Somebody who says no to everything, and counts how often they were asked.
fn counting(asked: &Arc<Mutex<Vec<String>>>) -> impl FnMut(Asking) -> Ready<Decision> + Send {
    let asked = asked.clone();
    move |asking: Asking| {
        asked.lock().unwrap().push(asking.tool);
        ready(Decision::Deny)
    }
}

#[tokio::test]
async fn every_tool_is_offered_until_somebody_names_a_set() {
    let rig = Rig::new();
    let script = Script::serving("scripted").then_say(&["Hello."]);
    let chat = rig.chat(SESSION, &script, &Memory::new()).await;

    chat.ask("Hello.", |_| {}, nobody()).await.unwrap();

    assert_eq!(
        names(&script.requests()[0]),
        [
            "read_file",
            "list_directory",
            "search_files",
            "write_file",
            "delete_file",
            "run_command"
        ]
    );
}

#[tokio::test]
async fn a_tool_switched_off_is_absent_from_what_the_backend_receives() {
    let rig = Rig::new();
    rig.set(
        &Scope::chat(SESSION),
        id::TOOLS_OFFERED,
        json!(["read_file", "list_directory"]),
    );
    let script = Script::serving("scripted").then_say(&["Hello."]);
    let log = Memory::new();
    let chat = rig.chat(SESSION, &script, &log).await;

    chat.ask("Hello.", |_| {}, nobody()).await.unwrap();

    let sent = &script.requests()[0];
    assert_eq!(names(sent), ["read_file", "list_directory"]);
    let everything = serde_json::to_string(sent).unwrap();
    assert!(
        !everything.contains("run_command") && !everything.contains("write_file"),
        "not held back, not mentioned: {everything}"
    );

    let offered = log
        .events()
        .unwrap()
        .into_iter()
        .find_map(|event| match event.body {
            Body::Offered { layer, .. } => Some(layer),
            _ => None,
        })
        .expect("the set is on the log");
    assert_eq!(offered, Layer::Chat, "and it says the chat decided it");
}

#[tokio::test]
async fn an_empty_set_sends_no_tools_at_all() {
    let rig = Rig::new();
    rig.set(&Scope::chat(SESSION), id::TOOLS_OFFERED, json!([]));
    let script = Script::serving("scripted").then_say(&["Hello."]);
    let chat = rig.chat(SESSION, &script, &Memory::new()).await;

    chat.ask("Hello.", |_| {}, nobody()).await.unwrap();

    assert!(script.requests()[0].tools.is_empty());
}

/// Disabled means absent, and a model that names it anyway is told who closed
/// it: not "no such tool", which would send it hunting for another name.
#[tokio::test]
async fn a_model_naming_a_switched_off_tool_is_told_the_user_turned_it_off() {
    let rig = Rig::new();
    rig.set(
        &Scope::chat(SESSION),
        id::TOOLS_OFFERED,
        json!(["read_file"]),
    );
    rig.set(&Scope::chat(SESSION), id::TOOLS_MODE, json!("autonomous"));
    let script = Script::serving("scripted")
        .then_call(
            "call-1",
            "run_command",
            r#"{"command": "echo ran > ran.txt"}"#,
        )
        .then_say(&["Shell access is off."]);
    let log = Memory::new();
    let chat = rig.chat(SESSION, &script, &log).await;

    let answer = chat.ask("Run it.", |_| {}, nobody()).await.unwrap();
    assert_eq!(answer.text, "Shell access is off.");
    assert!(!rig.project.path().join("ran.txt").exists());

    let told = script.requests()[1].messages.last().unwrap().clone();
    assert_eq!(told.role, Role::Tool);
    assert_eq!(told.answers.as_deref(), Some("call-1"));
    assert!(
        told.content.contains("turned `run_command` off"),
        "{}",
        told.content
    );

    let events = log.events().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event.body, Body::Refusal { .. })));
    assert!(!events
        .iter()
        .any(|event| matches!(event.body, Body::Result { .. })));
}

#[tokio::test]
async fn the_mode_comes_off_the_ladder_and_the_chat_is_the_last_word() {
    let write = r#"{"path": "plan.txt", "content": "x"}"#;
    let rig = Rig::new();

    // Global says Balanced, and a write runs without asking.
    rig.set(&Scope::Global, id::TOOLS_MODE, json!("balanced"));
    let script = Script::serving("scripted")
        .then_call("call-1", "write_file", write)
        .then_say(&["Written."]);
    let chat = rig.chat(SESSION, &script, &Memory::new()).await;
    chat.ask("Write it.", |_| {}, nobody()).await.unwrap();

    // The chat says Cautious over it, and the same write asks.
    rig.set(&Scope::chat(SESSION), id::TOOLS_MODE, json!("cautious"));
    let script = Script::serving("scripted")
        .then_call("call-2", "write_file", write)
        .then_say(&["Not written."]);
    let chat = rig.chat(SESSION, &script, &Memory::new()).await;
    let asked = Arc::new(Mutex::new(Vec::new()));
    chat.ask("Write it.", |_| {}, counting(&asked))
        .await
        .unwrap();

    assert_eq!(*asked.lock().unwrap(), ["write_file"]);
}

/// Read per turn, not fixed per conversation: the control changed between two
/// messages is the one the second message is ruled under.
#[tokio::test]
async fn a_mode_changed_between_messages_rules_the_next_one() {
    let write = r#"{"path": "plan.txt", "content": "x"}"#;
    let rig = Rig::new();
    let script = Script::serving("scripted")
        .then_call("call-1", "write_file", write)
        .then_say(&["Not written."])
        .then_call("call-2", "write_file", write)
        .then_say(&["Written."]);
    let chat = rig.chat(SESSION, &script, &Memory::new()).await;

    let asked = Arc::new(Mutex::new(Vec::new()));
    chat.ask("Write it.", |_| {}, counting(&asked))
        .await
        .unwrap();
    rig.set(&Scope::chat(SESSION), id::TOOLS_MODE, json!("balanced"));
    chat.ask("Now write it.", |_| {}, nobody()).await.unwrap();

    assert_eq!(
        asked.lock().unwrap().len(),
        1,
        "Cautious asked, Balanced did not"
    );
    assert!(rig.project.path().join("plan.txt").exists());
}

#[tokio::test]
async fn a_set_changed_in_one_chat_changes_no_other_chat() {
    let rig = Rig::new();
    rig.set(&Scope::chat("one"), id::TOOLS_OFFERED, json!(["read_file"]));

    let first = Script::serving("scripted").then_say(&["One."]);
    let second = Script::serving("scripted").then_say(&["Two."]);
    let one = rig.chat("one", &first, &Memory::new()).await;
    let two = rig.chat("two", &second, &Memory::new()).await;
    one.ask("Hello.", |_| {}, nobody()).await.unwrap();
    two.ask("Hello.", |_| {}, nobody()).await.unwrap();

    assert_eq!(names(&first.requests()[0]), ["read_file"]);
    assert_eq!(second.requests()[0].tools.len(), 6);
}

#[tokio::test]
async fn a_set_changed_globally_is_what_a_new_chat_opens_with() {
    let rig = Rig::new();
    rig.set(&Scope::Global, id::TOOLS_OFFERED, json!(["list_directory"]));

    let script = Script::serving("scripted").then_say(&["New."]);
    let fresh = rig.chat("made-after", &script, &Memory::new()).await;
    fresh.ask("Hello.", |_| {}, nobody()).await.unwrap();

    assert_eq!(names(&script.requests()[0]), ["list_directory"]);
}

/// What the picker draws, grouped the way the registry was assembled.
#[tokio::test]
async fn the_picker_is_given_the_groups_the_registry_holds() {
    let rig = Rig::new();
    let chat = rig
        .chat(SESSION, &Script::serving("scripted"), &Memory::new())
        .await;

    let groups: Vec<(String, usize)> = chat
        .groups()
        .into_iter()
        .map(|group| (group.group, group.tools.len()))
        .collect();
    assert_eq!(groups, [("files".to_owned(), 5), ("shell".to_owned(), 1)]);
}

/// The ladder's list of mode names is the matrix's, so the control cannot offer
/// a mode the matrix would read as an unknown name.
#[test]
fn the_modes_the_ladder_accepts_are_the_rows_the_matrix_has() {
    assert_eq!(
        demido_settings::MODES.to_vec(),
        demido_permission::Mode::names().collect::<Vec<_>>()
    );
}
