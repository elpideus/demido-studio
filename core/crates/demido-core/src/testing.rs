//! What the tests share.

use std::path::{Path, PathBuf};

/// A folder of its own for one test, removed when the test is done with it, whether it passed or not
/// (a failing test unwinds, and the folder goes with it).
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// A new, empty folder in the system's temporary folder, named `<prefix>-<a new id>`.
    pub(crate) fn new(prefix: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl std::ops::Deref for TempDir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tests_folder_goes_when_the_test_is_done_with_it() {
        let dir = TempDir::new("demido-testing");
        std::fs::write(dir.join("a"), "b").unwrap();
        let path = dir.to_path_buf();
        assert!(path.is_dir());
        drop(dir);
        assert!(!path.exists());
        let path = std::panic::catch_unwind(|| {
            let dir = TempDir::new("demido-testing");
            let path = dir.to_path_buf();
            if path.is_dir() {
                std::panic::resume_unwind(Box::new(path));
            }
            unreachable!()
        })
        .unwrap_err()
        .downcast::<PathBuf>()
        .unwrap();
        assert!(!path.exists(), "a test that fails leaves nothing either");
    }
}
