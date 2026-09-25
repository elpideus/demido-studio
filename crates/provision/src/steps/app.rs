//! Unpacks the app (embedded in the installer) into the install folder.

use std::io::Cursor;

use anyhow::{Context, bail};

use crate::folder::Claim;
use crate::plan::{AppPayload, StepId};
use crate::runner::Ctx;

/// The top-level names [`install`] creates in the install folder: the payload's, then the
/// uninstaller.
pub(crate) fn entries(payload: &AppPayload) -> anyhow::Result<Vec<String>> {
    let mut names = crate::folder::payload_entries(&payload.zip)?;
    if payload.uninstaller_source.is_some() {
        names.push(demido_core::platform::exe(demido_core::brand::UNINSTALLER_BIN));
    }
    Ok(names)
}

/// Unpacks the app and copies the uninstaller. `entries` are the top-level names that creates.
pub(crate) async fn install(ctx: &Ctx<'_>, claim: &mut Claim, entries: &[String]) -> anyhow::Result<()> {
    let payload = ctx.plan.payload.clone().context("no app payload")?;
    let running = crate::system::running_app_pids(ctx.install_dir());
    if !running.is_empty() {
        bail!("Demido Studio is running from this folder. Close it and retry.");
    }
    // Unpacking overwrites what it finds, so none of it may be in another program's way.
    claim.take(entries)?;
    let dir = ctx.install_dir().to_path_buf();
    let total = payload.zip.len() as u64;
    ctx.progress(StepId::App, 0, Some(total), 0.0, "Unpacking Demido Studio");

    let zip = payload.zip.clone();
    let target = dir.clone();
    let files = tokio::task::spawn_blocking(move || -> anyhow::Result<usize> {
        let mut archive = zip::ZipArchive::new(Cursor::new(&zip[..]))?;
        let mut count = 0;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(rel) = entry.enclosed_name() else {
                bail!("payload entry {} escapes the install folder", entry.name());
            };
            let out = target.join(rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(parent) = out.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut file = std::fs::File::create(&out).with_context(|| format!("writing {}", out.display()))?;
            std::io::copy(&mut entry, &mut file)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode))?;
            }
            count += 1;
        }
        Ok(count)
    })
    .await??;
    ctx.log(StepId::App, format!("Unpacked {files} files into {}", dir.display()));

    if let Some(source) = &payload.uninstaller_source {
        let name = demido_core::platform::exe(demido_core::brand::UNINSTALLER_BIN);
        let dest = dir.join(&name);
        if source != &dest {
            std::fs::copy(source, &dest).with_context(|| format!("copying the uninstaller to {}", dest.display()))?;
        }
    }
    ctx.progress(StepId::App, total, Some(total), 0.0, "Unpacked");
    Ok(())
}
