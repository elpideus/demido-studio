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

use std::path::{Path, PathBuf};
use std::sync::Arc;

use demido_chat::{Chat, Model};
use demido_inference::{LlamaCpp, LlamaCppConfig, Supervisor};
use demido_shell::{Debounced, Files};

/// Every subsystem, wired once.
///
/// Built at startup, handed to Tauri as managed state, and read from there by
/// every command.
#[non_exhaustive]
pub struct Wiring {
    /// The one backend, and the rule that only one model is resident.
    ///
    /// Shared with the chat rather than held beside it: two supervisors is two
    /// models on a card sized for one.
    pub inference: Arc<Inference>,
    /// What the desk looked like last time, and where the next arrangement
    /// goes.
    pub desk: Desk,
    /// The conversation: the log, the turn loop, and whatever is generating.
    pub chat: Talk,
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
/// **This alias is the wiring line.** The tile is `Files`; the second
/// implementation of that trait is `demido_shell::Memory`, which is what a
/// build with nowhere to write would be, and swapping to it is editing this
/// line and recompiling.
///
/// `Debounced` is not a second tile and does not implement `Store`. It is the
/// policy about *when* a gesture becomes a file, wrapped around whichever store
/// this line names, and it deliberately keeps no layout of its own: a type that
/// implemented `Store` while its writes were still in a queue would claim a
/// promise the contract suite tests and it only keeps eventually. See
/// `demido-shell/AGENTS.md`.
///
/// See
/// [`docs/decisions/0010-the-desk-remembers-itself.md`](../../docs/decisions/0010-the-desk-remembers-itself.md).
pub type Desk = Debounced<Files>;

/// The session log implementation this build writes.
///
/// **This alias is the wiring line.** The other implementation is
/// `demido_trace::Memory`, which is what a session the user asks not to keep
/// will be, and swapping to it is editing this line and recompiling.
pub type Trace = demido_trace::JsonLines;

/// The conversation this build runs: the turn loop over the two tiles above it.
///
/// Not a wiring line of its own. `demido_chat::Chat` implements no trait and is
/// generic over the two that matter, so this names the pair the two lines above
/// already chose. The supervisor it talks through is [`Wiring::inference`],
/// which is that line's one instance.
pub type Talk = Chat<LlamaCpp, Trace>;

/// The one session this build has, until there is a chat list to have more.
///
/// A conversation per profile, in a file the next launch reads. Chats,
/// projects and the list that holds them are their own tickets; what S1 needs
/// is that closing the window and opening it again finds the conversation
/// where it was left, and that is a path rather than a feature.
const SESSION: &str = "session";

/// Where the model comes from until something decides it properly.
///
/// The set-up wizard ([#48](https://github.com/elpideus/demido-studio/issues/48))
/// picks the runtime and the weights, and the settings ladder
/// ([#44](https://github.com/elpideus/demido-studio/issues/44)) is where they
/// are changed afterwards. Both land after this ticket, and until they do a
/// build has to get a model from somewhere or the desk has nothing to answer
/// with at all.
///
/// So: two environment variables, both naming files that must exist, and both
/// documented in the root `AGENTS.md` beside the command that uses them. An
/// absent one is not an error and not a warning. It is `Presence::Absent`,
/// which is the ordinary first launch on a machine where set-up has not run,
/// and it is the state the composer already knows how to draw.
///
/// `DEMIDO_MODEL_FILE` rather than `DEMIDO_MODEL`, because the live suites
/// already read `DEMIDO_MODELS` and that one is a library root
/// (`demido-inference/tests/rig.rs`). One character between two variables that
/// mean a file and a directory is a trap set for somebody in a hurry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rig {
    /// `llama-server`, fetched from upstream and never bundled (hard rule 3).
    pub binary: PathBuf,
    /// The GGUF to load.
    pub model: PathBuf,
}

impl Rig {
    /// The rig this machine is pointed at, or nothing.
    pub fn from_environment() -> Option<Self> {
        let binary = PathBuf::from(std::env::var_os("DEMIDO_LLAMA_BIN")?);
        let model = PathBuf::from(std::env::var_os("DEMIDO_MODEL_FILE")?);
        if !binary.is_file() || !model.is_file() {
            tracing::warn!(
                binary = %binary.display(),
                model = %model.display(),
                "the rig named by the environment is not on disk; the desk opens with nothing loaded"
            );
            return None;
        }
        Some(Self { binary, model })
    }

    /// What to start, and what the request must call it.
    ///
    /// The alias is the model file's own name, so the id in the log and the
    /// file on disk cannot come apart. `--parallel 1`, because the context
    /// length a caller asks for is divided by the slot count and this build
    /// asks for one window (`docs/rules/done.md`).
    fn model(self) -> Model<LlamaCpp> {
        // The whole file name where there is no stem, rather than a name this
        // file invented: an id nobody can trace back to a file on disk is worse
        // than an ugly one.
        let id = self
            .model
            .file_stem()
            .unwrap_or(self.model.as_os_str())
            .to_string_lossy()
            .into_owned();

        let mut config = LlamaCppConfig::new(self.binary, self.model);
        config.alias.clone_from(&id);
        config.parallel = 1;
        Model { config, id }
    }
}

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
        let sessions = profile.join("sessions").join(format!("{SESSION}.jsonl"));
        let inference = Arc::new(Inference::new());
        Ok(Self {
            desk: Desk::new(Files::in_profile(profile)),
            // The log is opened by the first thing that needs it, not here. A
            // root that opened one would make opening a window a thing that can
            // fail on a full or read-only disk, before there is a desk to
            // report it on.
            chat: Talk::new(
                SESSION,
                move || Trace::open(&sessions),
                inference.clone(),
                Rig::from_environment().map(Rig::model),
            ),
            inference,
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
