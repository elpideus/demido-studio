//! What a piece of text costs, and who is allowed to say so.
//!
//! `design/windows.md` puts cost on the monitor's second axis and requires the
//! number per event rather than per request. The number a tokeniser gives is
//! the right one, and there is no tokeniser in this process: the weights are in
//! the GGUF the backend loaded, and asking it costs a round trip.
//!
//! So this is a seam rather than a function. [`Estimate`] is what the app uses
//! when nothing better is available, marked [`Basis::Estimated`] on every event
//! it produces so a bar built out of it can say what it is. A [`Weigher`] that
//! asks the running backend's own tokeniser is one line at the composition
//! root, and the live suite already uses one, which is how the counted path is
//! held to the same shape as the estimated one.

use crate::event::{Basis, Weight};

/// Something that can say what a string costs.
///
/// Implementations must be safe to share: one session records from wherever a
/// turn is being assembled, and the weigher is held for the life of it.
pub trait Weigher: Send + Sync {
    fn weigh(&self, text: &str) -> Weight;
}

/// The weigher that needs nobody.
///
/// Four bytes to a token for everything that is not an ideograph, and one token
/// per ideograph. The second half is not a refinement for its own sake: this
/// app ships three wenyan paragraphs whose entire purpose is that Classical
/// Chinese says the same thing in fewer tokens, and a rule of four bytes would
/// price them at three quarters of what they cost and report the saving as
/// larger than it is. The one number the caveman selector exists to move is the
/// one number a naive estimator gets most wrong.
///
/// It is an estimate and says so. Measured against the pinned build's own
/// tokeniser on English prose it lands within a token or two per sentence,
/// which is enough for a bar and not enough for an invoice.
#[derive(Debug, Clone, Copy, Default)]
pub struct Estimate;

impl Weigher for Estimate {
    fn weigh(&self, text: &str) -> Weight {
        Weight::estimated(estimate(text))
    }
}

/// The estimate itself, so a caller that wants the number without a weigher can
/// have it and there is only one rule.
pub fn estimate(text: &str) -> u32 {
    let mut ideographs: u32 = 0;
    let mut bytes: u32 = 0;

    for character in text.chars() {
        if is_ideograph(character) {
            ideographs = ideographs.saturating_add(1);
        } else {
            bytes = bytes.saturating_add(character.len_utf8() as u32);
        }
    }

    // Rounded up, because a fragment of three bytes is still a token, and a
    // ledger of rows that each round to nothing is a ledger that says a session
    // is free.
    ideographs.saturating_add(bytes.div_ceil(4))
}

/// The CJK ranges a wenyan paragraph is written in.
///
/// Deliberately narrow: the unified ideographs, their first extension, and the
/// compatibility block. Kana and Hangul are not here, because they tokenise
/// closer to the byte rule than to one token a character.
fn is_ideograph(character: char) -> bool {
    matches!(character,
        '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}')
}

/// A weigher that was told the answer. What a backend's own tokeniser becomes.
///
/// It exists so that the counted path has a shape callers can hold, rather than
/// each caller inventing one: anything that can count tokens is a closure, and
/// this is the closure wearing the trait.
pub struct Counting<F>(pub F);

impl<F> Weigher for Counting<F>
where
    F: Fn(&str) -> u32 + Send + Sync,
{
    fn weigh(&self, text: &str) -> Weight {
        Weight {
            tokens: (self.0)(text),
            basis: Basis::Counted,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
    // A test asserts by panicking; the workspace denial is about application
    // code, where a panic is a window that vanishes.

    use super::*;

    #[test]
    fn an_estimate_is_marked_as_one() {
        assert_eq!(Estimate.weigh("hello").basis, Basis::Estimated);
    }

    #[test]
    fn nothing_costs_nothing() {
        assert_eq!(estimate(""), 0);
    }

    #[test]
    fn a_short_fragment_still_costs_a_token() {
        // Rounding down here is how a ledger of many small rows reports a
        // session as free.
        assert_eq!(estimate("a"), 1);
    }

    #[test]
    fn english_prose_lands_near_the_tokeniser() {
        // Counted on the pinned build's own tokeniser at 10 tokens.
        let sentence = "You are terse. Answer in one short sentence.";
        let estimated = estimate(sentence);
        assert!(
            (8..=13).contains(&estimated),
            "estimated {estimated} against a counted 10"
        );
    }

    #[test]
    fn an_ideograph_is_a_token_and_not_three_quarters_of_one() {
        // The wenyan case, which is the one the byte rule gets most wrong and
        // the one this app ships three paragraphs for.
        let wenyan = "答簡";
        assert_eq!(wenyan.len(), 6, "six bytes of UTF-8");
        assert_eq!(
            estimate(wenyan),
            2,
            "the byte rule would price two characters at two tokens by accident \
             and a longer line at three quarters of what it costs"
        );
        assert_eq!(estimate("答簡言直"), 4);
    }

    #[test]
    fn a_counting_weigher_says_it_counted() {
        let weigher = Counting(|text: &str| text.split_whitespace().count() as u32);
        let weight = weigher.weigh("one two three");
        assert_eq!(weight.tokens, 3);
        assert_eq!(weight.basis, Basis::Counted);
    }
}
