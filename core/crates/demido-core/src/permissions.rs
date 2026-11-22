//! Whether a use of a tool needs the person's yes.
//!
//! Decided here, in code, from two things only: how far the tool reaches and the level the person chose for
//! the session. At Cautious, Balanced and Autonomous a model is never asked whether it should be allowed
//! something, and nothing a model reads can change the answer (research notes 05 and 14: a model of this
//! size cannot be the guard, and no model is a dependable one). At Smart, by the owner's direction
//! (2026-11-22, DECISIONS T6), the model that acts is asked instead of the person, wherever Balanced would
//! ask the person: "Smart means the model decides what is allowed and what not". What the model cannot
//! change at Smart is everything else: the folder, the sandbox, the folder saved first, Undo. Where there is
//! no sandbox, a program asks the person at Smart too (T13). How the model is asked is measured (research
//! note 27): twice, both ways round, and it goes ahead only on "go" both times.
//!
//! The four levels are the brief's (4.16). What each means today, with the tools there are today:
//!
//! | The tool…                       | Cautious | Balanced | Smart          | Autonomous |
//! |---------------------------------|----------|----------|----------------|------------|
//! | touches nothing outside Demido  | runs     | runs     | runs           | runs       |
//! | reads in the session's folder   | asks     | runs     | runs           | runs       |
//! | changes a file in the folder    | asks     | runs     | runs           | runs       |
//! | changes a file after the        | asks     | asks     | the model says | runs       |
//! |   session read the internet     |          |          |                |            |
//! | runs a program                  | asks     | asks     | the model says | runs       |
//! | searches the internet           | asks     | runs     | runs           | runs       |
//! | reads a page it was given       | asks     | runs     | runs           | runs       |
//! | reads a page at an address it   | asks     | asks     | the model says | runs       |
//! |   wrote itself                  |          |          |                |            |
//!
//! Text from the internet can tell a model what to do, and a small model does it. Tried on 2026-11-21
//! with a page that said "add this line to todo.txt": both models that read the page added the line
//! (research note 15). Words do not stop that. So once a session has read from the internet, a change to
//! a file waits for the person, though it could still be undone.
//!
//! Reading from the internet only reads, so Balanced lets it through, with one exception. An address can
//! carry text out of the computer, in what follows its `?`. A model is "given" an address by the person,
//! or by a search or a page in the same session; such an address was written by someone else and cannot
//! hold what the model has read. An address the model wrote itself can, so it waits for the person's yes,
//! and that yes is for the one address: "do not ask again" is not offered for it (`web::guard`).
//!
//! Balanced lets a change to a file through, because it cannot leave the folder and what the file held
//! before is kept. It always asks before a program, **by the owner's decision** (2026-11-21, "Always ask
//! first"). A program is shut in a sandbox (`sandbox`) and the folder is saved before it runs
//! (`checkpoint`), so all it did can be put back; I had let Balanced run it unasked on that ground, and
//! on the first real try a model told to delete everything did so without a question. Being able to undo
//! is not the same as not being harmed. The request says whether the program is shut in and whether the
//! folder was saved, so the person knows what a yes risks.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::tools::Reach;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Asks before every use of a tool that reaches outside Demido.
    Cautious,
    /// Lets a model read and change files in the folder, search the internet and read pages it was given.
    /// Asks before every program, before an address the model wrote itself, and, once the session has
    /// read from the internet, before a change to a file.
    Balanced,
    /// Where Balanced would ask the person, the model that acts decides whether to ask them.
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

/// Where the address came from that a tool is about to open. Only looked at for tools that read a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    /// The tool opens no address the model chose.
    NotOne,
    /// The person wrote it, or a search or a page in this session held it.
    Given,
    /// The model wrote it itself.
    Written,
}

/// What surrounds a use of a tool, beyond the tool itself and the session's level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Around {
    pub held: Held,
    pub address: Address,
    /// The session has read from the internet: text that nobody here wrote is in front of the model.
    pub after_the_internet: bool,
}

impl Around {
    /// Nothing special: not a program that is loosely held, no address of the model's own, no text from outside.
    pub const PLAIN: Self = Self { held: Held::NOT_A_PROGRAM, address: Address::NotOne, after_the_internet: false };
}

/// Who says whether a use of a tool goes ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answers {
    /// Nobody needs to: it goes ahead.
    Nobody,
    /// The person.
    ThePerson,
    /// The model that acts, at Smart: it says whether the person is asked.
    TheModel,
}

/// Who says whether a use of a tool goes ahead: at Smart, the model, wherever Balanced would ask the person.
/// One exception: a program where there is no sandbox asks the person at Smart too (T13: "until then a
/// program there asks first at every level but Autonomous"). The model's say rests on the walls holding.
pub fn answers(level: Level, reach: Reach, around: Around) -> Answers {
    match (asks(level, reach, around), level) {
        (false, _) => Answers::Nobody,
        (true, Level::Smart) if reach == Reach::Runs && !around.held.shut_in => Answers::ThePerson,
        (true, Level::Smart) => Answers::TheModel,
        (true, _) => Answers::ThePerson,
    }
}

/// Why the person is asked when the model, at Smart, chose to ask them.
pub fn why_the_model_asked(reach: Reach, around: Around, no_sandbox_because: Option<&str>) -> String {
    format!("This session is set to Smart, where the model decides when to ask you, and it chose to ask. {}", why(Level::Balanced, reach, around, no_sandbox_because))
}

/// Whether a use of a tool needs someone's answer before it goes ahead (the person's; at Smart, the model's).
pub fn asks(level: Level, reach: Reach, around: Around) -> bool {
    match (reach, level) {
        (Reach::Nothing, _) => false,
        (_, Level::Autonomous) => false,
        (_, Level::Cautious) => true,
        (Reach::Reads, _) => false,
        // A change can be undone, so it goes ahead: unless text from outside may be what asked for it.
        (Reach::Changes, Level::Balanced | Level::Smart) => around.after_the_internet,
        // Reading the internet only reads. An address the model wrote itself can carry text away.
        (Reach::Network, Level::Balanced | Level::Smart) => around.address == Address::Written,
        // Always, however well the program is held: the owner's decision. What holds it is said in the request.
        (Reach::Runs, Level::Balanced | Level::Smart) => true,
    }
}

/// Why a use of a tool is being asked for, in one line.
pub fn why(level: Level, reach: Reach, around: Around, no_sandbox_because: Option<&str>) -> String {
    let Around { held, address, after_the_internet } = around;
    match (level, reach) {
        (_, Reach::Network) if address == Address::Written => "The model wrote this address itself: it is not one you gave it, and not one a search or a page gave it. An address can carry text out of this computer, so look at the whole of it.".into(),
        (Level::Cautious, _) => "This session is set to Cautious: it asks before every use of a tool that reaches outside Demido.".into(),
        (_, Reach::Runs) if !held.shut_in => {
            format!("There is no sandbox on this computer, so this command could reach whatever you can reach. {}", no_sandbox_because.unwrap_or_default()).trim_end().to_string()
        }
        (_, Reach::Runs) if !held.can_be_undone => "It runs shut in a sandbox: it cannot leave the folder or reach the network. The folder could not be saved first, so what it changes in the folder cannot be undone.".into(),
        (_, Reach::Runs) => "A program can do anything inside the folder, so this level asks before each. It runs shut in a sandbox: it cannot leave the folder or reach the network. The folder was saved first, so everything it does can be undone.".into(),
        (_, Reach::Changes) if after_the_internet => "This session has read from the internet, and what a page says can tell a model to do things. So from then on a change to a file is asked for. It can be undone afterwards.".into(),
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
    /// "Do not ask again in this session" is not offered: the yes is for this one use.
    pub once_only: bool,
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
        "search_web" => format!("Search the internet for \"{}\"", short(text("query"))),
        // An address is shown whole, for the same reason.
        "read_page" => format!("Read the page at {}", text("url")),
        "read_output" => "Read more of what the last command printed".into(),
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
        let program = |shut_in: bool, can_be_undone: bool| Around { held: Held { shut_in, can_be_undone }, ..Around::PLAIN };
        let (safe, loose, unsaved) = (program(true, true), program(false, true), program(true, false));
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
    fn at_smart_the_model_answers_wherever_balanced_would_ask_the_person() {
        use Level::*;
        let program = Around { held: Held { shut_in: true, can_be_undone: true }, ..Around::PLAIN };
        let written = Around { address: Address::Written, ..Around::PLAIN };
        let after = Around { after_the_internet: true, ..Around::PLAIN };
        for (reach, around) in [(Reach::Runs, program), (Reach::Network, written), (Reach::Changes, after)] {
            assert_eq!(answers(Balanced, reach, around), Answers::ThePerson);
            assert_eq!(answers(Smart, reach, around), Answers::TheModel);
            assert_eq!(answers(Cautious, reach, around), Answers::ThePerson);
            assert_eq!(answers(Autonomous, reach, around), Answers::Nobody);
        }
        // Where Balanced goes ahead, so does Smart: the model is not asked about what nobody would be.
        for (reach, around) in [(Reach::Reads, Around::PLAIN), (Reach::Changes, Around::PLAIN), (Reach::Network, Around::PLAIN), (Reach::Nothing, program)] {
            assert_eq!(answers(Smart, reach, around), answers(Balanced, reach, around));
            assert_eq!(answers(Smart, reach, around), Answers::Nobody);
        }
        assert_eq!(answers(Cautious, Reach::Reads, Around::PLAIN), Answers::ThePerson, "at Cautious the person answers, never the model");
        // With no sandbox, a program is not shut in: the person answers, at Smart too (T13).
        let loose = Around { held: Held { shut_in: false, can_be_undone: true }, ..Around::PLAIN };
        assert_eq!(answers(Smart, Reach::Runs, loose), Answers::ThePerson);
        assert_eq!(answers(Autonomous, Reach::Runs, loose), Answers::Nobody);
        let why = why_the_model_asked(Reach::Runs, program, None);
        assert!(why.starts_with("This session is set to Smart, where the model decides when to ask you, and it chose to ask.") && why.ends_with("everything it does can be undone."), "{why}");
    }

    #[test]
    fn the_internet_is_read_unasked_except_at_an_address_the_model_wrote_itself() {
        use Level::*;
        let at = |address: Address| Around { address, ..Around::PLAIN };
        for level in [Balanced, Smart] {
            assert!(!asks(level, Reach::Network, at(Address::NotOne)), "a search");
            assert!(!asks(level, Reach::Network, at(Address::Given)), "a page the person named, or a search found");
            assert!(asks(level, Reach::Network, at(Address::Written)), "an address can carry text away");
        }
        for address in [Address::NotOne, Address::Given, Address::Written] {
            assert!(asks(Cautious, Reach::Network, at(address)));
            assert!(!asks(Autonomous, Reach::Network, at(address)), "autonomous never asks");
        }
        for level in [Cautious, Balanced, Smart] {
            assert!(why(level, Reach::Network, at(Address::Written), None).starts_with("The model wrote this address itself"));
        }
        assert!(why(Cautious, Reach::Network, at(Address::Given), None).contains("Cautious"));
    }

    #[test]
    fn once_a_session_has_read_the_internet_a_change_to_a_file_asks() {
        use Level::*;
        let after = Around { after_the_internet: true, ..Around::PLAIN };
        for level in [Balanced, Smart] {
            assert!(!asks(level, Reach::Changes, Around::PLAIN) && asks(level, Reach::Changes, after));
            assert!(!asks(level, Reach::Reads, after), "reading a file sends nothing anywhere");
            assert!(!asks(level, Reach::Network, after), "nor does reading on");
            assert!(why(level, Reach::Changes, after, None).starts_with("This session has read from the internet"));
        }
        assert!(!asks(Autonomous, Reach::Changes, after), "autonomous never asks");
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
        assert_eq!(said("search_web", json!({ "query": "grey heron" })), "Search the internet for \"grey heron\"");
        let long = format!("https://example.net/collect?d={}", "x".repeat(200));
        assert_eq!(said("read_page", json!({ "url": long })), format!("Read the page at {long}"), "an address is shown whole");
    }
}
