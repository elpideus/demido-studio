//! The terminal a command draws on. Its output is played onto a real terminal screen, so what
//! the model and the chat read is what a person would have seen: a progress bar that redraws
//! its line ends as its last state, colours and cursor movement are gone, and a full-screen
//! program (btop, top, lazygit) shows as the screen it is drawing.

use avt::parser::Parser;
use avt::terminal::{BufferType, Terminal};

/// Lines kept above the screen. Output that scrolls further keeps its first lines in `head`.
const SCROLLBACK: usize = 5_000;
/// First lines kept of output longer than the scrollback, where errors and summaries often are.
const HEAD: usize = 200;

/// A terminal's device status report request, "where is the cursor?". Windows' pseudo console
/// asks it when it starts and waits for the answer before it shows anything.
const CURSOR_QUERY: &[u8] = b"\x1b[6n";

/// What the terminal shows at one moment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    /// See [`Screen::text`].
    pub text: String,
    /// A full-screen program is drawing: `text` ends with its screen, laid out in columns.
    pub full_screen: bool,
}

pub struct Screen {
    parser: Parser,
    terminal: Terminal,
    /// The start of a character whose bytes are split between two reads.
    partial: Vec<u8>,
    /// The end of the previous read, to spot a query split between two reads.
    tail: Vec<u8>,
    head: Vec<String>,
    /// Lines that scrolled away after `head` was full.
    dropped: usize,
}

impl Screen {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            parser: Parser::new(),
            terminal: Terminal::new((cols, rows), Some(SCROLLBACK)),
            partial: Vec::new(),
            tail: Vec::new(),
            head: Vec::new(),
            dropped: 0,
        }
    }

    /// Draws `bytes`. Returns how many cursor position queries they held; each needs
    /// [`Screen::cursor_report`] written back to the terminal.
    pub fn feed(&mut self, bytes: &[u8]) -> usize {
        let queries = self.count_queries(bytes);
        let mut buf = std::mem::take(&mut self.partial);
        buf.extend_from_slice(bytes);
        let mut start = 0;
        while start < buf.len() {
            match std::str::from_utf8(&buf[start..]) {
                Ok(text) => {
                    self.draw(text);
                    start = buf.len();
                }
                Err(e) => {
                    let valid = start + e.valid_up_to();
                    self.draw(std::str::from_utf8(&buf[start..valid]).unwrap_or_default());
                    match e.error_len() {
                        Some(bad) => {
                            self.draw("\u{FFFD}");
                            start = valid + bad;
                        }
                        // An incomplete character: the next read finishes it.
                        None => {
                            start = valid;
                            break;
                        }
                    }
                }
            }
        }
        buf.drain(..start);
        self.partial = buf;
        queries
    }

    /// The answer to a cursor position query: where the cursor is, counted from 1.
    pub fn cursor_report(&self) -> String {
        let cursor = self.terminal.cursor();
        format!("\x1b[{};{}R", cursor.row + 1, cursor.col + 1)
    }

    pub fn shown(&self) -> Shown {
        Shown {
            text: self.text(),
            full_screen: self.terminal.active_buffer_type() == BufferType::Alternate,
        }
    }

    /// What the terminal shows, as text: every line printed (long lines joined, trailing spaces
    /// and blank lines at the end left out) and, while a full-screen program runs, its screen.
    pub fn text(&self) -> String {
        let mut lines = self.head.clone();
        if self.dropped > 0 {
            lines.push(format!("… {} more lines …", self.dropped));
        }
        lines.extend(self.terminal.text());
        trim_blank_end(&mut lines);
        if self.terminal.active_buffer_type() == BufferType::Alternate {
            let mut screen: Vec<String> = self
                .terminal
                .view()
                .map(|line| line.text().trim_end().to_string())
                .collect();
            trim_blank_end(&mut screen);
            if !lines.is_empty() && !screen.is_empty() {
                lines.push(String::new());
            }
            lines.extend(screen);
        }
        lines.join("\n")
    }

    fn draw(&mut self, text: &str) {
        for ch in text.chars() {
            if let Some(op) = self.parser.feed(ch) {
                self.terminal.execute(op);
            }
        }
        // Lines past the scrollback leave here; the first ones are kept.
        for line in self.terminal.gc() {
            if self.head.len() < HEAD {
                self.head.push(line.text().trim_end().to_string());
            } else {
                self.dropped += 1;
            }
        }
    }

    fn count_queries(&mut self, bytes: &[u8]) -> usize {
        let mut joined = std::mem::take(&mut self.tail);
        let carried = joined.len();
        joined.extend_from_slice(bytes);
        let count = joined
            .windows(CURSOR_QUERY.len())
            .enumerate()
            // A query wholly inside the carried tail was counted with the previous read.
            .filter(|(i, w)| *w == CURSOR_QUERY && i + CURSOR_QUERY.len() > carried)
            .count();
        let keep = joined.len().min(CURSOR_QUERY.len() - 1);
        self.tail = joined[joined.len() - keep..].to_vec();
        count
    }
}

fn trim_blank_end(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_redrawn_progress_line_ends_as_its_last_state() {
        let mut s = Screen::new(80, 10);
        s.feed(b"Downloading\r\n 10%\r 55%\r100%\r\ndone\r\n");
        assert_eq!(s.text(), "Downloading\n100%\ndone");
    }

    #[test]
    fn colours_and_cursor_moves_leave_plain_text() {
        let mut s = Screen::new(80, 10);
        s.feed(b"\x1b[31m\x1b[1mError:\x1b[m bad\r\nnext\x1b[3;1Hthird\r\n");
        assert_eq!(s.text(), "Error: bad\nnext\nthird");
    }

    #[test]
    fn characters_split_between_reads_arrive_whole() {
        let mut s = Screen::new(80, 10);
        let text = "héllo ✓".as_bytes();
        for b in text {
            s.feed(std::slice::from_ref(b));
        }
        assert_eq!(s.text(), "héllo ✓");
    }

    #[test]
    fn invalid_bytes_become_replacement_characters() {
        let mut s = Screen::new(80, 10);
        s.feed(b"a\xffb");
        assert_eq!(s.text(), "a\u{FFFD}b");
    }

    #[test]
    fn cursor_queries_are_counted_even_across_reads() {
        let mut s = Screen::new(80, 10);
        assert_eq!(s.feed(b"\x1b[6nhello"), 1);
        assert_eq!(s.feed(b"x\x1b["), 0);
        assert_eq!(s.feed(b"6n"), 1);
        assert_eq!(s.feed(b"\x1b[6n\x1b[6n"), 2);
        // The cursor sits after "hellox" on the first row.
        assert_eq!(s.cursor_report(), "\x1b[1;7R");
    }

    #[test]
    fn a_full_screen_program_shows_its_screen_after_what_came_before() {
        let mut s = Screen::new(40, 5);
        s.feed(b"before\r\n\x1b[?1049h\x1b[H\x1b[2J CPU 12%\r\n MEM 40%");
        assert_eq!(
            s.shown(),
            Shown {
                text: "before\n\n CPU 12%\n MEM 40%".into(),
                full_screen: true
            }
        );
        // Once it leaves, only what it left behind in the normal screen remains.
        s.feed(b"\x1b[?1049lafter\r\n");
        assert_eq!(
            s.shown(),
            Shown {
                text: "before\nafter".into(),
                full_screen: false
            }
        );
    }

    #[test]
    fn long_lines_are_joined_back() {
        let mut s = Screen::new(10, 5);
        s.feed(b"abcdefghijklmnopqrstuvwxyz\r\nend\r\n");
        assert_eq!(s.text(), "abcdefghijklmnopqrstuvwxyz\nend");
    }

    #[test]
    fn output_longer_than_the_scrollback_keeps_its_start_and_its_end() {
        let mut s = Screen::new(20, 5);
        for i in 0..(SCROLLBACK + HEAD + 50) {
            s.feed(format!("line {i}\r\n").as_bytes());
        }
        let text = s.text();
        assert!(text.starts_with("line 0\nline 1\n"));
        assert!(text.contains("more lines …"));
        assert!(text.ends_with(&format!("line {}", SCROLLBACK + HEAD + 49)));
    }
}
