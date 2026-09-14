//! Showing a model what is in one folder.
//!
//! The reading and the wording are deliberately in separate functions. What is
//! in a directory has one answer; how a page of it is said to something that
//! will act on it is the part that gets rewritten, and in v2 it was rewritten
//! three times by live runs while the reading never changed.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Context, Failure, Intent, Outcome, Tool};

/// How many entries a listing shows in one page.
///
/// A directory of fifty thousand entries has to stop somewhere, and past a
/// point the useful move is to look in one of the subdirectories rather than to
/// page through the rest.
const MOST_ENTRIES: usize = 150;

pub struct ListDirectory;

#[async_trait]
impl Tool for ListDirectory {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "from": { "type": "integer" },
            },
            "required": ["path"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, _: &Context<'_>) -> Intent {
        Intent::reading(format!(
            "List {}",
            arguments["path"].as_str().unwrap_or(".")
        ))
    }

    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome {
        let asked = arguments["path"].as_str().unwrap_or_default();
        let from = arguments["from"].as_u64().unwrap_or(1).max(1) as usize;
        // Both sentences a path that is not a directory earns are
        // `Context`'s, and shared with `search_files`: a name off by a
        // character is answered with what the parent does hold, which is what
        // a model recovers from.
        let at = context.resolve_dir(asked)?;

        let read = std::fs::read_dir(at.path())
            .map_err(|err| Failure::retryable(format!("{asked} could not be read: {err}")))?;

        let mut here: Vec<Entry> = read
            .flatten()
            .map(|entry| {
                let path = entry.path();
                Entry {
                    // A symlink's own type is not what it points at, and a
                    // linked directory reads as a directory to anyone opening
                    // it, so the question is asked of the target. Where the
                    // target cannot be reached, it is not a directory anybody
                    // can list.
                    directory: path.is_dir(),
                    name: entry.file_name().to_string_lossy().into_owned(),
                }
            })
            .collect();
        // In name order, so the same folder listed twice answers the same way
        // twice and the number under an entry means something across calls.
        here.sort_by(|one, other| one.name.cmp(&other.name));

        if here.is_empty() {
            return Ok(format!("{asked} is empty."));
        }
        if from > here.len() {
            return Err(Failure::retryable(format!(
                "{asked} has {} entries, so there is no entry {from}.",
                here.len()
            )));
        }

        Ok(page_of(asked, from, &here))
    }
}

/// One thing in a directory, as a listing says it.
struct Entry {
    name: String,
    directory: bool,
}

/// One page of a folder, as a model reads it.
///
/// Nothing here can fail, which is why it is separate: what the entries are is
/// the filesystem's question and has one answer, and how a page says where it
/// sits is this tool's and is the part that gets rewritten.
fn page_of(asked: &str, from: usize, here: &[Entry]) -> String {
    // Numbered, because the sentence under a cut listing tells a model to call
    // again `from` a number, and until v2 fixed it that number appeared nowhere
    // in the listing it was about. `read_file` says the same thing about a line
    // and gets away with it because its lines are numbered where the model can
    // see them. A folder of bare names left the model to count, and what a live
    // run watched instead was the number being read as part of a name: told to
    // carry on `from 151`, gemma-4-E4B opened `bay-0151-packing-straw.txt`.
    let shown: Vec<String> = here
        .iter()
        .skip(from - 1)
        .take(MOST_ENTRIES)
        .enumerate()
        .map(|(index, entry)| {
            let slash = match entry.directory {
                true => "/",
                false => "",
            };
            format!("{:>4}  {}{slash}", from + index, entry.name)
        })
        .collect();

    let last = from + shown.len() - 1;
    let names = shown.join("\n");
    let past = here.len() - last;
    if past == 0 {
        return names;
    }

    format!(
        "{names}\n[{past} more of the {} entries in {asked}. Call list_directory again with \
         path {asked} and from {}.]",
        here.len(),
        last + 1
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::workspace::Workspace;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    async fn list(arguments: Value) -> Outcome {
        let (_dir, workspace) = workspace();
        ListDirectory
            .run(&arguments, &Context::over(&workspace))
            .await
    }

    #[tokio::test]
    async fn a_directory_comes_back_numbered_with_its_folders_marked() {
        let listing = list(json!({ "path": "." })).await.unwrap();

        assert!(listing.contains("   1  README.md"), "{listing}");
        assert!(listing.contains("   2  src/"), "{listing}");
    }

    #[tokio::test]
    async fn a_file_says_which_tool_to_use_instead() {
        let failure = list(json!({ "path": "README.md" }))
            .await
            .expect_err("a file");
        assert!(failure.message.contains("read_file"), "{failure}");
    }

    #[tokio::test]
    async fn a_name_that_is_slightly_wrong_is_answered_with_what_is_there() {
        let failure = list(json!({ "path": "srcc" })).await.expect_err("no such");
        assert!(failure.message.contains("src"), "{failure}");
    }

    #[tokio::test]
    async fn a_directory_outside_the_workspace_is_refused_by_the_tool_that_was_asked() {
        let failure = list(json!({ "path": "../.." })).await.expect_err("outside");
        assert!(
            failure.message.contains("outside the workspace"),
            "{failure}"
        );
    }

    #[tokio::test]
    async fn an_empty_directory_says_so_rather_than_answering_with_nothing() {
        // An empty result is the one reply a model cannot act on.
        let (dir, workspace) = workspace();
        std::fs::create_dir(dir.path().join("empty")).unwrap();

        let listing = ListDirectory
            .run(&json!({ "path": "empty" }), &Context::over(&workspace))
            .await
            .unwrap();
        assert!(listing.contains("empty"), "{listing}");
    }

    #[tokio::test]
    async fn a_directory_too_large_for_one_page_says_how_to_ask_for_the_rest() {
        let (dir, workspace) = workspace();
        let many = dir.path().join("many");
        std::fs::create_dir(&many).unwrap();
        for n in 0..MOST_ENTRIES + 20 {
            std::fs::write(many.join(format!("bay-{n:04}.txt")), "").unwrap();
        }
        let context = Context::over(&workspace);

        let first = ListDirectory
            .run(&json!({ "path": "many" }), &context)
            .await
            .unwrap();
        assert!(first.contains("20 more of the 170 entries"), "{first}");
        assert!(first.contains("from 151"), "{first}");
        // And the number it names is the entry after the last one shown, so a
        // model paging through a folder does not lose one every page.
        assert!(first.contains(" 150  bay-0149.txt"), "{first}");

        let second = ListDirectory
            .run(&json!({ "path": "many", "from": 151 }), &context)
            .await
            .unwrap();
        assert!(second.contains(" 151  bay-0150.txt"), "{second}");
        assert!(!second.contains("more of the"), "{second}");
    }

    #[tokio::test]
    async fn an_entry_past_the_end_says_how_many_there_are() {
        let failure = list(json!({ "path": ".", "from": 99 }))
            .await
            .expect_err("no such entry");
        assert!(failure.message.contains("2 entries"), "{failure}");
    }
}
