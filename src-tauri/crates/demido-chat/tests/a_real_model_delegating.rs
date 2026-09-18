//! The live-model suite for S4: a real model hands a task to a sub-agent, and
//! the parent answers from what came back.
//!
//! This is the model gate of
//! [`docs/rules/done.md`](../../../../docs/rules/done.md) for
//! [#69](https://github.com/elpideus/demido-studio/issues/69), the ticket that
//! closes [#37](https://github.com/elpideus/demido-studio/issues/37). It runs
//! from a terminal, with no window and nobody at the keyboard, holds one model
//! resident by the process-wide permit every live suite uses, and is re-run
//! every slice from here on.
//!
//! Brief B19:
//!
//! > Models should be able to delegate an agent to do a specific task in a
//! > separate clean context, for two main reasons: not filling up context with
//! > useless tool call outputs and such things, and so that it can be
//! > parallelized
//!
//! ## The bar splits, and the reason is recorded rather than assumed
//!
//! The delegation **returning correctly** is `chose`: a sub-agent handed a task
//! comes back with an answer the parent then uses. The model **electing to
//! delegate at all** is `used`, because v2 could never get a small model to do
//! it unprompted. So every scenario below except the election names
//! `delegate_task` in the message, and the election is the one that does not:
//! it measures whether the model reaches for it, and prints the answer rather
//! than asserting it. A red there is the reason the bar is `used`, not a
//! defect.
//!
//! ## What is asserted against what
//!
//! Against the log, scoped by agent, and against what `llama.cpp` was actually
//! handed. At the default parallelism a delegation blocks, so every request is
//! recorded and then sent in the order the log holds its assemblies, and
//! [`sent_by`] pairs the two by that order: which agent a request belonged to
//! is the log's to say, and what it carried is the backend's.
//!
//! ## Which tiers run what
//!
//! The returning delegation runs on all three, because it is the slice's claim.
//! The election runs on all three as well, because it is a measurement of the
//! model and nothing else. The rest are Demido's: what a child inherits, what
//! the depth takes away, how often a person is asked, what the card admits and
//! what a stop reaches. They run on the development tier, with the one
//! exception the ticket names: parallelism refused is the **reference** model,
//! because that is the one the card cannot afford a second slot for.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

// The rig is the inference crate's, included rather than copied. Two copies of
// where the models are is two places for a path to go stale.
#[path = "../../demido-inference/tests/rig.rs"]
mod rig;

// `Watching` and the prune, shared with the S2 suite.
#[path = "support/live.rs"]
mod live;

use std::future::{ready, Ready};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use demido_chat::{Asking, Chat, Decision, Model, Moment, Outcome, Pool, Presence, Toolbox};
use demido_inference::{FinishReason, Request, Role, Supervisor};
use demido_settings::{id, Memory as SettingsMemory, Scope, Settings};
use demido_tools::{delegation, files, shell, Registry, Workspace};
use demido_trace::{AgentId, Body, Event, JsonLines, Replay};
use demido_vram::{Queued, MIB};

use live::{prune, scrub, Watching};
use rig::Tier;

/// Codes nothing in any model's weights and nothing else in this repo contains,
/// each in a sentence about a thing that does not exist. A parent that answers
/// with one got it from a sub-agent that read it off the disk: the scenarios
/// assert the code is absent from the first request rather than claiming it.
const WINCH: Planted = Planted {
    sentence: "The winch code for bay four is 7731-MARGATE-OXIDE.\n",
    code: "7731-MARGATE-OXIDE",
};
const HATCH: Planted = Planted {
    sentence: "The hatch code for deck two is 4418-TALLOW-QUAY.\n",
    code: "4418-TALLOW-QUAY",
};

/// A line planted in a file, and the part of it nothing else can supply.
struct Planted {
    sentence: &'static str,
    code: &'static str,
}

/// The weighed cost of one more slot for the development model at 32k, from
/// `docs/rules/done.md`'s NVML reading on #65. What the pool is handed in the
/// afforded scenario, because the build's own pool has no producer for a slot's
/// cost yet (#74) and queues every slot above the first as unmeasured.
const DEVELOPMENT_PER_SLOT: u64 = 620 * MIB;

/// The same for the reference model, derived on #65 (`demido-vram`'s table).
const REFERENCE_PER_SLOT: u64 = 1129 * MIB;

// --- the rig ----------------------------------------------------------------

/// A project with something planted in it, a log on disk, and the ladder.
struct Rig {
    project: PathBuf,
    dir: PathBuf,
    settings: Arc<Settings>,
    supervisor: Arc<Supervisor<Watching>>,
    session: String,
    tier: Tier,
}

impl Rig {
    /// A scratch project and log for one scenario on one tier.
    fn new(name: &str, tier: Tier) -> Self {
        let dir = std::env::temp_dir()
            .join("demido-chat-delegating-live")
            .join(format!("{name}-{}-{}", tier.label(), std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("project");
        std::fs::create_dir_all(&project).expect("made the project");
        std::fs::create_dir_all(dir.join("prompts")).expect("made the prompts directory");

        Watching::forget();
        let rig = Self {
            project,
            settings: Arc::new(Settings::open(SettingsMemory::new())),
            supervisor: Arc::new(Supervisor::new()),
            session: format!("{name}-{}", tier.label()),
            tier,
            dir,
        };
        // Room for a sub-agent to read a file and still answer. The default is
        // 4k, and a child whose context overflowed is a finding about the
        // setting rather than about delegation.
        rig.set(id::CONTEXT_LENGTH, json!(8192));
        rig
    }

    fn plant(&self, name: &str, content: &str) {
        let path = self.project.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("made the directory");
        }
        std::fs::write(path, content).expect("planted the file");
    }

    fn set(&self, id: &str, value: serde_json::Value) {
        self.settings
            .set(&Scope::chat(&self.session), id, &value)
            .expect("set on this chat");
    }

    /// A chat over the Files, Shell and Delegation groups, in `mode`, logging
    /// to a file.
    ///
    /// The Delegation group is wired the way the composition root wires it:
    /// both ends of the pair made together, the tool's end into the registry and
    /// the loop's end into the conversation.
    fn chat(&self, mode: &str) -> Chat<Watching, JsonLines> {
        self.set(id::TOOLS_MODE, json!(mode));
        let (delegating, delegations) = demido_chat::delegations();
        let registry = Registry::open(Some(Workspace::open(&self.project).expect("a workspace")))
            .with_group(files())
            .with_group(shell())
            .with_group(delegation(delegating));
        let path = self.log();
        Chat::new(
            self.session.as_str(),
            move || JsonLines::open(&path),
            self.supervisor.clone(),
            Some(Model {
                config: rig::require(self.tier),
                id: self.tier.label().to_owned(),
            }),
            self.settings.clone(),
            Toolbox::open(registry, self.dir.join("prompts")),
            delegations,
        )
    }

    fn log(&self) -> PathBuf {
        self.dir.join("session.jsonl")
    }

    fn events(&self) -> Vec<Event> {
        JsonLines::read(self.log()).expect("read the log")
    }

    fn replay(&self) -> Replay {
        Replay::over(self.events())
    }
}

/// Load, or say which tier failed and what the backend said about it.
async fn loaded(chat: &Chat<Watching, JsonLines>, tier: Tier) -> Presence {
    match chat.load(|_| {}).await {
        ready @ Presence::Ready { .. } => ready,
        other => panic!("the {} model did not load: {other:?}", tier.label()),
    }
}

/// Nobody is at the window, and in this scenario nobody should need to be.
fn nobody() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |asking: Asking| {
        panic!(
            "nobody should have been asked about {}, and was asked anyway",
            asking.tool
        )
    }
}

/// Somebody at the window who answers `decision` about `delegate_task` and
/// allows everything else, and remembers what they were shown.
#[derive(Clone, Default)]
struct Person(Arc<Mutex<Vec<Asking>>>);

impl Person {
    fn answering(&self, decision: Decision) -> impl FnMut(Asking) -> Ready<Decision> + Send {
        let shown = self.0.clone();
        move |asking: Asking| {
            let answer = if asking.tool == "delegate_task" {
                decision
            } else {
                Decision::Allow
            };
            shown
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .push(asking);
            ready(answer)
        }
    }

    fn asked(&self) -> Vec<Asking> {
        self.0
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }

    fn asked_about(&self, tool: &str) -> Vec<Asking> {
        self.asked()
            .into_iter()
            .filter(|asking| asking.tool == tool)
            .collect()
    }
}

/// Every delegation the conversation made, as the transcript draws them.
fn delegations(chat: &Chat<Watching, JsonLines>) -> Vec<demido_chat::Delegation> {
    chat.transcript()
        .expect("a transcript")
        .into_iter()
        .filter_map(|moment| match moment {
            Moment::Delegated(delegation) => Some(delegation),
            Moment::Said(_) | Moment::Called(_) => None,
        })
        .collect()
}

/// Every call one agent made, as its own transcript draws them.
fn calls_of(replay: &Replay, agent: &AgentId) -> Vec<demido_chat::Called> {
    Replay::over(replay.events().to_vec())
        .scoped_to(agent.clone())
        .transcript()
        .expect("a transcript")
        .into_iter()
        .filter_map(|moment| match moment {
            demido_trace::Moment::Called(called) => Some(called),
            _ => None,
        })
        .collect()
}

/// The sub-agents on the log, with the depth each was opened at.
fn sub_agents(replay: &Replay) -> Vec<(AgentId, u32)> {
    replay
        .agents()
        .into_iter()
        .filter(|agent| agent.depth > 0)
        .map(|agent| (agent.agent, agent.depth))
        .collect()
}

/// Every request the backend was handed, with the agent whose assembly it was.
///
/// Paired by order, which is only sound where nothing ran beside anything else:
/// at the default a delegation blocks, so each assembly is recorded and then
/// sent before the next is recorded. Asserted rather than assumed, by count.
fn sent_by(replay: &Replay) -> Vec<(AgentId, u64, Request)> {
    let assemblies: Vec<(AgentId, u64)> = replay
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::Assembly { .. }))
        .map(|event| (event.agent.clone(), event.seq))
        .collect();
    let sent = Watching::sent();
    assert_eq!(
        assemblies.len(),
        sent.len(),
        "the log records {} assemblies against {} requests the backend was \
         handed",
        assemblies.len(),
        sent.len()
    );
    assemblies
        .into_iter()
        .zip(sent)
        .map(|((agent, seq), request)| (agent, seq, request))
        .collect()
}

/// The names of the tools a request offered.
fn offered(request: &Request) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

/// **The clean context, asserted.** No request the conversation sent carries a
/// single event a sub-agent wrote.
///
/// Asked of the assembly's blocks rather than of the text. An assembly names
/// every block it sent by position (`docs/decisions/0009-an-assembly-refers-to-its-blocks.md`),
/// and every event names its agent, so "the parent never carried the child's
/// tool output" is exactly "every block of a parent assembly is the parent's".
/// The answer a child gave arrives as the parent's own `tool/result` or its own
/// framed fragment, which is what the parent is supposed to see.
///
/// A first version compared text, and the breadth model showed why that is the
/// wrong question: handed the code by its sub-agent, it then read the same file
/// itself to check, and its own read matched the child's line for line. That is
/// a model spending its context, which is reported, and not a leak.
fn parent_never_carried_child_output(replay: &Replay) {
    let agent_of = |seq: u64| {
        replay
            .events()
            .iter()
            .find(|event| event.seq == seq)
            .map(|event| event.agent.clone())
            .expect("a block the log holds")
    };
    for event in replay.events() {
        let Body::Assembly { blocks, .. } = &event.body else {
            continue;
        };
        if event.agent != AgentId::main() {
            continue;
        }
        for block in blocks {
            assert_eq!(
                agent_of(*block),
                AgentId::main(),
                "the conversation's assembly at {} carries block {block}, which a \
                 sub-agent wrote: the one thing a clean context promises never \
                 to do",
                event.seq
            );
        }
    }

    // And the model's own choice, said rather than asserted: a parent that
    // repeats its sub-agent's work inline spends the context the delegation
    // saved.
    let delegated_at = replay
        .events()
        .iter()
        .find(|event| matches!(event.body, Body::Delegated { .. }))
        .map(|event| event.seq);
    if let Some(at) = delegated_at {
        let again: Vec<&str> = replay
            .events()
            .iter()
            .filter(|event| event.seq > at && event.agent == AgentId::main())
            .filter_map(|event| match &event.body {
                Body::Call { name, .. } if name != "delegate_task" => Some(name.as_str()),
                _ => None,
            })
            .collect();
        if !again.is_empty() {
            println!("note: after delegating, the parent also called {again:?} itself");
        }
    }
}

/// Filler for a planted file: lines that are nothing but weight, each one
/// distinct so a search for any of them finds one line.
fn filler(what: &str, lines: usize) -> String {
    (1..=lines)
        .map(|line| {
            format!(
                "{what} ledger entry {line}: crate {line} of tinned pilchards, \
                 checked by the night porter, stowed aft, no discrepancy \
                 recorded against the manifest.\n"
            )
        })
        .collect()
}

// --- the scenarios ----------------------------------------------------------

/// **The delegation returns.** Delegated, answered, and the parent's reply uses
/// it, on all three tiers.
///
/// The file is planted with a code nothing else holds, among enough filler
/// that reading it is real tool output. The message names the tool, because
/// this is the returning half of the bar and not the election.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_delegation_returns_and_the_parent_answers_from_it() {
    for tier in Tier::ALL {
        returns(tier, false).await;
    }
}

async fn returns(tier: Tier, keep_the_fixture: bool) {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let rig = Rig::new("returns", tier);
    rig.plant(
        "bay4.txt",
        &format!(
            "{}{}{}",
            filler("Bay four", 12),
            WINCH.sentence,
            filler("Bay four", 6)
        ),
    );
    let chat = rig.chat("autonomous");
    loaded(&chat, tier).await;

    let answer = chat
        .ask(
            "Use the delegate_task tool to have a sub-agent read bay4.txt and \
             report the winch code written in it. Then tell me the code, and \
             only the code.",
            |_| {},
            nobody(),
        )
        .await
        .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));

    let replay = rig.replay();
    let sent = sent_by(&replay);
    assert!(
        !serde_json::to_string(&sent.first().expect("something was sent").2)
            .expect("a request")
            .contains(WINCH.code),
        "the code was already in the first payload, so this scenario proves \
         nothing"
    );

    let delegated = delegations(&chat);
    assert!(
        !delegated.is_empty(),
        "the {} model was told to delegate and did not: {:?}",
        tier.label(),
        calls_of(&replay, &AgentId::main())
    );
    assert!(
        delegated.iter().any(|delegation| delegation
            .answer
            .as_ref()
            .is_some_and(|answer| !answer.failed && answer.text.contains(WINCH.code))),
        "no sub-agent of the {} model came back with the code: {delegated:?}",
        tier.label()
    );
    assert!(
        answer.text.contains(WINCH.code),
        "the {} model's sub-agent found the code and the parent said: {}",
        tier.label(),
        answer.text
    );
    parent_never_carried_child_output(&replay);

    println!(
        "the {} model delegated {} time(s) and answered {:?}",
        tier.label(),
        delegated.len(),
        answer.text.trim()
    );

    if keep_the_fixture {
        keep(&rig, "a-delegation");
    }
    chat.shutdown().await;
}

/// **The election.** Nothing in the message names delegation: does the model
/// reach for it?
///
/// **Reported, never asserted.** The work is exactly the shape the tool's own
/// description says to delegate, several files read for one fact, so a model
/// that reads `delegate_task` and decides to use it is choosing. One that reads
/// the files itself has done nothing wrong: it has answered, and it has not
/// elected. A red here is recorded as the reason the bar is `used`, with what
/// would raise it, and is not filed as a defect.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_election_is_measured_with_nothing_naming_delegation() {
    let mut elected = Vec::new();
    for tier in Tier::ALL {
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

        let rig = Rig::new("election", tier);
        for (name, day) in [
            ("monday", "no errors"),
            ("tuesday", "backup failed: disk full"),
            ("wednesday", "no errors"),
            ("thursday", "backup failed: permission denied"),
            ("friday", "no errors"),
        ] {
            rig.plant(
                &format!("logs/{name}.log"),
                &format!("{}{name}: {day}\n", filler(name, 8)),
            );
        }
        let chat = rig.chat("autonomous");
        loaded(&chat, tier).await;

        let answer = chat
            .ask(
                "Go through every log in logs/ and tell me on which days the \
                 backup failed, and why.",
                |_| {},
                nobody(),
            )
            .await
            .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));
        assert!(
            !answer.text.trim().is_empty(),
            "the {} model said nothing at all",
            tier.label()
        );

        let replay = rig.replay();
        let chose: Vec<String> = calls_of(&replay, &AgentId::main())
            .into_iter()
            .map(|call| call.name)
            .collect();
        let delegated = chose.iter().any(|name| name == "delegate_task");
        println!(
            "election: the {} model {} with nothing naming it, and called {chose:?}",
            tier.label(),
            if delegated {
                "delegated"
            } else {
                "did not delegate"
            }
        );
        elected.push((tier.label(), delegated));
        chat.shutdown().await;
    }
    println!("election, by tier: {elected:?}");
}

/// **The clean context.** The parent's prompt tokens across a delegation,
/// measured from the log, against the same task done inline.
///
/// The one scenario that tests the brief's *stated reason* rather than the
/// mechanism: "not filling up context with useless tool call outputs". Four
/// reports of real weight, one figure in each, and a sum. Delegated, the parent
/// should see one answer; inline, it sees four files. The measurement is the
/// `prompt_tokens` `llama.cpp` reported on the conversation's last completion
/// of the turn, read off the log, on each run.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_parent_context_across_a_delegation_is_measured_against_the_same_task_inline() {
    let tier = Tier::Development;
    let question = "Read every file in reports/ and tell me the sum of the four \
                    quarterly figures, as a number.";

    let delegated = clean(
        tier,
        "clean-delegated",
        &format!("{question} Hand the reading to a sub-agent with the delegate_task tool."),
        Doing::Delegated,
    )
    .await;
    let inline = clean(tier, "clean-inline", question, Doing::Inline).await;

    println!(
        "clean context: the parent's last prompt was {} tokens across a \
         delegation and {} tokens with the same task done inline",
        delegated, inline
    );
    assert!(
        delegated < inline,
        "a delegation cost the parent {delegated} prompt tokens against {inline} \
         inline, which is the brief's stated reason for delegating failing"
    );
}

/// One run of the clean-context task. The conversation's last prompt, in tokens.
/// Which way the clean-context task is done.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Doing {
    /// Told to hand the reading to a sub-agent.
    Delegated,
    /// With `delegate_task` switched off, so the parent reads the files itself.
    Inline,
}

async fn clean(tier: Tier, name: &str, message: &str, doing: Doing) -> u32 {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let rig = Rig::new(name, tier);
    rig.set(id::CONTEXT_LENGTH, json!(16384));
    for (quarter, figure) in [("q1", 1200), ("q2", 3400), ("q3", 560), ("q4", 78)] {
        rig.plant(
            &format!("reports/{quarter}.txt"),
            &format!(
                "{}The {quarter} figure is {figure}.\n{}",
                filler(quarter, 10),
                filler(quarter, 10)
            ),
        );
    }
    let chat = rig.chat("autonomous");
    if doing == Doing::Inline {
        rig.set(
            id::TOOLS_OFFERED,
            json!(["read_file", "list_directory", "search_files"]),
        );
    }
    loaded(&chat, tier).await;

    let answer = chat
        .ask(message, |_| {}, nobody())
        .await
        .unwrap_or_else(|error| {
            panic!("{name}: the {} model did not answer: {error}", tier.label())
        });

    let replay = rig.replay();
    let delegated = !sub_agents(&replay).is_empty();
    let read_inline = calls_of(&replay, &AgentId::main())
        .iter()
        .any(|call| call.name == "read_file");
    println!(
        "{name}: delegated {delegated}, read inline {read_inline}, answered {:?}",
        answer.text.trim()
    );
    if doing == Doing::Delegated {
        assert!(delegated, "{name}: the parent never delegated");
        parent_never_carried_child_output(&replay);
    } else {
        assert!(
            read_inline,
            "{name}: the parent never read a file, so there is nothing inline to \
             compare against"
        );
    }

    let last = replay
        .events()
        .iter()
        .rev()
        .filter(|event| event.agent == AgentId::main())
        .find_map(|event| match &event.body {
            Body::Completion { usage, .. } => Some(usage.prompt_tokens),
            _ => None,
        })
        .expect("the conversation completed");
    chat.shutdown().await;
    last
}

/// **The narrowed child.** A parent with the Shell group switched off, a child
/// that cannot run a command and says so.
///
/// The load-bearing rule of `docs/rules/tools.md`: without it, "a user who
/// switched the shell off has a model that restores it by delegating".
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_child_of_a_parent_with_the_shell_off_cannot_run_a_command() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("narrowed", tier);
    let chat = rig.chat("autonomous");
    rig.set(
        id::TOOLS_OFFERED,
        json!([
            "read_file",
            "list_directory",
            "search_files",
            "delegate_task"
        ]),
    );
    loaded(&chat, tier).await;

    let answer = chat
        .ask(
            "Use the delegate_task tool to have a sub-agent run the command \
             `echo narrowed` and report exactly what it printed.",
            |_| {},
            nobody(),
        )
        .await
        .expect("an answer");

    let replay = rig.replay();
    let children = sub_agents(&replay);
    assert!(
        !children.is_empty(),
        "the parent never delegated, so nothing was narrowed: {:?}",
        calls_of(&replay, &AgentId::main())
    );
    for (agent, seq, request) in sent_by(&replay) {
        assert!(
            !offered(&request).contains(&"run_command"),
            "{agent:?}'s request at {seq} offered run_command with the Shell \
             group switched off: {:?}",
            offered(&request)
        );
    }
    // Nothing ran on the shell, at any depth, whatever anybody named.
    for event in replay.events() {
        if let Body::Call { name, .. } = &event.body {
            if name == "run_command" {
                let ran = replay.events().iter().any(
                    |later| matches!(&later.body, Body::Result { call, .. } if *call == event.seq),
                );
                assert!(!ran, "a run_command ran under {:?}", event.agent);
            }
        }
    }
    // And a child that named it anyway was told who closed it.
    for (agent, _) in &children {
        for call in calls_of(&replay, agent) {
            if call.name == "run_command" {
                assert!(
                    matches!(&call.outcome, Some(Outcome::Refused { id, .. }) if id == "tools.off"),
                    "a child naming the switched-off shell was told {:?}",
                    call.outcome
                );
            }
        }
    }

    let delegated = delegations(&chat);
    let said: Vec<&str> = delegated
        .iter()
        .filter_map(|delegation| delegation.answer.as_ref())
        .map(|answer| answer.text.trim())
        .collect();
    assert!(
        said.iter().any(|text| !text.is_empty()),
        "the child came back with nothing to say about the command it could not \
         run: {delegated:?}"
    );
    println!(
        "narrowed: the child said {said:?}, and the parent said {:?}",
        answer.text.trim()
    );
    chat.shutdown().await;
}

/// **The strict child.** A Cautious parent, and a child that runs Cautious.
///
/// ## The request half is not reachable from a model, by design
///
/// The ticket's sentence is "a child that requested Balanced". The inheritance
/// rule takes a request (`demido_permission::Request::at_mode`), and nothing in
/// this build makes one with a mode in it: `delegate_task` takes a task and
/// nothing else, and it stays that way, because `the_matrix.rs` holds every text
/// Demido sends a model to naming no mode. A permission the model is told about
/// is one that depends on the model. So the stricter-of is locked offline, on
/// every pair of modes (`demido-permission/tests/inheritance.rs`), and what this
/// scenario asserts is the half a model can reach: a child opened under a
/// Cautious parent is **asked about its own write**, which no row but Cautious
/// does. The delegation is allowed and its grant does not carry to the write.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_child_of_a_cautious_parent_runs_cautious() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("strict", tier);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    let person = Person::default();
    chat.ask(
        "Use the delegate_task tool to have a sub-agent save the single word \
         strict into a file called strict.txt.",
        |_| {},
        person.answering(Decision::Allow),
    )
    .await
    .expect("an answer");

    let replay = rig.replay();
    assert_eq!(
        person.asked_about("delegate_task").len(),
        1,
        "Cautious asks once about a delegation: {:?}",
        person.asked()
    );
    let writes = person.asked_about("write_file");
    assert!(
        !writes.is_empty(),
        "a child under a Cautious parent wrote without asking, so it did not run \
         Cautious: {:?}",
        person.asked()
    );
    for asking in &writes {
        let by = replay
            .events()
            .iter()
            .find(|event| event.seq == asking.call)
            .map(|event| event.agent.clone())
            .expect("the call is on the log");
        assert_ne!(
            by,
            AgentId::main(),
            "the write was the parent's own, so this says nothing about the child"
        );
    }
    let written = std::fs::read_to_string(rig.project.join("strict.txt"))
        .expect("the approved write reached the disk");
    assert!(written.to_lowercase().contains("strict"), "{written}");
    chat.shutdown().await;
}

/// **The depth refusal.** A child at the limit has no `delegate_task`, is told
/// why if it names it, and answers from what it has. Then one above the limit,
/// where the chain is allowed.
///
/// The telling is conditional, and stays so, for the reason the S2 suite's
/// switched-off tool is: a live scenario cannot make a model name a tool it was
/// not shown. The absence is asserted every run; the wording is asserted
/// whenever the model gives it the chance, and is locked offline in
/// `tests/delegated.rs`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_child_at_the_depth_limit_answers_from_what_it_has_and_one_above_it_delegates() {
    let tier = Tier::Development;
    let message = "Use the delegate_task tool to have a sub-agent find the winch \
                   code in bay4.txt. In the task, tell that sub-agent to hand the \
                   reading on to a sub-agent of its own with delegate_task. Then \
                   tell me the code.";

    // At the limit: depth 1, so the first child is the bottom of the chain.
    {
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");
        let rig = Rig::new("depth-1", tier);
        rig.plant("bay4.txt", WINCH.sentence);
        rig.set(id::DELEGATION_DEPTH, json!(1));
        let chat = rig.chat("autonomous");
        loaded(&chat, tier).await;

        let answer = chat
            .ask(message, |_| {}, nobody())
            .await
            .expect("an answer at depth 1");

        let replay = rig.replay();
        let children = sub_agents(&replay);
        assert!(!children.is_empty(), "the parent never delegated");
        assert!(
            children.iter().all(|(_, depth)| *depth == 1),
            "a chain past the depth of 1: {children:?}"
        );
        for (agent, seq, request) in sent_by(&replay) {
            if agent != AgentId::main() {
                assert!(
                    !offered(&request).contains(&"delegate_task"),
                    "the child at the limit was offered delegate_task at {seq}"
                );
            }
        }
        let mut named = 0;
        for (agent, _) in &children {
            for call in calls_of(&replay, agent) {
                if call.name == "delegate_task" {
                    named += 1;
                    assert!(
                        matches!(&call.outcome, Some(Outcome::Refused { id, .. }) if id == "tools.depth"),
                        "a child at the limit naming delegate_task was told {:?}",
                        call.outcome
                    );
                }
            }
        }
        let delegated = delegations(&chat);
        assert!(
            delegated.iter().any(|delegation| delegation
                .answer
                .as_ref()
                .is_some_and(|answer| !answer.failed && !answer.text.trim().is_empty())),
            "the child at the limit did not answer from what it had: {delegated:?}"
        );
        println!(
            "depth 1: the child named delegate_task {named} time(s) and was told \
             why each time; the parent said {:?}",
            answer.text.trim()
        );
        chat.shutdown().await;
    }

    // One above it: depth 2, and the chain the child was asked for is allowed.
    {
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");
        let rig = Rig::new("depth-2", tier);
        rig.plant("bay4.txt", WINCH.sentence);
        rig.set(id::DELEGATION_DEPTH, json!(2));
        let chat = rig.chat("autonomous");
        loaded(&chat, tier).await;

        let answer = chat
            .ask(message, |_| {}, nobody())
            .await
            .expect("an answer at depth 2");

        let replay = rig.replay();
        let children = sub_agents(&replay);
        assert!(
            children.iter().any(|(_, depth)| *depth == 2),
            "at depth 2 the child was told to delegate and the chain stopped at \
             {children:?}"
        );
        let bottom: Vec<&AgentId> = children
            .iter()
            .filter(|(_, depth)| *depth == 2)
            .map(|(agent, _)| agent)
            .collect();
        for (agent, seq, request) in sent_by(&replay) {
            let tools = offered(&request);
            if bottom.contains(&&agent) {
                assert!(
                    !tools.contains(&"delegate_task"),
                    "the grandchild at the limit was offered delegate_task at {seq}"
                );
            } else {
                assert!(
                    tools.contains(&"delegate_task"),
                    "{agent:?} above the limit was not offered delegate_task at {seq}"
                );
            }
        }
        println!(
            "depth 2: the chain reached {children:?}, and the parent said {:?}",
            answer.text.trim()
        );
        chat.shutdown().await;
    }
}

/// **One approval per turn.** Cautious, two delegations in one turn, one prompt.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn two_delegations_in_one_cautious_turn_ask_once() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("one-approval", tier);
    rig.plant("bay4.txt", WINCH.sentence);
    rig.plant("deck2.txt", HATCH.sentence);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    let person = Person::default();
    let answer = chat
        .ask(
            "Use the delegate_task tool twice: one sub-agent reads bay4.txt for \
             the winch code, and a second, separate sub-agent reads deck2.txt for \
             the hatch code. Then tell me both codes.",
            |_| {},
            person.answering(Decision::Allow),
        )
        .await
        .expect("an answer");

    let delegated = delegations(&chat);
    assert!(
        delegated.len() >= 2,
        "the model was told to delegate twice and delegated {} time(s)",
        delegated.len()
    );
    assert!(
        delegated
            .iter()
            .all(|delegation| delegation.turn == delegated[0].turn),
        "the delegations were not in one turn: {delegated:?}"
    );
    assert_eq!(
        person.asked_about("delegate_task").len(),
        1,
        "two delegations in one Cautious turn asked {} times: {:?}",
        person.asked_about("delegate_task").len(),
        person.asked()
    );
    println!(
        "one approval: {} delegations, one prompt, and the parent said {:?}",
        delegated.len(),
        answer.text.trim()
    );
    chat.shutdown().await;
}

/// **The denial.** Denied, and the model does something else.
///
/// `Bar: chose` here means it chose something else. What is asserted is S2's
/// shape: one decision rather than one per retry, no sub-agent opened, the turn
/// ending in something said, and the declined delegation never made a third
/// time. A second attempt before it gives up is the model's, and is reported.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_denied_delegation_opens_nothing_and_the_model_does_something_else() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("denied", tier);
    rig.plant("bay4.txt", WINCH.sentence);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    let person = Person::default();
    let answer = chat
        .ask(
            "Use the delegate_task tool to have a sub-agent read bay4.txt and \
             tell me the winch code in it.",
            |_| {},
            person.answering(Decision::Deny),
        )
        .await
        .expect("a denial is not an error");

    let replay = rig.replay();
    assert!(
        sub_agents(&replay).is_empty(),
        "a denied delegation opened a sub-agent anyway: {:?}",
        sub_agents(&replay)
    );
    assert!(
        !answer.text.trim().is_empty(),
        "the turn ended without the model saying anything"
    );

    let called = calls_of(&replay, &AgentId::main());
    let attempts: Vec<&demido_chat::Called> = called
        .iter()
        .filter(|call| call.name == "delegate_task")
        .collect();
    let asked = person.asked_about("delegate_task").len();
    assert!(
        asked >= 1,
        "nobody was asked about a delegation, so nothing was denied: {called:?}"
    );
    // Every attempt was refused, and none of them ran.
    assert!(
        attempts
            .iter()
            .all(|call| matches!(&call.outcome, Some(Outcome::Refused { .. }))),
        "a delegation the person denied was answered as if it ran: {attempts:?}"
    );
    // The same declined call a third time is the loop, and breaking it is
    // Demido's job. A reworded task is a different call, asked about again,
    // which is what the window gate saw.
    for call in &attempts {
        let same = attempts
            .iter()
            .filter(|other| other.arguments == call.arguments)
            .count();
        assert!(
            same < 3,
            "the declined delegation was made {same} times, so nothing broke the \
             loop: {called:?}"
        );
    }
    assert_eq!(
        asked,
        attempts
            .iter()
            .filter(|call| matches!(&call.outcome, Some(Outcome::Refused { id, .. }) if id == "tools.denied"))
            .count(),
        "a person is asked once per distinct delegation, and a repeat is refused \
         without asking: {:?}",
        person.asked()
    );
    let instead: Vec<&str> = called
        .iter()
        .filter(|call| call.name != "delegate_task")
        .map(|call| call.name.as_str())
        .collect();
    println!(
        "denial: the model tried delegate_task {} time(s), then called \
         {instead:?} and said {:?}",
        attempts.len(),
        answer.text.trim()
    );
    chat.shutdown().await;
}

/// **Parallelism, afforded.** Two sub-agents at once on the development model,
/// both folded in at step boundaries: three slots, the conversation's and two
/// in the pool.
///
/// The pool is the build's own arithmetic over the card as it reads right now,
/// with a slot priced at what #65 weighed on this card rather than at zero: the
/// build has no producer of a slot's cost yet (#74), so its own pool queues
/// every slot above the first as unmeasured, and that is what
/// [`parallelism_the_card_cannot_afford_queues_and_says_so`] shows. Here the
/// question is what happens once a second slot is admitted.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn two_sub_agents_run_at_once_on_the_development_model_and_both_fold_in() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("afforded", tier);
    rig.plant("bay4.txt", WINCH.sentence);
    rig.plant("deck2.txt", HATCH.sentence);
    // Three, because the setting counts slots and the conversation is the
    // first of them (`demido-chat/src/pool.rs`): at two, one sub-agent runs in
    // the pool and the second falls back to blocking, which is what the first
    // run of this scenario found.
    rig.set(id::PARALLEL_AGENTS, json!(3));
    let free = demido_vram::free_now().expect("the card reads").free;
    let chat = rig
        .chat("autonomous")
        .against(Pool::on_a_card_with(free, DEVELOPMENT_PER_SLOT));
    let presence = loaded(&chat, tier).await;
    assert_eq!(
        presence,
        Presence::Ready {
            model: tier.label().to_owned(),
            slots: 3,
            limit: None,
        },
        "the development model with {} MiB free did not open three slots",
        free / MIB
    );

    let answer = chat
        .ask(
            "Use the delegate_task tool twice, both at once in one step: one \
             sub-agent reads bay4.txt for the winch code, and a second, separate \
             sub-agent reads deck2.txt for the hatch code. Then tell me both \
             codes.",
            |_| {},
            nobody(),
        )
        .await
        .expect("an answer");

    let events = rig.events();
    let returned: Vec<&Event> = events
        .iter()
        .filter(|event| matches!(event.body, Body::Returned { .. }))
        .collect();
    assert!(
        returned.len() >= 2,
        "two sub-agents in the pool and {} answer(s) folded in",
        returned.len()
    );
    // Folded in at a step boundary: every call the parent made before an
    // `agent/returned` already has its answer by then, so no answer lands in the
    // middle of a step.
    let answered = |call: u64, by: u64| {
        events.iter().any(|event| {
            event.seq < by
                && matches!(
                    &event.body,
                    Body::Result { call: of, .. } | Body::Refusal { call: of, .. } if *of == call
                )
        })
    };
    for event in &returned {
        for call in events.iter().filter(|call| {
            call.seq < event.seq
                && call.agent == event.agent
                && matches!(call.body, Body::Call { .. })
        }) {
            assert!(
                answered(call.seq, event.seq),
                "an answer was folded in at {} while the call at {} was still \
                 unanswered",
                event.seq,
                call.seq
            );
        }
    }
    // And they ran at once: one child's first request went out before the
    // other's last completion came back.
    let children = sub_agents(&Replay::over(events.clone()));
    let span = |agent: &AgentId| {
        let mine: Vec<u64> = events
            .iter()
            .filter(|event| &event.agent == agent)
            .filter(|event| matches!(event.body, Body::Assembly { .. } | Body::Completion { .. }))
            .map(|event| event.seq)
            .collect();
        (mine[0], *mine.last().expect("a completion"))
    };
    let overlapped = children.windows(2).any(|pair| {
        let (a, b) = (span(&pair[0].0), span(&pair[1].0));
        a.0 < b.1 && b.0 < a.1
    });
    println!(
        "afforded: {} sub-agents, {} folded in, overlapping {overlapped}, and the \
         parent said {:?}",
        children.len(),
        returned.len(),
        answer.text.trim()
    );
    assert!(
        overlapped,
        "two sub-agents were admitted and ran one after the other: {children:?}"
    );
    assert!(
        answer.text.contains(WINCH.code) && answer.text.contains(HATCH.code),
        "both answers were folded in and the parent said: {}",
        answer.text
    );
    chat.shutdown().await;
}

/// **Parallelism, refused.** The reference model at parallelism 2 opens one
/// slot, queues the second, and the number shown is the number given.
///
/// Through the build's own pool, `Pool::on_the_card`, which is what a window
/// runs. It queues for the reason it states. Then the arithmetic the reason
/// will become once #74 measures a slot is taken on the card as it stands with
/// the reference model resident, to show that it refuses on room as well.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn parallelism_the_card_cannot_afford_queues_and_says_so() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Reference;
    let rig = Rig::new("refused", tier);
    rig.plant("bay4.txt", WINCH.sentence);
    rig.set(id::PARALLEL_AGENTS, json!(2));
    let chat = rig.chat("autonomous");
    let presence = loaded(&chat, tier).await;

    let Presence::Ready { slots, limit, .. } = &presence else {
        unreachable!("loaded is Ready")
    };
    assert_eq!(*slots, 1, "the reference model opened {slots} slots");
    // The reason the build states today is that nothing has measured a slot
    // (#74), and it is asserted as that rather than as any reason at all: the
    // day it becomes room, this line changes on purpose.
    assert_eq!(
        *limit,
        Some(Queued::Unmeasured),
        "one slot opened out of two asked for, for the wrong stated reason"
    );

    // The number shown is the one `llama.cpp` reports, not the setting.
    let backend = rig.supervisor.current().await.expect("a running backend");
    assert_eq!(
        demido_inference::Backend::slots(&*backend)
            .await
            .expect("slots"),
        *slots,
        "the presence shows a slot count the server does not have"
    );

    // And on room, with the model resident, it would refuse too.
    let free = demido_vram::free_now().expect("the card reads").free;
    let admission = demido_vram::admit(demido_vram::Budget {
        free,
        per_slot: REFERENCE_PER_SLOT,
        open: 1,
        wanted: 2,
    });
    println!(
        "refused: {slots} slot, limit {limit:?}; with the reference model \
         resident the card has {} MiB free against {} MiB a slot costs, so on \
         room it would open {} and give {:?}",
        free / MIB,
        REFERENCE_PER_SLOT / MIB,
        admission.open,
        admission.reason
    );
    assert_eq!(admission.open, 1);
    assert!(matches!(admission.reason, Some(Queued::NoRoom { .. })));

    // A delegation on one slot is the blocking path: its answer is the call's
    // own result, and nothing is folded in afterwards.
    let answer = chat
        .ask(
            "Use the delegate_task tool to have a sub-agent read bay4.txt and \
             report the winch code. Then tell me the code.",
            |_| {},
            nobody(),
        )
        .await
        .expect("an answer");
    let events = rig.events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event.body, Body::Delegated { .. })),
        "the reference model never delegated"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.body, Body::Returned { .. })),
        "a queued delegation was folded in as if it had run in the background"
    );
    println!("refused: the parent said {:?}", answer.text.trim());
    chat.shutdown().await;
}

/// **The child's rebuild.** The exact assembly sent to the sub-agent, offered
/// set and tool wording included, byte for byte.
///
/// The run is the returning delegation again, on the development model. The
/// log is then read back from disk, and every request on it, the parent's and
/// the child's, is rebuilt and compared against what `llama.cpp` received. It
/// also writes the fixture `done.md` asks a closing comment for.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_log_rebuilds_the_exact_assembly_a_sub_agent_was_sent() {
    returns(Tier::Development, true).await;
}

/// Compare the log against what the backend received, then commit it.
fn keep(rig: &Rig, name: &str) {
    let replay = Replay::over(JsonLines::read(rig.log()).expect("read the log"));
    let sent = sent_by(&replay);

    // What the person said to the conversation, which a clean context never
    // hands a sub-agent.
    let question = sent
        .first()
        .and_then(|(_, _, request)| {
            request
                .messages
                .iter()
                .find(|message| message.role == Role::User)
        })
        .map(|message| message.content.clone())
        .expect("the conversation's first request carries the question");

    let mut children = 0;
    for (agent, seq, request) in &sent {
        let rebuilt = replay.request(*seq).expect("rebuilt");
        assert!(
            !rebuilt.tools.is_empty(),
            "the rebuild of {agent:?}'s step {seq} carries no tools, so the \
             descriptions the model read are not in the log"
        );
        assert_eq!(
            serde_json::to_string(&rebuilt).expect("a request"),
            serde_json::to_string(request).expect("a request"),
            "the log rebuilt a different request from the one {agent:?} sent at \
             step {seq}"
        );
        if *agent != AgentId::main() {
            children += 1;
            // The child's own context: its task, never the person's question.
            assert!(
                request
                    .messages
                    .iter()
                    .all(|message| !message.content.contains(&question)),
                "the sub-agent's request at {seq} carries the conversation"
            );
            // And the offered set it inherited, which at the default depth of 2
            // still holds `delegate_task` one level down.
            assert!(
                offered(request).contains(&"read_file")
                    && offered(request).contains(&"delegate_task"),
                "the sub-agent at {seq} was offered {:?}",
                offered(request)
            );
        }
    }
    assert!(children > 0, "no sub-agent request was sent to rebuild");

    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::create_dir_all(&fixtures).expect("made the fixtures directory");
    let log = fixtures.join(format!("{name}.jsonl"));
    prune(&rig.log(), &log, &rig.project);
    let requests: Vec<&Request> = sent.iter().map(|(_, _, request)| request).collect();
    std::fs::write(
        fixtures.join(format!("{name}.sent.json")),
        scrub(
            &serde_json::to_string_pretty(&requests).expect("the requests"),
            &rig.project,
        ),
    )
    .expect("kept the requests");
    println!("trace fixture: {}", log.display());
}

/// **Stop.** A cancel mid-delegation leaves nothing generating, at depth 2.
///
/// The stop is pressed the moment a grandchild is generating, read off the log
/// the way the monitor reads it. Afterwards no agent on the log has a request
/// out, and the model answers the next message at once: a grandchild still
/// generating on the only slot would make it wait.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_stop_mid_delegation_leaves_nothing_generating_at_depth_two() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("stop", tier);
    rig.plant(
        "bay4.txt",
        &format!("{}{}", filler("Bay four", 30), WINCH.sentence),
    );
    rig.set(id::DELEGATION_DEPTH, json!(2));
    let chat = rig.chat("autonomous");
    loaded(&chat, tier).await;

    let asking = chat.ask(
        "Use the delegate_task tool to have a sub-agent find the winch code in \
         bay4.txt. In the task, tell that sub-agent to hand the reading on to a \
         sub-agent of its own with delegate_task, and to have it write a long, \
         careful summary of every line of the file before giving the code.",
        |_| {},
        nobody(),
    );
    let pressing = async {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let replay = Replay::over(JsonLines::read(rig.log()).unwrap_or_default());
            let deep = replay
                .agents()
                .into_iter()
                .find(|agent| agent.depth == 2 && agent.generating);
            if let Some(agent) = deep {
                // Into its generation, so the stop lands on a stream rather than
                // on a request that has not been sent.
                tokio::time::sleep(Duration::from_millis(500)).await;
                assert!(chat.stop(), "nothing was running to stop");
                return agent.agent;
            }
        }
    };
    let (ended, stopped) = tokio::join!(
        async { tokio::time::timeout(Duration::from_secs(600), asking).await },
        async { tokio::time::timeout(Duration::from_secs(600), pressing).await },
    );
    let stopped = stopped.expect(
        "no grandchild was ever generating: the chain never reached depth 2 in \
         ten minutes",
    );
    let ended = ended.expect("the turn did not end after the stop");
    println!("stop: the turn ended as {ended:?}");

    let replay = rig.replay();
    let generating: Vec<AgentId> = replay
        .agents()
        .into_iter()
        .filter(|agent| agent.generating)
        .map(|agent| agent.agent)
        .collect();
    assert!(
        generating.is_empty(),
        "after a stop at depth 2 these agents still have a request out: \
         {generating:?}"
    );
    let cancelled = replay
        .events()
        .iter()
        .filter(|event| event.agent == stopped)
        .any(|event| {
            matches!(
                &event.body,
                Body::Completion {
                    reason: FinishReason::Cancelled,
                    ..
                }
            )
        });
    assert!(
        cancelled,
        "the grandchild's generation did not end as cancelled"
    );

    // Nothing left on the slot: the next message is answered, and promptly.
    let next = tokio::time::timeout(
        Duration::from_secs(60),
        chat.ask("Say the word ready.", |_| {}, nobody()),
    )
    .await
    .expect("the next message waited, so something was still generating")
    .expect("an answer after the stop");
    assert!(!next.text.trim().is_empty());
    chat.shutdown().await;
}
