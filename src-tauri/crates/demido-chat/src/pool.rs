//! The pool: how many slots this conversation gets, and why not more.
//!
//! A sub-agent runs the conversation's **own weights on a second `llama.cpp`
//! slot** rather than a second model, so what parallelism costs is a KV
//! reservation and nothing else. `demido-vram` owns the arithmetic and the
//! reading; this module is the one place that puts them together and hands the
//! answer to a load.
//!
//! **It is a module, not a trait.** A `Scheduler` trait would buy a second
//! implementation that exists only in tests, and
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md) says a trait is a
//! decision rather than a default. What the tests actually need is a card that
//! answers a chosen number, and that is a closure.
//!
//! ## What the number means
//!
//! `tools.parallel_agents` counts **slots**, and the conversation is the first
//! of them: at 1 there is one generation at a time, and at 4 there are three
//! sub-agents beside the conversation. So the conversation's own slot goes into
//! `admit` as already open and is never the pool's to refuse. It is the model,
//! and a budget that could refuse it would be a card with no room answering the
//! question by unloading the chat.
//!
//! `--ctx-size` is per slot and `demido_inference::llamacpp::arguments`
//! multiplies by exactly the number that opens, so the KV of every slot that
//! opens is in the reservation the server is started with: **either the slot's
//! KV is shown in the context arithmetic, or the slot is not opened.**
//!
//! ## The reading is taken at the load, never before it
//!
//! [`Pool::admit`] calls the card every time. The card's free memory is not
//! what a settings page saw when it was drawn: a browser window opened in
//! between moves it by more than a slot costs, and a decision made against the
//! stale figure is an allocation failure inside the driver rather than a queue.
//!
//! It is taken **before the weights land**, so it is the card as it stands
//! rather than as it will stand. Answering that needs the load priced whole,
//! which is the fit verdict's
//! ([#74](https://github.com/elpideus/demido-studio/issues/74)) and is also
//! what will first measure a slot at all. Until it does, `per_slot` is
//! unmeasured and the only admission reachable from a window is the one that
//! needs no room: the default.

use std::sync::Arc;

use demido_vram::{admit, free_now, Admission, Budget, Card};

/// How the card is asked. A closure rather than a trait, per the note above.
type Reading = Arc<dyn Fn() -> Option<Card> + Send + Sync>;

/// What a conversation may open, decided against the card at the moment it
/// opens it.
#[derive(Clone)]
pub struct Pool {
    reading: Reading,
    /// What one slot's KV reserves on the model in force, in bytes.
    ///
    /// **Measured, never assumed**, and zero until something has measured one.
    /// Zero is not free: `demido_vram::admit` queues an unmeasured slot and
    /// says so, because a slot admitted on a number nobody measured is the
    /// paging-through-host-memory failure the whole budget exists to avoid. The
    /// producer is the fit verdict
    /// ([#74](https://github.com/elpideus/demido-studio/issues/74)), which
    /// reads the attention geometry a KV cache is sized from; until it lands,
    /// parallelism above the slot a conversation already has queues with a
    /// stated reason, which is the honest answer rather than a guess.
    per_slot: u64,
}

impl Pool {
    /// The pool this build runs: the real card, asked each time.
    #[must_use]
    pub fn on_the_card() -> Self {
        Self {
            reading: Arc::new(free_now),
            per_slot: 0,
        }
    }

    /// A card that always reports `free`, on which a slot reserves `per_slot`.
    ///
    /// What a suite runs against: a conversation's slot count is a decision
    /// about a card, and a test that asked the machine it happens to be running
    /// on would pass or fail by what else has a browser open.
    #[must_use]
    pub fn on_a_card_with(free: u64, per_slot: u64) -> Self {
        let card = Card { free, total: free };
        Self {
            reading: Arc::new(move || Some(card)),
            per_slot,
        }
    }

    /// How many slots to open, given the slots already open and the ladder's
    /// `tools.parallel_agents`.
    ///
    /// A card that cannot be read is a card with nothing to spare, and the
    /// slots already open stay open: this never evicts and never closes, so an
    /// unreadable card costs the conversation nothing it already has.
    #[must_use]
    pub fn admit(&self, open: u32, wanted: u32) -> Admission {
        let free = (self.reading)().map_or(0, |card| card.free);
        admit(Budget {
            free,
            per_slot: self.per_slot,
            open,
            wanted,
        })
    }
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Pool")
            .field("per_slot", &self.per_slot)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use demido_vram::{Queued, MIB};

    /// The rig's reference model at 32k on the 12 GB card: 423 MiB free, and a
    /// slot costs 1129. The default asks for nothing, so it is admitted on a
    /// card with no room at all.
    #[test]
    fn the_default_opens_the_one_slot_the_conversation_already_has() {
        let pool = Pool::on_a_card_with(423 * MIB, 1129 * MIB);
        let admission = pool.admit(1, 1);

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 0);
        assert_eq!(admission.reason, None);
    }

    /// The same card asked for two. It degrades to a queue with the reason
    /// stated, rather than to a load that fails.
    #[test]
    fn a_parallelism_the_card_cannot_honour_queues() {
        let pool = Pool::on_a_card_with(423 * MIB, 1129 * MIB);
        let admission = pool.admit(1, 2);

        assert_eq!(admission.open, 1, "the slot that is open stays open");
        assert_eq!(admission.queued, 1);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 423 * MIB,
                needed: 1129 * MIB,
            })
        );
    }

    /// The development model on the same card has room, so the same setting is
    /// honoured. Which is the point of it being a budget rather than a
    /// preference: the answer depends on what is loaded.
    #[test]
    fn a_card_with_room_opens_what_was_asked_for() {
        let pool = Pool::on_a_card_with(5583 * MIB, 563 * MIB);
        assert_eq!(pool.admit(1, 4).open, 4);
    }

    /// Nothing has measured a slot, so nothing is admitted on a guess, however
    /// much is free.
    #[test]
    fn an_unmeasured_slot_queues_on_the_emptiest_card() {
        let pool = Pool::on_a_card_with(12288 * MIB, 0);
        let admission = pool.admit(1, 4);

        assert_eq!(admission.open, 1);
        assert_eq!(admission.reason, Some(Queued::Unmeasured));
    }

    /// A card nothing could read is a card with nothing to spare, and the slot
    /// the conversation already has is untouched. Never an eviction.
    #[test]
    fn an_unreadable_card_opens_nothing_new_and_closes_nothing() {
        let pool = Pool {
            reading: Arc::new(|| None),
            per_slot: 563 * MIB,
        };
        let admission = pool.admit(1, 3);

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 2);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 0,
                needed: 563 * MIB,
            })
        );
    }

    /// The reading is taken per call, not at construction. A card that empties
    /// between two loads is the ordinary case, and a pool that had cached the
    /// first answer would open a slot against memory somebody else now holds.
    #[test]
    fn the_card_is_asked_every_time() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let asked = Arc::new(AtomicU64::new(0));
        let counted = asked.clone();
        let pool = Pool {
            reading: Arc::new(move || {
                // 4 MiB the first time, nothing after it.
                let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
                Some(Card {
                    free: if first { 4 * MIB } else { 0 },
                    total: 12288 * MIB,
                })
            }),
            per_slot: MIB,
        };

        assert_eq!(pool.admit(1, 2).open, 2);
        assert_eq!(
            pool.admit(1, 2).open,
            1,
            "the second reading is the one that decides"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }
}
