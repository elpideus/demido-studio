//! Sources footer: asks the model to end a web-informed reply with a markdown link list.
//!
//! The frontend (`src/lib/parseSources.ts`) peels that list off the tail of the message and
//! renders it as source chips, so the block's shape is a contract, not a suggestion — hence the
//! rider spells out the exact heading and bullet form, and says what *not* to add around it.
//!
//! Only appended when a web tool is actually on the offered list: a model with no way to browse
//! that is told about a sources footer either ignores it or, worse, invents one.

/// Appended last in `effective_prompt`, after the caveman block, because the caveman levels
/// compress prose and a URL is not prose — the footer rules have to be the nearest instruction
/// about their own format.
const SOURCES_RIDER: &str = "\
## Sources footer

Used `web_search` or `web_fetch` this turn, and the reply draws on what came back → end the \
reply with a sources footer. Never otherwise: no web tool call, or the results contributed \
nothing → no footer.

Exact format, at the very end of the message, nothing after it:

Sources:
- [Label](https://full.url/of/the/page)
- [Label](https://other.url/article)

Rules:
- Literal line `Sources:` on its own line, then one `- [Label](url)` per line. No heading marks, \
no numbering, no bold, no prose before or after the list.
- Label = the site or publication name (`Reddit`, `Medium`, `Wikipedia`, `BBC News`), not the \
page title, not a bare domain like `medium.com`.
- URL = the full absolute `https://` link to the page actually used, copied verbatim from the \
tool result. Never shorten, guess, normalize, or reconstruct a URL. No link you did not get \
from a tool result.
- Only pages that informed the answer. One entry per URL — dedupe. Cap at 8, most useful first.
- This footer is the only place links are collected; keep citing inline as usual if useful.
- Style rules do not apply here: the label and URL are written out in full whatever the response \
style asks for.";

/// Appended to every `web_search` / `web_fetch` result.
///
/// The system-prompt rider alone measured inert: Qwen3.5-9B-Q4_K_M searched, answered from the
/// results, and emitted no footer — same shape as the thinking-rider finding, where a rule stated
/// once at the top of the prompt lost to thousands of tokens of intervening context. This reminder
/// rides the tool result instead, so it lands next to the URLs it is talking about, at the moment
/// the model is deciding what to write. Keep it short: it is billed once per search, and a long
/// block here competes with the results themselves.
const RESULT_REMINDER: &str = "\n\n---\nReminder: this reply must end with a `Sources:` footer \
listing the pages above that you actually used — one `- [Site](url)` bullet each, URLs copied \
verbatim from this result. Nothing after the list.";

/// Appends the per-result reminder to web tool output. `output` is the tool result as the model
/// will see it.
pub fn append_to_web_result(output: String) -> String {
    // An error string or an empty result has no URLs to cite — reminding there invites a
    // fabricated footer, which is the one failure worse than a missing one.
    if output.trim().is_empty() || !output.contains("http") {
        return output;
    }
    format!("{}{}", output, RESULT_REMINDER)
}

/// Appends the sources rider when the model has at least one web tool available.
pub fn append_to_prompt(prompt: String, web_tools_available: bool) -> String {
    if !web_tools_available {
        return prompt;
    }
    if prompt.trim().is_empty() {
        return SOURCES_RIDER.to_string();
    }
    format!("{}\n\n{}", prompt, SOURCES_RIDER)
}

/// True when the offered tool list contains a tool that returns web content.
pub fn web_tools_available<'a>(tool_names: impl IntoIterator<Item = &'a str>) -> bool {
    tool_names
        .into_iter()
        .any(|n| n == "web_search" || n == "web_fetch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_web_tools_means_no_rider() {
        let p = append_to_prompt("base".into(), false);
        assert_eq!(p, "base");
    }

    #[test]
    fn rider_appended_after_existing_prompt() {
        let p = append_to_prompt("base".into(), true);
        assert!(p.starts_with("base\n\n"));
        assert!(p.contains("Sources:"));
    }

    #[test]
    fn empty_prompt_gets_bare_rider() {
        let p = append_to_prompt("   ".into(), true);
        assert_eq!(p, SOURCES_RIDER);
    }

    #[test]
    fn detects_web_tools_by_name() {
        assert!(web_tools_available(["read_file", "web_search"]));
        assert!(web_tools_available(["web_fetch"]));
        assert!(!web_tools_available(["read_file", "list_emails"]));
        assert!(!web_tools_available([]));
    }

    /// The frontend parser keys off the literal `Sources:` line and `- [Label](url)` bullets.
    /// If this rider ever stops teaching that exact shape, the chips silently stop rendering.
    #[test]
    fn rider_teaches_the_shape_the_parser_expects() {
        assert!(SOURCES_RIDER.contains("\nSources:\n"));
        assert!(SOURCES_RIDER.contains("- [Label](https://full.url/of/the/page)"));
    }

    /// A model that emits a footer with no tool call is fabricating citations — the "never
    /// otherwise" clause is the only thing standing against that.
    #[test]
    fn rider_forbids_footer_without_tool_results() {
        assert!(SOURCES_RIDER.contains("Never otherwise"));
        assert!(SOURCES_RIDER.contains("No link you did not get"));
    }

    #[test]
    fn web_result_carries_the_reminder() {
        let out = append_to_web_result("Title: X\nURL: https://x.com/1".into());
        assert!(out.starts_with("Title: X"));
        assert!(out.contains("Sources:"));
    }

    /// No URLs in the output means the search failed or found nothing. Reminding a model to cite
    /// pages it was never given is how invented citations happen.
    #[test]
    fn resultless_output_gets_no_reminder() {
        assert_eq!(append_to_web_result("".into()), "");
        assert_eq!(
            append_to_web_result("No results found for \"x\".".into()),
            "No results found for \"x\"."
        );
        assert_eq!(
            append_to_web_result("Search request error: timed out".into()),
            "Search request error: timed out"
        );
    }
}
