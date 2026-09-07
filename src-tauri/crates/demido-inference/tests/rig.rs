//! Where the live rig is, and the permit that keeps one model on the card.
//!
//! Shared by the suites that need a real model. Not a test file of its own: it
//! is included by the ones that are, because a `tests/` file is its own crate
//! and a helper compiled into each is cheaper than a crate to hold it.
//!
//! **It fails rather than skips when the rig is absent.** A live suite that
//! quietly passes on a machine with no models is the "built but never driven"
//! failure [`docs/rules/done.md`] exists to prevent, and it would pass hardest
//! in CI, where it proves the least.

#![allow(dead_code)]
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::path::PathBuf;
use std::time::Duration;

use demido_inference::{LlamaCppConfig, Offload};

/// The card holds one model at a time, so the suite holds one too.
///
/// `done.md` measured it: the breadth model at 32k leaves 271 MiB on a 12 GB
/// card. Two resident models is not a slow test, it is a failed load reported
/// as a product defect. Process-wide because the test harness runs cases on
/// threads in one process, which is exactly the scope that needs bounding.
pub static ONE_MODEL_AT_A_TIME: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

/// A model the suite can run, named by the role it plays rather than by its
/// weights. `done.md`: they are not interchangeable, and pretending they are is
/// how a quantisation artifact gets recorded as a product defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// `gemma-4-E4B-it` Q8_0. Every scenario, every iteration.
    Development,
    /// `gemma-4-26B-A4B-it-UD` IQ2_M. Must be green before a slice closes.
    Reference,
    /// `Qwen3.8-27B-Uncensored` IQ2_M. Once per slice. A red here alone is a
    /// note rather than a defect, though after #19's three probes out of three
    /// it is surprising enough to look at twice.
    Breadth,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Development, Tier::Reference, Tier::Breadth];

    pub fn label(self) -> &'static str {
        match self {
            Tier::Development => "development",
            Tier::Reference => "reference",
            Tier::Breadth => "breadth",
        }
    }

    /// The file, relative to the models root.
    fn file(self) -> &'static str {
        match self {
            Tier::Development => "unsloth/gemma-4-E4B-it-GGUF/gemma-4-E4B-it-Q8_0.gguf",
            Tier::Reference => "unsloth/gemma-4-26B-A4B-it-GGUF/gemma-4-26B-A4B-it-UD-IQ2_M.gguf",
            Tier::Breadth => {
                "JonathanColetti/Qwen3.8-27B-Uncensored-GGUF/Qwen3.8-27B-Uncensored-IQ2_M.gguf"
            }
        }
    }

    pub fn path(self) -> PathBuf {
        models_root().join(self.file())
    }
}

/// `llama-server`, at the pinned build.
///
/// `DEMIDO_LLAMA_BIN` overrides it. There is no search of `PATH`: a suite that
/// finds whichever build happens to be installed cannot report the SHA a
/// closing comment owes ([`docs/rules/done.md`](../../../../docs/rules/done.md)),
/// and a red that turns out to be an upstream fix you did not have is a day
/// spent on nothing.
pub fn binary() -> PathBuf {
    match std::env::var("DEMIDO_LLAMA_BIN") {
        Ok(path) => PathBuf::from(path),
        Err(_) => PathBuf::from("S:/Development/llama.cpp-release/bin/llama-server.exe"),
    }
}

/// Where the GGUFs are. `DEMIDO_MODELS` overrides it.
pub fn models_root() -> PathBuf {
    match std::env::var("DEMIDO_MODELS") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_default();
            PathBuf::from(home).join(".lmstudio/models")
        }
    }
}

/// Refuse to run rather than pass on a machine with no rig.
pub fn require(tier: Tier) -> LlamaCppConfig {
    let binary = binary();
    assert!(
        binary.exists(),
        "no llama-server at {}. The live suite needs the pinned build (b10816, \
         427291b5). Point DEMIDO_LLAMA_BIN at it.",
        binary.display()
    );

    let model = tier.path();
    assert!(
        model.exists(),
        "no {} model at {}. Point DEMIDO_MODELS at the library root.",
        tier.label(),
        model.display()
    );

    let mut config = LlamaCppConfig::new(binary, model);
    config.alias = tier.label().to_owned();
    // Every layer on the card, or fail loudly. The point of the rig is to find
    // out whether the model fits, and `auto` would answer by quietly putting
    // half of it on the CPU and taking twenty times as long.
    config.offload = Offload::All;
    config.context_length = 4096;
    config.parallel = 1;
    config.startup_timeout = Duration::from_secs(300);
    config
}
