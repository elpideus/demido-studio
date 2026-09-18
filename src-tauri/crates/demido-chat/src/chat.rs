//! The session, the assembly it composes, and the loop that runs a turn.
//!
//! One type, [`Chat`], holding three things: a journal, a supervisor, and the
//! cancellation token of whatever is generating right now. It holds no
//! messages, and that absence is the design (see the crate docs).

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;

use demido_inference::{
    Backend, Cancel, Chunk, FinishReason, Options, Role, Supervisor, ToolCall, Usage,
};
use demido_permission::{inherit, Mode, Request, Resolution, Verdict};
use demido_prompts::{catalog, id};
use demido_settings::{Ladder, Origin, Resolved, Settings, Tier};
use demido_tools::{Failure, Outcome, Registry};
use demido_trace::{
    Called, Decision, Delegation, Journal, Layer, Replay, Sent, Session, SessionId, Source,
};

use crate::delegation::Delegations;
use crate::flight::{Flight, Harvest, Slots};
use crate::monitor::Assembly;
use crate::pool::Pool;
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

/// One moment in the transcript: something said, a call and what came back, or
/// a delegation and what the sub-agent answered.
///
/// The window draws a bubble for the first and a **tool call row**
/// (`design/system.md`) for the second, at the point in the turn where each
/// happened. Tagged rather than two lists, because the order is the thing being
/// drawn: a call that arrived between two sentences belongs between them.
///
/// The third is [#67](https://github.com/elpideus/demido-studio/issues/67): a
/// delegation is **one exchange**, the task out and the answer back, rather
/// than a call row whose result is prose nothing says the author of. A clean
/// context should also be a clean transcript.
///
/// [`Said`] is this crate's own because it is [`demido_trace::Exchange`] with
/// the monitor's two axes taken off it. [`Called`] and [`Delegation`] are the
/// log's own types unchanged, because there is nothing on either to take off: a
/// copy here would be a rename and two `From` impls, which is a second
/// declaration that can drift rather than a narrowing that means something.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "moment", rename_all = "camelCase")]
pub enum Moment {
    Said(Said),
    Called(Called),
    Delegated(Delegation),
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
    /// The conversation's own recorder, over a **shared** handle on the log.
    ///
    /// `Arc<J>` rather than `J` because a sub-agent records into its parent's
    /// journal rather than one of its own
    /// (`docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md`), and the
    /// sequence number lives on the handle: two handles over one file each
    /// number from where they opened it, and the second line claiming position
    /// nine is a log that cannot be replayed at all. `demido_trace` makes
    /// `Arc<J>` a `Journal` for exactly this, so sharing is the type rather
    /// than a rule somebody keeps.
    session: Mutex<Option<Session<Arc<J>>>>,
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
    /// What the card can hold, asked at every load.
    ///
    /// A sub-agent is a second slot on the conversation's own weights, so the
    /// parallelism the ladder asks for is a VRAM budget rather than a
    /// preference and this is what turns one into the other
    /// ([#65](https://github.com/elpideus/demido-studio/issues/65)). Held
    /// rather than passed in per load, because the card it asks is the machine
    /// and does not change; what changes is the answer, which is why it is
    /// asked again every time.
    pool: Pool,
    /// The turn loop's end of the delegation pair, the other end of which is
    /// the `delegate_task` in this conversation's registry.
    ///
    /// Shared rather than locked here, because the queue has a lock of its own
    /// and a background sub-agent's run is a future the turn holds beside its
    /// own work rather than a borrow it can hand out. The turn lock is still
    /// what makes this conversation's rendezvous single-reader: one turn runs
    /// at a time, and a sub-agent has a rendezvous of its own
    /// ([#66](https://github.com/elpideus/demido-studio/issues/66)).
    delegations: Arc<Delegations>,
}

impl<B: Backend, J: Journal> Chat<B, J> {
    /// A chat over a log that opens when there is something to put in it,
    /// talking to the model given, or to nothing.
    ///
    /// `None` is the ordinary first launch: no set-up has run, so there is
    /// nothing to answer with and the composer says so rather than pretending.
    ///
    /// `delegations` is the turn loop's half of [`crate::delegations`], whose
    /// other half belongs in `tools`' registry as the Delegation group. They
    /// are made together and split here because a `delegate_task` wired to one
    /// conversation's loop and registered on another's is a delegation that
    /// answers in the wrong session. A conversation that offers no Delegation
    /// group still takes one: nothing ever asks on it, and a parameter that is
    /// sometimes absent is a second shape of conversation.
    pub fn new(
        id: impl Into<SessionId>,
        open: impl Fn() -> demido_trace::Result<J> + Send + Sync + 'static,
        supervisor: Arc<Supervisor<B>>,
        model: Option<Model<B>>,
        settings: Arc<Settings>,
        tools: Toolbox,
        delegations: Delegations,
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
            pool: Pool::on_the_card(),
            delegations: Arc::new(delegations),
        }
    }

    /// The same conversation, deciding its slots against a different card.
    ///
    /// A builder rather than an eighth parameter, because there is one pool on
    /// a machine and every caller but a test wants it: `Pool::on_the_card` is
    /// what [`Chat::new`] takes, and this is how a test that has no card, or a
    /// card it wants to choose the numbers of, gets a deterministic answer out
    /// of the same code the window runs.
    #[must_use]
    pub fn against(mut self, pool: Pool) -> Self {
        self.pool = pool;
        self
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

    /// Every tool this conversation could offer, by name, with its schema's
    /// shape and no prose on it. What the tool register's editor draws as the
    /// half of a tool that is a contract with the parser.
    pub fn shapes(&self) -> Vec<(String, serde_json::Value)> {
        self.tools.shapes()
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
        let resolved = self.resolved();
        let config = B::with_context_length(model.config.clone(), resolved.context_length());

        // And the slots, which is the same shape of decision one layer along:
        // the ladder asks for a parallelism and `crate::pool` decides what the
        // card can hold. Its module doc is where that reasoning lives; the one
        // thing that has to be read here is the 1, which is the conversation's
        // own slot going in as already open. It is the model being loaded
        // rather than a sub-agent, and a budget that could refuse it would be a
        // card with no room answering the question by unloading the chat.
        let admission = self.pool.admit(1, resolved.parallel_agents());
        if let Some(reason) = admission.reason {
            tracing::info!(
                opened = admission.open,
                queued = admission.queued,
                ?reason,
                "the card could not hold every slot asked for, so the rest queue"
            );
        }
        let config = B::with_slots(config, admission.open);

        // Weighed around the load, so the fit verdict knows what the resident
        // model gives back (`Pool::weigh`). Only a load that started a server
        // is weighed: one the supervisor answered with the server already
        // running changed nothing on the card.
        let resident = self.supervisor.current().await;
        let before = self.pool.card();
        match self.supervisor.ensure(config).await {
            Ok(backend) => {
                let started = resident
                    .as_ref()
                    .is_none_or(|resident| !Arc::ptr_eq(resident, &backend));
                if started {
                    self.pool.weigh(before, resident.is_some());
                }
                self.report(
                    Presence::Ready {
                        model: model.id.clone(),
                        // Asked of the backend, never repeated from the admission:
                        // the number shown to the user is the number actually
                        // opened, and a server that opened a different one is a
                        // defect this is the only place that could notice.
                        slots: backend.slots().await.unwrap_or(admission.open),
                        limit: admission.reason,
                    },
                    &mut report,
                )
            }
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

    /// What the resident model was weighed at holding on the card when it
    /// loaded, in bytes, and zero when nothing is resident. What a model chosen
    /// next gets back, which the fit verdict counts as room
    /// ([#74](https://github.com/elpideus/demido-studio/issues/74)).
    pub async fn held(&self) -> u64 {
        if self.supervisor.current().await.is_none() {
            return 0;
        }
        self.pool.held()
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
            let groups = crate::monitor::grouped(
                self.tools.registry().groups(),
                rebuild.tools.as_ref(),
                &rebuild.agent,
            );
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
                    demido_trace::Moment::Delegated(delegation) => Moment::Delegated(delegation),
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
        sink: impl FnMut(Update) + Send,
        approve: impl FnMut(Asking) -> F + Send,
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
        let registry = self.tools.narrowed(resolved.offered().as_deref());
        // What the pool may hold beside this conversation, read off the
        // presence rather than off the ladder: `Presence::Ready` carries the
        // slots the running backend said it opened, and a parallelism the card
        // could not honour is already a queue by the time it gets here
        // ([#65](https://github.com/elpideus/demido-studio/issues/65)).
        let slots = match self.presence() {
            Presence::Ready { slots, .. } => slots,
            _ => 1,
        };
        let agent = Agent {
            chat: self,
            child: None,
            // The conversation's own rendezvous. Every sub-agent mints one of
            // its own on the way in, so an ask is never ambiguous about whose
            // delegation it is.
            delegations: Arc::clone(&self.delegations),
            rules: Rules {
                // The conversation's own resolution, and the only one in this
                // crate that is minted rather than inherited
                // (`demido_permission::Resolution::root`). Every child's comes
                // from this one through `inherit`, which is what makes the two
                // controls of S2 ceilings rather than suggestions.
                resolution: Resolution::root(
                    registry.offered_names(),
                    Mode::named(resolved.mode()),
                ),
                registry,
                limit: resolved.step_limit(),
                // The conversation is level zero and the depth is at least
                // one, so this always answers that it may delegate: what it is
                // offered on that axis is the picker's, and `delegate_task`
                // switched off there is told as switched off. The number earns
                // its place in a child, where the same absence has the other
                // reason.
                depth: resolved.delegation_depth(),
                // Off the ladder's chat tier rather than off the log (#55). The
                // log still says which of the three the person answered,
                // because that is what happened; what is in force next turn is
                // a setting, so it is where every other value in force is, and
                // a person who wants it back has a row rather than an
                // un-appendable log to edit.
                always: resolved.always(),
            },
        };
        let sent = agent.compose(said, Carrying::Everything, &model, &resolved)?;
        let number = sent.turn;

        let cancel = Cancel::new();
        self.arm(Some(cancel.clone()));
        // Behind locks for the length of the turn, because above the default
        // parallelism a sub-agent runs beside the turn that asked for it and
        // both of them talk to the same window and the same person. The sink's
        // is a plain lock and is never held across an await; the approval's is
        // an async one and is held across the person's answer, which is the
        // right shape for a modal prompt: two sub-agents cannot ask at once.
        let sink = Mutex::new(sink);
        let approve = tokio::sync::Mutex::new(approve);
        let outcome = agent
            .steps(
                &backend,
                sent,
                &Driving {
                    cancel: &cancel,
                    sink: &sink,
                    approve: &approve,
                    slots: &Slots::under(slots),
                },
            )
            .await;
        self.arm(None);
        let mut sink = sink.into_inner().unwrap_or_else(|held| held.into_inner());

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
    fn with_session<T>(&self, act: impl FnOnce(&Session<Arc<J>>) -> Result<T>) -> Result<T> {
        let mut held = self.session.lock().unwrap_or_else(|held| held.into_inner());

        let session = match held.take() {
            Some(session) => session,
            None => {
                let session = Session::new(self.id.clone(), Arc::new((self.open)()?));
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

/// What one agent's run is ruled by: the tools on offer, what the matrix
/// decides about a call under them, how many steps it may take, and what has
/// already been answered *always for this tool*.
///
/// The conversation's is resolved off the ladder once per message. A
/// sub-agent's is its parent's, through [`inherit`] and through nothing else,
/// which is where the offered set and the mode become ceilings rather than
/// suggestions (`docs/rules/tools.md`).
struct Rules {
    registry: Registry,
    /// What this agent may be shown, the mode it is ruled under, and how many
    /// further levels of delegation may open below it.
    ///
    /// It carries no verdicts of its own: [`Resolution::verdict`] is the same
    /// matrix the conversation's calls go through, with this agent's mode in
    /// it, so a child has no permission shape to drift from its parent's.
    resolution: Resolution,
    limit: u32,
    always: Vec<String>,
    /// The delegation depth this agent's registry was derived under.
    ///
    /// Held rather than re-read, so `resolution.may_delegate(depth)` answers
    /// here the way it answered where the offered set was built: what the
    /// payload lacks and what a refusal says about the absence cannot disagree.
    /// A second reading of the ladder could land either side of a change made
    /// mid-turn and answer for a payload built under the other one.
    depth: u32,
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

/// What one call is ruled on with: the tools on offer, what the matrix decides
/// under this agent's resolution, and what the person has already said this
/// turn and before it.
struct Ruling<'a> {
    registry: &'a Registry,
    resolution: &'a Resolution,
    /// [`Rules::depth`], which is what tells the two reasons `delegate_task`
    /// can be missing apart.
    depth: u32,
    always: &'a mut Vec<String>,
    /// Calls the person declined this turn, by name and arguments.
    declined: &'a mut Vec<(String, serde_json::Value)>,
}

/// What a turn puts in front of the model before the message it is answering.
///
/// Two values rather than a `bool`, because the second of them is the whole
/// reason a delegation exists. The brief:
///
/// > Models should be able to delegate an agent to do a specific task in a
/// > separate clean context
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Carrying {
    /// Everything this agent has said and been told, by position on the log.
    Everything,
    /// Nothing at all, which is what a sub-agent starts with. Clean is not the
    /// same as hidden: the child's own half of the log is written in full, it
    /// is simply not carried into anybody's next request.
    Nothing,
}

/// What a turn is driven by, for the length of one.
///
/// Four things that always travel together, because every one of them is the
/// turn's rather than the agent's: what the window is told, who is asked about
/// a call, what a stop reaches, and how many sub-agents the card can hold
/// beside the conversation. A sub-agent runs on all four of the turn that asked
/// for it, which is what makes a child a run of the same loop rather than a
/// second one with a narrower idea of who is watching.
///
/// **Shared rather than borrowed uniquely**
/// ([#66](https://github.com/elpideus/demido-studio/issues/66)). Above the
/// default parallelism a sub-agent's run is a future the turn holds beside its
/// own, so two runs are driven by all of this at once and a `&mut` is a borrow
/// nothing can hand out twice. What each one needs exclusively it takes for as
/// long as it needs it: the sink for the length of one update, the person for
/// the length of one question.
///
/// Where a delegation is answered is **not** here any more, and that is the
/// change this shape is for. A rendezvous is the agent's, because an ask
/// answered by whichever of two concurrent loops polled first is a sub-agent
/// carried out correctly and recorded under the wrong parent.
struct Driving<'a, S, A> {
    cancel: &'a Cancel,
    /// Never held across an await, so a plain lock is the right one and a
    /// sub-agent cannot park the window behind it.
    sink: &'a Mutex<S>,
    /// Held across the person's answer, which is what makes one modal prompt at
    /// a time rather than two.
    approve: &'a tokio::sync::Mutex<A>,
    slots: &'a Slots,
}

impl<S: FnMut(Update), A> Driving<'_, S, A> {
    /// Tell the window something.
    ///
    /// One function rather than a lock taken at each of the call sites, so
    /// there is nowhere for a guard to be held across an await by accident.
    fn tell(&self, update: Update) {
        let mut sink = self.sink.lock().unwrap_or_else(|held| held.into_inner());
        sink(update);
    }
}

/// One sub-agent's run: where its answer ended up on the log, and what it said.
///
/// The position is an `Option` because a child whose log refused its own events
/// has nowhere for its answer to be, and an event that named a position nothing
/// is at would be worse than the absence.
///
/// Spelled once because it is written twice, and it is boxed for a reason the
/// declaration of [`Agent::run`] gives: this is the point in the recursion
/// where `Send` is named rather than inferred.
type Ran<'r> = Pin<Box<dyn Future<Output = Result<(Option<u64>, Outcome)>> + Send + 'r>>;

/// A delegation, opened: the sub-agent that will carry it out, or the reason
/// nothing will.
///
/// Two variants rather than an `Option`, because the absence is a **result**
/// the model is told about rather than a failure of the turn: a model that went
/// away under a delegation ends the delegation and nothing else.
enum Opened<'a, B: Backend, J: Journal> {
    Nowhere(Failure),
    /// Boxed because the two sides of this answer are nothing like the same
    /// size: one is a sentence, and the other is a whole agent and the request
    /// it opens with.
    Child(Box<Opening<'a, B, J>>),
}

/// A sub-agent that has been opened and has not generated yet.
struct Opening<'a, B: Backend, J: Journal> {
    child: Agent<'a, B, J>,
    /// Who the child is, carried out of the session that named it rather than
    /// read back off the agent: it is what `agent/returned` says came back, and
    /// reaching into the child for it would be asking a question the opening
    /// already answered.
    agent: demido_trace::AgentId,
    sent: Sent,
    backend: Arc<B>,
}

/// One agent running the loop: the conversation itself, or one sub-agent of it.
///
/// It exists because a delegation runs **the whole agent loop again**
/// ([#63](https://github.com/elpideus/demido-studio/issues/63)) rather than
/// interleaving a second kind of step into the first one's. There is one loop
/// in this crate and a child is another run of it, so there is no second path
/// through the matrix, no second way to answer a call, and no event kind that
/// means two things depending on whose turn it was.
///
/// What it is not is a second [`Chat`]. The backend, the ladder, the register
/// and the log are the conversation's, and this borrows them: a sub-agent with
/// a supervisor of its own would be a second model resident on a card the whole
/// design is sized against, and one with a log of its own would be the second
/// store `docs/decisions/0013-a-sub-agent-is-a-scope-on-one-log.md` refuses.
struct Agent<'a, B: Backend, J: Journal> {
    chat: &'a Chat<B, J>,
    /// Where this agent's own `delegate_task` calls are answered.
    ///
    /// The conversation's is the chat's; a sub-agent mints one and has its
    /// registry rebound to it on the way in
    /// (`demido_tools::Registry::delegating_to`). One rendezvous per agent is
    /// what keeps an ask unambiguous once two of them can be running at once
    /// ([#66](https://github.com/elpideus/demido-studio/issues/66)).
    delegations: Arc<Delegations>,
    /// A sub-agent's recorder, held for the length of its run.
    ///
    /// `None` is the conversation's own, which lives in the chat's lock because
    /// the window reads the transcript while a turn is running, and a session
    /// held out of that lock for a whole turn would be a second session opened
    /// over the same log to answer it.
    child: Option<Session<Arc<J>>>,
    rules: Rules,
}

impl<'a, B: Backend, J: Journal> Agent<'a, B, J> {
    /// Do something with this agent's recorder.
    ///
    /// A child's is held right here and a conversation's is in the chat's lock,
    /// and every recording site in the loop goes through this one function, so
    /// there is nowhere for an event to be written against the wrong agent.
    fn with_session<T>(&self, act: impl FnOnce(&Session<Arc<J>>) -> Result<T>) -> Result<T> {
        match self.child.as_ref() {
            Some(session) => act(session),
            None => self.chat.with_session(act),
        }
    }

    /// Record what this agent is about to send, and hand back the request that
    /// recording produced.
    ///
    /// The order is the crate's one invariant: recorded, then sent. A caller
    /// cannot send an assembly it did not record, because the assembly **is**
    /// what recording produced, and a crash between the two leaves a log that
    /// says what was about to happen.
    fn compose(
        &self,
        said: &str,
        carrying: Carrying,
        model: &str,
        resolved: &Resolved,
    ) -> Result<Sent> {
        // One reading of what is on offer, so the tools the log names, the
        // tools the request carries and the tools a call is planned against are
        // one list.
        let offered: Vec<(demido_prompts::Document, serde_json::Value)> = self
            .chat
            .tools
            .offered(&self.rules.registry)
            .into_iter()
            .map(|spec| (spec.document, spec.shape))
            .collect();
        let layer = layer(resolved.origin(demido_settings::id::TOOLS_OFFERED));

        // Everything up to the send is recording, and it happens under the
        // session lock. Nothing is awaited while it is held.
        self.with_session(|session| {
            let carry = match carrying {
                // History reaches the model as positions on the log rather than
                // as copies, so a long conversation does not grow the log as
                // the square of itself. It carries the calls and what came back
                // from them too, so a model is not made to call again for what
                // it already has. `Replay` scopes it to this agent, so a
                // conversation does not carry its sub-agents' messages.
                Carrying::Everything => Replay::of(session.journal())?.conversation(),
                Carrying::Nothing => Vec::new(),
            };
            let mut turn = session.begin();
            // Who the model is being goes first, before anything anybody said,
            // which is the only position a system message has.
            //
            // `Source::Inject` rather than `Source::System`: the taxonomy is
            // about who put the text in the window, and Demido put it there
            // without being asked this turn. `System` is text Demido *wrote*,
            // and this is the user's own, resolved off the ladder. An empty one
            // is left out entirely rather than sent as a blank message.
            //
            // A sub-agent gets it too. The clean context is the conversation it
            // is not carrying; who the model is being is not conversation.
            if !resolved.system_prompt().is_empty() {
                turn.message(Source::Inject, Role::System, resolved.system_prompt())?;
            }
            for seq in carry {
                turn.carry(seq);
            }
            turn.user(said)?;
            turn.offer(layer, &offered)?;
            turn.parameters(model, options(resolved))?;
            Ok(turn.send()?)
        })
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
    ///
    /// **A sub-agent gets its own budget of that many steps**, rather than a
    /// share of its parent's. A step limit is about one agent going round in a
    /// circle, which is what it is asked to end, and a child spending a parent's
    /// steps would make the limit mean something different depending on how far
    /// down the chain it was read. The chain's own bound is the depth.
    async fn steps<'s, S, A, F>(
        &'s self,
        backend: &'s B,
        mut sent: Sent,
        driving: &'s Driving<'_, S, A>,
    ) -> Result<Answer>
    where
        S: FnMut(Update) + Send + 's,
        A: FnMut(Asking) -> F + Send + 's,
        F: Future<Output = Decision> + Send,
        B: 's,
        J: 's,
    {
        let cancel = driving.cancel;
        // This agent's own delegations in flight. A sub-agent's run has one of
        // these too, because it runs the whole loop again and a background
        // delegation of its own is its to harvest.
        let mut flight = Flight::empty();
        let limit = self.rules.limit;
        // Standing answers come off the ladder, resolved once with everything
        // else this message is ruled by, so an *always* given on an earlier
        // message still holds on this one.
        let mut always = self.rules.always.clone();
        let mut declined: Vec<(String, serde_json::Value)> = Vec::new();
        let mut taken = 0u32;
        let mut withheld = false;

        loop {
            // The generation, with whatever is in flight advancing beside it.
            // This is the only place a sub-agent is polled while the
            // conversation is working, and nothing is written here: a child
            // that finishes waits in the buffer for the boundary below.
            let Generation { answer, calls } = flight
                .beside(self.stream(backend, &sent, cancel.clone(), driving))
                .await?;

            // A stop while the model was still producing: what it said is kept,
            // and a call that had already arrived is answered as stopped
            // rather than run, so the next message can carry it.
            if answer.reason == FinishReason::Cancelled {
                self.refuse_all(answer.turn, &calls, id::TOOLS_STOPPED, &[])?;
                // The stop is the sub-agents' too, through the token they
                // share, so this waits for endings rather than for work. What
                // they managed to say is still written down: a run that dropped
                // it would lose it exactly where somebody is most likely to
                // wonder what happened.
                self.settle(&mut flight, answer.turn, driving).await?;
                return Ok(answer);
            }
            if calls.is_empty() {
                // **A run may not end with a delegation in flight.** The model
                // has stopped asking for tools, so nothing else is going to
                // mention the sub-agent again, and a delegation nobody mentions
                // again is a silent loss.
                if !flight.is_busy() {
                    return Ok(answer);
                }
                let folded = self.settle(&mut flight, answer.turn, driving).await?;
                // Written down either way. Handed back to the model only if the
                // turn has a step left to hand it back in: past the ceiling the
                // answer is on the log and the turn is over, which is the
                // honest end rather than a step the limit says may not happen.
                if taken == limit || folded.is_empty() {
                    return Ok(answer);
                }
                taken += 1;
                let mut blocks = vec![answer.seq];
                blocks.extend(folded);
                sent = self.with_session(|session| {
                    Ok(session.step(&sent, &blocks, demido_trace::NextStep::Offering)?)
                })?;
                continue;
            }
            if taken == limit {
                let steps = limit.to_string();
                self.refuse_all(
                    answer.turn,
                    &calls,
                    id::TOOLS_LIMIT,
                    &[(catalog::STEPS, &steps)],
                )?;
                // **Out of steps it is still waited for and written down.** The
                // turn ends as a failure whatever the sub-agent says, and a
                // ceiling that ate an answer would be the one loss worse than
                // the one it exists to prevent.
                self.settle(&mut flight, answer.turn, driving).await?;
                return Err(Error::StepLimit { steps: limit });
            }

            let mut blocks = vec![answer.seq];
            let mut attempts: Vec<Attempt> = Vec::new();
            for (at, (seq, call)) in calls.iter().enumerate() {
                let ruling = Ruling {
                    registry: &self.rules.registry,
                    resolution: &self.rules.resolution,
                    depth: self.rules.depth,
                    always: &mut always,
                    declined: &mut declined,
                };
                match self
                    .dispatch(answer.turn, *seq, call, ruling, driving, &mut flight)
                    .await?
                {
                    Some(answered) => {
                        blocks.push(answered.block);
                        attempts.push(answered.attempt);
                        // The call has an answer now, and the transcript draws
                        // one row for the pair. The window is told there is
                        // something to read, and reads the log for what.
                        driving.tell(Update::Recorded);
                    }
                    // Stopped while this call waited or ran. It and every call
                    // after it are answered as stopped, and nothing else runs.
                    None => {
                        self.refuse_all(answer.turn, &calls[at..], id::TOOLS_STOPPED, &[])?;
                        self.settle(&mut flight, answer.turn, driving).await?;
                        return Ok(Answer {
                            reason: FinishReason::Cancelled,
                            ..answer
                        });
                    }
                }
            }

            // **The boundary.** Every call of this step has been answered, so
            // this is the one place a background answer may arrive, and it
            // arrives in the order the model asked for it.
            blocks.extend(self.fold_in(&mut flight, answer.turn, driving)?);

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
    async fn dispatch<'s, S, A, F>(
        &'s self,
        turn: u32,
        seq: u64,
        call: &ToolCall,
        ruling: Ruling<'_>,
        driving: &'s Driving<'_, S, A>,
        flight: &mut Flight<'s>,
    ) -> Result<Option<Answered>>
    where
        S: FnMut(Update) + Send + 's,
        A: FnMut(Asking) -> F + Send + 's,
        F: Future<Output = Decision> + Send,
        'a: 's,
        B: 's,
        J: 's,
    {
        let cancel = driving.cancel;
        // Registered, and not in the set: something closed it. Told as that
        // rather than as a name that is not a tool, which would send the model
        // looking for another way to do what was deliberately taken away
        // (`docs/rules/tools.md`).
        //
        // **Two absences, two wordings.** Nearly always somebody switched the
        // tool off in the picker, and that is what the model is told. The one
        // exception is `delegate_task` under an agent that has run out of
        // depth: the user did not turn this off, the chain reached its limit,
        // and a paragraph saying *the user turned it off* would send a
        // sub-agent to ask a person for a setting that is not the one in the
        // way. The absence is the same absence either way, which is the whole
        // point of #64 doing this with the offered set rather than with a
        // second mechanism.
        //
        // The set is the ladder's at every depth, because a child's is its
        // parent's and nothing narrows further yet (`Request::inheriting`). The
        // day a child asks for less, this branch needs the child's own ceiling
        // beside the chat's registry: `tools.md` is explicit that an absence a
        // sub-agent chose is not one the user is told they chose.
        if !ruling.registry.offers(&call.name) && self.chat.tools.registry().offers(&call.name) {
            let at_the_limit = call.name == demido_tools::DelegateTask::NAME
                && !ruling.resolution.may_delegate(ruling.depth);
            let reason = if at_the_limit {
                id::TOOLS_DEPTH
            } else {
                id::TOOLS_OFF
            };
            return self
                .refuse(turn, seq, reason, &[(catalog::TOOL, call.name.as_str())])
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

        if ruling
            .resolution
            .verdict(planned.tool(), &planned.intent, ruling.always)
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
            // The person, one question at a time. The lock is held across
            // their answer on purpose: above the default parallelism a
            // sub-agent runs beside the turn that asked for it, and two modal
            // prompts at once is a window nobody can answer.
            let decision = tokio::select! {
                biased;
                () = cancel.cancelled() => return Ok(None),
                decision = async {
                    let mut approve = driving.approve.lock().await;
                    approve(asking).await
                } => decision,
            };
            self.with_session(|session| Ok(session.decided(turn, seq, decision)?))?;

            match decision {
                // One prompt per turn for a delegation, not one per sub-agent
                // (`docs/rules/tools.md`). The grant is pushed onto this turn's
                // own copy of *always* and never written to the ladder, so the
                // second delegation of this turn runs and the first of the next
                // one asks again. Which tools that covers is
                // `demido_permission`'s to say: a tool decides nothing about
                // permission, and the loop decides nothing about which tool.
                Decision::Allow => {
                    if demido_permission::answers_for_the_turn(&call.name)
                        && !ruling.always.contains(&call.name)
                    {
                        ruling.always.push(call.name.clone());
                    }
                }
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
                    self.chat.remember_always(ruling.always);
                }
                Decision::Deny => {
                    ruling.declined.push(this);
                    return self
                        .refuse(turn, seq, id::TOOLS_DENIED, &declined)
                        .map(Answered::declined);
                }
            }
        }

        // What the person has answered *always for this tool* about as it
        // stands now, rather than as the ladder resolved it: a sub-agent opened
        // under this call inherits the answers already given this turn, the
        // delegation's own grant among them.
        let standing = ruling.always.clone();
        let Some(outcome) = self
            .running(&planned, turn, seq, &standing, driving, flight)
            .await?
        else {
            return Ok(None);
        };
        match outcome {
            Ok(text) => self.returned(turn, seq, &text, false),
            Err(failure) => self.returned(turn, seq, &failure.message, true),
        }
        .map(Answered::ran)
    }

    /// Run one call, carrying out any delegation it asks for on the way.
    ///
    /// `None` when a stop landed before the call finished.
    ///
    /// The middle arm is why this is a loop rather than the one `select!` it
    /// used to be. `delegate_task` holds the sending end of a
    /// [`crate::delegations`] pair, because a registry entry outlives every
    /// turn it is offered in and so cannot hold the turn's sink, the turn's
    /// person or the turn's cancellation. The tool asks and **this** answers,
    /// which puts the child's run on the stack of the turn that asked for it,
    /// with everything that turn has. That is also the whole of "at parallelism
    /// 1 the call blocks and the answer is the tool's result": the call is
    /// still awaiting its own future while the child runs, and what the child
    /// said is what that future resolves to.
    ///
    /// **The cancel arm is not raced against a child.** A delegation is carried
    /// out inside an arm, so nothing else is polled while it is, and a stop
    /// reaches the child the way it reaches this agent: through the token they
    /// share, ending its generation with a `Done` it records like any other.
    /// Dropping the child instead would be a second path to a stop, and this
    /// crate does not have one.
    async fn running<'s, S, A, F>(
        &'s self,
        planned: &demido_tools::Planned<'_>,
        turn: u32,
        call: u64,
        standing: &[String],
        driving: &'s Driving<'_, S, A>,
        flight: &mut Flight<'s>,
    ) -> Result<Option<Outcome>>
    where
        S: FnMut(Update) + Send + 's,
        A: FnMut(Asking) -> F + Send + 's,
        F: Future<Output = Decision> + Send,
        'a: 's,
        B: 's,
        J: 's,
    {
        let cancel = driving.cancel;
        // Dropping the call's future is what ends it, and for `run_command`
        // that kills the whole process tree (`demido-tools`' `tree`).
        let mut running = std::pin::pin!(planned.run());
        loop {
            let asked = tokio::select! {
                biased;
                () = cancel.cancelled() => return Ok(None),
                asked = self.delegations.next() => asked,
                outcome = &mut running => return Ok(Some(outcome)),
            };
            let opening = match self.open(asked.task(), turn, call, standing).await? {
                Opened::Child(opening) => *opening,
                Opened::Nowhere(failure) => {
                    asked.answer(Err(failure));
                    continue;
                }
            };
            let Opening {
                child,
                agent,
                sent,
                backend,
            } = opening;

            // **Above the default the call is answered at once.** A request
            // whose assistant message asks for a call nothing answered is one
            // no compatible server accepts, so the result cannot wait for the
            // sub-agent: it says the work has gone out, and what came back
            // arrives later as a message of its own.
            //
            // A slot is what decides, and taking one is also how the pool holds
            // its own size. Nothing here is a scheduler: at the default there
            // are no spare slots at all, and a full pool sends this delegation
            // down the blocking path rather than telling a model to try again.
            match driving.slots.take() {
                Some(permit) => {
                    let task = asked.task().to_owned();
                    flight.start(async move {
                        let (answer, outcome) = child.run(&backend, sent, driving).await?;
                        // The slot goes back before the answer is even built,
                        // so a delegation waiting for one gets it as soon as
                        // this sub-agent has stopped generating rather than
                        // when its answer is folded in.
                        drop(permit);
                        Ok(Harvest {
                            call,
                            agent,
                            task,
                            answer,
                            outcome,
                        })
                    });
                    asked.answer(Ok(self.acknowledged()?));
                }
                // The blocking path, which is the default and is also what a
                // full pool falls back to: the child runs on the stack of the
                // turn that asked for it, and what it said is this call's own
                // result. Nothing is folded in, because nothing was deferred.
                None => {
                    let (_, outcome) = child.run(&backend, sent, driving).await?;
                    asked.answer(outcome);
                }
            }
        }
    }

    /// What a delegation that did not wait answers its call with.
    ///
    /// Host prompt text, so a catalog entry (hard rule 10), and read the way
    /// every refusal in this loop is read.
    fn acknowledged(&self) -> Result<String> {
        let prompt = self
            .chat
            .tools
            .paragraph(id::AGENT_DELEGATED)
            .ok_or(Error::Unregistered(id::AGENT_DELEGATED))?;
        Ok(prompt.fill(&[]))
    }

    /// Wait for every delegation still in flight, then fold in what they said.
    ///
    /// The one path to an ending. A run whose model stopped asking for tools
    /// goes through it, and so does one that has used its last step and one a
    /// person stopped: a delegation nobody mentions again is a silent loss, and
    /// a ceiling that ate an answer would be worse than the runaway it exists
    /// to end.
    async fn settle<S: FnMut(Update), A>(
        &self,
        flight: &mut Flight<'_>,
        turn: u32,
        driving: &Driving<'_, S, A>,
    ) -> Result<Vec<u64>> {
        flight.settle().await;
        self.fold_in(flight, turn, driving)
    }

    /// Write down every background answer that has arrived, and hand back the
    /// blocks they became.
    ///
    /// **Called at a step boundary and nowhere else.** Two events per answer,
    /// and they are two on purpose: `agent/returned` is the fact that a
    /// delegation came back, naming the child's own answer by position rather
    /// than copying it, and the fragment beside it is what the parent's model
    /// is shown. A tool result would be the wrong shape twice over, because the
    /// call it would answer already has a result and a second one is a log that
    /// can disagree with itself about what came back.
    fn fold_in<S: FnMut(Update), A>(
        &self,
        flight: &mut Flight<'_>,
        turn: u32,
        driving: &Driving<'_, S, A>,
    ) -> Result<Vec<u64>> {
        let harvested = flight.take()?;
        if harvested.is_empty() {
            return Ok(Vec::new());
        }

        let frame = self
            .chat
            .tools
            .paragraph(id::AGENT_RETURNED)
            .ok_or(Error::Unregistered(id::AGENT_RETURNED))?;
        let mut blocks = Vec::with_capacity(harvested.len());
        for one in &harvested {
            blocks.push(self.with_session(|session| {
                if let Some(answer) = one.answer {
                    session.folded_in(turn, one.call, &one.agent, answer)?;
                }
                // `Source::Tool` rather than `Inject`, and it decides more than
                // a colour: `Replay::conversation` carries a fragment a tool put
                // there and leaves the ones Demido re-derives every turn, so the
                // sub-agent's answer is still in front of the model on the next
                // message rather than only for the rest of this turn.
                Ok(session.framed(
                    turn,
                    Source::Tool,
                    Role::User,
                    &frame,
                    &[
                        (catalog::TASK, one.task.as_str()),
                        (catalog::ANSWER, one.text()),
                    ],
                )?)
            })?);
        }

        driving.tell(Update::Recorded);
        Ok(blocks)
    }

    /// Open one delegation: a child session, its rules, and the request it
    /// starts with. Nothing is generated here.
    ///
    /// Split from running it because the two paths need the same opening and
    /// diverge after it ([#66](https://github.com/elpideus/demido-studio/issues/66)):
    /// at the default the child runs on the stack of the call that asked for
    /// it, and above it the child is put in the air and the call is answered at
    /// once. The `agent/delegated` event is written here either way, so a
    /// delegation is on the log where it was asked for rather than where it
    /// happened to finish.
    async fn open(
        &self,
        task: &str,
        turn: u32,
        call: u64,
        standing: &[String],
    ) -> Result<Opened<'a, B, J>> {
        // Both halves of "is there anything to talk to" answer the same way,
        // and it is a **result** rather than an error: a model that went away
        // under a delegation ends the delegation, not the turn that asked for
        // it. Only the log failing stops a run, at any depth.
        let answering = self
            .chat
            .answering()
            .ok()
            .zip(self.chat.supervisor.current().await);
        let Some((model, backend)) = answering else {
            // not-a-prompt: a tool result naming what was wrong with this call,
            // as the registry's own objections are.
            return Ok(Opened::Nowhere(Failure::final_(
                "there is no model to delegate to.",
            )));
        };

        // The child and the event that opened it come out together, so there is
        // no child that is not on the log. It shares this agent's journal
        // handle, and it is named after the call that asked for it.
        let session = self.with_session(|session| Ok(session.delegate(turn, call)?))?;
        // Resolved again rather than carried down, because the ladder is what
        // is in force: a value changed while the parent was generating rules
        // this child as it rules the next message.
        let resolved = self.chat.resolved();
        // The inheritance rule, on the way into this child as into every child
        // at every depth: the offered set intersected, the mode at its
        // stricter, the level one lower, and no fourth axis.
        //
        // **The depth is read here**, off the `resolved` above, which is the
        // ladder as it stands at this dispatch rather than as it stood when the
        // turn began. That is what makes a depth changed mid-conversation rule
        // the next delegation
        // ([#64](https://github.com/elpideus/demido-studio/issues/64)), and it
        // is also what decides whether `delegate_task` is in the set this child
        // is shown at all.
        let depth = resolved.delegation_depth();
        let resolution = inherit(&self.rules.resolution, &Request::inheriting(), depth);
        // This child's own rendezvous, and the registry it was handed rebound
        // to it. While a delegation blocked there was one loop awaiting one at
        // a time and one channel could not be ambiguous; above the default a
        // sub-agent runs beside the turn that asked for it, and an ask answered
        // by whichever of two loops polled first is a grandchild carried out
        // correctly and recorded under the wrong parent. A registry with no
        // `delegate_task` in it, which is what a child at the depth limit
        // inherits, is left exactly as it is.
        let (delegating, delegations) = crate::delegations();
        let registry = self
            .chat
            .tools
            .narrowed(Some(resolution.offered()))
            .delegating_to(delegating);
        let agent = session.agent().clone();
        let child = Agent {
            chat: self.chat,
            delegations: Arc::new(delegations),
            // What the child may call, narrowed to what it inherited, with its
            // `delegate_task` rebound to the rendezvous above.
            //
            // **What is rebound is a channel, never a session.** #63 got to
            // "no sub-agent holds a handle on a conversation it is not in" by
            // there being no handle to hold: the tool holds a channel, and
            // whichever loop answers on it records into its own session. That
            // is still true, and it is why the rebinding here is cheap. What
            // changed on #66 is that *which* loop answers stopped being
            // obvious: two of them run at once, so the channel has to name the
            // agent rather than the moment.
            rules: Rules {
                registry,
                resolution,
                limit: self.rules.limit,
                always: standing.to_vec(),
                // The one reading of the depth this child was built under, kept
                // so both questions are answered the same way: whether the tool
                // is in the registry a line above, and what this child is told
                // if it names the tool anyway.
                depth,
            },
            child: Some(session),
        };

        let sent = child.compose(task, Carrying::Nothing, &model, &resolved)?;
        Ok(Opened::Child(Box::new(Opening {
            child,
            agent,
            sent,
            backend,
        })))
    }

    /// Run this sub-agent's turn to its end, and say what it answered and where
    /// that answer is.
    ///
    /// **A tool failure inside a child is a result, not an error.** The child
    /// answers its own calls with what came back from them, exactly as its
    /// parent does, and a turn that ended badly comes back as a failed result
    /// the parent can act on. Only [`Error::Journal`] is returned, and it is the
    /// one thing that stops a run at any depth: a child whose events cannot be
    /// written is a child nothing can say happened.
    ///
    /// The position it hands back is what `agent/returned` names: the
    /// completion the child ended on, or the failure it ended on instead. Both
    /// are events on the child's own half of the log, which is the point of
    /// naming rather than copying.
    ///
    /// **It hands back a boxed future rather than being an `async fn`**, and
    /// that is load-bearing rather than style. This is the recursion: a child's
    /// loop dispatches a call, which opens a delegation, which runs a loop, and
    /// a background delegation is a future the turn holds, so the whole chain
    /// has to be `Send`. A compiler asked to decide `Send` for a future that
    /// awaits itself reports a cycle rather than an answer. Naming the bound at
    /// one point in the ring turns the cycle into a check, and this is the
    /// natural place for it because it is also where the recursion is already
    /// boxed for its size.
    fn run<'r, S, A, F>(self, backend: &'r B, sent: Sent, driving: &'r Driving<'_, S, A>) -> Ran<'r>
    where
        S: FnMut(Update) + Send + 'r,
        A: FnMut(Asking) -> F + Send + 'r,
        F: Future<Output = Decision> + Send,
        'a: 'r,
        B: 'r,
        J: 'r,
    {
        Box::pin(async move {
            let number = sent.turn;
            // The same token, so the parent's Stop is the child's, at every depth.
            // A cancel that leaves a sub-agent generating against a model nobody is
            // waiting for is the next question's VRAM.
            //
            match self.steps(backend, sent, driving).await {
                Ok(answer) => Ok((Some(answer.seq), Ok(answer.text))),
                Err(Error::Journal(journal)) => Err(Error::Journal(journal)),
                Err(error) => {
                    // On the child's own half of the log, because it is the child's
                    // turn that ended. Best effort for the reason `ask`'s is: when
                    // the log is what failed, recording that it failed fails too.
                    let detail = error.to_string();
                    // Nothing on the log to point at when the write itself is
                    // what failed, and `None` says so rather than a position
                    // nothing is at.
                    let at = self
                        .with_session(|session| {
                            Ok(session.failed(number, error.kind(), &detail)?)
                        })
                        .inspect_err(|unrecorded| {
                            tracing::warn!(%unrecorded, "the sub-agent's failure was not recorded");
                        })
                        .ok();
                    // not-a-prompt: what the sub-agent's turn said went wrong,
                    // handed to the model that asked for it as the call's result.
                    Ok((at, Err(Failure::final_(detail))))
                }
            }
        })
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
        let prompt = self
            .chat
            .tools
            .paragraph(id)
            .ok_or(Error::Unregistered(id))?;
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
    async fn stream<S, A, F>(
        &self,
        backend: &B,
        sent: &Sent,
        cancel: Cancel,
        driving: &Driving<'_, S, A>,
    ) -> Result<Generation>
    where
        S: FnMut(Update) + Send,
        A: FnMut(Asking) -> F + Send,
        F: Future<Output = Decision> + Send,
    {
        let mut stream = backend.generate(sent.request.clone(), cancel).await?;

        let mut text = String::new();
        let mut thinking = String::new();
        let mut calls: Vec<ToolCall> = Vec::new();
        let mut finished: Option<(FinishReason, Usage)> = None;

        while let Some(chunk) = stream.next().await {
            match chunk? {
                Chunk::Text { text: said } => {
                    text.push_str(&said);
                    driving.tell(Update::Text { text: said });
                }
                Chunk::Thinking { text: thought } => {
                    thinking.push_str(&thought);
                    driving.tell(Update::Thinking { text: thought });
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
            driving.tell(Update::Recorded);
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
}
