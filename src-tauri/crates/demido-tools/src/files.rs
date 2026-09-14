//! Reading, writing and deleting a file of the workspace.
//!
//! What all three are really about is failing usefully. A small model gets a
//! path slightly wrong constantly, and the difference between an agent that
//! recovers and one that spirals is whether "no such file" comes back as an
//! error code or as a sentence naming what is actually there.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tool::{Ability, Context, Failure, Intent, Outcome, Tool};
use crate::workspace::Resolved;

/// Most bytes `read_file` will return in one call.
///
/// A ceiling rather than a setting: this is not about taste, it is about a
/// model calling `read_file` on a lock file and losing its whole context to it.
pub(crate) const MOST_BYTES: usize = 200_000;

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "from_line": { "type": "integer" },
                "lines": { "type": "integer" },
            },
            "required": ["path"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, _: &Context<'_>) -> Intent {
        Intent::reading(format!("Read {}", asked_path(arguments)))
    }

    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome {
        let asked = asked_path(arguments);
        let at = context.resolve(asked)?;

        if at.path().is_dir() {
            return Err(Failure::retryable(format!(
                "{asked} is a directory. Use list_directory to see what is in it."
            )));
        }

        let bytes = std::fs::read(at.path()).map_err(|err| missing(context, asked, &err))?;
        if bytes.len() > MOST_BYTES {
            return Err(Failure::retryable(format!(
                "{asked} is {} bytes, which is too much to read at once. Ask for a range with \
                 from_line and lines.",
                bytes.len()
            )));
        }

        let Ok(text) = String::from_utf8(bytes) else {
            return Err(Failure::final_(format!(
                "{asked} is not a text file. There is no way to read it as one."
            )));
        };

        let from = arguments["from_line"].as_u64().unwrap_or(1).max(1) as usize;
        let count = arguments["lines"].as_u64().unwrap_or(u64::MAX) as usize;

        let all = text.lines().count();
        if from > all && all > 0 {
            return Err(Failure::retryable(format!(
                "{asked} has {all} lines, so there is no line {from}."
            )));
        }

        let numbered = numbered(&text, from, count);
        if numbered.is_empty() {
            return Ok(format!("{asked} is empty."));
        }
        Ok(numbered.join("\n"))
    }
}

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" },
            },
            "required": ["path", "content"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, context: &Context<'_>) -> Intent {
        let asked = asked_path(arguments);
        // A path that will not resolve names nothing, so nothing is declared
        // and nothing is written: `run` refuses it in a moment. Saying it
        // touches a file it cannot reach would be worse than saying nothing.
        let resolved = context.resolve(asked).ok();
        let existing = resolved.as_ref().is_some_and(|at| at.path().exists());

        Intent {
            ability: Ability::Write,
            summary: if existing {
                format!("Replace the contents of {asked}")
            } else {
                format!("Create {asked}")
            },
            // Whatever was there is declared in `touches`, so a copy can be
            // taken before the call and the write put back. That is what makes
            // writing something a Balanced profile can approve on its own, and
            // it is the whole of the difference between this and `delete_file`.
            destructive: false,
            touches: resolved.into_iter().collect(),
        }
    }

    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome {
        let asked = asked_path(arguments);
        let content = arguments["content"].as_str().unwrap_or_default();
        let at = context.resolve(asked)?;

        if at.path().is_dir() {
            return Err(Failure::retryable(format!(
                "{asked} is a directory, so it cannot be written as a file."
            )));
        }

        let existed = at.path().exists();
        replace(&at, content)
            .map_err(|err| Failure::retryable(format!("{asked} could not be written: {err}")))?;

        let lines = content.lines().count();
        Ok(if existed {
            format!("Replaced {asked}. It is now {lines} lines.")
        } else {
            format!("Created {asked}, {lines} lines.")
        })
    }
}

pub struct DeleteFile;

#[async_trait]
impl Tool for DeleteFile {
    fn name(&self) -> &str {
        "delete_file"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
            },
            "required": ["path"],
            "additionalProperties": false,
        })
    }

    fn intent(&self, arguments: &Value, context: &Context<'_>) -> Intent {
        let asked = asked_path(arguments);
        Intent {
            ability: Ability::Write,
            summary: format!("Delete {asked}"),
            // **The one declaration in this crate that is true.** Nothing else
            // knows that deleting is destructive: not the registry, not the
            // matrix, which reads this field rather than the tool's name, and
            // not the transcript. A destructive call asks in every mode
            // including Autonomous and cannot be waived by *always for this
            // tool*, and this line is the whole reason that happens.
            destructive: true,
            touches: context.resolve(asked).ok().into_iter().collect(),
        }
    }

    async fn run(&self, arguments: &Value, context: &Context<'_>) -> Outcome {
        let asked = asked_path(arguments);
        let at = context.resolve(asked)?;

        if at.path().is_dir() {
            return Err(Failure::retryable(format!(
                "{asked} is a directory, and this deletes one file at a time. Name a file in it."
            )));
        }
        if !at.path().exists() {
            return Err(Failure::retryable(context.absent(asked)));
        }

        std::fs::remove_file(at.path())
            .map_err(|err| Failure::retryable(format!("{asked} could not be deleted: {err}")))?;
        Ok(format!("Deleted {asked}."))
    }
}

/// The path a call named, or an empty string. The tools all take one and all
/// answer about it by name, so the reading happens in one place.
fn asked_path(arguments: &Value) -> &str {
    arguments["path"].as_str().unwrap_or_default()
}

/// Replace a file's contents, all at once.
///
/// Staged beside the destination and renamed over it, the way `demido-settings`
/// writes a profile: a process killed halfway through leaves the old file
/// whole rather than a truncated one. Beside the destination rather than in the
/// system temporary directory because a rename is only atomic within one
/// volume, and a workspace routinely sits on another one.
///
/// The parent directory is created if it is missing. A model writing the first
/// file of a directory it has just decided on is the ordinary case, and the
/// alternative is a failure it can only fix with a tool this group does not
/// have. Confinement is unaffected: the parent was resolved with the path.
fn replace(at: &Resolved, content: &str) -> std::io::Result<()> {
    if let Some(parent) = at.path().parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut staged = at.path().as_os_str().to_owned();
    staged.push(".part");
    let staged = std::path::PathBuf::from(staged);

    std::fs::write(&staged, content)?;
    match std::fs::rename(&staged, at.path()) {
        Ok(()) => Ok(()),
        Err(err) => {
            // The half-written file is not left in somebody's project. It is
            // the destination that failed, and the sidecar is Demido's mess.
            let _ = std::fs::remove_file(&staged);
            Err(err)
        }
    }
}

/// A file's lines, numbered from 1, from `from` and no more than `count` of
/// them.
///
/// Numbered, because the next thing anyone wants to do with a file is talk
/// about a particular line of it, and a model that has to count will get it
/// wrong.
fn numbered(text: &str, from: usize, count: usize) -> Vec<String> {
    text.lines()
        .enumerate()
        .skip(from.saturating_sub(1))
        .take(count)
        .map(|(index, line)| format!("{:>5}  {line}", index + 1))
        .collect()
}

/// What to say when a path could not be opened.
///
/// A missing path is answered with what is actually in the parent directory,
/// because the overwhelmingly common cause is a name off by a character or two
/// and the fix is one glance away. Anything else is reported as it happened:
/// inventing an explanation for a permissions error would send the model
/// somewhere useless.
fn missing(context: &Context<'_>, asked: &str, err: &std::io::Error) -> Failure {
    if err.kind() != std::io::ErrorKind::NotFound {
        return Failure::retryable(format!("{asked} could not be read: {err}"));
    }
    Failure::retryable(context.absent(asked))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

    use super::*;
    use crate::workspace::Workspace;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src").join("main.rs"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(dir.path().join("README.md"), "# Hello\n").unwrap();
        let workspace = Workspace::open(dir.path()).expect("a workspace");
        (dir, workspace)
    }

    async fn read(arguments: Value) -> Outcome {
        let (_dir, workspace) = workspace();
        ReadFile.run(&arguments, &Context::over(&workspace)).await
    }

    #[tokio::test]
    async fn a_file_comes_back_with_its_line_numbers() {
        // The next thing anyone does with a file is talk about a line of it,
        // and a model that has to count will get it wrong.
        let text = read(json!({ "path": "src/main.rs" })).await.unwrap();

        assert!(text.starts_with("    1  one"), "{text}");
        assert!(text.contains("    3  three"), "{text}");
    }

    #[tokio::test]
    async fn a_range_keeps_the_numbers_it_had_in_the_whole_file() {
        let text = read(json!({ "path": "src/main.rs", "from_line": 2, "lines": 1 }))
            .await
            .unwrap();
        assert_eq!(text, "    2  two");
    }

    #[tokio::test]
    async fn a_name_that_is_slightly_wrong_is_answered_with_what_is_there() {
        // The commonest failure a small model produces, and the one where the
        // fix is one glance away.
        let failure = read(json!({ "path": "src/mian.rs" }))
            .await
            .expect_err("no such file");

        assert!(failure.retryable);
        assert!(failure.message.contains("main.rs"), "{failure}");
    }

    #[tokio::test]
    async fn a_missing_file_at_the_root_names_the_root_rather_than_a_dot() {
        let failure = read(json!({ "path": "REDAME.md" }))
            .await
            .expect_err("no such file");

        assert!(failure.message.contains("the workspace root"), "{failure}");
        assert!(failure.message.contains("README.md"), "{failure}");
    }

    #[tokio::test]
    async fn reading_a_directory_says_which_tool_to_use_instead() {
        let failure = read(json!({ "path": "src" }))
            .await
            .expect_err("a directory");
        assert!(failure.message.contains("list_directory"), "{failure}");
    }

    #[tokio::test]
    async fn a_line_past_the_end_says_how_many_lines_there_are() {
        let failure = read(json!({ "path": "src/main.rs", "from_line": 99 }))
            .await
            .expect_err("no such line");
        assert!(failure.message.contains("3 lines"), "{failure}");
    }

    #[tokio::test]
    async fn something_that_is_not_text_is_refused_and_not_retried() {
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("blob"), [0xff, 0xfe, 0x00]).unwrap();

        let failure = ReadFile
            .run(&json!({ "path": "blob" }), &Context::over(&workspace))
            .await
            .expect_err("not text");

        assert!(!failure.retryable, "no wording of the arguments fixes this");
    }

    #[tokio::test]
    async fn a_file_too_large_to_read_says_how_to_ask_for_less() {
        let (dir, workspace) = workspace();
        std::fs::write(dir.path().join("huge"), "x".repeat(MOST_BYTES + 1)).unwrap();

        let failure = ReadFile
            .run(&json!({ "path": "huge" }), &Context::over(&workspace))
            .await
            .expect_err("too big");

        assert!(failure.retryable);
        assert!(failure.message.contains("from_line"), "{failure}");
    }

    #[tokio::test]
    async fn writing_replaces_the_whole_file_and_leaves_nothing_beside_it() {
        let (dir, workspace) = workspace();
        let answer = WriteFile
            .run(
                &json!({ "path": "README.md", "content": "# New\n" }),
                &Context::over(&workspace),
            )
            .await
            .unwrap();

        assert!(answer.contains("Replaced README.md"), "{answer}");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
            "# New\n"
        );
        // The staged file is renamed, never left in somebody's project.
        assert!(!dir.path().join("README.md.part").exists());
    }

    #[tokio::test]
    async fn writing_somewhere_new_creates_it_inside_the_workspace() {
        let (dir, workspace) = workspace();
        let answer = WriteFile
            .run(
                &json!({ "path": "src/new.rs", "content": "fn new() {}" }),
                &Context::over(&workspace),
            )
            .await
            .unwrap();

        assert!(answer.contains("Created src/new.rs"), "{answer}");
        assert!(dir.path().join("src").join("new.rs").exists());
    }

    #[test]
    fn a_write_declares_the_file_it_will_touch_so_a_copy_can_be_kept() {
        // A tool that writes a file it did not declare is a file that cannot be
        // put back, and that is the only way to get undo wrong.
        let (_dir, workspace) = workspace();
        let context = Context::over(&workspace);

        let intent = WriteFile.intent(&json!({ "path": "README.md", "content": "x" }), &context);

        assert_eq!(intent.ability, Ability::Write);
        assert!(!intent.destructive, "a copy is kept, so it can be undone");
        assert_eq!(intent.touches.len(), 1);
        // Compared through the workspace: the resolved path is the real one,
        // which on Windows wears an extended-length prefix the test never saw.
        assert_eq!(workspace.relative(&intent.touches[0]), "README.md");
        assert!(intent.summary.contains("Replace"), "{}", intent.summary);
    }

    #[test]
    fn creating_a_file_and_replacing_one_read_differently_in_the_prompt() {
        // The person deciding needs to know which of the two it is, and the
        // difference is not visible from the arguments.
        let (_dir, workspace) = workspace();
        let made = WriteFile.intent(
            &json!({ "path": "src/new.rs", "content": "x" }),
            &Context::over(&workspace),
        );

        assert!(made.summary.contains("Create"), "{}", made.summary);
        assert_eq!(made.touches.len(), 1, "the file it is about to make");
    }

    #[test]
    fn a_path_that_cannot_be_reached_is_said_to_touch_nothing() {
        // `run` refuses it in a moment. Claiming to touch a file it cannot
        // reach would have something taking a copy of a file outside.
        let (_dir, workspace) = workspace();
        let intent = WriteFile.intent(
            &json!({ "path": "../../elsewhere", "content": "x" }),
            &Context::over(&workspace),
        );

        assert!(intent.touches.is_empty());
    }

    #[test]
    fn reading_declares_itself_as_reading() {
        let (_dir, workspace) = workspace();
        let intent = ReadFile.intent(&json!({ "path": "README.md" }), &Context::over(&workspace));

        assert_eq!(intent.ability, Ability::Read);
        assert!(intent.touches.is_empty());
        assert!(!intent.destructive);
    }

    #[test]
    fn deleting_is_the_one_tool_here_that_declares_itself_destructive() {
        // The floor under every mode, including the one that approves
        // everything else, and this declaration is the only place it lives.
        let (_dir, workspace) = workspace();
        let intent = DeleteFile.intent(&json!({ "path": "README.md" }), &Context::over(&workspace));

        assert!(intent.destructive);
        assert_eq!(intent.ability, Ability::Write);
        assert_eq!(intent.touches.len(), 1, "what it is about to take away");
    }

    #[tokio::test]
    async fn deleting_removes_the_file_and_says_so() {
        let (dir, workspace) = workspace();
        let answer = DeleteFile
            .run(&json!({ "path": "README.md" }), &Context::over(&workspace))
            .await
            .unwrap();

        assert!(answer.contains("Deleted README.md"), "{answer}");
        assert!(!dir.path().join("README.md").exists());
    }

    #[tokio::test]
    async fn deleting_a_directory_is_refused_rather_than_taking_the_tree_with_it() {
        let (dir, workspace) = workspace();
        let failure = DeleteFile
            .run(&json!({ "path": "src" }), &Context::over(&workspace))
            .await
            .expect_err("a directory");

        assert!(failure.retryable, "{failure}");
        assert!(dir.path().join("src").join("main.rs").exists());
    }

    #[tokio::test]
    async fn deleting_something_that_is_not_there_says_what_is() {
        let (_dir, workspace) = workspace();
        let failure = DeleteFile
            .run(
                &json!({ "path": "src/mian.rs" }),
                &Context::over(&workspace),
            )
            .await
            .expect_err("no such file");

        assert!(failure.message.contains("main.rs"), "{failure}");
    }
}
