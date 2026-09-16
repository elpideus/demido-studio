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
use demido_trace::{Body, Event, Memory, Source};
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
        Self::saying(&["Hello."])
    }

    /// A rig whose model answers in the pieces given, which is what a real one
    /// does: an answer arrives as a run of chunks.
    fn saying(chunks: &[&str]) -> Self {
        Self::running(Script::serving("scripted").then_say(chunks))
    }

    /// A rig whose model follows `script`, for the turns that are not one
    /// answer and nothing else.
    fn running(script: Script) -> Self {
        let project = tempfile::tempdir().unwrap();
        std::fs::write(project.path().join("notes.txt"), "Thursday.\n").unwrap();
        Self {
            project,
            prompts: tempfile::tempdir().unwrap(),
            log: Memory::new(),
            script,
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
            demido_chat::delegations().1,
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
async fn an_assembly_that_offered_nothing_claims_neither_reason_for_it() {
    // There is no workspace, so the registry offered nothing at all. Nobody
    // switched anything off either, and the log cannot tell those two apart:
    // an empty set is an empty set. A monitor that guessed *switched off* would
    // send somebody to a control that would not fix it, and one that guessed
    // *dropped* would tell somebody who did switch everything off that their
    // install is broken.
    let rig = Rig::new();
    let chat = rig.chat(false);
    let assembly = assembly(&chat).await;

    assert!(
        assembly.rebuild.tools.is_none(),
        "an assembly that offered nothing names no set"
    );
    assert_eq!(standing(&assembly, "files"), Standing::Nothing);
    assert_eq!(standing(&assembly, "shell"), Standing::Nothing);
}

#[tokio::test]
async fn a_workspace_that_is_not_there_is_not_reported_as_the_picker() {
    // The crossed case, and the one worth having a test for: a person has named
    // a set, every tool in it is switched on, and there is still no workspace
    // for any of them to act in. The set the log records is empty and its layer
    // is the chat, so a verdict read off the layer alone would call this a
    // deliberate absence. It is a defect, and those two must not look alike
    // (`docs/rules/tools.md`).
    let rig = Rig::new();
    rig.picked(&["read_file", "run_command"]);
    let chat = rig.chat(false);
    let assembly = assembly(&chat).await;

    assert_eq!(standing(&assembly, "files"), Standing::Nothing);
    assert_eq!(standing(&assembly, "shell"), Standing::Nothing);
}

#[tokio::test]
async fn the_answer_is_one_event_rather_than_the_run_of_chunks_that_assembled_it() {
    // "The stream groups events into turns, with chunk runs folded into the
    // message they assembled" (#57). The fold is the log's own: tokens are
    // updates while the turn runs and never become events, so the monitor reads
    // one `turn/completion` carrying the whole answer. Asserted here because a
    // window cannot assert it, and because the day a chunk becomes an event is
    // the day the stream turns into a firehose.
    let rig = Rig::saying(&["The meeting ", "moved to ", "Thursday."]);
    let chat = rig.chat(true);
    chat.load(|_| {}).await;

    let mut chunks = 0;
    let answer = chat
        .ask(
            "when?",
            |update| {
                if matches!(update, demido_chat::Update::Text { .. }) {
                    chunks += 1;
                }
            },
            |_| async { unreachable!() },
        )
        .await
        .expect("an answer");

    assert_eq!(chunks, 3, "the backend streamed the answer in pieces");
    assert_eq!(answer.text, "The meeting moved to Thursday.");

    let log = chat.log().expect("read");
    let completions: Vec<&Event> = log
        .iter()
        .filter(|event| matches!(event.body, Body::Completion { .. }))
        .collect();
    assert_eq!(completions.len(), 1, "one event, not one per chunk");
    assert!(
        matches!(
            &completions[0].body,
            Body::Completion { text, .. } if text == "The meeting moved to Thursday."
        ),
        "the one event carries the whole answer the chunks assembled"
    );
}

/// The third road to an empty assembly, and the one a person cannot go and
/// change.
///
/// A turn that only repeats a call the person already declined loses its tools
/// ([#59](https://github.com/elpideus/demido-studio/issues/59)), so a reader who
/// selects the last step of such a turn finds no tools in it. That must not read
/// as a picker with everything switched off and must not read as a registry with
/// nowhere to act: it is Demido's own doing, and the monitor says so.
#[tokio::test]
async fn a_group_demido_withheld_after_a_repeated_denial_is_drawn_as_withheld() {
    let same = r#"{"path": "a.txt", "content": "a"}"#;
    let rig = Rig::running(
        Script::serving("scripted")
            .then_call("call-1", "write_file", same)
            .then_call("call-2", "write_file", same)
            .then_say(&["Not written, then."]),
    );
    let chat = rig.chat(true);
    chat.load(|_| {}).await;

    chat.ask(
        "Write it.",
        |_| {},
        |_| async { demido_chat::Decision::Deny },
    )
    .await
    .expect("a denial is not an error");

    // The turn's first assembly is the ordinary one, and the step after the
    // denial is not. Selected by their own events rather than by the answer's,
    // because the assembly in force at the end of the turn is the last one.
    let assemblies: Vec<Event> = chat
        .log()
        .expect("read")
        .into_iter()
        .filter(|event| matches!(event.body, Body::Assembly { .. }))
        .collect();
    assert_eq!(
        assemblies.len(),
        3,
        "the denial, the repeat, and the step that lost its tools"
    );

    let first = chat
        .assembly(assemblies[0].seq)
        .expect("read")
        .expect("an assembly");
    assert_eq!(standing(&first, "files"), Standing::Offered);

    let last = assemblies.last().expect("the turn stepped");
    let withheld = chat.assembly(last.seq).expect("read").expect("an assembly");
    assert_eq!(standing(&withheld, "files"), Standing::Withheld);
    assert_eq!(standing(&withheld, "shell"), Standing::Withheld);
}
