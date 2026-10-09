//! The starter model and the search model, straight from Hugging Face into the models folder.

use std::path::PathBuf;

use anyhow::Context;
use demido_core::manifest::StarterModel;
use demido_fetch::DownloadRequest;

use crate::plan::StepId;
use crate::runner::Ctx;

/// The starter model, then its projector: the app loads the projector it finds next to a model,
/// which lets the model read pictures and sound.
pub(crate) async fn download(ctx: &Ctx<'_>) -> anyhow::Result<StarterModel> {
    let pick = ctx.plan.model.as_ref().context("no model selected")?;
    let mut bar = Bar::new(StepId::Model, pick.download_size());
    let dest = fetch(
        ctx,
        &mut bar,
        Pinned {
            repo: &pick.repo,
            file: &pick.file,
            size: pick.size,
            sha256: &pick.sha256,
        },
    )
    .await?;
    if let Some(projector) = &pick.projector {
        fetch(
            ctx,
            &mut bar,
            Pinned {
                repo: &pick.repo,
                file: &projector.file,
                size: projector.size,
                sha256: &projector.sha256,
            },
        )
        .await?;
    }
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
    let mut bar = Bar::new(StepId::SearchModel, model.size);
    fetch(
        ctx,
        &mut bar,
        Pinned {
            repo: &model.repo,
            file: &model.file,
            size: model.size,
            sha256: &model.sha256,
        },
    )
    .await
}

/// A file of a Hugging Face repo, pinned to its size and SHA-256.
struct Pinned<'a> {
    repo: &'a str,
    file: &'a str,
    size: u64,
    sha256: &'a str,
}

/// One step's progress over the files it downloads one after another.
struct Bar {
    step: StepId,
    /// Bytes of the files already done.
    done: u64,
    total: u64,
}

impl Bar {
    fn new(step: StepId, total: u64) -> Self {
        Self { step, done: 0, total }
    }
}

/// Downloads `file` to `<models folder>/<repo>/<file>`, verified against its size and SHA-256.
/// A reinstall finds the file already there; it is checked instead of downloaded again.
async fn fetch(ctx: &Ctx<'_>, bar: &mut Bar, file: Pinned<'_>) -> anyhow::Result<PathBuf> {
    let url = format!("https://huggingface.co/{}/resolve/main/{}", file.repo, file.file);
    let dest = ctx
        .plan
        .models_dir
        .join(file.repo.replace('/', std::path::MAIN_SEPARATOR_STR))
        .join(file.file);
    let req = DownloadRequest::new(url.clone(), &dest)
        .size(file.size)
        .sha256(file.sha256);
    let action = if dest.is_file() {
        format!("Checking the copy already at {}", dest.display())
    } else {
        format!("Downloading {url} to {}", dest.display())
    };
    ctx.log(bar.step, action);
    let (step, done, total) = (bar.step, bar.done, bar.total);
    ctx.downloader
        .download(&req, ctx.cancel, |p| {
            ctx.progress(
                step,
                done + p.downloaded,
                Some(total),
                p.bytes_per_second,
                format!("Downloading {}", file.file),
            )
        })
        .await?;
    bar.done += file.size;
    Ok(dest)
}
