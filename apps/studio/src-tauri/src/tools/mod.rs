//! The assistant's tools.
//!
//! Tools come in groups the person can switch on and off from the composer's Tools menu:
//! market data (Pine scripts and the chart among them), coding (Python and the terminal), workspace
//! files and skill authoring. Every call runs in the context of one chat, whose workspace folder
//! holds the files tools produce (market data CSVs, charts, downloads).
//!
//! A chat starts with none of the groups loaded: the system prompt lists them in a line each, and
//! the model loads the ones a request needs with `load_tools`. A group stays loaded for the rest
//! of the chat, which is read back from the chat's tool calls (see [`loaded_groups`]), so a
//! greeting does not carry thousands of tokens of tool schemas.

mod changes;
mod command;
mod files;
mod mail;
mod market;
mod pine;
mod python;
mod skills;

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::agent::{Approval, Cancelled, ToolRow};
use crate::db::{Message, Role};
use crate::llm::ToolSpec;
use crate::settings::Settings;
use crate::state::AppState;

/// A tool group as the Tools menu shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolGroup {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub enabled: bool,
    pub available: bool,
    pub reason: Option<String>,
    pub tools: Vec<&'static str>,
}

pub struct ToolContext {
    pub state: Arc<AppState>,
    pub chat_id: String,
    pub workspace: PathBuf,
    pub cancel: CancellationToken,
    /// Set when the person ends this call early; a tool that can stop keeps what it has.
    pub stop: CancellationToken,
    /// The call's chat row, shared with the agent loop.
    pub row: ToolRow,
}

impl ToolContext {
    /// Shows `display` on the call's card while the tool keeps running.
    pub fn set_display(&self, display: Value) {
        self.row.set_display(display);
    }

    /// Asks the person mid-run with `card` (see `agent::row`); `Err` when the turn is stopped.
    pub async fn request_approval(&self, card: Value) -> Result<Approval, Cancelled> {
        self.row.request_approval(Some(card), &self.cancel).await
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolOutput {
    pub ok: bool,
    /// What the model reads.
    pub content: String,
    /// What the UI shows.
    pub display: Value,
}

impl ToolOutput {
    pub fn ok(content: impl Into<String>, display: Value) -> Self {
        Self {
            ok: true,
            content: content.into(),
            display,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            ok: false,
            content: json!({ "error": message }).to_string(),
            display: json!({ "error": message }),
        }
    }
}

struct ToolDef {
    name: &'static str,
    group: &'static str,
    description: &'static str,
    parameters: fn() -> Value,
    /// Asks the person before running, unless they chose "always allow".
    approval: bool,
}

const TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "market_search",
        group: "market",
        description: "Find TradingView symbols for stocks, forex, crypto, indices and futures.",
        parameters: market::search_schema,
        approval: false,
    },
    ToolDef {
        name: "market_quote",
        group: "market",
        description: "Live prices of one or more symbols: last price, change, bid/ask and the day's range.",
        parameters: market::quote_schema,
        approval: false,
    },
    ToolDef {
        name: "market_candles",
        group: "market",
        description: "OHLCV candles of a symbol (needs a TradingView sign-in): recent ones from TradingView, older ones from the stored history. Saves every row to a CSV in the workspace and returns a summary with its path.",
        parameters: market::candles_schema,
        approval: false,
    },
    ToolDef {
        name: "market_history",
        group: "market",
        description: "Historical candles without a TradingView sign-in: years of forex, metals, commodities, indices and crypto from Dukascopy (stocks only from stored TradingView data). Saves a CSV in the workspace and returns a summary with its path.",
        parameters: market::history_schema,
        approval: false,
    },
    ToolDef {
        name: "market_download",
        group: "market",
        description: "Download a symbol's price history into the local store. Without from/to it fetches all the history the source has. Waits up to 90 seconds; a longer download goes on in the background with a progress bar in the chat.",
        parameters: market::download_schema,
        approval: false,
    },
    ToolDef {
        name: "market_data_status",
        group: "market",
        description: "Which market history is stored locally: per market and source (Dukascopy, or TradingView per timeframe), the date ranges covered, gaps, size on disk and running downloads.",
        parameters: market::data_status_schema,
        approval: false,
    },
    ToolDef {
        name: "chart_add_indicator",
        group: "market",
        description: "Put an indicator on the user's chart in the Market window (opening it if closed): a script from Demido's Pine library or a TradingView indicator.",
        parameters: pine::add_indicator_schema,
        approval: false,
    },
    ToolDef {
        name: "chart_draw",
        group: "market",
        description: "Draw on the user's chart in the Market window without writing an indicator, to show what you found: levels, trend lines, zones, labels, markers and series of values.",
        parameters: pine::draw_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_list",
        group: "market",
        description: "List the Pine scripts in Demido's library, and optionally the user's own scripts on TradingView.",
        parameters: pine::list_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_read",
        group: "market",
        description: "Read a library Pine script with line numbers and its compile errors and warnings, or first import a TradingView script into the library to read and change it.",
        parameters: pine::read_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_save",
        group: "market",
        description: "Save a whole Pine Script v6 indicator in Demido's library (not on TradingView) and compile it: returns errors and warnings with their lines, or the script's inputs and plots. Strategies and libraries can be saved but not run. To change part of a script, use pine_edit.",
        parameters: pine::save_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_edit",
        group: "market",
        description: "Replace old_string with new_string in a library script, then save and compile it: returns the errors and warnings left, with their lines, and the changed lines. Fix errors one change at a time. Nothing is saved to TradingView.",
        parameters: pine::edit_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_test",
        group: "market",
        description: "Run a Pine script on real bars of a symbol and timeframe as TradingView's chart does (needs a TradingView sign-in): returns compile or runtime errors with their lines, or each plot's latest values, ranges and signals, what it drew, and a CSV in the workspace with every bar's OHLCV and plot values.",
        parameters: pine::test_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_publish",
        group: "market",
        description: "Save a library script to the user's TradingView account (My scripts): a new script the first time, its next version afterwards. It must compile. The user is asked first.",
        parameters: pine::publish_schema,
        approval: false,
    },
    ToolDef {
        name: "mail_list",
        group: "mail",
        description: "List the newest messages of an email folder with sender, subject, date, unread state and a preview, and name the account's folders. Never marks anything as read.",
        parameters: mail::list_schema,
        approval: false,
    },
    ToolDef {
        name: "mail_search",
        group: "mail",
        description: "Search the whole mailbox on the mail server, not only what was downloaded. Returns the newest matches with their ids.",
        parameters: mail::search_schema,
        approval: false,
    },
    ToolDef {
        name: "mail_export",
        group: "mail",
        description: "Write many emails (up to 1000) with their whole text into a JSON Lines file in the workspace's mail folder, chosen by a mail_search query, a date range, both, or neither for the newest. Returns the file's path and a summary, not the emails. Use it to work through more than a handful of emails.",
        parameters: mail::export_schema,
        approval: false,
    },
    ToolDef {
        name: "mail_read",
        group: "mail",
        description: "Read one email by id: sender, recipients, date, subject, whole text and attachment list. Does not mark it as read.",
        parameters: mail::read_schema,
        approval: false,
    },
    ToolDef {
        name: "mail_attachment",
        group: "mail",
        description: "Save an email's attachment in the workspace's mail folder and return its path, for read_file or run_python.",
        parameters: mail::attachment_schema,
        approval: false,
    },
    ToolDef {
        name: "run_python",
        group: "coding",
        description: "Run Python 3 in the chat's workspace folder, where files open by their relative path (numpy, pandas, matplotlib and requests are installed). Print what you need to see. Charts saved as PNG files are shown to the user. Every run is a fresh process.",
        parameters: python::schema,
        approval: true,
    },
    ToolDef {
        name: "run_command",
        group: "coding",
        description: "Run a command line in the user's own shell on their computer and get its output and exit code: for the programs they installed (yt-dlp, ffmpeg, git, winget…) and questions about the computer. The user approves each command. It runs in the chat's workspace folder (so downloads land there) unless directory is given.",
        parameters: command::schema,
        approval: true,
    },
    ToolDef {
        name: "list_files",
        group: "files",
        description: "List the files in the chat's workspace folder.",
        parameters: files::list_schema,
        approval: false,
    },
    ToolDef {
        name: "read_file",
        group: "files",
        description: "Read a workspace file as text: CSV, JSON, Markdown and code, and also PDF, Word, PowerPoint, Excel and web pages.",
        parameters: files::read_schema,
        approval: false,
    },
    ToolDef {
        name: "search_files",
        group: "files",
        description: "Search the files attached to this chat for passages about something; returns the best ones with their file and page. Matches by meaning and by words (words only while indexing): when nothing is found, try other words, synonyms or fewer words.",
        parameters: files::search_schema,
        approval: false,
    },
    ToolDef {
        name: "write_file",
        group: "files",
        description: "Create or overwrite a text file in the chat's workspace folder.",
        parameters: files::write_schema,
        approval: false,
    },
    ToolDef {
        name: "create_skill",
        group: "skills",
        description: "Save what you did as a reusable skill when the user asks: future conversations list it by its description and read its instructions when it applies. Write numbered steps naming the tools and parameters to use; put Python code in files, run with run_python's file as skill:<skill-id>/<file>.",
        parameters: skills::create_schema,
        approval: false,
    },
    ToolDef {
        name: "read_skill_file",
        group: "skills",
        description: "Read a skill's instructions (SKILL.md, the default) or another of its files.",
        parameters: skills::read_schema,
        approval: false,
    },
    ToolDef {
        name: LOADER,
        group: "",
        description: "Load groups of tools the system prompt lists, to use their tools from your next step on.",
        parameters: || load_schema(&GROUPS.iter().map(|g| g.id).collect::<Vec<_>>()),
        approval: false,
    },
];

/// The tool that loads the others.
const LOADER: &str = "load_tools";
/// Tools offered without loading a group: the loader, and reading the skills the system prompt
/// lists (with skill authoring off too).
const ALWAYS: [&str; 2] = [LOADER, "read_skill_file"];

struct GroupDef {
    id: &'static str,
    /// The Tools menu's name and description.
    label: &'static str,
    description: &'static str,
    /// What the model reads in the system prompt, to know when to load the group.
    for_model: &'static str,
}

const GROUPS: &[GroupDef] = &[
    GroupDef {
        id: "market",
        label: "Market data",
        description: "Live prices from TradingView, history from Dukascopy, and Pine indicators",
        for_model: "prices, quotes and price history of stocks, forex, crypto, indices and commodities; \
                    indicators and drawings on the user's chart (Market window); Pine Script",
    },
    GroupDef {
        id: "mail",
        label: "Email",
        description: "Read the email of the accounts connected in the Mail window",
        for_model: "the user's email: list, search, read and export messages, save attachments",
    },
    GroupDef {
        id: "coding",
        label: "Coding",
        description: "Run Python analysis and the programs installed on this computer",
        for_model: "run Python (data analysis, charts, file conversion) and command lines on the user's computer",
    },
    GroupDef {
        id: "files",
        label: "Workspace files",
        description: "Read and write files in the chat's folder, and search attached files",
        for_model: "list, read and write files in the chat's workspace folder; search attached files",
    },
    GroupDef {
        id: "skills",
        label: "Skill authoring",
        description: "Let the assistant save what it did as a skill",
        for_model: "save what you did as a reusable skill, when the user asks",
    },
];

fn load_schema(groups: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": {
            "groups": {"type": "array", "items": {"type": "string", "enum": groups}},
        },
        "required": ["groups"],
    })
}

/// Offered while any of its tools can run. The reason says what is missing, also when the rest of
/// the group still runs.
fn group_availability(state: &AppState, group: &str) -> (bool, Option<String>) {
    if group == "market" {
        let s = state.market.status();
        return (s.available, s.reason);
    }
    if group == "mail" {
        let connected = state.mail.has_accounts();
        return (
            connected,
            (!connected).then(|| "Connect an email account in the Mail window first.".to_string()),
        );
    }
    let tools: Vec<&ToolDef> = TOOLS.iter().filter(|t| t.group == group).collect();
    let missing: Vec<String> = tools.iter().filter_map(|t| tool_missing(state, t.name)).collect();
    let available = missing.len() < tools.len();
    (available, (!missing.is_empty()).then(|| missing.join(" ")))
}

/// Why one tool cannot run on this computer.
fn tool_missing(state: &AppState, name: &str) -> Option<String> {
    match name {
        "run_python" if state.paths.python().is_none() => {
            Some("Python is not installed. Run the installer again to add it.".into())
        }
        // Offered until the search for a shell (at startup) finds none.
        "run_command" if crate::shell::missing() => Some("No shell was found on this computer.".into()),
        _ => None,
    }
}

pub fn groups(state: &AppState) -> Vec<ToolGroup> {
    let settings = state.settings.get();
    GROUPS
        .iter()
        .map(|g| {
            let (available, reason) = group_availability(state, g.id);
            ToolGroup {
                id: g.id,
                label: g.label,
                description: g.description,
                enabled: settings.tool_group_enabled(g.id),
                available,
                reason,
                tools: TOOLS.iter().filter(|t| t.group == g.id).map(|t| t.name).collect(),
            }
        })
        .collect()
}

/// Why the model cannot load a group now: turned off in the Tools menu, or nothing in it can run
/// on this computer. `None` when it can.
fn unready(state: &AppState, settings: &Settings, group: &GroupDef) -> Option<String> {
    if !settings.tool_group_enabled(group.id) {
        return Some(format!(
            "The {} tools are turned off in the Tools menu; the user can turn them on there.",
            group.label
        ));
    }
    match group_availability(state, group.id) {
        (true, _) => None,
        (false, reason) => {
            Some(reason.unwrap_or_else(|| format!("The {} tools cannot run on this computer.", group.label)))
        }
    }
}

/// The groups the model can load and has not, as the system prompt lists them: id and what they
/// are for.
pub fn loadable(state: &AppState, settings: &Settings, loaded: &HashSet<&str>) -> Vec<(&'static str, &'static str)> {
    GROUPS
        .iter()
        .filter(|g| !loaded.contains(g.id) && unready(state, settings, g).is_none())
        .map(|g| (g.id, g.for_model))
        .collect()
}

/// Tool specs offered to the model: those of the `loaded` groups that can run, the loader while
/// there is more to load, and reading skills while any is in use.
pub fn specs(state: &AppState, settings: &Settings, loaded: &HashSet<&str>) -> Vec<ToolSpec> {
    let loadable: Vec<&str> = loadable(state, settings, loaded)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let ready: HashSet<&str> = GROUPS
        .iter()
        .filter(|g| loaded.contains(g.id) && unready(state, settings, g).is_none())
        .map(|g| g.id)
        .collect();
    TOOLS
        .iter()
        .filter_map(|t| {
            let parameters = match t.name {
                // The system prompt lists skills without their instructions (see
                // `SkillRegistry::prompt_section`), so reading them is offered while any skill is in
                // use, also with skill authoring off.
                "read_skill_file" if state.skills.usable().is_empty() => return None,
                LOADER if loadable.is_empty() => return None,
                // Only the groups left to load, so the model cannot ask for any other.
                LOADER => load_schema(&loadable),
                name if ALWAYS.contains(&name) => (t.parameters)(),
                name if !ready.contains(t.group) || tool_missing(state, name).is_some() => return None,
                _ => (t.parameters)(),
            };
            Some(ToolSpec {
                name: t.name.to_string(),
                description: t.description.to_string(),
                parameters,
            })
        })
        .collect()
}

/// Names of the tools the model is offered or can load, for hints that name tools.
pub fn usable_names(state: &AppState, settings: &Settings) -> HashSet<String> {
    let all: HashSet<&str> = GROUPS.iter().map(|g| g.id).collect();
    specs(state, settings, &all).into_iter().map(|t| t.name).collect()
}

/// The groups a chat has loaded: those its `load_tools` calls named, and those of any other tool
/// it called. All of the chat counts, also what a summary replaced, so a group once loaded stays
/// loaded and the tools sent with each request change only when one is added.
pub fn loaded_groups(messages: &[Message]) -> HashSet<&'static str> {
    let mut loaded = HashSet::new();
    let calls = messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .flat_map(|m| &m.tool_calls);
    for call in calls {
        if call.name == LOADER {
            let args = crate::llm::parse_arguments(&call.arguments).unwrap_or_default();
            loaded.extend(requested_groups(&args).iter().filter_map(|g| group_of(g)).map(|g| g.id));
        } else if let Some(t) = TOOLS.iter().find(|t| t.name == call.name && !ALWAYS.contains(&t.name)) {
            loaded.insert(t.group);
        }
    }
    loaded
}

/// The groups a `load_tools` call asks for, as written: `groups` as a list (or a string), or a
/// single `group`.
fn requested_groups(args: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for key in ["groups", "group"] {
        match &args[key] {
            Value::String(s) => out.extend(s.split([',', ' ']).map(str::to_string)),
            Value::Array(items) => out.extend(items.iter().filter_map(Value::as_str).map(str::to_string)),
            _ => {}
        }
    }
    out.iter()
        .map(|g| g.trim().to_ascii_lowercase())
        .filter(|g| !g.is_empty())
        .collect()
}

/// The group named `name`, or the group of the tool named `name`.
fn group_of(name: &str) -> Option<&'static GroupDef> {
    let id = match TOOLS.iter().find(|t| t.name == name && !t.group.is_empty()) {
        Some(t) => t.group,
        None => name,
    };
    GROUPS.iter().find(|g| g.id == id)
}

/// Why the model may not run `name` now: there is no such tool, or its group is turned off or
/// cannot run on this computer. A tool of a group not loaded yet runs, which loads the group.
pub fn refusal(state: &AppState, settings: &Settings, name: &str) -> Option<String> {
    let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
        return Some(format!(
            "There is no tool called {name}. Use only the tools you were given."
        ));
    };
    if ALWAYS.contains(&tool.name) {
        return None;
    }
    let group = GROUPS.iter().find(|g| g.id == tool.group)?;
    unready(state, settings, group).or_else(|| tool_missing(state, name))
}

/// Loads the groups asked for: they are in the tools of the next request, and from then on, since
/// [`loaded_groups`] reads this call back from the chat.
fn load(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let settings = ctx.state.settings.get();
    let names = || GROUPS.iter().map(|g| g.id).collect::<Vec<_>>().join(", ");
    let asked = requested_groups(args);
    if asked.is_empty() {
        return Err(format!("Name the groups to load in \"groups\": {}.", names()));
    }
    let mut loaded: Vec<&GroupDef> = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    for name in asked {
        let Some(group) = group_of(&name) else {
            problems.push(format!(
                "There is no tool group called {name}; the groups are {}.",
                names()
            ));
            continue;
        };
        match unready(&ctx.state, &settings, group) {
            Some(why) => problems.push(why),
            None if !loaded.iter().any(|g| g.id == group.id) => loaded.push(group),
            None => {}
        }
    }
    if loaded.is_empty() {
        return Err(problems.join(" "));
    }
    let tools: serde_json::Map<String, Value> = loaded
        .iter()
        .map(|g| {
            let names: Vec<&str> = TOOLS
                .iter()
                .filter(|t| t.group == g.id && tool_missing(&ctx.state, t.name).is_none())
                .map(|t| t.name)
                .collect();
            (g.id.to_string(), json!(names))
        })
        .collect();
    let mut content = json!({"loaded": tools, "next": "Use these tools now, as the request needs."});
    if !problems.is_empty() {
        content["not_loaded"] = json!(problems);
    }
    let labels: Vec<&str> = loaded.iter().map(|g| g.label).collect();
    Ok(ToolOutput::ok(content.to_string(), json!({ "groups": labels })))
}

/// Finds what the system prompt says about the tools: which shell commands run in. The first
/// time, on Windows, that takes a second or two.
pub async fn prepare(settings: &Settings) {
    if settings.tool_group_enabled("coding") {
        let _ = tokio::task::spawn_blocking(crate::shell::detect).await;
    }
}

pub fn needs_approval(name: &str, settings: &Settings) -> bool {
    TOOLS.iter().find(|t| t.name == name).is_some_and(|t| t.approval) && !settings.always_allowed_tools.contains(name)
}

/// A short present-tense label for the UI, e.g. `Fetching FX:EURUSD 1h candles`.
pub fn describe(name: &str, args: &Value) -> String {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    match name {
        "market_search" => format!("Searching markets for “{}”", s("query")),
        "market_quote" => {
            let symbols: Vec<String> = args["symbols"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default();
            format!("Getting live prices for {}", symbols.join(", "))
        }
        "market_candles" => format!("Fetching {} {} candles", s("symbol"), s("timeframe")),
        "market_history" => format!("Reading {} {} history", s("instrument"), s("timeframe")),
        "market_download" => match (s("from"), s("to")) {
            (from, to) if from.is_empty() && to.is_empty() => format!("Downloading all {} history", s("symbol")),
            (from, to) if to.is_empty() => format!("Downloading {} history from {from}", s("symbol")),
            (from, to) if from.is_empty() => format!("Downloading {} history up to {to}", s("symbol")),
            (from, to) => format!("Downloading {} history {from} → {to}", s("symbol")),
        },
        "market_data_status" => match s("symbol") {
            symbol if symbol.is_empty() => "Checking stored market data".into(),
            symbol => format!("Checking stored data for {symbol}"),
        },
        "chart_add_indicator" => match s("symbol") {
            symbol if symbol.is_empty() => format!("Adding {} to the chart", s("script")),
            symbol => format!("Adding {} to the {symbol} chart", s("script")),
        },
        "chart_draw" => {
            if args["items"].as_array().is_some_and(|a| a.is_empty()) {
                format!("Removing the drawings “{}”", s("name"))
            } else {
                format!("Drawing “{}” on the chart", s("name"))
            }
        }
        "pine_list" => "Listing Pine scripts".into(),
        "pine_read" => match s("tradingview_id") {
            tv if tv.is_empty() => "Reading a Pine script".into(),
            _ => "Importing a Pine script from TradingView".into(),
        },
        "pine_save" => match pine_title(&s("source")) {
            Some(title) => format!("Saving the Pine script “{title}”"),
            None => "Saving a Pine script".into(),
        },
        "pine_edit" => "Editing a Pine script".into(),
        "pine_test" => format!("Testing a Pine script on {} {}", s("symbol"), s("timeframe")),
        "pine_publish" => "Saving a Pine script to TradingView".into(),
        "mail_list" | "mail_search" | "mail_export" => {
            let what = match (name, s("folder"), s("query")) {
                ("mail_list", folder, _) if folder.is_empty() => "Checking the inbox".to_string(),
                ("mail_list", folder, _) => format!("Listing the {folder} folder"),
                ("mail_search", _, query) => format!("Searching email for “{query}”"),
                (_, _, query) if query.is_empty() => "Exporting emails".into(),
                (_, _, query) => format!("Exporting emails matching “{query}”"),
            };
            match s("account") {
                account if account.is_empty() => what,
                account => format!("{what} · {account}"),
            }
        }
        "mail_read" => "Reading an email".into(),
        "mail_attachment" => format!("Saving the attachment {}", s("attachment")),
        "run_python" => {
            if s("file").is_empty() {
                "Running Python".into()
            } else {
                format!("Running {}", s("file"))
            }
        }
        "run_command" => format!("Running {}", command::one_line(&s("command"), 60)),
        "list_files" => "Listing workspace files".into(),
        "read_file" => match s("pages") {
            pages if pages.is_empty() => format!("Reading {}", s("path")),
            pages => format!("Reading {}, pages {pages}", s("path")),
        },
        "search_files" => format!("Searching attached files for “{}”", s("query")),
        "write_file" => format!("Writing {}", s("path")),
        "create_skill" => format!("Creating the skill “{}”", s("name")),
        "read_skill_file" => format!("Reading skill {}", s("skill")),
        LOADER => {
            let mut labels: Vec<&str> = requested_groups(args)
                .iter()
                .filter_map(|g| group_of(g))
                .map(|g| g.label)
                .collect();
            labels.dedup();
            match labels.as_slice() {
                [] => "Loading tools".into(),
                _ => format!("Loading tools: {}", labels.join(", ")),
            }
        }
        other => format!("Running {other}"),
    }
}

pub async fn run(name: &str, args: Value, ctx: &ToolContext) -> ToolOutput {
    if !args.is_object() {
        return ToolOutput::error("Arguments must be a JSON object.");
    }
    let result = match name {
        "market_search" => market::search(ctx, &args).await,
        "market_quote" => market::quote(ctx, &args).await,
        "market_candles" => market::candles(ctx, &args).await,
        "market_history" => market::history(ctx, &args).await,
        "market_download" => market::download(ctx, &args).await,
        "market_data_status" => market::data_status(ctx, &args).await,
        "chart_add_indicator" => pine::add_indicator(ctx, &args).await,
        "chart_draw" => pine::draw(ctx, &args).await,
        "pine_list" => pine::list(ctx, &args).await,
        "pine_read" => pine::read(ctx, &args).await,
        "pine_save" => pine::save(ctx, &args).await,
        "pine_edit" => pine::edit(ctx, &args).await,
        "pine_test" => pine::test(ctx, &args).await,
        "pine_publish" => pine::publish(ctx, &args).await,
        "mail_list" => mail::list(ctx, &args).await,
        "mail_search" => mail::search(ctx, &args).await,
        "mail_export" => mail::export(ctx, &args).await,
        "mail_read" => mail::read(ctx, &args).await,
        "mail_attachment" => mail::attachment(ctx, &args).await,
        "run_python" => python::run(ctx, &args).await,
        "run_command" => command::run(ctx, &args).await,
        "list_files" => files::list(ctx, &args),
        "read_file" => files::read(ctx, &args).await,
        "search_files" => files::search(ctx, &args).await,
        "write_file" => files::write(ctx, &args),
        "create_skill" => skills::create(ctx, &args),
        "read_skill_file" => skills::read(ctx, &args),
        LOADER => load(ctx, &args),
        other => Err(format!("There is no tool called {other}.")),
    };
    result.unwrap_or_else(ToolOutput::error)
}

/// The title a Pine source declares (`indicator("Title", …)`), for labels.
fn pine_title(source: &str) -> Option<String> {
    let line = source
        .lines()
        .map(str::trim_start)
        .find(|l| ["indicator", "strategy", "library", "study"].iter().any(|k| l.starts_with(k)))?;
    let start = line.find(['"', '\''])?;
    let quote = line[start..].chars().next()?;
    let rest = &line[start + 1..];
    let title = &rest[..rest.find(quote)?];
    Some(title.trim().to_string()).filter(|t| !t.is_empty())
}

/// Resolves a workspace-relative path, refusing anything that would leave the workspace.
pub fn workspace_path(workspace: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = rel.trim().replace('\\', "/");
    let rel = rel.trim_start_matches("./");
    if rel.is_empty() {
        return Ok(workspace.to_path_buf());
    }
    let candidate = Path::new(rel);
    if candidate.is_absolute() || rel.contains(':') {
        return Err("Use a path relative to the workspace folder.".into());
    }
    let mut out = workspace.to_path_buf();
    for c in candidate.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return Err("Paths may not leave the workspace folder.".into()),
        }
    }
    Ok(out)
}

pub(crate) fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

pub(crate) fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    arg_str(args, key).ok_or_else(|| format!("The \"{key}\" argument is required."))
}

/// Shortens long tool text for the model, keeping the start and the end.
pub(crate) fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let head: String = text.chars().take(max * 2 / 3).collect();
    let tail: String = {
        let chars: Vec<char> = text.chars().collect();
        chars[chars.len() - max / 3..].iter().collect()
    };
    let omitted = text.chars().count() - head.chars().count() - tail.chars().count();
    format!("{head}\n… [{omitted} characters omitted] …\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_paths_stay_inside() {
        let ws = Path::new("/w");
        assert_eq!(
            workspace_path(ws, "data/a.csv").unwrap(),
            PathBuf::from("/w/data/a.csv")
        );
        assert_eq!(workspace_path(ws, "./x").unwrap(), PathBuf::from("/w/x"));
        assert!(workspace_path(ws, "../x").is_err());
        assert!(workspace_path(ws, "/etc/passwd").is_err());
        assert!(workspace_path(ws, "C:/Windows").is_err());
    }

    #[test]
    fn clip_keeps_both_ends() {
        let long = "a".repeat(100) + &"b".repeat(100);
        let c = clip(&long, 60);
        assert!(c.starts_with("aaaa") && c.ends_with("bbbb") && c.contains("omitted"));
        assert_eq!(clip("short", 60), "short");
    }

    #[test]
    fn pine_titles_label_the_save() {
        assert_eq!(pine_title("//@version=6\nindicator(\"RSI cross\", overlay=true)").as_deref(), Some("RSI cross"));
        assert_eq!(pine_title("strategy('Edge')").as_deref(), Some("Edge"));
        assert_eq!(pine_title("plot(close)"), None);
    }

    #[test]
    fn mail_titles_name_the_account_asked_for() {
        assert_eq!(describe("mail_list", &serde_json::json!({})), "Checking the inbox");
        assert_eq!(
            describe("mail_list", &serde_json::json!({"folder": "sent", "account": "Work"})),
            "Listing the sent folder · Work"
        );
        assert_eq!(
            describe(
                "mail_search",
                &serde_json::json!({"query": "invoice", "account": "ada@work.com"})
            ),
            "Searching email for “invoice” · ada@work.com"
        );
        assert_eq!(describe("mail_export", &serde_json::json!({})), "Exporting emails");
    }

    #[test]
    fn every_tool_has_an_object_schema() {
        for t in TOOLS {
            let schema = (t.parameters)();
            assert_eq!(schema["type"], "object", "{}", t.name);
            assert!(
                (t.name == LOADER && t.group.is_empty()) || GROUPS.iter().any(|g| g.id == t.group),
                "{}",
                t.name
            );
        }
    }

    fn calls(calls: &[(&str, &str)]) -> Message {
        let mut m = Message::new("c", 1, Role::Assistant, "");
        m.tool_calls = calls
            .iter()
            .enumerate()
            .map(|(i, (name, arguments))| crate::db::ToolCall {
                id: format!("call{i}"),
                name: name.to_string(),
                arguments: arguments.to_string(),
            })
            .collect();
        m
    }

    #[test]
    fn loaded_groups_are_read_back_from_the_chat() {
        let none = loaded_groups(&[Message::new("c", 0, Role::User, "Hi!")]);
        assert!(none.is_empty());
        let chat = [
            calls(&[(LOADER, r#"{"groups": ["market", "nonsense"]}"#)]),
            // Small models write a string, or name a tool instead of its group.
            calls(&[
                (LOADER, r#"{"group": "Mail"}"#),
                (LOADER, r#"{"groups": "run_python"}"#),
            ]),
            // A tool called without loading its group loads it; reading skills loads nothing.
            calls(&[("write_file", "{}"), ("read_skill_file", "{}")]),
        ];
        let loaded = loaded_groups(&chat);
        let mut ids: Vec<&str> = loaded.into_iter().collect();
        ids.sort();
        assert_eq!(ids, ["coding", "files", "mail", "market"]);
    }

    #[test]
    fn the_loader_names_only_groups() {
        let schema = load_schema(&["mail", "files"]);
        assert_eq!(
            schema["properties"]["groups"]["items"]["enum"],
            json!(["mail", "files"])
        );
        assert_eq!(
            describe(LOADER, &json!({"groups": ["market", "coding"]})),
            "Loading tools: Market data, Coding"
        );
        assert_eq!(describe(LOADER, &json!({})), "Loading tools");
        assert!(group_of("pine_test").is_some_and(|g| g.id == "market"));
        assert!(group_of("load_tools").is_none());
    }
}
