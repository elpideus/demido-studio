//! What a program printed: kept whole, and shown to a model short.
//!
//! The brief asks for this (4.18): output made shorter, "while always keeping a way to see the entire
//! output if it's really needed". A build prints thousands of lines and a small model has room for a few
//! dozen; but which few dozen is not always its start and its end.
//!
//! - **The whole is kept**, as a terminal would show it at the end: without the colour codes, and with a
//!   progress line that kept rewriting itself as its last state. The person can open it; a model can
//!   read on in it by line number.
//! - **The short view** is chosen by rules, not by a model: the lines that speak of something going
//!   wrong first, then the ending (where a program says how it went), then the beginning. A run of lines
//!   that differ only in their numbers is folded into one. What was left out is marked where it was left
//!   out, by its line numbers, so that it can be asked for.
//!
//! This is the general case. Shortening that knows each program (what `cargo test` or `git log` prints
//! and which lines of it matter) is what rtk does; research note 16 says where that stands.

/// What marks a place in the short view where lines were left out. `chat` looks for it.
pub const LEFT_OUT: &str = "left out]";

/// No line of the short view is longer than this.
const A_LINE_AT_MOST: usize = 240;
/// A run of this many lines alike, or more, is folded.
const A_RUN_IS: usize = 4;
/// Words that mark a line as one a reader must see.
const TELLING: [&str; 16] =
    ["error", "fail", "panic", "exception", "traceback", "fatal", "denied", "not found", "no such", "cannot", "can't", "warning", "assert", "undefined", "segmentation", "killed"];

/// What a program printed, as a terminal would show it once the program has ended.
pub fn as_a_terminal_shows(raw: &str) -> String {
    // Colour and cursor codes: an escape, then either `[`…letter, or `]`…bell.
    let mut plain = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            plain.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for code in chars.by_ref() {
                    if ('@'..='~').contains(&code) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(code) = chars.next() {
                    if code == '\u{7}' || (code == '\u{1b}' && chars.peek() == Some(&'\\')) {
                        if code == '\u{1b}' {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // A lone escape, or one with a single letter after it.
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    let mut out = String::with_capacity(plain.len());
    for line in plain.replace("\r\n", "\n").split('\n') {
        // A line that was written over shows what was written last.
        let last = line.rsplit('\r').find(|part| !part.is_empty()).unwrap_or_default();
        out.push_str(last.trim_end());
        out.push('\n');
    }
    out.trim_end().to_string()
}

/// A text of many lines, made short.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub text: String,
    /// How many lines the whole has.
    pub lines: usize,
    /// How many of them are not in `text`.
    pub left_out: usize,
}

fn cut(line: &str) -> String {
    if line.chars().count() <= A_LINE_AT_MOST { line.to_string() } else { format!("{}…", line.chars().take(A_LINE_AT_MOST).collect::<String>()) }
}

/// A line with every number in it replaced: two lines alike but for their numbers have the same shape.
fn shape(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_number = false;
    for c in line.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
            }
            in_number = true;
        } else {
            out.push(c);
            in_number = false;
        }
    }
    out
}

fn is_telling(line: &str) -> bool {
    let lower = line.to_lowercase();
    TELLING.iter().any(|word| lower.contains(word))
}

/// One piece of the short view: a line, or a run of lines alike folded into its first.
struct Piece {
    /// The lines it stands for, from 1.
    from: usize,
    to: usize,
    text: String,
    telling: bool,
    /// How many of the lines it stands for it shows: one, or for a folded run its first and its last.
    shows: usize,
}

/// The short view of a text (as `as_a_terminal_shows` gives it), in no more than about `at_most` characters.
pub fn shortened(text: &str, at_most: usize) -> Shown {
    let lines: Vec<&str> = text.lines().collect();
    if text.chars().count() <= at_most {
        return Shown { text: text.to_string(), lines: lines.len(), left_out: 0 };
    }
    // Runs of lines alike are folded first: a hundred "Compiling …" lines say what one says.
    let mut pieces: Vec<Piece> = Vec::new();
    let mut at = 0;
    while at < lines.len() {
        let this = shape(lines[at]);
        // A line that tells of something going wrong is never folded into lines that do not: a run of
        // "test … ok" ends at the one "test … FAILED".
        let telling = is_telling(lines[at]);
        let run = lines[at..].iter().take_while(|line| shape(line) == this && is_telling(line) == telling).count();
        // The first words of a line, for lines that differ in more than numbers ("Compiling serde v1…").
        let first_word = lines[at].split_whitespace().next().filter(|w| w.chars().count() >= 4 && w.chars().all(char::is_alphabetic));
        let alike = first_word.map_or(0, |word| lines[at..].iter().take_while(|line| line.split_whitespace().next() == Some(word) && is_telling(line) == telling).count());
        let run = run.max(alike);
        if run >= A_RUN_IS && !lines[at].trim().is_empty() {
            // Its first line and its last: the last of a run of progress lines says how far it got.
            let text = format!("{}\n(… {} more lines like these …)\n{}", cut(lines[at]), run - 2, cut(lines[at + run - 1]));
            pieces.push(Piece { from: at + 1, to: at + run, text, telling, shows: 2 });
            at += run;
        } else {
            pieces.push(Piece { from: at + 1, to: at + 1, text: cut(lines[at]), telling, shows: 1 });
            at += 1;
        }
    }
    // What is taken, in order of what a reader needs most: what went wrong (with the line after it, which
    // often says where), then the ending, then the beginning.
    let size = |piece: &Piece| piece.text.chars().count() + 1;
    let mut taken = vec![false; pieces.len()];
    let mut room = at_most as i64;
    let take = |i: usize, taken: &mut [bool], room: &mut i64| {
        if !taken[i] && *room - size(&pieces[i]) as i64 >= 0 {
            taken[i] = true;
            *room -= size(&pieces[i]) as i64;
        }
    };
    let for_what_went_wrong = at_most as i64 / 2;
    for i in 0..pieces.len() {
        if pieces[i].telling && room > at_most as i64 - for_what_went_wrong {
            take(i, &mut taken, &mut room);
            if i + 1 < pieces.len() {
                take(i + 1, &mut taken, &mut room);
            }
        }
    }
    let for_the_beginning = at_most as i64 / 6;
    for i in (0..pieces.len()).rev() {
        if room <= for_the_beginning {
            break;
        }
        take(i, &mut taken, &mut room);
    }
    for i in 0..pieces.len() {
        if room <= 0 {
            break;
        }
        take(i, &mut taken, &mut room);
    }
    let (mut out, mut left_out, mut gap_from) = (String::new(), 0usize, None::<usize>);
    for (i, piece) in pieces.iter().enumerate() {
        if taken[i] {
            if let Some(from) = gap_from.take() {
                let to = piece.from - 1;
                out.push_str(&if from == to { format!("[line {from} is left out]\n") } else { format!("[lines {from} to {to} are left out]\n") });
            }
            out.push_str(&piece.text);
            out.push('\n');
            // A folded run shows one of its lines.
            left_out += piece.to - piece.from + 1 - piece.shows;
        } else {
            gap_from.get_or_insert(piece.from);
            left_out += piece.to - piece.from + 1;
        }
    }
    if let Some(from) = gap_from {
        out.push_str(&format!("[lines {from} to {} are left out]\n", lines.len()));
    }
    Shown { text: out.trim_end().to_string(), lines: lines.len(), left_out }
}

/// Some lines of a text, from line `from` on (from 1), each with nothing cut out of it: for reading on in
/// what the short view left out.
pub fn lines_from(text: &str, from: usize, most_lines: usize, most_chars: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return "It printed nothing.".into();
    }
    // A line that is not there means the start: a model asked for line 1000000 of a file of three.
    let from = if (1..=lines.len()).contains(&from) { from } else { 1 };
    let (mut out, mut to) = (String::new(), from - 1);
    for line in lines[from - 1..].iter().take(most_lines) {
        if !out.is_empty() && out.chars().count() + line.chars().count() > most_chars {
            break;
        }
        out.push_str(line);
        out.push('\n');
        to += 1;
    }
    let rest = if to < lines.len() { format!(" More follows from line {}.", to + 1) } else { " That is the end of it.".to_string() };
    format!("Lines {from} to {to} of {}:\n{}\n({} lines in all.{rest})", lines.len(), out.trim_end(), lines.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_kept_as_a_terminal_would_show_it() {
        let raw = "\u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m demo v0.1.0\r\nDownloading 10%\rDownloading 55%\rDownloading 100%\n\u{1b}]0;a window title\u{7}done   \n\n";
        assert_eq!(as_a_terminal_shows(raw), "   Compiling demo v0.1.0\nDownloading 100%\ndone");
        assert_eq!(as_a_terminal_shows("plain\ntext"), "plain\ntext");
        assert_eq!(as_a_terminal_shows(""), "");
    }

    #[test]
    fn a_short_output_is_shown_whole() {
        let shown = shortened("ok\nall 3 tests passed", 500);
        assert_eq!(shown, Shown { text: "ok\nall 3 tests passed".into(), lines: 2, left_out: 0 });
    }

    #[test]
    fn of_a_long_output_what_went_wrong_and_the_ending_are_shown_and_runs_are_folded() {
        let mut text = String::from("Building the project\n");
        for i in 1..=300 {
            text.push_str(&format!("   Compiling crate-{i} v0.{i}.0\n"));
        }
        // Lines that are all different, so that something has to be left out.
        let words = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"];
        for i in 1..=300 {
            text.push_str(&format!("{} stage {i} went its own way, which is worth a line\n", words[i % 7]));
        }
        for i in 1..=400 {
            text.push_str(&format!("test tests::case_{i} ... ok and nothing more to say about it, at some length, so that the page is long\n"));
        }
        text.push_str(
            "test tests::the_sum ... FAILED\n---- tests::the_sum stdout ----\nthread 'tests::the_sum' panicked at src/lib.rs:12:5:\nassertion `left == right` failed\n  left: 4\n right: 5\n",
        );
        for i in 401..=800 {
            text.push_str(&format!("test tests::case_{i} ... ok and nothing more to say about it, at some length, so that the page is long\n"));
        }
        text.push_str("\ntest result: FAILED. 800 passed; 1 failed; 0 ignored\n");
        let shown = shortened(&text, 1500);
        assert_eq!(shown.lines, 1409);
        assert!(shown.text.chars().count() <= 1700, "{}", shown.text.chars().count());
        // What went wrong, and how it ended, are there, in the order they were printed.
        for needed in ["test tests::the_sum ... FAILED", "panicked at src/lib.rs:12:5:", "assertion `left == right` failed", "test result: FAILED. 800 passed; 1 failed"] {
            assert!(shown.text.contains(needed), "{needed} is missing from:\n{}", shown.text);
        }
        assert!(shown.text.find("the_sum ... FAILED") < shown.text.find("test result: FAILED"));
        // Three hundred lines alike are one line and a count.
        assert!(shown.text.contains("   Compiling crate-1 v0.1.0\n(… 298 more lines like these …)\n   Compiling crate-300 v0.300.0"), "{}", shown.text);
        // What was left out is marked by its line numbers, where it was left out.
        assert!(shown.text.contains("are left out]"), "{}", shown.text);
        assert!(shown.left_out > 1000 && shown.left_out < shown.lines);
    }

    #[test]
    fn with_nothing_wrong_the_ending_and_the_beginning_are_shown() {
        let text: String = (1..=500).map(|i| format!("{} of the story, told in its own words\n", ["once", "then", "later", "after", "at last"][i % 5].repeat(i % 3 + 1))).collect();
        let shown = shortened(&text, 600);
        let first = text.lines().next().unwrap();
        let last = text.lines().last().unwrap();
        assert!(shown.text.starts_with(first) && shown.text.ends_with(last), "{}", shown.text);
        assert!(shown.text.contains("[lines ") && shown.left_out > 400);
    }

    #[test]
    fn a_very_long_line_is_cut_in_the_short_view_only() {
        let text = format!("{}\n{}", "x".repeat(5000), "y".repeat(5000));
        let shown = shortened(&text, 1000);
        assert!(shown.text.lines().all(|line| line.chars().count() <= A_LINE_AT_MOST + 1), "{}", shown.text);
        assert!(lines_from(&text, 2, 10, 6000).contains(&"y".repeat(5000)), "read on, a line is whole");
    }

    #[test]
    fn the_rest_can_be_read_by_line_number() {
        let text: String = (1..=50).map(|i| format!("line {i}\n")).collect();
        assert_eq!(lines_from(&text, 48, 10, 1000), "Lines 48 to 50 of 50:\nline 48\nline 49\nline 50\n(50 lines in all. That is the end of it.)");
        let some = lines_from(&text, 10, 3, 1000);
        assert_eq!(some, "Lines 10 to 12 of 50:\nline 10\nline 11\nline 12\n(50 lines in all. More follows from line 13.)");
        assert!(lines_from(&text, 1_000_000, 2, 1000).starts_with("Lines 1 to 2 of 50:"), "a line that is not there means the start");
        assert!(lines_from(&text, 1, 100, 40).starts_with("Lines 1 to 5 of 50:"), "no more characters than fit");
        assert_eq!(lines_from("", 1, 10, 100), "It printed nothing.");
    }
}
