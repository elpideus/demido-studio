//! Removing Demido Studio: shortcuts, the OS registration, the install folder and, only when
//! asked, the person's own data.

use std::path::{Path, PathBuf};

use anyhow::bail;
use demido_core::{InstallManifest, InstallScope};

#[derive(Clone, Debug)]
pub struct UninstallOptions {
    pub install_dir: PathBuf,
    /// Also delete chats, settings, skills and downloaded models.
    pub remove_user_data: bool,
}

/// Uninstalls. Returns notes about anything that could not be removed right away (for example
/// the running uninstaller itself, which is scheduled for deletion instead).
pub fn uninstall(opts: &UninstallOptions) -> anyhow::Result<Vec<String>> {
    let dir = &opts.install_dir;
    if !crate::system::running_app_pids(dir).is_empty() {
        bail!("Demido Studio is still running. Close it and try again.");
    }
    let manifest = InstallManifest::load(dir).ok();
    let scope = manifest
        .as_ref()
        .map(|m| m.scope)
        .unwrap_or(InstallScope::User);
    let mut notes = Vec::new();

    if let Ok(links) = super::shortcuts::platform::shortcut_paths(scope) {
        for link in links {
            if link.exists() {
                if let Err(e) = std::fs::remove_file(&link) {
                    notes.push(format!("Could not remove {}: {e}", link.display()));
                }
            }
        }
    }
    if let Err(e) = super::shortcuts::platform::unregister(scope) {
        notes.push(format!("Could not remove the Installed apps entry: {e}"));
    }

    // A machine-wide install keeps its starter model in a shared folder that belongs to the
    // installation, not to any one person.
    if scope == InstallScope::Machine {
        if let Some(models) = manifest.as_ref().and_then(|m| m.models_dir.clone()) {
            remove_tree(&models, &mut notes);
            if let Some(parent) = models.parent() {
                let _ = std::fs::remove_dir(parent);
            }
        }
    }

    remove_install_dir(dir, &mut notes)?;

    if opts.remove_user_data {
        remove_tree(&demido_core::paths::user_data_dir(), &mut notes);
    }
    Ok(notes)
}

fn remove_install_dir(dir: &Path, notes: &mut Vec<String>) -> anyhow::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let current = std::env::current_exe().ok();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if current.as_deref() == Some(path.as_path()) {
            continue;
        }
        if path.is_dir() {
            remove_tree(&path, notes);
        } else if let Err(e) = std::fs::remove_file(&path) {
            notes.push(format!("Could not remove {}: {e}", path.display()));
        }
    }
    match current {
        Some(exe) if exe.starts_with(dir) => {
            crate::system::delete_after_exit(dir);
            notes.push("The uninstaller removes itself after it closes.".into());
        }
        _ => {
            let _ = std::fs::remove_dir(dir);
        }
    }
    Ok(())
}

fn remove_tree(path: &Path, notes: &mut Vec<String>) {
    if path.exists() {
        if let Err(e) = std::fs::remove_dir_all(path) {
            notes.push(format!("Could not remove {}: {e}", path.display()));
        }
    }
}
