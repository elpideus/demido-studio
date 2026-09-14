//! Settings that never reach a disk.
//!
//! The second implementation, and it earns its place twice. It is what every
//! test about the ladder rather than about a file uses, and it is what a build
//! with nowhere to write would be:
//! [`docs/rules/tiles.md`](../../../../../docs/rules/tiles.md) calls a swap of
//! this kind one wiring line, and this is the line that has to already work for
//! that claim to be true.
//!
//! It is `Clone`, and two clones share one document. That is not a convenience,
//! it is what makes it a store: the contract asks that a store opened again
//! over the same storage reads back what the last one wrote, and for this
//! implementation "opened again" is a clone.

use std::sync::{Arc, Mutex};

use crate::document::Document;
use crate::store::{Result, Store};

/// Settings in memory, shared by every clone of the handle.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    document: Option<Document>,
    writes: usize,
}

impl Memory {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many writes have reached this store. What a test asserting that a
    /// change was saved once, rather than on every keystroke, reads.
    #[must_use]
    pub fn writes(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .writes
    }
}

impl Store for Memory {
    fn read(&self) -> Result<Document> {
        Ok(self
            .state
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .document
            .clone()
            .unwrap_or_default())
    }

    fn write(&self, document: &Document) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|held| held.into_inner());
        state.document = Some(document.clone());
        state.writes += 1;
        Ok(())
    }
}
