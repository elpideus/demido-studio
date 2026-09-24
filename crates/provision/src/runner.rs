use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Context;
use demido_catalog::Catalog;
use demido_core::InstallManifest;
use demido_fetch::{CancellationToken, Downloader};

use crate::events::{ProvisionEvent, StepState};
use crate::plan::{InstallPlan, StepId};
use crate::steps;

/// Shared state every step works with.
pub(crate) struct Ctx<'a> {
    pub plan: &'a InstallPlan,
    pub catalog: &'a Catalog,
    pub cancel: &'a CancellationToken,
    emit: &'a (dyn Fn(ProvisionEvent) + Send + Sync),
    pub downloader: Downloader,
    /// Scratch folder for archives, removed when the installation finishes.
    pub downloads: PathBuf,
    log: Mutex<Option<std::fs::File>>,
}

impl Ctx<'_> {
    pub fn emit(&self, event: ProvisionEvent) {
        if let Ok(mut guard) = self.log.lock() {
            if let Some(file) = guard.as_mut() {
                let line = match &event {
                    ProvisionEvent::Progress { .. } => None,
                    other => serde_json::to_string(other).ok(),
                };
                if let Some(line) = line {
                    let _ = writeln!(
                        file,
                        "{} {line}",
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
                    );
                }
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
    std::fs::create_dir_all(install_dir)
        .with_context(|| format!("creating {}", install_dir.display()))?;
    let logs = install_dir.join("logs");
    std::fs::create_dir_all(&logs)?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(logs.join("install.log"))
        .ok();

    let ctx = Ctx {
        plan,
        catalog,
        cancel: &cancel,
        emit,
        downloader: Downloader::new(&format!(
            "DemidoStudio-Setup/{}",
            demido_core::brand::VERSION
        ))?,
        downloads: install_dir.join(".downloads"),
        log: Mutex::new(log_file),
    };
    std::fs::create_dir_all(&ctx.downloads)?;

    let steps = plan.steps(catalog);
    ctx.emit(ProvisionEvent::Plan {
        steps: steps.clone(),
    });

    // Start from the previous manifest, so re-running the installer repairs instead of redoing.
    let mut manifest = InstallManifest::load(install_dir).unwrap_or_else(|_| {
        InstallManifest::new(plan.scope)
    });
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
        let result = run_step(&ctx, id, &mut manifest).await;
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
        ctx.emit(ProvisionEvent::Finished {
            success: false,
            failed,
        });
        anyhow::bail!("installation cancelled");
    }

    // Partial downloads of failed steps are kept so a retry resumes them.
    if failed.is_empty() {
        let _ = std::fs::remove_dir_all(&ctx.downloads);
    }
    ctx.emit(ProvisionEvent::Finished {
        success: failed.is_empty(),
        failed,
    });
    Ok(manifest)
}

async fn run_step(ctx: &Ctx<'_>, id: StepId, manifest: &mut InstallManifest) -> anyhow::Result<()> {
    match id {
        StepId::App => steps::app::install(ctx).await,
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
            manifest.installed_at = chrono::Utc::now();
            if manifest.models_dir.is_none() {
                manifest.models_dir = Some(ctx.plan.models_dir.clone());
            }
            manifest
                .save(ctx.install_dir())
                .context("writing install.json")?;
            ctx.log(
                id,
                format!("Wrote {}", ctx.install_dir().join(InstallManifest::FILE_NAME).display()),
            );
            Ok(())
        }
    }
}
