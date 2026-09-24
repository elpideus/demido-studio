pub(crate) mod app;
pub(crate) mod model;
pub(crate) mod python;
pub(crate) mod runtime;
pub(crate) mod shortcuts;
pub(crate) mod tools;
pub mod uninstall;

use std::path::{Path, PathBuf};

use demido_fetch::DownloadRequest;

use crate::plan::StepId;
use crate::runner::Ctx;

/// Downloads into the scratch folder, reporting progress as part of `step`. `offset` and
/// `step_total` let several files share one step's progress bar.
pub(crate) async fn fetch(
    ctx: &Ctx<'_>,
    step: StepId,
    url: &str,
    size: u64,
    sha256: &str,
    offset: u64,
    step_total: u64,
) -> anyhow::Result<PathBuf> {
    let name = url.rsplit('/').next().unwrap_or("download");
    let dest = ctx.downloads.join(name);
    let req = DownloadRequest::new(url, &dest).size(size).sha256(sha256);
    ctx.log(step, format!("Downloading {url}"));
    ctx.downloader
        .download(&req, ctx.cancel, |p| {
            ctx.progress(
                step,
                offset + p.downloaded,
                Some(step_total),
                p.bytes_per_second,
                format!("Downloading {name}"),
            )
        })
        .await?;
    Ok(dest)
}

/// Replaces `target` with `staging` (both folders).
pub(crate) fn swap_dir(staging: &Path, target: &Path) -> anyhow::Result<()> {
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|e| {
            anyhow::anyhow!(
                "could not replace {}: {e}. Close any program using it and retry.",
                target.display()
            )
        })?;
    }
    std::fs::rename(staging, target)?;
    Ok(())
}

/// A fresh, empty folder next to `target` to build into.
pub(crate) fn staging_for(target: &Path) -> anyhow::Result<PathBuf> {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "dir".into());
    let staging = target.with_file_name(format!("{name}.new"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    Ok(staging)
}

/// `path` relative to the install folder, for the manifest.
pub(crate) fn relative(ctx: &Ctx<'_>, path: &Path) -> PathBuf {
    path.strip_prefix(ctx.install_dir())
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.to_path_buf())
}
