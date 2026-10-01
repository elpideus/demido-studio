//! Splitting a text into passages for search.
//!
//! A passage is about `target_chars` long and ends where the text naturally breaks: the end of a
//! paragraph if one is near, else a line, a sentence, a word. Each one repeats the end of the
//! previous passage (`overlap_chars` or so, from a word boundary), so a sentence cut in two is
//! whole in one of them. A page marker line always stays with the text after it.

/// A passage of a document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// 0-based position in the document.
    pub seq: u32,
    /// Page (slide, sheet) the passage starts on, 1-based; None for a text without pages.
    pub page: Option<u32>,
    pub text: String,
}

/// Splits `text` into passages of about `target_chars` characters. `page_starts` are the byte
/// offsets of the page marker lines ([`crate::Extracted::page_starts`]).
pub fn chunks(text: &str, page_starts: &[usize], target_chars: usize, overlap_chars: usize) -> Vec<Chunk> {
    let target = target_chars.max(16);
    let overlap = overlap_chars.min(target / 2);
    let mut out: Vec<Chunk> = Vec::new();
    let mut start = skip_space(text, 0);
    while start < text.len() {
        let limit = advance(text, start, target);
        let end = if limit >= text.len() {
            text.len()
        } else {
            let end = break_before(text, start, limit, target / 2);
            keep_markers_with_text(text, start, end, page_starts)
        };
        let piece = text[start..end].trim();
        if !piece.is_empty() {
            out.push(Chunk {
                seq: u32::try_from(out.len()).unwrap_or(u32::MAX),
                page: page_at(page_starts, start),
                text: piece.to_owned(),
            });
        }
        if end >= text.len() {
            break;
        }
        // The next passage starts a little before this one ended, on a word boundary, and always
        // after this one's start, so the loop ends.
        let back = retreat(text, end, overlap);
        let mut next = skip_space(text, word_start(text, back));
        // Nor does it reach back across a page marker: it starts at the marker, so its page is
        // the one its text is on.
        if let Some(&marker) = page_starts.iter().rev().find(|&&m| m > next && m <= end) {
            next = marker;
        }
        start = if next > start && next < end {
            next
        } else {
            skip_space(text, end)
        };
    }
    out
}

/// The byte offset `n` characters after `from` (or the end).
fn advance(text: &str, from: usize, n: usize) -> usize {
    text[from..].char_indices().nth(n).map_or(text.len(), |(i, _)| from + i)
}

/// The byte offset `n` characters before `from` (or the start).
fn retreat(text: &str, from: usize, n: usize) -> usize {
    text[..from]
        .char_indices()
        .rev()
        .nth(n.saturating_sub(1))
        .map_or(0, |(i, _)| i)
}

fn skip_space(text: &str, from: usize) -> usize {
    text[from..]
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map_or(text.len(), |(i, _)| from + i)
}

/// The start of the word `at` is in, or `at` itself when it is at whitespace.
fn word_start(text: &str, at: usize) -> usize {
    text[..at]
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8())
}

/// Where a passage from `start` should end, at most at `limit`: after the last paragraph break,
/// else line break, sentence end or space in the second half of the window (at least `min_chars`
/// in). A window with no break at all ends at `limit`, which is always a character boundary.
fn break_before(text: &str, start: usize, limit: usize, min_chars: usize) -> usize {
    let earliest = advance(text, start, min_chars).min(limit);
    let window = &text[earliest..limit];
    let found = |at: Option<usize>, len: usize| at.map(|i| earliest + i + len);
    found(window.rfind("\n\n"), 2)
        .or_else(|| found(window.rfind('\n'), 1))
        .or_else(|| {
            [". ", "! ", "? ", ".\t", "。", "！", "？"]
                .iter()
                .filter_map(|end| window.rfind(end).map(|i| earliest + i + end.len()))
                .max()
        })
        .or_else(|| {
            window
                .char_indices()
                .rev()
                .find(|(_, c)| c.is_whitespace())
                .map(|(i, c)| earliest + i + c.len_utf8())
        })
        .unwrap_or(limit)
}

/// Moves `end` back to the start of a page marker line when the passage would end with that
/// marker (or with the marker's own line), so the marker opens the next passage.
fn keep_markers_with_text(text: &str, start: usize, end: usize, page_starts: &[usize]) -> usize {
    for &marker in page_starts.iter().rev() {
        if marker <= start || marker >= end {
            continue;
        }
        let line_end = text[marker..].find('\n').map_or(text.len(), |i| marker + i);
        // Nothing but the marker (and whitespace) between it and the end of the passage.
        if text[line_end.min(end)..end].trim().is_empty() {
            return marker;
        }
        break;
    }
    end
}

/// The page containing byte `at`: the number of markers at or before it (1 before the first).
fn page_at(page_starts: &[usize], at: usize) -> Option<u32> {
    if page_starts.is_empty() {
        return None;
    }
    let n = page_starts.partition_point(|&p| p <= at).max(1);
    Some(u32::try_from(n).unwrap_or(u32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(chunks: &[Chunk]) -> String {
        chunks.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn short_text_is_one_passage() {
        let c = chunks("  Just a line.  ", &[], 100, 10);
        assert_eq!(
            c,
            vec![Chunk {
                seq: 0,
                page: None,
                text: "Just a line.".into()
            }]
        );
        assert!(chunks("", &[], 100, 10).is_empty());
        assert!(chunks(" \n\n ", &[], 100, 10).is_empty());
    }

    #[test]
    fn passages_break_at_paragraphs_and_overlap() {
        let para = |n: usize| format!("Paragraph {n} says something useful about topic {n}.");
        let text = (0..12).map(para).collect::<Vec<_>>().join("\n\n");
        let c = chunks(&text, &[], 160, 40);
        assert!(c.len() > 3);
        for (i, chunk) in c.iter().enumerate() {
            assert_eq!(chunk.seq as usize, i);
            assert!(chunk.text.chars().count() <= 160, "{}", chunk.text);
            assert!(!chunk.text.is_empty());
        }
        // Each passage but the last ends at the end of a paragraph.
        for chunk in &c[..c.len() - 1] {
            assert!(chunk.text.ends_with('.'), "{:?}", chunk.text);
        }
        // Every paragraph is whole in some passage.
        for n in 0..12 {
            assert!(c.iter().any(|chunk| chunk.text.contains(&para(n))), "paragraph {n}");
        }
        // Passages overlap: the start of each is in the one before.
        for pair in c.windows(2) {
            let first_word = pair[1].text.split_whitespace().next().unwrap();
            assert!(pair[0].text.contains(first_word));
        }
    }

    #[test]
    fn words_and_multibyte_text_are_never_cut() {
        let text = "caffè perché città ".repeat(60);
        let c = chunks(&text, &[], 50, 10);
        assert!(c.len() > 10);
        for chunk in &c {
            for word in chunk.text.split_whitespace() {
                assert!(["caffè", "perché", "città"].contains(&word), "{word:?}");
            }
        }
        // One enormous "word" still splits, on character boundaries.
        let blob = "é".repeat(500);
        let c = chunks(&blob, &[], 100, 20);
        assert!(c.iter().all(|chunk| chunk.text.chars().count() <= 100));
        assert!(joined(&c).chars().filter(|&ch| ch == 'é').count() >= 500);
    }

    #[test]
    fn pages_are_known_and_markers_stay_with_their_text() {
        let pages: Vec<String> = (1..=4)
            .map(|n| format!("--- Page {n} ---\n{}", format!("Words on page {n}. ").repeat(6)))
            .collect();
        let text = pages.join("\n\n");
        let starts: Vec<usize> = (1..=4)
            .map(|n| text.find(&format!("--- Page {n} ---")).unwrap())
            .collect();
        let c = chunks(&text, &starts, 140, 20);
        for chunk in &c {
            let page = chunk.page.unwrap();
            // A passage never ends on a marker.
            assert!(
                !chunk.text.lines().last().unwrap().starts_with("--- Page"),
                "{:?}",
                chunk.text
            );
            // It starts on the page it says.
            let first = chunk.text.lines().next().unwrap();
            if first.starts_with("--- Page") {
                assert_eq!(first, format!("--- Page {page} ---"));
            } else {
                assert!(first.contains(&format!("page {page}")), "{first:?} on page {page}");
            }
        }
        assert_eq!(c.first().unwrap().page, Some(1));
        assert_eq!(c.last().unwrap().page, Some(4));
        assert_eq!(page_at(&starts, 0), Some(1));
        assert_eq!(page_at(&[], 5), None);
    }
}
