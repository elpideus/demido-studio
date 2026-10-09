//! The assistant's tools.
//!
//! Tools come in groups the person can switch on and off from the composer's Tools menu:
//! market data (Pine scripts and the chart among them), coding (Python and the terminal), workspace
//! files and skill authoring. Every call runs in the context of one chat, whose workspace folder
//! holds the files tools produce (market data CSVs, charts, downloads).

mod changes;
mod command;
mod files;
mod market;
mod pine;
mod python;
mod skills;

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::agent::{Approval, Cancelled, ToolRow};
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
        description: "Find market symbols on TradingView (stocks, forex, crypto, indices, futures). Returns symbols such as FX:EURUSD to use with the other market tools.",
        parameters: market::search_schema,
        approval: false,
    },
    ToolDef {
        name: "market_quote",
        group: "market",
        description: "Get live prices from TradingView for one or more symbols: last price, change, bid/ask and the day's range.",
        parameters: market::quote_schema,
        approval: false,
    },
    ToolDef {
        name: "market_candles",
        group: "market",
        description: "Get OHLCV candles for a symbol (needs a TradingView sign-in). Recent and live data come from TradingView; older data comes from the stored Dukascopy history, and history that is not stored yet is downloaded first (a long download asks the user). All rows are saved as a CSV file in the workspace with a source column; the result is a summary with the file path.",
        parameters: market::candles_schema,
        approval: false,
    },
    ToolDef {
        name: "market_history",
        group: "market",
        description: "Historical candles without signing in: forex pairs, metals, commodities, indices and crypto from Dukascopy, years back (stocks come from stored TradingView data). Reads the local store and first downloads what is missing as 1-minute candles, from which every timeframe is built; a long download asks the user. Check market_data_status first to see what is stored. Saves a CSV file in the workspace and returns a summary with the file path.",
        parameters: market::history_schema,
        approval: false,
    },
    ToolDef {
        name: "market_download",
        group: "market",
        description: "Download a symbol's price history into the local store as 1-minute candles, from which every timeframe is built, so no timeframe ever needs another download. Without from/to it fetches all the history the source has. Check market_data_status first: stored data is never downloaded twice. A download estimated to take long asks the user first. The tool waits up to 90 seconds; a longer download continues in the background with a progress bar in the chat.",
        parameters: market::download_schema,
        approval: false,
    },
    ToolDef {
        name: "market_data_status",
        group: "market",
        description: "Show which market history is stored locally: for each market and source (Dukascopy or TradingView), the covered date ranges and gaps (Dukascopy: of its 1-minute history, which serves every timeframe; TradingView: per timeframe), the size on disk and any running downloads. Use it before downloading.",
        parameters: market::data_status_schema,
        approval: false,
    },
    ToolDef {
        name: "chart_add_indicator",
        group: "market",
        description: "Put an indicator on the user's chart in the Market window (it opens if closed): a script from Demido's Pine library or a TradingView indicator (STD;RSI, the user's USER;… scripts, community PUB;… scripts). TradingView computes it, so it shows the same values as on tradingview.com. Optionally switches the chart's symbol and timeframe first.",
        parameters: pine::add_indicator_schema,
        approval: false,
    },
    ToolDef {
        name: "chart_draw",
        group: "market",
        description: "Draw on the user's chart in the Market window, without writing an indicator: horizontal levels, trend lines, boxes (zones), labels, markers on bars, and series of values. Use it to show what you found (support and resistance, signals, a computed line) or for anything temporary. The set has a name in the chart's legend; drawing again with the same name replaces it, and an empty items list removes it.",
        parameters: pine::draw_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_list",
        group: "market",
        description: "List the Pine scripts in Demido's library (the indicators written in Demido), and optionally the user's own scripts saved on TradingView.",
        parameters: pine::list_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_read",
        group: "market",
        description: "Read a Pine script from Demido's library: its source with line numbers, and TradingView's compile errors and warnings with their lines. Or import one from TradingView first (the user's own scripts, or open-source community scripts) to read and change it. A long script comes in parts: the answer says which start_line reads on.",
        parameters: pine::read_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_save",
        group: "market",
        description: "Save a whole Pine Script indicator in Demido's library (new, or replacing a script's source by id) and compile it with TradingView's compiler: returns the compile errors and warnings with their lines, or the script's inputs and plots. Nothing is saved to TradingView. Write Pine Script v6 (//@version=6, indicator(…)); strategies and libraries can be saved but not run. To change part of a script, use pine_edit.",
        parameters: pine::save_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_edit",
        group: "market",
        description: "Change part of a library script: replaces old_string (copied from pine_read) with new_string, saves it and compiles it with TradingView's compiler, answering with the errors and warnings left (with their lines) and the changed lines. Use it to fix errors and warnings one change at a time. Nothing is saved to TradingView.",
        parameters: pine::edit_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_test",
        group: "market",
        description: "Run a Pine script on real bars of a symbol and timeframe (needs a TradingView sign-in), the way TradingView's chart runs it: returns compile or runtime errors with their lines, or each plot's latest values, ranges and signals, what it drew, and a CSV in the workspace with every bar's OHLCV and plot values. Test a library script by its id (save new code with pine_save first), or a TradingView indicator. Inputs can be changed for the run.",
        parameters: pine::test_schema,
        approval: false,
    },
    ToolDef {
        name: "pine_publish",
        group: "market",
        description: "Save a library script to the user's TradingView account, so they can use it on tradingview.com (My scripts, Pine Editor). The first time it creates a new script there; afterwards it saves the next version of that same script. It must compile. The user is asked first.",
        parameters: pine::publish_schema,
        approval: false,
    },
    ToolDef {
        name: "run_python",
        group: "coding",
        description: "Run Python 3 in the chat's workspace folder (numpy, pandas, matplotlib and requests are installed). Use it to analyse data files, compute statistics or draw charts. Print what you need to see. Charts saved as PNG files are shown to the user. Every run is a fresh process.",
        parameters: python::schema,
        approval: true,
    },
    ToolDef {
        name: "run_command",
        group: "coding",
        description: "Run a command line on the user's computer in their own shell (PowerShell on Windows) and get what it printed and its exit code. Use it for programs the user has installed (yt-dlp, ffmpeg, git, ping, winget and others) and for questions about the computer itself. The user approves each command. It runs in the chat's workspace folder unless directory is given, and is stopped after timeout seconds.",
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
        description: "Read a file from the chat's workspace folder as text: CSV, JSON, Markdown, code, and also PDF, Word, PowerPoint, Excel and web pages. For PDFs, presentations and spreadsheets, pages chooses pages (slides, sheets).",
        parameters: files::read_schema,
        approval: false,
    },
    ToolDef {
        name: "search_files",
        group: "files",
        description: "Search the files the user attached to this chat (PDF, Word, text, spreadsheets and others) for passages about something. Returns the best matching passages with their file and page. Matching is by meaning and by words (by words alone while the files are being indexed): when nothing is found, try other words, synonyms or fewer words.",
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
        description: "Save a reusable skill: instructions that are added to future conversations so a task can be repeated. Use it when the user asks to turn what you did into a skill. Write clear, numbered steps naming the tools and parameters to use; put Python code in extra files and run them with run_python's file parameter as skill:<skill-id>/<file>.",
        parameters: skills::create_schema,
        approval: false,
    },
    ToolDef {
        name: "read_skill_file",
        group: "skills",
        description: "Read a file of a skill (its SKILL.md, or a file it references).",
        parameters: skills::read_schema,
        approval: false,
    },
];

const GROUPS: &[(&str, &str, &str)] = &[
    (
        "market",
        "Market data",
        "Live prices from TradingView, history from Dukascopy, and Pine indicators",
    ),
    (
        "pine",
        "Pine scripts",
        "Write and test TradingView indicators, and save them to your TradingView account",
    ),
    (
        "coding",
        "Coding",
        "Run Python analysis and the programs installed on this computer",
    ),
    (
        "files",
        "Workspace files",
        "Read and write files in the chat's folder, and search attached files",
    ),
    (
        "skills",
        "Skill authoring",
        "Let the assistant save what it did as a skill",
    ),
];

/// Offered while any of its tools can run. The reason says what is missing, also when the rest of
/// the group still runs.
fn group_availability(state: &AppState, group: &str) -> (bool, Option<String>) {
    match group {
        "market" | "pine" => {
            let s = state.market.status();
            (s.available, s.reason)
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
        .map(|&(id, label, description)| {
            let (available, reason) = group_availability(state, id);
            ToolGroup {
                id,
                label,
                description,
                enabled: settings.tool_group_enabled(id),
                available,
                reason,
                tools: TOOLS.iter().filter(|t| t.group == id).map(|t| t.name).collect(),
            }
        })
        .collect()
}

/// Tool specs offered to the model, given the enabled groups.
pub fn specs(state: &AppState, settings: &Settings) -> Vec<ToolSpec> {
    TOOLS
        .iter()
        .filter(|t| {
            settings.tool_group_enabled(t.group)
                && group_availability(state, t.group).0
                && tool_missing(state, t.name).is_none()
        })
        .filter(|t| t.group != "skills" || t.name == "create_skill" || !state.skills.list().is_empty())
        .map(|t| ToolSpec {
            name: t.name.to_string(),
            description: t.description.to_string(),
            parameters: (t.parameters)(),
        })
        .collect()
}

/// Finds what the system prompt says about the tools: which shell commands run in. The first
/// time, on Windows, that takes a second or two.
pub async fn prepare(settings: &Settings) {
    if settings.tool_group_enabled("coding") {
        let _ = tokio::task::spawn_blocking(crate::shell::detect).await;
    }
}

pub fn exists(name: &str) -> bool {
    TOOLS.iter().any(|t| t.name == name)
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
        "run_python" => python::run(ctx, &args).await,
        "run_command" => command::run(ctx, &args).await,
        "list_files" => files::list(ctx, &args),
        "read_file" => files::read(ctx, &args).await,
        "search_files" => files::search(ctx, &args).await,
        "write_file" => files::write(ctx, &args),
        "create_skill" => skills::create(ctx, &args),
        "read_skill_file" => skills::read(ctx, &args),
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
    fn every_tool_has_an_object_schema() {
        for t in TOOLS {
            let schema = (t.parameters)();
            assert_eq!(schema["type"], "object", "{}", t.name);
            assert!(GROUPS.iter().any(|g| g.0 == t.group));
        }
    }
}
