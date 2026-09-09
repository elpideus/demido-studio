//! The state machine: `docs/rules/runtimes.md` as code rather than as a
//! comment.
//!
//! `fetch`, `unpack` and `verify` are plumbing. This is what decides, per row,
//! whether what arrived becomes managed, is refused, or replaces a
//! predecessor, and it is the only module that deletes anything.

use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::pin::Pin;

use crate::fetch::{fetch, Cancel, FetchError, Fetchable, Progress};
use crate::state::{directory_name, Ledger, RowState};
use crate::store::Store;
use crate::unpack::{directory_size_mib, unpack};
use crate::verify::Installed;

/// How a row's verification is run.
///
/// A closure rather than a trait object with an implementation to mock,
/// because what it stands in front of is a real process against a real card.
/// The seam exists so the rules in this file can be tested without a GPU, not
/// so verification can be faked in production: `crate::verify::Verification`
/// is what the composition root passes.
pub type VerifyFn<'a> = dyn Fn(Installed) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send>>
    + Send
    + Sync
    + 'a;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] crate::store::Error),
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error(transparent)]
    Unpack(#[from] crate::unpack::UnpackError),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    /// An action the row's state does not allow, made a value rather than a
    /// silent no-op that leaves a caller believing bytes were freed.
    #[error("{action} is not allowed on {state} row")]
    NotAllowed {
        action: &'static str,
        state: &'static str,
    },
    /// A ledger naming a directory outside the profile's runtimes folder.
    /// `runtimes.json` is a file a person can open and edit, so the guard on
    /// "Demido prunes no cache it did not fill" is checked here rather than
    /// assumed from how the name was built.
    #[error("{name} is not a directory inside the profile's runtimes folder")]
    Outside { name: String },
}

impl Error {
    fn io(context: impl std::fmt::Display, source: std::io::Error) -> Self {
        Self::Io {
            context: context.to_string(),
            source,
        }
    }
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Io { context, source } => demido_core::Error::io(context, source),
            Error::NotAllowed { action, state } => {
                demido_core::Error::invalid(action, format!("the row is {state} row"))
            }
            other => demido_core::Error::unavailable("the runtimes", other.to_string()),
        }
    }
}

/// What a fetch or a link did, which is not the same question as whether the
/// call failed.
///
/// A refused verification is an ordinary outcome, not an error: the row was
/// updated (to absent with a reason, or left as it was), the ledger was
/// written, and nothing went wrong with the machinery. Returning `Ok(())` for
/// it would make "the archive arrived and does not work" indistinguishable
/// from "the runtime is ready".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It verified, and the row now runs it.
    Verified,
    /// Verification refused it. What the row is now is in the ledger: absent
    /// with this reason, or still managed at the pin that already worked.
    Refused { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnusedEntry {
    pub name: String,
    pub path: PathBuf,
    pub size_mib: f64,
}

/// Owns the ledger and the profile's runtimes directory.
///
/// One per profile: `docs/rules/profiles.md` scopes runtimes per profile, so
/// a second Windows user gets their own and nobody replaces a binary another
/// user executes.
///
/// The verification command is held here rather than passed to each call,
/// because a profile verifies one way and the composition root is where that
/// is decided ([`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): one
/// wiring line names the implementation). It is also what lets the rules
/// below be tested without a card.
pub struct Runtimes<S: Store> {
    store: S,
    runtimes_dir: PathBuf,
    client: reqwest::Client,
    verify: Box<VerifyFn<'static>>,
}

impl<S: Store> Runtimes<S> {
    pub fn new(
        store: S,
        runtimes_dir: impl Into<PathBuf>,
        verify: impl Fn(Installed) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            store,
            runtimes_dir: runtimes_dir.into(),
            client: reqwest::Client::new(),
            verify: Box::new(verify),
        }
    }

    pub fn read(&self) -> Result<Ledger, Error> {
        Ok(self.store.read()?)
    }

    pub fn runtimes_dir(&self) -> &Path {
        &self.runtimes_dir
    }

    /// One directory immediately inside the runtimes folder, or a refusal.
    ///
    /// Everything this module deletes goes through here. A name carrying a
    /// separator, a `..`, or a root would otherwise let an edited
    /// `runtimes.json` aim `remove_dir_all` at `~/.agent-browser` or anywhere
    /// else Demido did not fill.
    fn inside(&self, name: &str) -> Result<PathBuf, Error> {
        let mut components = Path::new(name).components();
        let only = matches!(components.next(), Some(Component::Normal(_)));
        if !only || components.next().is_some() {
            return Err(Error::Outside {
                name: name.to_owned(),
            });
        }
        Ok(self.runtimes_dir.join(name))
    }

    fn row_dir(&self, id: &str, pin: &str) -> Result<PathBuf, Error> {
        self.inside(&directory_name(id, pin))
    }

    fn delete(&self, dir: &Path) -> Result<(), Error> {
        std::fs::remove_dir_all(dir).or_else(|error| match error.kind() {
            std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(Error::io(format!("removing {}", dir.display()), error)),
        })
    }

    /// Fetch every archive of one row, unpack them into that row's directory,
    /// and verify the result before the row is allowed to claim it works.
    ///
    /// `items` is a build and its companions, which for CUDA is the pair
    /// section 7 calls "one row's worth of action": they move together or not
    /// at all, because a CUDA build that cannot resolve `cublasLt64_13.dll`
    /// does not load a model.
    ///
    /// The predecessor is deleted only after `verify` succeeds, and inside
    /// this same call, so there is never a window where both pins or neither
    /// are on disk (section 3). A refused update leaves the pin that already
    /// worked exactly where it was: section 8's promise is that a user with a
    /// working runtime keeps one, and a rule that deleted it on a failed
    /// upgrade would make "never automatic" mean "automatic, or nothing
    /// answers".
    pub async fn fetch_row<F: Fetchable>(
        &self,
        id: &str,
        pin: &str,
        items: &[F],
        mut on_progress: impl FnMut(&str, Progress),
        cancel: &Cancel,
    ) -> Result<Outcome, Error> {
        let mut ledger = self.store.read()?;
        let managed_at = match ledger.state(id) {
            Some(RowState::Managed { pin, .. }) => Some(pin.clone()),
            _ => None,
        };

        let dest = self.row_dir(id, pin)?;
        let mut archives = Vec::new();
        for item in items {
            let archive = fetch(
                item,
                &self.runtimes_dir,
                &self.client,
                |progress| on_progress(item.name(), progress),
                cancel,
            )
            .await?;
            unpack(&archive, &dest)?;
            // The zip is this crate's litter once expanded, and only the
            // expanded tree counts toward what the row spent.
            let _ = std::fs::remove_file(&archive);
            archives.push(item.name().to_owned());
        }

        let verified = (self.verify)(Installed::Managed {
            directory: dest.clone(),
        })
        .await;

        let reason = match verified {
            Ok(()) => {
                if let Some(previous) = managed_at.filter(|previous| previous != pin) {
                    self.delete(&self.row_dir(id, &previous)?)?;
                }
                let on_disk_mib = directory_size_mib(&dest)
                    .map_err(|error| Error::io(format!("measuring {}", dest.display()), error))?;
                ledger.set(
                    id,
                    RowState::Managed {
                        pin: pin.to_owned(),
                        archives,
                        on_disk_mib,
                    },
                );
                self.store.write(&ledger)?;
                return Ok(Outcome::Verified);
            }
            Err(reason) => reason,
        };

        match managed_at {
            // A re-fetch over the pin the row is already running, refused.
            // The bytes on disk are the ones that were working before this
            // call merged over them, so they are left alone: deleting them
            // would answer a failed verification by removing the user's
            // runtime.
            Some(previous) if previous == pin => {}
            // An update refused. The new pin's bytes are useless and the
            // predecessor is untouched, so the row goes on running what it
            // ran before.
            Some(_) => self.delete(&dest)?,
            // A first fetch refused: absent with a reason, never managed
            // (section 2). The unpacked bytes go, so a later fetch of the
            // same row downloads rather than finding a directory that looks
            // finished.
            None => {
                self.delete(&dest)?;
                ledger.set(
                    id,
                    RowState::Absent {
                        reason: Some(reason.clone()),
                    },
                );
                self.store.write(&ledger)?;
            }
        }

        Ok(Outcome::Refused { reason })
    }

    /// Point a row at a binary the user already has.
    ///
    /// Verified at the moment it is pointed at (section 10), with the same
    /// declared command a fetch runs: a user could otherwise link a
    /// `llama.cpp` that cannot resolve `cublasLt64_13.dll` and find out days
    /// later, mid conversation. A refusal never becomes linked, and never
    /// costs the row whatever was already working.
    ///
    /// The path is read and launched, never written to, and its bytes are
    /// never counted in the disk total.
    pub async fn link(
        &self,
        id: &str,
        binary: PathBuf,
        detected_version: Option<String>,
    ) -> Result<Outcome, Error> {
        let mut ledger = self.store.read()?;

        match (self.verify)(Installed::Linked {
            binary: binary.clone(),
        })
        .await
        {
            Ok(()) => {
                ledger.set(
                    id,
                    RowState::Linked {
                        path: binary,
                        detected_version,
                    },
                );
                self.store.write(&ledger)?;
                Ok(Outcome::Verified)
            }
            Err(reason) => {
                // A row already managed keeps running what it runs. Only a row
                // with nothing behind it records the refusal, which is section
                // 10's "stays absent with a reason".
                if !ledger.state(id).is_some_and(RowState::is_managed) {
                    ledger.set(
                        id,
                        RowState::Absent {
                            reason: Some(reason.clone()),
                        },
                    );
                    self.store.write(&ledger)?;
                }
                Ok(Outcome::Refused { reason })
            }
        }
    }

    /// Delete a managed row's bytes.
    ///
    /// Refused on anything else. A linked row has no delete control at all,
    /// which section 0 calls stronger than a delete control that refuses, and
    /// this is the refusal underneath that absence.
    pub fn remove(&self, id: &str) -> Result<(), Error> {
        let mut ledger = self.store.read()?;
        let state = ledger.state(id);
        let pin = match state {
            Some(RowState::Managed { pin, .. }) => pin.clone(),
            other => {
                return Err(Error::NotAllowed {
                    action: "remove",
                    state: other.map_or("an absent", RowState::label),
                })
            }
        };

        self.delete(&self.row_dir(id, &pin)?)?;
        ledger.set(id, RowState::Absent { reason: None });
        self.store.write(&ledger)?;
        Ok(())
    }

    /// Everything in the runtimes folder that no row is running from.
    ///
    /// Section 5: a fetch killed halfway, a pin abandoned when the user
    /// switched a row to linked, and a directory left by an older Demido all
    /// leave bytes nothing names. Computed by diffing the folder against the
    /// ledger on every call, never kept as a list, because a list of orphans
    /// is itself a thing that goes stale and that is the failure this whole
    /// file is about.
    ///
    /// Files count as well as directories: a cancelled fetch leaves its
    /// `.part` beside the folders, and 373 MiB of half a `cudart` that no
    /// screen ever mentions is the same silence with a smaller number.
    pub fn unused(&self) -> Result<Vec<UnusedEntry>, Error> {
        let claimed = self.store.read()?.claimed_directories();

        let entries = match std::fs::read_dir(&self.runtimes_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(Error::io(
                    format!("reading {}", self.runtimes_dir.display()),
                    error,
                ))
            }
        };

        let mut unused = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| Error::io("reading the runtimes folder", error))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if claimed.contains(&name) {
                continue;
            }
            let path = entry.path();
            let size_mib = if path.is_dir() {
                directory_size_mib(&path)
                    .map_err(|error| Error::io(format!("measuring {}", path.display()), error))?
            } else {
                entry
                    .metadata()
                    .map_err(|error| Error::io(format!("measuring {}", path.display()), error))?
                    .len() as f64
                    / (1024.0 * 1024.0)
            };
            unused.push(UnusedEntry {
                name,
                path,
                size_mib,
            });
        }
        Ok(unused)
    }

    /// Remove one entry `unused` reported, by the name it reported.
    ///
    /// Named rather than handed a path, so the only thing this can delete is
    /// something immediately inside the profile's own runtimes folder.
    pub fn remove_unused(&self, name: &str) -> Result<(), Error> {
        let path = self.inside(name)?;
        if self
            .store
            .read()?
            .claimed_directories()
            .contains(&name.to_owned())
        {
            return Err(Error::NotAllowed {
                action: "remove",
                state: "a managed",
            });
        }
        if path.is_dir() {
            self.delete(&path)
        } else {
            std::fs::remove_file(&path).or_else(|error| match error.kind() {
                std::io::ErrorKind::NotFound => Ok(()),
                _ => Err(Error::io(format!("removing {}", path.display()), error)),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use crate::file::Files;
    use std::future::Future;
    use std::pin::Pin;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-runtimes-manage-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    type Verified = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

    fn passes(_installed: Installed) -> Verified {
        Box::pin(async { Ok(()) })
    }

    fn refuses(_installed: Installed) -> Verified {
        Box::pin(async { Err("it reported its build and generated nothing".to_owned()) })
    }

    /// A profile whose verification always answers the same way, which is how
    /// the rules below are asserted without a card in the machine.
    fn runtimes(dir: &Path, verify: fn(Installed) -> Verified) -> Runtimes<Files> {
        Runtimes::new(Files::in_profile(dir), dir.join("runtimes"), verify)
    }

    fn managed(pin: &str) -> RowState {
        RowState::Managed {
            pin: pin.to_owned(),
            archives: vec![],
            on_disk_mib: 182.6,
        }
    }

    /// Puts a row in the ledger and its directory on disk, standing in for a
    /// fetch that already happened.
    fn already_managed(
        dir: &Path,
        id: &str,
        pin: &str,
        verify: fn(Installed) -> Verified,
    ) -> Runtimes<Files> {
        let runtimes = runtimes(dir, verify);
        std::fs::create_dir_all(runtimes.runtimes_dir().join(directory_name(id, pin)))
            .expect("made the row directory");
        let mut ledger = runtimes.read().expect("read");
        ledger.set(id, managed(pin));
        runtimes.store.write(&ledger).expect("wrote");
        runtimes
    }

    #[tokio::test]
    async fn a_binary_that_fails_verification_never_becomes_linked() {
        let dir = scratch("link-refused");
        let runtimes = runtimes(&dir, refuses);

        let outcome = runtimes
            .link(
                "llama.cpp",
                PathBuf::from("C:/theirs/llama-server.exe"),
                None,
            )
            .await
            .expect("the call itself succeeds");

        assert!(matches!(outcome, Outcome::Refused { .. }));
        match runtimes.read().expect("read").state("llama.cpp") {
            Some(RowState::Absent {
                reason: Some(reason),
            }) => {
                assert!(reason.contains("generated nothing"), "{reason}");
            }
            other => panic!("expected absent with a reason, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_linked_row_is_counted_in_no_total_and_has_no_delete() {
        let dir = scratch("linked");
        let runtimes = runtimes(&dir, passes);

        runtimes
            .link(
                "llama.cpp",
                PathBuf::from("C:/theirs/llama-server.exe"),
                Some("b10820".into()),
            )
            .await
            .expect("linked");

        let ledger = runtimes.read().expect("read");
        assert!(ledger.state("llama.cpp").is_some_and(RowState::is_linked));
        assert_eq!(ledger.on_disk_mib(), 0.0);
        assert!(matches!(
            runtimes.remove("llama.cpp"),
            Err(Error::NotAllowed {
                state: "a linked",
                ..
            })
        ));
    }

    /// Pointing at a bad binary is an action that failed, not a reason to
    /// forget the runtime that works.
    #[tokio::test]
    async fn a_refused_link_leaves_a_managed_row_running() {
        let dir = scratch("link-over-managed");
        let runtimes = already_managed(&dir, "llama.cpp", "b10816", refuses);

        runtimes
            .link(
                "llama.cpp",
                PathBuf::from("C:/theirs/llama-server.exe"),
                None,
            )
            .await
            .expect("the call itself succeeds");

        assert_eq!(
            runtimes.read().expect("read").state("llama.cpp"),
            Some(&managed("b10816")),
            "a failed link never costs the user the pin that was working"
        );
    }

    #[tokio::test]
    async fn removing_a_row_nothing_fetched_is_refused_rather_than_silent() {
        let dir = scratch("remove-absent");
        assert!(matches!(
            runtimes(&dir, passes).remove("llama.cpp"),
            Err(Error::NotAllowed { .. })
        ));
    }

    #[tokio::test]
    async fn removing_a_managed_row_takes_its_directory_and_nothing_else() {
        let dir = scratch("remove-managed");
        let runtimes = already_managed(&dir, "llama.cpp", "b10816", passes);
        let stranger = runtimes.runtimes_dir().join("someone-elses");
        std::fs::create_dir_all(&stranger).expect("made a neighbour");

        runtimes.remove("llama.cpp").expect("removed");

        assert!(!runtimes.runtimes_dir().join("llama.cpp-b10816").exists());
        assert!(stranger.exists(), "nothing else in the folder was touched");
    }

    /// `runtimes.json` is a file a person can open and edit, so a pin that
    /// escapes the runtimes folder has to be refused rather than obeyed.
    #[tokio::test]
    async fn a_pin_that_points_out_of_the_profile_is_refused() {
        let dir = scratch("escape");
        let runtimes = runtimes(&dir, passes);
        let mut ledger = runtimes.read().expect("read");
        ledger.set("llama.cpp", managed("../../.agent-browser/browsers"));
        runtimes.store.write(&ledger).expect("wrote");

        assert!(matches!(
            runtimes.remove("llama.cpp"),
            Err(Error::Outside { .. })
        ));
        assert!(matches!(
            runtimes.remove_unused("../settings.json"),
            Err(Error::Outside { .. })
        ));
    }

    #[tokio::test]
    async fn a_directory_no_row_runs_from_is_unused() {
        let dir = scratch("unused");
        let runtimes = already_managed(&dir, "llama.cpp", "b10816", passes);
        let stray = runtimes.runtimes_dir().join("llama.cpp-b10700");
        std::fs::create_dir_all(&stray).expect("made a stray pin");
        std::fs::write(stray.join("llama-server.exe"), vec![0u8; 1024 * 1024]).expect("wrote");

        let unused = runtimes.unused().expect("diffed");
        assert_eq!(unused.len(), 1);
        assert_eq!(unused[0].name, "llama.cpp-b10700");
        assert!((unused[0].size_mib - 1.0).abs() < 0.01);

        runtimes.remove_unused("llama.cpp-b10700").expect("removed");
        assert!(runtimes.unused().expect("diffed").is_empty());
    }

    /// Section 5's first case, and the one a directory diff alone would miss.
    #[tokio::test]
    async fn a_cancelled_fetchs_partial_file_is_unused_rather_than_invisible() {
        let dir = scratch("unused-part");
        let runtimes = runtimes(&dir, passes);
        std::fs::create_dir_all(runtimes.runtimes_dir()).expect("made the folder");
        std::fs::write(
            runtimes
                .runtimes_dir()
                .join("cudart-llama-bin-win-cuda-13.3-x64.zip.part"),
            vec![0u8; 2 * 1024 * 1024],
        )
        .expect("wrote");

        let unused = runtimes.unused().expect("diffed");
        assert_eq!(unused.len(), 1);
        assert!((unused[0].size_mib - 2.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn a_row_the_ledger_runs_from_is_never_offered_as_unused() {
        let dir = scratch("claimed");
        let runtimes = already_managed(&dir, "llama.cpp", "b10816", passes);
        assert!(runtimes.unused().expect("diffed").is_empty());
        assert!(matches!(
            runtimes.remove_unused("llama.cpp-b10816"),
            Err(Error::NotAllowed { .. })
        ));
    }
}
