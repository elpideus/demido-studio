//! The composition root.
//!
//! `docs/rules/tiles.md`: "The composition root names exactly one
//! implementation per trait, in one place." This is that place.
//!
//! It sits in the application package rather than in `demido-core`, because
//! naming an implementation means depending on the crate that has it, and
//! `demido-core` is the bottom of the dependency graph. This package is the
//! ceiling, so it is the only one allowed to depend on every crate at once. Swapping a
//! compile-time tile is a single line here and a recompile, and the value of
//! that is only real while it stays a single line, so a trait is never
//! constructed anywhere else and no crate reaches for another crate's concrete
//! type.
//!
//! Nothing else in the workspace constructs a subsystem, which is what keeps
//! this a wiring file rather than a second place where tiles reach for each
//! other.

use std::path::Path;

use demido_inference::{LlamaCpp, Supervisor};
use demido_shell::{Debounced, Files};

/// Every subsystem, wired once.
///
/// Built at startup, handed to Tauri as managed state, and read from there by
/// every command.
#[non_exhaustive]
pub struct Wiring {
    /// The one backend, and the rule that only one model is resident.
    pub inference: Inference,
    /// What the desk looked like last time, and where the next arrangement
    /// goes.
    pub desk: Desk,
}

/// The inference implementation this build runs.
///
/// **This alias is the wiring line.** One name, one implementation, in one
/// place. The second implementation the contract suite was written for is an
/// OpenAI-compatible endpoint, and adopting it is editing this line and
/// recompiling.
pub type Inference = Supervisor<LlamaCpp>;

/// The shell layout store this build keeps the desk in.
///
/// **This alias is the wiring line**, and it names two tiles at once: where a
/// layout is kept, and when a gesture becomes a file. The second implementation
/// of the inner one is `demido_shell::Memory`, which is what a build with
/// nowhere to write would be, and swapping to it is editing this line.
///
/// See
/// [`docs/decisions/0010-the-desk-remembers-itself.md`](../../docs/decisions/0010-the-desk-remembers-itself.md).
pub type Desk = Debounced<Files>;

/// The session log implementation this build writes.
///
/// A type rather than a field on [`Wiring`], because a journal belongs to one
/// session and nothing opens a session yet: chat is
/// [#42](https://github.com/elpideus/demido-studio/issues/42). The alias is
/// here all the same, because it is the wiring line, and the point of a tile
/// being one line is only real while the line is in the one place that names
/// implementations. The other implementation is `demido_trace::Memory`, which
/// is what a session the user asks not to keep will be.
pub type Trace = demido_trace::JsonLines;

impl Wiring {
    /// The application's wiring: one implementation per trait.
    ///
    /// `profile` is the current profile's data directory, which is a Windows
    /// profile's ([`docs/rules/profiles.md`](../../docs/rules/profiles.md)).
    /// Nothing is created in it here: a store that made a directory when it was
    /// constructed would put a folder on disk for a profile that never arranged
    /// anything.
    ///
    /// Fallible from the first line, because it will be. A subsystem that
    /// cannot start is reported and skipped rather than fatal (`AGENTS.md`),
    /// so a failure here is reserved for the case where there would be no
    /// window worth opening at all.
    pub fn assemble(profile: &Path) -> demido_core::Result<Self> {
        Ok(Self {
            inference: Inference::new(),
            desk: Desk::new(Files::in_profile(profile)),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use demido_shell::{Shell, Side};

    /// A profile directory of this test's own, which nothing is expected to
    /// create.
    fn profile(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join("demido-wiring-tests")
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_root_assembles() {
        assert!(Wiring::assemble(&profile("assembles")).is_ok());
    }

    /// Assembling touches no disk. The composition root runs before the window
    /// exists, and a root that wrote to the profile would make opening one a
    /// thing that can fail on a full or read-only disk.
    #[test]
    fn assembling_writes_nothing_to_the_profile() {
        let dir = profile("untouched");
        let wiring = Wiring::assemble(&dir).expect("assembled");
        assert!(
            !dir.exists(),
            "no profile directory until something is saved"
        );
        assert!(wiring.desk.read().is_none());
    }

    /// The desk a fresh profile opens on, and the arrangement it keeps once
    /// something has been moved. Dropping the wiring is what flushes it, which
    /// is the shape a closing window has.
    #[test]
    fn an_arranged_desk_survives_the_process_that_arranged_it() {
        let dir = profile("remembered");
        let arranged = Shell { rail: Side::Right };
        {
            let wiring = Wiring::assemble(&dir).expect("assembled");
            wiring.desk.remember(arranged);
        }
        let reopened = Wiring::assemble(&dir).expect("assembled again");
        assert_eq!(reopened.desk.read(), Some(arranged));
    }

    /// Assembling starts nothing. Startup never blocks (`AGENTS.md`), and a
    /// root that loaded a model would make opening the window wait on several
    /// gigabytes off disk.
    #[tokio::test]
    async fn nothing_is_running_until_something_asks() {
        let wiring = Wiring::assemble(&profile("idle")).expect("assembled");
        assert!(wiring.inference.current().await.is_none());
    }
}
