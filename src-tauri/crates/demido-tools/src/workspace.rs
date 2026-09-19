//! Where a tool is allowed to look.
//!
//! Every path a model produces comes through [`Workspace::resolve`], and a path
//! that leaves the workspace is refused there. This is the single most
//! important thing in the crate: a tool that opened whatever it was handed
//! would let a model with no idea what it is doing read a password file by
//! getting a relative path slightly wrong, without anyone having to be
//! malicious about it.
//!
//! It is also what makes the Read row of the capability matrix honest.
//! [`docs/rules/tools.md`](../../../../docs/rules/tools.md) says Read is Allow
//! in every mode, and that is only defensible because *outside the project* is
//! not a state that can be approved or refused: it never reaches the point of
//! asking.
//!
//! Three properties hold that up, and each of them is a way a prefix comparison
//! on its own is wrong.
//!
//! **The check is done against the real path, after symlinks are followed.** A
//! symlink inside the workspace pointing at the rest of the disk is the oldest
//! way there is around a prefix comparison, and it can be created by something
//! as ordinary as a package manager.
//!
//! **A path that does not exist yet is still checked**, because writing a new
//! file is a legitimate thing to ask for and the ancestor a `..` would climb
//! into may not exist either. Only the part that exists can be made real, so
//! the leftover is checked separately.
//!
//! **A path naming another volume is refused before the filesystem is asked.**
//! Windows accepts more spellings of "somewhere else" than any other platform:
//! a UNC share, a device namespace, a drive letter with no root after it. Those
//! are decided here, from the path, rather than left to `canonicalize` to fail
//! on with an error message about a server nobody mentioned.

use std::path::{Component, Path, PathBuf, Prefix};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// The root is not a directory that exists. Refused when the workspace is
    /// opened rather than when a tool first runs, so the failure names the
    /// setting instead of naming a file the user never mentioned.
    #[error("{0} is not a directory")]
    NotADirectory(PathBuf),

    /// The root has to be absolute. A relative one would mean "wherever the app
    /// happened to be started", which is a different directory depending on how
    /// it was launched.
    #[error("a workspace has to be an absolute path, and {0} is not")]
    NotAbsolute(PathBuf),

    /// The path leaves the workspace. The message names the workspace, because
    /// the model is about to be told this and has to be able to act on it.
    #[error("{path} is outside the workspace ({root})")]
    Outside { path: String, root: String },

    #[error("{path}: {detail}")]
    Unreadable { path: String, detail: String },
}

/// A path that has been through [`Workspace::resolve`], and is therefore
/// somewhere a tool may act.
///
/// A type rather than a `PathBuf` so that the confinement rule is carried by
/// the value instead of by everyone remembering it. A tool is handed arguments
/// and a [`crate::Context`]; the only way from the first to a path it can open
/// is through the second, and the only thing that comes back is one of these.
/// Nothing outside this module can build one, so a function that takes a
/// `Resolved` is a function that cannot be reached with a path nobody checked.
///
/// It is not a capability and does not pretend to be. A tool determined to call
/// `std::fs` with a raw string still can, which is why the registry's own suite
/// drives every registered tool at every shape of escape rather than trusting
/// the type. What this buys is that the correct path is the shortest one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Resolved {
    at: PathBuf,
}

impl Resolved {
    /// Where it is on disk, absolute and real.
    pub fn path(&self) -> &Path {
        &self.at
    }
}

impl AsRef<Path> for Resolved {
    fn as_ref(&self) -> &Path {
        &self.at
    }
}

/// The directory tool paths are resolved against, and confined to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Open a workspace at `root`.
    ///
    /// The root is resolved through symlinks once, here, so every later
    /// comparison is between two real paths. Comparing a real path against a
    /// root that was itself a symlink would refuse everything in it.
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(Error::NotAbsolute(root.to_path_buf()));
        }
        if !root.is_dir() {
            return Err(Error::NotADirectory(root.to_path_buf()));
        }

        let real = std::fs::canonicalize(root).map_err(|err| Error::Unreadable {
            path: root.display().to_string(),
            detail: err.to_string(),
        })?;

        Ok(Self { root: real })
    }

    /// What the model is told the workspace is called.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The root as a person reads it, without the verbatim prefix
    /// canonicalising put on it. What the tool picker names.
    pub fn shown(&self) -> PathBuf {
        plain(&self.root)
    }

    /// Turn a path the model produced into one a tool may open.
    ///
    /// Relative paths are taken as relative to the workspace, which is the only
    /// meaning that is stable: the process working directory is a property of
    /// how the app was launched and no model can reason about it.
    ///
    /// A path that does not exist yet still resolves, because writing a new
    /// file is a legitimate thing to ask for. Its parent has to exist and has
    /// to be inside, which is what keeps `../../elsewhere/new.txt` out.
    pub fn resolve(&self, given: &str) -> Result<Resolved> {
        let asked = Path::new(given);

        // Decided from the path rather than from the filesystem, because these
        // are the spellings whose failure would otherwise be an error message
        // about a machine nobody mentioned. `\\server\share\x` names another
        // volume; `C:tmp` names whatever directory that process happens to be
        // in on that drive; `\Windows` names the root of the current drive.
        // Only the first is obviously an escape, and all three are.
        if !asked.is_absolute() && (has_prefix(asked) || asked.has_root()) {
            return Err(self.outside(given));
        }
        if asked.is_absolute() && volume(asked) != volume(&self.root) {
            return Err(self.outside(given));
        }

        let joined = if asked.is_absolute() {
            asked.to_path_buf()
        } else {
            self.root.join(asked)
        };

        // The nearest ancestor that exists, and what is left over. Only the
        // part that exists can be made real, and only a real path can be
        // compared honestly.
        let (existing, rest) = split_at_existing(&joined);

        let real = std::fs::canonicalize(&existing).map_err(|err| Error::Unreadable {
            path: given.to_owned(),
            detail: err.to_string(),
        })?;

        if !real.starts_with(&self.root) {
            return Err(self.outside(given));
        }

        // The leftover cannot contain `..`: the ancestor it would climb into
        // does not exist yet, so no amount of canonicalising would catch it.
        if rest.components().any(|part| part == Component::ParentDir) {
            return Err(self.outside(given));
        }

        // Joining an empty remainder would append a separator, which turns a
        // file into a directory name and fails at open with a message about
        // directories that has nothing to do with what was asked.
        let at = if rest.as_os_str().is_empty() {
            real
        } else {
            real.join(rest)
        };
        Ok(Resolved { at })
    }

    /// Confine a path that already exists, such as one a walk just found.
    ///
    /// [`Workspace::resolve`] answers a string the model wrote; this answers a
    /// path Demido itself produced, and it is not the same question. A walk
    /// that started inside the workspace stays inside it right up until it
    /// reaches a symlinked directory, and `is_dir` follows one without saying
    /// so. Every entry a walk keeps goes through here, so a link out of the
    /// project is refused at the entry rather than descended into.
    pub fn confine(&self, path: &Path) -> Result<Resolved> {
        let real = std::fs::canonicalize(path).map_err(|err| Error::Unreadable {
            path: path.display().to_string(),
            detail: err.to_string(),
        })?;

        if !real.starts_with(&self.root) {
            return Err(self.outside(&path.display().to_string()));
        }
        Ok(Resolved { at: real })
    }

    /// How a path inside the workspace is written back to the model.
    ///
    /// Relative, always, and with forward slashes. An absolute path invites the
    /// model to use it later somewhere the workspace is not what it was, and it
    /// puts the user's account name into every listing for no reason.
    pub fn relative(&self, at: &Resolved) -> String {
        at.path()
            .strip_prefix(&self.root)
            .unwrap_or_else(|_| at.path())
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn outside(&self, given: &str) -> Error {
        Error::Outside {
            path: given.to_owned(),
            root: self.root.display().to_string(),
        }
    }
}

/// Whether a path names a volume: a drive letter, a share, a device namespace.
///
/// Always false off Windows, where `Component::Prefix` is never produced.
fn has_prefix(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Which volume a path is on, in a form two spellings of the same one compare
/// equal in.
///
/// `C:\project` and `\\?\C:\project` are the same drive, and the workspace root
/// is always the second of those because it has been canonicalised. Comparing
/// the prefixes as they were written would make every absolute path the model
/// produced look like another volume.
fn volume(path: &Path) -> Option<String> {
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return None;
    };
    Some(match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            format!("disk:{}", letter.to_ascii_uppercase() as char)
        }
        Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
            "unc:{}/{}",
            server.to_string_lossy().to_lowercase(),
            share.to_string_lossy().to_lowercase()
        ),
        Prefix::Verbatim(name) => format!("verbatim:{}", name.to_string_lossy().to_lowercase()),
        Prefix::DeviceNS(name) => format!("device:{}", name.to_string_lossy().to_lowercase()),
    })
}

/// The longest prefix of `path` that exists, and the remainder.
///
/// Walking up rather than down because the interesting case is a path most of
/// which is real: `project/src/new.rs` in a project where `src` exists.
///
/// **The remainder never ends in a separator**, and getting that wrong is not a
/// tidiness point. `Path::join` on an empty path appends one, so the obvious
/// `name.join(&rest)` turns the first step of this walk into `new.txt/`, the
/// caller joins that onto the real root, and what comes out is
/// `.../work/new.txt/`. On Linux and macOS opening that is `ENOTDIR`, so
/// `write_file` could not create a file at all: every new path came back as
/// "Not a directory (os error 20)". Windows tolerates the trailing separator
/// and the whole thing worked there, which is why it survived in v2 until CI
/// caught it. The caller guards the same mistake for a remainder that is
/// entirely empty. This is the same rule, one function further in.
fn split_at_existing(path: &Path) -> (PathBuf, PathBuf) {
    let mut existing = path.to_path_buf();
    let mut rest = PathBuf::new();

    while !existing.exists() {
        let Some(name) = existing.file_name().map(std::ffi::OsString::from) else {
            break;
        };
        if !existing.pop() {
            break;
        }
        rest = if rest.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            Path::new(&name).join(&rest)
        };
    }

    (existing, rest)
}

/// A path without the verbatim prefix, as `cmd.exe` and a person both read it.
///
/// The workspace root is canonical, and on Windows canonical means the
/// `\\?\` prefix, which `cmd.exe` refuses as a current directory and replaces
/// with the Windows directory without failing.
pub(crate) fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\UNC\") {
        Some(share) => PathBuf::from(format!(r"\\{share}")),
        None => PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    #[test]
    fn a_relative_path_is_relative_to_the_workspace_and_not_to_the_process() {
        let (_dir, workspace) = workspace();
        let at = workspace.resolve("src/main.rs").expect("inside");

        assert!(at.path().starts_with(workspace.root()));
        assert_eq!(workspace.relative(&at), "src/main.rs");
    }

    #[test]
    fn climbing_out_is_refused_however_it_is_spelled() {
        let (_dir, workspace) = workspace();

        for attempt in ["..", "../..", "src/../..", "src/../../elsewhere"] {
            assert!(
                matches!(workspace.resolve(attempt), Err(Error::Outside { .. })),
                "{attempt} was allowed"
            );
        }
    }

    #[test]
    fn an_absolute_path_elsewhere_is_refused_rather_than_taken_at_its_word() {
        let (_dir, workspace) = workspace();
        let elsewhere = tempfile::tempdir().unwrap();

        assert!(matches!(
            workspace.resolve(&elsewhere.path().to_string_lossy()),
            Err(Error::Outside { .. })
        ));
    }

    #[test]
    fn a_file_that_does_not_exist_yet_resolves_if_its_parent_is_inside() {
        // Writing a new file is a legitimate thing to ask for, so a missing
        // leaf is not the same as a path that escapes.
        let (_dir, workspace) = workspace();
        let at = workspace.resolve("src/new.rs").expect("a new file inside");

        assert!(at.path().starts_with(workspace.root()));
        assert!(!at.path().exists());
        // And it is a file, not a directory with an empty name. See
        // `split_at_existing`: this is the property that failed on two of the
        // three platforms this ships to.
        std::fs::write(at.path(), "hello").expect("a resolved new path is writable");
        assert_eq!(std::fs::read_to_string(at.path()).unwrap(), "hello");
    }

    #[test]
    fn a_new_file_beside_an_existing_one_is_a_file_too() {
        // The shallowest case, where the remainder starts empty.
        let (_dir, workspace) = workspace();
        let at = workspace
            .resolve("new.txt")
            .expect("a new file at the root");

        std::fs::write(at.path(), "hello").expect("a resolved new path is writable");
        assert!(at.path().is_file());
    }

    #[test]
    fn a_path_that_does_not_exist_and_climbs_out_is_still_refused() {
        // The ancestor it climbs into does not exist either, so no amount of
        // canonicalising would notice. This is the case a prefix check on the
        // real path alone gets wrong.
        let (_dir, workspace) = workspace();

        assert!(matches!(
            workspace.resolve("src/../../nowhere/new.rs"),
            Err(Error::Outside { .. })
        ));
    }

    #[test]
    #[cfg_attr(
        windows,
        ignore = "making a symlink on Windows needs developer mode or elevation"
    )]
    fn a_symlink_pointing_out_of_the_workspace_is_refused() {
        // The oldest way around a prefix comparison, and one an ordinary
        // package manager can create without anybody meaning anything by it.
        let (dir, workspace) = workspace();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("secret"), "shh").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("escape")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(elsewhere.path(), dir.path().join("escape")).unwrap();

        assert!(matches!(
            workspace.resolve("escape/secret"),
            Err(Error::Outside { .. })
        ));
        assert!(matches!(
            workspace.confine(&dir.path().join("escape").join("secret")),
            Err(Error::Outside { .. })
        ));
    }

    #[test]
    fn a_root_that_is_not_an_absolute_directory_is_refused_when_it_is_named() {
        // At the point the setting is read, so the message can name the
        // setting rather than a file the user never mentioned.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, "").unwrap();

        assert!(matches!(
            Workspace::open("relative/path"),
            Err(Error::NotAbsolute(_))
        ));
        assert!(matches!(
            Workspace::open(&file),
            Err(Error::NotADirectory(_))
        ));
        assert!(matches!(
            Workspace::open(dir.path().join("nothing")),
            Err(Error::NotADirectory(_))
        ));
    }

    #[test]
    fn a_workspace_reached_through_a_symlink_does_not_refuse_its_own_contents() {
        // The root is made real once, at open. Comparing a real path against a
        // root that was itself a link would refuse everything in it, which is
        // exactly what happens on macOS where /tmp is a link to /private/tmp.
        let (dir, workspace) = workspace();
        let same = Workspace::open(std::fs::canonicalize(dir.path()).unwrap()).unwrap();

        assert_eq!(workspace.root(), same.root());
        assert!(same.resolve("src/main.rs").is_ok());
    }

    #[test]
    fn a_path_inside_is_confined_and_comes_back_real() {
        let (dir, workspace) = workspace();
        let at = workspace
            .confine(&dir.path().join("src").join("main.rs"))
            .expect("inside");

        assert_eq!(workspace.relative(&at), "src/main.rs");
    }

    #[test]
    fn a_long_path_inside_the_workspace_is_still_inside_it() {
        // Windows refuses a path over 260 characters unless it carries the
        // extended-length prefix, and a canonicalised root already does. The
        // property under test is that length changes nothing about the answer:
        // deep is not the same as outside.
        let (_dir, workspace) = workspace();
        let deep: String = (0..20).map(|n| format!("directory-{n:03}/")).collect();
        std::fs::create_dir_all(workspace.root().join(&deep)).expect("a deep directory");

        let at = workspace
            .resolve(&format!("{deep}note.txt"))
            .expect("deep is not outside");
        assert!(
            at.path().to_string_lossy().len() > 260,
            "not actually a long path: {at:?}"
        );
        std::fs::write(at.path(), "hello").expect("writable at depth");
    }

    #[test]
    fn a_long_path_that_climbs_out_is_refused_like_a_short_one() {
        let (_dir, workspace) = workspace();
        let up: String = (0..40).map(|_| "../").collect();

        assert!(matches!(
            workspace.resolve(&format!("{up}secret.txt")),
            Err(Error::Outside { .. })
        ));
    }

    #[test]
    #[cfg(windows)]
    fn every_windows_spelling_of_somewhere_else_is_refused() {
        // Windows accepts more ways of saying "not here" than any other
        // platform, and only the first of these looks like an escape.
        //
        // `\\server\share\x` is another volume. `\\?\C:\Windows\win.ini` is the
        // same drive as the workspace with the extended-length prefix on it, so
        // the volume comparison passes and the prefix comparison is what
        // refuses it. `C:tmp\x` is drive-relative: it means "wherever this
        // process happens to be on C:", which is not a directory anybody can
        // reason about. `\Windows\win.ini` is the root of the current drive,
        // and `Path::join` would splice it onto the workspace's drive letter
        // rather than under the workspace.
        let (_dir, workspace) = workspace();

        for attempt in [
            r"\\server\share\secret.txt",
            r"\\?\C:\Windows\win.ini",
            r"\\.\PhysicalDrive0",
            r"C:tmp\secret.txt",
            r"\Windows\win.ini",
            r"C:\Windows\win.ini",
        ] {
            assert!(
                matches!(workspace.resolve(attempt), Err(Error::Outside { .. })),
                "{attempt} was not refused as outside"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn the_workspaces_own_drive_letter_is_not_another_volume() {
        // The other half of the rule above: a spelling of the workspace itself
        // has to survive it, or an absolute path the window handed over would
        // be refused for being written the way Windows writes it.
        let (dir, workspace) = workspace();
        let inside = dir.path().join("src").join("main.rs");

        let at = workspace
            .resolve(&inside.to_string_lossy())
            .expect("the workspace's own path");
        assert_eq!(workspace.relative(&at), "src/main.rs");
    }
}
