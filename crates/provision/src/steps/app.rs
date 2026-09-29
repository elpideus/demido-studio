//! Puts the app, embedded in the installer, into the install folder.
//!
//! The payload is unpacked into a staging folder first. Only then does each of its top-level
//! entries take the place of the one already in the folder, which moves aside into a backup
//! folder. Renaming works on Windows where overwriting a file in use fails, and a swap that fails
//! partway is undone by moving everything back, so an update that cannot finish leaves the
//! previous version whole instead of a mix of both. Replacing whole entries also drops the files
//! the new version no longer ships inside them.

use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use demido_core::brand;

use crate::folder::{Claim, present};
use crate::plan::{AppPayload, StepId};
use crate::runner::Ctx;

/// Scratch folder in the install folder that the payload is unpacked into.
pub(crate) const STAGING: &str = ".update-new";
/// Scratch folder in the install folder where the entries being replaced wait until the new ones
/// are in place.
pub(crate) const BACKUP: &str = ".update-old";

/// Why the app step stops when the uninstaller it copied does not match the release signature.
pub(crate) const SETUP_CHANGED: &str = "The setup file changed while it ran, so the update was stopped. Run it again.";

/// A swap that failed and could not put everything back: the install folder holds part of each
/// version, and what could not go back waits in the backup folder. Running setup again finishes
/// the update, since it brings the whole app. The runner tells the installer's window, which then
/// does not say the previous version is still installed.
#[derive(Debug)]
pub(crate) struct SwapIncomplete {
    /// Why the swap failed.
    failure: String,
    /// The entries the folder does not hold as before.
    stuck: Vec<String>,
}

impl std::fmt::Display for SwapIncomplete {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} Setup could not undo its changes to {}; running Setup again finishes the update.",
            self.failure,
            self.stuck.join(", ")
        )
    }
}

impl std::error::Error for SwapIncomplete {}

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
    // The swap replaces what it finds under these names and creates the scratch folders, so none
    // of them may be in another program's way.
    let mut names = vec![STAGING, BACKUP];
    names.extend(entries.iter().map(String::as_str));
    claim.take(&names)?;
    let dir = ctx.install_dir().to_path_buf();
    let (staging, backup) = (dir.join(STAGING), dir.join(BACKUP));
    // What a run that stopped partway, or a backup that could not be removed, left behind. None of
    // it is needed again: this run brings the whole app.
    for scratch in [&staging, &backup] {
        clear(scratch)?;
    }

    let total = payload.zip.len() as u64;
    ctx.progress(StepId::App, 0, Some(total), 0.0, "Unpacking Demido Studio");
    let files = match stage(ctx, &payload, &staging).await {
        Ok(files) => files,
        Err(err) => {
            discard(&staging);
            return Err(err);
        }
    };
    ctx.log(
        StepId::App,
        format!("Unpacked {files} files into {}", staging.display()),
    );

    ctx.progress(StepId::App, total, Some(total), 0.0, "Replacing the previous version");
    // The uninstaller is left out when setup runs as that very file.
    let swapped: Vec<String> = entries
        .iter()
        .filter(|name| present(&staging.join(name)))
        .cloned()
        .collect();
    let target = dir.clone();
    let note = tokio::task::spawn_blocking(move || swap(&target, &staging, &backup, &swapped)).await??;
    if let Some(note) = note {
        ctx.log(StepId::App, note);
    }
    ctx.log(StepId::App, format!("Demido Studio is in place in {}", dir.display()));
    ctx.progress(StepId::App, total, Some(total), 0.0, "Unpacked");
    Ok(())
}

/// Unpacks the payload and copies the uninstaller into `staging`. Returns how many files it
/// wrote.
async fn stage(ctx: &Ctx<'_>, payload: &AppPayload, staging: &Path) -> anyhow::Result<usize> {
    let total = payload.zip.len() as u64;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<u64>();
    let (zip, target) = (payload.zip.clone(), staging.to_path_buf());
    let unpacking = tokio::task::spawn_blocking(move || {
        unpack(&zip, &target, |read| {
            let _ = tx.send(read);
        })
    });
    // The channel closes when unpacking ends, however it ends.
    while let Some(read) = rx.recv().await {
        ctx.progress(
            StepId::App,
            read.min(total),
            Some(total),
            0.0,
            "Unpacking Demido Studio",
        );
    }
    let mut files = unpacking.await??;

    if let Some(source) = &payload.uninstaller_source {
        let name = demido_core::platform::exe(brand::UNINSTALLER_BIN);
        let installed = ctx.install_dir().join(&name);
        if !same_file(source, &installed) {
            let dest = staging.join(&name);
            std::fs::copy(source, &dest).with_context(|| format!("copying the uninstaller to {}", dest.display()))?;
            // The copy is checked, where only setup writes, never the file it came from: that may
            // sit in a folder the user can write to, and be replaced right after a check.
            let checked = check_uninstaller(
                source,
                &dest,
                payload.signature_required,
                demido_core::signature::PUBLIC_KEY,
                &brand::setup_file_name(brand::VERSION),
                brand::VERSION,
            );
            if let Err(err) = checked {
                ctx.log(StepId::App, format!("The uninstaller was not accepted: {err:#}"));
                bail!(SETUP_CHANGED);
            }
            files += 1;
        }
    }
    Ok(files)
}

/// Checks `copy`, the uninstaller just copied from `source`, against the release signature next
/// to `source` (`<source>.sig`), made by `key` for `file_name` at `version`. Without a signature
/// there, fails only when one is `required`.
fn check_uninstaller(
    source: &Path,
    copy: &Path,
    required: bool,
    key: &str,
    file_name: &str,
    version: &str,
) -> anyhow::Result<()> {
    let sig = signature_path(source);
    let signature = match std::fs::read_to_string(&sig) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound && !required => return Ok(()),
        Err(e) => bail!("could not read {}: {e}", sig.display()),
    };
    demido_core::signature::verify_file(copy, &signature, key, file_name, version)
}

/// Where the app puts the release signature of an installer it stages: the same path plus `.sig`.
fn signature_path(installer: &Path) -> PathBuf {
    let mut path = installer.as_os_str().to_os_string();
    path.push(".sig");
    PathBuf::from(path)
}

/// Unpacks `zip` into `target`, calling `progress` now and then with the compressed bytes read so
/// far. Returns how many files it wrote.
fn unpack(zip: &[u8], target: &Path, mut progress: impl FnMut(u64)) -> anyhow::Result<usize> {
    std::fs::create_dir_all(target).with_context(|| format!("creating {}", target.display()))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(zip))?;
    // About a hundred updates over the whole payload, however many files it holds.
    let every = (zip.len() as u64 / 100).max(1);
    let (mut count, mut read, mut reported) = (0, 0u64, 0u64);
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else {
            bail!("payload entry {} escapes the install folder", entry.name());
        };
        let out = target.join(rel);
        read += entry.compressed_size();
        if read - reported >= every {
            progress(read);
            reported = read;
        }
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
}

/// What the swap did to one entry, so it can be undone.
struct Moved {
    name: String,
    /// The entry the folder held went into the backup.
    aside: bool,
    /// The new entry went into the folder.
    placed: bool,
}

/// Moves each of `names` from `staging` into `dir`, moving the entry `dir` holds under that name
/// into `backup` first, then removes both scratch folders. When a move fails, everything moved so
/// far goes back where it came from, so `dir` holds the previous version as it was. Returns a note
/// when the backup could not be removed afterwards (a file in it still held open, say): it stays
/// claimed, so the next update or the uninstaller removes it.
fn swap(dir: &Path, staging: &Path, backup: &Path, names: &[String]) -> anyhow::Result<Option<String>> {
    if let Err(e) = std::fs::create_dir_all(backup) {
        discard(staging);
        bail!("Could not create {}: {e}", backup.display());
    }
    let mut done: Vec<Moved> = Vec::new();
    for name in names {
        if let Err(err) = swap_one(dir, staging, backup, name, &mut done) {
            let stuck = roll_back(dir, staging, backup, &done);
            discard(staging);
            // Goes only when everything in it went back; otherwise what it holds is the previous
            // version, which the next run replaces.
            let _ = std::fs::remove_dir(backup);
            if stuck.is_empty() {
                return Err(err);
            }
            return Err(SwapIncomplete {
                failure: format!("{err:#}"),
                stuck,
            }
            .into());
        }
    }
    discard(staging);
    Ok(remove(backup).err().map(|e| {
        format!(
            "Could not remove {}: {e}. The next update or the uninstaller removes it.",
            backup.display()
        )
    }))
}

/// Swaps one entry, recording in `done` what it moved before anything can fail.
fn swap_one(dir: &Path, staging: &Path, backup: &Path, name: &str, done: &mut Vec<Moved>) -> anyhow::Result<()> {
    let current = dir.join(name);
    let failed = |e: std::io::Error| {
        anyhow::anyhow!(
            "Could not replace {}: {e}. Close any program using it and try again.",
            current.display()
        )
    };
    done.push(Moved {
        name: name.to_string(),
        aside: false,
        placed: false,
    });
    let moved = done.last_mut().expect("just pushed");
    if present(&current) {
        rename(&current, &backup.join(name)).map_err(failed)?;
        moved.aside = true;
    }
    rename(&staging.join(name), &current).map_err(failed)?;
    moved.placed = true;
    Ok(())
}

/// Undoes `done`, newest first: each new entry goes back to the staging folder and each previous
/// one back into the folder. Returns the names the folder does not hold as before.
fn roll_back(dir: &Path, staging: &Path, backup: &Path, done: &[Moved]) -> Vec<String> {
    let mut stuck = Vec::new();
    for moved in done.iter().rev() {
        let current = dir.join(&moved.name);
        // Out of the way one way or another: the previous entry needs its name back.
        let new_gone =
            !moved.placed || rename(&current, &staging.join(&moved.name)).is_ok() || remove(&current).is_ok();
        let old_back = !moved.aside || (new_gone && rename(&backup.join(&moved.name), &current).is_ok());
        if !new_gone || !old_back {
            stuck.push(moved.name.clone());
        }
    }
    stuck
}

/// Removes what an earlier run left at `path`, if anything.
fn clear(path: &Path) -> anyhow::Result<()> {
    if present(path) {
        remove(path).map_err(|e| {
            anyhow::anyhow!(
                "Could not remove {}, which an earlier update left: {e}. Close any program using it and try again.",
                path.display()
            )
        })?;
    }
    Ok(())
}

/// Removes `path` if it is there, as far as it can: a scratch folder that stays behind is still
/// claimed, so the next run or the uninstaller removes it.
fn discard(path: &Path) {
    if present(path) {
        let _ = remove(path);
    }
}

/// Removes the file or folder at `path`. A link is removed, never what it points to.
fn remove(path: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path)?.is_dir() {
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

/// `std::fs::rename`, tried again for a couple of seconds when it fails. Right after unpacking, an
/// antivirus scanner may still hold a new file open, and Windows refuses to move a folder while
/// any file in it is open; such a hold lasts a moment. In tests, a rename into a path listed in
/// [`FAIL_RENAMES_INTO`] fails at once instead, once per listing, so a test can stop the swap at
/// any entry on every platform.
fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    {
        let mut failing = FAIL_RENAMES_INTO.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = failing.iter().position(|path| path == to) {
            failing.remove(i);
            return Err(std::io::Error::other("the test made this rename fail"));
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut pause = Duration::from_millis(50);
    loop {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            // Nothing to move, or something in the way: waiting changes neither.
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::AlreadyExists) => return Err(e),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => {
                std::thread::sleep(pause);
                pause = (pause * 2).min(Duration::from_millis(400));
            }
        }
    }
}

/// Paths a rename into fails, once for each time a path is listed. Tests list paths inside their
/// own temporary folders, so tests running at the same time do not trip each other.
#[cfg(test)]
pub(crate) static FAIL_RENAMES_INTO: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Makes the next rename into `path` fail.
#[cfg(test)]
pub(crate) fn fail_rename_into(path: &Path) {
    FAIL_RENAMES_INTO
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(path.to_path_buf());
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Every file under `dir`, by its path relative to `dir`, with its contents.
    fn tree(dir: &Path) -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            for entry in std::fs::read_dir(&d).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    let rel = path.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
                    files.insert(rel, std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        files
    }

    /// [`tree`], without the staging folder.
    fn installed(dir: &Path) -> BTreeMap<String, String> {
        tree(dir)
            .into_iter()
            .filter(|(path, _)| !path.starts_with(&format!("{STAGING}/")))
            .collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|n| n.to_string()).collect()
    }

    /// An install folder holding `old`, and the staging folder holding `new`.
    fn prepared(root: &Path, old: &[(&str, &str)], new: &[(&str, &str)]) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = root.join("Demido Studio");
        let staging = dir.join(STAGING);
        for (file, content) in old {
            write(&dir.join(file), content);
        }
        for (file, content) in new {
            write(&staging.join(file), content);
        }
        (dir, staging)
    }

    #[test]
    fn the_swap_replaces_whole_entries_and_removes_the_scratch_folders() {
        let root = tempfile::tempdir().unwrap();
        let (dir, staging) = prepared(
            root.path(),
            &[("app.exe", "v1"), ("resources/old.md", "v1"), ("notes.txt", "theirs")],
            &[("app.exe", "v2"), ("resources/new.md", "v2"), ("locales/en.pak", "v2")],
        );
        let backup = dir.join(BACKUP);
        let note = swap(&dir, &staging, &backup, &names(&["app.exe", "locales", "resources"])).unwrap();
        assert_eq!(note, None);
        assert_eq!(
            tree(&dir),
            BTreeMap::from([
                ("app.exe".into(), "v2".into()),
                ("locales/en.pak".into(), "v2".into()),
                ("notes.txt".into(), "theirs".into()),
                ("resources/new.md".into(), "v2".into()),
            ])
        );
        assert!(!staging.exists());
        assert!(!backup.exists());
    }

    #[test]
    fn a_failed_swap_puts_everything_back() {
        let root = tempfile::tempdir().unwrap();
        let (dir, staging) = prepared(
            root.path(),
            &[("app.exe", "v1"), ("resources/a.md", "v1"), ("notes.txt", "theirs")],
            &[("app.exe", "v2"), ("locales/en.pak", "v2"), ("resources/b.md", "v2")],
        );
        let before = installed(&dir);
        let backup = dir.join(BACKUP);
        // `app.exe` and the new `locales` are in place by the time `resources` fails.
        fail_rename_into(&dir.join("resources"));
        let err = swap(&dir, &staging, &backup, &names(&["app.exe", "locales", "resources"])).unwrap_err();
        // Everything went back, so the previous version is whole.
        assert!(err.downcast_ref::<SwapIncomplete>().is_none(), "{err:#}");
        let err = err.to_string();
        assert!(
            err.starts_with(&format!("Could not replace {}: ", dir.join("resources").display())),
            "{err}"
        );
        assert!(err.ends_with("Close any program using it and try again."), "{err}");
        assert_eq!(tree(&dir), before);
        assert!(!staging.exists());
        assert!(!backup.exists());
    }

    #[test]
    fn a_failure_moving_the_old_entry_aside_puts_back_what_came_before() {
        let root = tempfile::tempdir().unwrap();
        let (dir, staging) = prepared(
            root.path(),
            &[("app.exe", "v1"), ("resources/a.md", "v1")],
            &[("app.exe", "v2"), ("resources/b.md", "v2")],
        );
        let before = installed(&dir);
        let backup = dir.join(BACKUP);
        fail_rename_into(&backup.join("resources"));
        assert!(swap(&dir, &staging, &backup, &names(&["app.exe", "resources"])).is_err());
        assert_eq!(tree(&dir), before);
        assert!(!backup.exists());
    }

    /// What cannot go back stays in the backup, which the next run clears: it still holds the
    /// previous version's files, never nothing.
    #[test]
    fn what_cannot_be_put_back_stays_in_the_backup() {
        let root = tempfile::tempdir().unwrap();
        let (dir, staging) = prepared(
            root.path(),
            &[("app.exe", "v1"), ("resources/a.md", "v1")],
            &[("app.exe", "v2"), ("resources/b.md", "v2")],
        );
        let backup = dir.join(BACKUP);
        // Putting the new `resources` in place fails, and so does putting the old one back.
        fail_rename_into(&dir.join("resources"));
        fail_rename_into(&dir.join("resources"));
        let err = swap(&dir, &staging, &backup, &names(&["app.exe", "resources"])).unwrap_err();
        // Typed, so the runner can tell the window the folder holds part of each version.
        let incomplete = err.downcast_ref::<SwapIncomplete>().expect("a SwapIncomplete");
        assert_eq!(incomplete.stuck, ["resources"]);
        let err = format!("{err:#}");
        assert!(
            err.starts_with(&format!("Could not replace {}: ", dir.join("resources").display())),
            "{err}"
        );
        assert!(
            err.ends_with("Setup could not undo its changes to resources; running Setup again finishes the update."),
            "{err}"
        );
        assert_eq!(std::fs::read_to_string(dir.join("app.exe")).unwrap(), "v1");
        assert_eq!(
            std::fs::read_to_string(backup.join("resources").join("a.md")).unwrap(),
            "v1"
        );
        assert!(!dir.join("resources").exists());
    }

    /// A setup file staged the way the app stages one (`signed` puts its `.sig` next to it), and
    /// the uninstaller copied from it into the staging folder.
    fn staged_setup(root: &Path, signed: bool) -> (PathBuf, PathBuf) {
        use demido_core::signature::fixtures::{TEST_DATA, TEST_FILE, TEST_SIGNATURE};

        let source = root.join("updates").join(TEST_FILE);
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, TEST_DATA).unwrap();
        if signed {
            std::fs::write(signature_path(&source), TEST_SIGNATURE).unwrap();
        }
        let copy = root.join(STAGING).join("uninstall.exe");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        std::fs::copy(&source, &copy).unwrap();
        (source, copy)
    }

    /// [`check_uninstaller`] with the test key, whose signature is for the fixture's file and
    /// version.
    fn check(source: &Path, copy: &Path, required: bool) -> anyhow::Result<()> {
        use demido_core::signature::fixtures::{TEST_FILE, TEST_PUBLIC_KEY, TEST_VERSION};
        check_uninstaller(source, copy, required, TEST_PUBLIC_KEY, TEST_FILE, TEST_VERSION)
    }

    #[test]
    fn the_signature_sits_next_to_the_installer() {
        assert_eq!(
            signature_path(Path::new(r"C:\Updates\Demido-Studio-Setup-0.5.0.exe")),
            Path::new(r"C:\Updates\Demido-Studio-Setup-0.5.0.exe.sig")
        );
    }

    #[test]
    fn a_copy_that_matches_the_signature_passes() {
        let root = tempfile::tempdir().unwrap();
        let (source, copy) = staged_setup(root.path(), true);
        check(&source, &copy, true).unwrap();
        check(&source, &copy, false).unwrap();
        // The copy is what counts: the file it came from may change once it is copied.
        std::fs::write(&source, b"replaced afterwards").unwrap();
        check(&source, &copy, true).unwrap();
    }

    /// The file was replaced before setup copied it, after the app had checked it.
    #[test]
    fn a_copy_that_differs_from_what_was_signed_fails() {
        let root = tempfile::tempdir().unwrap();
        let (source, copy) = staged_setup(root.path(), true);
        std::fs::write(&copy, b"someone else's program").unwrap();
        for required in [true, false] {
            let err = check(&source, &copy, required).unwrap_err();
            assert!(err.to_string().contains("does not match"), "{err:#}");
        }
    }

    #[test]
    fn a_missing_signature_fails_only_when_one_is_required() {
        let root = tempfile::tempdir().unwrap();
        let (source, copy) = staged_setup(root.path(), false);
        let err = check(&source, &copy, true).unwrap_err();
        assert!(err.to_string().starts_with("could not read "), "{err:#}");
        // An installer someone downloaded and opened has no signature next to it.
        check(&source, &copy, false).unwrap();
    }

    /// A signature for another release does not vouch for this one.
    #[test]
    fn a_signature_for_another_version_fails() {
        use demido_core::signature::fixtures::{TEST_FILE, TEST_PUBLIC_KEY, TEST_VERSION};

        let root = tempfile::tempdir().unwrap();
        let (source, copy) = staged_setup(root.path(), true);
        assert!(check_uninstaller(&source, &copy, true, TEST_PUBLIC_KEY, TEST_FILE, "9.9.10").is_err());
        // Nor is a signature made with another key than the one setup trusts.
        let release = demido_core::signature::PUBLIC_KEY;
        assert!(check_uninstaller(&source, &copy, false, release, TEST_FILE, TEST_VERSION).is_err());
    }

    #[test]
    fn moving_what_is_not_there_fails_at_once() {
        let root = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let err = rename(&root.path().join("missing"), &root.path().join("moved")).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::NotFound);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    /// A scanner holding a new file open for a moment does not fail the update: Windows refuses to
    /// move a folder while a file in it is open, and the move is tried again.
    #[cfg(windows)]
    #[test]
    fn a_folder_held_open_for_a_moment_is_moved_once_it_is_let_go() {
        use std::os::windows::fs::OpenOptionsExt;

        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("resources");
        write(&from.join("a.md"), "v2");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(from.join("a.md"))
            .unwrap();
        // Held, the folder cannot move.
        assert!(std::fs::rename(&from, root.path().join("probe")).is_err());
        let letting_go = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        let to = root.path().join("moved");
        rename(&from, &to).unwrap();
        letting_go.join().unwrap();
        assert_eq!(std::fs::read_to_string(to.join("a.md")).unwrap(), "v2");
    }

    #[test]
    fn leftovers_of_an_earlier_run_are_cleared() {
        let root = tempfile::tempdir().unwrap();
        let (dir, staging) = prepared(root.path(), &[], &[("half/unpacked.bin", "x")]);
        clear(&staging).unwrap();
        assert!(!staging.exists());
        // Nothing there is fine too.
        clear(&dir.join(BACKUP)).unwrap();
    }

    #[test]
    fn unpacking_reports_progress_and_counts_files() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, content) in [("app.exe", "v2"), ("resources/skills/a.md", "skill")] {
            zip.start_file(name, options).unwrap();
            std::io::Write::write_all(&mut zip, content.as_bytes()).unwrap();
        }
        zip.add_directory("empty/", options).unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join(STAGING);
        let mut reports = Vec::new();
        assert_eq!(unpack(&bytes, &target, |read| reports.push(read)).unwrap(), 2);
        assert!(!reports.is_empty());
        assert!(reports.windows(2).all(|w| w[0] < w[1]), "{reports:?}");
        assert!(*reports.last().unwrap() <= bytes.len() as u64);
        assert_eq!(
            std::fs::read_to_string(target.join("resources").join("skills").join("a.md")).unwrap(),
            "skill"
        );
        assert!(target.join("empty").is_dir());
    }
}
