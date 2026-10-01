//! Finding passages of attached files. Passages are indexed by SQLite FTS5 and ranked with BM25,
//! with Porter stemming, so "terminating" finds "terminate". No embedding model is needed, and
//! the model can search again with other words through the `search_files` tool.

use std::collections::HashSet;

/// Words too common to say what a message is about.
const STOPWORDS: &[&str] = &[
    "a", "about", "above", "after", "again", "all", "also", "am", "an", "and", "any", "are", "as", "at", "be",
    "because", "been", "before", "being", "below", "between", "both", "but", "by", "can", "could", "did", "do", "does",
    "doing", "down", "during", "each", "either", "else", "even", "ever", "every", "few", "file", "files", "for",
    "from", "further", "get", "give", "had", "has", "have", "having", "he", "her", "here", "hers", "him", "his", "how",
    "i", "if", "in", "into", "is", "it", "its", "just", "let", "like", "me", "more", "most", "my", "no", "nor", "not",
    "now", "of", "off", "on", "once", "only", "or", "other", "our", "out", "over", "own", "please", "really", "same",
    "say", "says", "she", "should", "show", "so", "some", "such", "tell", "than", "thanks", "thank", "that", "the",
    "their", "them", "then", "there", "these", "they", "this", "those", "through", "to", "too", "under", "until", "up",
    "us", "very", "was", "we", "were", "what", "when", "where", "which", "while", "who", "whom", "why", "will", "with",
    "would", "yes", "you", "your", "yours",
];

/// Most words of a message a query uses.
const MAX_TERMS: usize = 32;

/// The words of a message that say what it is about, lowercase, each once: none for "thanks!" or
/// "summarise this" beyond "summarise".
pub fn terms(text: &str) -> Vec<String> {
    let stop: HashSet<&str> = STOPWORDS.iter().copied().collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for word in text.split(|c: char| !c.is_alphanumeric()) {
        let word = word.to_lowercase();
        let long_enough = word.chars().count() >= 2 || !word.is_ascii();
        if word.is_empty() || !long_enough || stop.contains(word.as_str()) || !seen.insert(word.clone()) {
            continue;
        }
        out.push(word);
        if out.len() == MAX_TERMS {
            break;
        }
    }
    out
}

/// One word as an FTS5 query: quoted, so it is always a plain term, never an operator.
pub fn phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', ""))
}

/// An FTS5 query matching passages with any of `terms`, or `None` when there are none.
pub fn match_any(terms: &[String]) -> Option<String> {
    (!terms.is_empty()).then(|| terms.iter().map(|t| phrase(t)).collect::<Vec<_>>().join(" OR "))
}

/// An FTS5 query matching passages with any meaningful word of `text`.
pub fn fts_query(text: &str) -> Option<String> {
    match_any(&terms(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_keep_the_words_that_matter() {
        assert_eq!(
            fts_query("What does the contract say about termination?").as_deref(),
            Some("\"contract\" OR \"termination\"")
        );
        assert_eq!(fts_query("thanks!"), None);
        assert_eq!(fts_query(""), None);
        // Operators and quotes cannot reach FTS5.
        assert_eq!(
            fts_query("NEAR(\"x\" OR y) AND revenue 2024").as_deref(),
            Some("\"near\" OR \"revenue\" OR \"2024\"")
        );
        assert_eq!(
            fts_query("Umsatz Umsatz über").as_deref(),
            Some("\"umsatz\" OR \"über\"")
        );
        assert_eq!(terms("Notice NOTICE notices"), ["notice", "notices"]);
    }
}
