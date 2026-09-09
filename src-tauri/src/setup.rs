//! The guided set-up: what the wizard reads, and the eight gestures it makes.
//!
//! `docs/rules/setup.md` is the contract. The reasoning is `demido-setup`, the
//! fetching is `demido-runtimes`, the pins are `demido-catalog` and the
//! detection is `demido-hardware`. What is here is the assembly of those four
//! into the one answer a wizard step draws, and the writing of a choice back.
//!
//! **Every command returns the whole view.** A gesture changes one answer and
//! moves several rows: ticking a capability changes a total, choosing an
//! accelerator changes which archives are offered, and a fetch that verifies
//! settles a step. A window that edited its own copy would be a wizard that
//! disagrees with the disk, which is the failure this whole slice is arranged
//! around, so nothing here returns a fragment.
//!
//! **This file states facts and writes no sentences**, with one exception
//! marked where it happens: a verification refusal is recorded in the ledger
//! as a reason and there is no window on the other side of it to write one.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use demido_catalog::{Archive, Availability, Group, Kind, Selector};
use demido_hardware::{Ecosystem, Machine, Preselection};
use demido_runtimes::{
    Installed, Ledger, Outcome, Progress, RowState, Runtimes, Verification, LLAMA_CPP,
};
use demido_setup::{Answers, Model, Plan, Store as _};

use crate::wiring::{AnswersStore, RuntimesStore, Wiring};

/// Bytes arriving, while they arrive.
///
/// An event rather than the return value of [`setup_fetch`], for the reason
/// `chat://update` is one: a call is a question with one answer, and 515 MiB
/// is not one answer. The window draws the bar from these and learns the
/// outcome from the call resolving.
const PROGRESS: &str = "setup://progress";

/// Everything the wizard and the settings page need, in one read.
pub struct Setup {
    answers: Arc<AnswersStore>,
    runtimes: Arc<Runtimes<RuntimesStore>>,
    runtimes_dir: PathBuf,
    /// This machine, probed once.
    ///
    /// Once rather than per call, because DXGI and the CUDA driver are asked
    /// the same question every time and the answer does not change while the
    /// app is open. Lazily rather than at assembly, because the composition
    /// root runs before there is a window to report a failed probe on, and
    /// detection that failed is a `Note` rather than an error anyway.
    machine: OnceLock<Machine>,
    /// Held across a read-modify-write of the answers.
    ///
    /// Two gestures arriving together is a person clicking quickly, and
    /// without this the second would write the answers it read before the
    /// first landed, losing it.
    writing: Mutex<()>,
}

impl Setup {
    pub fn new(
        answers: Arc<AnswersStore>,
        runtimes: Arc<Runtimes<RuntimesStore>>,
        runtimes_dir: PathBuf,
    ) -> Self {
        Self {
            answers,
            runtimes,
            runtimes_dir,
            machine: OnceLock::new(),
            writing: Mutex::new(()),
        }
    }

    /// This machine, probed at most once.
    pub fn machine(&self) -> &Machine {
        self.machine.get_or_init(Machine::detect)
    }

    /// What the person has chosen. The default answers when the file has
    /// never been written, which is the ordinary first launch.
    pub fn answers(&self) -> demido_core::Result<Answers> {
        Ok(self.answers.read()?)
    }

    /// The runtimes ledger, or an empty one.
    ///
    /// A ledger that cannot be read is reported and skipped rather than
    /// fatal (`AGENTS.md`): the wizard then draws every row as absent, which
    /// is what the disk looks like from where it is standing, and the fetch
    /// that follows writes a fresh ledger.
    pub fn ledger(&self) -> Ledger {
        self.runtimes.read().unwrap_or_else(|error| {
            tracing::warn!(%error, "the runtimes ledger could not be read");
            Ledger::default()
        })
    }

    /// What the answers name as the thing to talk to, or nothing.
    pub fn target(&self) -> Option<demido_setup::Target> {
        let answers = self.answers().ok()?;
        demido_setup::target(&self.ledger(), &self.runtimes_dir, &answers)
    }

    /// The accelerator selector for this machine, with the person's override
    /// applied.
    ///
    /// Built per call rather than kept, because it is derived from the machine
    /// and from one answer, and a kept copy is a copy that can be stale.
    fn selector(&self, answers: &Answers) -> Selector<'static> {
        let mut selector = Selector::for_machine(self.machine());
        if let Some(chosen) = answers.ecosystem {
            // A row carrying no build refuses the override and changes
            // nothing, which is `Selector::choose`'s own rule: an override is
            // never a broken install.
            selector.choose(chosen);
        }
        selector
    }

    /// Change the answers, then read everything back.
    fn amend(&self, change: impl FnOnce(&mut Answers)) -> demido_core::Result<Answers> {
        let held = self.writing.lock().unwrap_or_else(|held| held.into_inner());
        let mut answers = self.answers.read()?;
        change(&mut answers);
        self.answers.write(&answers)?;
        drop(held);
        Ok(answers)
    }
}

/// The whole of what a set-up surface draws.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    /// The steps and where each stands, derived from disk on every read.
    pub plan: Plan,
    pub complete: bool,
    /// Whether the wizard has been closed, by leaving it or by finishing it.
    /// It is front and centre on first launch, which is a profile where this
    /// is false.
    pub closed: bool,
    pub accelerator: AcceleratorView,
    /// The manifest, grouped. Two groups, from data.
    pub manifest: Vec<GroupView>,
    pub models: ModelsView,
}

/// The accelerator row: pre-selected from detection, still overridable.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorView {
    /// One per accelerator, always, including the ones no build is fetched
    /// for. A row's absence is a question nobody can ask about.
    pub rows: Vec<demido_catalog::Row>,
    /// What the machine indicated and why. The window writes the sentence.
    pub preselection: Preselection,
    /// The row selected right now.
    pub chosen: Ecosystem,
    /// Whether the person overrode the pre-selection.
    pub overridden: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupView {
    pub group: Group,
    pub rows: Vec<RuntimeRowView>,
}

/// One runtime row: what it costs, what it is now, and whether it is wanted.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRowView {
    /// The ledger's id for this row, which is also what a link names.
    pub id: String,
    /// The archives that arrive together as one row's worth of action.
    pub archives: Vec<ArchiveView>,
    /// Stated before a byte is spent, both of them, in MiB.
    pub download_mib: f64,
    pub on_disk_mib: f64,
    /// Whether this row is one the person wants fetched. Checkboxes, all on
    /// by default, and a row cleared is a row set-up leaves alone.
    pub ticked: bool,
    /// What the ledger says this row is: absent, managed or linked.
    pub state: RowState,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveView {
    pub name: String,
    pub pin: String,
    pub download_mib: f64,
    pub on_disk_mib: f64,
    /// How section 4 writes the license, which is what the credits surface
    /// shows too.
    pub license: String,
}

/// Where models are read from, and which one answers.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelsView {
    /// The folders the person has confirmed.
    pub folders: Vec<PathBuf>,
    /// Folders already readable on this machine that are not confirmed yet.
    /// The pre-fill, offered rather than adopted.
    pub suggested: Vec<PathBuf>,
    /// Every GGUF under the confirmed folders.
    pub models: Vec<Model>,
    /// The one that answers, if it is still on disk.
    pub chosen: Option<PathBuf>,
}

/// Assemble the view. One place, so every command returns the same shape.
fn view(setup: &Setup) -> demido_core::Result<View> {
    let answers = setup.answers()?;
    let ledger = setup.ledger();
    let plan = Plan::of(
        &demido_setup::Situation::observe(&ledger, &answers),
        &answers,
    );
    let selector = setup.selector(&answers);

    let confirmed = folders(&answers);
    let suggested = demido_setup::discover::folders()
        .into_iter()
        .filter(|folder| !confirmed.contains(folder))
        .collect();

    Ok(View {
        complete: plan.complete(),
        plan,
        closed: answers.closed,
        accelerator: AcceleratorView {
            rows: selector.rows.clone(),
            preselection: selector.preselection.clone(),
            chosen: selector.chosen,
            overridden: answers
                .ecosystem
                .is_some_and(|chosen| chosen != selector.preselection.ecosystem),
        },
        manifest: manifest(&selector, &answers, &ledger),
        models: ModelsView {
            models: demido_setup::discover::models(&confirmed),
            folders: confirmed,
            suggested,
            chosen: answers.model.filter(|model| model.is_file()),
        },
    })
}

/// The folders models are read from: the confirmed ones, or the pre-fill when
/// nobody has confirmed anything yet.
///
/// The pre-fill is what makes "a person with models from LM Studio moves no
/// files and makes no symlinks" true before they have clicked anything, and it
/// is also what a fetch verifies against: `docs/rules/runtimes.md` declares the
/// required group's check as loading a model, and on a fresh profile the only
/// model on the machine is in somebody else's folder.
fn folders(answers: &Answers) -> Vec<PathBuf> {
    if answers.folders.is_empty() {
        demido_setup::discover::preselected_folder()
            .into_iter()
            .collect()
    } else {
        answers.folders.clone()
    }
}

/// The manifest as the step draws it: one row per runtime, grouped.
///
/// The grouping is `Archive::group`, which is data on the pin. The capability
/// group is empty in this build and this function has no branch for that: when
/// uv, Python, SearXNG, Node, `agent-browser` and Chrome are pinned they are
/// rows here, which is section 4's "more rows, never a second screen" being
/// true of the code rather than promised by it.
fn manifest(selector: &Selector<'_>, answers: &Answers, ledger: &Ledger) -> Vec<GroupView> {
    let Some(selection) = selector.selection() else {
        return Vec::new();
    };

    let mut groups: Vec<GroupView> = Vec::new();
    for archive in selection.archives() {
        let id = runtime_id(archive);
        let entry = ArchiveView {
            name: archive.name.to_owned(),
            pin: archive.pin.to_owned(),
            download_mib: archive.download_mib,
            on_disk_mib: archive.on_disk_mib,
            license: archive.license.label(),
        };

        let group = match groups.iter().position(|had| had.group == archive.group) {
            Some(at) => at,
            None => {
                groups.push(GroupView {
                    group: archive.group,
                    rows: Vec::new(),
                });
                groups.len() - 1
            }
        };
        let Some(group) = groups.get_mut(group) else {
            continue;
        };

        // The build and its companion are one row: they move together or not
        // at all, because a CUDA build that cannot resolve its runtime does
        // not load a model. `demido-runtimes` calls that one row's worth of
        // action, and this is the same row.
        match group.rows.iter_mut().find(|row| row.id == id) {
            Some(row) => {
                row.download_mib += entry.download_mib;
                row.on_disk_mib += entry.on_disk_mib;
                row.archives.push(entry);
            }
            None => group.rows.push(RuntimeRowView {
                download_mib: entry.download_mib,
                on_disk_mib: entry.on_disk_mib,
                archives: vec![entry],
                ticked: answers.ticked(&id),
                state: ledger
                    .state(&id)
                    .cloned()
                    .unwrap_or(RowState::Absent { reason: None }),
                id,
            }),
        }
    }
    groups
}

/// Which ledger row an archive belongs to.
///
/// A build and the CUDA runtime it links against are the same row, which is
/// why this is a function rather than the archive's own name: the ledger has
/// one entry for the pair, and the wizard states one size for the pair.
fn runtime_id(archive: &Archive) -> String {
    match archive.kind {
        Kind::Build | Kind::CudaRuntime => LLAMA_CPP.to_owned(),
    }
}

/// Everything a set-up surface draws, in one call.
#[tauri::command]
pub fn setup_state(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<View> {
    view(&wiring.setup)
}

/// Override the accelerator.
///
/// A row carrying no build changes nothing, which is `Selector::choose`'s
/// rule: ROCm and Vulkan are honest empty rows, and choosing one would
/// otherwise be a broken install rather than a refusal.
#[tauri::command]
pub fn setup_choose_accelerator(
    wiring: tauri::State<'_, Wiring>,
    ecosystem: Ecosystem,
) -> demido_core::Result<View> {
    let setup = &wiring.setup;
    let offered = Selector::for_machine(setup.machine())
        .rows
        .iter()
        .any(|row| {
            row.ecosystem == ecosystem && matches!(row.availability, Availability::Offered { .. })
        });
    if offered {
        setup.amend(|answers| answers.ecosystem = Some(ecosystem))?;
    }
    view(setup)
}

/// Tick or clear one manifest row.
#[tauri::command]
pub fn setup_tick(
    wiring: tauri::State<'_, Wiring>,
    id: String,
    on: bool,
) -> demido_core::Result<View> {
    wiring.setup.amend(|answers| answers.tick(&id, on))?;
    view(&wiring.setup)
}

/// Confirm a folder models are read from.
#[tauri::command]
pub fn setup_add_folder(
    wiring: tauri::State<'_, Wiring>,
    path: String,
) -> demido_core::Result<View> {
    let folder = PathBuf::from(path);
    if !folder.is_dir() {
        return Err(demido_core::Error::not_found(
            "a folder",
            folder.display().to_string(),
        ));
    }
    wiring.setup.amend(|answers| answers.add_folder(folder))?;
    view(&wiring.setup)
}

/// Stop reading models from a folder. The folder itself is untouched.
#[tauri::command]
pub fn setup_remove_folder(
    wiring: tauri::State<'_, Wiring>,
    path: String,
) -> demido_core::Result<View> {
    let folder = PathBuf::from(path);
    wiring
        .setup
        .amend(|answers| answers.remove_folder(&folder))?;
    view(&wiring.setup)
}

/// Choose the model that answers.
///
/// Choosing one also confirms the folder it came out of, which is what turns a
/// pre-filled row into an answer: until then the folders being read are
/// whatever `discover` found, and after it they are what the person picked
/// from. Nothing is moved and no symlink is made either way
/// (`docs/rules/setup.md` section 7).
#[tauri::command]
pub fn setup_choose_model(
    wiring: tauri::State<'_, Wiring>,
    path: String,
) -> demido_core::Result<View> {
    let model = PathBuf::from(path);
    if !model.is_file() {
        return Err(demido_core::Error::not_found(
            "a model",
            model.display().to_string(),
        ));
    }
    let from = demido_setup::discover::models(&folders(&wiring.setup.answers()?))
        .into_iter()
        .find(|found| found.path == model)
        .map(|found| found.folder);
    wiring.setup.amend(|answers| {
        answers.model = Some(model);
        if let Some(folder) = from {
            answers.add_folder(folder);
        }
    })?;
    view(&wiring.setup)
}

/// Leave the wizard. The desk carries the rest.
#[tauri::command]
pub fn setup_leave(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<View> {
    wiring.setup.amend(|answers| answers.closed = true)?;
    view(&wiring.setup)
}

/// Take up the rest of the set-up, from the desk.
#[tauri::command]
pub fn setup_resume(wiring: tauri::State<'_, Wiring>) -> demido_core::Result<View> {
    wiring.setup.amend(|answers| answers.closed = false)?;
    view(&wiring.setup)
}

/// Fetch the ticked rows, verify them, and report the bytes as they arrive.
#[tauri::command]
pub async fn setup_fetch(app: AppHandle) -> demido_core::Result<View> {
    let (setup, runtimes) = {
        let wiring = app.state::<Wiring>();
        (wiring.setup.clone(), wiring.setup.runtimes.clone())
    };
    let answers = setup.answers()?;
    let selector = setup.selector(&answers);

    if answers.ticked(LLAMA_CPP) {
        if let Some(selection) = selector.selection() {
            let archives: Vec<Archive> = selection.archives().copied().collect();
            let pin = selection.build.pin.to_owned();
            let handle = app.clone();
            let outcome = runtimes
                .fetch_row(
                    LLAMA_CPP,
                    &pin,
                    &archives,
                    |archive, progress| report(&handle, LLAMA_CPP, archive, progress),
                    &demido_runtimes::Cancel::new(),
                )
                .await?;
            if let Outcome::Refused { reason } = outcome {
                tracing::warn!(reason, "the fetched runtime did not verify");
            }
        }
    }

    view(&setup)
}

/// Point a row at a binary the person already has.
///
/// `docs/rules/setup.md` section 7's second escape, and the same verification
/// a fetch runs: a link that does not load a model never becomes linked.
#[tauri::command]
pub async fn setup_link(app: AppHandle, id: String, path: String) -> demido_core::Result<View> {
    let (setup, runtimes) = {
        let wiring = app.state::<Wiring>();
        (wiring.setup.clone(), wiring.setup.runtimes.clone())
    };
    let binary = PathBuf::from(path);
    if !binary.is_file() {
        return Err(demido_core::Error::not_found(
            "a binary",
            binary.display().to_string(),
        ));
    }
    if let Outcome::Refused { reason } = runtimes.link(&id, binary, None).await? {
        tracing::warn!(reason, "the linked binary did not verify");
    }
    view(&setup)
}

/// Finish: point the conversation at what the answers name, and start it.
///
/// The wizard's last step and the composer cannot disagree about what answers,
/// because this is the only place either of them is told, and what it is told
/// is `demido_setup::target`.
///
/// The wizard closes only when a model is actually loaded. A finish that ends
/// in [`demido_chat::Presence::Failed`] leaves the wizard open on the step
/// carrying the reason, because set-up "is finished when a model has answered,
/// never because the steps were clicked through".
#[tauri::command]
pub async fn setup_finish(app: AppHandle) -> demido_core::Result<demido_chat::Presence> {
    let target = {
        let wiring = app.state::<Wiring>();
        let target = wiring.setup.target();
        wiring.chat.point_at(
            target
                .clone()
                .map(crate::wiring::Rig::from)
                .map(crate::wiring::Rig::model),
        );
        target
    };
    if target.is_none() {
        return Err(demido_core::Error::not_found("a model", "set-up"));
    }

    let presence = crate::chat::chat_load(app.clone()).await;
    if matches!(presence, demido_chat::Presence::Ready { .. }) {
        app.state::<Wiring>()
            .setup
            .amend(|answers| answers.closed = true)?;
    }
    Ok(presence)
}

/// Tell the window how far a fetch has got.
fn report(app: &AppHandle, id: &str, archive: &str, progress: Progress) {
    let payload = FetchProgress {
        id: id.to_owned(),
        archive: archive.to_owned(),
        bytes: progress.bytes,
        total: progress.total,
    };
    if let Err(error) = app.emit(PROGRESS, payload) {
        tracing::warn!(%error, "the window was not told about the fetch");
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FetchProgress {
    id: String,
    archive: String,
    bytes: u64,
    total: u64,
}

/// How a runtime row is verified, wherever it came from.
///
/// The declared command of `docs/rules/runtimes.md`: start the backend, load a
/// model already on disk, generate one token, stop.
///
/// **Which model, in two steps.** The one the models step settled on, if there
/// is one, because that is the pair that has to work and verifying the pair is
/// stronger than verifying either half. Otherwise the smallest under the
/// folders being read, which on a fresh profile is the folder somebody else's
/// tool filled, and which is the cheapest honest load available before anybody
/// has chosen anything.
///
/// The fallback is not free and the rig proved it: a folder can hold a GGUF
/// this build of `llama.cpp` cannot load at all, and being the smallest file
/// there is exactly what puts it in front of a runtime that works. A refusal
/// carries the backend's own message, which names the file, so that case reads
/// as the model it is rather than as a broken download.
pub fn verification(
    answers: Arc<AnswersStore>,
) -> impl Fn(Installed) -> Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
       + Send
       + Sync
       + 'static {
    move |installed: Installed| {
        let answers = answers.clone();
        Box::pin(async move {
            let chosen = answers.read().ok();
            let model = chosen.as_ref().and_then(|read| {
                read.model
                    .clone()
                    .filter(|model| model.is_file())
                    .or_else(|| demido_setup::discover::smallest(&folders(read)))
            });
            let Some(model) = model else {
                // not-a-prompt: the reason a row is absent, recorded in the
                // ledger for the runtimes step to draw. No model reads it, and
                // there is no window on this side of the call to write it.
                return Err("no model on disk to verify against".to_owned());
            };
            Verification::LoadsAModelAndGeneratesOneToken
                .run(&installed, &model)
                .await
                .map_err(|error| error.to_string())
        })
    }
}
