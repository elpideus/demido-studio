//! Fetches the llama.cpp build for the chosen backend and checks it starts.

use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use demido_core::manifest::RuntimeInfo;

use crate::plan::StepId;
use crate::process::{self, Run};
use crate::runner::Ctx;

const STEP: StepId = StepId::Runtime;

pub(crate) async fn install(
    ctx: &Ctx<'_>,
    existing: Option<&RuntimeInfo>,
) -> anyhow::Result<RuntimeInfo> {
    let llama = &ctx.catalog.runtimes.llama_cpp;
    let variant = llama
        .variant(&ctx.plan.variant)
        .with_context(|| format!("unknown runtime variant {}", ctx.plan.variant))?;
    let target = ctx.install_dir().join("runtime").join("llama");

    if let Some(info) = existing {
        let server = ctx.install_dir().join(&info.server);
        if info.variant == variant.id && info.release == llama.release && server.is_file() {
            ctx.log(STEP, format!("{} is already installed", llama.label));
            let size = variant.download_size();
            ctx.progress(STEP, size, Some(size), 0.0, "Already installed");
            return Ok(info.clone());
        }
    }

    let total = variant.download_size();
    let mut archives = Vec::new();
    let mut offset = 0;
    for asset in &variant.assets {
        let path = super::fetch(
            ctx,
            STEP,
            &llama.asset_url(asset),
            asset.size,
            &asset.sha256,
            offset,
            total,
        )
        .await?;
        offset += asset.size;
        archives.push(path);
    }

    ctx.progress(STEP, total, Some(total), 0.0, "Unpacking the runtime");
    let staging = super::staging_for(&target)?;
    let server_name = demido_core::platform::exe("llama-server");
    let staged = staging.clone();
    let server_in_staging = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let (main, extra) = archives.split_first().context("variant has no assets")?;
        demido_fetch::extract(main, &staged)?;
        let server = demido_fetch::find_file(&staged, &server_name)
            .with_context(|| format!("{server_name} is missing from the runtime archive"))?;
        // Companion archives (CUDA runtime libraries) must sit next to the server binary.
        let bin_dir = server.parent().unwrap_or(&staged).to_path_buf();
        for archive in extra {
            demido_fetch::extract(archive, &bin_dir)?;
        }
        Ok(server)
    })
    .await??;

    let rel_server = server_in_staging
        .strip_prefix(&staging)
        .context("server outside staging")?
        .to_path_buf();
    verify(ctx, &server_in_staging, variant.backend.is_gpu()).await?;

    super::swap_dir(&staging, &target)?;
    let server = target.join(&rel_server);
    Ok(RuntimeInfo {
        backend: variant.backend,
        variant: variant.id.clone(),
        release: llama.release.clone(),
        dir: super::relative(ctx, &target),
        server: super::relative(ctx, &server),
    })
}

/// Runs `llama-server --version` and `--list-devices`, so a runtime that cannot start (missing
/// libraries, unsupported CPU) fails here rather than on the first question.
async fn verify(ctx: &Ctx<'_>, server: &Path, expects_gpu: bool) -> anyhow::Result<()> {
    ctx.progress(STEP, 1, Some(1), 0.0, "Checking the runtime starts");
    let version = process::run(
        Run {
            program: server,
            args: vec!["--version".into()],
            envs: vec![],
            cwd: server.parent(),
            timeout: Duration::from_secs(60),
        },
        ctx.cancel,
        |line| ctx.log(STEP, line),
    )
    .await
    .context("the runtime did not start")?;
    tracing::debug!("llama-server --version: {version}");

    let devices = process::run(
        Run {
            program: server,
            args: vec!["--list-devices".into()],
            envs: vec![],
            cwd: server.parent(),
            timeout: Duration::from_secs(60),
        },
        ctx.cancel,
        |line| ctx.log(STEP, line),
    )
    .await
    .unwrap_or_default();
    let gpu_visible = devices
        .lines()
        .any(|l| l.contains("MiB") && !l.to_ascii_lowercase().contains("cpu"));
    if expects_gpu && !gpu_visible {
        ctx.log(
            STEP,
            "Warning: the runtime could not see a GPU. Models will run on the CPU until the \
             graphics driver is updated.",
        );
    }
    Ok(())
}
