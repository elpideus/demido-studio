//! A journal that never reaches a disk.
//!
//! The second implementation, and it earns its place twice. It is what every
//! test that is about a turn rather than about a file uses, and it is what a
//! session the user asked not to keep will be: `docs/rules/tiles.md` calls a
//! swap of this kind one wiring line, and this is the line that has to already
//! work for that claim to be true.
//!
//! It is `Clone`, and two clones share one log. That is not a convenience, it
//! is what makes it a journal: the contract asks that a journal opened again
//! over the same storage replays everything, and for this implementation
//! "opened again" is a clone.

use std::sync::{Arc, Mutex};

use crate::event::{Entry, Event};
use crate::journal::{now, Journal, Result};

/// A log in memory, shared by every clone of the handle.
#[derive(Debug, Clone, Default)]
pub struct Memory {
    events: Arc<Mutex<Vec<Event>>>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Journal for Memory {
    fn append(&self, entry: Entry) -> Result<Event> {
        let mut events = self.events.lock().unwrap_or_else(|held| held.into_inner());

        let event = Event {
            seq: events.len() as u64 + 1,
            at: now(),
            session: entry.session,
            turn: entry.turn,
            source: entry.source,
            weight: entry.weight,
            body: entry.body,
        };
        events.push(event.clone());
        Ok(event)
    }

    fn events(&self) -> Result<Vec<Event>> {
        Ok(self
            .events
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone())
    }
}
