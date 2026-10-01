//! The starter model and the search model, straight from Hugging Face into the models folder.

use std::path::PathBuf;

use anyhow::Context;
use demido_core::manifest::StarterModel;
use demido_fetch::DownloadRequest;

use crate::plan::StepId;
use crate::runner::Ctx;

pub(crate) async fn download(ctx: &Ctx<'_>) -> anyhow::Result<StarterModel> {
    let pick = ctx.plan.model.as_ref().context("no model selected")?;
    let dest = fetch(
        ctx,
        StepId::Model,
        &pick.repo,
        &pick.file,
        &pick.url(),
        pick.size,
        &pick.sha256,
    )
    .await?;
    Ok(StarterModel {
        name: pick.name.clone(),
        repo: pick.repo.clone(),
        file: pick.file.clone(),
        context_length: ctx.plan.model_context,
        path: dest,
    })
}

/// The embedding model the app searches attached files with. The app finds it in the models
/// folder by its file name, so nothing about it goes into the manifest.
pub(crate) async fn download_search(ctx: &Ctx<'_>) -> anyhow::Result<PathBuf> {
    let model = ctx.plan.search_model.as_ref().context("no search model selected")?;
    fetch(
        ctx,
        StepId::SearchModel,
        &model.repo,
        &model.file,
        &model.url(),
        model.size,
        &model.sha256,
    )
    .await
}

/// Downloads `repo`'s `file` to `<models folder>/<repo>/<file>`, verified against its size and
/// SHA-256. A reinstall finds the file already there; it is checked instead of downloaded again.
async fn fetch(
    ctx: &Ctx<'_>,
    step: StepId,
    repo: &str,
    file: &str,
    url: &str,
    size: u64,
    sha256: &str,
) -> anyhow::Result<PathBuf> {
    let dest = ctx
        .plan
        .models_dir
        .join(repo.replace('/', std::path::MAIN_SEPARATOR_STR))
        .join(file);
    let req = DownloadRequest::new(url.to_string(), &dest).size(size).sha256(sha256);
    let action = if dest.is_file() {
        format!("Checking the copy already at {}", dest.display())
    } else {
        format!("Downloading {url} to {}", dest.display())
    };
    ctx.log(step, action);
    ctx.downloader
        .download(&req, ctx.cancel, |p| {
            ctx.progress(
                step,
                p.downloaded,
                Some(size),
                p.bytes_per_second,
                format!("Downloading {file}"),
            )
        })
        .await?;
    Ok(dest)
}
