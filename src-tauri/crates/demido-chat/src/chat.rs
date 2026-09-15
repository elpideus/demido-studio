//! The session, the assembly it composes, and the loop that runs a turn.
//!
//! One type, [`Chat`], holding three things: a journal, a supervisor, and the
//! cancellation token of whatever is generating right now. It holds no
//! messages, and that absence is the design (see the crate docs).

use std::future::Future;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;

use demido_inference::{
    Backend, Cancel, Chunk, FinishReason, Options, Role, Supervisor, ToolCall, Usage,
};
use demido_permission::{Mode, Verdict};
use demido_prompts::{catalog, id};
use demido_settings::{Ladder, Origin, Resolved, Settings, Tier};
use demido_tools::Registry;
use demido_trace::{Called, Decision, Journal, Layer, Replay, Sent, Session, SessionId, Source};

use crate::monitor::Assembly;
use crate::presence::Presence;
use crate::toolbox::{Asking, Offering, Toolbox};
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

    /// The model asked for one more round of calls than the turn allows. Those
    /// calls are answered as not run, so the next message can carry them, and
    /// the turn ends here: a loop that has not answered in this many rounds is
    /// the runaway the limit exists to end.
    #[error("the turn used all {steps} of its tool steps without answering")]
    StepLimit { steps: u32 },

    /// A paragraph the loop tells the model something with is not in the
    /// register. Only a build that dropped one can reach it.
    #[error("the prompt register has no {0}")]
    Unregistered(&'static str),

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
            Error::StepLimit { .. } => "step-limit",
            Error::Unregistered(_) => "unregistered",
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

/// One moment in the transcript: something said, or a call and what came back.
///
/// The window draws a bubble for the first and a **tool call row**
/// (`design/system.md`) for the second, at the point in the turn where each
/// happened. Tagged rather than two lists, because the order is the thing being
/// drawn: a call that arrived between two sentences belongs between them.
///
/// [`Said`] is this crate's own because it is [`demido_trace::Exchange`] with
/// the monitor's two axes taken off it. [`Called`] is the log's own type
/// unchanged, because there is nothing on it to take off: a copy here would be
/// a rename and two `From` impls, which is a second declaration that can drift
/// rather than a narrowing that means something.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "moment", rename_all = "camelCase")]
pub enum Moment {
    Said(Said),
    Called(Called),
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
    ///
    /// One token for the whole turn rather than one per generation, so a stop
    /// reaches whatever the turn is doing: generating, waiting on a person, or
    /// running a command.
    running: Mutex<Option<Cancel>>,
    /// What this conversation offers, and the mode its calls are ruled under.
    tools: Toolbox,
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
        tools: Toolbox,
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
            tools,
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

    /// Every group this conversation could offer, with its tools: what the
    /// picker draws. Which of them are on is the ladder's, in [`Chat::resolved`].
    pub fn groups(&self) -> Vec<Offering> {
        self.tools.groups()
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

    /// The log, whole, as the session monitor reads it.
    ///
    /// Every event, in order, with nothing folded and nothing left out: the
    /// monitor's stream is the log rather than a summary of it, and its last
    /// tab is the raw JSON of the selected line because the record is the
    /// record (`design/windows.md`). A projection narrower than this would be a
    /// second answer to "what happened", which is the thing this crate does not
    /// have.
    pub fn log(&self) -> Result<Vec<demido_trace::Event>> {
        self.with_session(|session| Ok(session.journal().events()?))
    }

    /// The assembly as it stood at one event, with what became of each tool
    /// group in it.
    ///
    /// Two halves from two places, and they belong apart. The rebuild is the
    /// log's ([`Replay::rebuild`]), because it is a projection of events. The
    /// groups are the registry's, because a log records tool **names** and only
    /// a registry knows which group a name is in.
    pub fn assembly(&self, at: u64) -> Result<Option<Assembly>> {
        self.with_session(|session| {
            let Some(rebuild) = Replay::of(session.journal())?.rebuild(at)? else {
                return Ok(None);
            };
            let groups =
                crate::monitor::grouped(self.tools.registry().groups(), rebuild.tools.as_ref());
            Ok(Some(Assembly { rebuild, groups }))
        })
    }

    /// The transcript, out of the log and nowhere else.
    ///
    /// This is what a restart draws, and it is the same read as the first
    /// launch: a chat survives closing the app because it was never anywhere
    /// but the log.
    ///
    /// It carries the calls as well as the messages, because
    /// [#55](https://github.com/elpideus/demido-studio/issues/55) draws a call
    /// and its result **in the transcript at the point in the turn where they
    /// happened**, rather than in a window somebody has to know to open. The
    /// pairing of a call with what came back is
    /// [`demido_trace::Replay::transcript`]'s, over the same events the monitor
    /// reads as two rows.
    pub fn transcript(&self) -> Result<Vec<Moment>> {
        self.with_session(|session| {
            let replay = Replay::of(session.journal())?;
            Ok(replay
                .transcript()?
                .into_iter()
                .map(|moment| match moment {
                    demido_trace::Moment::Said(exchange) => Moment::Said(Said {
                        seq: exchange.seq,
                        turn: exchange.turn,
                        role: exchange.role,
                        text: exchange.text,
                    }),
                    demido_trace::Moment::Called(called) => Moment::Called(called),
                })
                .collect())
        })
    }

    /// Compose a turn, send it, and record what came back, stepping through
    /// every call the model asks for on the way.
    ///
    /// `sink` is handed every token as it arrives. A callback rather than a
    /// stream because the caller is a Tauri command emitting an event, and
    /// handing it a stream would mean it had to drive one.
    ///
    /// `approve` is asked about a call the matrix will not run on its own, and
    /// answers allow, deny or always. A callback rather than a trait: the
    /// window is the one real implementation, and [`Asking`] is the interface a
    /// trait would have if a second genuine one ever appears. A stop does not
    /// wait for it.
    ///
    /// The order is fixed and it is the reason this function exists: the
    /// assembly is recorded, then sent, then the answer is recorded against it.
    /// A caller cannot send an assembly it did not record, because the assembly
    /// is what recording produced. A step is the same: what came back from the
    /// calls is recorded, and the next request is what recording it produced.
    pub async fn ask<F>(
        &self,
        said: &str,
        mut sink: impl FnMut(Update) + Send,
        mut approve: impl FnMut(Asking) -> F + Send,
    ) -> Result<Answer>
    where
        F: Future<Output = Decision> + Send,
    {
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
        // The same for the register: one reading of what is on offer, so the
        // tools the log names, the tools the request carries and the tools a
        // call is planned against are one list. The set is the ladder's, so a
        // tool switched off is not in any of the three (`docs/rules/tools.md`:
        // disabled means absent).
        let rules = Rules {
            registry: self.tools.narrowed(resolved.offered().as_deref()),
            mode: Mode::named(resolved.mode()),
            limit: resolved.step_limit(),
            // Off the ladder's chat tier rather than off the log (#55). The log
            // still says which of the three the person answered, because that
            // is what happened; what is in force next turn is a setting, so it
            // is where every other value in force is, and a person who wants it
            // back has a row rather than an un-appendable log to edit.
            always: resolved.always(),
        };
        let layer = layer(resolved.origin(demido_settings::id::TOOLS_OFFERED));
        let offered: Vec<(demido_prompts::Document, serde_json::Value)> = self
            .tools
            .offered(&rules.registry)
            .into_iter()
            .map(|spec| (spec.document, spec.shape))
            .collect();

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
            // square of itself. It carries the calls and what came back from
            // them too, so a model is not made to call again for what it
            // already has.
            for seq in earlier.conversation() {
                turn.carry(seq);
            }
            turn.user(said)?;
            turn.offer(layer, &offered)?;
            turn.parameters(&model, options(&resolved))?;
            Ok(turn.send()?)
        })?;
        let number = sent.turn;

        let cancel = Cancel::new();
        self.arm(Some(cancel.clone()));
        let outcome = self
            .steps(&backend, sent, &cancel, &mut sink, &mut approve, &rules)
            .await;
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
                    session.failed(number, error.kind(), &detail)?;
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
                    turn: number,
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

    /// Run a turn to its end.
    ///
    /// Generate; if the model asked for calls, answer every one of them and
    /// send the turn again carrying the answers; stop when it answers without
    /// calling, when a stop lands, or when the step limit is reached.
    ///
    /// **The limit is the ladder's and never the mode's.** Both are resolved
    /// off the ladder once per message, and the mode is handed to the matrix per
    /// call and read by nothing else here (`docs/rules/tools.md`: the mode gates
    /// permissions and nothing else).
    async fn steps<F>(
        &self,
        backend: &B,
        mut sent: Sent,
        cancel: &Cancel,
        sink: &mut (impl FnMut(Update) + Send),
        approve: &mut (impl FnMut(Asking) -> F + Send),
        rules: &Rules,
    ) -> Result<Answer>
    where
        F: Future<Output = Decision> + Send,
    {
        let limit = rules.limit;
        // Standing answers come off the ladder, resolved once with everything
        // else this message is ruled by, so an *always* given on an earlier
        // message still holds on this one.
        let mut always = rules.always.clone();
        let mut declined: Vec<(String, serde_json::Value)> = Vec::new();
        let mut taken = 0u32;
        let mut withheld = false;

        loop {
            let Generation { answer, calls } =
                self.stream(backend, &sent, cancel.clone(), sink).await?;

            // A stop while the model was still producing: what it said is kept,
            // and a call that had already arrived is answered as stopped
            // rather than run, so the next message can carry it.
            if answer.reason == FinishReason::Cancelled {
                self.refuse_all(answer.turn, &calls, id::TOOLS_STOPPED, &[])?;
                return Ok(answer);
            }
            if calls.is_empty() {
                return Ok(answer);
            }
            if taken == limit {
                let steps = limit.to_string();
                self.refuse_all(
                    answer.turn,
                    &calls,
                    id::TOOLS_LIMIT,
                    &[(catalog::STEPS, &steps)],
                )?;
                return Err(Error::StepLimit { steps: limit });
            }

            let mut blocks = vec![answer.seq];
            let mut attempts: Vec<Attempt> = Vec::new();
            for (at, (seq, call)) in calls.iter().enumerate() {
                let ruling = Ruling {
                    registry: &rules.registry,
                    mode: &rules.mode,
                    always: &mut always,
                    declined: &mut declined,
                };
                match self
                    .dispatch(answer.turn, *seq, call, ruling, cancel, approve)
                    .await?
                {
                    Some(answered) => {
                        blocks.push(answered.block);
                        attempts.push(answered.attempt);
                        // The call has an answer now, and the transcript draws
                        // one row for the pair. The window is told there is
                        // something to read, and reads the log for what.
                        sink(Update::Recorded);
                    }
                    // Stopped while this call waited or ran. It and every call
                    // after it are answered as stopped, and nothing else runs.
                    None => {
                        self.refuse_all(answer.turn, &calls[at..], id::TOOLS_STOPPED, &[])?;
                        return Ok(Answer {
                            reason: FinishReason::Cancelled,
                            ..answer
                        });
                    }
                }
            }

            taken += 1;
            // **A step whose every call was a declined call it had already made
            // takes the tools away for the rest of the turn.** It ran nothing,
            // and what it ran nothing on was a question the person has now
            // answered twice. Another step with the same tools in front of it
            // has the same call to make a third time, and what the refusal
            // asked for was an answer.
            //
            // It is guidance rather than a limit, and it is the first thing in
            // this repo put there by a measurement instead of by taste (#59).
            // The step limit still ends a runaway; this is what keeps an
            // ordinary refusal from becoming one.
            //
            // **The first denial is deliberately left alone.** A model handed a
            // refusal with its tools still in front of it is the whole of what
            // `Bar: chose` claims, and a turn that stripped them at the first
            // no would be measuring this rule rather than the model. It also
            // has somewhere useful to go: a denied write is often followed by a
            // read that answers the question anyway.
            //
            // **A tool the picker switched off is not a reason either.** That
            // call was never among the options, so the options are not what
            // went wrong, and taking away the groups a person left on because
            // of a group they turned off is the opposite of what the picker is
            // for (`docs/rules/tools.md`).
            let withhold = !attempts.is_empty()
                && attempts.iter().all(|attempt| *attempt != Attempt::Ran)
                && attempts.contains(&Attempt::Repeated);
            // Once withheld, withheld: the empty set is carried forward on
            // `Sent::offered`, so a later step neither puts the tools back nor
            // writes a second set saying the same thing.
            let offering = match withhold && !withheld {
                true => {
                    withheld = true;
                    demido_trace::NextStep::Withholding
                }
                false => demido_trace::NextStep::Offering,
            };
            sent = self.with_session(|session| Ok(session.step(&sent, &blocks, offering)?))?;
        }
    }

    /// Answer one call: run it, or record why not. `None` when a stop landed
    /// before it finished.
    ///
    /// The order is the matrix's to set out: a call to a tool the person
    /// switched off is refused as that; a call that cannot be understood is
    /// answered with why; one identical to a call the person just declined is
    /// refused without asking again, and told so in its own words rather than
    /// in the ones it has already ignored once; the matrix rules on the rest,
    /// and the person is asked only when it says to ask.
    async fn dispatch<F>(
        &self,
        turn: u32,
        seq: u64,
        call: &ToolCall,
        ruling: Ruling<'_>,
        cancel: &Cancel,
        approve: &mut (impl FnMut(Asking) -> F + Send),
    ) -> Result<Option<Answered>>
    where
        F: Future<Output = Decision> + Send,
    {
        // Registered, and not in the set: somebody closed it. Told as that
        // rather than as a name that is not a tool, which would send the model
        // looking for another way to do what a person deliberately turned off
        // (`docs/rules/tools.md`).
        if !ruling.registry.offers(&call.name) && self.tools.registry().offers(&call.name) {
            return self
                .refuse(
                    turn,
                    seq,
                    id::TOOLS_OFF,
                    &[(catalog::TOOL, call.name.as_str())],
                )
                .map(Answered::declined);
        }

        let planned = match ruling.registry.plan(&demido_tools::Call {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        }) {
            Ok(planned) => planned,
            // Attempted and failed: a name that is not a tool, arguments that
            // do not fit. The model has something to fix, and it is told what.
            Err(failure) => {
                return self
                    .returned(turn, seq, &failure.message, true)
                    .map(Answered::ran)
            }
        };

        // A planned call's arguments parsed, so this is never `Null` in
        // practice. Compared as values, so the same call with its keys in a
        // different order is the same call.
        let arguments: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or_default();
        let this = (call.name.clone(), arguments);
        let declined = [(catalog::TOOL, call.name.as_str())];

        // The same call a second time, which is a different situation from the
        // first and is told as one. Repeating the declined paragraph verbatim
        // is what a model already ignoring it reads again, and #59 saw that:
        // the development model made the identical write three times in one
        // turn. This wording says the call has already been refused and names
        // writing the reply as the next thing to do.
        //
        // It is also the signal the step loop reads to decide that this turn
        // has stopped getting anywhere. No measurement was taken against these
        // words on their own, so the entry declares no dependants: what was
        // measured is the withholding above it.
        if ruling.declined.contains(&this) {
            return self
                .refuse(turn, seq, id::TOOLS_REPEATED, &declined)
                .map(Answered::repeated);
        }

        if demido_permission::verdict(ruling.mode, planned.tool(), &planned.intent, ruling.always)
            == Verdict::Ask
        {
            let asking = Asking {
                turn,
                call: seq,
                id: call.id.clone(),
                tool: call.name.clone(),
                ability: planned.intent.ability,
                summary: planned.intent.summary.clone(),
                destructive: planned.intent.destructive,
                arguments: this.1.clone(),
            };
            let decision = tokio::select! {
                biased;
                () = cancel.cancelled() => return Ok(None),
                decision = approve(asking) => decision,
            };
            self.with_session(|session| Ok(session.decided(turn, seq, decision)?))?;

            match decision {
                Decision::Allow => {}
                // Never on a destructive call, whatever the window sent. The
                // matrix's floor is that such a call asks every time and that
                // *always* cannot waive it (`docs/rules/tools.md`), and a floor
                // that only held while the frontend agreed with it would be a
                // floor a second frontend could step through. The decision is
                // still recorded as what the person answered, because it is.
                Decision::Always if planned.intent.destructive => {}
                Decision::Always => {
                    if !ruling.always.contains(&call.name) {
                        ruling.always.push(call.name.clone());
                    }
                    self.remember_always(ruling.always);
                }
                Decision::Deny => {
                    ruling.declined.push(this);
                    return self
                        .refuse(turn, seq, id::TOOLS_DENIED, &declined)
                        .map(Answered::declined);
                }
            }
        }

        // Dropping the call's future is what ends it, and for `run_command`
        // that kills the whole process tree (`demido-tools`' `tree`).
        let outcome = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(None),
            outcome = planned.run() => outcome,
        };
        match outcome {
            Ok(text) => self.returned(turn, seq, &text, false),
            Err(failure) => self.returned(turn, seq, &failure.message, true),
        }
        .map(Answered::ran)
    }

    /// Keep *always for this tool* where every other value in force is kept:
    /// the ladder's **chat** tier, and never the global one.
    ///
    /// #55's own line, and the reason is the size of the promise. The person
    /// answered about a call in this conversation; writing that globally would
    /// turn one answer into consent for every conversation they ever open,
    /// which is not what was said and is not something a row in a transcript
    /// should be able to do. `Scope::chat` is the only scope this writes, and
    /// the tier is named here rather than passed in so there is nowhere to pass
    /// a different one from.
    ///
    /// Best effort, and deliberately not `?`: a ladder that would not take the
    /// write means the next turn asks again, which is the safe direction, and
    /// failing the turn over it would throw away a call the person just allowed.
    fn remember_always(&self, names: &[String]) {
        let scope = demido_settings::Scope::chat(self.id.to_string());
        if let Err(error) =
            self.settings
                .set(&scope, demido_settings::id::TOOLS_ALWAYS, &json!(names))
        {
            tracing::warn!(%error, "always for this tool was not saved; it holds for this turn only");
        }
    }

    fn returned(&self, turn: u32, call: u64, text: &str, failed: bool) -> Result<u64> {
        self.with_session(|session| Ok(session.returned(turn, call, text, failed)?))
    }

    /// Tell the model, in a paragraph's wording, why the call at `call` did not
    /// run.
    fn refuse(
        &self,
        turn: u32,
        call: u64,
        id: &'static str,
        values: &[(&str, &str)],
    ) -> Result<u64> {
        let prompt = self.tools.paragraph(id).ok_or(Error::Unregistered(id))?;
        self.with_session(|session| Ok(session.refused(turn, call, &prompt, values)?))
    }

    fn refuse_all(
        &self,
        turn: u32,
        calls: &[(u64, ToolCall)],
        id: &'static str,
        values: &[(&str, &str)],
    ) -> Result<()> {
        for (seq, _) in calls {
            self.refuse(turn, *seq, id, values)?;
        }
        Ok(())
    }

    /// Read one generation, telling the window as it goes, and record the
    /// answer and then each call it asked for.
    async fn stream(
        &self,
        backend: &B,
        sent: &Sent,
        cancel: Cancel,
        sink: &mut impl FnMut(Update),
    ) -> Result<Generation> {
        let mut stream = backend.generate(sent.request.clone(), cancel).await?;

        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<ToolCall> = Vec::new();
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
                Chunk::Call { call } => calls.push(call),
                Chunk::Done { reason, usage } => finished = Some((reason, usage)),
            }
        }

        let Some((reason, usage)) = finished else {
            return Err(Error::Unfinished);
        };

        let (seq, calls) = self.with_session(|session| {
            let seq = session.completed(sent, &text, &thinking, reason, usage)?;
            let calls = calls
                .into_iter()
                .map(|call| Ok((session.called(sent.turn, seq, &call)?, call)))
                .collect::<Result<Vec<_>>>()?;
            Ok((seq, calls))
        })?;

        // Told after the recording and not before it, so a window that reads
        // the log on being told finds what it was told about. What it was
        // streaming is now on the record, which is what lets it stop drawing a
        // draft and draw the log instead.
        if !calls.is_empty() {
            sink(Update::Recorded);
        }

        Ok(Generation {
            answer: Answer {
                turn: sent.turn,
                seq,
                text,
                thinking,
                reason,
                usage,
            },
            calls,
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

/// One generation, recorded: the answer, and each call it asked for with the
/// call's position on the log.
struct Generation {
    answer: Answer,
    calls: Vec<(u64, ToolCall)>,
}

/// What one message is ruled by, resolved off the ladder once: the tools on
/// offer, the mode, how many steps it may take, and what has already been
/// answered *always for this tool*.
struct Rules {
    registry: Registry,
    mode: Mode,
    limit: u32,
    always: Vec<String>,
}

/// Which layer decided the offered set: a tier of the ladder, or nobody, which
/// is everything the registry has.
fn layer(origin: Origin) -> Layer {
    match origin {
        Origin::Default => Layer::Registry,
        Origin::Tier(Tier::Global) => Layer::Global,
        Origin::Tier(Tier::Model) => Layer::Model,
        Origin::Tier(Tier::Character) => Layer::Character,
        Origin::Tier(Tier::Chat) => Layer::Chat,
    }
}

/// What became of one call: the block that answers it, and what kind of thing
/// happened to it.
#[derive(Debug, Clone, Copy)]
struct Answered {
    /// Where the model's answer to this call is on the log.
    block: u64,
    attempt: Attempt,
}

/// The three things that can become of a call, as the turn loop needs to tell
/// them apart.
///
/// They are three rather than two because the next step's tools depend on which
/// one it was, and the three are not interchangeable: only [`Attempt::Repeated`]
/// says the model is going round in a circle the tools are feeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Attempt {
    /// Something was tried. The tool ran, or it failed, or the arguments did
    /// not fit its schema: either way there is a result, and if it is a bad one
    /// there is something to fix by calling again.
    Ran,
    /// The person said no to this call, or the picker had the tool switched
    /// off. Nothing ran, and the model has been told why, once.
    Declined,
    /// The same call the person declined earlier in this turn, made again.
    Repeated,
}

impl Answered {
    fn ran(block: u64) -> Option<Self> {
        Some(Self {
            block,
            attempt: Attempt::Ran,
        })
    }

    fn declined(block: u64) -> Option<Self> {
        Some(Self {
            block,
            attempt: Attempt::Declined,
        })
    }

    fn repeated(block: u64) -> Option<Self> {
        Some(Self {
            block,
            attempt: Attempt::Repeated,
        })
    }
}

/// What one call is ruled on with: the tools on offer, the mode, and what the
/// person has already said this turn and before it.
struct Ruling<'a> {
    registry: &'a Registry,
    mode: &'a Mode,
    always: &'a mut Vec<String>,
    /// Calls the person declined this turn, by name and arguments.
    declined: &'a mut Vec<(String, serde_json::Value)>,
}
