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
//! which is what the fit verdict does with this same arithmetic
//! ([#74](https://github.com/elpideus/demido-studio/issues/74)). What a slot
//! costs is still unmeasured
//! ([#105](https://github.com/elpideus/demido-studio/issues/105)), so the only
//! admission reachable from a window is the one that needs no room: the
//! default.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use demido_vram::{admit, free_now, weighed, Admission, Budget, Card};

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
    /// producer is a header reading of the model in force, which reads the
    /// attention geometry a KV cache is sized from
    /// ([#105](https://github.com/elpideus/demido-studio/issues/105)); until it
    /// lands, parallelism above the slot a conversation already has queues with
    /// a stated reason, which is the honest answer rather than a guess.
    per_slot: u64,
    /// What the resident model was weighed at holding on the card, in bytes,
    /// by [`Pool::weigh`]. Zero until a load has been weighed.
    ///
    /// Shared between clones: there is one card and one resident model, and a
    /// copy of the pool that had its own figure would be a second answer to
    /// what is on it.
    held: Arc<AtomicU64>,
}

impl Pool {
    /// The pool this build runs: the real card, asked each time.
    #[must_use]
    pub fn on_the_card() -> Self {
        Self {
            reading: Arc::new(free_now),
            per_slot: 0,
            held: Arc::default(),
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
            held: Arc::default(),
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

impl Pool {
    /// The card, read now.
    #[must_use]
    pub fn card(&self) -> Option<Card> {
        (self.reading)()
    }

    /// Weigh the server a load just started: `before` is the card as it read
    /// before the load, and `replaced` says whether a server this pool had
    /// weighed was resident then and has made way.
    ///
    /// Weighed rather than taken from the model file, because a file is not
    /// what lands on the card (`demido_vram::weighed`). What it is for is the
    /// fit verdict ([#74](https://github.com/elpideus/demido-studio/issues/74)):
    /// the resident model is room for the one that would replace it, and this
    /// is how much.
    pub fn weigh(&self, before: Option<Card>, replaced: bool) {
        let given_back = if replaced { self.held() } else { 0 };
        let held = weighed(before, given_back, self.card()).unwrap_or(0);
        self.held.store(held, Ordering::SeqCst);
    }

    /// What the resident model was weighed at holding. Zero when nothing has
    /// been weighed, or the card could not be read around the load.
    #[must_use]
    pub fn held(&self) -> u64 {
        self.held.load(Ordering::SeqCst)
    }
}

impl std::fmt::Debug for Pool {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.debug_struct("Pool")
            .field("per_slot", &self.per_slot)
            .field("held", &self.held())
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
            held: Arc::default(),
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
            held: Arc::default(),
        };

        assert_eq!(pool.admit(1, 2).open, 2);
        assert_eq!(
            pool.admit(1, 2).open,
            1,
            "the second reading is the one that decides"
        );
        assert_eq!(asked.load(Ordering::SeqCst), 2);
    }

    /// A pool that answers each reading in turn, from `done.md`'s weighed
    /// table.
    fn reading_in_turn(frees: &'static [u64]) -> Pool {
        let next = Arc::new(AtomicU64::new(0));
        Pool {
            reading: Arc::new(move || {
                let at = usize::try_from(next.fetch_add(1, Ordering::SeqCst)).unwrap();
                Some(Card {
                    free: frees[at.min(frees.len() - 1)] * MIB,
                    total: 12288 * MIB,
                })
            }),
            per_slot: 0,
            held: Arc::default(),
        }
    }

    /// The load is weighed the way `done.md` weighed one: nothing loaded, the
    /// development model at 32k on one slot, then on two slots replacing it.
    /// What the pool holds is the table's own cost column each time, and a
    /// clone of the pool reads the same figure.
    #[test]
    fn a_load_is_weighed_and_a_replacement_gives_the_first_back() {
        // Read once before the first load and once after it, then the same
        // for the second.
        let pool = reading_in_turn(&[7984, 2235, 2235, 1615]);
        let copy = pool.clone();
        assert_eq!(pool.held(), 0, "nothing has been weighed");

        let before = pool.card();
        pool.weigh(before, false);
        assert_eq!(copy.held(), 5749 * MIB);

        let before = pool.card();
        pool.weigh(before, true);
        assert_eq!(copy.held(), 6369 * MIB);
    }

    /// A load the card could not be read around weighs nothing, rather than a
    /// figure nobody took.
    #[test]
    fn a_load_nobody_could_weigh_holds_nothing() {
        let pool = Pool {
            reading: Arc::new(|| None),
            per_slot: 0,
            held: Arc::new(AtomicU64::new(5749 * MIB)),
        };
        pool.weigh(None, true);
        assert_eq!(pool.held(), 0);
    }
}
