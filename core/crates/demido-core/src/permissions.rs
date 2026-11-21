//! Whether a use of a tool needs the person's yes.
//!
//! Decided here, in code, from two things only: how far the tool reaches and the level the person chose for
//! the session. A model is never asked whether it should be allowed something, and nothing a model reads
//! can change the answer (research notes 05 and 14: a model of this size cannot be the guard, and no model
//! is a dependable one).
//!
//! The four levels are the brief's (4.16). What each means today, with the tools there are today:
//!
//! | The tool…                       | Cautious | Balanced | Smart | Autonomous |
//! |---------------------------------|----------|----------|-------|------------|
//! | touches nothing outside Demido  | runs     | runs     | runs  | runs       |
//! | reads in the session's folder   | asks     | runs     | runs  | runs       |
//! | changes a file in the folder    | asks     | runs     | runs  | runs       |
//! | runs a program                  | asks     | asks     | asks  | runs       |
//!
//! Balanced lets a change to a file through, because it cannot leave the folder and what the file held
//! before is kept. It always asks before a program, **by the owner's decision** (2026-11-21, "Always ask
//! first"). A program is shut in a sandbox (`sandbox`) and the folder is saved before it runs
//! (`checkpoint`), so all it did can be put back; I had let Balanced run it unasked on that ground, and
//! on the first real try a model told to delete everything did so without a question. Being able to undo
//! is not the same as not being harmed. The request says whether the program is shut in and whether the
//! folder was saved, so the person knows what a yes risks. Smart is the same as Balanced until a second
//! look by a model exists.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::tools::Reach;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Asks before every use of a tool that reaches outside Demido.
    Cautious,
    /// Lets a model read and change files in the folder. Asks before every program.
    Balanced,
    /// As Balanced, until a second look by a model exists to decide when a program need not ask.
    Smart,
    /// Never asks.
    Autonomous,
}

pub const LEVELS: [&str; 4] = ["cautious", "balanced", "smart", "autonomous"];

impl Level {
    pub fn from(name: &str) -> Option<Self> {
        match name {
            "cautious" => Some(Self::Cautious),
            "balanced" => Some(Self::Balanced),
            "smart" => Some(Self::Smart),
            "autonomous" => Some(Self::Autonomous),
            _ => None,
        }
    }
}

/// What holds a program that is about to run. Only looked at for tools that run one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    /// It runs in a sandbox: it cannot leave the folder or reach the network.
    pub shut_in: bool,
    /// The folder was saved first: all the program does can be undone.
    pub can_be_undone: bool,
}

impl Held {
    pub const NOT_A_PROGRAM: Self = Self { shut_in: true, can_be_undone: true };
}

/// Whether a use of a tool goes ahead or waits for the person.
pub fn asks(level: Level, reach: Reach, held: Held) -> bool {
    match (reach, level) {
        (Reach::Nothing, _) => false,
        (_, Level::Autonomous) => false,
        (_, Level::Cautious) => true,
        (Reach::Reads | Reach::Changes, _) => false,
        // Always, however well the program is held: the owner's decision. What holds it is said in the request.
        (Reach::Runs, Level::Balanced | Level::Smart) => {
            let _ = held;
            true
        }
    }
}

/// Why a use of a tool is being asked for, in one line.
pub fn why(level: Level, reach: Reach, held: Held, no_sandbox_because: Option<&str>) -> String {
    match (level, reach) {
        (Level::Cautious, _) => "This session is set to Cautious: it asks before every use of a tool that reaches outside Demido.".into(),
        (_, Reach::Runs) if !held.shut_in => {
            format!("There is no sandbox on this computer, so this command could reach whatever you can reach. {}", no_sandbox_because.unwrap_or_default()).trim_end().to_string()
        }
        (_, Reach::Runs) if !held.can_be_undone => "It runs shut in a sandbox: it cannot leave the folder or reach the network. The folder could not be saved first, so what it changes in the folder cannot be undone.".into(),
        (_, Reach::Runs) => "A program can do anything inside the folder, so this level asks before each. It runs shut in a sandbox: it cannot leave the folder or reach the network. The folder was saved first, so everything it does can be undone.".into(),
        _ => "This could do something that cannot be undone.".into(),
    }
}

/// What the person can say to a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Said {
    /// Yes, this once.
    Once,
    /// Yes, and do not ask again for this tool in this session.
    Session,
    /// No.
    No,
}

/// A use of a tool that is waiting for the person's yes. Every word of it is made by Demido from the
/// request itself; none of it is written by the model.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Request {
    pub id: String,
    pub session_id: String,
    /// The answer the use is part of.
    pub message_id: String,
    pub tool: String,
    #[schema(value_type = Object)]
    pub args: serde_json::Value,
    pub reach: Reach,
    /// What would happen, in one line: "Replace the file notes.md".
    pub what: String,
    /// Why it is being asked, in one line.
    pub why: String,
    pub asked_at: i64,
}

/// What a use of a tool would do, in one plain line, made from the request itself.
pub fn what(tool: &str, args: &serde_json::Map<String, serde_json::Value>, exists: impl Fn(&str) -> bool) -> String {
    let text = |name: &str| args.get(name).and_then(|v| v.as_str()).unwrap_or_default();
    let short = |s: &str| {
        let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() <= 60 { flat } else { format!("{}…", flat.chars().take(60).collect::<String>()) }
    };
    match tool {
        "list_files" => "List the files in the folder".into(),
        "read_file" => format!("Read the file {}", text("path")),
        "search_files" => format!("Search the files for \"{}\"", short(text("text"))),
        "add_to_file" => format!("Add to the end of {}: \"{}\"", text("path"), short(text("text"))),
        "write_file" if exists(text("path")) => format!("Replace all of the file {} ({} characters)", text("path"), text("content").chars().count()),
        "write_file" => format!("Create the file {} ({} characters)", text("path"), text("content").chars().count()),
        "edit_file" => format!("Change {}: \"{}\" becomes \"{}\"", text("path"), short(text("find")), short(text("replace"))),
        // A command is shown whole: every character of it matters.
        "run_command" => format!("Run this command in the folder: {}", text("command")),
        other => format!("Use {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn what_asks_follows_from_the_reach_and_the_level_and_nothing_else() {
        use Level::*;
        let (safe, loose, unsaved) = (Held { shut_in: true, can_be_undone: true }, Held { shut_in: false, can_be_undone: true }, Held { shut_in: true, can_be_undone: false });
        for level in [Cautious, Balanced, Smart, Autonomous] {
            assert!(!asks(level, Reach::Nothing, loose), "the clock and the calculator never ask");
        }
        assert!(asks(Cautious, Reach::Reads, safe) && asks(Cautious, Reach::Changes, safe) && asks(Cautious, Reach::Runs, safe), "cautious asks for everything that reaches outside");
        for level in [Balanced, Smart, Autonomous] {
            assert!(!asks(level, Reach::Reads, loose) && !asks(level, Reach::Changes, loose));
        }
        // A program: balanced and smart always ask, however well it is held. The request says how well.
        for level in [Balanced, Smart] {
            assert!(asks(level, Reach::Runs, safe) && asks(level, Reach::Runs, loose) && asks(level, Reach::Runs, unsaved));
        }
        assert!(why(Balanced, Reach::Runs, safe, None).ends_with("The folder was saved first, so everything it does can be undone."));
        assert!(!asks(Autonomous, Reach::Runs, loose), "autonomous never asks");
        assert!(
            why(Balanced, Reach::Runs, loose, Some("bubblewrap is not installed.")).starts_with("There is no sandbox on this computer")
                && why(Balanced, Reach::Runs, loose, Some("bubblewrap is not installed.")).ends_with("bubblewrap is not installed.")
        );
        assert!(why(Smart, Reach::Runs, unsaved, None).ends_with("what it changes in the folder cannot be undone."));
        assert!(why(Cautious, Reach::Runs, safe, None).contains("Cautious"));
        assert_eq!(LEVELS.map(Level::from), [Some(Cautious), Some(Balanced), Some(Smart), Some(Autonomous)]);
        assert_eq!(Level::from("root"), None);
    }

    #[test]
    fn a_request_says_what_would_happen_in_words_made_from_the_request() {
        let said = |tool: &str, args: serde_json::Value| what(tool, args.as_object().unwrap(), |path| path == "old.md");
        assert_eq!(said("list_files", json!({})), "List the files in the folder");
        assert_eq!(said("read_file", json!({ "path": "todo.txt" })), "Read the file todo.txt");
        assert_eq!(said("search_files", json!({ "text": "invoice" })), "Search the files for \"invoice\"");
        assert_eq!(said("write_file", json!({ "path": "new.md", "content": "Hello." })), "Create the file new.md (6 characters)");
        assert_eq!(said("write_file", json!({ "path": "old.md", "content": "Hello." })), "Replace all of the file old.md (6 characters)");
        assert_eq!(said("add_to_file", json!({ "path": "todo.txt", "text": "Book the venue." })), "Add to the end of todo.txt: \"Book the venue.\"");
        assert_eq!(said("run_command", json!({ "command": "cargo test && rm -rf target" })), "Run this command in the folder: cargo test && rm -rf target");
        assert_eq!(said("edit_file", json!({ "path": "a.md", "find": "Friday\n  morning", "replace": "Monday" })), "Change a.md: \"Friday morning\" becomes \"Monday\"");
    }
}
