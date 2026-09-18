//! Whether this card can hold this model, answered before anything is fetched.
//!
//! Brief B22: "Models Browser & Downloader"
//!
//! The detail pane's verdict
//! ([#74](https://github.com/elpideus/demido-studio/issues/74)). A person with
//! a 12 GB card choosing a 17 GB file should learn about partial offload from
//! the browser rather than from a slow first token.
//!
//! **It is the pool's arithmetic, not a second copy of it.** Loading a model is
//! priced the way a slot is: one reservation of known size against what the
//! card has free. So [`verdict`] asks [`crate::admit`] for one slot costing the
//! whole load, and an answer of *no room* is partial offload. A rule about
//! when a number fits lives in one function, and the two consumers cannot
//! drift apart on the boundary.
//!
//! **It is the shape of `docs/rules/done.md`'s table.** A row there is the
//! weights, then the total once a context is on top of them, with the idle
//! desktop inside the total. Here the desktop is already out of the card's
//! free reading, the weights are the number the server states, and the context
//! goes on top when something has priced it. The attention geometry a KV cache
//! is sized from lives in the file's header, so a model on disk has its context
//! priced by the same reading that prices a slot for the pool
//! ([#105](https://github.com/elpideus/demido-studio/issues/105)), and a file
//! not yet downloaded does not: its header is not fetched to draw a pane. The
//! verdict says which it is, rather than adding a figure nobody measured
//! ([`Load::context`]).
//!
//! **It informs, it never decides.** A verdict is a value the window writes a
//! sentence from. Nothing here refuses a download, and nothing that queues one
//! takes a verdict as an argument: the machine is the person's.

use serde::Serialize;

use crate::{admit, Budget, Card, Queued};

/// What loading one model costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    /// The weights, as the server states their size: every piece of a split
    /// model added up, and nothing that is not weights. A projector is only
    /// resident when an image is being read, which this build does not do.
    ///
    /// A file is not always what lands on the card: the rig's development
    /// model keeps its per-layer embeddings in system memory, so its 7813 MiB
    /// file is 4942 MiB of weights on the card. Before a download the file is
    /// all there is to price, and it errs towards *partial offload*, which a
    /// person can act on, never towards a *fits* the load disproves.
    pub weights: u64,
    /// What a context of the length in force reserves on top of the weights:
    /// one slot's KV, read from the model's header (`demido_models::slot`),
    /// and the compute buffers and CUDA context beside it, which no header
    /// states and which are weighed instead ([`crate::BESIDE_THE_KV`]).
    ///
    /// `None` is **not priced**: a file not on disk yet, or an architecture
    /// the reading does not know. Which is not the same as free. The verdict
    /// carries the difference to the window, so a model whose weights fit
    /// with a sliver to spare is not shown as fitting without saying the
    /// context is still to come.
    pub context: Option<u64>,
}

/// Whether the card holds the load.
///
/// Every variant carries its figures, because *"partial offload"* and *"1.7 GB
/// over the 10.8 GB this card has free"* are different messages to somebody
/// choosing between two quantisations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "fit",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Verdict {
    /// Every layer fits on the card.
    Fits {
        /// What the load costs.
        needs: u64,
        /// What there is to pay with.
        room: u64,
        /// What the card is left with once it has.
        spare: u64,
        /// Whether `needs` includes the context.
        context_priced: bool,
    },
    /// The load is larger than the room, so `llama.cpp` puts what fits on the
    /// card and runs the rest from system memory, more slowly.
    Partial {
        needs: u64,
        room: u64,
        /// How much does not fit.
        over: u64,
        context_priced: bool,
    },
    /// The card could not be read: no NVIDIA driver, or one that refused.
    /// Nobody asked the card is not the same fact as the card is full.
    Unread { needs: u64 },
    /// The server stated no size for the weights, so there is nothing to price.
    Unpriced,
}

/// Whether `card` can hold `load`.
///
/// `replacing` is what the model Demido already has resident holds, which a
/// load gives back: one model is resident at a time, so choosing another one
/// unloads it, and a card that looks full because Demido's own model is on it
/// is not full for the model that would replace it. Zero when nothing is
/// resident, and [`weighed`] when something is.
///
/// Pure: the card is read by the caller, at the moment it asks
/// ([`crate::free_now`]), and never carried from an earlier drawing.
#[must_use]
pub fn verdict(card: Option<Card>, replacing: u64, load: Load) -> Verdict {
    let context_priced = load.context.is_some();
    let needs = load.weights.saturating_add(load.context.unwrap_or(0));
    if load.weights == 0 {
        return Verdict::Unpriced;
    }
    let Some(card) = card else {
        return Verdict::Unread { needs };
    };
    // What the resident model gives back can never make the card larger than
    // it is.
    let room = card.free.saturating_add(replacing).min(card.total);

    // One slot, costing the whole load, with nothing open yet.
    let admission = admit(Budget {
        free: room,
        per_slot: needs,
        open: 0,
        wanted: 1,
    });
    match admission.reason {
        None => Verdict::Fits {
            needs,
            room,
            spare: admission.free_after,
            context_priced,
        },
        Some(Queued::NoRoom { .. }) => Verdict::Partial {
            needs,
            room,
            over: needs - room,
            context_priced,
        },
        // `needs` is at least the weights, which are not zero here.
        Some(Queued::Unmeasured) => Verdict::Unpriced,
    }
}

/// What a server that just started holds on the card, **weighed rather than
/// derived**: the free reading before it started, plus what the server it
/// replaced was weighed at (zero if none), less the reading once it answered.
///
/// This is how `docs/rules/done.md` weighed a second slot, and the reason it is
/// not the model file's size: a file is not what lands on the card. The rig's
/// development model is a 7813 MiB file of which 4942 are weights on the card,
/// because its per-layer embeddings stay in system memory, and its context is
/// on the card and in no file at all.
///
/// `None` when either reading is missing: nobody weighed it.
#[must_use]
pub fn weighed(before: Option<Card>, given_back: u64, after: Option<Card>) -> Option<u64> {
    let (before, after) = (before?, after?);
    Some(
        before
            .free
            .saturating_add(given_back)
            .saturating_sub(after.free),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;
    use crate::MIB;

    /// The rig's card, from `docs/rules/done.md`: an RTX 3060 with 12 GB.
    const CARD: u64 = 12288 * MIB;

    /// The idle desktop `done.md` counts inside every total, "about 1.2 GB".
    /// Its exact figure cancels out of every row below, which is the point of
    /// pricing against a free reading: the desktop is already out of it.
    const DESKTOP: u64 = 1229 * MIB;

    /// A row of `done.md`'s "What the card holds": the weights, and the total
    /// once a context is on top of them, idle desktop included.
    struct Row {
        what: &'static str,
        weights: u64,
        total: u64,
    }

    const DEVELOPMENT_AT_32K: Row = Row {
        what: "development, gemma-4-E4B-it Q8_0, at 32k",
        weights: 4942 * MIB,
        total: 6705 * MIB,
    };
    const REFERENCE_AT_32K: Row = Row {
        what: "reference, gemma-4-26B-A4B-it-UD IQ2_M, at 32k",
        weights: 9536 * MIB,
        total: 11865 * MIB,
    };
    const BREADTH_AT_32K: Row = Row {
        what: "breadth, Qwen3.8-27B IQ2_M, at 32k",
        weights: 9010 * MIB,
        total: 12017 * MIB,
    };
    const SECONDARY_AT_4K: Row = Row {
        what: "secondary, Qwen3.5-9B Q4_K_M, at 4k",
        weights: 4861 * MIB,
        total: 6474 * MIB,
    };

    /// The card as it reads with nothing of Demido's on it.
    fn idle() -> Option<Card> {
        Some(Card {
            free: CARD - DESKTOP,
            total: CARD,
        })
    }

    /// A row's load, its context being what the total holds past the weights
    /// and the desktop.
    fn load(row: &Row) -> Load {
        Load {
            weights: row.weights,
            context: Some(row.total - row.weights - DESKTOP),
        }
    }

    /// The whole reason this is a pure function: every measured row of the
    /// rig's table, decided without the rig, leaves exactly what `done.md`
    /// says it leaves.
    #[test]
    fn the_rig_is_decided_without_the_rig() {
        for row in [
            DEVELOPMENT_AT_32K,
            REFERENCE_AT_32K,
            BREADTH_AT_32K,
            SECONDARY_AT_4K,
        ] {
            match verdict(idle(), 0, load(&row)) {
                Verdict::Fits {
                    spare,
                    context_priced,
                    ..
                } => {
                    assert_eq!(spare, CARD - row.total, "{}", row.what);
                    assert!(context_priced, "{}", row.what);
                }
                other => panic!("{} fits on the rig, and was {other:?}", row.what),
            }
        }
    }

    /// `done.md`'s tightest row, said out loud: breadth at 32k leaves 271 MiB,
    /// "less than one browser window". One browser window later it does not
    /// fit, which is why the card is read when the pane asks rather than once.
    #[test]
    fn breadth_at_32k_fits_by_271_and_a_browser_window_later_does_not() {
        let fits = verdict(idle(), 0, load(&BREADTH_AT_32K));
        assert!(matches!(fits, Verdict::Fits { spare, .. } if spare == 271 * MIB));

        let browser = 400 * MIB;
        let later = Some(Card {
            free: CARD - DESKTOP - browser,
            total: CARD,
        });
        assert_eq!(
            verdict(later, 0, load(&BREADTH_AT_32K)),
            Verdict::Partial {
                needs: BREADTH_AT_32K.total - DESKTOP,
                room: CARD - DESKTOP - browser,
                over: browser - 271 * MIB,
                context_priced: true,
            }
        );
    }

    /// The board's own example: a 17.74 GB file on a 16 GB card is partial
    /// offload, and says by how much.
    #[test]
    fn a_file_larger_than_the_card_is_partial_offload_by_the_difference() {
        let card = Some(Card {
            free: 16_000_000_000,
            total: 16_000_000_000,
        });
        let file = Load {
            weights: 17_740_000_000,
            context: None,
        };

        assert_eq!(
            verdict(card, 0, file),
            Verdict::Partial {
                needs: 17_740_000_000,
                room: 16_000_000_000,
                over: 1_740_000_000,
                context_priced: false,
            }
        );
    }

    /// Exactly the room is every layer on the card; one byte more is not. The
    /// boundary is `admit`'s, asserted here so that a change to it is seen by
    /// both of its consumers.
    #[test]
    fn exactly_the_room_fits_and_one_byte_more_does_not() {
        let card = Some(Card {
            free: 8 * 1024 * MIB,
            total: CARD,
        });
        let exactly = Load {
            weights: 8 * 1024 * MIB,
            context: None,
        };
        assert!(matches!(
            verdict(card, 0, exactly),
            Verdict::Fits { spare: 0, .. }
        ));

        let over = Load {
            weights: 8 * 1024 * MIB + 1,
            context: None,
        };
        assert!(matches!(
            verdict(card, 0, over),
            Verdict::Partial { over: 1, .. }
        ));
    }

    /// A context nobody priced is carried as unpriced, never as free: the
    /// weights alone fit, and the verdict says that is all it knows.
    #[test]
    fn an_unpriced_context_is_said_rather_than_counted_as_nothing() {
        let weights = Load {
            weights: REFERENCE_AT_32K.weights,
            context: None,
        };

        assert!(matches!(
            verdict(idle(), 0, weights),
            Verdict::Fits {
                context_priced: false,
                needs,
                ..
            } if needs == REFERENCE_AT_32K.weights
        ));
    }

    /// A card that looks full because Demido's own reference model is on it
    /// is not full for the model that would replace it.
    #[test]
    fn the_resident_model_is_room_for_the_one_that_replaces_it() {
        let with_reference_loaded = Some(Card {
            free: CARD - REFERENCE_AT_32K.total,
            total: CARD,
        });
        let development = load(&DEVELOPMENT_AT_32K);

        assert!(matches!(
            verdict(with_reference_loaded, 0, development),
            Verdict::Partial { .. }
        ));
        // What it was weighed at holding: its total, less the desktop that was
        // on the card before it loaded.
        let weighed_at = REFERENCE_AT_32K.total - DESKTOP;
        assert!(matches!(
            verdict(with_reference_loaded, weighed_at, development),
            Verdict::Fits { .. }
        ));
    }

    /// What is given back never makes the card larger than it is.
    #[test]
    fn the_room_is_never_more_than_the_card() {
        let card = Some(Card {
            free: CARD,
            total: CARD,
        });
        let everything_back = verdict(
            card,
            u64::MAX,
            Load {
                weights: 1,
                context: None,
            },
        );
        assert!(matches!(everything_back, Verdict::Fits { room: CARD, .. }));
    }

    /// A machine whose card cannot be read gets no verdict about the card,
    /// only about the load.
    #[test]
    fn an_unread_card_is_not_a_full_one() {
        assert_eq!(
            verdict(None, 0, load(&DEVELOPMENT_AT_32K)),
            Verdict::Unread {
                needs: DEVELOPMENT_AT_32K.total - DESKTOP
            }
        );
    }

    /// A file the server stated no size for is not a model that costs nothing.
    #[test]
    fn weights_of_no_stated_size_are_unpriced() {
        let nothing = Load {
            weights: 0,
            context: Some(563 * MIB),
        };
        assert_eq!(verdict(idle(), 0, nothing), Verdict::Unpriced);
    }

    /// An absurd context in a settings file saturates into a load that does
    /// not fit, never wraps into one that does.
    #[test]
    fn the_arithmetic_saturates_rather_than_wrapping() {
        let absurd = Load {
            weights: u64::MAX,
            context: Some(u64::MAX),
        };
        assert!(matches!(
            verdict(idle(), 0, absurd),
            Verdict::Partial {
                needs: u64::MAX,
                ..
            }
        ));
    }

    /// `done.md`'s weighed table, read back: nothing loaded, then the
    /// development model at 32k on one slot, then the same on two slots
    /// replacing it. Each cost is the table's own column.
    #[test]
    fn a_load_is_weighed_the_way_done_md_weighed_one() {
        let reading = |free: u64| {
            Some(Card {
                free: free * MIB,
                total: CARD,
            })
        };
        let one_slot = weighed(reading(7984), 0, reading(2235));
        assert_eq!(one_slot, Some(5749 * MIB));

        let two_slots = weighed(reading(2235), 5749 * MIB, reading(1615));
        assert_eq!(two_slots, Some(6369 * MIB));

        assert_eq!(weighed(None, 0, reading(1615)), None);
        assert_eq!(weighed(reading(1615), 0, None), None);
    }

    /// A card that freed memory while a load ran reads as the load holding
    /// nothing, never as a wrapped number that is most of the address space.
    #[test]
    fn a_weighing_saturates_rather_than_wrapping() {
        let before = Some(Card {
            free: MIB,
            total: CARD,
        });
        let after = Some(Card {
            free: 2 * MIB,
            total: CARD,
        });
        assert_eq!(weighed(before, 0, after), Some(0));
    }

    /// The shape the window reads.
    #[test]
    fn the_window_reads_a_tagged_value() {
        let wire = serde_json::to_value(verdict(idle(), 0, load(&DEVELOPMENT_AT_32K)))
            .expect("serialises");
        assert_eq!(wire["fit"], "fits");
        assert_eq!(wire["contextPriced"], true);
        assert_eq!(wire["spare"], (CARD - DEVELOPMENT_AT_32K.total));
    }
}
