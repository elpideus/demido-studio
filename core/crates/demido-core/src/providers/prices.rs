//! What a model costs, as its provider's page said on the day it was read (research note 18).
//!
//! OpenRouter says what each answer cost, so its models need no table. Anthropic and Google say how many
//! tokens an answer took; Demido works out the cost from these tables. Where a provider charges less for a
//! while (an introductory price), the table has the regular price: Demido's count is then above the bill,
//! never below it. A model that is not in the table is counted at the provider's highest price.

/// When the prices below were read from the providers' own pages.
pub const READ_ON: &str = "2026-11-21";

/// Dollars per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    /// What a token read from the provider's cache costs, as a share of `input`.
    pub cached: f64,
    /// A prompt longer than this many tokens costs `long` instead.
    pub long_after: Option<(u64, f64, f64)>,
}

const fn p(input: f64, output: f64, cached: f64) -> Price {
    Price { input, output, cached, long_after: None }
}

const fn long(input: f64, output: f64, cached: f64, after: u64, long_input: f64, long_output: f64) -> Price {
    Price { input, output, cached, long_after: Some((after, long_input, long_output)) }
}

/// Anthropic's models (from the Claude API's model list, read on `READ_ON`). A model's id may carry a date
/// after these names (`claude-haiku-4-5-20251001`); the longest name it starts with counts.
const ANTHROPIC: &[(&str, Price)] = &[
    ("claude-fable-5-2", p(8.0, 40.0, 0.025)),
    ("claude-mythos-5-2", p(8.0, 40.0, 0.025)),
    ("claude-fable-5-1", p(10.0, 50.0, 0.025)),
    ("claude-mythos-5-1", p(10.0, 50.0, 0.025)),
    ("claude-fable-5", p(10.0, 50.0, 0.1)),
    ("claude-mythos-5", p(10.0, 50.0, 0.1)),
    ("claude-opus-5-5", p(4.0, 20.0, 0.05)),
    ("claude-opus-5", p(5.0, 25.0, 0.1)),
    ("claude-opus-4-8", p(5.0, 25.0, 0.1)),
    ("claude-opus-4-7", p(5.0, 25.0, 0.1)),
    ("claude-opus-4-6", p(5.0, 25.0, 0.1)),
    ("claude-sonnet-5-5", p(2.0, 10.0, 0.05)),
    ("claude-sonnet-5", p(2.0, 10.0, 0.1)),
    ("claude-sonnet-4-6", p(3.0, 15.0, 0.1)),
    ("claude-haiku-5-5", long(0.10, 0.50, 0.1, 100_000, 0.50, 2.50)),
    ("claude-haiku-4-5", p(1.0, 5.0, 0.1)),
];
/// What a token written to Anthropic's cache costs, as a share of `input` (five minutes' keeping).
pub const ANTHROPIC_CACHE_WRITE: f64 = 1.25;
const ANTHROPIC_HIGHEST: Price = p(10.0, 50.0, 0.1);

/// Google's models, paid tier (ai.google.dev/gemini-api/docs/pricing, read on `READ_ON`). Output includes
/// the tokens a model thinks. Gemini 4 Carbon and Argon cost $2 / $10 through 30 November 2026, and Gemini
/// 3.8 and 3.6 Flash $0.75 / $3.75 through 31 December 2026: the table has the price after that.
const GEMINI: &[(&str, Price)] = &[
    ("gemini-4-carbon", p(4.0, 20.0, 0.05)),
    ("gemini-4-argon", p(4.0, 20.0, 0.05)),
    ("gemini-4-neon", p(3.0, 15.0, 0.1)),
    ("gemini-4-hydrogen", p(1.5, 7.5, 0.1)),
    ("gemini-3.8-flash", p(1.5, 7.5, 0.1)),
    ("gemini-3.6-flash", p(1.5, 7.5, 0.1)),
    ("gemini-3.5-flash-lite", p(0.30, 2.50, 0.1)),
    ("gemini-3.1-flash-lite", p(0.25, 1.50, 0.1)),
    ("gemini-3.1-pro", long(2.0, 12.0, 0.1, 200_000, 4.0, 18.0)),
    ("gemini-3-flash", p(0.50, 3.00, 0.1)),
    ("gemini-2.5-pro", long(1.25, 10.0, 0.1, 200_000, 2.50, 15.0)),
    ("gemini-2.5-flash-lite", p(0.10, 0.40, 0.1)),
    ("gemini-2.5-flash", p(0.30, 2.50, 0.1)),
];
const GEMINI_HIGHEST: Price = p(4.0, 20.0, 0.1);

/// The price of a model, and whether it is in the table (`false`: the provider's highest was taken).
pub fn of(provider: &str, model: &str) -> Option<(Price, bool)> {
    let (table, highest) = match provider {
        "anthropic" => (ANTHROPIC, ANTHROPIC_HIGHEST),
        "gemini" => (GEMINI, GEMINI_HIGHEST),
        _ => return None,
    };
    let found = table.iter().filter(|(name, _)| model.starts_with(name)).max_by_key(|(name, _)| name.len()).map(|(_, price)| *price);
    Some(found.map_or((highest, false), |price| (price, true)))
}

/// Tokens an answer took, as the provider counted them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tokens {
    /// Read at the full price.
    pub input: u64,
    /// Read from the provider's cache.
    pub cached: u64,
    /// Written to the provider's cache (Anthropic charges a little more for these).
    pub cache_written: u64,
    /// Written by the model, its thinking included.
    pub output: u64,
}

/// What `tokens` cost at `price`, in dollars.
pub fn cost(price: &Price, tokens: &Tokens, cache_write: f64) -> f64 {
    let prompt = tokens.input + tokens.cached + tokens.cache_written;
    let (input, output) = match price.long_after {
        Some((after, long_input, long_output)) if prompt > after => (long_input, long_output),
        _ => (price.input, price.output),
    };
    let per = 1_000_000.0;
    (tokens.input as f64 * input + tokens.cached as f64 * input * price.cached + tokens.cache_written as f64 * input * cache_write + tokens.output as f64 * output) / per
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_name_a_model_starts_with_counts() {
        assert_eq!(of("anthropic", "claude-opus-5-5").unwrap(), (p(4.0, 20.0, 0.05), true));
        assert_eq!(of("anthropic", "claude-opus-5").unwrap().0.input, 5.0);
        assert_eq!(of("anthropic", "claude-haiku-4-5-20251001").unwrap().0.input, 1.0);
        assert_eq!(of("gemini", "gemini-2.5-flash-lite").unwrap().0.input, 0.10);
        assert_eq!(of("gemini", "gemini-3.1-pro-preview").unwrap().0.output, 12.0);
        // Not in the table: the highest price, and it says so.
        assert_eq!(of("anthropic", "claude-something-new").unwrap(), (ANTHROPIC_HIGHEST, false));
        assert_eq!(of("openrouter", "anything"), None);
    }

    #[test]
    fn cost_counts_cache_and_long_prompts() {
        let opus = of("anthropic", "claude-opus-5-5").unwrap().0;
        // A million read, a million written: $4 + $20.
        assert!((cost(&opus, &Tokens { input: 1_000_000, output: 1_000_000, ..Tokens::default() }, ANTHROPIC_CACHE_WRITE) - 24.0).abs() < 1e-9);
        // From the cache at a twentieth; into it at a quarter more.
        assert!((cost(&opus, &Tokens { cached: 1_000_000, cache_written: 1_000_000, ..Tokens::default() }, ANTHROPIC_CACHE_WRITE) - (0.2 + 5.0)).abs() < 1e-9);
        // A Gemini 2.5 Pro prompt over 200,000 tokens is all at the higher price.
        let pro = of("gemini", "gemini-2.5-pro").unwrap().0;
        assert!((cost(&pro, &Tokens { input: 300_000, output: 1_000, ..Tokens::default() }, 1.0) - (0.75 + 0.015)).abs() < 1e-9);
    }
}
