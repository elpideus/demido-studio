//! The starter model, straight from Hugging Face into the models folder.

use anyhow::Context;
use demido_core::manifest::StarterModel;
use demido_fetch::DownloadRequest;

use crate::plan::StepId;
use crate::runner::Ctx;

pub(crate) async fn download(ctx: &Ctx<'_>) -> anyhow::Result<StarterModel> {
    let pick = ctx.plan.model.as_ref().context("no model selected")?;
    let dest = ctx
        .plan
        .models_dir
        .join(pick.repo.replace('/', std::path::MAIN_SEPARATOR_STR))
        .join(&pick.file);
    let req = DownloadRequest::new(pick.url(), &dest)
        .size(pick.size)
        .sha256(&pick.sha256);
    // A reinstall finds the model already there; it is checked instead of downloaded again.
    let action = if dest.is_file() {
        format!("Checking the copy already at {}", dest.display())
    } else {
        format!("Downloading {} to {}", pick.url(), dest.display())
    };
    ctx.log(StepId::Model, action);
    ctx.downloader
        .download(&req, ctx.cancel, |p| {
            ctx.progress(
                StepId::Model,
                p.downloaded,
                Some(pick.size),
                p.bytes_per_second,
                format!("Downloading {}", pick.file),
            )
        })
        .await?;
    Ok(StarterModel {
        name: pick.name.clone(),
        repo: pick.repo.clone(),
        file: pick.file.clone(),
        context_length: ctx.plan.model_context,
        path: dest,
    })
}
