//! Node.js and uv: single archives with one executable we care about.

use std::time::Duration;

use anyhow::Context;
use demido_catalog::ToolDist;
use demido_core::manifest::ToolInfo;
use demido_core::{Arch, Os};

use crate::plan::StepId;
use crate::process::{self, Run};
use crate::runner::Ctx;

pub(crate) async fn install_node(ctx: &Ctx<'_>, existing: Option<&ToolInfo>) -> anyhow::Result<ToolInfo> {
    install(
        ctx,
        StepId::Node,
        &ctx.catalog.runtimes.node,
        "node",
        &demido_core::platform::exe("node"),
        existing,
    )
    .await
}

pub(crate) async fn install_uv(ctx: &Ctx<'_>, existing: Option<&ToolInfo>) -> anyhow::Result<ToolInfo> {
    install(
        ctx,
        StepId::Uv,
        &ctx.catalog.runtimes.uv,
        "uv",
        &demido_core::platform::exe("uv"),
        existing,
    )
    .await
}

async fn install(
    ctx: &Ctx<'_>,
    step: StepId,
    dist: &ToolDist,
    folder: &str,
    exe_name: &str,
    existing: Option<&ToolInfo>,
) -> anyhow::Result<ToolInfo> {
    let asset = dist
        .for_platform(Os::current(), Arch::current())
        .with_context(|| format!("{folder} has no build for this platform"))?;
    let target = ctx.install_dir().join("runtime").join(folder);

    if let Some(info) = existing
        && info.version == dist.version
        && ctx.install_dir().join(&info.exe).is_file()
    {
        ctx.log(step, format!("{folder} {} is already installed", dist.version));
        ctx.progress(step, asset.size, Some(asset.size), 0.0, "Already installed");
        return Ok(info.clone());
    }

    let archive = super::fetch(ctx, step, &asset.url, asset.size, &asset.sha256, 0, asset.size).await?;
    ctx.progress(step, asset.size, Some(asset.size), 0.0, format!("Unpacking {folder}"));
    let staging = super::staging_for(&target)?;
    let staged = staging.clone();
    let name = exe_name.to_string();
    let exe_in_staging = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        demido_fetch::extract(&archive, &staged)?;
        demido_fetch::find_file(&staged, &name).with_context(|| format!("{name} is missing from the archive"))
    })
    .await??;

    let version = process::run(
        Run {
            program: &exe_in_staging,
            args: vec!["--version".into()],
            envs: vec![],
            cwd: None,
            timeout: Duration::from_secs(30),
        },
        ctx.cancel,
        |line| ctx.log(step, line),
    )
    .await
    .with_context(|| format!("{folder} did not start"))?;
    tracing::debug!("{folder} --version: {}", version.trim());

    let rel = exe_in_staging.strip_prefix(&staging)?.to_path_buf();
    super::swap_dir(&staging, &target)?;
    Ok(ToolInfo {
        version: dist.version.clone(),
        exe: super::relative(ctx, &target.join(rel)),
    })
}
