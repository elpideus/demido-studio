//! HTML to readable text: no scripts or styles; headings, paragraphs, list items and table rows
//! on lines of their own.
//!
//! A small tolerant scanner rather than a full HTML parser: web pages and e-book chapters only
//! need their text and its block structure, and a scanner cannot fail on broken markup.

use std::borrow::Cow;

/// Elements whose content is not text for a reader. (`<head>` holds nothing else with text but
/// `<title>`, which is kept; its end tag is optional, so it cannot be skipped as a whole.)
const SKIPPED: &[&str] = &["script", "style", "noscript", "template", "svg"];

pub(crate) fn to_text(html: &str) -> String {
    let mut w = Writer::default();
    let b = html.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            let end = html[i..].find('<').map_or(b.len(), |n| i + n);
            w.text(&decode_entities(&html[i..end]));
            i = end;
            continue;
        }
        let rest = &html[i..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            i += 4 + comment.find("-->").map_or(comment.len(), |n| n + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i += rest.find('>').map_or(rest.len(), |n| n + 1);
            continue;
        }
        let Some(tag) = Tag::parse(rest) else {
            w.text("<");
            i += 1;
            continue;
        };
        i += tag.len;
        if tag.closing {
            w.close(&tag.name);
        } else if SKIPPED.contains(&tag.name.as_str()) {
            if !tag.self_closing {
                i = skip_element(html, i, &tag.name);
            }
        } else {
            w.open(&tag.name);
        }
    }
    w.finish()
}

struct Tag {
    /// Lowercase element name.
    name: String,
    closing: bool,
    self_closing: bool,
    /// Bytes from `<` to `>` inclusive.
    len: usize,
}

impl Tag {
    /// Parses the tag `s` starts with, or None when `<` does not start a tag (`a < b`).
    fn parse(s: &str) -> Option<Tag> {
        let b = s.as_bytes();
        let closing = b.get(1) == Some(&b'/');
        let start = if closing { 2 } else { 1 };
        let mut j = start;
        while j < b.len() && (b[j].is_ascii_alphanumeric() || matches!(b[j], b'-' | b':' | b'_')) {
            j += 1;
        }
        if j == start || !b[start].is_ascii_alphabetic() {
            return None;
        }
        let name = s[start..j].to_ascii_lowercase();
        let mut quote = None;
        while j < b.len() {
            match (quote, b[j]) {
                (None, b'"' | b'\'') => quote = Some(b[j]),
                (Some(q), c) if c == q => quote = None,
                (None, b'>') => break,
                _ => {}
            }
            j += 1;
        }
        let self_closing = j < b.len() && j > 0 && b[j - 1] == b'/';
        Some(Tag {
            name,
            closing,
            self_closing,
            len: (j + 1).min(b.len()),
        })
    }
}

/// The position just after `</name ...>`, searching from `from`; the end of the text without one.
fn skip_element(html: &str, from: usize, name: &str) -> usize {
    let b = html.as_bytes();
    let mut i = from;
    while let Some(n) = html[i..].find("</") {
        let after = i + n + 2;
        if b.len() >= after + name.len() && b[after..after + name.len()].eq_ignore_ascii_case(name.as_bytes()) {
            return html[after..].find('>').map_or(b.len(), |m| after + m + 1);
        }
        i = after;
    }
    b.len()
}

/// Resolves `&amp;`, `&eacute;`, `&#233;` and `&#xE9;`. Anything else stays as written.
pub(crate) fn decode_entities(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 1..];
        // An entity name is short, so the `;` is looked for only nearby: searching the rest of
        // the text for every `&` would take quadratic time on text full of bare ampersands.
        if let Some(end) = tail.bytes().take(33).position(|b| b == b';').filter(|&e| e > 0)
            && let Some(resolved) = entity(&tail[..end])
        {
            out.push_str(&resolved);
            rest = &tail[end + 1..];
            continue;
        }
        out.push('&');
        rest = tail;
    }
    out.push_str(rest);
    Cow::Owned(out)
}

fn entity(name: &str) -> Option<Cow<'static, str>> {
    if let Some(number) = name.strip_prefix('#') {
        let code = match number.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => number.parse().ok()?,
        };
        let c = char::from_u32(code).filter(|&c| c != '\0').unwrap_or('\u{FFFD}');
        return Some(Cow::Owned(c.to_string()));
    }
    quick_xml::escape::resolve_html5_entity(name).map(Cow::Borrowed)
}

/// Writes text with the line structure of the elements around it.
#[derive(Default)]
struct Writer {
    out: String,
    /// Line breaks owed before the next text: 1 starts a new line, 2 leaves a blank line.
    breaks: u8,
    /// A space is owed before the next word.
    space: bool,
    /// What the next line starts with: a heading's `#`s, a list item's dash.
    prefix: Option<String>,
    /// Inside `<pre>`: whitespace is kept.
    pre: u32,
    /// Nesting depth of lists.
    lists: usize,
    /// Cells written so far in the current row of each open table.
    cells: Vec<usize>,
    /// Inside a table cell, where blocks run together on the row's line.
    in_cell: u32,
}

impl Writer {
    fn open(&mut self, name: &str) {
        match name {
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.block(2);
                if self.in_cell == 0 {
                    let level = usize::from(name.as_bytes()[1] - b'0');
                    self.prefix = Some(format!("{} ", "#".repeat(level)));
                }
            }
            "li" => {
                self.block(1);
                if self.in_cell == 0 {
                    self.prefix = Some(format!("{}- ", "  ".repeat(self.lists.saturating_sub(1).min(8))));
                }
            }
            "ul" | "ol" | "menu" => {
                self.lists += 1;
                self.block(if self.lists == 1 { 2 } else { 1 });
            }
            "table" => {
                self.cells.push(0);
                self.block(2);
            }
            "tr" => {
                self.block(1);
                if let Some(cells) = self.cells.last_mut() {
                    *cells = 0;
                }
            }
            "td" | "th" => {
                if self.cells.last().is_some_and(|&cells| cells > 0) {
                    self.flush();
                    while self.out.ends_with(' ') {
                        self.out.pop();
                    }
                    self.out.push_str(if self.out.ends_with('\n') || self.out.is_empty() {
                        "| "
                    } else {
                        " | "
                    });
                }
                if let Some(cells) = self.cells.last_mut() {
                    *cells += 1;
                }
                self.in_cell += 1;
            }
            "pre" => {
                self.block(2);
                self.pre += 1;
            }
            "br" => {
                if self.pre > 0 || self.in_cell > 0 {
                    self.space = true;
                } else {
                    self.block(1);
                }
            }
            "img" | "input" | "wbr" => self.space = true,
            _ => self.block(block_breaks(name)),
        }
    }

    fn close(&mut self, name: &str) {
        match name {
            "ul" | "ol" | "menu" => {
                self.lists = self.lists.saturating_sub(1);
                self.block(if self.lists == 0 { 2 } else { 1 });
            }
            "table" => {
                self.cells.pop();
                self.block(2);
            }
            "td" | "th" => {
                self.in_cell = self.in_cell.saturating_sub(1);
                self.space = true;
            }
            "pre" => {
                self.pre = self.pre.saturating_sub(1);
                self.block(2);
            }
            "li" | "tr" => self.block(1),
            _ => self.block(block_breaks(name)),
        }
    }

    /// Owes `n` line breaks before the next text; inside a table cell, just a space.
    fn block(&mut self, n: u8) {
        if n == 0 {
            return;
        }
        if self.in_cell > 0 {
            self.space = true;
        } else {
            self.breaks = self.breaks.max(n);
        }
    }

    fn text(&mut self, s: &str) {
        if self.pre > 0 {
            if s.is_empty() {
                return;
            }
            self.flush();
            self.out.push_str(&s.replace("\r\n", "\n"));
            return;
        }
        if s.starts_with(char::is_whitespace) {
            self.space = true;
        }
        let mut words = s.split_whitespace().peekable();
        while let Some(word) = words.next() {
            self.flush();
            self.out.push_str(word);
            self.space = words.peek().is_some() || s.ends_with(char::is_whitespace);
        }
    }

    /// Writes the line breaks, prefix or space owed before new text.
    fn flush(&mut self) {
        if self.breaks > 0 || self.prefix.is_some() {
            if !self.out.is_empty() {
                while self.out.ends_with(' ') {
                    self.out.pop();
                }
                let have = self.out.chars().rev().take_while(|&c| c == '\n').count();
                for _ in have..usize::from(self.breaks) {
                    self.out.push('\n');
                }
            }
            self.breaks = 0;
            if let Some(prefix) = self.prefix.take() {
                self.out.push_str(&prefix);
            }
        } else if self.space && !self.out.is_empty() && !self.out.ends_with(['\n', ' ']) {
            self.out.push(' ');
        }
        self.space = false;
    }

    fn finish(self) -> String {
        let mut text = String::with_capacity(self.out.len());
        let mut blank = 0;
        for line in self.out.lines() {
            let line = line.trim_end();
            if line.is_empty() {
                blank += 1;
                if blank > 1 {
                    continue;
                }
            } else {
                blank = 0;
            }
            text.push_str(line);
            text.push('\n');
        }
        text.trim().to_owned()
    }
}

/// Line breaks around a block element: 2 for a paragraph-like block, 1 for a line, 0 for inline.
fn block_breaks(name: &str) -> u8 {
    match name {
        "p" | "blockquote" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure"
        | "form" | "fieldset" | "hr" | "title" | "address" | "details" | "dl" | "center" | "h1" | "h2" | "h3"
        | "h4" | "h5" | "h6" => 2,
        "div" | "dt" | "dd" | "caption" | "figcaption" | "summary" | "option" | "body" | "html" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_and_inline_text() {
        let html = "<html><head><title>My page</title><style>p { color: red }</style>\
                    <script>var x = '<p>';</script></head><body>\
                    <h1>Main &amp; only</h1><p>First <b>bold</b>\n  paragraph.</p>\
                    <ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>\
                    <table><tr><th>Name</th><th>Age</th></tr><tr><td>Ann</td><td>31</td></tr></table>\
                    <p>caf&eacute; &#233; &#xE9; &unknown; a&lt;b</p><!-- hidden --><pre>keep\n  this</pre></body></html>";
        assert_eq!(
            to_text(html),
            "My page\n\n# Main & only\n\nFirst bold paragraph.\n\n- one\n- two\n  - deep\n\n\
             Name | Age\nAnn | 31\n\ncafé é é &unknown; a<b\n\nkeep\n  this"
        );
    }

    #[test]
    fn broken_markup_does_not_lose_text() {
        assert_eq!(to_text("a < b and <p>c"), "a < b and\n\nc");
        assert_eq!(to_text("<p>unclosed <b attr=\"x>y\">bold"), "unclosed bold");
        assert_eq!(to_text("<script>never closed"), "");
    }
}
