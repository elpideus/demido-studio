//! A backend that is entirely bookkeeping.
//!
//! Every rule the turn loop has is about ordering: what is recorded before what
//! is sent, what a stop leaves behind, which call runs, what the next request
//! carries. None of that can be observed through a real `llama.cpp` without a
//! card and several gigabytes, so the loop is driven by a script: a canned call,
//! then a canned answer.
//!
//! It is a real implementation rather than a mock, and the difference is one
//! file: `tests/scripted_contract.rs` holds it to [`crate::contract`] exactly as
//! `llama.cpp` is held, so a loop that works against a script is working against
//! the promises every backend keeps rather than against a fake that keeps its
//! own ([`docs/rules/tiles.md`](../../../../docs/rules/tiles.md)).
//!
//! It ships in the library rather than under `cfg(test)` for the reason the
//! contract suite does: more than one crate's tests are written against it.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::backend::{Backend, Cancel, ChunkStream, Error, Result};
use crate::model::{Chunk, FinishReason, Loaded, Request, ToolCall, Usage};

/// One piece of a scripted reply, handed out as one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Say(String),
    Think(String),
    Call(ToolCall),
}

/// What the backend says, and how it behaves.
///
/// Also its configuration, so it is what a supervisor compares, and what a test
/// keeps a clone of to read back what was sent: the recorder is shared by every
/// clone.
#[derive(Clone)]
pub struct Script {
    model: String,
    /// The reply to each generation in turn. The last one is repeated once they
    /// run out, so a script that says one thing says it every time.
    replies: Vec<Vec<Step>>,
    pause: Duration,
    context: u32,
    slots: u32,
    /// Why it will not start, the way a model too large for the card will not.
    refusal: Option<String>,
    seen: Arc<Mutex<Vec<Request>>>,
    /// Shared by every clone, because a crash is staged from outside whichever
    /// backend happens to be running.
    alive: Arc<AtomicBool>,
}

impl Script {
    /// A script serving the model named `model`, which is the one name its
    /// requests may carry. It says nothing until told to.
    pub fn serving(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            replies: Vec::new(),
            pause: Duration::from_millis(1),
            context: 4096,
            slots: 1,
            refusal: None,
            seen: Arc::new(Mutex::new(Vec::new())),
            alive: Arc::new(AtomicBool::new(true)),
        }
    }

    /// The next generation replies with these steps.
    #[must_use]
    pub fn then(mut self, steps: Vec<Step>) -> Self {
        self.replies.push(steps);
        self
    }

    /// The next generation answers with these tokens, one chunk each, so a stop
    /// can land in the middle of them.
    #[must_use]
    pub fn then_say(self, tokens: &[&str]) -> Self {
        self.then(
            tokens
                .iter()
                .map(|token| Step::Say((*token).to_owned()))
                .collect(),
        )
    }

    /// The next generation asks for one call and says nothing else.
    #[must_use]
    pub fn then_call(self, id: &str, name: &str, arguments: &str) -> Self {
        self.then(vec![Step::Call(ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        })])
    }

    /// How long each step takes. Long enough, and a stop is a generation in
    /// flight rather than one that had already finished.
    #[must_use]
    pub fn pausing(mut self, pause: Duration) -> Self {
        self.pause = pause;
        self
    }

    /// It refuses to start, saying why.
    #[must_use]
    pub fn refusing_to_start(mut self, detail: impl Into<String>) -> Self {
        self.refusal = Some(detail.into());
        self
    }

    /// Every request any backend started from this script was handed, in
    /// order. How "what was sent" is asserted: at the seam, on what arrived.
    pub fn requests(&self) -> Vec<Request> {
        self.seen
            .lock()
            .map(|seen| seen.clone())
            .unwrap_or_default()
    }

    /// The process died, and nothing told anybody.
    pub fn crash(&self) {
        self.alive.store(false, Ordering::SeqCst);
    }

    /// It can be started again.
    pub fn revive(&self) {
        self.alive.store(true, Ordering::SeqCst);
    }

    fn reply(&self, generation: usize) -> Vec<Step> {
        self.replies
            .get(generation)
            .or(self.replies.last())
            .cloned()
            .unwrap_or_default()
    }
}

/// Two scripts are the same backend when they would load the same thing and
/// say the same things. The recorder and the crash switch have no say in it.
impl PartialEq for Script {
    fn eq(&self, other: &Self) -> bool {
        self.model == other.model
            && self.replies == other.replies
            && self.pause == other.pause
            && self.context == other.context
            && self.refusal == other.refusal
    }
}

impl std::fmt::Debug for Script {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Script")
            .field("model", &self.model)
            .field("replies", &self.replies.len())
            .field("refusal", &self.refusal)
            .finish()
    }
}

/// A running script.
pub struct Scripted {
    script: Script,
    /// This instance has not been stopped. Per backend rather than per script,
    /// because a supervisor replacing one backend with another stops the first,
    /// and a flag shared with the configuration would mark the replacement dead
    /// before it answered anything.
    running: AtomicBool,
    /// How many generations this backend has started, which is which reply the
    /// next one gets.
    generated: AtomicUsize,
}

#[async_trait::async_trait]
impl Backend for Scripted {
    type Config = Script;

    fn name() -> &'static str {
        "scripted"
    }

    fn with_context_length(mut config: Script, tokens: u32) -> Script {
        config.context = tokens;
        config
    }

    fn with_slots(mut config: Script, slots: u32) -> Script {
        config.slots = slots.max(1);
        config
    }

    async fn start(config: Script) -> Result<Self> {
        if let Some(detail) = &config.refusal {
            return Err(Error::DidNotStart {
                backend: Self::name().to_owned(),
                detail: detail.clone(),
            });
        }
        Ok(Self {
            script: config,
            running: AtomicBool::new(true),
            generated: AtomicUsize::new(0),
        })
    }

    async fn ready(&self) -> bool {
        self.running.load(Ordering::SeqCst) && self.script.alive.load(Ordering::SeqCst)
    }

    async fn loaded(&self) -> Result<Loaded> {
        Ok(Loaded {
            id: self.script.model.clone(),
            size: None,
        })
    }

    async fn context_length(&self) -> Result<u32> {
        Ok(self.script.context)
    }

    /// What it was told to open, which for a script is the same thing as what
    /// it opened: there is no pool to run out of. The cases that matter are
    /// the caller's, and they are about which number was asked for.
    async fn slots(&self) -> Result<u32> {
        Ok(self.script.slots)
    }

    async fn generate(&self, request: Request, cancel: Cancel) -> Result<ChunkStream> {
        if let Ok(mut seen) = self.script.seen.lock() {
            seen.push(request.clone());
        }
        if request.model != self.script.model {
            return Err(Error::Refused {
                backend: Self::name().to_owned(),
                detail: format!(
                    "this script serves {}, not {}",
                    self.script.model, request.model
                ),
            });
        }

        let steps = self
            .script
            .reply(self.generated.fetch_add(1, Ordering::SeqCst));
        let pause = self.script.pause;

        Ok(Box::pin(async_stream::stream! {
            let mut produced = 0u32;
            let mut called = false;
            for step in steps {
                tokio::select! {
                    // Checked first, so a cancel that has already landed ends
                    // the stream rather than racing the next step.
                    biased;
                    () = cancel.cancelled() => {
                        yield Ok(Chunk::Done { reason: FinishReason::Cancelled, usage: usage(produced) });
                        return;
                    }
                    () = tokio::time::sleep(pause) => {}
                }
                produced += 1;
                yield Ok(match step {
                    Step::Say(text) => Chunk::Text { text },
                    Step::Think(text) => Chunk::Thinking { text },
                    Step::Call(call) => {
                        called = true;
                        Chunk::Call { call }
                    }
                });
            }
            // A cancel that arrived after the last step still ends the stream
            // as a stop, because it is one: the caller asked before it read
            // the end.
            let reason = if cancel.is_cancelled() {
                FinishReason::Cancelled
            } else if called {
                FinishReason::ToolCalls
            } else {
                FinishReason::Stop
            };
            yield Ok(Chunk::Done { reason, usage: usage(produced) });
        }))
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// A made-up count, one token per step, so the cost axis has numbers to carry.
fn usage(produced: u32) -> Usage {
    Usage {
        prompt_tokens: 7,
        completion_tokens: produced,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use futures_util::StreamExt;

    use super::*;
    use crate::model::Message;

    fn request() -> Request {
        Request {
            model: "scripted".into(),
            messages: vec![Message::user("hello")],
            tools: Vec::new(),
            options: crate::model::Options::default(),
        }
    }

    async fn chunks(backend: &Scripted) -> Vec<Chunk> {
        backend
            .generate(request(), Cancel::new())
            .await
            .expect("generate")
            .map(|chunk| chunk.expect("a chunk"))
            .collect()
            .await
    }

    #[tokio::test]
    async fn each_generation_gets_the_next_reply_and_the_last_one_repeats() {
        let script = Script::serving("scripted")
            .then_call("call-1", "read_file", "{}")
            .then_say(&["done"]);
        let backend = Scripted::start(script.clone()).await.expect("started");

        let first = chunks(&backend).await;
        assert!(matches!(first[0], Chunk::Call { .. }), "{first:?}");
        assert!(matches!(
            first.last(),
            Some(Chunk::Done {
                reason: FinishReason::ToolCalls,
                ..
            })
        ));

        for _ in 0..2 {
            let later = chunks(&backend).await;
            assert_eq!(
                later[0],
                Chunk::Text {
                    text: "done".into()
                }
            );
            assert!(matches!(
                later.last(),
                Some(Chunk::Done {
                    reason: FinishReason::Stop,
                    ..
                })
            ));
        }
        assert_eq!(script.requests().len(), 3, "every request is recorded");
    }

    #[tokio::test]
    async fn a_crash_is_a_backend_that_stops_being_ready() {
        let script = Script::serving("scripted");
        let backend = Scripted::start(script.clone()).await.expect("started");
        script.crash();
        assert!(!backend.ready().await);
        script.revive();
        assert!(backend.ready().await);
    }
}
