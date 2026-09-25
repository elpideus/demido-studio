use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use demido_catalog::Catalog;
use demido_core::InstallManifest;
use demido_fetch::{CancellationToken, Downloader};

use crate::events::{ProvisionEvent, StepState};
use crate::folder::Claim;
use crate::plan::{InstallPlan, StepId};
use crate::steps;

/// Shared state every step works with.
pub(crate) struct Ctx<'a> {
    pub plan: &'a InstallPlan,
    pub catalog: &'a Catalog,
    pub cancel: &'a CancellationToken,
    emit: &'a (dyn Fn(ProvisionEvent) + Send + Sync),
    pub downloader: Downloader,
    /// Scratch folder for archives, claimed by the steps that download and removed once every
    /// step has succeeded.
    pub downloads: PathBuf,
    log: Mutex<Option<std::fs::File>>,
}

impl Ctx<'_> {
    pub fn emit(&self, event: ProvisionEvent) {
        if let Ok(mut guard) = self.log.lock()
            && let Some(file) = guard.as_mut()
        {
            let line = match &event {
                ProvisionEvent::Progress { .. } => None,
                other => serde_json::to_string(other).ok(),
            };
            if let Some(line) = line {
                let _ = writeln!(file, "{} {line}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
            }
        }
        (self.emit)(event);
    }

    pub fn log(&self, id: StepId, line: impl Into<String>) {
        let line = line.into();
        tracing::info!(step = ?id, "{line}");
        self.emit(ProvisionEvent::Log { id, line });
    }

    pub fn progress(
        &self,
        id: StepId,
        done: u64,
        total: Option<u64>,
        bytes_per_second: f64,
        activity: impl Into<String>,
    ) {
        self.emit(ProvisionEvent::Progress {
            id,
            done,
            total,
            bytes_per_second,
            activity: activity.into(),
        });
    }

    pub fn install_dir(&self) -> &std::path::Path {
        &self.plan.install_dir
    }
}

/// Executes `plan`. Returns the manifest that was written to `install.json`, even when some
/// non-critical steps failed (they are listed in the final [`ProvisionEvent::Finished`]).
pub async fn run(
    plan: &InstallPlan,
    catalog: &Catalog,
    cancel: CancellationToken,
    emit: &(dyn Fn(ProvisionEvent) + Send + Sync),
) -> anyhow::Result<InstallManifest> {
    let install_dir = &plan.install_dir;
    // The wizard checks the folder before it gets here, but not every caller does (the dev CLI
    // takes any folder), so the engine refuses it too, before creating anything there.
    if let Some(problem) = crate::folder::check(install_dir, &demido_core::paths::user_data_dir()).problem() {
        anyhow::bail!(problem);
    }
    std::fs::create_dir_all(install_dir).with_context(|| format!("creating {}", install_dir.display()))?;
    // Start from the previous manifest, so re-running the installer repairs instead of redoing.
    let previous = crate::folder::installation(install_dir);
    // The top-level names the app step creates.
    let app = match &plan.payload {
        Some(payload) => steps::app::entries(payload)?,
        None => Vec::new(),
    };
    // Before anything else goes into the folder: what setup creates there is claimed first, so
    // even a run that stops halfway leaves a record, and nothing that was there is claimed.
    let mut claim = Claim::begin(install_dir, previous.as_ref())?;
    let result = run_claimed(plan, catalog, &cancel, emit, &mut claim, previous, &app).await;
    if result.is_err() {
        // Steps claim names before creating them: the app's whole payload before unpacking,
        // `runtime` and `.downloads` before downloading. Left in the marker, what a run that
        // stopped never created would let a retry or the uninstaller take another program's
        // file of that name for setup's.
        if let Err(e) = claim.forget_missing() {
            tracing::warn!("{e:#}");
        }
    }
    result
}

/// The rest of [`run`], once `claim` records what setup creates in the install folder.
async fn run_claimed(
    plan: &InstallPlan,
    catalog: &Catalog,
    cancel: &CancellationToken,
    emit: &(dyn Fn(ProvisionEvent) + Send + Sync),
    claim: &mut Claim,
    previous: Option<InstallManifest>,
    app: &[String],
) -> anyhow::Result<InstallManifest> {
    let install_dir = &plan.install_dir;
    // The log goes into another program's `logs` folder no more than anything else does; the
    // events still carry every line.
    claim.add(["logs".to_string()])?;
    let log_file = if claim.owns("logs") {
        let logs = install_dir.join("logs");
        std::fs::create_dir_all(&logs)?;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(logs.join("install.log"))
            .ok()
    } else {
        None
    };

    let ctx = Ctx {
        plan,
        catalog,
        cancel,
        emit,
        downloader: Downloader::new(&format!("DemidoStudio-Setup/{}", demido_core::brand::VERSION))?,
        downloads: install_dir.join(".downloads"),
        log: Mutex::new(log_file),
    };

    let steps = plan.steps(catalog);
    ctx.emit(ProvisionEvent::Plan { steps: steps.clone() });

    let mut manifest = previous.unwrap_or_else(|| InstallManifest::new(plan.scope));
    manifest.scope = plan.scope;
    manifest.version = demido_core::brand::VERSION.to_string();
    manifest.hardware = plan.hardware.clone();

    let mut failed: Vec<StepId> = Vec::new();
    let mut unusable: HashSet<StepId> = HashSet::new();

    for step in &steps {
        let id = step.id;
        if cancel.is_cancelled() {
            break;
        }
        let blocked_by = match id {
            StepId::Python if unusable.contains(&StepId::Uv) => Some("uv"),
            StepId::PythonPackages if unusable.contains(&StepId::Uv) => Some("uv"),
            StepId::PythonPackages if unusable.contains(&StepId::Python) => Some("Python"),
            _ => None,
        };
        if let Some(dep) = blocked_by {
            unusable.insert(id);
            ctx.emit(ProvisionEvent::Step {
                id,
                state: StepState::Skipped,
                message: Some(format!("Skipped because {dep} could not be installed.")),
            });
            continue;
        }

        ctx.emit(ProvisionEvent::Step {
            id,
            state: StepState::Running,
            message: None,
        });
        let result = run_step(&ctx, id, &mut manifest, claim, app, failed.is_empty()).await;
        match result {
            Ok(()) => ctx.emit(ProvisionEvent::Step {
                id,
                state: StepState::Done,
                message: None,
            }),
            Err(err) => {
                let cancelled = cancel.is_cancelled();
                let message = if cancelled {
                    "Cancelled.".to_string()
                } else {
                    format!("{err:#}")
                };
                tracing::error!(step = ?id, "{message}");
                ctx.emit(ProvisionEvent::Step {
                    id,
                    state: StepState::Failed,
                    message: Some(message),
                });
                failed.push(id);
                unusable.insert(id);
                if id.critical() || cancelled {
                    ctx.emit(ProvisionEvent::Finished {
                        success: false,
                        failed: failed.clone(),
                    });
                    return Err(err.context(format!("{id:?} failed")));
                }
            }
        }
    }

    if cancel.is_cancelled() {
        ctx.emit(ProvisionEvent::Finished { success: false, failed });
        anyhow::bail!("installation cancelled");
    }

    ctx.emit(ProvisionEvent::Finished {
        success: failed.is_empty(),
        failed,
    });
    Ok(manifest)
}

/// Stamps the manifest for this run. The models folder is always this plan's, never one carried
/// over from an earlier install of another scope; `created` is everything setup has put in the
/// install folder, this run and earlier ones.
fn finalize(manifest: &mut InstallManifest, plan: &InstallPlan, created: Vec<String>) {
    manifest.installed_at = chrono::Utc::now();
    manifest.models_dir = Some(plan.models_dir.clone());
    manifest.created = Some(created);
}

/// Runs step `id`. `app` are the top-level names the app step creates; `clean` says every step
/// before this one succeeded.
async fn run_step(
    ctx: &Ctx<'_>,
    id: StepId,
    manifest: &mut InstallManifest,
    claim: &mut Claim,
    app: &[String],
    clean: bool,
) -> anyhow::Result<()> {
    // These install into `runtime` through the scratch folder, replacing what is there: neither
    // may be another program's.
    if matches!(
        id,
        StepId::Runtime | StepId::Node | StepId::Uv | StepId::Python | StepId::PythonPackages
    ) {
        claim.take(&["runtime", ".downloads"])?;
    }
    match id {
        StepId::App => steps::app::install(ctx, claim, app).await,
        StepId::Runtime => {
            manifest.runtime = Some(steps::runtime::install(ctx, manifest.runtime.as_ref()).await?);
            Ok(())
        }
        StepId::Node => {
            manifest.node = Some(steps::tools::install_node(ctx, manifest.node.as_ref()).await?);
            Ok(())
        }
        StepId::Uv => {
            manifest.uv = Some(steps::tools::install_uv(ctx, manifest.uv.as_ref()).await?);
            Ok(())
        }
        StepId::Python => {
            let uv = manifest.uv.clone().context("uv is not installed")?;
            manifest.python = Some(steps::python::install_python(ctx, &uv).await?);
            Ok(())
        }
        StepId::PythonPackages => {
            let uv = manifest.uv.clone().context("uv is not installed")?;
            let python = manifest.python.clone().context("Python is not installed")?;
            steps::python::install_packages(ctx, &uv, &python).await
        }
        StepId::Model => {
            let model = steps::model::download(ctx).await?;
            manifest.models_dir = Some(ctx.plan.models_dir.clone());
            manifest.starter_model = Some(model);
            Ok(())
        }
        StepId::Shortcuts => steps::shortcuts::create(ctx).await,
        StepId::Finalize => {
            // Partial downloads of failed steps are kept so a retry resumes them.
            if clean && claim.owns(".downloads") {
                let _ = std::fs::remove_dir_all(&ctx.downloads);
            }
            // What setup claimed but did not create, or has removed, is not recorded, so whatever
            // takes that name later is never taken for setup's.
            claim.forget_missing()?;
            claim.add([InstallManifest::FILE_NAME.to_string()])?;
            finalize(manifest, ctx.plan, claim.names());
            manifest.save(ctx.install_dir()).context("writing install.json")?;
            claim.finish();
            ctx.log(
                id,
                format!("Wrote {}", ctx.install_dir().join(InstallManifest::FILE_NAME).display()),
            );
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use demido_core::{Backend, InstallScope};

    use super::*;

    fn plan(scope: InstallScope) -> InstallPlan {
        InstallPlan {
            scope,
            install_dir: std::env::temp_dir().join("demido-plan"),
            backend: Backend::Cpu,
            variant: "windows-cpu".into(),
            model: None,
            model_context: 4096,
            models_dir: demido_core::paths::starter_models_dir(scope),
            python: false,
            node: false,
            shortcuts: false,
            register: false,
            payload: None,
            hardware: serde_json::Value::Null,
        }
    }

    /// A plan that only unpacks an app made of `files` into `dir`: nothing is downloaded, and
    /// there are no shortcuts or registration.
    fn unpack_only(dir: &std::path::Path, files: &[&str]) -> InstallPlan {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(b"x").unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        InstallPlan {
            install_dir: dir.to_path_buf(),
            // No runtime build has this name, so there is no runtime step.
            variant: "none".into(),
            payload: Some(crate::AppPayload {
                zip: std::sync::Arc::from(bytes),
                uninstaller_source: None,
            }),
            ..plan(InstallScope::User)
        }
    }

    /// `demido-setup-cli --dev --dir` into `dir`, with nothing to download: no app, no runtime,
    /// no tools.
    fn dev_only(dir: &std::path::Path) -> InstallPlan {
        InstallPlan {
            install_dir: dir.to_path_buf(),
            variant: "none".into(),
            ..plan(InstallScope::User)
        }
    }

    async fn install(plan: &InstallPlan) -> anyhow::Result<InstallManifest> {
        install_noting_failures(plan).await.0
    }

    /// Runs `plan`, returning its result and why each step that failed did.
    async fn install_noting_failures(plan: &InstallPlan) -> (anyhow::Result<InstallManifest>, Vec<String>) {
        let failures = Mutex::new(Vec::new());
        let note = |event: ProvisionEvent| {
            if let ProvisionEvent::Step {
                state: StepState::Failed,
                message: Some(message),
                ..
            } = event
            {
                failures.lock().unwrap().push(message);
            }
        };
        let result = run(plan, demido_catalog::catalog(), CancellationToken::new(), &note).await;
        (result, failures.into_inner().unwrap())
    }

    /// Runs `plan`, calling `on_start` with the run's cancellation token as each step starts.
    async fn install_hooked(
        plan: &InstallPlan,
        on_start: impl Fn(StepId, &CancellationToken) + Send + Sync,
    ) -> anyhow::Result<InstallManifest> {
        let cancel = CancellationToken::new();
        let token = cancel.clone();
        let hook = move |event: ProvisionEvent| {
            if let ProvisionEvent::Step {
                id,
                state: StepState::Running,
                ..
            } = event
            {
                on_start(id, &token);
            }
        };
        run(plan, demido_catalog::catalog(), cancel, &hook).await
    }

    fn has(created: &[String], name: &str) -> bool {
        created.iter().any(|n| n == name)
    }

    /// The names in `dir`, sorted.
    fn listing(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    const APP: [&str; 3] = ["demido-studio.exe", "resources/skills/a.md", "LICENSE"];
    const MARKER: &str = crate::folder::MARKER;

    #[tokio::test]
    async fn a_fresh_install_records_what_it_created_and_drops_its_marker() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        let manifest = install(&unpack_only(&dir, &APP)).await.unwrap();
        let created = manifest.created.unwrap();
        // No step downloaded anything or installed a runtime, so neither name is setup's.
        assert_eq!(
            created,
            [
                MARKER,
                "LICENSE",
                "demido-studio.exe",
                "install.json",
                "logs",
                "resources",
            ]
        );
        assert_eq!(InstallManifest::load(&dir).unwrap().created, Some(created));
        assert!(!dir.join(MARKER).exists());
        assert!(!dir.join(".downloads").exists());
        assert!(!dir.join("runtime").exists());
        assert!(dir.join("resources").join("skills").join("a.md").is_file());
    }

    #[tokio::test]
    async fn a_retry_takes_over_what_an_unfinished_install_left() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        // Unpacking stops at an entry that would leave the folder, which fails the install.
        let broken = unpack_only(&dir, &["demido-studio.exe", "resources/a.md", "../escape", "LICENSE"]);
        assert!(install(&broken).await.is_err());
        assert!(!dir.join(InstallManifest::FILE_NAME).exists());
        assert!(dir.join(MARKER).is_file());
        assert!(dir.join("demido-studio.exe").is_file());
        // The wizard lets the person pick the folder again.
        assert_eq!(crate::folder::check(&dir, &root.path().join("data")).problem(), None);

        std::fs::write(dir.join("notes.txt"), b"the person's").unwrap();
        let created = install(&unpack_only(&dir, &APP)).await.unwrap().created.unwrap();
        for name in [MARKER, "demido-studio.exe", "resources", "LICENSE", "logs"] {
            assert!(has(&created, name), "{name} missing from {created:?}");
        }
        assert!(!has(&created, "notes.txt"));
        assert!(!dir.join(MARKER).exists());
    }

    /// Another program's folder, say D:\Server. Setup and the app use each of these names; on
    /// Windows and macOS `Resources` is the `resources` folder the app unpacks into.
    const THEIRS: [&str; 3] = ["logs/server.log", ".downloads/update.bin", "Resources/theirs.txt"];

    /// The engine checks the folder itself: the dev CLI, say, takes any folder it is given.
    #[tokio::test]
    async fn the_engine_refuses_another_programs_folder() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Server");
        put_theirs(&dir, &THEIRS);
        let before = listing(&dir);
        for refused in [unpack_only(&dir, &APP), dev_only(&dir)] {
            let err = install(&refused).await.unwrap_err();
            assert_eq!(
                err.to_string(),
                "This folder already contains other files. Choose an empty or new folder."
            );
            assert_eq!(listing(&dir), before);
            assert_theirs(&dir, &THEIRS);
        }
    }

    /// Reaching into `runtime` would replace another program's `runtime/<tool>` with setup's.
    #[tokio::test]
    async fn the_engine_refuses_a_folder_holding_another_programs_runtime() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Tools");
        let theirs = [
            "runtime/llama/llama-server.exe",
            "runtime/node/node.exe",
            "runtime/uv/uv.exe",
            "runtime/pyenv/Scripts/python.exe",
        ];
        put_theirs(&dir, &theirs);
        let tools = InstallPlan {
            install_dir: dir.clone(),
            node: true,
            python: true,
            ..plan(InstallScope::User)
        };
        let err = install(&tools).await.unwrap_err();
        assert_eq!(
            err.to_string(),
            "This folder already contains other files. Choose an empty or new folder."
        );
        assert_eq!(listing(&dir), ["runtime"]);
        assert_theirs(&dir, &theirs);
    }

    /// An `install.json` setup cannot act on is never overwritten: a damaged one, a newer setup's,
    /// or one that does not record what setup created (only builds never released wrote those).
    #[tokio::test]
    async fn the_engine_refuses_an_install_json_it_cannot_read() {
        let root = tempfile::tempdir().unwrap();
        let mut newer = InstallManifest::new(InstallScope::User);
        newer.schema = InstallManifest::SCHEMA + 1;
        newer.created = Some(vec!["install.json".into()]);
        let mut unrecorded = InstallManifest::new(InstallScope::User);
        unrecorded.created = None;
        let contents = [
            b"{\"schema\": 1, \"vers".to_vec(),
            serde_json::to_vec(&newer).unwrap(),
            serde_json::to_vec(&unrecorded).unwrap(),
        ];
        for (i, content) in contents.into_iter().enumerate() {
            let dir = root.path().join(format!("Folder {i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(InstallManifest::FILE_NAME), &content).unwrap();
            // Not even next to the marker of an unfinished run.
            for with_marker in [false, true] {
                if with_marker {
                    std::fs::write(dir.join(MARKER), b"{\"created\": [\"install.json\"]}").unwrap();
                }
                let before = listing(&dir);
                for refused in [unpack_only(&dir, &APP), dev_only(&dir)] {
                    let err = install(&refused).await.unwrap_err().to_string();
                    assert!(
                        err.starts_with("This folder has an install.json that Setup can't read"),
                        "{err}"
                    );
                    assert_eq!(std::fs::read(dir.join(InstallManifest::FILE_NAME)).unwrap(), content);
                    assert_eq!(listing(&dir), before);
                }
            }
        }
    }

    #[tokio::test]
    async fn reinstalling_keeps_what_the_installation_created() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &["demido-studio.exe", "old-folder/file"]))
            .await
            .unwrap();
        std::fs::write(dir.join("notes.txt"), b"the person's").unwrap();
        let created = install(&unpack_only(&dir, &APP)).await.unwrap().created.unwrap();
        for name in [
            "old-folder",
            "demido-studio.exe",
            "resources",
            "LICENSE",
            "install.json",
        ] {
            assert!(has(&created, name), "{name} missing from {created:?}");
        }
        assert!(!has(&created, "notes.txt"));
        assert!(!dir.join(MARKER).exists());
    }

    /// Removes the installation in `dir` the way the uninstaller does, and returns what is left.
    fn uninstall(dir: &std::path::Path) -> Vec<String> {
        let manifest = crate::folder::installation(dir);
        let mut notes = Vec::new();
        crate::steps::uninstall::remove_install_dir(
            dir,
            &crate::folder::removable_entries(dir, manifest.as_ref()),
            &mut notes,
        );
        listing(dir)
    }

    #[tokio::test]
    async fn an_update_that_stops_partway_still_leaves_a_record() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        // The new version brings a `locales` folder, and unpacking fails after it.
        let update = unpack_only(&dir, &["demido-studio.exe", "locales/en.pak", "../escape", "LICENSE"]);
        assert!(install(&update).await.is_err());
        assert!(dir.join("locales").join("en.pak").is_file());
        let manifest = InstallManifest::load(&dir).unwrap();
        assert!(!has(&manifest.created.unwrap(), "locales"));

        // The uninstaller takes it too, and with it the folder.
        assert_eq!(uninstall(&dir), Vec::<String>::new());
        assert!(!dir.exists());
    }

    /// Only builds that were never released wrote a `created` that leaves out some of what setup
    /// made. What it leaves out is not setup's to overwrite, so an update stops before unpacking,
    /// and the uninstaller leaves it behind rather than risk taking another program's files.
    #[tokio::test]
    async fn an_update_does_not_take_back_what_a_partial_manifest_left_out() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        std::fs::create_dir_all(dir.join("runtime").join("llama")).unwrap();
        std::fs::write(dir.join("runtime").join("llama").join("llama-server.exe"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"the person's").unwrap();
        // A manifest that lists only the app.
        let mut partial = InstallManifest::load(&dir).unwrap();
        partial.created = Some(vec!["demido-studio.exe".into()]);
        partial.save(&dir).unwrap();

        let err = format!("{:#}", install(&unpack_only(&dir, &APP)).await.unwrap_err());
        assert!(err.contains("Setup did not create "), "{err}");
        let removable = crate::folder::removable_entries(&dir, crate::folder::installation(&dir).as_ref());
        for name in ["install.json", "logs", "runtime", "resources", "LICENSE", "notes.txt"] {
            assert!(!has(&removable, name), "{name} taken back in {removable:?}");
        }
        assert!(has(&removable, "demido-studio.exe"));
        assert_eq!(
            uninstall(&dir),
            ["LICENSE", "install.json", "logs", "notes.txt", "resources", "runtime"]
        );
        assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"the person's");
    }

    /// Writes another program's `files` into `dir`, each holding `theirs`.
    fn put_theirs(dir: &std::path::Path, files: &[&str]) {
        for file in files {
            let path = dir.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"theirs").unwrap();
        }
    }

    /// Fails unless each of `files` is still in `dir` as the other program left it.
    fn assert_theirs(dir: &std::path::Path, files: &[&str]) {
        for file in files {
            let content = std::fs::read(dir.join(file)).ok();
            assert_eq!(content.as_deref(), Some(&b"theirs"[..]), "{file} was not kept");
        }
    }

    /// Fails if any of setup's `names` is still in `dir`.
    fn assert_gone(dir: &std::path::Path, names: &[&str]) {
        for name in names {
            assert!(std::fs::symlink_metadata(dir.join(name)).is_err(), "{name} was kept");
        }
    }

    /// What another program adds to an installation afterwards, under names setup uses but did
    /// not create there: an install that only unpacks the app downloads nothing and installs no
    /// runtime.
    const LATER: [&str; 2] = [".downloads/update.bin", "runtime/theirs.txt"];

    #[tokio::test]
    async fn an_update_leaves_another_programs_entries() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        put_theirs(&dir, &LATER);
        // The wizard still treats the folder as an installation to update.
        assert_eq!(crate::folder::check(&dir, &root.path().join("data")).problem(), None);

        let created = install(&unpack_only(&dir, &APP)).await.unwrap().created.unwrap();
        for name in [".downloads", "runtime"] {
            assert!(!has(&created, name), "{name} recorded in {created:?}");
        }
        assert_theirs(&dir, &LATER);
        assert_eq!(uninstall(&dir), [".downloads", "runtime"]);
        assert_theirs(&dir, &LATER);
    }

    #[tokio::test]
    async fn a_failed_update_leaves_another_programs_entries() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        put_theirs(&dir, &LATER);
        // The new version brings a `locales` folder, and unpacking fails after it.
        let update = unpack_only(&dir, &["demido-studio.exe", "locales/en.pak", "../escape", "LICENSE"]);
        assert!(install(&update).await.is_err());
        assert!(dir.join(MARKER).is_file());
        let removable = crate::folder::removable_entries(&dir, crate::folder::installation(&dir).as_ref());
        for name in [".downloads", "runtime"] {
            assert!(!has(&removable, name), "{name} in {removable:?}");
        }

        assert_eq!(uninstall(&dir), [".downloads", "runtime"]);
        assert_theirs(&dir, &LATER);
        assert_gone(
            &dir,
            &["demido-studio.exe", "locales", "LICENSE", "install.json", MARKER],
        );
    }

    #[tokio::test]
    async fn a_second_dev_run_leaves_another_programs_entries() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&dev_only(&dir)).await.unwrap();
        put_theirs(&dir, &LATER);
        let created = install(&dev_only(&dir)).await.unwrap().created.unwrap();
        assert!(!has(&created, "runtime") && !has(&created, ".downloads"), "{created:?}");
        assert_theirs(&dir, &LATER);

        uninstall(&dir);
        assert_theirs(&dir, &LATER);
        assert_gone(&dir, &["install.json", "logs", MARKER]);
    }

    #[tokio::test]
    async fn an_update_does_not_write_into_what_another_program_added_since() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        put_theirs(&dir, &["locales/en.pak"]);
        let manifest = std::fs::read(dir.join(InstallManifest::FILE_NAME)).unwrap();
        // The new version brings a `locales` folder of its own, with a file of the same name.
        let update = unpack_only(
            &dir,
            &[
                "demido-studio.exe",
                "resources/skills/a.md",
                "LICENSE",
                "locales/en.pak",
            ],
        );
        let err = format!("{:#}", install(&update).await.unwrap_err());
        let locales = dir.join("locales");
        assert!(
            err.contains(&format!(
                "Setup did not create {} and will not change it.",
                locales.display()
            )),
            "{err}"
        );
        assert_theirs(&dir, &["locales/en.pak"]);
        assert_eq!(std::fs::read(dir.join(InstallManifest::FILE_NAME)).unwrap(), manifest);

        assert_eq!(uninstall(&dir), ["locales"]);
        assert_theirs(&dir, &["locales/en.pak"]);
    }

    /// What another program puts in the folder while a run is under way is its own by the time a
    /// step reaches that name.
    #[tokio::test]
    async fn an_update_does_not_write_into_what_another_program_adds_while_it_runs() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        let update = unpack_only(&dir, &["demido-studio.exe", "LICENSE", "locales/en.pak"]);
        let err = install_hooked(&update, |id, _| {
            if id == StepId::App {
                put_theirs(&dir, &["locales/en.pak"]);
            }
        })
        .await
        .unwrap_err();
        let err = format!("{err:#}");
        assert!(err.contains("Setup did not create "), "{err}");
        assert_theirs(&dir, &["locales/en.pak"]);

        assert_eq!(uninstall(&dir), ["locales"]);
        assert_theirs(&dir, &["locales/en.pak"]);
    }

    /// Unpacking claims every name the payload brings before it starts. What a failed update never
    /// unpacked is not setup's, so another program may take the name.
    #[tokio::test]
    async fn a_failed_update_forgets_what_it_never_unpacked() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        // The new version brings `locales` and `swiftshader`, and unpacking fails between them.
        let update = unpack_only(
            &dir,
            &["demido-studio.exe", "locales/en.pak", "../escape", "swiftshader/vk.dll"],
        );
        assert!(install(&update).await.is_err());
        assert!(dir.join("locales").join("en.pak").is_file());
        assert!(!dir.join("swiftshader").exists());
        let removable = crate::folder::removable_entries(&dir, crate::folder::installation(&dir).as_ref());
        assert!(has(&removable, "locales"), "{removable:?}");
        assert!(!has(&removable, "swiftshader"), "{removable:?}");

        put_theirs(&dir, &["swiftshader/theirs.dll"]);
        assert_eq!(uninstall(&dir), ["swiftshader"]);
        assert_theirs(&dir, &["swiftshader/theirs.dll"]);
    }

    /// The runtime step claims `runtime` before it downloads. Cancelled before creating it, the
    /// run leaves the name to whoever takes it next, and a retry never replaces what they put
    /// there.
    #[tokio::test]
    async fn a_cancelled_run_forgets_the_runtime_it_never_created() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        let runtime = InstallPlan {
            install_dir: dir.clone(),
            ..plan(InstallScope::User)
        };
        // Cancelled as the runtime step starts, so its download stops before fetching anything.
        let err = install_hooked(&runtime, |id, cancel| {
            if id == StepId::Runtime {
                cancel.cancel();
            }
        })
        .await
        .unwrap_err();
        assert!(err.to_string().starts_with("Runtime failed"), "{err:#}");
        assert!(!dir.join("runtime").exists());
        let removable = crate::folder::removable_entries(&dir, None);
        assert!(has(&removable, MARKER), "{removable:?}");
        assert!(!has(&removable, "runtime"), "{removable:?}");

        put_theirs(&dir, &["runtime/theirs.txt"]);
        let (result, failures) = install_noting_failures(&runtime).await;
        let created = result.unwrap().created.unwrap();
        assert!(!has(&created, "runtime"), "{created:?}");
        let expected = format!(
            "Setup did not create {} and will not change it.",
            dir.join("runtime").display()
        );
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert!(failures[0].starts_with(&expected), "{failures:?}");
        assert_theirs(&dir, &["runtime/theirs.txt"]);
        assert_eq!(uninstall(&dir), ["runtime"]);
        assert_theirs(&dir, &["runtime/theirs.txt"]);
    }

    #[tokio::test]
    async fn the_runtime_steps_never_replace_another_programs_runtime() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        // No step created `runtime`, so another program may have taken the name since.
        let theirs = ["runtime/node/node.exe", "runtime/uv/uv.exe"];
        put_theirs(&dir, &theirs);
        // The runtime, Node, uv and Python: each fails before it downloads or replaces anything.
        let tools = InstallPlan {
            install_dir: dir.clone(),
            node: true,
            python: true,
            ..plan(InstallScope::User)
        };
        let (result, failures) = install_noting_failures(&tools).await;
        let created = result.unwrap().created.unwrap();
        assert!(!has(&created, "runtime") && !has(&created, ".downloads"), "{created:?}");
        assert!(!failures.is_empty());
        let expected = format!(
            "Setup did not create {} and will not change it.",
            dir.join("runtime").display()
        );
        for failure in &failures {
            assert!(failure.starts_with(&expected), "{failure}");
        }
        assert_theirs(&dir, &theirs);
        assert!(!dir.join(".downloads").exists());
        assert_eq!(uninstall(&dir), ["runtime"]);
        assert_theirs(&dir, &theirs);
    }

    #[tokio::test]
    async fn downloads_removed_after_a_successful_run_are_no_longer_recorded() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        install(&unpack_only(&dir, &APP)).await.unwrap();
        // What a run whose download failed leaves: a partial file, kept so a retry resumes it.
        std::fs::create_dir_all(dir.join(".downloads")).unwrap();
        std::fs::write(dir.join(".downloads").join("partial.bin"), b"x").unwrap();
        let mut manifest = crate::folder::installation(&dir).unwrap();
        manifest.created.as_mut().unwrap().push(".downloads".into());
        manifest.save(&dir).unwrap();

        let created = install(&unpack_only(&dir, &APP)).await.unwrap().created.unwrap();
        assert!(!dir.join(".downloads").exists());
        assert!(!has(&created, ".downloads"), "{created:?}");

        // So one another program makes later is never taken for setup's.
        put_theirs(&dir, &[".downloads/update.bin"]);
        let created = install(&unpack_only(&dir, &APP)).await.unwrap().created.unwrap();
        assert!(!has(&created, ".downloads"), "{created:?}");
        assert_eq!(uninstall(&dir), [".downloads"]);
        assert_theirs(&dir, &[".downloads/update.bin"]);
    }

    #[test]
    fn finalize_records_this_plans_models_folder_and_what_setup_created() {
        let mut m = InstallManifest::new(InstallScope::Machine);
        m.models_dir = Some(demido_core::paths::starter_models_dir(InstallScope::User));
        let plan = plan(InstallScope::Machine);
        finalize(&mut m, &plan, vec!["install.json".into(), "runtime".into()]);
        assert_eq!(m.models_dir, Some(plan.models_dir));
        assert_eq!(m.created.unwrap(), ["install.json", "runtime"]);
    }
}
