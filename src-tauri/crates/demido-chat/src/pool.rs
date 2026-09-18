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
//! rather than as it will stand, and the load is priced whole against it: the
//! weights, the conversation's own slot and what the build holds beside them
//! (`demido_vram::BESIDE_THE_KV`) come out of the reading first, with
//! what the resident model was weighed at holding given back when this load
//! replaces it, and the slots above the first are admitted from what is left.
//! A reading taken before a 9 GB model lands that admitted slots against the
//! whole of it would be the paging failure the budget exists to prevent.
//!
//! What a slot costs is read from the header of the model in force, at the
//! context length in force (`demido_models::slot`,
//! [#105](https://github.com/elpideus/demido-studio/issues/105)). An
//! architecture that reading does not know is unmeasured, and queues every slot
//! above the first with that reason rather than admitting one on a guess.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use demido_vram::{admit, free_now, weighed, Admission, Budget, Card, Load, BESIDE_THE_KV};

/// How the card is asked. A closure rather than a trait, per the note above.
type Reading = Arc<dyn Fn() -> Option<Card> + Send + Sync>;

/// What a conversation may open, decided against the card at the moment it
/// opens it.
#[derive(Clone)]
pub struct Pool {
    reading: Reading,
    /// A slot's price pinned by a test's card, whose reading is then the room
    /// beside the load rather than the card before it.
    ///
    /// `None` on the real card, where both come from the load being admitted
    /// ([`Pool::admit`]). A scripted backend has no file to price a slot from
    /// and no weights to land, so a suite's card answers the whole question
    /// instead: what the conversation's model leaves, and what a slot costs.
    pinned_price: Option<u64>,
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
            pinned_price: None,
            held: Arc::default(),
        }
    }

    /// A card on which the conversation's model leaves `free`, and a slot
    /// reserves `per_slot`, whatever is loaded.
    ///
    /// What a suite runs against: a conversation's slot count is a decision
    /// about a card, and a test that asked the machine it happens to be running
    /// on would pass or fail by what else has a browser open.
    #[must_use]
    pub fn on_a_card_with(free: u64, per_slot: u64) -> Self {
        let card = Card { free, total: free };
        Self {
            reading: Arc::new(move || Some(card)),
            pinned_price: Some(per_slot),
            held: Arc::default(),
        }
    }

    /// How many slots a load of `load` opens, given the ladder's
    /// `tools.parallel_agents`. `replacing` says whether a model this pool
    /// weighed is resident and makes way for it.
    ///
    /// The conversation's own slot goes in as already open: it is the model
    /// being loaded rather than a sub-agent, and a budget that could refuse it
    /// would be a card with no room answering the question by unloading the
    /// chat. A slot `load` does not price (`Load::context` of `None`) is
    /// unmeasured.
    ///
    /// A card that cannot be read is a card with nothing to spare, and the
    /// slots already open stay open: this never evicts and never closes, so an
    /// unreadable card costs the conversation nothing it already has.
    #[must_use]
    pub fn admit(&self, load: Load, replacing: bool, wanted: u32) -> Admission {
        let card = (self.reading)();
        let (free, per_slot) = match self.pinned_price {
            Some(per_slot) => (card.map_or(0, |card| card.free), per_slot),
            None => {
                let per_slot = load.context.unwrap_or(0);
                let given_back = if replacing { self.held() } else { 0 };
                let beside = card.map_or(0, |card| {
                    // What the resident model gives back can never make the
                    // card larger than it is.
                    card.free
                        .saturating_add(given_back)
                        .min(card.total)
                        .saturating_sub(load.weights)
                        .saturating_sub(per_slot)
                        .saturating_sub(BESIDE_THE_KV)
                });
                (beside, per_slot)
            }
        };
        admit(Budget {
            free,
            per_slot,
            open: 1,
            wanted,
        })
    }

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
            .field("pinned_price", &self.pinned_price)
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

    /// The rig's card, an RTX 3060 with 12 GB, as NVML read it with nothing
    /// loaded on #105.
    const TOTAL: u64 = 12288 * MIB;
    const IDLE: u64 = 10511 * MIB;

    /// The development model as a load prices it: its file, and one slot at
    /// 32k read from its header (`demido_models::slot`).
    const DEVELOPMENT: Load = Load {
        weights: 7813 * MIB,
        context: Some(552 * MIB),
    };

    /// The reference model the same way.
    const REFERENCE: Load = Load {
        weights: 9551 * MIB,
        context: Some(940 * MIB),
    };

    /// What a scripted backend loads: nothing a pool could price.
    const SCRIPTED: Load = Load {
        weights: 0,
        context: None,
    };

    /// A real-card pool over a card that always reads `free`.
    fn reading(free: u64) -> Pool {
        Pool {
            reading: Arc::new(move || Some(Card { free, total: TOTAL })),
            pinned_price: None,
            held: Arc::default(),
        }
    }

    /// The load is priced whole against the reading taken before it lands.
    /// The development model leaves 1881 MiB beside its weights, its own slot
    /// and what the build holds beside them, which is three slots more at 552:
    /// asked for five, four open.
    #[test]
    fn the_development_model_is_priced_whole_before_it_lands() {
        let admission = reading(IDLE).admit(DEVELOPMENT, false, 5);

        assert_eq!(admission.open, 4);
        assert_eq!(admission.queued, 1);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 1881 * MIB,
                needed: 552 * MIB,
            })
        );
    }

    /// The reference model at 32k on the same card leaves nothing beside its
    /// load, against 940 a second slot costs. The whole card read before the
    /// load would have admitted ten; that is the failure pricing it whole is
    /// for.
    #[test]
    fn the_reference_model_has_no_room_once_its_weights_are_counted() {
        let admission = reading(IDLE).admit(REFERENCE, false, 2);

        assert_eq!(admission.open, 1, "the conversation's own slot stays");
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 0,
                needed: 940 * MIB,
            })
        );
    }

    /// The reference model at the default 4k: a slot is 380 MiB, and the card
    /// read before the load has 580 beside the weights and the first slot, but
    /// 315 once the compute buffers and the CUDA context are counted. Weighed,
    /// one slot of it held 10181 MiB, so a second would have overrun a card
    /// reading 10511 by 50.
    #[test]
    fn what_the_build_holds_beside_the_kv_is_counted() {
        let at_4k = Load {
            weights: 9551 * MIB,
            context: Some(380 * MIB),
        };
        let admission = reading(IDLE).admit(at_4k, false, 2);

        assert_eq!(admission.open, 1);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 315 * MIB,
                needed: 380 * MIB,
            })
        );
        assert_eq!(
            reading(11046 * MIB).admit(at_4k, false, 2).open,
            2,
            "with 535 MiB more free, as the card read when it was weighed, it fits"
        );
    }

    /// A load that replaces the resident model gets back what that model was
    /// weighed at holding, so a card that looks full because Demido's own
    /// model is on it is not full for the load that replaces it.
    #[test]
    fn a_replacement_counts_what_the_resident_model_gives_back() {
        let pool = reading(4804 * MIB);
        pool.held.store(5707 * MIB, Ordering::SeqCst);

        assert_eq!(pool.admit(DEVELOPMENT, true, 4).open, 4);
        assert_eq!(
            pool.admit(DEVELOPMENT, false, 4).open,
            1,
            "a model nothing weighed gives nothing back"
        );
    }

    /// Given back is never more than the card holds.
    #[test]
    fn nothing_given_back_makes_the_card_larger_than_it_is() {
        let pool = reading(IDLE);
        pool.held.store(TOTAL, Ordering::SeqCst);
        let admission = pool.admit(REFERENCE, true, 3);

        assert_eq!(admission.open, 2, "1532 MiB beside the load is one slot");
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: (12288 - 9551 - 940 - 265) * MIB,
                needed: 940 * MIB,
            })
        );
    }

    /// A model whose header this build cannot price queues every slot above the
    /// first as unmeasured, however much is free.
    #[test]
    fn an_unpriced_slot_queues_on_the_emptiest_card() {
        let load = Load {
            weights: 5000 * MIB,
            context: None,
        };
        let admission = reading(TOTAL).admit(load, false, 4);

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 3);
        assert_eq!(admission.reason, Some(Queued::Unmeasured));
    }

    /// The default asks for nothing above the conversation's own slot, so it
    /// is admitted on a card with no room at all, whatever the load costs.
    #[test]
    fn the_default_opens_the_one_slot_the_conversation_already_has() {
        let admission = reading(0).admit(REFERENCE, false, 1);

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 0);
        assert_eq!(admission.reason, None);
    }

    /// A card nothing could read is a card with nothing to spare, and the slot
    /// the conversation already has is untouched. Never an eviction.
    #[test]
    fn an_unreadable_card_opens_nothing_new_and_closes_nothing() {
        let pool = Pool {
            reading: Arc::new(|| None),
            pinned_price: None,
            held: Arc::default(),
        };
        let admission = pool.admit(DEVELOPMENT, false, 3);

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 2);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 0,
                needed: 552 * MIB,
            })
        );
    }

    /// A suite's card answers the whole question: the rig's reference model at
    /// 32k leaves 423 MiB and a slot costs 1129, so two queue to one. The load
    /// is not consulted, because a scripted backend has none.
    #[test]
    fn a_pinned_card_answers_for_the_load() {
        let pool = Pool::on_a_card_with(423 * MIB, 1129 * MIB);
        let admission = pool.admit(SCRIPTED, false, 2);

        assert_eq!(admission.open, 1);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free: 423 * MIB,
                needed: 1129 * MIB,
            })
        );
        let roomy = Pool::on_a_card_with(5583 * MIB, 563 * MIB);
        assert_eq!(roomy.admit(SCRIPTED, false, 4).open, 4);
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
                // Room for a second slot the first time, none after it.
                let first = counted.fetch_add(1, Ordering::SeqCst) == 0;
                Some(Card {
                    free: if first { IDLE } else { 7813 * MIB },
                    total: TOTAL,
                })
            }),
            pinned_price: None,
            held: Arc::default(),
        };

        assert_eq!(pool.admit(DEVELOPMENT, false, 2).open, 2);
        assert_eq!(
            pool.admit(DEVELOPMENT, false, 2).open,
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
                    total: TOTAL,
                })
            }),
            pinned_price: None,
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
            pinned_price: None,
            held: Arc::new(AtomicU64::new(5749 * MIB)),
        };
        pool.weigh(None, true);
        assert_eq!(pool.held(), 0);
    }
}
