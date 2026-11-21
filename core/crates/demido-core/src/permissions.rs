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
//!
//! Balanced lets a change through because it is contained and can be undone: it cannot leave the folder,
//! and what a file held before is kept. Smart is the same as Balanced until there is something for a
//! judgement to be about: it differs once tools can run programs or reach the network, and says so.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::tools::Reach;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Asks before every use of a tool that reaches outside Demido.
    Cautious,
    /// Asks only for what could do damage that cannot be undone.
    Balanced,
    /// As Balanced, and a second look by a model decides the uncertain cases. No such cases exist yet.
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

/// Whether a use of a tool goes ahead or waits for the person.
pub fn asks(level: Level, reach: Reach) -> bool {
    match (reach, level) {
        (Reach::Nothing, _) => false,
        (Reach::Reads | Reach::Changes, Level::Cautious) => true,
        (Reach::Reads | Reach::Changes, _) => false,
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
        for level in [Cautious, Balanced, Smart, Autonomous] {
            assert!(!asks(level, Reach::Nothing), "the clock and the calculator never ask");
        }
        assert!(asks(Cautious, Reach::Reads) && asks(Cautious, Reach::Changes), "cautious asks for everything that reaches outside");
        for level in [Balanced, Smart, Autonomous] {
            assert!(!asks(level, Reach::Reads) && !asks(level, Reach::Changes));
        }
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
        assert_eq!(said("edit_file", json!({ "path": "a.md", "find": "Friday\n  morning", "replace": "Monday" })), "Change a.md: \"Friday morning\" becomes \"Monday\"");
    }
}
