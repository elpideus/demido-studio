//! The assistant's tools.
//!
//! Tools come in groups the person can switch on and off from the composer's Tools menu:
//! market data, Python, the terminal, workspace files and skill authoring. Every call runs in the
//! context of one chat, whose workspace folder holds the files tools produce (market data CSVs,
//! charts, downloads).

mod changes;
mod command;
mod files;
mod market;
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
        name: "run_python",
        group: "python",
        description: "Run Python 3 in the chat's workspace folder (numpy, pandas, matplotlib and requests are installed). Use it to analyse data files, compute statistics or draw charts. Print what you need to see. Charts saved as PNG files are shown to the user. Every run is a fresh process.",
        parameters: python::schema,
        approval: true,
    },
    ToolDef {
        name: "run_command",
        group: "terminal",
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
        description: "Read a text file from the chat's workspace folder (CSV, JSON, Markdown, code).",
        parameters: files::read_schema,
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
        "Live prices from TradingView and history from Dukascopy",
    ),
    ("python", "Python", "Run analysis code in the chat's workspace"),
    ("terminal", "Terminal", "Run the programs installed on this computer"),
    ("files", "Workspace files", "Read and write files in the chat's folder"),
    (
        "skills",
        "Skill authoring",
        "Let the assistant save what it did as a skill",
    ),
];

fn group_availability(state: &AppState, group: &str) -> (bool, Option<String>) {
    match group {
        "market" => {
            let s = state.market.status();
            (s.available, s.reason)
        }
        "python" => match state.paths.python() {
            Some(_) => (true, None),
            None => (
                false,
                Some("Python is not installed. Run the installer again to add it.".into()),
            ),
        },
        // Offered until the search for a shell (at startup) finds none.
        "terminal" if crate::shell::missing() => (false, Some("No shell was found on this computer.".into())),
        _ => (true, None),
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
        .filter(|t| settings.tool_group_enabled(t.group) && group_availability(state, t.group).0)
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
    if settings.tool_group_enabled("terminal") {
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
        "run_python" => {
            if s("file").is_empty() {
                "Running Python".into()
            } else {
                format!("Running {}", s("file"))
            }
        }
        "run_command" => format!("Running {}", command::one_line(&s("command"), 60)),
        "list_files" => "Listing workspace files".into(),
        "read_file" => format!("Reading {}", s("path")),
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
        "run_python" => python::run(ctx, &args).await,
        "run_command" => command::run(ctx, &args).await,
        "list_files" => files::list(ctx, &args),
        "read_file" => files::read(ctx, &args),
        "write_file" => files::write(ctx, &args),
        "create_skill" => skills::create(ctx, &args),
        "read_skill_file" => skills::read(ctx, &args),
        other => Err(format!("There is no tool called {other}.")),
    };
    result.unwrap_or_else(ToolOutput::error)
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
    fn every_tool_has_an_object_schema() {
        for t in TOOLS {
            let schema = (t.parameters)();
            assert_eq!(schema["type"], "object", "{}", t.name);
            assert!(GROUPS.iter().any(|g| g.0 == t.group));
        }
    }
}
