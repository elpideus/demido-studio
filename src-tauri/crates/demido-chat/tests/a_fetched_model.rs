//! The live-model suite for S3: a model Demido fetched itself, in this run,
//! answering and calling a tool.
//!
//! This is the model gate of
//! [`docs/rules/done.md`](../../../../docs/rules/done.md) for
//! [#78](https://github.com/elpideus/demido-studio/issues/78), the ticket that
//! closes [#36](https://github.com/elpideus/demido-studio/issues/36), and the
//! last gate in v0.1. The brief's line:
//!
//! > Models Browser & Downloader
//!
//! ## Nothing about the model is hand-typed
//!
//! Every other live suite loads a file the rig names. This one names a
//! **repository** and nothing else. The index lists it, `choices` reads the
//! listing, the smallest quantisation is picked by the bytes the server states,
//! the queue fetches it into a download folder that did not exist when the run
//! started, and the path a backend is handed is the one the library offers on
//! its next scan. There is no path to the weights anywhere in this file.
//!
//! ## What `chose` means here
//!
//! A downloader is not something a model reaches for, so the bar is carried by
//! what the fetched model does next: asked for something only a file holds,
//! with nothing in the message naming a tool, it picks one out of six through
//! S2's registry. That is where `Bar: chose` is earned.
//!
//! ## Which tiers run what
//!
//! The answer and the tool call run on all three, at the smallest quantisation
//! each repository publishes, because both are claims about what a model does
//! and the smallest is what makes fetching three of them affordable. The rest
//! are claims about Demido (the projector's arrival, the borrowed folder, the
//! rebuild across an edit) and run on the development tier.
//!
//! **A red on the breadth model alone is a note, not a defect**, and it is
//! examined before it is written off: the same question goes to the secondary
//! model at Q4_K_M, and a red there too is a defect.
//!
//! ## Cost
//!
//! Roughly 25 GB the first time a tier is asked for in a process, cached for
//! the rest of that process and deleted by the next run's first fetch.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

#[path = "../../demido-inference/tests/rig.rs"]
mod rig;

#[path = "support/live.rs"]
mod live;

use std::collections::BTreeMap;
use std::future::{ready, Ready};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde_json::json;
use tokio::sync::OnceCell;

use demido_chat::{Asking, Chat, Decision, Delegations, Model, Moment, Outcome, Presence, Toolbox};
use demido_download::{Files, Item, Queue, State, HOST};
use demido_inference::{LlamaCppConfig, Request, Supervisor};
use demido_models::index::{Answer, Index};
use demido_models::{choices, Fact, Folders, Library, Local, Part};
use demido_prompts::Tools;
use demido_settings::{id, Memory as SettingsMemory, Scope, Settings};
use demido_tools::{files, shell, Registry, Workspace};
use demido_trace::JsonLines;

use live::{keep, rebuilds, Watching};
use rig::Tier;

/// What is planted, and the part of it nothing else can have supplied. New
/// words rather than S2's, so a model that met S2's code in some cache of an
/// earlier run has nothing to remember.
const PLANTED: &str = "The ferry manifest seal for pier nine is 5082-CORMORANT-BASALT.\n";
const UNGUESSABLE: &str = "5082-CORMORANT-BASALT";

/// The question: it names the file and never a tool.
const ASKED: &str = "The ferry manifest seal for pier nine is written in manifest.txt. \
                     What is it? Answer with the seal alone.";

/// Asked of a model with nothing on offer. Arithmetic, so the answer is
/// checkable and short, and a model that loads and says nothing, or says
/// something unrelated, is caught.
const SUM: &str = "What is 17 plus 25? Answer with the number alone.";

// --- the fetch --------------------------------------------------------------

/// A model the queue fetched in this process, as the library offers it.
struct Fetched {
    /// What the queue was asked for: every piece and the projector, as one item.
    item: Item,
    /// What the library offered afterwards. `local.path` is the only path to
    /// the weights this suite ever holds.
    local: Local,
    /// The folders the library was read from.
    folders: Folders,
}

static FETCHED: [OnceCell<Fetched>; 3] = [
    OnceCell::const_new(),
    OnceCell::const_new(),
    OnceCell::const_new(),
];

/// The model `tier`'s repository publishes at its smallest, fetched once per
/// process and shared by every scenario that asks for it.
async fn fetched(tier: Tier) -> &'static Fetched {
    let at = Tier::ALL
        .iter()
        .position(|each| *each == tier)
        .expect("one of the three");
    FETCHED[at].get_or_init(|| fetch(tier)).await
}

/// Where this run's downloads land: a directory of its own, with every earlier
/// run's deleted the first time it is asked for, so a machine keeps one run's
/// worth of weights rather than one per run.
fn scratch() -> PathBuf {
    static CLEARED: std::sync::Once = std::sync::Once::new();
    let root = std::env::temp_dir().join("demido-fetched-live");
    CLEARED.call_once(|| {
        let _ = std::fs::remove_dir_all(&root);
    });
    let dir = root.join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).expect("made the scratch directory");
    dir
}

async fn fetch(tier: Tier) -> Fetched {
    let repo = tier.repo();
    let root = scratch().join(tier.label());
    let folders = Folders {
        download: root.join("models"),
        scan: vec![],
    };
    let library = Library::open(&folders);
    assert!(
        library.scan().models.is_empty(),
        "the download folder had a model in it before anything was fetched"
    );

    let Answer::Read { found } = Index::default().files(repo).await else {
        panic!("the index could not list {repo}");
    };
    let offered = choices(found);
    // The smallest quantisation is the fewest weight bytes. The projector is
    // the same file whichever quantisation it rides with, so it does not
    // decide which one is smallest.
    let choice = offered
        .iter()
        .min_by_key(|choice| choice.weights)
        .unwrap_or_else(|| panic!("{repo} offers nothing to choose"));
    let item = Item::chosen(HOST, repo, choice, &library);
    println!(
        "{}: fetching {} from {repo}, {} MiB in {} file(s)",
        tier.label(),
        choice.name,
        item.bytes() / (1024 * 1024),
        item.files.len()
    );

    let queue = Queue::open(Files::in_profile(&root));
    let id = queue.enqueue(item.clone());
    let started = std::time::Instant::now();
    let state = tokio::time::timeout(Duration::from_secs(60 * 60), queue.settled(id))
        .await
        .unwrap_or_else(|_| panic!("{repo} did not arrive within the hour"));
    assert_eq!(
        state,
        Some(State::Done),
        "the queue did not finish {}",
        choice.name
    );
    println!(
        "{}: arrived and verified in {}s",
        tier.label(),
        started.elapsed().as_secs()
    );

    // S1's path starts from what the library offers, not from where the queue
    // said it put things. A download the library does not list is one nobody
    // could choose.
    let scan = library.scan();
    assert!(scan.damaged.is_empty(), "damaged: {:?}", scan.damaged);
    let local = scan
        .models
        .into_iter()
        .find(|model| model.repo.as_deref() == Some(repo))
        .unwrap_or_else(|| panic!("the library does not offer what was fetched from {repo}"));
    assert!(!local.borrowed, "a download landed somewhere borrowed");

    Fetched {
        item,
        local,
        folders,
    }
}

// --- the rig ----------------------------------------------------------------

/// A project with something planted in it, a log on disk, the ladder, and a
/// model that is not necessarily the rig's.
struct Rig {
    project: PathBuf,
    dir: PathBuf,
    settings: Arc<Settings>,
    supervisor: Arc<Supervisor<Watching>>,
    session: String,
    config: LlamaCppConfig,
    tier: Tier,
}

impl Rig {
    fn new(name: &str, tier: Tier, config: LlamaCppConfig) -> Self {
        let dir = std::env::temp_dir()
            .join("demido-chat-fetched-live")
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
            config,
            tier,
            dir,
        }
    }

    /// The model `tier` fetched in this run, and nothing else.
    async fn fetched(name: &str, tier: Tier) -> Self {
        let fetched = fetched(tier).await;
        Self::new(
            name,
            tier,
            rig::configured(fetched.local.path.clone(), tier),
        )
    }

    fn plant(&self, name: &str, content: &str) {
        std::fs::write(self.project.join(name), content).expect("planted the file");
    }

    /// A chat in Cautious, over the Files and Shell groups when `tools`, and
    /// over nothing otherwise.
    fn chat(&self, tools: bool) -> Chat<Watching, JsonLines> {
        self.settings
            .set(
                &Scope::chat(&self.session),
                id::TOOLS_MODE,
                &json!("cautious"),
            )
            .expect("set on this chat");
        let registry = if tools {
            Registry::open(Some(Workspace::open(&self.project).expect("a workspace")))
                .with_group(files())
                .with_group(shell())
        } else {
            Registry::open(None)
        };
        let path = self.log();
        Chat::new(
            self.session.as_str(),
            move || JsonLines::open(&path),
            self.supervisor.clone(),
            Some(Model {
                config: self.config.clone(),
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
}

async fn loaded(chat: &Chat<Watching, JsonLines>, tier: Tier) {
    match chat.load(|_| {}).await {
        Presence::Ready { .. } => {}
        other => panic!("the {} model did not load: {other:?}", tier.label()),
    }
}

/// Nobody is at the window, and in these scenarios nobody should need to be.
fn nobody() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |asking: Asking| {
        panic!(
            "nobody should have been asked about {}, and was asked anyway",
            asking.tool
        )
    }
}

/// Somebody who says no to everything. For the tool scenario on a tier that
/// might reach for a shell: a callback that panics cannot report the thing the
/// scenario exists to measure.
fn refusing() -> impl FnMut(Asking) -> Ready<Decision> + Send {
    |_| ready(Decision::Deny)
}

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

fn offered(request: &Request) -> Vec<&str> {
    request
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect()
}

// --- the scenarios ----------------------------------------------------------

/// **The fetched model answers**, on all three tiers.
///
/// A model that did not exist on this machine when the process started, read
/// off the library and started through the same `LlamaCppConfig` every other
/// live suite uses, answers a question with nothing on offer.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the network, a card and ~25 GB; see the live command in AGENTS.md"]
async fn a_model_fetched_in_this_run_answers_on_every_tier() {
    for tier in Tier::ALL {
        let rig = Rig::fetched("answers", tier).await;
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");
        let chat = rig.chat(false);
        loaded(&chat, tier).await;

        let answer = chat
            .ask(SUM, |_| {}, nobody())
            .await
            .unwrap_or_else(|error| {
                panic!("the fetched {} model did not answer: {error}", tier.label())
            });
        assert!(
            offered(&Watching::sent()[0]).is_empty(),
            "this scenario offers nothing"
        );
        assert!(
            answer.text.contains("42"),
            "the fetched {} model ({}) was asked 17 plus 25 and said: {}",
            tier.label(),
            fetched(tier).await.local.label,
            answer.text
        );
        println!(
            "the fetched {} model ({}) answered: {}",
            tier.label(),
            fetched(tier).await.local.label,
            answer.text.trim()
        );
        chat.shutdown().await;
    }
}

/// **The fetched model calls a tool**, with nothing in the prompt naming one.
///
/// The slice's `Bar: chose`. S2's planted file, against weights Demido chose
/// the file for: the answer carries a code only the file holds, the first
/// payload is asserted not to carry it, and a call the model picked out of six
/// is where it came back from.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the network, a card and ~25 GB; see the live command in AGENTS.md"]
async fn a_fetched_model_calls_a_tool_with_nothing_naming_one() {
    let mut red: Vec<Tier> = Vec::new();
    for tier in Tier::ALL {
        let rig = Rig::fetched("calls", tier).await;
        if !called(&rig, false).await {
            red.push(tier);
        }
    }

    match red.as_slice() {
        [] => {}
        // Examined before it is written off, the way the rig gives the
        // secondary tier the job of doing it: the same question to the
        // secondary model at Q4_K_M. It is a different model, not the breadth
        // weights at a kinder quant (those do not fit the card), so green
        // there says the red belongs to the breadth weights at this
        // quantisation rather than to Demido, and red there says Demido.
        [Tier::Breadth] => {
            let rig = Rig::new("calls", Tier::Secondary, rig::require(Tier::Secondary));
            assert!(
                called(&rig, false).await,
                "the fetched breadth model did not answer out of the file, and \
                 neither did the secondary model at Q4_K_M, so the red is \
                 Demido's rather than the breadth weights'"
            );
            println!(
                "note: the fetched breadth model did not answer out of the file \
                 and the secondary model at Q4_K_M did, so it is recorded as a \
                 model note rather than a defect (docs/rules/done.md)"
            );
        }
        other => panic!(
            "these fetched tiers did not answer out of the planted file: {:?}. \
             A red anywhere but breadth is a defect rather than a note",
            other.iter().map(|tier| tier.label()).collect::<Vec<_>>()
        ),
    }
}

/// The closing run's trace: the fetched development model's tool call, rebuilt
/// from its log and committed as `tests/fixtures/a-fetched-model`, which
/// `replayed.rs` reads with no card.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the network, a card and ~4 GB; see the live command in AGENTS.md"]
async fn the_log_of_a_fetched_model_s_tool_call_rebuilds_and_is_kept() {
    let rig = Rig::fetched("calls", Tier::Development).await;
    assert!(
        called(&rig, true).await,
        "the fetched development model did not answer out of the planted file"
    );
}

/// Ask `rig`'s model for the planted code. `true` when it answered with it and
/// a call it chose brought it back.
async fn called(rig: &Rig, keep_the_fixture: bool) -> bool {
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");
    let tier = rig.tier;
    rig.plant("manifest.txt", PLANTED);
    let chat = rig.chat(true);
    loaded(&chat, tier).await;

    let answer = chat
        .ask(ASKED, |_| {}, refusing())
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
    let before_anything_ran = serde_json::to_string(&sent[0]).expect("a request");
    assert!(
        !before_anything_ran.contains(UNGUESSABLE),
        "the planted seal was already in the first payload, so this proves \
         nothing: {before_anything_ran}"
    );

    let made = calls(&chat);
    let brought_it_back: Vec<&str> = made
        .iter()
        .filter(|call| {
            matches!(
                &call.outcome,
                Some(Outcome::Returned { text, failed: false }) if text.contains(UNGUESSABLE)
            )
        })
        .map(|call| call.name.as_str())
        .collect();
    let green = answer.text.contains(UNGUESSABLE) && !brought_it_back.is_empty();
    println!(
        "the {} model chose {:?} out of six and answered: {}",
        tier.label(),
        made.iter()
            .map(|call| call.name.as_str())
            .collect::<Vec<_>>(),
        answer.text.trim()
    );

    chat.shutdown().await;
    if keep_the_fixture && green {
        keep(&rig.log(), &rig.project, &sent, "a-fetched-model");
    }
    green
}

/// **The projector arrives with its weights.**
///
/// The development tier's repository publishes a vision projector, and the
/// smallest quantisation of it is fetched as **one item**: the weights and the
/// projector, one row, one `enqueue`. Both files are then in the library, the
/// library offers exactly one model for that repository, and the projector is
/// a companion of it rather than a model of its own.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the network and ~4 GB; see the live command in AGENTS.md"]
async fn the_projector_arrives_with_its_weights_and_is_not_offered_as_a_model() {
    let fetched = fetched(Tier::Development).await;

    let projector = fetched
        .item
        .files
        .last()
        .filter(|_| fetched.item.files.len() > 1)
        .expect("the item carries a projector after its weights");
    assert!(
        projector
            .destination
            .file_name()
            .is_some_and(|name| name.to_string_lossy().to_lowercase().contains("mmproj")),
        "the last piece is not a projector: {projector:?}"
    );
    for piece in &fetched.item.files {
        assert!(
            piece.destination.is_file(),
            "{} is not in the library",
            piece.destination.display()
        );
    }

    let scan = Library::open(&fetched.folders).scan();
    let repo = Tier::Development.repo();
    let offered: Vec<&Local> = scan
        .models
        .iter()
        .filter(|model| model.repo.as_deref() == Some(repo))
        .collect();
    assert_eq!(offered.len(), 1, "one model, offered once: {offered:?}");
    let model = offered[0];
    assert!(
        scan.models
            .iter()
            .all(|model| model.path != projector.destination),
        "the projector is offered as a model"
    );
    assert!(
        model
            .companions
            .iter()
            .any(|companion| companion.kind == Part::Projector
                && companion.path == projector.destination),
        "the projector is not the model's companion: {:?}",
        model.companions
    );
    assert_eq!(
        model.capabilities.vision,
        Fact::Yes,
        "a projector beside the weights is what vision is read from"
    );
    println!(
        "{} offered once, with {} beside it",
        model.label,
        projector.destination.display()
    );
}

/// **The borrowed model loads**, and nothing in its folder is written.
///
/// The rig's own library, which another tool filled, as a scan folder. The
/// development model is found in it, offered as borrowed, refused a delete, and
/// answers. Every file under that folder is the same length with the same
/// modification time afterwards as before, and no file has appeared or gone.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs a card and the rig; see the live command in AGENTS.md"]
async fn a_borrowed_model_loads_and_nothing_in_its_folder_is_written() {
    let borrowed = rig::models_root();
    let before = snapshot(&borrowed);
    assert!(!before.is_empty(), "the rig's library is empty");

    let folders = Folders {
        download: scratch().join("borrowing").join("models"),
        scan: vec![borrowed.clone()],
    };
    let library = Library::open(&folders);
    let wanted = std::fs::canonicalize(Tier::Development.path()).expect("the rig's model");
    let local = library
        .scan()
        .models
        .into_iter()
        .find(|model| std::fs::canonicalize(&model.path).is_ok_and(|path| path == wanted))
        .expect("the development model is offered out of the borrowed folder");
    assert!(local.borrowed, "a model in a scan folder is borrowed");
    assert!(
        library.remove(&local.path).is_err(),
        "the library agreed to delete a borrowed model"
    );

    {
        let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");
        let tier = Tier::Development;
        let rig = Rig::new("borrowed", tier, rig::configured(local.path.clone(), tier));
        let chat = rig.chat(false);
        loaded(&chat, tier).await;
        let answer = chat.ask(SUM, |_| {}, nobody()).await.expect("an answer");
        assert!(
            answer.text.contains("42"),
            "the borrowed model said: {}",
            answer.text
        );
        chat.shutdown().await;
        println!(
            "{} from {} answered: {}",
            local.label,
            local.library,
            answer.text.trim()
        );
    }

    assert_eq!(
        snapshot(&borrowed),
        before,
        "something under the borrowed folder {} changed",
        borrowed.display()
    );
}

/// Every entry under `root`, directories included: its length and when it was
/// last written. A directory is in it so an empty one made inside the folder
/// is a change too.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let mut found = BTreeMap::new();
    let mut waiting = vec![root.to_path_buf()];
    while let Some(dir) = waiting.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            found.insert(path.clone(), (meta.len(), meta.modified().ok()));
            if meta.is_dir() {
                waiting.push(path);
            }
        }
    }
    found
}

/// **The trace rebuild survives an edit**, on the fetched model.
///
/// #77's promise against weights nobody had an hour ago: a question under the
/// shipped `read_file`, the document edited through the register the Settings
/// page writes through, a second question under the new words, and the log
/// rebuilding every request byte for byte in the wording that produced it.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs the network, a card and ~4 GB; see the live command in AGENTS.md"]
async fn a_tool_edited_mid_session_rebuilds_each_reply_against_its_own_wording() {
    let tier = Tier::Development;
    let rig = Rig::fetched("edited", tier).await;
    let _permit = rig::ONE_MODEL_AT_A_TIME.acquire().await.expect("a permit");
    rig.plant("manifest.txt", PLANTED);
    rig.plant("locker.txt", LOCKER);
    let chat = rig.chat(true);
    loaded(&chat, tier).await;

    chat.ask(ASKED, |_| {}, refusing())
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
            "The locker combination is written in locker.txt. What is it? \
             Answer with the combination alone.",
            |_| {},
            refusing(),
        )
        .await
        .expect("the second answer");
    chat.shutdown().await;

    let sent = Watching::sent();
    assert!(sent.len() > before, "the second question sent nothing");
    let mut wordings = 0;
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
        wordings += 1;
    }
    assert!(
        wordings > before,
        "no request after the edit offered read_file"
    );
    rebuilds(&rig.log(), &sent);

    println!(
        "{before} step(s) under {}, {} under {}; the second answer: {}",
        shipped.hash,
        sent.len() - before,
        edited.hash,
        answer.text.trim()
    );
    assert!(
        answer.text.contains(LOCKER_CODE),
        "under the edited wording the fetched model was asked for a combination \
         only locker.txt holds, and said: {}",
        answer.text
    );
}

const LOCKER: &str = "The locker combination for the east stairwell is 3316-HALYARD-FENNEL.\n";
const LOCKER_CODE: &str = "3316-HALYARD-FENNEL";

/// `read_file`, reworded the way a person in the editor would: the same
/// parameters, new prose on all of them.
const EDITED_READ_FILE: &str = "Fetch the text of one file in the workspace, returned with line numbers. The path is relative to the workspace root.

## path

The file to fetch, relative to the workspace root, for example docs/plan.md

## from_line

The line to start at, counting from 1. Omit it to start at the top.

## lines

How many lines to return. Omit it for everything from the start line on.
";
