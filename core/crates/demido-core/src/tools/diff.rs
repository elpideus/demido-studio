//! What a change changed, as a person reads it: the lines that went and the lines that came, with a few
//! around them.
//!
//! Made at the moment of the change and kept with it, so that it says what that change did and not what
//! the file is like later. The diff is in the usual unified form (`-` for what went, `+` for what came),
//! which every programmer reads and every diff viewer draws.

use similar::{ChangeTag, TextDiff};

/// No more of a diff than this is kept: a change that rewrites a large file shows its start.
const MOST_KEPT: usize = 300_000;
/// A file larger than this is not compared line by line.
pub const MOST_COMPARED: usize = 1_000_000;

/// What a file held, as far as it is compared.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Bytes(Vec<u8>),
    /// Larger than `MOST_COMPARED`: only its size is known. Two of the same size count as the same.
    Large(u64),
}

/// One file that differs: its path, and what it held before and after (`None`: not there).
pub type Differs = (String, Option<Content>, Option<Content>);

/// A diff, and how much it adds and removes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    pub text: String,
    pub added: usize,
    pub removed: usize,
    pub files: usize,
}

impl Shown {
    /// In a few characters: `+3 −1`, or `2 files, +10 −4`.
    pub fn stat(&self) -> String {
        if self.files == 0 {
            return "nothing changed".into();
        }
        let files = if self.files == 1 { "1 file".to_string() } else { format!("{} files", self.files) };
        // A change that is not one of lines (a picture, a large file) has no lines to count.
        if self.added + self.removed == 0 {
            return files;
        }
        let lines = format!("+{} −{}", self.added, self.removed);
        if self.files > 1 { format!("{files}, {lines}") } else { lines }
    }
}

/// Whether bytes are text that can be shown line by line.
fn as_text(bytes: &[u8]) -> Option<&str> {
    if bytes.len() > MOST_COMPARED || bytes.contains(&0) {
        return None;
    }
    std::str::from_utf8(bytes).ok()
}

/// What happened to one file. `None` before: it was made. `None` after: it was removed.
pub fn of_file(path: &str, before: Option<&[u8]>, after: Option<&[u8]>) -> Shown {
    let (old_name, new_name) = (if before.is_some() { format!("a/{path}") } else { "/dev/null".into() }, if after.is_some() { format!("b/{path}") } else { "/dev/null".into() });
    let (old, new) = (before.unwrap_or_default(), after.unwrap_or_default());
    match (as_text(old), as_text(new)) {
        (Some(old), Some(new)) => {
            let diff = TextDiff::from_lines(old, new);
            let (mut added, mut removed) = (0, 0);
            for change in diff.iter_all_changes() {
                match change.tag() {
                    ChangeTag::Insert => added += 1,
                    ChangeTag::Delete => removed += 1,
                    ChangeTag::Equal => {}
                }
            }
            let text = if added + removed == 0 { format!("{path}: nothing changed in it.\n") } else { diff.unified_diff().context_radius(3).header(&old_name, &new_name).to_string() };
            Shown { text, added, removed, files: 1 }
        }
        // Not text, or too large to compare: what can be said is its size.
        _ => of_sizes(path, before.map(|b| b.len() as u64), after.map(|b| b.len() as u64)),
    }
}

/// What can be said of a file that is not shown line by line: its size before and after.
fn of_sizes(path: &str, before: Option<u64>, after: Option<u64>) -> Shown {
    let size = |bytes: Option<u64>| bytes.map_or("not there".to_string(), |b| format!("{b} bytes"));
    let text = format!("{path}: not shown line by line (not text, or larger than {} MB). Before: {}. After: {}.\n", MOST_COMPARED / 1_000_000, size(before), size(after));
    Shown { text, added: 0, removed: 0, files: 1 }
}

/// What happened to several files, as one diff.
pub fn of_files(changed: &[Differs]) -> Shown {
    let mut all = Shown::default();
    for (path, before, after) in changed {
        let one = match (before, after) {
            (Some(Content::Large(_)), _) | (_, Some(Content::Large(_))) => {
                let size = |c: &Option<Content>| {
                    c.as_ref().map(|c| match c {
                        Content::Bytes(b) => b.len() as u64,
                        Content::Large(n) => *n,
                    })
                };
                of_sizes(path, size(before), size(after))
            }
            _ => {
                let bytes = |c: &Option<Content>| match c {
                    Some(Content::Bytes(b)) => Some(b.clone()),
                    _ => None,
                };
                of_file(path, bytes(before).as_deref(), bytes(after).as_deref())
            }
        };
        (all.added, all.removed, all.files) = (all.added + one.added, all.removed + one.removed, all.files + 1);
        if all.text.len() < MOST_KEPT {
            all.text.push_str(&one.text);
        }
    }
    if all.text.len() > MOST_KEPT {
        let cut = (0..=MOST_KEPT).rev().find(|at| all.text.is_char_boundary(*at)).unwrap_or(0);
        all.text.truncate(cut);
        all.text.push_str("\n… (the rest of the diff is not kept)\n");
    }
    if changed.is_empty() {
        all.text = "Nothing in the folder changed.\n".into();
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_to_a_file_is_shown_as_the_lines_that_went_and_came() {
        let shown = of_file("todo.txt", Some(b"Buy stamps.\nCall Mira about Friday.\nWater the plants.\n"), Some(b"Buy stamps.\nCall Mira about Monday.\nWater the plants.\n"));
        assert_eq!((shown.added, shown.removed, shown.stat().as_str()), (1, 1, "+1 −1"));
        assert_eq!(shown.text, "--- a/todo.txt\n+++ b/todo.txt\n@@ -1,3 +1,3 @@\n Buy stamps.\n-Call Mira about Friday.\n+Call Mira about Monday.\n Water the plants.\n");
    }

    #[test]
    fn a_file_made_or_removed_is_all_added_or_all_removed() {
        let made = of_file("notes/new.md", None, Some(b"# New\nA line.\n"));
        assert_eq!((made.added, made.removed), (2, 0));
        assert!(made.text.starts_with("--- /dev/null\n+++ b/notes/new.md\n"), "{}", made.text);
        let gone = of_file("old.md", Some(b"one\n"), None);
        assert_eq!((gone.added, gone.removed), (0, 1));
        assert!(gone.text.starts_with("--- a/old.md\n+++ /dev/null\n"));
        assert_eq!(of_file("same.txt", Some(b"x\n"), Some(b"x\n")).text, "same.txt: nothing changed in it.\n");
    }

    #[test]
    fn what_is_not_text_is_said_in_words() {
        let shown = of_file("picture.png", Some(&[0x89, b'P', b'N', b'G', 0, 1]), Some(&[0x89, b'P', b'N', b'G', 0, 1, 2]));
        assert_eq!(shown.text, "picture.png: not shown line by line (not text, or larger than 1 MB). Before: 6 bytes. After: 7 bytes.\n");
    }

    #[test]
    fn several_files_are_one_diff_with_a_total() {
        let bytes = |b: &[u8]| Some(Content::Bytes(b.to_vec()));
        let all = of_files(&[
            ("a.txt".into(), bytes(b"1\n"), bytes(b"2\n")),
            ("b.txt".into(), None, bytes(b"new\nfile\n")),
            ("big.bin".into(), Some(Content::Large(2_000_000)), Some(Content::Large(3_000_000))),
        ]);
        assert_eq!((all.files, all.added, all.removed, all.stat().as_str()), (3, 3, 1, "3 files, +3 −1"));
        assert!(all.text.contains("+++ b/a.txt") && all.text.contains("+++ b/b.txt"));
        assert!(all.text.contains("big.bin: not shown line by line (not text, or larger than 1 MB). Before: 2000000 bytes. After: 3000000 bytes."), "{}", all.text);
        assert_eq!((of_files(&[]).text.as_str(), of_files(&[]).stat().as_str()), ("Nothing in the folder changed.\n", "nothing changed"));
    }
}
