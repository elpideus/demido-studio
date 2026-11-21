//! A folder saved before a program runs in it, so that what the program did can be undone as a whole.
//!
//! A file tool changes one file, and what the file held before is kept with the change. A program can do
//! anything inside the folder: one press of Undo has to cover all of it. So the folder is copied first.
//! On a disk that can share blocks between copies (btrfs, XFS, APFS) the copy costs nothing; elsewhere it
//! costs the folder's size, and so there is a limit: a folder too large to save is not saved, Demido says
//! so, and the session's level then decides whether the program needs the person's yes.
//!
//! What programs make and can make again is left alone both ways (`NOT_SAVED`): a build folder is neither
//! copied nor put back. Everything else is, `.git` included.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A folder with more than this is not saved.
pub const MOST_FILES: usize = 20_000;
pub const MOST_BYTES: u64 = 300_000_000;
/// Folders that programs fill and can fill again. Neither saved nor put back.
pub const NOT_SAVED: [&str; 7] = ["node_modules", "target", ".venv", "venv", "__pycache__", "dist", ".cache"];

/// What is in a folder, as paths from it: its files and links, and its folders.
struct Held {
    files: BTreeSet<PathBuf>,
    folders: BTreeSet<PathBuf>,
    bytes: u64,
}

/// Walks a folder without following links and without going into what programs make. `Err` when it holds
/// more than may be saved (only checked when `limited`).
fn held(root: &Path, limited: bool) -> Result<Held, String> {
    let mut found = Held { files: BTreeSet::new(), folders: BTreeSet::new(), bytes: 0 };
    let mut waiting = vec![root.to_path_buf()];
    while let Some(dir) = waiting.pop() {
        for entry in fs::read_dir(&dir).map_err(|e| format!("{} cannot be read: {e}", dir.display()))?.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            let within = path.strip_prefix(root).expect("under the root").to_path_buf();
            if kind.is_dir() {
                if entry.file_name().to_str().is_some_and(|n| NOT_SAVED.contains(&n)) {
                    continue;
                }
                found.folders.insert(within);
                waiting.push(path);
            } else {
                found.bytes += if kind.is_file() { entry.metadata().map_or(0, |m| m.len()) } else { 0 };
                found.files.insert(within);
                if limited && (found.files.len() > MOST_FILES || found.bytes > MOST_BYTES) {
                    return Err(format!("the folder holds more than {MOST_FILES} files or {} MB, which is more than is saved before a command", MOST_BYTES / 1_000_000));
                }
            }
        }
    }
    Ok(found)
}

/// Copies one file or link, making the folder it goes into.
fn copy(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{} cannot be made: {e}", parent.display()))?;
    }
    let kind = fs::symlink_metadata(from).map_err(|e| format!("{} cannot be read: {e}", from.display()))?.file_type();
    if kind.is_symlink() {
        // A link is kept as a link. It is never followed: it may lead anywhere.
        #[cfg(unix)]
        {
            let target = fs::read_link(from).map_err(|e| format!("{} cannot be read: {e}", from.display()))?;
            let _ = fs::remove_file(to);
            return std::os::unix::fs::symlink(target, to).map_err(|e| format!("{} cannot be made: {e}", to.display()));
        }
        #[cfg(not(unix))]
        return Ok(());
    }
    // A file that is in the way as a folder, or the other way round, goes first.
    if to.is_dir() {
        fs::remove_dir_all(to).map_err(|e| format!("{} cannot be replaced: {e}", to.display()))?;
    }
    // Where the disk can share blocks, this shares them: the copy takes no room until one side changes.
    fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} cannot be copied: {e}", from.display()))
}

/// How much was saved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub files: usize,
    pub bytes: u64,
}

/// Saves a folder into `into`, which is made. `Err` says why it was not saved; nothing is left half done.
pub fn save(folder: &Path, into: &Path) -> Result<Saved, String> {
    let now = held(folder, true)?;
    let done = (|| {
        fs::create_dir_all(into).map_err(|e| format!("{} cannot be made: {e}", into.display()))?;
        for dir in &now.folders {
            fs::create_dir_all(into.join(dir)).map_err(|e| format!("{} cannot be made: {e}", into.join(dir).display()))?;
        }
        for file in &now.files {
            copy(&folder.join(file), &into.join(file))?;
        }
        Ok(Saved { files: now.files.len(), bytes: now.bytes })
    })();
    if done.is_err() {
        let _ = fs::remove_dir_all(into);
    }
    done
}

/// What putting a folder back did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PutBack {
    /// Files that were brought back, or brought back to what they held.
    pub restored: usize,
    /// Files that were made after the folder was saved, and are gone again.
    pub removed: usize,
}

/// Puts a folder back as it was saved: what changed is as it was, what was removed is there again, what
/// was made since is gone. What programs make (`NOT_SAVED`) is left as it is.
pub fn restore(folder: &Path, from: &Path) -> Result<PutBack, String> {
    let (was, now) = (held(from, false)?, held(folder, false)?);
    let mut done = PutBack::default();
    for made_since in now.files.difference(&was.files) {
        let path = folder.join(made_since);
        if fs::remove_file(&path).is_ok() {
            done.removed += 1;
        }
    }
    // Folders made since, deepest first, when nothing is left in them.
    let made_since: Vec<&PathBuf> = now.folders.difference(&was.folders).collect();
    for dir in made_since.into_iter().rev() {
        let _ = fs::remove_dir(folder.join(dir));
    }
    for dir in &was.folders {
        let path = folder.join(dir);
        if path.is_file() {
            fs::remove_file(&path).map_err(|e| format!("{} cannot be replaced: {e}", path.display()))?;
        }
        fs::create_dir_all(&path).map_err(|e| format!("{} cannot be made: {e}", path.display()))?;
    }
    for file in &was.files {
        let (saved, there) = (from.join(file), folder.join(file));
        // A file that is as it was is not written again.
        let same = fs::symlink_metadata(&saved).is_ok_and(|m| m.is_file()) && fs::symlink_metadata(&there).is_ok_and(|m| m.is_file()) && fs::read(&saved).ok() == fs::read(&there).ok();
        if !same {
            copy(&saved, &there)?;
            done.restored += 1;
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("demido-checkpoint-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(dir.join("src/deep")).unwrap();
        fs::create_dir_all(dir.join("empty")).unwrap();
        fs::create_dir_all(dir.join("target/debug")).unwrap();
        fs::write(dir.join("README.md"), "# A project\n").unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.join("src/deep/data.bin"), [0u8, 1, 2, 255]).unwrap();
        fs::write(dir.join("target/debug/built"), "old build").unwrap();
        dir
    }

    /// Every file under a folder with what it holds, for comparing two states.
    fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        let held = held(dir, false).unwrap();
        held.files.iter().map(|f| (f.clone(), fs::read(dir.join(f)).unwrap_or_default())).chain(held.folders.iter().map(|d| (d.clone(), b"<folder>".to_vec()))).collect()
    }

    #[test]
    fn a_folder_is_saved_and_put_back_whatever_was_done_to_it() {
        let (dir, kept) = (folder("roundtrip"), std::env::temp_dir().join(format!("demido-checkpoint-kept-{}", uuid::Uuid::new_v4())));
        let before = snapshot(&dir);
        let saved = save(&dir, &kept).unwrap();
        assert_eq!(saved, Saved { files: 3, bytes: 12 + 13 + 4 }, "what programs make is not saved");
        assert!(!kept.join("target").exists() && kept.join("empty").is_dir());

        // What a program might do: change, remove, make, replace a file by a folder and a folder by a file.
        fs::write(dir.join("README.md"), "wiped").unwrap();
        fs::remove_file(dir.join("src/main.rs")).unwrap();
        fs::remove_dir_all(dir.join("src/deep")).unwrap();
        fs::write(dir.join("src/deep"), "now a file").unwrap();
        fs::remove_dir(dir.join("empty")).unwrap();
        fs::create_dir_all(dir.join("new/inner")).unwrap();
        fs::write(dir.join("new/inner/made.txt"), "made").unwrap();
        fs::write(dir.join("stray.log"), "noise").unwrap();
        fs::write(dir.join("target/debug/built"), "new build").unwrap();
        assert_ne!(snapshot(&dir), before);

        let done = restore(&dir, &kept).unwrap();
        assert_eq!(snapshot(&dir), before, "the folder is as it was");
        assert_eq!((done.restored, done.removed), (3, 3), "three files brought back; three made since are gone (a file where a folder was, a new file, a stray one)");
        assert!(!dir.join("new").exists() && !dir.join("stray.log").exists());
        assert_eq!(fs::read_to_string(dir.join("target/debug/built")).unwrap(), "new build", "what programs make is left as it is");
        // Putting back what is already as it was writes nothing.
        assert_eq!(restore(&dir, &kept).unwrap(), PutBack::default());
        fs::remove_dir_all(dir).ok();
        fs::remove_dir_all(kept).ok();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_kept_as_a_link_and_never_followed() {
        let (dir, kept) = (folder("links"), std::env::temp_dir().join(format!("demido-checkpoint-kept-{}", uuid::Uuid::new_v4())));
        let outside = std::env::temp_dir().join(format!("demido-checkpoint-outside-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("big.txt"), "not part of the folder").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("way-out")).unwrap();
        save(&dir, &kept).unwrap();
        assert!(fs::symlink_metadata(kept.join("way-out")).unwrap().file_type().is_symlink());
        assert!(
            !kept.join("way-out").join("big.txt").symlink_metadata().is_ok_and(|m| m.is_file()) || fs::read_link(kept.join("way-out")).unwrap() == outside,
            "the link was copied, not what it leads to"
        );
        fs::remove_file(dir.join("way-out")).unwrap();
        restore(&dir, &kept).unwrap();
        assert_eq!(fs::read_link(dir.join("way-out")).unwrap(), outside);
        assert_eq!(fs::read_to_string(outside.join("big.txt")).unwrap(), "not part of the folder");
        for d in [dir, kept, outside] {
            fs::remove_dir_all(d).ok();
        }
    }

    #[test]
    fn a_folder_that_is_too_large_is_not_saved_and_nothing_is_left_half_done() {
        let (dir, kept) = (folder("large"), std::env::temp_dir().join(format!("demido-checkpoint-kept-{}", uuid::Uuid::new_v4())));
        for i in 0..=MOST_FILES {
            fs::write(dir.join("src").join(format!("f{i}")), "").unwrap();
        }
        let why = save(&dir, &kept).unwrap_err();
        assert!(why.contains("more than 20000 files"), "{why}");
        assert!(!kept.exists());
        fs::remove_dir_all(dir).ok();
    }
}
