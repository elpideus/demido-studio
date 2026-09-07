//! One resident model at a time, and who owns its lifetime.
//!
//! [`crate::LlamaCpp`] starts a server and kills it when the handle goes away.
//! That is enough for a probe, which starts one, proves a model answers and
//! stops it again. It is not enough for chat, which needs the server that
//! answered to still be there for the next message, and needs the message after
//! that to reuse it rather than load the weights a second time.
//!
//! So the lifetime belongs here rather than to whichever call happened to need
//! a model first. Every caller asks the same question, "give me the backend for
//! this configuration", and the supervisor decides whether that means starting
//! one, handing back the running one, or replacing it.
//!
//! Three rules, and they are the whole design:
//!
//! **At most one.** [`docs/rules/done.md`](../../../../docs/rules/done.md)
//! measured the card this is built for: the breadth model at 32k leaves 271 MiB
//! on a 12 GB card, which is less than one browser window. Holding one model
//! resident is not tidiness, it is the only way that row loads at all.
//!
//! **The old one stops before the new one starts.** The memory has to be free
//! before it can be asked for again. Doing it the other way round fails on
//! exactly the machines this product is for.
//!
//! **A dead backend is not a running one.** A process that exited leaves a
//! handle that looks fine from the outside, so whether the running backend is
//! still good is asked on every request rather than assumed from the fact that
//! it started once.
//!
//! Generic over what it supervises, and not for the sake of abstraction: it is
//! the only way those rules get tested. Every one of them is about *when* a
//! process is started, and none of them can be observed through a real
//! `llama.cpp` without a card and several gigabytes.

use std::sync::Arc;

use tokio::sync::Mutex;

use crate::backend::{Backend, Result};

/// The one running backend, and whatever it takes to keep that true.
pub struct Supervisor<B: Backend> {
    /// One lock over the whole decision, held across a start. Two callers
    /// asking at once is the normal case, and the second has to wait for the
    /// first backend rather than race it into starting a duplicate that then
    /// fights it for the card.
    live: Mutex<Option<Live<B>>>,
}

struct Live<B: Backend> {
    config: B::Config,
    backend: Arc<B>,
}

impl<B: Backend> Default for Supervisor<B> {
    fn default() -> Self {
        Self {
            live: Mutex::new(None),
        }
    }
}

impl<B: Backend> Supervisor<B> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The running backend for this configuration, starting one if there is not
    /// one already.
    ///
    /// Returns a backend that was alive a moment ago, which is all anything can
    /// promise: a process can die between the check and the request. That is
    /// why a failed request is reported rather than quietly retried here.
    pub async fn ensure(&self, config: B::Config) -> Result<Arc<B>> {
        let mut live = self.live.lock().await;

        if let Some(running) = live.as_ref() {
            if running.config == config && running.backend.ready().await {
                return Ok(running.backend.clone());
            }
            // Taken before the stop, so a stop that hangs cannot leave a stopped
            // backend recorded as the live one.
            if let Some(previous) = live.take() {
                previous.backend.stop().await;
            }
        }

        let backend = Arc::new(B::start(config.clone()).await?);
        *live = Some(Live {
            config,
            backend: backend.clone(),
        });
        Ok(backend)
    }

    /// Whatever is running, without starting anything.
    ///
    /// What a status indicator asks. `None` means nothing is loaded, not that
    /// nothing can be: a caller that wants something to talk to wants
    /// [`Supervisor::ensure`].
    pub async fn current(&self) -> Option<Arc<B>> {
        let live = self.live.lock().await;
        let running = live.as_ref()?;
        if running.backend.ready().await {
            return Some(running.backend.clone());
        }
        None
    }

    /// Stop whatever is running and hold nothing.
    ///
    /// The user-facing "give me my card back", and what shutdown calls:
    /// dropping the supervisor kills the process without waiting for it to be
    /// gone.
    pub async fn shutdown(&self) {
        let taken = self.live.lock().await.take();
        if let Some(live) = taken {
            live.backend.stop().await;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use super::*;
    use crate::backend::{Cancel, ChunkStream, Error};
    use crate::model::{Loaded, Request};

    /// What happened, in order. The order is the point: "the old one stopped
    /// before the new one started" is not observable any other way.
    #[derive(Debug, Default)]
    struct Journal(StdMutex<Vec<String>>);

    impl Journal {
        fn note(&self, entry: impl Into<String>) {
            if let Ok(mut entries) = self.0.lock() {
                entries.push(entry.into());
            }
        }

        fn entries(&self) -> Vec<String> {
            self.0.lock().map(|e| e.clone()).unwrap_or_default()
        }
    }

    /// A backend that is entirely bookkeeping.
    #[derive(Debug)]
    struct Fake {
        name: String,
        alive: AtomicBool,
        journal: Arc<Journal>,
    }

    #[derive(Clone, Debug)]
    struct FakeConfig {
        name: String,
        /// Fails to start, the way a model too large for the card does.
        broken: bool,
        journal: Arc<Journal>,
    }

    impl FakeConfig {
        fn named(name: &str, journal: &Arc<Journal>) -> Self {
            Self {
                name: name.into(),
                broken: false,
                journal: journal.clone(),
            }
        }
    }

    /// Two configurations are the same backend when they load the same thing.
    /// The journal is test scaffolding and has no say in that.
    impl PartialEq for FakeConfig {
        fn eq(&self, other: &Self) -> bool {
            self.name == other.name && self.broken == other.broken
        }
    }

    #[async_trait::async_trait]
    impl Backend for Fake {
        type Config = FakeConfig;

        fn name() -> &'static str {
            "fake"
        }

        async fn start(config: FakeConfig) -> Result<Self> {
            // Long enough that a second caller arriving during a start really
            // is racing it rather than finding it already finished.
            tokio::time::sleep(Duration::from_millis(20)).await;
            if config.broken {
                config.journal.note(format!("failed {}", config.name));
                return Err(Error::DidNotStart {
                    backend: "fake".into(),
                    detail: "it does not fit".into(),
                });
            }
            config.journal.note(format!("started {}", config.name));
            Ok(Fake {
                name: config.name,
                alive: AtomicBool::new(true),
                journal: config.journal,
            })
        }

        async fn ready(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }

        async fn loaded(&self) -> Result<Loaded> {
            Ok(Loaded {
                id: self.name.clone(),
                size: None,
            })
        }

        async fn context_length(&self) -> Result<u32> {
            Ok(0)
        }

        async fn generate(&self, _request: Request, _cancel: Cancel) -> Result<ChunkStream> {
            Err(Error::Refused {
                backend: "fake".into(),
                detail: "this backend is lifetime bookkeeping and does not generate".into(),
            })
        }

        async fn stop(&self) {
            if self.alive.swap(false, Ordering::SeqCst) {
                self.journal.note(format!("stopped {}", self.name));
            }
        }
    }

    #[tokio::test]
    async fn the_second_ask_for_the_same_model_reuses_the_running_backend() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();
        let config = FakeConfig::named("tiny", &journal);

        let first = supervisor.ensure(config.clone()).await.expect("started");
        let second = supervisor.ensure(config).await.expect("still running");

        assert!(
            Arc::ptr_eq(&first, &second),
            "reloading weights that are already loaded is the cost this exists to avoid"
        );
        assert_eq!(journal.entries(), vec!["started tiny"]);
    }

    #[tokio::test]
    async fn a_different_model_frees_the_card_before_it_asks_for_it() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();

        supervisor
            .ensure(FakeConfig::named("small", &journal))
            .await
            .expect("started");
        supervisor
            .ensure(FakeConfig::named("large", &journal))
            .await
            .expect("started");

        assert_eq!(
            journal.entries(),
            vec!["started small", "stopped small", "started large"],
            "starting the second before releasing the first fails on exactly \
             the machines that only just fit the first"
        );
    }

    #[tokio::test]
    async fn a_backend_that_died_is_replaced_rather_than_handed_out() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();
        let config = FakeConfig::named("tiny", &journal);

        let first = supervisor.ensure(config.clone()).await.expect("started");
        // It crashed. Nothing told the supervisor.
        first.alive.store(false, Ordering::SeqCst);

        let second = supervisor.ensure(config).await.expect("started again");
        assert!(!Arc::ptr_eq(&first, &second));
        assert!(second.ready().await);
        assert!(
            supervisor.current().await.is_some(),
            "the replacement is the live one"
        );
    }

    #[tokio::test]
    async fn nothing_is_reported_running_after_it_dies() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();
        let backend = supervisor
            .ensure(FakeConfig::named("tiny", &journal))
            .await
            .expect("started");

        backend.alive.store(false, Ordering::SeqCst);
        assert!(
            supervisor.current().await.is_none(),
            "a status indicator must not report a dead process as loaded"
        );
    }

    /// A backend that fails to start is reported and skipped, never fatal, and
    /// what it must not do is leave the previous one recorded as still serving
    /// after it was stopped to make room.
    #[tokio::test]
    async fn a_failed_start_leaves_nothing_behind() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();
        supervisor
            .ensure(FakeConfig::named("small", &journal))
            .await
            .expect("started");

        let mut broken = FakeConfig::named("huge", &journal);
        broken.broken = true;
        let error = supervisor
            .ensure(broken)
            .await
            .expect_err("it does not fit");
        assert!(matches!(error, Error::DidNotStart { .. }), "{error}");

        assert!(
            supervisor.current().await.is_none(),
            "the previous backend was stopped to make room and must not be \
             reported as still serving"
        );
        assert_eq!(
            journal.entries(),
            vec!["started small", "stopped small", "failed huge"]
        );
    }

    #[tokio::test]
    async fn two_callers_arriving_together_get_one_backend() {
        let journal = Arc::new(Journal::default());
        let supervisor = Arc::new(Supervisor::<Fake>::new());
        let config = FakeConfig::named("tiny", &journal);

        let one = {
            let supervisor = supervisor.clone();
            let config = config.clone();
            tokio::spawn(async move { supervisor.ensure(config).await })
        };
        let two = {
            let supervisor = supervisor.clone();
            tokio::spawn(async move { supervisor.ensure(config).await })
        };

        let one = one.await.expect("joined").expect("started");
        let two = two.await.expect("joined").expect("started");

        assert!(Arc::ptr_eq(&one, &two));
        assert_eq!(
            journal.entries(),
            vec!["started tiny"],
            "two callers asking at once is the normal case"
        );
    }

    #[tokio::test]
    async fn shutting_down_releases_the_card_and_can_be_asked_twice() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();
        supervisor
            .ensure(FakeConfig::named("tiny", &journal))
            .await
            .expect("started");

        supervisor.shutdown().await;
        supervisor.shutdown().await;

        assert!(supervisor.current().await.is_none());
        assert_eq!(journal.entries(), vec!["started tiny", "stopped tiny"]);
    }

    #[tokio::test]
    async fn a_model_can_be_started_again_after_a_shutdown() {
        let journal = Arc::new(Journal::default());
        let supervisor = Supervisor::<Fake>::new();
        let config = FakeConfig::named("tiny", &journal);

        supervisor.ensure(config.clone()).await.expect("started");
        supervisor.shutdown().await;
        supervisor.ensure(config).await.expect("started again");

        assert_eq!(
            journal.entries(),
            vec!["started tiny", "stopped tiny", "started tiny"]
        );
    }
}
