//! Finding a phrase in the workspace, without asking the machine for a `grep`.
//!
//! A model reaches for a shell search unprompted. Three of v2's live scenarios,
//! written for other things, watched gemma-4-E4B do it: asked which crate
//! something travelled in, its first move was `grep -r "brass astrolabe"
//! depot/`, ahead of the tree, ahead of `list_directory`, ahead of opening
//! anything. That is the right instinct, and answering it with whatever happens
//! to be on the machine's `PATH` means a turn that works because a developer
//! tool is installed and fails in front of the person this product is for.
//!
//! So the search is first-party, and everything else here follows from rules
//! the other tools already keep.
//!
//! **What comes back is a coordinate, not a file.** A match is a path, a line
//! number and the line, numbered exactly the way `read_file` numbers it, so the
//! next call is `read_file` with `from_line` and the two tools compose without
//! the model counting anything.
//!
//! **The walk is complete and the page is not.** The first sentence is an exact
//! count: how many lines hold the phrase and how many files they are in. That
//! is the number a model decides on, and what it decides is to narrow the
//! phrase rather than to page through eight thousand coordinates. An exact
//! count cannot be had without looking at every file, so the walk is not
//! bounded; what is bounded is what it keeps.
//!
//! **`.git` is not searched.** It is not part of the project, and a hit inside
//! a packed object is a coordinate nobody can act on. Nothing else is skipped
//! for being uninteresting: what cannot be read as text is counted and said
//! rather than passed over in silence, because a search that finds nothing and
//! a search that never looked are indistinguishable from the outside and a
//! model believes both.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Context, Failure, Intent, Outcome, Tool};

/// The one directory that is never searched. See the module note.
const NEVER_SEARCHED: &str = ".git";

/// How many matches come back in one page.
///
/// A hundred coordinates is more than a person would read and about as much as
/// a model can hold an intention across. Past that the useful move is a
/// narrower phrase, which the sentence under a cut page says.
const MOST_MATCHES: usize = 100;

/// How much of a matching line is shown.
///
/// A minified bundle is one line of two hundred thousand characters, and one
/// hit in it would otherwise be the whole page. The coordinate is the part a
/// model acts on and it survives; the rest is `read_file`'s to give.
const MOST_OF_A_LINE: usize = 200;

/// The largest file this reads in order to search it.
///
/// The same ceiling `read_file` refuses at, for the same reason and with a
/// different answer: reading is refused because the model could not use what
/// came back, and searching is skipped and counted because the model asked
/// about the project rather than about that file.
///
/// It is asked of the directory entry rather than of the bytes, which is the
/// difference between a ceiling and a ceiling that costs what it refuses. A
/// project carrying sixty four gigabytes under `target` would otherwise be read
/// in full to discover that none of it could be read.
const MOST_BYTES: usize = crate::files::MOST_BYTES;

pub struct SearchFiles;

#[async_trait]
impl Tool for SearchFiles {
    fn name(&self) -> &str {
        "search_files"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" },
                "path": { "type": "string" },
                "from": { "type": "integer" },
            },
            "required": ["text"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, _: &Context<'_>) -> Intent {
        Intent::reading(format!(
            "Search the project for {}",
            arguments["text"].as_str().unwrap_or("something")
        ))
    }

    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome {
        let text = arguments["text"].as_str().unwrap_or_default();
        if text.trim().is_empty() {
            return Err(Failure::retryable(format!(
                "{} needs some text to look for.",
                self.name()
            )));
        }
        let asked = arguments["path"].as_str().unwrap_or(".");
        let from = arguments["from"].as_u64().unwrap_or(1).max(1) as usize;

        // Both sentences a path that is not a directory earns are `Context`'s,
        // and shared with `list_directory`.
        let at = context.resolve_dir(asked)?;

        let mut swept = Swept::default();
        sweep(&mut swept, context, at.path(), &text.to_lowercase(), from);
        Ok(found(&swept, text, asked, from))
    }
}

/// One match, as the module note calls it: a coordinate, not a file.
///
/// A path, a line number and the line, numbered exactly the way `read_file`
/// numbers it, so the next call is `read_file` with `from_line` and the two
/// tools compose without the model counting anything.
#[derive(Debug)]
struct Coordinate {
    path: String,
    line: usize,
    matched: String,
}

/// Every match in a directory counted, the ones on the page kept, and what
/// could not be looked at.
///
/// The split is the module note's second rule made into a type: the counts are
/// of the whole directory and `shown` is only the page, so a phrase that
/// matches everywhere costs a hundred lines of memory rather than all of them.
#[derive(Debug, Default)]
struct Swept {
    /// The page, in the order the walk found it, beginning at the match that
    /// was asked for.
    shown: Vec<Coordinate>,
    /// How many matches there are in all, on the page or past it.
    matches: usize,
    /// How many files hold at least one. Counted as the walk goes rather than
    /// read off the matches afterwards, because the matches it would be read
    /// off are no longer all here.
    files: usize,
    /// Files that are not text, or are too large to read at once, or that lead
    /// out of the workspace. Counted rather than named: what a model can act on
    /// is that the sweep was not complete, and one line saying how many is the
    /// whole of that.
    unread: usize,
}

/// Walk a directory, reading every text file in it, counting the lines that
/// match and keeping the ones the page begins at.
///
/// Depth first and in name order, so the same project searched twice answers
/// the same way twice. `needle` arrives already lowercased, because lowering it
/// once per search rather than once per line is the difference between this
/// being usable on a repository and not.
///
/// **Every symlink is confined before it is followed.** A walk that started
/// inside the project stays inside it right up until it meets a link, and
/// `is_dir` follows one without saying so. This is the one place in the crate
/// where a path arrives from the filesystem rather than from the model, and it
/// goes through the workspace exactly as the model's paths do.
fn sweep(
    swept: &mut Swept,
    context: &Context<'_>,
    at: &std::path::Path,
    needle: &str,
    from: usize,
) {
    let Ok(read) = std::fs::read_dir(at) else {
        return;
    };

    let mut here: Vec<std::path::PathBuf> = read.flatten().map(|entry| entry.path()).collect();
    here.sort();

    for path in here {
        if path.file_name().is_some_and(|name| name == NEVER_SEARCHED) {
            continue;
        }

        let linked = path
            .symlink_metadata()
            .is_ok_and(|of| of.file_type().is_symlink());
        let inside = match linked {
            false => None,
            true => match context.confine(&path) {
                Ok(real) => Some(real),
                Err(_) => {
                    // A link out of the project, counted rather than followed
                    // and rather than passed over in silence.
                    swept.unread += 1;
                    continue;
                }
            },
        };
        let real = inside.as_ref().map_or(path.as_path(), |at| at.path());

        if real.is_dir() {
            sweep(swept, context, real, needle, from);
            continue;
        }

        if real.metadata().is_ok_and(|of| of.len() > MOST_BYTES as u64) {
            swept.unread += 1;
            continue;
        }
        let Ok(bytes) = std::fs::read(real) else {
            swept.unread += 1;
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            swept.unread += 1;
            continue;
        };
        let Ok(named) = context.confine(&path) else {
            swept.unread += 1;
            continue;
        };

        let relative = context.relative(&named);
        let mut counted_here = false;
        for (index, line) in text.lines().enumerate() {
            if !line.to_lowercase().contains(needle) {
                continue;
            }
            swept.matches += 1;
            if !counted_here {
                swept.files += 1;
                counted_here = true;
            }
            if swept.matches >= from && swept.shown.len() < MOST_MATCHES {
                swept.shown.push(Coordinate {
                    path: relative.clone(),
                    line: index + 1,
                    matched: shortened(line),
                });
            }
        }
    }
}

/// A number and the word for it.
///
/// "1 matches in 1 files" is the sort of sentence that makes a reader trust
/// everything around it slightly less, and every sentence this module produces
/// is read by something that will act on it.
fn counted(many: usize, one: &str, more: &str) -> String {
    match many {
        1 => format!("1 {one}"),
        many => format!("{many} {more}"),
    }
}

/// A matching line, cut to something a result can afford.
///
/// Whole characters rather than bytes, so a match in a line of Japanese does
/// not come back as a broken one.
fn shortened(line: &str) -> String {
    let trimmed = line.trim_end();
    match trimmed.chars().count() > MOST_OF_A_LINE {
        false => trimmed.to_owned(),
        true => {
            let kept: String = trimmed.chars().take(MOST_OF_A_LINE).collect();
            format!("{kept} ... (line continues)")
        }
    }
}

/// What a sweep looks like to a model.
///
/// Separate from the walking above it for the reason `listing::page_of` is:
/// what is in the project has one answer, and how it is said to something that
/// will act on it is the part that gets rewritten.
fn found(swept: &Swept, text: &str, asked: &str, from: usize) -> String {
    // `.` is what a model passes for the whole project and is not what anybody
    // calls it. Every sentence below is read by something that will act on it,
    // and "No line in . contains tallow" reads like a bug.
    let asked = match asked {
        "." | "" => "the workspace",
        named => named,
    };

    let unread = match swept.unread {
        0 => String::new(),
        many => format!(
            "\n[{} could not be searched as text.]",
            counted(many, "file", "files")
        ),
    };

    if swept.matches == 0 {
        return format!("No line in {asked} contains {text}.{unread}");
    }
    if from > swept.matches {
        return format!(
            "There are {} for {text} in {asked}, so there is no match {from}.{unread}",
            counted(swept.matches, "match", "matches")
        );
    }

    // Numbered from `from` rather than from one, because the sweep dropped what
    // came before it: the match a model sees at 101 is the hundred and first in
    // the project, and that number is the one it passes back.
    let shown: Vec<String> = swept
        .shown
        .iter()
        .enumerate()
        .map(|(index, at)| {
            format!(
                "{:>4}  {}:{}  {}",
                from + index,
                at.path,
                at.line,
                at.matched
            )
        })
        .collect();

    let last = from + shown.len() - 1;
    let past = swept.matches - last;
    let header = format!(
        "{} for {text}, in {}.",
        counted(swept.matches, "match", "matches"),
        counted(swept.files, "file", "files")
    );
    let more = match past {
        0 => String::new(),
        past => format!(
            "\n[{past} more. Call search_files again with the same text and from {}, or search \
             for something narrower.]",
            last + 1
        ),
    };

    format!("{header}\n{}{more}{unread}", shown.join("\n"))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::workspace::Workspace;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src").join("main.rs"),
            "fn main() {}\n// the brass astrolabe\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("README.md"),
            "# Hello\nA brass astrolabe.\n",
        )
        .unwrap();
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    async fn search(arguments: Value) -> Outcome {
        let (_dir, workspace) = workspace();
        SearchFiles
            .run(&arguments, &Context::over(&workspace))
            .await
    }

    #[tokio::test]
    async fn a_match_is_a_coordinate_read_file_can_take() {
        let answer = search(json!({ "text": "astrolabe" })).await.unwrap();

        assert!(answer.contains("2 matches"), "{answer}");
        assert!(answer.contains("in 2 files"), "{answer}");
        assert!(answer.contains("README.md:2"), "{answer}");
        assert!(answer.contains("src/main.rs:2"), "{answer}");
    }

    #[tokio::test]
    async fn the_phrase_is_matched_literally_and_ignoring_case() {
        let answer = search(json!({ "text": "BRASS Astrolabe" })).await.unwrap();
        assert!(answer.contains("2 matches"), "{answer}");
    }

    #[tokio::test]
    async fn nothing_found_says_so_in_words_about_the_project() {
        let answer = search(json!({ "text": "tallow" })).await.unwrap();
        assert_eq!(answer, "No line in the workspace contains tallow.");
    }

    #[tokio::test]
    async fn one_match_is_not_reported_as_one_matches() {
        let answer = search(json!({ "text": "Hello" })).await.unwrap();
        assert!(
            answer.starts_with("1 match for Hello, in 1 file."),
            "{answer}"
        );
    }

    #[tokio::test]
    async fn a_search_can_be_narrowed_to_a_directory() {
        let answer = search(json!({ "text": "astrolabe", "path": "src" }))
            .await
            .unwrap();

        assert!(answer.contains("1 match"), "{answer}");
        assert!(!answer.contains("README"), "{answer}");
    }

    #[tokio::test]
    async fn a_directory_outside_the_workspace_is_refused() {
        let failure = search(json!({ "text": "x", "path": "../.." }))
            .await
            .expect_err("outside");
        assert!(
            failure.message.contains("outside the workspace"),
            "{failure}"
        );
    }

    #[tokio::test]
    async fn nothing_to_look_for_is_refused_rather_than_matching_everything() {
        let failure = search(json!({ "text": "   " })).await.expect_err("no text");
        assert!(failure.retryable);
    }

    #[tokio::test]
    async fn more_matches_than_a_page_says_how_many_and_how_to_go_on() {
        let (dir, workspace) = workspace();
        let many: String = (0..MOST_MATCHES + 30).map(|_| "astrolabe\n").collect();
        std::fs::write(dir.path().join("ledger.txt"), many).unwrap();
        let context = Context::over(&workspace);

        let answer = SearchFiles
            .run(&json!({ "text": "astrolabe" }), &context)
            .await
            .unwrap();

        assert!(answer.contains("132 matches"), "{answer}");
        assert!(answer.contains("32 more"), "{answer}");
        assert!(answer.contains("from 101"), "{answer}");

        let rest = SearchFiles
            .run(&json!({ "text": "astrolabe", "from": 101 }), &context)
            .await
            .unwrap();
        assert!(rest.contains(" 101  "), "{rest}");
        assert!(!rest.contains("more."), "{rest}");
    }

    #[tokio::test]
    async fn a_file_that_is_not_text_is_counted_rather_than_passed_over_in_silence() {
        // A search that finds nothing and a search that never looked are
        // indistinguishable from the outside, and a model believes both.
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("blob"), [0xff, 0xfe, 0x00]).unwrap();

        let answer = SearchFiles
            .run(&json!({ "text": "tallow" }), &Context::over(&workspace))
            .await
            .unwrap();
        assert!(answer.contains("1 file could not be searched"), "{answer}");
    }

    #[tokio::test]
    async fn a_long_line_keeps_its_coordinate_and_loses_its_tail() {
        let (dir, workspace) = workspace();
        std::fs::write(
            dir.path().join("bundle.js"),
            format!("{}astrolabe{}", "x".repeat(500), "y".repeat(500)),
        )
        .unwrap();

        let answer = SearchFiles
            .run(
                &json!({ "text": "astrolabe", "path": "." }),
                &Context::over(&workspace),
            )
            .await
            .unwrap();

        assert!(answer.contains("bundle.js:1"), "{answer}");
        assert!(answer.contains("line continues"), "{answer}");
        assert!(!answer.contains(&"y".repeat(300)), "the tail survived");
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "making a symlink on Windows needs developer mode or elevation"
    )]
    async fn a_link_out_of_the_project_is_counted_rather_than_followed() {
        let (dir, workspace) = workspace();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("secret.txt"), "the astrolabe\n").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join("escape")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(elsewhere.path(), dir.path().join("escape")).unwrap();

        let answer = SearchFiles
            .run(&json!({ "text": "astrolabe" }), &Context::over(&workspace))
            .await
            .unwrap();

        assert!(
            !answer.contains("secret"),
            "the walk left the project: {answer}"
        );
        assert!(answer.contains("could not be searched"), "{answer}");
    }
}
