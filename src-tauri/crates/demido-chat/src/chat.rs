//! The session, the assembly it composes, and the loop that runs a turn.
//!
//! One type, [`Chat`], holding three things: a journal, a supervisor, and the
//! cancellation token of whatever is generating right now. It holds no
//! messages, and that absence is the design (see the crate docs).

use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::Serialize;

use demido_inference::{Backend, Cancel, Chunk, FinishReason, Options, Role, Supervisor, Usage};
use demido_settings::{Ladder, Resolved, Settings};
use demido_trace::{Journal, Replay, Session, SessionId, Source};

use crate::presence::Presence;
use crate::update::Update;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Asked to answer with nothing loaded. The composer is disabled in this
    /// state, so reaching it means the window and the loop disagreed, and the
    /// loop is the one that decides.
    #[error("nothing is loaded to answer with: {0:?}")]
    NotReady(Presence),

    /// The backend was there a moment ago and is not now. Reported rather than
    /// fatal: the desk stays usable and the next attempt starts it again.
    #[error("the backend stopped answering")]
    Gone,

    /// A stream that ended without the one `Done` the contract promises. Its
    /// own error rather than a silent empty answer, because a turn that never
    /// ends is the failure the contract suite exists to catch, and this is what
    /// it looks like from above.
    #[error("the generation ended without saying how")]
    Unfinished,

    #[error(transparent)]
    Journal(#[from] demido_trace::Error),

    #[error(transparent)]
    Backend(#[from] demido_inference::Error),
}

impl Error {
    /// The machine-readable half, for the `turn/failure` event's `kind`.
    fn kind(&self) -> &'static str {
        match self {
            Error::NotReady(_) => "not-ready",
            Error::Gone => "gone",
            Error::Unfinished => "unfinished",
            Error::Journal(_) => "journal",
            Error::Backend(_) => "backend",
        }
    }
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Journal(journal) => journal.into(),
            Error::Backend(backend) => backend.into(),
            other => demido_core::Error::unavailable("the model", other.to_string()),
        }
    }
}

/// What this chat talks to: what it takes to start, and what to call it.
///
/// The two are separate because they come from different places. The
/// configuration is everything that decides which weights get loaded, and it is
/// what [`Supervisor`] compares to decide whether the running backend is the
/// one being asked for. The id is what `Request::model` has to carry, which the
/// backend itself declares and refuses any other name for.
#[derive(Debug, PartialEq, Eq)]
pub struct Model<B: Backend> {
    pub config: B::Config,
    pub id: String,
}

/// Written out rather than derived, because a derive would ask for `B: Clone`
/// and `B` is the backend itself rather than anything anybody clones. What has
/// to be cloneable is the configuration, which is the thing the supervisor
/// compares.
impl<B: Backend> Clone for Model<B> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            id: self.id.clone(),
        }
    }
}

/// One thing said, as the transcript draws it.
///
/// A projection of the log, computed on demand. Deliberately narrower than
/// [`demido_trace::Exchange`], which also carries the source and the weight:
/// those are the Session Monitor's axes, and a bubble handed them is a bubble
/// somebody eventually renders them in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    /// Where it sits on the log. The transcript keys a bubble by it, and a
    /// later turn carries it back into an assembly by it.
    pub seq: u64,
    pub turn: u32,
    pub role: Role,
    pub text: String,
}

/// A finished turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub turn: u32,
    /// Where the answer sits on the log.
    pub seq: u64,
    pub text: String,
    pub thinking: String,
    pub reason: FinishReason,
    pub usage: Usage,
}

/// The sampling settings of a turn, out of the ladder and nowhere else.
///
/// Deliberately not [`Options::default`]: a default here would be a second
/// place a temperature can come from, and the whole point of the ladder is that
/// there is one. Only the settings this slice exposes are read; `max_tokens`
/// and `seed` have no rows in the schema, so a request carries neither, and the
/// server decides.
fn options(resolved: &Resolved) -> Options {
    Options {
        temperature: resolved.temperature(),
        max_tokens: None,
        seed: None,
    }
}

/// One conversation.
///
/// Generic over both seams it sits between, and neither for the sake of
/// abstraction: the journal is generic because a session the user asks not to
/// keep is [`demido_trace::Memory`] and nothing else changes, and the backend
/// is generic because that is the trait the whole product is written against.
/// It is also the only reason the turn loop is testable, since none of its
/// rules can be observed through a real `llama.cpp` without a card and several
/// gigabytes.
pub struct Chat<B: Backend, J: Journal> {
    id: SessionId,
    /// How to get the log, called at most once and **not** at construction.
    ///
    /// The composition root runs before the window exists, so a root that
    /// opened a session log would make opening a window a thing that can fail
    /// on a full or read-only disk, with nothing on screen to report it on
    /// (`src-tauri/src/wiring.rs`). Deferring it moves that failure to a desk
    /// that is already drawn, where it is a sentence.
    ///
    /// It is the first thing that *needs* the log rather than the first thing
    /// that writes to it, and the difference shows: the desk asks for its
    /// transcript on mount, so a profile that opens the app and says nothing
    /// still gets an empty log file. That is a real session for a real profile
    /// and it is left alone.
    open: Box<dyn Fn() -> demido_trace::Result<J> + Send + Sync>,
    session: Mutex<Option<Session<J>>>,
    /// Shared rather than owned. The rule the supervisor enforces is that one
    /// model is resident on the card, and a chat that made its own would make
    /// that one model *per conversation*: the second chat would load a second
    /// set of weights beside the first, on a card the whole design is sized
    /// against (`docs/rules/done.md`).
    supervisor: Arc<Supervisor<B>>,
    /// What answers, or nothing.
    ///
    /// Behind a lock rather than owned outright, because the set-up wizard is
    /// what settles it and the wizard runs while this chat already exists
    /// ([#48](https://github.com/elpideus/demido-studio/issues/48)). A model
    /// fixed at construction would mean the wizard's last step and the
    /// composer disagreeing about what answers until the app was restarted,
    /// which is the one thing `demido-setup`'s `target` exists to prevent.
    model: Mutex<Option<Model<B>>>,
    /// The ladder, shared with every other conversation and with the settings
    /// page. Held rather than resolved once, because a value changed while the
    /// window is open takes effect on the next turn and not on the next launch.
    settings: Arc<Settings>,
    /// Which scopes this conversation resolves through: global, then this chat.
    ///
    /// Built once from [`Chat::id`], because a chat's own tier is the chat, and
    /// a ladder assembled per call is a ladder that can be assembled wrongly.
    ladder: Ladder,
    presence: Mutex<Presence>,
    /// One turn at a time.
    ///
    /// Not because a backend could not take two, but because [`Chat::stop`]
    /// would then have two generations to mean and could only hold one. A
    /// second `ask` waits for the first rather than being refused: whoever
    /// asked meant to ask, and a conversation is sequential anyway.
    turn: tokio::sync::Mutex<()>,
    /// The generation in flight, so a stop can reach it. `None` between turns.
    running: Mutex<Option<Cancel>>,
}

impl<B: Backend, J: Journal> Chat<B, J> {
    /// A chat over a log that opens when there is something to put in it,
    /// talking to the model given, or to nothing.
    ///
    /// `None` is the ordinary first launch: no set-up has run, so there is
    /// nothing to answer with and the composer says so rather than pretending.
    pub fn new(
        id: impl Into<SessionId>,
        open: impl Fn() -> demido_trace::Result<J> + Send + Sync + 'static,
        supervisor: Arc<Supervisor<B>>,
        model: Option<Model<B>>,
        settings: Arc<Settings>,
    ) -> Self {
        let id = id.into();
        let ladder = Ladder::for_chat(id.to_string());
        Self {
            id,
            open: Box::new(open),
            model: Mutex::new(model),
            session: Mutex::new(None),
            supervisor,
            settings,
            ladder,
            presence: Mutex::new(Presence::Absent),
            turn: tokio::sync::Mutex::new(()),
            running: Mutex::new(None),
        }
    }

    /// What is in force for this conversation, right now.
    ///
    /// Read again per load and per turn rather than kept, because the settings
    /// page and this chat are looking at the same ladder and a copy taken at
    /// construction would be a conversation that ignores what the user just
    /// changed.
    pub fn resolved(&self) -> Resolved {
        self.settings.resolve(&self.ladder)
    }

    /// What the composer should say about the model.
    pub fn presence(&self) -> Presence {
        self.presence
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }

    /// What answers right now.
    ///
    /// Cloned out of the lock rather than borrowed, because loading takes
    /// minutes and a guard held across it would be a settings page that
    /// blocked on a model reading several gigabytes off disk.
    fn model(&self) -> Option<Model<B>> {
        self.model
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }

    /// Point this conversation at a model, or at nothing.
    ///
    /// What the set-up wizard's last step calls. It does not load: loading is
    /// [`Chat::load`], which the caller runs next and the composer already
    /// draws every state of. Separating them is what lets a model be settled
    /// while the previous one is still resident, with the supervisor deciding
    /// what that means for the card.
    pub fn point_at(&self, model: Option<Model<B>>) {
        *self.model.lock().unwrap_or_else(|held| held.into_inner()) = model;
    }

    /// Start the model, reporting every state it passes through.
    ///
    /// `report` is called on each transition rather than only at the end,
    /// because the whole point of the loading state is that it is visible while
    /// it lasts: several gigabytes off a cold disk is minutes, and a window
    /// told only the outcome shows an idle composer for all of them.
    ///
    /// Never fails. A backend that will not start is a [`Presence::Failed`] the
    /// desk carries on around, per `AGENTS.md`: startup never blocks, and a
    /// subsystem that fails is reported and skipped.
    pub async fn load(&self, mut report: impl FnMut(&Presence)) -> Presence {
        let Some(model) = self.model() else {
            return self.report(Presence::Absent, &mut report);
        };

        self.report(
            Presence::Loading {
                model: model.id.clone(),
            },
            &mut report,
        );

        // The context length the ladder resolved, applied to the configuration
        // the supervisor compares. So a chat that asked for a bigger window
        // gets a server started for that window, and changing the number and
        // loading again really is a restart rather than a no-op:
        // `Supervisor::ensure` sees a different configuration.
        //
        // What the user set is what is reserved, not that number divided by the
        // slot count (`demido_inference::llamacpp::arguments`), which is the
        // defect `docs/rules/done.md` records and the contract suite measures.
        let config = B::with_context_length(model.config.clone(), self.resolved().context_length());

        match self.supervisor.ensure(config).await {
            Ok(_) => self.report(
                Presence::Ready {
                    model: model.id.clone(),
                },
                &mut report,
            ),
            Err(error) => {
                tracing::warn!(%error, "the model did not start");
                self.report(
                    Presence::Failed {
                        detail: error.to_string(),
                    },
                    &mut report,
                )
            }
        }
    }

    /// The log, read.
    ///
    /// Every projection anything draws comes from here: the transcript below,
    /// an export, and the Session Monitor when it exists. Public because those
    /// are three readers of one log rather than three copies of a session, and
    /// a reader that had to be given its own accessor would eventually be given
    /// its own store.
    pub fn replay(&self) -> Result<Replay> {
        self.with_session(|session| Ok(Replay::of(session.journal())?))
    }

    /// The transcript, out of the log and nowhere else.
    ///
    /// This is what a restart draws, and it is the same read as the first
    /// launch: a chat survives closing the app because it was never anywhere
    /// but the log.
    pub fn history(&self) -> Result<Vec<Said>> {
        self.with_session(|session| {
            let replay = Replay::of(session.journal())?;
            Ok(replay
                .history()
                .into_iter()
                .map(|exchange| Said {
                    seq: exchange.seq,
                    turn: exchange.turn,
                    role: exchange.role,
                    text: exchange.text,
                })
                .collect())
        })
    }

    /// Compose a turn, send it, and record what came back.
    ///
    /// `sink` is handed every token as it arrives. A callback rather than a
    /// stream because the caller is a Tauri command emitting an event, and
    /// handing it a stream would mean it had to drive one.
    ///
    /// The order is fixed and it is the reason this function exists: the
    /// assembly is recorded, then sent, then the answer is recorded against it.
    /// A caller cannot send an assembly it did not record, because the assembly
    /// is what recording produced.
    pub async fn ask(&self, said: &str, mut sink: impl FnMut(Update) + Send) -> Result<Answer> {
        let _turn = self.turn.lock().await;
        let model = self.answering()?;
        let backend = match self.supervisor.current().await {
            Some(backend) => backend,
            // Ready a moment ago and gone now, which is a process that exited
            // without telling anybody. Reported, and the desk stays usable.
            None => {
                self.failed(Error::Gone.to_string());
                return Err(Error::Gone);
            }
        };

        // Resolved once, before the assembly, so the system prompt in the log
        // and the temperature in the request are the same reading of the
        // ladder. Two reads could straddle a change made from the settings page
        // mid turn, and the log would then describe a turn nobody sent.
        let resolved = self.resolved();

        // Everything up to the send is recording, and it happens under the
        // session lock. Nothing is awaited while it is held.
        let sent = self.with_session(|session| {
            let earlier = Replay::of(session.journal())?;
            let mut turn = session.begin();
            // Who the model is being goes first, before anything anybody said,
            // which is the only position a system message has.
            //
            // `Source::Inject` rather than `Source::System`: the taxonomy is
            // about who put the text in the window, and Demido put it there
            // without being asked this turn. `System` is text Demido *wrote*,
            // and this is the user's own, resolved off the ladder. An empty one
            // is left out entirely rather than sent as a blank message.
            if !resolved.system_prompt().is_empty() {
                turn.message(Source::Inject, Role::System, resolved.system_prompt())?;
            }
            // History reaches the model as positions on the log rather than as
            // copies, so a long conversation does not grow the log as the
            // square of itself. This is the line that makes a second message a
            // conversation rather than a second first question.
            for exchange in earlier.history() {
                turn.carry(exchange.seq);
            }
            turn.user(said)?;
            turn.parameters(&model, options(&resolved))?;
            Ok(turn.send()?)
        })?;

        let cancel = Cancel::new();
        self.arm(Some(cancel.clone()));
        let outcome = self.stream(&backend, &sent, cancel, &mut sink).await;
        self.arm(None);

        match outcome {
            Ok(answer) => {
                sink(Update::Done {
                    turn: answer.turn,
                    seq: answer.seq,
                    text: answer.text.clone(),
                    thinking: answer.thinking.clone(),
                    reason: answer.reason,
                });
                Ok(answer)
            }
            Err(error) => {
                let detail = error.to_string();
                // The failure goes on the same log as everything else. A
                // session whose errors are only in a log file somewhere cannot
                // explain itself.
                //
                // Best effort, and deliberately not `?`: when the log is what
                // failed, recording that it failed fails too, and returning the
                // second error would replace the one that says what happened.
                if let Err(unrecorded) = self.with_session(|session| {
                    session.failed(sent.turn, error.kind(), &detail)?;
                    Ok(())
                }) {
                    tracing::warn!(%unrecorded, "the failure was not recorded");
                }
                // A turn can fail because the request was wrong or because the
                // process died. Only the second one changes what the composer
                // may offer next, and the backend is the one that knows which
                // it was.
                if !backend.ready().await {
                    self.failed(detail.clone());
                }
                sink(Update::Failed {
                    turn: sent.turn,
                    detail,
                });
                Err(error)
            }
        }
    }

    /// Call off whatever is generating. `false` when nothing is.
    ///
    /// It waits for nothing and records nothing. The stream ends with a `Done`
    /// carrying [`FinishReason::Cancelled`], and [`Chat::ask`] records that
    /// like any other ending, so the partial answer and the stop are written by
    /// the same line that writes a completed answer. A second path here is a
    /// second path to forget.
    pub fn stop(&self) -> bool {
        let running = self.running.lock().unwrap_or_else(|held| held.into_inner());
        match running.as_ref() {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Give the card back. What closing the window calls.
    ///
    /// It stops the shared backend, so a build with more than one chat calls
    /// this once, on the way out, rather than per conversation.
    pub async fn shutdown(&self) {
        self.stop();
        self.supervisor.shutdown().await;
        self.report(Presence::Absent, &mut |_: &Presence| {});
    }

    /// Read the stream, telling the window as it goes, and record the answer.
    async fn stream(
        &self,
        backend: &B,
        sent: &demido_trace::Sent,
        cancel: Cancel,
        sink: &mut impl FnMut(Update),
    ) -> Result<Answer> {
        let mut stream = backend.generate(sent.request.clone(), cancel).await?;

        let mut text = String::new();
        let mut thinking = String::new();
        let mut finished: Option<(FinishReason, Usage)> = None;

        while let Some(chunk) = stream.next().await {
            match chunk? {
                Chunk::Text { text: said } => {
                    text.push_str(&said);
                    sink(Update::Text { text: said });
                }
                Chunk::Thinking { text: thought } => {
                    thinking.push_str(&thought);
                    sink(Update::Thinking { text: thought });
                }
                Chunk::Done { reason, usage } => finished = Some((reason, usage)),
            }
        }

        let Some((reason, usage)) = finished else {
            return Err(Error::Unfinished);
        };

        let seq = self.with_session(|session| {
            Ok(session.completed(sent, &text, &thinking, reason, usage)?)
        })?;

        Ok(Answer {
            turn: sent.turn,
            seq,
            text,
            thinking,
            reason,
            usage,
        })
    }

    /// The model to send a turn as, or a refusal naming what is loaded instead.
    fn answering(&self) -> Result<String> {
        let presence = self.presence();
        match presence.model() {
            Some(model) if presence.is_ready() => Ok(model.to_owned()),
            _ => Err(Error::NotReady(presence)),
        }
    }

    /// Record a presence and tell whoever asked to be told.
    fn report(&self, presence: Presence, report: &mut impl FnMut(&Presence)) -> Presence {
        {
            let mut held = self
                .presence
                .lock()
                .unwrap_or_else(|held| held.into_inner());
            held.clone_from(&presence);
        }
        report(&presence);
        presence
    }

    /// The model is gone, and nobody asked to be told.
    fn failed(&self, detail: String) {
        self.report(Presence::Failed { detail }, &mut |_: &Presence| {});
    }

    fn arm(&self, cancel: Option<Cancel>) {
        *self.running.lock().unwrap_or_else(|held| held.into_inner()) = cancel;
    }

    /// Do something with the session, opening the log if this is the first
    /// thing that needed it.
    ///
    /// The session is taken out of the lock for the duration and put back
    /// after, so there is no second branch where the log is open and unusable
    /// and nothing to reason about if `act` unwinds.
    fn with_session<T>(&self, act: impl FnOnce(&Session<J>) -> Result<T>) -> Result<T> {
        let mut held = self.session.lock().unwrap_or_else(|held| held.into_inner());

        let session = match held.take() {
            Some(session) => session,
            None => {
                let session = Session::new(self.id.clone(), (self.open)()?);
                // A log that already has turns in it numbers the next one after
                // them, derived from the events rather than remembered: what is
                // remembered elsewhere can disagree with the log.
                session.resume()?;
                session
            }
        };

        let outcome = act(&session);
        *held = Some(session);
        outcome
    }
}
