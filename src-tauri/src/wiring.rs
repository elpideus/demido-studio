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

use demido_inference::{LlamaCpp, Supervisor};

/// Every subsystem, wired once.
///
/// Built at startup, handed to Tauri as managed state, and read from there by
/// every command.
#[derive(Default)]
#[non_exhaustive]
pub struct Wiring {
    /// The one backend, and the rule that only one model is resident.
    pub inference: Inference,
}

/// The inference implementation this build runs.
///
/// **This alias is the wiring line.** One name, one implementation, in one
/// place. The second implementation the contract suite was written for is an
/// OpenAI-compatible endpoint, and adopting it is editing this line and
/// recompiling.
pub type Inference = Supervisor<LlamaCpp>;

impl Wiring {
    /// The application's wiring: one implementation per trait.
    ///
    /// Fallible from the first line, because it will be. A subsystem that
    /// cannot start is reported and skipped rather than fatal (`AGENTS.md`),
    /// so a failure here is reserved for the case where there would be no
    /// window worth opening at all.
    pub fn assemble() -> demido_core::Result<Self> {
        Ok(Self {
            inference: Inference::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn the_root_assembles() {
        assert!(Wiring::assemble().is_ok());
    }

    /// Assembling starts nothing. Startup never blocks (`AGENTS.md`), and a
    /// root that loaded a model would make opening the window wait on several
    /// gigabytes off disk.
    #[tokio::test]
    async fn nothing_is_running_until_something_asks() {
        let wiring = Wiring::assemble().expect("assembled");
        assert!(wiring.inference.current().await.is_none());
    }
}
