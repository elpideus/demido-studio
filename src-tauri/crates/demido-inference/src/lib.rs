//! The inference seam.
//!
//! One trait, one implementation so far, and one contract suite holding any
//! future implementation to the same promises. Everything above this crate is
//! written against the contract rather than against `llama.cpp`, which is what
//! makes a backend swappable
//! ([`docs/rules/tiles.md`](../../../../docs/rules/tiles.md)).

pub mod backend;
pub mod contract;
pub mod llamacpp;
pub mod model;
pub mod supervisor;

pub use backend::{Backend, Cancel, ChunkStream, Error, Result};
pub use llamacpp::{Config as LlamaCppConfig, LlamaCpp, Offload};
pub use model::{Chunk, FinishReason, Loaded, Message, Options, Request, Role, Usage};
pub use supervisor::Supervisor;
