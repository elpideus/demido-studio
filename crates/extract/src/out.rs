//! Assembling the text a model reads: page markers, paragraphs, and the character cap.

use crate::MAX_TEXT_CHARS;

/// Text being assembled, with the byte offset of every page marker.
///
/// Stops taking text at [`MAX_TEXT_CHARS`], so a huge spreadsheet or PDF is never held in memory
/// as a whole; readers check [`TextOut::is_full`] to stop early.
pub(crate) struct TextOut {
    text: String,
    page_starts: Vec<usize>,
    chars: usize,
    truncated: bool,
    /// Something other than page markers and whitespace was written.
    has_content: bool,
    /// Nothing but a page marker has been written since the last page started.
    at_page_start: bool,
}

/// What a [`TextOut`] ends as.
pub(crate) struct Finished {
    /// None when nothing but page markers and whitespace was written.
    pub text: Option<String>,
    pub page_starts: Vec<usize>,
    pub truncated: bool,
}

impl TextOut {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            page_starts: Vec::new(),
            chars: 0,
            truncated: false,
            has_content: false,
            at_page_start: false,
        }
    }

    /// The cap was reached: anything written from now on is dropped.
    pub fn is_full(&self) -> bool {
        self.truncated
    }

    /// Appends text as it is.
    pub fn push(&mut self, s: &str) {
        if !s.trim().is_empty() {
            self.has_content = true;
            self.at_page_start = false;
        }
        self.append(s);
    }

    /// Starts a paragraph: a blank line after what came before (a single line break after a page
    /// marker), then `s`.
    pub fn paragraph(&mut self, s: &str) {
        let s = s.trim_end();
        if s.trim().is_empty() {
            return;
        }
        self.end_block();
        self.push(s.trim_start_matches(['\n', '\r']));
    }

    /// Continues the current block on a new line: the next list item or table row.
    pub fn line(&mut self, s: &str) {
        let s = s.trim_end();
        if s.trim().is_empty() {
            return;
        }
        self.trim_end();
        if !self.text.is_empty() {
            self.append("\n");
        }
        self.push(s.trim_start_matches(['\n', '\r']));
    }

    /// Starts page (slide, sheet) `marker` on a line of its own, after a blank line.
    pub fn page(&mut self, marker: &str) {
        if self.truncated {
            return;
        }
        self.trim_end();
        if !self.text.is_empty() {
            self.append("\n\n");
        }
        // A marker is written whole or not at all, so every offset in `page_starts` points at one.
        if self.chars + marker.chars().count() + 1 > MAX_TEXT_CHARS {
            self.truncated = true;
            return;
        }
        self.page_starts.push(self.text.len());
        self.append(marker);
        self.append("\n");
        self.at_page_start = true;
    }

    pub fn finish(mut self) -> Finished {
        self.trim_end();
        Finished {
            text: self.has_content.then_some(self.text),
            page_starts: self.page_starts,
            truncated: self.truncated,
        }
    }

    /// Ends the current block with a blank line, or a single line break right after a marker.
    fn end_block(&mut self) {
        self.trim_end();
        if self.text.is_empty() {
            return;
        }
        self.append(if self.at_page_start { "\n" } else { "\n\n" });
    }

    fn trim_end(&mut self) {
        while let Some(c) = self.text.chars().next_back()
            && c.is_whitespace()
        {
            self.text.pop();
            self.chars -= 1;
        }
    }

    fn append(&mut self, s: &str) {
        if self.truncated || s.is_empty() {
            return;
        }
        let room = MAX_TEXT_CHARS - self.chars;
        let n = s.chars().take(room + 1).count();
        if n <= room {
            self.text.push_str(s);
            self.chars += n;
            return;
        }
        let cut = s.char_indices().nth(room).map_or(s.len(), |(i, _)| i);
        self.text.push_str(&s[..cut]);
        self.chars += room;
        self.truncated = true;
    }
}

/// Lays out paragraphs, list items and table rows from a document's structure.
///
/// Shared by Word, OpenDocument and PowerPoint: headings become Markdown `#` lines, list items
/// `- ` lines, and each table row one `a | b | c` line. A paragraph inside a table cell becomes
/// part of that cell; a table inside a cell becomes `;`-separated text in it.
pub(crate) struct Blocks<'a> {
    pub out: &'a mut TextOut,
    prev: Prev,
    tables: Vec<Table>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Prev {
    Paragraph,
    ListItem,
    Row,
}

#[derive(Default)]
struct Table {
    row: Vec<String>,
    cell: Option<String>,
}

impl<'a> Blocks<'a> {
    pub fn new(out: &'a mut TextOut) -> Self {
        Self {
            out,
            prev: Prev::Paragraph,
            tables: Vec::new(),
        }
    }

    /// Writes a finished paragraph. `heading` (1-9) makes it a Markdown heading, `list` (the
    /// nesting level, 0 at the top) a list item.
    pub fn paragraph(&mut self, text: &str, heading: Option<u8>, list: Option<u8>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        if let Some(cell) = self.tables.last_mut().and_then(|t| t.cell.as_mut()) {
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(text);
            return;
        }
        match (heading, list) {
            (Some(level), _) => {
                let marks = "#".repeat(usize::from(level.clamp(1, 6)));
                self.out.paragraph(&format!("{marks} {text}"));
                self.prev = Prev::Paragraph;
            }
            (None, Some(level)) => {
                let item = format!("{}- {text}", "  ".repeat(usize::from(level.min(8))));
                if self.prev == Prev::ListItem {
                    self.out.line(&item);
                } else {
                    self.out.paragraph(&item);
                }
                self.prev = Prev::ListItem;
            }
            (None, None) => {
                self.out.paragraph(text);
                self.prev = Prev::Paragraph;
            }
        }
    }

    /// The next paragraph starts a new block, even if it is a list item or table row.
    pub fn separate(&mut self) {
        self.prev = Prev::Paragraph;
    }

    pub fn table_start(&mut self) {
        self.tables.push(Table::default());
    }

    pub fn row_start(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.row.clear();
            table.cell = None;
        }
    }

    pub fn cell_start(&mut self) {
        if let Some(table) = self.tables.last_mut() {
            table.cell = Some(String::new());
        }
    }

    pub fn cell_end(&mut self) {
        if let Some(table) = self.tables.last_mut()
            && let Some(cell) = table.cell.take()
        {
            table.row.push(cell);
        }
    }

    pub fn row_end(&mut self) {
        self.cell_end();
        let Some(table) = self.tables.last_mut() else {
            return;
        };
        let cells = std::mem::take(&mut table.row);
        if cells.iter().all(|c| c.trim().is_empty()) {
            return;
        }
        let line = cells.iter().map(|c| c.trim()).collect::<Vec<_>>().join(" | ");
        let depth = self.tables.len();
        if depth >= 2 {
            if let Some(cell) = self.tables[depth - 2].cell.as_mut() {
                if !cell.is_empty() {
                    cell.push_str("; ");
                }
                cell.push_str(&line);
            }
            return;
        }
        if self.prev == Prev::Row {
            self.out.line(&line);
        } else {
            self.out.paragraph(&line);
        }
        self.prev = Prev::Row;
    }

    pub fn table_end(&mut self) {
        if self
            .tables
            .last()
            .is_some_and(|t| t.cell.is_some() || !t.row.is_empty())
        {
            self.row_end();
        }
        if self.tables.pop().is_some() && self.tables.is_empty() {
            self.prev = Prev::Paragraph;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_marked_and_separated() {
        let mut out = TextOut::new();
        out.page("--- Page 1 ---");
        out.paragraph("one");
        out.paragraph("two");
        out.page("--- Page 2 ---");
        out.push("three");
        let done = out.finish();
        let text = done.text.unwrap();
        assert_eq!(text, "--- Page 1 ---\none\n\ntwo\n\n--- Page 2 ---\nthree");
        assert_eq!(done.page_starts, vec![0, text.find("--- Page 2").unwrap()]);
    }

    #[test]
    fn markers_alone_are_no_text() {
        let mut out = TextOut::new();
        out.page("--- Slide 1 ---");
        out.page("--- Slide 2 ---");
        assert!(out.finish().text.is_none());
    }

    #[test]
    fn tables_become_rows() {
        let mut out = TextOut::new();
        let mut blocks = Blocks::new(&mut out);
        blocks.paragraph("Intro", Some(1), None);
        blocks.table_start();
        for row in [["a", "b"], ["c", "d"]] {
            blocks.row_start();
            for cell in row {
                blocks.cell_start();
                blocks.paragraph(cell, None, None);
                blocks.cell_end();
            }
            blocks.row_end();
        }
        blocks.table_end();
        blocks.paragraph("first", None, Some(0));
        blocks.paragraph("nested", None, Some(1));
        assert_eq!(
            out.finish().text.unwrap(),
            "# Intro\n\na | b\nc | d\n\n- first\n  - nested"
        );
    }
}
