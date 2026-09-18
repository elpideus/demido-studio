//! What the two live suites of this crate share: a backend that records what it
//! was handed, and the prune that turns a run's log into a committed fixture.
//!
//! Included by path rather than made a crate, like `demido-inference`'s rig and
//! for the same reason: a `tests/` file is its own crate, and a helper compiled
//! into each suite that needs it is cheaper than a crate to hold it. Two copies
//! would be two places for "what the backend was actually handed" to mean
//! different things.

#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
// A test asserts by panicking. The workspace denies these in application code,
// where a panic is a window that vanishes; here a panic is the report.

use std::path::Path;
use std::sync::Mutex;

use serde_json::json;

use demido_inference::{Backend, Cancel, ChunkStream, LlamaCpp, LlamaCppConfig, Loaded, Request};

/// Every request the backend was handed, this process wide.
///
/// The live suites already run one model at a time under a process-wide permit
/// and `--test-threads=1`, so a process-wide recorder has exactly the scope the
/// permit has. A scenario clears it before it asks anything.
static SENT: Mutex<Vec<Request>> = Mutex::new(Vec::new());

/// `llama.cpp`, with a note taken of what it was actually given.
///
/// The seam is here for this: everything above `demido-inference` is written
/// against [`Backend`] rather than against `llama.cpp`, so a wrapper that
/// forwards every method and records one argument is a backend like any other
/// and needs nothing from the crate under test. What it buys is the difference
/// between "the loop meant to send six tools" and "the backend received six
/// tools", which is the only version of that sentence worth asserting.
pub struct Watching(LlamaCpp);

impl Watching {
    /// What has been sent since the last [`Watching::forget`], in the order it
    /// was sent.
    pub fn sent() -> Vec<Request> {
        SENT.lock().unwrap_or_else(|held| held.into_inner()).clone()
    }

    pub fn forget() {
        SENT.lock().unwrap_or_else(|held| held.into_inner()).clear();
    }
}

#[async_trait::async_trait]
impl Backend for Watching {
    type Config = LlamaCppConfig;

    fn name() -> &'static str {
        LlamaCpp::name()
    }

    async fn start(config: Self::Config) -> demido_inference::Result<Self> {
        LlamaCpp::start(config).await.map(Self)
    }

    fn with_context_length(config: Self::Config, tokens: u32) -> Self::Config {
        LlamaCpp::with_context_length(config, tokens)
    }

    fn with_slots(config: Self::Config, slots: u32) -> Self::Config {
        LlamaCpp::with_slots(config, slots)
    }

    async fn ready(&self) -> bool {
        self.0.ready().await
    }

    async fn loaded(&self) -> demido_inference::Result<Loaded> {
        self.0.loaded().await
    }

    async fn context_length(&self) -> demido_inference::Result<u32> {
        self.0.context_length().await
    }

    async fn slots(&self) -> demido_inference::Result<u32> {
        self.0.slots().await
    }

    async fn generate(
        &self,
        request: Request,
        cancel: Cancel,
    ) -> demido_inference::Result<ChunkStream> {
        SENT.lock()
            .unwrap_or_else(|held| held.into_inner())
            .push(request.clone());
        self.0.generate(request, cancel).await
    }

    async fn stop(&self) {
        self.0.stop().await;
    }
}

/// Write the log out as a fixture: the same events, with the two things that
/// differ between two runs of the same scenario taken out.
///
/// A timestamp and a scratch directory are not evidence, they are the machine
/// that produced the evidence, and a fixture carrying them is one that cannot be
/// diffed against the next run. Nothing else is touched: the model's own words
/// stay exactly as it said them, which is the whole point of committing it.
pub fn prune(from: &Path, to: &Path, project: &Path) {
    let raw = std::fs::read_to_string(from).expect("read the log");
    let mut pruned = String::new();

    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let mut event: serde_json::Value = serde_json::from_str(line).expect("an event");
        event["at"] = json!(0);
        pruned.push_str(&scrub(
            &serde_json::to_string(&event).expect("an event"),
            project,
        ));
        pruned.push('\n');
    }

    std::fs::write(to, pruned).expect("kept the log");
}

/// Every spelling of the scratch project's path, replaced by a name.
///
/// Three spellings, because the same directory reaches JSON as itself, as
/// itself with the separators escaped, and as itself with them turned round by
/// whatever produced the string. Missing one is a fixture that carries a
/// machine's home directory into the repo, so the last thing this does is check.
pub fn scrub(text: &str, project: &Path) -> String {
    let workspace = project.display().to_string();
    let mut scrubbed = text.to_owned();
    for spelling in [
        workspace.clone(),
        workspace.replace('\\', "\\\\"),
        workspace.replace('\\', "/"),
    ] {
        scrubbed = scrubbed.replace(&spelling, "<workspace>");
    }
    assert!(
        !scrubbed.contains(&workspace),
        "a path from this machine survived the prune: {scrubbed}"
    );
    scrubbed
}
