//! Removing Demido Studio: shortcuts, the OS registration, what setup put in the install folder
//! and, only when asked, the person's own data.

use std::path::{Path, PathBuf};

use anyhow::bail;
use demido_core::{InstallManifest, InstallScope};

use crate::folder::{RECORDS, present};

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
    let manifest = crate::folder::installation(dir);
    let scope = manifest.as_ref().map(|m| m.scope).unwrap_or(InstallScope::User);
    let mut notes = Vec::new();

    if let Ok(links) = super::shortcuts::platform::shortcut_paths(scope) {
        for link in links {
            if link.exists()
                && let Err(e) = std::fs::remove_file(&link)
            {
                notes.push(format!("Could not remove {}: {e}", link.display()));
            }
        }
    }
    if let Err(e) = super::shortcuts::platform::unregister(scope) {
        notes.push(format!("Could not remove the Installed apps entry: {e}"));
    }

    if let Some(models) = shared_models_dir(manifest.as_ref()) {
        remove_tree(&models, &mut notes);
        if let Some(parent) = models.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }

    remove_install_dir(
        dir,
        &crate::folder::removable_entries(dir, manifest.as_ref()),
        &mut notes,
    );

    if opts.remove_user_data {
        remove_tree(&demido_core::paths::user_data_dir(), &mut notes);
    }
    Ok(notes)
}

/// A machine-wide install keeps its starter model in a shared folder that belongs to the
/// installation, not to any one person. Any other recorded folder (an install over an earlier
/// per-user one can carry the person's own models folder) is left alone.
fn shared_models_dir(manifest: Option<&InstallManifest>) -> Option<PathBuf> {
    let manifest = manifest?;
    let shared = demido_core::paths::starter_models_dir(InstallScope::Machine);
    (manifest.scope == InstallScope::Machine && manifest.models_dir.as_deref() == Some(shared.as_path()))
        .then_some(shared)
}

/// Removes the `created` entries from `dir`, then `dir` itself if nothing else is left in it.
pub(crate) fn remove_install_dir(dir: &Path, created: &[String], notes: &mut Vec<String>) {
    remove_install_dir_with(dir, created, notes, remove_entry);
}

/// [`remove_install_dir`], removing each entry with `remove`. Setup's records of what it created
/// (`install.json`, the marker) go last, and only once everything else has: while anything is
/// left they list just that, so uninstalling again, or an update, finishes the job.
fn remove_install_dir_with(
    dir: &Path,
    created: &[String],
    notes: &mut Vec<String>,
    remove: impl Fn(&Path) -> std::io::Result<()>,
) {
    if !dir.exists() {
        return;
    }
    let current = std::env::current_exe().and_then(|p| p.canonicalize()).ok();
    let (records, entries): (Vec<&String>, Vec<&String>) =
        created.iter().partition(|name| RECORDS.contains(&name.as_str()));
    let mut running = None;
    let mut left = Vec::new();
    for name in entries {
        let path = dir.join(name);
        if !present(&path) {
            continue;
        }
        if current.is_some() && path.canonicalize().ok() == current {
            running = Some((name.clone(), path));
            continue;
        }
        if let Err(e) = remove(&path) {
            notes.push(format!("Could not remove {}: {e}", path.display()));
        }
        if present(&path) {
            left.push(name.clone());
        }
    }

    if left.is_empty() {
        for name in &records {
            let path = dir.join(name);
            if present(&path)
                && let Err(e) = remove(&path)
            {
                notes.push(format!("Could not remove {}: {e}", path.display()));
            }
        }
    } else {
        // The running uninstaller stays as well, so starting it again removes the rest.
        left.extend(running.take().map(|(name, _)| name));
        if let Err(e) = crate::folder::keep_records(dir, created, &left) {
            notes.push(format!("Could not record what is left in {}: {e:#}", dir.display()));
        }
        notes.push("Close any program using those files, then uninstall again to remove the rest.".into());
    }

    // What is left of setup's own is not another program's.
    let others = std::fs::read_dir(dir).is_ok_and(|mut entries| {
        entries.any(|e| {
            e.is_ok_and(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                !left.contains(&name)
                    && !records.contains(&&name)
                    && running.as_ref().is_none_or(|(_, exe)| !same_file(&e.path(), exe))
            })
        })
    });
    match running {
        // The running uninstaller cannot delete itself; it goes once it has exited.
        Some((_, exe)) => {
            crate::system::delete_after_exit(&exe, dir);
            notes.push("The uninstaller removes itself after it closes.".into());
        }
        None => {
            let _ = std::fs::remove_dir(dir);
        }
    }
    if others {
        notes.push(format!(
            "{} was kept because it also holds files that Demido Studio did not put there.",
            dir.display()
        ));
    }
}

/// Removes the file or folder at `path`.
fn remove_entry(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

fn remove_tree(path: &Path, notes: &mut Vec<String>) {
    if path.exists()
        && let Err(e) = std::fs::remove_dir_all(path)
    {
        notes.push(format!("Could not remove {}: {e}", path.display()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"x").unwrap();
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn only_what_setup_created_is_removed() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Tools");
        touch(&dir.join("runtime").join("llama").join("llama-server.exe"));
        touch(&dir.join("install.json"));
        touch(&dir.join("demido-studio.exe"));
        touch(&dir.join("Other App").join("other.exe"));
        touch(&dir.join("notes.txt"));

        let mut notes = Vec::new();
        remove_install_dir(
            &dir,
            &names(&["runtime", "install.json", "demido-studio.exe", "logs"]),
            &mut notes,
        );
        assert!(!dir.join("runtime").exists());
        assert!(!dir.join("install.json").exists());
        assert!(!dir.join("demido-studio.exe").exists());
        assert!(dir.join("Other App").join("other.exe").is_file());
        assert!(dir.join("notes.txt").is_file());
        assert!(dir.is_dir());
        assert!(notes.iter().any(|n| n.contains("was kept")), "{notes:?}");
    }

    #[test]
    fn the_folder_goes_once_it_is_empty() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        touch(&dir.join("resources").join("skills").join("a.md"));
        touch(&dir.join("LICENSE"));
        let mut notes = Vec::new();
        remove_install_dir(&dir, &names(&["resources", "LICENSE"]), &mut notes);
        assert!(!dir.exists());
        assert!(notes.is_empty(), "{notes:?}");
    }

    /// Only builds that were never released wrote a manifest without `created`: like one setup
    /// cannot read, it says nothing about what is setup's, so the uninstaller takes only itself,
    /// and only when it is the one that was started.
    #[test]
    fn a_manifest_that_does_not_record_what_setup_created_removes_only_the_running_uninstaller() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        let uninstaller = demido_core::platform::exe(demido_core::brand::UNINSTALLER_BIN);
        touch(&dir.join("logs").join("install.log"));
        touch(&dir.join("chats.db"));
        touch(&dir.join(&uninstaller));
        let mut manifest = InstallManifest::new(InstallScope::User);
        manifest.created = None;
        manifest.save(&dir).unwrap();
        // `uninstall` reads it as no installation at all.
        assert_eq!(crate::folder::installation(&dir), None);

        // These tests are what runs, so that file may be another program's.
        let mut notes = Vec::new();
        remove_install_dir(
            &dir,
            &crate::folder::removable_entries(&dir, Some(&manifest)),
            &mut notes,
        );
        assert!(dir.join(&uninstaller).is_file());

        let started = dir.join(&uninstaller);
        remove_install_dir(
            &dir,
            &crate::folder::removable_entries_with(&dir, Some(&manifest), Some(&started)),
            &mut notes,
        );
        assert!(!dir.join(&uninstaller).exists());
        assert!(dir.join("logs").join("install.log").is_file());
        assert!(dir.join("chats.db").is_file());
        assert!(dir.join("install.json").is_file());
    }

    const MARKER: &str = crate::folder::MARKER;

    /// The names in `dir`, sorted.
    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// What the uninstaller removes from `dir`.
    fn removable(dir: &Path) -> Vec<String> {
        crate::folder::removable_entries(dir, crate::folder::installation(dir).as_ref())
    }

    /// An installation with `install.json`, the marker of an update that did not finish, and
    /// what each lists.
    fn installed(dir: &Path) {
        for file in [
            "logs/install.log",
            "resources/skills/a.md",
            "runtime/llama/llama-server.exe",
            "LICENSE",
            "locales/en.pak",
        ] {
            touch(&dir.join(file));
        }
        let mut manifest = InstallManifest::new(InstallScope::User);
        manifest.created = Some(names(&[
            MARKER,
            "LICENSE",
            "install.json",
            "logs",
            "resources",
            "runtime",
        ]));
        manifest.save(dir).unwrap();
        std::fs::write(dir.join(MARKER), br#"{"created": [".demido-setup", "locales"]}"#).unwrap();
    }

    #[test]
    fn what_cannot_be_removed_stays_on_record() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        installed(&dir);
        touch(&dir.join("notes.txt"));

        // Something holds `logs`.
        let mut notes = Vec::new();
        remove_install_dir_with(&dir, &removable(&dir), &mut notes, |path| {
            if path.ends_with("logs") {
                Err(std::io::Error::other("in use"))
            } else {
                remove_entry(path)
            }
        });
        // The records stay, listing only what is left.
        assert_eq!(listing(&dir), [MARKER, "install.json", "logs", "notes.txt"]);
        let recorded = crate::folder::installation(&dir).unwrap().created.unwrap();
        assert_eq!(recorded, [MARKER, "install.json", "logs"]);
        assert_eq!(removable(&dir), recorded);
        assert!(
            notes
                .iter()
                .any(|n| n.starts_with("Could not remove ") && n.ends_with(": in use")),
            "{notes:?}"
        );

        // Another program takes a name already removed. Uninstalling again, once nothing holds
        // `logs`, finishes the job and leaves that.
        touch(&dir.join("resources").join("theirs.txt"));
        let mut notes = Vec::new();
        remove_install_dir(&dir, &removable(&dir), &mut notes);
        assert_eq!(listing(&dir), ["notes.txt", "resources"]);
        assert!(notes.iter().all(|n| n.ends_with("did not put there.")), "{notes:?}");
    }

    /// Opened without sharing, a file cannot be deleted on Windows until it is closed.
    #[cfg(windows)]
    #[test]
    fn a_file_in_use_keeps_the_record_until_uninstalling_again() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Demido Studio");
        installed(&dir);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(dir.join("logs").join("install.log"))
            .unwrap();
        let mut notes = Vec::new();
        remove_install_dir(&dir, &removable(&dir), &mut notes);
        assert_eq!(listing(&dir), [MARKER, "install.json", "logs"]);
        assert_eq!(removable(&dir), [MARKER, "install.json", "logs"]);

        drop(held);
        remove_install_dir(&dir, &removable(&dir), &mut notes);
        assert!(!dir.exists());
    }

    #[test]
    fn only_the_shared_machine_models_folder_is_removed() {
        let shared = demido_core::paths::starter_models_dir(InstallScope::Machine);
        let mut manifest = InstallManifest::new(InstallScope::Machine);
        manifest.models_dir = Some(shared.clone());
        assert_eq!(shared_models_dir(Some(&manifest)), Some(shared.clone()));

        // Carried over from an earlier per-user install: the person's own models.
        manifest.models_dir = Some(demido_core::paths::starter_models_dir(InstallScope::User));
        assert_eq!(shared_models_dir(Some(&manifest)), None);

        let mut user = InstallManifest::new(InstallScope::User);
        user.models_dir = Some(shared);
        assert_eq!(shared_models_dir(Some(&user)), None);
        assert_eq!(shared_models_dir(None), None);
    }
}
