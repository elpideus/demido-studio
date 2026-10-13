//! The setup lock on Unix: a file in the user's runtime folder that setup holds an exclusive lock
//! on (`flock`) while it installs or updates. The kernel lets go of it however the process ends,
//! as Windows does of the named mutex [`crate::brand::SETUP_LOCK`] names there.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};

/// The lock file: `$XDG_RUNTIME_DIR/demido-studio-setup.lock`, else one per user in the temp
/// folder.
pub fn path() -> PathBuf {
    let name = format!("{}-setup.lock", crate::brand::APP_SLUG);
    match std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.is_dir())
    {
        Some(dir) => dir.join(name),
        None => {
            use std::os::unix::fs::MetadataExt;
            let uid = dirs::home_dir()
                .and_then(|home| std::fs::metadata(home).ok())
                .map_or(0, |m| m.uid());
            std::env::temp_dir().join(format!("{uid}-{name}"))
        }
    }
}

/// The lock while it is held; dropping it lets go.
#[derive(Debug)]
pub struct Held(#[allow(dead_code)] File);

/// Takes the lock at `path`, or `None` when another process holds it (or the file cannot be
/// opened, which leaves nothing to hold).
pub fn try_acquire(path: &Path) -> Option<Held> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .ok()?;
    match file.try_lock() {
        Ok(()) => Some(Held(file)),
        Err(TryLockError::WouldBlock) | Err(TryLockError::Error(_)) => None,
    }
}

/// Whether a process holds the lock at `path`. Never waits.
pub fn is_held(path: &Path) -> bool {
    let Ok(file) = File::open(path) else {
        return false;
    };
    matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_holds_the_lock_until_it_lets_go() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("setup.lock");
        assert!(!is_held(&path), "nobody holds a lock that was never taken");
        let first = try_acquire(&path).expect("nothing else holds it");
        assert!(is_held(&path));
        assert!(try_acquire(&path).is_none(), "a second taker got the lock");
        drop(first);
        assert!(!is_held(&path), "the lock was not let go");
        assert!(try_acquire(&path).is_some());
    }

    #[test]
    fn the_lock_lives_in_the_runtime_folder() {
        let p = path();
        assert!(p.is_absolute());
        assert!(
            p.to_string_lossy().ends_with("demido-studio-setup.lock"),
            "{}",
            p.display()
        );
    }
}
