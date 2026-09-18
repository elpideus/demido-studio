//! The live-model suite for S2: a real model is shown Demido's tools, in
//! Demido's words, and decides what to do about them.
//!
//! This is the model gate of
//! [`docs/rules/done.md`](../../../../docs/rules/done.md) for
//! [#59](https://github.com/elpideus/demido-studio/issues/59), the ticket that
//! closes [#35](https://github.com/elpideus/demido-studio/issues/35). It runs
//! from a terminal, with no window and nobody at the keyboard, it holds one
//! model resident by the same process-wide permit the other live suites use,
//! and it is re-run every slice from here on.
//!
//! `demido-chat/tests/a_real_model.rs` is S1's, a model answering with nothing
//! on offer. This is the other half: what a model *does* when six tools are in
//! the payload and nothing in the message names one.
//!
//! ## The bar is `chose`
//!
//! S1 was `used`, because there was nothing for a model to choose. Here the
//! planted file names a file and never a tool, and the model picks `read_file`
//! out of six; on the denial it picks something other than the call it was just
//! refused. That is the first evidence this repo has for or against the brief's
//! own claim:
//!
//! > By properly "guiding" the llms, in a smart way that also does not consume
//! > too much context, it is possible to make even smaller models [...] behave
//! > "properly".
//!
//! ## Unguessable, and asserted to be
//!
//! The planted answer is a string no model has ever seen and nothing else in
//! this repo contains. A scenario that asked for the capital of France with a
//! file saying "Paris" in it would pass on a model that never called anything,
//! which is the agreeable-model failure the whole slice is about. So the
//! assertion is made in two halves: the answer has to carry the planted string,
//! **and** the planted string has to be absent from the first request the
//! backend was handed, which is the payload as it stood before any tool ran.
//! There is nowhere else for it to have come from.
//!
//! ## Which tiers run what
//!
//! The planted file and the greeting run on all three, because both are claims
//! about what a model does. The rest run on the development tier, because what
//! they assert is Demido's: that an approval reaches the disk, that a denial is
//! not a retry, that a switched-off tool is absent from the payload, that an
//! exit status and not stderr decides a failure, and that the log rebuilds what
//! was sent. A second tier would exercise the same lines with slower weights.
//!
//! **A red on the breadth model alone is a note, not a defect**, and after
//! [#19](https://github.com/elpideus/demido-studio/issues/19)'s three probes out
//! of three it is surprising enough to look at twice before it is written off as
//! the quant.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

// The rig is the inference crate's, included rather than copied. Two copies of
// where the models are is two places for a path to go stale.
#[path = "../../demido-inference/tests/rig.rs"]
mod rig;

// `Watching` and the prune are shared with the S4 suite, for the same reason.
#[path = "support/live.rs"]
mod live;

use std::future::{ready, Ready};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::json;

use demido_chat::{Asking, Chat, Decision, Delegations, Model, Moment, Outcome, Presence, Toolbox};
use demido_inference::{Request, Role, Supervisor};
use demido_prompts::Tools;
use demido_settings::{id, Memory as SettingsMemory, Scope, Settings};
use demido_tools::{files, shell, Registry, Workspace};
use demido_trace::{Body, Event, JsonLines};

use live::Watching;
use rig::Tier;

/// What was planted, and the part of it that cannot have come from anywhere
/// else.
///
/// Two unrelated words and a number, in a sentence about a thing that does not
/// exist. It is not in the weights, it is not in this repo outside this file and
/// the file the scenario writes, and it is not in the question. The scenario
/// asserts the last of those against the payload rather than claiming it.
const PLANTED: &str = "The winch code for bay four is 7731-MARGATE-OXIDE.\n";
const UNGUESSABLE: &str = "7731-MARGATE-OXIDE";

// --- the rig ----------------------------------------------------------------

/// A project with something planted in it, a log on disk, and the ladder.
struct Rig {
    /// The workspace the tools may act in. A real directory rather than a
    /// `tempfile` one, because the fixture this run writes must carry no path
    /// from this machine and a named directory is one that can be checked for.
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
            .join("demido-chat-tools-live")
            .join(format!("{name}-{}-{}", tier.label(), std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let project = dir.join("project");
        std::fs::create_dir_all(&project).expect("made the project");
        std::fs::create_dir_all(dir.join("prompts")).expect("made the prompts directory");

        Watching::forget();
        Self {
            project,
            settings: Arc::new(Settings::open(SettingsMemory::new())),
            supervisor: Arc::new(Supervisor::new()),
            session: format!("{name}-{}", tier.label()),
            tier,
            dir,
        }
    }

    fn plant(&self, name: &str, content: &str) {
        std::fs::write(self.path(name), content).expect("planted the file");
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.join(name)
    }

    fn set(&self, id: &str, value: serde_json::Value) {
        self.settings
            .set(&Scope::chat(&self.session), id, &value)
            .expect("set on this chat");
    }

    /// A chat over the Files and Shell groups, in `mode`, logging to a file.
    fn chat(&self, mode: &str) -> Chat<Watching, JsonLines> {
        self.set(id::TOOLS_MODE, json!(mode));
        let registry = Registry::open(Some(Workspace::open(&self.project).expect("a workspace")))
            .with_group(files())
            .with_group(shell());
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
            Delegations::none(),
        )
    }

    fn log(&self) -> PathBuf {
        self.dir.join("session.jsonl")
    }

    fn events(&self) -> Vec<Event> {
        JsonLines::read(self.log()).expect("read the log")
    }
}

/// Load, or say which tier failed and what the backend said about it.
async fn loaded(chat: &Chat<Watching, JsonLines>, tier: Tier) {
    match chat.load(|_| {}).await {
        Presence::Ready { .. } => {}
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

/// Somebody at the window who answers `decision` every time, and remembers what
/// they were shown.
#[derive(Clone, Default)]
struct Person(Arc<Mutex<Vec<Asking>>>);

impl Person {
    fn answering(&self, decision: Decision) -> impl FnMut(Asking) -> Ready<Decision> + Send {
        let shown = self.0.clone();
        move |asking| {
            shown
                .lock()
                .unwrap_or_else(|held| held.into_inner())
                .push(asking);
            ready(decision)
        }
    }

    fn asked(&self) -> Vec<Asking> {
        self.0
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }
}

/// Every call the turn made, as the transcript draws them.
fn calls(chat: &Chat<Watching, JsonLines>) -> Vec<demido_chat::Called> {
    chat.transcript()
        .expect("a transcript")
        .into_iter()
        .filter_map(|moment| match moment {
            Moment::Called(called) => Some(called),
            Moment::Said(_) | Moment::Delegated(_) => None,
        })
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

// --- the scenarios ----------------------------------------------------------

/// **The planted file.** The slice's own sentence, on every tier.
///
/// A file nobody has read, a question that names the file and no tool, and an
/// answer that carries something the model cannot have known. Six tools are on
/// offer and `read_file` is one of them: this is `Bar: chose`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_planted_file_is_read_and_the_answer_comes_out_of_it() {
    for tier in Tier::ALL {
        planted(tier, false).await;
    }
}

async fn planted(tier: Tier, keep_the_fixture: bool) {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let rig = Rig::new("planted", tier);
    rig.plant("winch.txt", PLANTED);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    let answer = chat
        .ask(
            "The winch code for bay four is written in winch.txt. What is it? \
             Answer with the code alone.",
            |_| {},
            // Reading is Allow in every row of the matrix, so a model that
            // chose the right tool never reaches a person. One that chose to
            // write or to run a command does, and that is a failure of this
            // scenario rather than a question for anybody.
            nobody(),
        )
        .await
        .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));

    let sent = Watching::sent();
    assert_eq!(
        offered(&sent[0]),
        [
            "read_file",
            "list_directory",
            "search_files",
            "write_file",
            "delete_file",
            "run_command"
        ],
        "the {} model was shown the wrong set",
        tier.label()
    );

    // **The unguessability, asserted rather than claimed.** The payload as it
    // stood before anything ran carries the question, the tool descriptions and
    // nothing else, so a code in the answer came off the disk.
    let before_anything_ran = serde_json::to_string(&sent[0]).expect("a request");
    assert!(
        !before_anything_ran.contains(UNGUESSABLE),
        "the planted code was already in the first payload, so this scenario \
         proves nothing: {before_anything_ran}"
    );

    assert!(
        answer.text.contains(UNGUESSABLE),
        "the {} model was asked for a code that exists only in a file it was \
         not given, and said: {}",
        tier.label(),
        answer.text
    );

    // It chose, and what it chose brought the code back. **Which** tool is
    // deliberately not asserted: the breadth model reached for `search_files`
    // with the word from the question, got the line, and answered from it, which
    // is a model choosing well rather than a model choosing differently from the
    // other two. What has to be true is that a call it picked out of six, with
    // nothing in the message naming one, is where the code came from.
    let called = calls(&chat);
    let fetched: Vec<&str> = called
        .iter()
        .filter(|call| {
            matches!(
                &call.outcome,
                Some(Outcome::Returned { text, failed: false }) if text.contains(UNGUESSABLE)
            )
        })
        .map(|call| call.name.as_str())
        .collect();
    assert!(
        !fetched.is_empty(),
        "the {} model answered with the code and no call in the turn brought it \
         back, which is an answer from somewhere this scenario cannot account \
         for: {called:?}",
        tier.label()
    );
    println!(
        "the {} model chose {fetched:?} out of six, with nothing in the message \
         naming a tool",
        tier.label()
    );

    if keep_the_fixture {
        keep(&rig, &sent, "a-planted-file");
    }
    chat.shutdown().await;
}

/// **The greeting.** The full registry, "hello", and no tool call.
///
/// The cost of there being no Chat mode
/// ([#20](https://github.com/elpideus/demido-studio/issues/20)), measured rather
/// than assumed, on all three tiers. A tier that fails this is fixed by widening
/// `tools.shown` or by the user switching a group off in the picker, and neither
/// is a re-decision.
///
/// ## The allowance, and what it costs to use
///
/// `done.md` lets a red on the breadth model alone stand as a note. It also says
/// the note only holds "unless it reproduces at Q4_K_M", and #59's own line is
/// that a breadth red is now surprising enough to be looked at twice before it
/// is written off as the quant. So this scenario does not fail on breadth and
/// does not shrug at it either: every tier is run, the three are reported
/// together, and a breadth-only red sends the same greeting to the secondary
/// model at Q4_K_M. If it reproduces there, the quant is not the explanation
/// and the scenario fails.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_greeting_with_every_tool_on_offer_is_answered_and_calls_nothing() {
    let mut reached: Vec<Tier> = Vec::new();
    for tier in Tier::ALL {
        if !greeted(tier).await {
            reached.push(tier);
        }
    }

    match reached.as_slice() {
        [] => {}
        // The one case the standing allowance covers, and it is checked rather
        // than assumed. Q4_K_M on a 9B is not 2.9 bits on a dense 27B, so a
        // model that keeps its hands off the tools there says the quant is the
        // difference.
        [Tier::Breadth] => {
            assert!(
                greeted(Tier::Secondary).await,
                "the breadth model reached for a tool on a greeting and so did \
                 the secondary model at Q4_K_M, so this is not the quant: it is \
                 the registry being too wide for a greeting, and the fix is \
                 tools.shown or the picker (#20)"
            );
            println!(
                "note: the breadth model called a tool on a greeting and the \
                 secondary model at Q4_K_M did not, so it is recorded as the \
                 quant and nothing is changed (docs/rules/done.md)"
            );
        }
        other => panic!(
            "these tiers reached for a tool on a greeting: {:?}. A red anywhere \
             but breadth is a defect rather than a note",
            other.iter().map(|tier| tier.label()).collect::<Vec<_>>()
        ),
    }
}

/// Greet one tier with the whole registry on offer. `true` when it answered and
/// called nothing.
async fn greeted(tier: Tier) -> bool {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let rig = Rig::new("greeting", tier);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    // Not `nobody()`. Cautious asks about a shell call, and a greeting that
    // reached for one would then panic inside the callback, which is a scenario
    // that cannot report the thing it exists to measure. The person says no, and
    // what they were shown is the finding.
    let person = Person::default();
    let answer = chat
        .ask("hello", |_| {}, person.answering(Decision::Deny))
        .await
        .unwrap_or_else(|error| panic!("the {} model did not answer: {error}", tier.label()));

    assert_eq!(
        offered(&Watching::sent()[0]).len(),
        6,
        "the whole registry was on offer for this to mean anything"
    );
    assert!(
        !answer.text.trim().is_empty(),
        "the {} model was greeted and said nothing at all",
        tier.label()
    );

    let called = calls(&chat);
    if !called.is_empty() {
        println!(
            "the {} model was greeted with six tools on offer and reached for: \
             {:?}",
            tier.label(),
            called.iter().map(|call| &call.name).collect::<Vec<_>>()
        );
    }
    chat.shutdown().await;
    called.is_empty()
}

/// **The approval.** Cautious, a write, approved, and the write happened.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn an_approved_write_reaches_the_disk() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("approved", tier);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    let person = Person::default();
    chat.ask(
        "Save the single word shipped into a file called plan.txt.",
        |_| {},
        person.answering(Decision::Allow),
    )
    .await
    .expect("an answer");

    let asked = person.asked();
    assert!(
        asked.iter().any(|asking| asking.tool == "write_file"),
        "Cautious asks about a write, and the person was shown {asked:?}"
    );
    let written =
        std::fs::read_to_string(rig.path("plan.txt")).expect("the approved write reached the disk");
    assert!(
        written.to_lowercase().contains("shipped"),
        "the file the person approved holds: {written}"
    );

    chat.shutdown().await;
}

/// **The denial.** Cautious, a call, denied, and the model does something else.
///
/// The path most likely to be broken and least likely to be exercised, because a
/// small model handed a refusal typically retries the same call forever.
///
/// ## What is asserted, and what is only reported
///
/// **Asserted, every run**: the person is asked once and not once per retry;
/// nothing is written; the turn ends with the model saying something; and the
/// declined call is never made a **third** time. The third is the loop, and
/// breaking it is Demido's job.
///
/// **Reported, not asserted**: whether the model made the declined call a second
/// time before it gave up. That is the model's, and on the development tier it
/// happened in two runs out of ten. Asserting it would make a suite that is
/// re-run every slice red a quarter of the time for a thing no line of this
/// repo controls, and quietly widening the assertion until it passed would be
/// the gate weakening itself, which is the failure `done.md` exists to prevent.
/// So it is printed with the wording the model was given, on every run, and the
/// rate is in #59's closing comment.
///
/// `Bar: chose` rests on where the turn ends up: something other than the call
/// the person refused. The rate above is what that costs today.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_denied_call_is_not_retried_and_the_turn_ends_in_an_answer() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("denied", tier);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    let person = Person::default();
    let answer = chat
        .ask(
            "Save the single word shipped into a file called plan.txt.",
            |_| {},
            person.answering(Decision::Deny),
        )
        .await
        .expect("a denial is not an error");

    assert_eq!(
        person.asked().len(),
        1,
        "one decision, not one per retry: the person was shown {:?}",
        person.asked()
    );
    assert!(
        !rig.path("plan.txt").exists(),
        "a denied call wrote the file anyway"
    );
    assert!(
        !answer.text.trim().is_empty(),
        "the turn ended without the model saying anything about the refusal"
    );

    let called = calls(&chat);
    let denied = called
        .iter()
        .find(|call| matches!(&call.outcome, Some(Outcome::Refused { .. })))
        .expect("the denial is in the transcript");
    let again: Vec<&demido_chat::Called> = called
        .iter()
        .filter(|call| {
            !std::ptr::eq(*call, denied)
                && call.name == denied.name
                && call.arguments == denied.arguments
        })
        .collect();

    // The loop is broken, whatever the model tried. The first repeat is
    // answered in its own words and the tools come off the turn; a second one
    // would mean neither of those worked.
    assert!(
        again.len() < 2,
        "the declined call was made {} more times, so nothing broke the loop:          {called:?}",
        again.len()
    );
    if let [repeat] = again.as_slice() {
        println!(
            "note: the {} model made the declined call once more before it gave              up, and was told: {}",
            tier.label(),
            match &repeat.outcome {
                Some(Outcome::Refused { text, .. }) => text.trim(),
                other => panic!("a repeat that was not refused: {other:?}"),
            }
        );
    }

    chat.shutdown().await;
}

/// **The absent tool.** A tool switched off in the picker is not in the payload,
/// and a model that names it anyway is told the user turned it off.
///
/// ## The tool is switched off between two messages, on purpose
///
/// A model shown three tools does not usually reach for a fourth it has never
/// heard of, and the first run of this scenario measured exactly that: the
/// payload half was exercised and the wording half was not, because the
/// development model behaved. So the shape is the one a person actually
/// produces. The first message runs `run_command` with everything on offer;
/// then the picker closes the Shell group; then the same request is made again.
/// A model with its own successful call to that tool in the transcript reaches
/// for it a second time, which is the case the wording exists for.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_tool_switched_off_is_absent_from_the_payload_and_named_as_switched_off() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("switched-off", tier);
    // Autonomous, so that a call to a tool that *was* offered would simply run:
    // nothing here should be waiting on a person.
    let chat = rig.chat("autonomous");
    loaded(&chat, tier).await;

    chat.ask(
        "Run the command `echo one` with the run_command tool and tell me what \
         it printed.",
        |_| {},
        nobody(),
    )
    .await
    .expect("an answer");
    assert!(
        calls(&chat).iter().any(|call| call.name == "run_command"
            && matches!(&call.outcome, Some(Outcome::Returned { failed: false, .. }))),
        "the {} model never ran the command this scenario then takes away from \
         it: {:?}",
        tier.label(),
        calls(&chat)
    );

    // Somebody switches the Shell group off in the picker, between two
    // messages. Everything below is about the next one.
    rig.set(
        id::TOOLS_OFFERED,
        json!(["read_file", "list_directory", "search_files"]),
    );
    Watching::forget();

    // **The step limit is a legal end to this turn, and it is the one the
    // development model reaches.** A model that has just used a tool
    // successfully goes on naming it from its own transcript, whether or not
    // the payload still has it and whether or not the tools were withheld after
    // the first refusal: three runs, eight steps each, every one of them
    // `run_command`. The limit ending it is the limit doing its job.
    //
    // What this scenario asserts is Demido's half, and both halves of it hold in
    // either ending: the tool is out of the payload, and every call to it is
    // answered with who turned it off and runs nothing.
    let ended = chat
        .ask(
            "Now run the command `echo two` the same way.",
            |_| {},
            nobody(),
        )
        .await;
    match &ended {
        Ok(answer) => assert!(
            !answer.text.trim().is_empty(),
            "the model said nothing about a tool it could not have"
        ),
        Err(demido_chat::Error::StepLimit { .. }) => println!(
            "note: the {} model used every step of the turn without answering. \
             The calls in this chat, across both turns, were {:?}. The limit is \
             what that is for, and this is recorded rather than asserted away",
            tier.label(),
            calls(&chat)
                .iter()
                .map(|call| call.name.clone())
                .collect::<Vec<_>>()
        ),
        Err(error) => panic!("the turn failed for something else: {error}"),
    }

    // Absent, not held back and not mentioned.
    let sent = Watching::sent();
    assert_eq!(
        offered(&sent[0]),
        ["read_file", "list_directory", "search_files"]
    );
    let payload = serde_json::to_string(&sent[0]).expect("a request");
    assert!(
        !payload.contains("\"description\":\"Run a shell command"),
        "the switched-off tool is in the payload: {payload}"
    );

    // Nothing ran on the switched-off tool, whatever the model decided to do
    // instead. This half holds in every run.
    //
    // Asked of the `run_command` results rather than of every result: told it
    // cannot run a command, the model sometimes reaches for `read_file` on a
    // file named after the command's own words, and a search of every result
    // for "two" then finds "two.txt does not exist." and reports the switched
    // off tool as having run.
    assert_eq!(
        commands(&rig)
            .iter()
            .filter(|(arguments, _)| arguments.contains("two"))
            .count(),
        0,
        "a call to a switched-off tool ran: {:?}",
        commands(&rig)
    );

    // **And if it named the tool anyway, it was told who closed it.**
    //
    // Conditional, and it stays conditional: the ticket's sentence is about "a
    // model that names it", and a live scenario cannot make one misbehave to
    // order. Priming it with a successful call to the same tool one message
    // earlier is as close as this gets, and across four runs it produced all
    // three outcomes: eight steps of `run_command`, a clean answer, and one run
    // where it reached for `read_file` instead and found nothing there. The
    // wording is locked offline, in `demido-chat/tests/offered.rs`; what is
    // asserted here is that when it does happen live, the wording is this one.
    let named: Vec<&str> = sent
        .iter()
        .flat_map(|request| &request.messages)
        .filter(|message| message.role == Role::Tool && message.content.contains("turned `"))
        .map(|message| message.content.as_str())
        .collect();
    match named.as_slice() {
        [] => println!(
            "note: the {} model did not name the switched-off tool this run, so \
             only the payload half of this scenario was exercised live",
            tier.label()
        ),
        told => assert!(
            told.iter()
                .all(|content| content.contains("turned `run_command` off")),
            "a model naming a switched-off tool is told who turned it off, and \
             was told: {told:?}"
        ),
    }

    chat.shutdown().await;
}

/// **The failure.** A command exiting non-zero records `failed: true`; one
/// exiting zero with noisy stderr does not.
///
/// The exact command is in the message rather than planted as a script, and the
/// two together are what the first two runs of this scenario cost.
///
/// Planted as `breaks.cmd` and `noisy.cmd`, neither ran at all: this machine
/// sets `NoDefaultCurrentDirectoryInExePath`, so `cmd.exe` does not look in the
/// working directory for a program, both calls came back exit 1, and the
/// scenario recorded Demido as classifying a clean command as failed when what
/// it had classified was a command that never started. Asked for both at once,
/// the model chained them into one `breaks.cmd && noisy.cmd`, and a single call
/// naming both cannot say which of the two the exit status came from. So: one
/// command per message, and each one a thing `cmd.exe` will certainly run.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn an_exit_status_decides_a_failure_and_stderr_never_does() {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let tier = Tier::Development;
    let rig = Rig::new("failed", tier);
    let chat = rig.chat("autonomous");
    loaded(&chat, tier).await;

    // `ffmpeg` writes its banner to stderr and `curl` its progress meter, both
    // exiting zero (`docs/rules/lessons.md`). Only the first of these failed.
    // Each is found afterwards by a word that is in it and not in the other, so
    // a model that reworded the rest of the line is still findable.
    let mut ran: Vec<(&str, bool)> = Vec::new();
    for (word, command) in [
        ("wrong", "echo it went wrong 1>&2 & exit 3"),
        ("banner", "echo a banner nobody asked for 1>&2 & echo done"),
    ] {
        chat.ask(
            &format!(
                "Run this exact command with the run_command tool and tell me \
                 how it went: {command}"
            ),
            |_| {},
            nobody(),
        )
        .await
        .expect("an answer");
        ran.extend(
            commands(&rig)
                .into_iter()
                .filter(|(arguments, _)| arguments.contains(word))
                .map(|(_, failed)| (word, failed)),
        );
    }

    let failed = |word: &str| -> bool {
        let mine: Vec<bool> = ran
            .iter()
            .filter(|(asked, _)| *asked == word)
            .map(|(_, failed)| *failed)
            .collect();
        assert!(
            !mine.is_empty(),
            "the {} model never ran the command with {word} in it: {ran:?}",
            tier.label()
        );
        // Whether *any* run of it failed. A model that ran the failing command
        // twice still ran a failing command.
        mine.into_iter().any(|failed| failed)
    };
    assert!(
        failed("wrong"),
        "a command that exited three is not recorded as failed"
    );
    assert!(
        !failed("banner"),
        "a command that exited zero is recorded as failed because it wrote to \
         stderr, which is every well-behaved tool that prints a banner"
    );

    chat.shutdown().await;
}

/// Every `run_command` on the log so far, as its arguments and whether it
/// failed.
fn commands(rig: &Rig) -> Vec<(String, bool)> {
    let events = rig.events();
    events
        .iter()
        .filter_map(|event| match &event.body {
            Body::Result { call, failed, .. } => {
                let arguments = events.iter().find_map(|earlier| match &earlier.body {
                    Body::Call {
                        arguments, name, ..
                    } if earlier.seq == *call && name == "run_command" => Some(arguments.clone()),
                    _ => None,
                })?;
                Some((arguments, *failed))
            }
            _ => None,
        })
        .collect()
}

/// **The rebuild.** The trace written by the run reconstructs the exact assembly
/// that was sent, tool descriptions included, byte for byte.
///
/// The run is the planted file again, because a rebuild is only worth asserting
/// over an assembly with something in it: a question, six tool documents, a
/// call, and the file that came back. The log is then dropped, opened again from
/// disk, and asked to produce every request the backend was handed. `Watching`
/// is what makes that a comparison rather than a restatement: the right-hand
/// side is the value `llama.cpp` received, not the value the loop recorded.
///
/// It also writes the fixture `done.md` asks a closing comment for.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn the_log_of_a_turn_with_tools_rebuilds_every_assembly_that_was_sent() {
    let tier = Tier::Development;
    planted(tier, true).await;
}

/// **A tool document edited mid-session**
/// ([#77](https://github.com/elpideus/demido-studio/issues/77)).
///
/// The editor's promise, with a real model on the other end: an edit made from
/// Settings is the document the next turn offers, and a reply from before it
/// still rebuilds with the wording that produced it. The same planted file, read
/// once under the shipped `read_file`, then the document is edited through the
/// register the Settings page writes through, and a second file is read under
/// the new words.
///
/// Three things are asserted and all of them are Demido's rather than the
/// model's: every request before the edit carried the shipped document and
/// every one after it the edited one, the model still answered out of a file
/// under the new wording, and the log rebuilds every request byte for byte,
/// both wordings included. It writes the fixture `replayed.rs` reads.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and a model; see the live command in AGENTS.md"]
async fn a_tool_document_edited_mid_session_is_what_the_next_turn_offers() {
    let tier = Tier::Development;
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");

    let rig = Rig::new("edited", tier);
    rig.plant("winch.txt", PLANTED);
    rig.plant("gate.txt", GATE);
    let chat = rig.chat("cautious");
    loaded(&chat, tier).await;

    chat.ask(
        "The winch code for bay four is written in winch.txt. What is it? Answer with the code alone.",
        |_| {},
        nobody(),
    )
    .await
    .expect("the first answer");
    let before = Watching::sent().len();

    let documents = Tools::open(rig.dir.join("prompts"));
    let shipped = documents.get("read_file").expect("a declared tool");
    let edited = documents
        .set("read_file", EDITED_READ_FILE)
        .expect("an edit is never refused for what depends on it");
    assert_ne!(edited.hash, shipped.hash);

    let answer = chat
        .ask(
            "The gate code is written in gate.txt. What is it? Answer with the code alone.",
            |_| {},
            nobody(),
        )
        .await
        .expect("the second answer");

    let sent = Watching::sent();
    assert!(
        sent.len() > before,
        "the second question sent nothing after the edit"
    );
    for (step, request) in sent.iter().enumerate() {
        let Some(read) = request.tools.iter().find(|tool| tool.name == "read_file") else {
            continue;
        };
        let expected = if step < before {
            shipped.description()
        } else {
            edited.description()
        };
        assert_eq!(
            read.description, expected,
            "step {step} offered read_file in the wrong wording"
        );
    }

    assert!(
        answer.text.contains(GATE_CODE),
        "under the edited wording the {} model was asked for a code only gate.txt holds, and said: {}",
        tier.label(),
        answer.text
    );

    chat.shutdown().await;
    // Both wordings are in the log once each, and every step rebuilds as sent.
    keep(&rig, &sent, "an-edited-tool");
    println!(
        "{before} steps under {}, {} under {}",
        shipped.hash,
        sent.len() - before,
        edited.hash
    );
}

/// The second planted file, for the turn after the edit.
const GATE: &str = "The gate code for the north yard is 4410-TALLOW-QUAY.
";
const GATE_CODE: &str = "4410-TALLOW-QUAY";

/// `read_file`, reworded the way a person in the editor would: the same
/// parameters, new prose on all of them.
const EDITED_READ_FILE: &str = "Open a text file in the workspace and return it with numbered lines. The path is relative to the workspace root.

## path

Where the file is, relative to the workspace root, for example notes/todo.txt

## from_line

The first line you want, counting from 1. Leave it out to start at line 1.

## lines

How many lines you want. Leave it out to get the rest of the file.
";

/// Compare the log against what the backend received, then commit it.
fn keep(rig: &Rig, sent: &[Request], name: &str) {
    live::keep(&rig.log(), &rig.project, sent, name);
}
