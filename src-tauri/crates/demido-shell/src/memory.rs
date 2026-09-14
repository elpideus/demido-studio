//! A layout that never reaches a disk.
//!
//! The second implementation, and it earns its place twice. It is what every
//! test about the debouncer rather than about a file uses, and it is what a
//! build with nowhere to write would be: `docs/rules/tiles.md` calls a swap of
//! this kind one wiring line, and this is the line that has to already work for
//! that claim to be true.
//!
//! It is `Clone`, and two clones share one layout. That is not a convenience,
//! it is what makes it a store: the contract asks that a store opened again
//! over the same storage reads back what the last one wrote, and for this
//! implementation "opened again" is a clone.

use std::sync::{Arc, Mutex};

use crate::layout::Shell;
use crate::store::{Result, Store};

/// A layout in memory, shared by every clone of the handle.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    shell: Option<Shell>,
    /// How many times the layout has actually been replaced.
    ///
    /// The debouncer's whole claim is that a hundred gestures cost one write,
    /// and a store that only reports its final contents cannot tell the
    /// difference between one write and a hundred. This is how that claim is
    /// asserted rather than described.
    writes: usize,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many writes have reached this store.
    #[must_use]
    pub fn writes(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .writes
    }
}

impl Store for Memory {
    fn read(&self) -> Option<Shell> {
        self.state
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .shell
    }

    fn write(&self, shell: &Shell) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(|held| held.into_inner());
        state.shell = Some(*shell);
        state.writes += 1;
        Ok(())
    }
}
