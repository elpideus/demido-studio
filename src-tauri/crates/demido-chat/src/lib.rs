//! The conversation: what is said, what is sent, and what comes back.
//!
//! This crate is the turn loop. It sits between [`demido_trace`], which is the
//! source of truth, and [`demido_inference`], which is the seam a request goes
//! through, and it owns the one thing neither of them can: the order those two
//! happen in.
//!
//! Brief B06: "everything about what happens, from the input getting sent to
//! the model using the tools available up until the reply (and after, but we'll
//! get to this later), should be visible."
//!
//! ## There is no chat state
//!
//! [`Chat`] holds a journal, a supervisor and a cancellation token. It holds no
//! messages. A transcript is [`Chat::transcript`], which is a projection of the
//! log, so closing the window and opening it again is the same read as drawing
//! the desk for the first time. That is not a feature of this crate, it is the
//! absence of one: v2 kept a message list beside its log and the two could
//! disagree about a conversation, which is exactly the claim the product makes
//! about itself.
//!
//! ## The order is the design
//!
//! A turn records itself, hands back the request that recording produced, and
//! only then sends it ([`demido_trace::Turn::send`]). So the assembly on the
//! log is what the backend was given rather than a description written beside
//! it, and a crash between the two leaves a log that says what was about to
//! happen.
//!
//! A stop is the same shape. Cancelling ends the stream with one
//! [`demido_inference::Chunk::Done`] carrying
//! [`demido_inference::FinishReason::Cancelled`], so the partial answer and the
//! stop are recorded by the same line of code that records a completed one.
//! There is no second path, and therefore no second path to forget.
//!
//! ## A delegation is another run of the same loop
//!
//! [#63](https://github.com/elpideus/demido-studio/issues/63). A delegated task
//! opens a **child session**: a second recorder over the conversation's own
//! journal handle, with its own agent, running the whole loop again in a
//! separate clean context. There is one loop in this crate, so there is no
//! second path through the matrix and no event kind that means two things
//! depending on whose turn it was.
//!
//! The child runs on the stack of the turn that asked for it, through the pair
//! in [`delegation`], so at the default parallelism the call blocks and the
//! answer is the tool's own result. The parent's cancellation token is the
//! child's, at every depth.
//!
//! ## What is here
//!
//! | Module | What |
//! |---|---|
//! | [`presence`] | Whether there is anything to talk to, and what to say if not. |
//! | [`update`] | What the window is told while a turn runs. |
//! | [`chat`] | The session, the assembly, and the loop. |
//! | [`delegation`] | The two ends of a delegation: the tool's, and the loop's. |
//! | [`toolbox`] | What a conversation may call, and what a person is asked. |
//! | [`monitor`] | What the session monitor draws over one assembly. |
//!
//! See `AGENTS.md` beside this file for the invariants.

pub mod chat;
pub mod delegation;
pub mod monitor;
pub mod presence;
pub mod toolbox;
pub mod update;

pub use chat::{Answer, Chat, Error, Model, Moment, Result, Said};
pub use delegation::{delegations, Asked, Delegations};
pub use demido_trace::{Called, Decision, Outcome};
pub use monitor::{Assembly, Grouped, Standing};
pub use presence::Presence;
pub use toolbox::{Asking, Offering, Toolbox};
pub use update::Update;
