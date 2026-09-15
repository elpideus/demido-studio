//! What the session monitor reads, against a scripted backend.
//!
//! One question this suite exists for, and `docs/rules/tools.md` writes it
//! down: a person debugging *why did it not use the file tools* opens the
//! monitor, sees no file tools in the assembly, and has to be able to tell a
//! deliberate absence from a dropped one. A group switched off in the picker
//! and a group nothing ever offered must not read alike.
//!
//! The rebuild itself is asserted on the log, in `demido-trace`, because that
//! is where it is computed ([#57](https://github.com/elpideus/demido-studio/issues/57):
//! "The rebuild gets no seam of its own").

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::sync::Arc;

use demido_chat::{Assembly, Chat, Model, Standing, Toolbox};
use demido_inference::scripted::{Script, Scripted};
use demido_inference::Supervisor;
use demido_settings::{Memory as SettingsMemory, Scope, Settings};
use demido_tools::{files, shell, Registry, Workspace};
use demido_trace::{Memory, Source};
use serde_json::json;

const SESSION: &str = "monitored";

/// A project with a file in it, a log, the script, and the ladder.
struct Rig {
    project: tempfile::TempDir,
    prompts: tempfile::TempDir,
    log: Memory,
    script: Script,
    settings: Arc<Settings>,
}

impl Rig {
    fn new() -> Self {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("notes.txt"), "Thursday.\n").unwrap();
        Self {
            project,
            prompts: tempfile::tempdir().unwrap(),
            log: Memory::new(),
            script: Script::serving("scripted").then_say(&["Hello."]),
            settings: Arc::new(Settings::open(SettingsMemory::new())),
        }
    }

    /// A chat over the Files and Shell groups, with or without a folder for
    /// them to act in. No workspace is a registry that offers nothing, which is
    /// the ordinary state of a conversation nobody has attached a project to.
    fn chat(&self, workspace: bool) -> Chat<Scripted, Memory> {
        let workspace = workspace.then(|| Workspace::open(self.project.path()).unwrap());
        let registry = Registry::open(workspace)
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

    /// What the picker writes: the set this conversation offers.
    fn picked(&self, names: &[&str]) {
        self.settings
            .set(
                &Scope::chat(SESSION),
                demido_settings::id::TOOLS_OFFERED,
                &json!(names),
            )
            .unwrap();
    }
}

/// One turn, and the assembly the monitor draws when its answer is selected.
async fn assembly(chat: &Chat<Scripted, Memory>) -> Assembly {
    chat.load(|_| {}).await;
    let answer = chat
        .ask("hello", |_| {}, |_| async { unreachable!() })
        .await
        .expect("an answer");
    chat.assembly(answer.seq)
        .expect("read")
        .expect("an assembly was sent")
}

fn standing(assembly: &Assembly, group: &str) -> Standing {
    assembly
        .groups
        .iter()
        .find(|grouped| grouped.group == group)
        .unwrap_or_else(|| panic!("no {group} group: {:?}", assembly.groups))
        .standing
}

#[tokio::test]
async fn the_assembly_at_an_event_is_the_one_that_produced_it() {
    let rig = Rig::new();
    let chat = rig.chat(true);
    let assembly = assembly(&chat).await;

    let question = assembly
        .rebuild
        .blocks
        .iter()
        .find(|block| block.source == Source::User)
        .expect("the question is a block of the assembly");
    assert_eq!(question.text, "hello");

    let offered = assembly.rebuild.tools.as_ref().expect("a set was offered");
    assert!(
        offered
            .tools
            .iter()
            .any(|tool| tool.name == "read_file" && !tool.text.is_empty()),
        "the wording each tool was offered in is on the rebuild"
    );
}

#[tokio::test]
async fn a_group_the_picker_switched_off_is_drawn_as_switched_off() {
    // The person closed the Shell group and left Files open. The model was
    // never shown `run_command`, and the monitor says whose decision that was.
    let rig = Rig::new();
    rig.picked(&[
        "read_file",
        "list_directory",
        "search_files",
        "write_file",
        "delete_file",
    ]);
    let chat = rig.chat(true);
    let assembly = assembly(&chat).await;

    assert_eq!(standing(&assembly, "files"), Standing::Offered);
    assert_eq!(standing(&assembly, "shell"), Standing::SwitchedOff);

    let shell = assembly
        .groups
        .iter()
        .find(|grouped| grouped.group == "shell")
        .unwrap();
    assert_eq!(
        shell.absent,
        ["run_command"],
        "the absence names what is missing, not only that something is"
    );
    assert!(shell.offered.is_empty());
}

#[tokio::test]
async fn some_of_a_group_switched_off_is_partial_rather_than_off() {
    let rig = Rig::new();
    rig.picked(&["read_file", "run_command"]);
    let chat = rig.chat(true);
    let assembly = assembly(&chat).await;

    assert_eq!(standing(&assembly, "files"), Standing::Partial);
    assert_eq!(standing(&assembly, "shell"), Standing::Offered);
}

#[tokio::test]
async fn a_group_nothing_offered_is_drawn_as_dropped_rather_than_as_switched_off() {
    // Nobody switched anything off: there is no workspace, so the registry
    // offered nothing at all. A monitor that reported this as a person's choice
    // would send them to a control that would not fix it.
    let rig = Rig::new();
    let chat = rig.chat(false);
    let assembly = assembly(&chat).await;

    assert!(
        assembly.rebuild.tools.is_none(),
        "an assembly that offered nothing names no set"
    );
    assert_eq!(standing(&assembly, "files"), Standing::Dropped);
    assert_eq!(standing(&assembly, "shell"), Standing::Dropped);
}
