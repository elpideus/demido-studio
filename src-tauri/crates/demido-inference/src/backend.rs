//! The inference seam: the only trait S1 introduces.
//!
//! One trait covering the whole life of a backend, because for a supervised
//! `llama.cpp` those are not separable concerns: the thing that answers a
//! request is the process that had to be started first, and a caller that can
//! generate but cannot tell whether the server is up has to discover it from a
//! failure.
//!
//! The second implementation is an OpenAI-compatible endpoint, and it is what
//! [`crate::contract`] is written for rather than something it will be adapted
//! to later ([`docs/rules/tiles.md`](../../../../docs/rules/tiles.md): the
//! contract suite is the trait's second file, and it is written before the
//! second implementation exists). For such an endpoint `start` connects and
//! `stop` is a no-op, which the contract already allows because it never asks
//! that stopping kill anything, only that a stopped backend stop claiming to be
//! ready.

use std::pin::Pin;

use futures_core::Stream;

use crate::model::{Chunk, Loaded, Request};

pub type Result<T> = std::result::Result<T, Error>;

/// A stream of chunks. Boxed because backends produce very different stream
/// types and the caller should not care which.
pub type ChunkStream = Pin<Box<dyn Stream<Item = Result<Chunk>> + Send>>;

/// A generation the caller can call off.
///
/// `tokio_util`'s token rather than one of ours: it is exactly this, it is
/// already in the dependency tree through `tokio`, and a newtype over it would
/// be a middle man with a different name.
pub type Cancel = tokio_util::sync::CancellationToken;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A backend Demido supervises never came up. Distinct from
    /// [`Error::Unreachable`], which is a server that exists and is not
    /// answering: this one is a process that failed, and it usually said why
    /// before it died.
    #[error("{backend} did not start: {detail}")]
    DidNotStart { backend: String, detail: String },

    #[error("{backend} is not reachable: {detail}")]
    Unreachable { backend: String, detail: String },

    /// The backend said no, and said why. Kept apart from
    /// [`Error::Unreachable`] because one is worth retrying and the other is
    /// not.
    #[error("{backend} refused the request: {detail}")]
    Refused { backend: String, detail: String },

    #[error("the response could not be read: {0}")]
    Malformed(String),
}

impl From<Error> for demido_core::Error {
    fn from(error: Error) -> Self {
        demido_core::Error::unavailable("inference", error.to_string())
    }
}

/// Something that can run a model.
///
/// Implementations must be safe to share across threads: the supervisor hands
/// the same backend to every caller that wants to talk to it, and serialising
/// them is the backend's decision to make rather than the caller's.
///
/// The guarantees, which [`crate::contract`] is the executable statement of:
///
/// - `start` returns only once the backend is ready, so a caller holding one
///   has a backend that works.
/// - A successful stream ends with **exactly one** [`Chunk::Done`], and nothing
///   follows it, so a caller knows a turn is over without counting anything.
/// - Cancelling ends the stream promptly, and what was already generated is
///   kept.
/// - After `stop`, `ready` is false.
#[async_trait::async_trait]
pub trait Backend: Send + Sync + Sized + 'static {
    /// What it takes to start one.
    ///
    /// Compared for equality to decide whether the running backend is the one
    /// being asked for ([`crate::Supervisor`]), so everything that changes what
    /// gets loaded has to be part of it.
    type Config: Clone + PartialEq + Send + Sync + 'static;

    /// Short, stable, and used in error messages the user will read.
    fn name() -> &'static str;

    /// Start against a model and a set of parameters, returning once it is
    /// ready to answer.
    async fn start(config: Self::Config) -> Result<Self>;

    /// The same configuration, asking for a different window per generation.
    ///
    /// The writer beside [`Backend::context_length`], and the two are a pair on
    /// purpose: the settings ladder resolves a number, this is how that number
    /// reaches a process, and the reader is how the contract checks it arrived
    /// ([`crate::contract`]). A caller cannot do this by editing a field,
    /// because `Config` is an associated type and the ladder does not know
    /// which backend it is talking to.
    ///
    /// It takes and returns the configuration rather than mutating one, so that
    /// a supervisor comparing the running configuration against the wanted one
    /// is comparing two whole values ([`crate::Supervisor::ensure`]).
    fn with_context_length(config: Self::Config, tokens: u32) -> Self::Config;

    /// Whether it is answering. Asked on every request rather than assumed from
    /// the fact that it started once, because a process that exited leaves a
    /// handle that looks fine from the outside.
    async fn ready(&self) -> bool;

    /// The model this backend is serving.
    async fn loaded(&self) -> Result<Loaded>;

    /// The context length the generation slot actually has.
    ///
    /// A method rather than a copy of what the caller asked for, because those
    /// two numbers coming apart is a known defect rather than a hypothetical:
    /// `llama.cpp`'s `--ctx-size` is **per slot**, so with the default four
    /// slots a caller asking for 4096 gets 1024 each and 16k of KV reserved
    /// ([`docs/rules/done.md`](../../../../docs/rules/done.md)). The contract
    /// asks the backend what the slot got, so the claim is checked rather than
    /// commented.
    async fn context_length(&self) -> Result<u32>;

    /// Run one generation, ending early if `cancel` fires.
    async fn generate(&self, request: Request, cancel: Cancel) -> Result<ChunkStream>;

    /// Stop, and wait until the resources are back.
    ///
    /// Takes `&self` because a running backend is shared: whoever decides it
    /// should stop is rarely the last holder of it. Calling it twice is fine.
    async fn stop(&self);
}
