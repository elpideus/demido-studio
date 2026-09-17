//! Parallelism as a budget rather than as a preference.
//!
//! A sub-agent runs the conversation's **own weights on a second `llama.cpp`
//! slot**, never a second model, so the process-wide rule that one model is
//! resident still holds. What a second slot costs is therefore not a model: it
//! is a KV reservation, and `--ctx-size` is per slot
//! ([#19](https://github.com/elpideus/demido-studio/issues/19)), so parallelism
//! multiplies it.
//!
//! ## The whole crate is two functions that do not know about each other
//!
//! [`admit`] is **pure**: free bytes in, a decision out, no card involved. That
//! is the point of it. The number it reasons about is machine dependent, and a
//! rule about a machine dependent number is otherwise only testable on the
//! machine it was written on: the table in this file asserts the rig's own
//! measurements without a rig.
//!
//! [`free_now`] is the **real reading**, and it is a plain function beside the
//! arithmetic rather than a field inside it, because *the card's free memory is
//! not what a settings page saw when it was drawn*. Anything that opens a slot
//! reads again at the moment it opens one.
//!
//! ## What it will not do
//!
//! **Either the slot's KV is shown in the context arithmetic, or the slot is
//! not opened.** `demido_inference::llamacpp::arguments` multiplies the context
//! length by the slot count, so a slot that is opened is a slot that was paid
//! for, and the number the user sees is the number they get.
//!
//! **A parallelism the card cannot honour degrades to a queue, with the reason
//! stated.** Never to an eviction, and never to a failed load. Nothing here
//! frees anything, and nothing here can: a driver asked for memory it does not
//! have often obliges by paging tensors through host memory, and the result is
//! not an error but an app that has become twenty times slower for a reason no
//! log explains. The accounting happens before the allocation rather than
//! around it.
//!
//! **It is a module, not a trait.** A `Scheduler` trait would buy a second
//! implementation that exists only in tests, and
//! [`docs/rules/tiles.md`](../../../../docs/rules/tiles.md) is explicit that a
//! trait is a decision rather than a default.

mod reading;

pub use reading::{free_now, Card};

use serde::Serialize;

/// One mebibyte. Every figure the rig records is in MiB, so the tests read like
/// the table they came from.
pub const MIB: u64 = 1024 * 1024;

/// What there is to pay with, what a slot costs, and how many are asked for.
///
/// Four numbers and no card, which is what makes [`admit`] testable without
/// one. Where each comes from:
///
/// - `free` is [`free_now`], read at the moment a slot is about to open.
/// - `per_slot` is what one slot's KV reserves at the context length in force.
///   Measured, never assumed: zero means nothing has measured one yet, and that
///   is a stated reason rather than a free slot (see [`Queued::Unmeasured`]).
/// - `open` is the slots already open, whose KV is already out of `free`.
/// - `wanted` is the ladder's `tools.parallel_agents`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Bytes the card reports free, now.
    pub free: u64,
    /// Bytes one more slot reserves.
    pub per_slot: u64,
    /// Slots already open.
    pub open: u32,
    /// Slots the user asked for.
    pub wanted: u32,
}

/// What may be opened, what may not, and why not.
///
/// One value rather than a verdict per slot, because the slot count is a flag
/// on a process: `llama.cpp` is started with the slots it will ever have, so
/// the question is always *how many of these*, never *may I have one more* a
/// slot at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Admission {
    /// Slots open once this is applied. Never fewer than
    /// [`Budget::open`], never more than [`Budget::wanted`].
    ///
    /// **This is the number shown to the user**, and the number the server is
    /// started with. A preference the card could not honour is not shown back
    /// as though it had been.
    pub open: u32,
    /// Slots asked for and not opened. Work for them waits its turn.
    pub queued: u32,
    /// Why anything queued. `None` when everything asked for fits.
    pub reason: Option<Queued>,
    /// What the slots opened by this admission reserve, on top of what the
    /// already-open ones hold.
    pub reserved: u64,
    /// What the card is left with once they have.
    pub free_after: u64,
}

/// Why a slot was not opened.
///
/// A variant rather than a sentence, the way `demido-hardware` reports: the
/// window owns the words, and the monitor's slot strip
/// ([#68](https://github.com/elpideus/demido-studio/issues/68)) is what draws
/// them. The numbers travel with it because "there is no room" and "there is no
/// room, 423 MiB against 1129" are different messages to somebody deciding
/// whether to shorten their context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "queued", rename_all = "kebab-case")]
pub enum Queued {
    /// The card has less free than one more slot reserves.
    NoRoom {
        /// What was free when it was asked.
        free: u64,
        /// What one slot needed.
        needed: u64,
    },
    /// Nothing has measured what a slot costs on this model at this context
    /// length, so there is no arithmetic to admit one on.
    ///
    /// Refusing rather than guessing, for the reason at the top of this file: a
    /// slot admitted on a number nobody measured is an allocation failure
    /// inside the driver, halfway through a load, with a message written for a
    /// CUDA programmer. The fit verdict
    /// ([#74](https://github.com/elpideus/demido-studio/issues/74)) is what
    /// will produce the number; until it does, parallelism above the slots
    /// already open queues and says so.
    Unmeasured,
}

/// How many of the slots asked for may be opened.
///
/// Pure, total, and saturating throughout: an absurd context length in a
/// settings file has to refuse a slot, never wrap around into a small number
/// that admits one.
#[must_use]
pub fn admit(budget: Budget) -> Admission {
    let Budget {
        free,
        per_slot,
        open,
        wanted,
    } = budget;

    // Never fewer than are already open. Admission opens slots; it does not
    // close them, and a preference smaller than the running server has is a
    // reload rather than an eviction, so the answer here is the slots that are
    // open rather than the smaller number that was asked for.
    let asked = wanted.saturating_sub(open);
    if asked == 0 {
        return Admission {
            open,
            queued: 0,
            reason: None,
            reserved: 0,
            free_after: free,
        };
    }

    if per_slot == 0 {
        return Admission {
            open,
            queued: asked,
            reason: Some(Queued::Unmeasured),
            reserved: 0,
            free_after: free,
        };
    }

    // Integer division is the admission: a slot whose KV is half paid for is a
    // slot that does not open.
    let room = u32::try_from(free / per_slot).unwrap_or(u32::MAX);
    let granted = asked.min(room);
    let reserved = u64::from(granted).saturating_mul(per_slot);

    Admission {
        open: open.saturating_add(granted),
        queued: asked - granted,
        reason: (granted < asked).then_some(Queued::NoRoom {
            free,
            needed: per_slot,
        }),
        reserved,
        free_after: free - reserved,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    /// The rig's card, from `docs/rules/done.md`: an RTX 3060 with 12 GB.
    const CARD: u64 = 12288 * MIB;

    /// What the development model holds at 32k, idle desktop included, and what
    /// one more of its slots reserves. `docs/rules/done.md`, "What the card
    /// holds".
    const DEVELOPMENT_AT_32K: u64 = 6705 * MIB;
    const DEVELOPMENT_PER_SLOT: u64 = 563 * MIB;

    /// The same slot, weighed rather than derived: `done.md` also records a
    /// second slot on the development model at 32k costing this, read off NVML
    /// around three states of the pinned build instead of subtracted out of a
    /// total. The table below asserts both, because a rule that answered
    /// differently depending on which of two readings of one slot it was handed
    /// would be a rule about the readings.
    const DEVELOPMENT_WEIGHED: u64 = 620 * MIB;

    /// The same two for the reference model, which is the one that decides what
    /// may ship as a default: a value the reference gate cannot run at is not a
    /// default.
    const REFERENCE_AT_32K: u64 = 11865 * MIB;
    const REFERENCE_PER_SLOT: u64 = 1129 * MIB;

    fn budget(free: u64, per_slot: u64, open: u32, wanted: u32) -> Budget {
        Budget {
            free,
            per_slot,
            open,
            wanted,
        }
    }

    /// The whole reason this is a pure function: the rig's own numbers, decided
    /// without the rig.
    ///
    /// Each row is one model resident at 32k on the 12 GB card, with its one
    /// slot already open, being asked for a second. The development model has
    /// room for nine more; the reference model has room for none, which is why
    /// `tools.parallel_agents` defaults to 1.
    #[test]
    fn the_rig_is_decided_without_the_rig() {
        struct Case {
            what: &'static str,
            free: u64,
            per_slot: u64,
            wanted: u32,
            open: u32,
            queued: u32,
        }

        let table = [
            Case {
                what: "the development model has room for a second slot",
                free: CARD - DEVELOPMENT_AT_32K,
                per_slot: DEVELOPMENT_PER_SLOT,
                wanted: 2,
                open: 2,
                queued: 0,
            },
            Case {
                what: "and for nine more, which is what 5583 over 563 leaves",
                free: CARD - DEVELOPMENT_AT_32K,
                per_slot: DEVELOPMENT_PER_SLOT,
                wanted: 20,
                open: 10,
                queued: 10,
            },
            Case {
                what: "and the weighed figure decides the same, which is the point",
                free: CARD - DEVELOPMENT_AT_32K,
                per_slot: DEVELOPMENT_WEIGHED,
                wanted: 2,
                open: 2,
                queued: 0,
            },
            Case {
                what: "the reference model has room for none: 423 against 1129",
                free: CARD - REFERENCE_AT_32K,
                per_slot: REFERENCE_PER_SLOT,
                wanted: 2,
                open: 1,
                queued: 1,
            },
            Case {
                what: "which is why one is the default: it needs no room at all",
                free: CARD - REFERENCE_AT_32K,
                per_slot: REFERENCE_PER_SLOT,
                wanted: 1,
                open: 1,
                queued: 0,
            },
            Case {
                what: "exactly zero headroom is a slot that opens",
                free: REFERENCE_PER_SLOT,
                per_slot: REFERENCE_PER_SLOT,
                wanted: 2,
                open: 2,
                queued: 0,
            },
            Case {
                what: "and one byte under it is a slot that does not",
                free: REFERENCE_PER_SLOT - 1,
                per_slot: REFERENCE_PER_SLOT,
                wanted: 2,
                open: 1,
                queued: 1,
            },
        ];

        for case in table {
            let admission = admit(budget(case.free, case.per_slot, 1, case.wanted));
            assert_eq!(admission.open, case.open, "{}", case.what);
            assert_eq!(admission.queued, case.queued, "{}", case.what);
            assert_eq!(
                admission.reason.is_some(),
                case.queued > 0,
                "a slot that does not open says why: {}",
                case.what
            );
        }
    }

    /// Exactly zero headroom, in full: the slot opens, and it opens on the last
    /// byte. The boundary is asserted on both sides above; this is the one that
    /// says the arithmetic left nothing behind.
    #[test]
    fn a_slot_that_fits_exactly_leaves_nothing() {
        let admission = admit(budget(REFERENCE_PER_SLOT, REFERENCE_PER_SLOT, 1, 2));

        assert_eq!(admission.open, 2);
        assert_eq!(admission.reserved, REFERENCE_PER_SLOT);
        assert_eq!(admission.free_after, 0);
        assert_eq!(admission.reason, None);
    }

    /// A slot that does not fit queues, with the two numbers a person needs to
    /// decide what to do about it. It never evicts, and there is nothing here
    /// that could: `admit` returns a decision and frees nothing.
    #[test]
    fn a_slot_that_does_not_fit_queues_and_says_what_it_needed() {
        let free = CARD - REFERENCE_AT_32K;
        let admission = admit(budget(free, REFERENCE_PER_SLOT, 1, 3));

        assert_eq!(admission.open, 1, "the open slot stays open and no other");
        assert_eq!(admission.queued, 2);
        assert_eq!(
            admission.reason,
            Some(Queued::NoRoom {
                free,
                needed: REFERENCE_PER_SLOT,
            })
        );
        assert_eq!(admission.reserved, 0);
        assert_eq!(admission.free_after, free, "nothing was taken");
    }

    /// Nobody has measured a slot, so nobody admits one. The refusal is
    /// distinct from having no room, because the two lead to different
    /// answers: one is shortened by a smaller context and the other is not
    /// answerable by the user at all.
    #[test]
    fn an_unmeasured_slot_queues_rather_than_being_guessed_at() {
        let admission = admit(budget(CARD, 0, 1, 4));

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 3);
        assert_eq!(admission.reason, Some(Queued::Unmeasured));
    }

    /// The default, on every machine and every model: one slot, asked for by a
    /// conversation that already has it. Nothing is admitted, nothing queues,
    /// and no card is consulted, which is what makes the synchronous path
    /// independent of all of this.
    #[test]
    fn the_default_needs_no_room() {
        let admission = admit(budget(0, 0, 1, 1));

        assert_eq!(admission.open, 1);
        assert_eq!(admission.queued, 0);
        assert_eq!(admission.reason, None);
    }

    /// A parallelism lowered below what is running is a reload, not an
    /// eviction. Admission never closes a slot, so the answer is the slots
    /// that are open.
    #[test]
    fn lowering_the_setting_evicts_nothing() {
        let admission = admit(budget(CARD, DEVELOPMENT_PER_SLOT, 4, 1));

        assert_eq!(admission.open, 4);
        assert_eq!(admission.queued, 0);
        assert_eq!(admission.reserved, 0);
    }

    /// An absurd number in a settings file refuses slots rather than wrapping
    /// into a small one that admits them.
    #[test]
    fn the_arithmetic_saturates_rather_than_wrapping() {
        let admission = admit(budget(u64::MAX, 1, 0, u32::MAX));
        assert_eq!(admission.open, u32::MAX);

        let absurd = admit(budget(1, u64::MAX, 0, 8));
        assert_eq!(absurd.open, 0);
        assert_eq!(absurd.queued, 8);
    }
}
