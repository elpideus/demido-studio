//! Pine Script tools: the assistant writes TradingView indicators, tests them on real bars and,
//! when the person approves, saves them to their TradingView account. The scripts live in
//! Demido's Pine library (the market service's `pine.*` methods); TradingView compiles and runs
//! them, without saving anything there until `pine_publish`.
//!
//! The chart tools put an indicator, or a temporary set of drawings, on the Market window's chart
//! (a `market://chart` event the UI acts on).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::market::{arg_u64, call_live, parse_date, timeframe};
use super::{ToolContext, ToolOutput, arg_str, clip, require_str, workspace_path};
use crate::agent::{Approval, Cancelled};

/// Library calls: local files, no TradingView.
const LOCAL_CALL: Duration = Duration::from_secs(30);
/// Compiling, importing and saving to TradingView.
const FACADE_CALL: Duration = Duration::from_secs(60);
/// A test run: a chart session, the bars, the study.
const TEST_CALL: Duration = Duration::from_secs(150);
/// Chart ids of library scripts.
const PREFIX: &str = "DEMIDO;";
/// Rows of a test's values the model reads; the CSV has them all.
const TAIL_ROWS: usize = 8;
/// What one `pine_read` answer holds, so it fits in what the model reads of a result; a longer
/// script comes in parts.
const READ_CHARS: usize = 10_500;
/// Compiler messages an answer lists; the rest are counted, with their lines.
const MAX_MESSAGES: usize = 12;

const ID_HINT: &str = "The script's id in Demido's Pine library (from pine_list or pine_save)";

pub fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "tradingview": {"type": "boolean", "description": "Also list the user's own scripts saved on their TradingView account (needs a TradingView sign-in). Bring one into the library with pine_read"}
        }
    })
}

pub fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": ID_HINT},
            "tradingview_id": {"type": "string", "description": "Instead of id: a TradingView script id (USER;… from pine_list, or PUB;… for an open-source community script). Its source is copied into the library first; the user's own scripts stay linked, so pine_publish updates them"},
            "start_line": {"type": "integer", "description": "First line to show, from 1 (default 1). A long script comes in parts, and each part says where the next one starts"}
        }
    })
}

pub fn edit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": ID_HINT},
            "old_string": {"type": "string", "description": "The exact text to replace, copied from pine_read without the line numbers and with its indentation. Include enough of the lines around it to make it unique"},
            "new_string": {"type": "string", "description": "The text that takes its place (empty to delete it)"},
            "replace_all": {"type": "boolean", "description": "Replace every occurrence of old_string (by default it must occur exactly once)"}
        },
        "required": ["id", "old_string", "new_string"]
    })
}

pub fn save_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "source": {"type": "string", "description": "The whole Pine Script source. Start with //@version=6 and an indicator(\"Title\", …) declaration; the title names the script"},
            "id": {"type": "string", "description": "Replace this library script's source (from an earlier pine_save or pine_list). Leave it out to create a new script"},
            "name": {"type": "string", "description": "Optional name; by default the declaration's title"}
        },
        "required": ["source"]
    })
}

pub fn test_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": "The script to test: a library script's id (from pine_list or pine_save), or a TradingView indicator id such as STD;RSI. To test new code, save it with pine_save first"},
            "symbol": {"type": "string", "description": "TradingView symbol, e.g. FX:EURUSD, NASDAQ:AAPL, BINANCE:BTCUSDT"},
            "timeframe": {"type": "string", "enum": ["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M"]},
            "bars": {"type": "integer", "description": "Newest bars to run it on (10 to 5000, default 500)"},
            "inputs": {"type": "object", "description": "Input values by input id (in_0, in_1, …) or by the input's title, e.g. {\"Length\": 50}. The result lists the inputs"}
        },
        "required": ["id", "symbol", "timeframe"]
    })
}

pub fn publish_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "id": {"type": "string", "description": ID_HINT},
            "name": {"type": "string", "description": "Optional name on TradingView; by default the script's name"}
        },
        "required": ["id"]
    })
}

pub fn add_indicator_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "script": {"type": "string", "description": "A library script id (from pine_save), or a TradingView indicator id: STD;RSI, STD;MACD, USER;… (the user's own), PUB;… (community)"},
            "inputs": {"type": "object", "description": "Optional input values by input id (in_0, …) or title"},
            "symbol": {"type": "string", "description": "Switch the chart to this symbol first"},
            "timeframe": {"type": "string", "enum": ["1m", "5m", "15m", "1h", "4h", "1d", "1w"], "description": "Switch the chart to this timeframe first"}
        },
        "required": ["script"]
    })
}

pub fn draw_schema() -> Value {
    let point = |what: &str| json!({"description": format!("{what}: ISO date-time (2026-10-01T14:00:00Z) or Unix seconds")});
    json!({
        "type": "object",
        "properties": {
            "name": {"type": "string", "description": "Name of this set of drawings, shown in the chart's legend. Drawing again with the same name replaces the set"},
            "items": {
                "type": "array",
                "description": "What to draw. Leave it empty to remove the set with this name (\"*\" removes every set)",
                "items": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["hline", "line", "box", "label", "marker", "series"], "description": "hline: a horizontal level at price. line: a segment from (time, price) to (time2, price2). box: a rectangle between two corners. label: text at (time, price). marker: an arrow or shape above or below the bar at time. series: a line through points, like an indicator plot"},
                        "time": point("Bar time"),
                        "price": {"type": "number"},
                        "time2": point("Second point's time (line, box)"),
                        "price2": {"type": "number", "description": "Second point's price (line, box)"},
                        "points": {"type": "array", "items": {"type": "array", "items": {}}, "description": "series: [[time, value], …]"},
                        "text": {"type": "string"},
                        "color": {"type": "string", "description": "CSS color, e.g. #f23645 or rgba(41,98,255,0.3)"},
                        "width": {"type": "integer", "description": "Line width in pixels (1 to 4)"},
                        "style": {"type": "string", "enum": ["solid", "dashed", "dotted"]},
                        "extend": {"type": "string", "enum": ["none", "left", "right", "both"], "description": "line: extend beyond its points"},
                        "position": {"type": "string", "enum": ["above", "below"], "description": "marker, label: above or below the bar (a label without it sits at price)"},
                        "shape": {"type": "string", "enum": ["arrow_up", "arrow_down", "triangle_up", "triangle_down", "circle", "square", "cross", "diamond", "flag"], "description": "marker's shape"}
                    },
                    "required": ["type"]
                }
            },
            "pane": {"type": "string", "enum": ["overlay", "separate"], "description": "overlay (default) draws on the candles; separate draws in a pane of its own below them, for values on another scale"},
            "symbol": {"type": "string", "description": "The chart to draw on; switches the chart to it. By default the chart's current symbol"},
            "timeframe": {"type": "string", "enum": ["1m", "5m", "15m", "1h", "4h", "1d", "1w"], "description": "Switch the chart to this timeframe"}
        },
        "required": ["name", "items"]
    })
}

// ---------------------------------------------------------------------------------------------
// Helpers

async fn local(ctx: &ToolContext, method: &str, params: Value) -> Result<Value, String> {
    ctx.state
        .market
        .call(method, params, LOCAL_CALL)
        .await
        .map_err(|e| e.message)
}

/// A library id from what the model sent: `abc123` or `DEMIDO;abc123`.
fn library_id(raw: &str) -> &str {
    raw.strip_prefix(PREFIX).unwrap_or(raw).trim()
}

fn iso_ms(v: &Value) -> Value {
    v.as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|t| Value::String(t.format("%Y-%m-%d %H:%M UTC").to_string()))
        .unwrap_or(Value::Null)
}

fn iso_s(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|t| t.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .unwrap_or_else(|| t.to_string())
}

/// What the model reads about a library script.
fn brief(s: &Value) -> Value {
    let id = s["id"].as_str().unwrap_or_default();
    let mut out = json!({
        "id": id,
        "chartId": format!("{PREFIX}{id}"),
        "name": s["name"],
        "kind": s["kind"],
        "lines": s["lines"],
        "revision": s["revision"],
        "modified": iso_ms(&s["modified"]),
    });
    if let Some(tv) = s["tradingview"].as_object() {
        out["tradingview"] = json!({"id": tv.get("id"), "version": tv.get("version"), "editedSince": tv.get("changed")});
    }
    out
}

/// Compiler messages with the line they point at, so the model sees what to fix.
fn placed(messages: &Value, source: &str) -> Vec<Value> {
    let lines: Vec<&str> = source.split('\n').collect();
    messages
        .as_array()
        .map(|list| {
            list.iter()
                .map(|m| {
                    let line = m["line"].as_u64().unwrap_or(1) as usize;
                    let mut out = json!({"line": line, "column": m["column"], "message": m["message"]});
                    if let Some(text) = lines.get(line.saturating_sub(1)) {
                        out["code"] = text.trim_end_matches('\r').chars().take(200).collect::<String>().into();
                    }
                    out
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A library script without its source, for the UI's cards.
fn without_source(s: &Value) -> Value {
    let mut out = s.clone();
    if let Some(o) = out.as_object_mut() {
        o.remove("source");
    }
    out
}

/// The library's scripts, for an answer to a call that named none of them.
async fn library_hint(ctx: &ToolContext) -> String {
    let list = local(ctx, "pine.list", json!({})).await.unwrap_or(Value::Null);
    let scripts: Vec<String> = list
        .as_array()
        .map(|l| {
            l.iter()
                .take(12)
                .map(|s| format!("\"{}\" ({})", s["id"].as_str().unwrap_or_default(), s["name"].as_str().unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default();
    if scripts.is_empty() {
        " Demido's library is empty: write a script with pine_save.".into()
    } else {
        format!(" The library's scripts, by id: {}.", scripts.join(", "))
    }
}

/// A script's id from its name, for a model that gave the name: the same name, or the only one
/// that contains it (case aside).
fn by_name(list: &Value, name: &str) -> Option<String> {
    let wanted = name.trim().to_lowercase();
    let scripts = list.as_array()?;
    if wanted.is_empty() {
        return None;
    }
    let named = |s: &&Value| s["name"].as_str().unwrap_or_default().to_lowercase();
    if let Some(s) = scripts.iter().find(|s| named(s) == wanted) {
        return s["id"].as_str().map(str::to_string);
    }
    let mut containing = scripts.iter().filter(|s| named(s).contains(&wanted));
    match (containing.next(), containing.next()) {
        (Some(s), None) => s["id"].as_str().map(str::to_string),
        _ => None,
    }
}

/// The library script a call names by `id` (or by its name), with its source. Without one, or
/// with one the library does not have, the answer lists the ids there are.
async fn script_of(ctx: &ToolContext, args: &Value, what: &str) -> Result<Value, String> {
    let Some(raw) = arg_str(args, "id") else {
        return Err(format!("The \"id\" argument is required: {what}.{}", library_hint(ctx).await));
    };
    let id = library_id(raw);
    match local(ctx, "pine.get", json!({"id": id})).await {
        Ok(s) => Ok(s),
        Err(e) => {
            let list = local(ctx, "pine.list", json!({})).await.unwrap_or(Value::Null);
            match by_name(&list, id) {
                Some(found) => local(ctx, "pine.get", json!({"id": found})).await,
                None => Err(format!("{e}{}", library_hint(ctx).await)),
            }
        }
    }
}

/// Compiles without asking the person to sign in: `None` when they are signed out, or when
/// TradingView does not answer.
async fn quiet_check(ctx: &ToolContext, source: &str) -> Option<Value> {
    if !ctx.state.market.logged_in() {
        return None;
    }
    ctx.state
        .market
        .call("pine.check", json!({"source": source}), FACADE_CALL)
        .await
        .ok()
}

/// At most `MAX_MESSAGES` of a list, and a word on the rest.
fn capped(list: Vec<Value>) -> (Vec<Value>, Option<String>) {
    if list.len() <= MAX_MESSAGES {
        return (list, None);
    }
    let rest: Vec<String> = list[MAX_MESSAGES..]
        .iter()
        .filter_map(|m| m["line"].as_u64())
        .take(20)
        .map(|l| l.to_string())
        .collect();
    let more = format!("{} more, on lines {}", list.len() - MAX_MESSAGES, rest.join(", "));
    (list.into_iter().take(MAX_MESSAGES).collect(), Some(more))
}

/// Source lines as `pine_read` shows them, numbered from `first`: ` 9| code`.
fn numbered(lines: &[&str], first: usize) -> String {
    let width = (first + lines.len()).saturating_sub(1).max(1).to_string().len();
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("{:>width$}| {}", first + i, l.trim_end_matches('\r')))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text the model copied from `pine_read` with its line numbers, without them. `None` unless
/// every line that is not blank is numbered.
fn without_numbers(text: &str) -> Option<String> {
    let mut found = false;
    let mut out = Vec::new();
    for line in text.split('\n') {
        let body = line.trim_start();
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && body[digits..].starts_with('|') {
            let rest = &body[digits + 1..];
            out.push(rest.strip_prefix(' ').unwrap_or(rest));
            found = true;
        } else if line.trim().is_empty() {
            out.push(if line.ends_with('\r') { "\r" } else { "" });
        } else {
            return None;
        }
    }
    found.then(|| out.join("\n"))
}

/// What an edit changed.
#[derive(Debug, PartialEq)]
struct Edited {
    source: String,
    /// Replacements made.
    count: usize,
    /// The line (from 1) of the first one, and how many lines its new text has.
    line: usize,
    lines: usize,
}

fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count() + 1
}

fn line_list(lines: &[usize]) -> String {
    lines.iter().take(8).map(|l| l.to_string()).collect::<Vec<_>>().join(", ")
}

/// Replaces `old` with `new` in a script's source: as given, else without the line numbers
/// `pine_read` shows, else line by line ignoring indentation, `new` taking the indentation the
/// script has there. Small models copy indentation and line numbers along; Pine needs the
/// indentation right.
fn apply_edit(source: &str, old: &str, new: &str, all: bool) -> Result<Edited, String> {
    let crlf = source.contains("\r\n");
    let fit = |t: &str| {
        let t = t.replace("\r\n", "\n");
        if crlf { t.replace('\n', "\r\n") } else { t }
    };
    let (mut old, mut new) = (fit(old), fit(new));
    if old.trim().is_empty() {
        return Err("old_string is empty: give the text to replace, copied from pine_read.".into());
    }
    if !source.contains(&old)
        && let Some(plain) = without_numbers(&old)
    {
        old = plain;
    }
    // No Pine line starts with a number and a bar, so these are pine_read's.
    if let Some(plain) = without_numbers(&new) {
        new = plain;
    }
    if old == new {
        return Err("old_string and new_string are the same, so there is nothing to change.".into());
    }
    let at: Vec<usize> = source.match_indices(old.as_str()).map(|(i, _)| i).collect();
    let lines = new.matches('\n').count() + 1;
    match at.len() {
        0 => loose_edit(source, &old, &new, crlf),
        1 => Ok(Edited {
            source: format!("{}{new}{}", &source[..at[0]], &source[at[0] + old.len()..]),
            count: 1,
            line: line_of(source, at[0]),
            lines,
        }),
        n if all => Ok(Edited { source: source.replace(old.as_str(), &new), count: n, line: line_of(source, at[0]), lines }),
        n => {
            let on: Vec<usize> = at.iter().map(|&i| line_of(source, i)).collect();
            Err(format!(
                "old_string occurs {n} times (lines {}). Include more of the lines around it so it occurs once, or set replace_all to change every one.",
                line_list(&on)
            ))
        }
    }
}

fn indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// `old` matched whole line by whole line, ignoring indentation and trailing spaces.
fn loose_edit(source: &str, old: &str, new: &str, crlf: bool) -> Result<Edited, String> {
    let lines: Vec<&str> = source.split('\n').collect();
    let trimmed = |text: &str| -> Vec<String> {
        let all: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
        let first = all.iter().position(|l| !l.trim().is_empty()).unwrap_or(all.len());
        let last = all.iter().rposition(|l| !l.trim().is_empty()).map_or(first, |l| l + 1);
        all[first..last.max(first)].iter().map(|l| l.to_string()).collect()
    };
    let wanted = trimmed(old);
    let Some(head) = wanted.first() else {
        return Err("old_string is empty: give the text to replace, copied from pine_read.".into());
    };
    let found: Vec<usize> = (0..lines.len())
        .filter(|&i| {
            wanted
                .iter()
                .enumerate()
                .all(|(j, w)| lines.get(i + j).is_some_and(|l| l.trim() == w.trim()))
        })
        .collect();
    let i = match found.as_slice() {
        [i] => *i,
        [] => {
            let probe = head.trim();
            let near: Vec<usize> = lines
                .iter()
                .enumerate()
                .filter(|(_, l)| l.contains(probe))
                .map(|(i, _)| i + 1)
                .collect();
            let base = "old_string is not in the script. Copy it exactly as pine_read shows it, without the line numbers.";
            return Err(match near.first() {
                None => format!("{base} Read the script again with pine_read to see its current text."),
                Some(&first) => format!(
                    "{base} Its first line is on line {} but the lines after it differ: read from there again with pine_read (start_line {}).",
                    line_list(&near),
                    first.saturating_sub(2).max(1)
                ),
            });
        }
        many => {
            let on: Vec<usize> = many.iter().map(|i| i + 1).collect();
            return Err(format!(
                "old_string occurs {} times (lines {}). Include more of the lines around it so it occurs once.",
                many.len(),
                line_list(&on)
            ));
        }
    };
    let end = i + wanted.len();
    // Each new line takes the script's indentation of the old line it repeats, else of the old
    // line in its place, shifted as far as the model shifted it from that line.
    let width = |l: &str| indent(l).chars().count();
    let mut next = 0;
    let replacement: Vec<String> = trimmed(new)
        .iter()
        .map(|l| {
            if l.trim().is_empty() {
                return String::new();
            }
            let j = match (next..wanted.len()).find(|&j| wanted[j].trim() == l.trim()) {
                Some(j) => {
                    next = j + 1;
                    j
                }
                None => {
                    let j = next.min(wanted.len() - 1);
                    wanted[..=j].iter().rposition(|w| !w.trim().is_empty()).unwrap_or(0)
                }
            };
            // A line the model put further out lines up with the copied line it went out to.
            let j = if width(l) < width(&wanted[j]) {
                wanted[..=j]
                    .iter()
                    .rposition(|w| !w.trim().is_empty() && width(w) <= width(l))
                    .unwrap_or(j)
            } else {
                j
            };
            let have = indent(lines[i + j].trim_end_matches('\r'));
            let (mine, theirs) = (width(l), width(&wanted[j]));
            let shifted: String = if mine >= theirs {
                format!("{have}{}", indent(l).chars().skip(theirs).collect::<String>())
            } else {
                have.chars().take(have.chars().count().saturating_sub(theirs - mine)).collect()
            };
            format!("{shifted}{}", l.trim_start())
        })
        .collect();
    // In a CRLF source every line but a last one without an ending keeps its \r.
    let ends_cr = lines[end - 1].ends_with('\r');
    let count = replacement.len();
    let mut out: Vec<String> = lines[..i].iter().map(|l| l.to_string()).collect();
    for (k, l) in replacement.into_iter().enumerate() {
        out.push(if crlf && (k + 1 < count || ends_cr) { format!("{l}\r") } else { l });
    }
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    Ok(Edited { source: out.join("\n"), count: 1, line: i + 1, lines: count.max(1) })
}

/// Lines an edit changed, with two around them, numbered as `pine_read` shows them.
fn around(source: &str, line: usize, count: usize) -> String {
    let lines: Vec<&str> = source.split('\n').collect();
    let from = line.saturating_sub(2).max(1);
    let to = (line + count + 1).min(lines.len());
    if from > to {
        return String::new();
    }
    clip(&numbered(&lines[from - 1..to], from), 2_000)
}

/// What `pine_read` answers: the script, what the compiler says of it, and as much of its source
/// from `start` as fits in one answer, saying where the next part starts.
fn read_text(s: &Value, source: &str, start: usize, check: Option<&Value>, note: Option<&str>) -> String {
    let mut lines: Vec<&str> = source.split('\n').collect();
    if lines.len() > 1 && lines.last() == Some(&"") {
        lines.pop();
    }
    let total = lines.len();
    let some_lines = |n: usize| if n == 1 { "1 line".to_string() } else { format!("{n} lines") };
    let id = s["id"].as_str().unwrap_or_default();
    let mut out = String::new();
    let _ = write!(
        out,
        "Pine script \"{}\" (id {id}, {}, revision {}",
        s["name"].as_str().unwrap_or_default(),
        s["kind"].as_str().unwrap_or("unknown kind"),
        s["revision"]
    );
    if let Some(tv) = s["tradingview"]["id"].as_str() {
        let _ = write!(out, "; saved on TradingView as {tv}");
        if s["tradingview"]["changed"] == true {
            out.push_str(", changed here since");
        }
    }
    out.push_str(").\n");
    if let Some(note) = note {
        let _ = writeln!(out, "{note}");
    }
    match check {
        None => out.push_str("Not compiled (the user is not signed in to TradingView); pine_edit and pine_save compile it.\n"),
        Some(c) => {
            let errors = placed(&c["errors"], source);
            let warnings = placed(&c["warnings"], source);
            let count = |n: usize, what: &str| if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") };
            let _ = writeln!(
                out,
                "{}",
                match (errors.len(), warnings.len()) {
                    (0, 0) => "TradingView's compiler: it compiles, with no warnings.".to_string(),
                    (0, w) => format!("TradingView's compiler: it compiles, with {}:", count(w, "warning")),
                    (e, 0) => format!("TradingView's compiler: it does not compile, {}:", count(e, "error")),
                    (e, w) => format!("TradingView's compiler: it does not compile, {} and {}:", count(e, "error"), count(w, "warning")),
                }
            );
            let tagged = errors
                .into_iter()
                .map(|m| ("error", m))
                .chain(warnings.into_iter().map(|m| ("warning", m)));
            let all: Vec<Value> = tagged.map(|(kind, mut m)| {
                m["kind"] = kind.into();
                m
            }).collect();
            let (shown, more) = capped(all);
            for m in &shown {
                let message: String = m["message"].as_str().unwrap_or_default().chars().take(300).collect();
                let _ = writeln!(out, "- line {} ({}): {message}", m["line"], m["kind"].as_str().unwrap_or_default());
                if let Some(code) = m["code"].as_str().map(str::trim).filter(|c| !c.is_empty()) {
                    let _ = writeln!(out, "    {}", code.chars().take(160).collect::<String>());
                }
            }
            if let Some(more) = more {
                let _ = writeln!(out, "- and {more}");
            }
        }
    }
    let _ = writeln!(out, "Change it with pine_edit (id {id}); it compiles it again.");
    if start > total {
        let _ = write!(out, "It has only {}.", some_lines(total));
        return out;
    }
    // Lines from `start` while they fit; at least one.
    let budget = READ_CHARS.saturating_sub(out.chars().count()).max(2_000);
    let mut end = start - 1;
    let mut used = 0;
    while end < total {
        let cost = lines[end].chars().count() + 8;
        if used + cost > budget && end >= start {
            break;
        }
        used += cost;
        end += 1;
    }
    if start == 1 && end == total {
        let _ = writeln!(out, "Its source, {} (the line numbers are not part of it):", some_lines(total));
    } else {
        let _ = writeln!(out, "Lines {start} to {end} of {total} (the line numbers are not part of the source):");
    }
    out.push_str(&numbered(&lines[start - 1..end], start));
    if end < total {
        let _ = write!(out, "\n… it continues: pine_read with id {id} and start_line {}.", end + 1);
    } else if start > 1 {
        out.push_str("\nThat is the end of the script.");
    }
    out
}

/// The inputs a test can set, as the model reads them.
fn inputs_brief(meta: &Value) -> Vec<Value> {
    meta["inputs"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|i| i["hidden"] != true)
                .map(|i| {
                    let mut out = json!({"id": i["id"], "title": i["name"], "type": i["type"], "value": i["value"]});
                    if let Some(o) = i.get("options").filter(|o| o.is_array()) {
                        out["options"] = o.clone();
                    }
                    out
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Input values keyed by id; titles the model used are matched to their ids (the service needs ids).
fn input_ids(given: &Value, meta: Option<&Value>) -> Value {
    let Some(given) = given.as_object() else {
        return json!({});
    };
    let by_title: HashMap<String, String> = meta
        .and_then(|m| m["inputs"].as_array())
        .map(|list| {
            list.iter()
                .filter_map(|i| Some((i["name"].as_str()?.to_lowercase(), i["id"].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let mut out = Map::new();
    for (k, v) in given {
        let id = by_title.get(&k.to_lowercase()).cloned().unwrap_or_else(|| k.clone());
        out.insert(id, v.clone());
    }
    Value::Object(out)
}

fn file_safe(text: &str, max: usize) -> String {
    let safe: String = text
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let trimmed = safe.trim_matches('_');
    let mut out: String = trimmed.chars().take(max).collect();
    if out.is_empty() {
        out = "script".into();
    }
    out
}

// ---------------------------------------------------------------------------------------------
// pine_list, pine_read, pine_save

pub async fn list(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let library = local(ctx, "pine.list", json!({})).await?;
    let scripts: Vec<Value> = library.as_array().map(|l| l.iter().map(brief).collect()).unwrap_or_default();
    let mut content = json!({ "library": scripts });
    let mut display = json!({"kind": "pineList", "scripts": library});
    if args["tradingview"] == true {
        let catalog = call_live(ctx, "indicators.catalog", json!({}), FACADE_CALL).await?;
        let mine: Vec<Value> = catalog["mine"]
            .as_array()
            .map(|l| l.iter().map(|e| json!({"id": e["id"], "name": e["name"]})).collect())
            .unwrap_or_default();
        display["tradingview"] = mine.len().into();
        content["tradingview"] = mine.into();
    }
    if scripts.is_empty() {
        content["note"] = "The library is empty. Write a script with pine_save.".into();
    }
    Ok(ToolOutput::ok(content.to_string(), display))
}

pub async fn read(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let mut note = None;
    let imported = arg_str(args, "tradingview_id");
    let s = match imported {
        Some(tv) => {
            let imported = call_live(ctx, "pine.import", json!({"script": tv}), FACADE_CALL).await?;
            if imported["updated"] == false {
                note = Some("It was already in the library with edits not yet saved to TradingView; those were kept.");
            }
            let id = imported["script"]["id"].as_str().unwrap_or_default().to_string();
            local(ctx, "pine.get", json!({"id": id})).await?
        }
        None => {
            script_of(ctx, args, "the script to read, from pine_list (or a tradingview_id to import one from TradingView)")
                .await?
        }
    };
    let source = s["source"].as_str().unwrap_or_default();
    let check = quiet_check(ctx, source).await;
    let start = args["start_line"].as_u64().unwrap_or(1).max(1) as usize;
    let text = read_text(&s, source, start, check.as_ref(), note);
    let mut display = json!({
        "kind": "pineScript",
        "action": if imported.is_some() { "imported" } else { "read" },
        "script": without_source(&s),
    });
    if let Some(c) = &check {
        display["ok"] = (c["ok"] == true).into();
        display["errors"] = c["errors"].as_array().map_or(0, Vec::len).into();
        display["warnings"] = c["warnings"].as_array().map_or(0, Vec::len).into();
    }
    Ok(ToolOutput::ok(text, display))
}

pub async fn save(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let given = args["source"].as_str().filter(|s| !s.trim().is_empty()).ok_or("The \"source\" argument is required.")?;
    // A whole script copied from pine_read comes with its line numbers.
    let plain = without_numbers(given);
    let source = plain.as_deref().unwrap_or(given);
    let mut params = json!({"source": source});
    if let Some(id) = arg_str(args, "id") {
        params["id"] = library_id(id).into();
    }
    if let Some(name) = arg_str(args, "name") {
        params["name"] = name.into();
    }
    let (content, display) = store(ctx, params, source, "Saved in Demido's library").await?;
    Ok(ToolOutput::ok(content.to_string(), display))
}

pub async fn edit(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let s = script_of(ctx, args, "the script to change, from pine_list").await?;
    let old = args["old_string"]
        .as_str()
        .ok_or("The \"old_string\" argument is required: the text to replace, copied from pine_read.")?;
    let new = args["new_string"]
        .as_str()
        .ok_or("The \"new_string\" argument is required: the text that replaces old_string (empty to delete it).")?;
    let id = s["id"].as_str().unwrap_or_default().to_string();
    let edited = apply_edit(s["source"].as_str().unwrap_or_default(), old, new, args["replace_all"] == true)?;
    let params = json!({"id": id, "source": edited.source});
    let (mut content, mut display) = store(ctx, params, &edited.source, "Changed and saved in Demido's library").await?;
    content["replaced"] = edited.count.into();
    content["now"] = around(&edited.source, edited.line, edited.lines).into();
    display["edited"] = json!({"line": edited.line, "count": edited.count});
    Ok(ToolOutput::ok(content.to_string(), display))
}

/// Saves a source in the library and compiles it: what the model reads, and the card.
async fn store(ctx: &ToolContext, params: Value, source: &str, done: &str) -> Result<(Value, Value), String> {
    let saved = local(ctx, "pine.save", params).await?;
    let id = saved["id"].as_str().unwrap_or_default().to_string();
    let mut content = brief(&saved);
    let mut display = json!({"kind": "pineSave", "script": without_source(&saved)});
    match call_live(ctx, "pine.check", json!({"source": source}), FACADE_CALL).await {
        Ok(check) => {
            let ok = check["ok"] == true;
            content["compiled"] = ok.into();
            let errors = placed(&check["errors"], source);
            let warnings = placed(&check["warnings"], source);
            if !errors.is_empty() {
                let (shown, more) = capped(errors.clone());
                content["errors"] = shown.into();
                if let Some(more) = more {
                    content["moreErrors"] = more.into();
                }
            }
            if !warnings.is_empty() {
                let (shown, more) = capped(warnings.clone());
                content["warnings"] = shown.into();
                if let Some(more) = more {
                    content["moreWarnings"] = more.into();
                }
            }
            if let Some(meta) = check.get("meta") {
                content["inputs"] = inputs_brief(meta).into();
                content["plots"] = meta["plots"]
                    .as_array()
                    .map(|l| l.iter().map(|p| json!({"title": p["title"], "kind": p["kind"]})).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into();
            }
            content["note"] = if ok && !warnings.is_empty() {
                let n = if warnings.len() == 1 { "1 warning".to_string() } else { format!("{} warnings", warnings.len()) };
                format!("{done} and compiled, with {n} left (in \"warnings\", by line). To fix them, change those lines with pine_edit (id {id}); or test it with pine_test (id {id}).")
            } else if ok {
                format!("{done} and compiled, with no warnings. Test it with pine_test (id {id}), or put it on the user's chart with chart_add_indicator.")
            } else {
                format!("{done}, but it does not compile. Fix the errors with pine_edit (id {id}).")
            }
            .into();
            display["ok"] = ok.into();
            display["errors"] = errors.into();
            display["warnings"] = warnings.into();
        }
        Err(e) => {
            content["compiled"] = Value::Null;
            content["note"] = format!("{done}, but not compiled: {e}").into();
        }
    }
    Ok((content, display))
}

// ---------------------------------------------------------------------------------------------
// pine_test

pub async fn test(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let symbol = require_str(args, "symbol")?;
    let tf = timeframe(args)?;
    let bars = arg_u64(args, "bars").unwrap_or(500).clamp(10, 5000);
    let mut params = json!({"symbol": symbol, "timeframe": tf, "bars": bars});
    // The source, for pointing at lines; the name, for the CSV.
    let (source, name) = match (args["source"].as_str().filter(|s| !s.trim().is_empty()), arg_str(args, "id")) {
        (Some(source), _) => {
            params["source"] = source.into();
            (source.to_string(), String::new())
        }
        (None, Some(id)) if id.contains(';') && !id.starts_with(PREFIX) => {
            params["script"] = id.into();
            (String::new(), id.to_string())
        }
        (None, _) => {
            let what = "the script to test, from pine_list (or a TradingView indicator id such as STD;RSI)";
            let s = script_of(ctx, args, what).await?;
            params["script"] = format!("{PREFIX}{}", s["id"].as_str().unwrap_or_default()).into();
            (s["source"].as_str().unwrap_or_default().to_string(), s["name"].as_str().unwrap_or_default().to_string())
        }
    };
    // Titles become ids once the script's inputs are known: a quick compile tells them.
    let mut meta_hint = None;
    if args["inputs"].as_object().is_some_and(|o| !o.is_empty())
        && !source.is_empty()
        && let Ok(check) = call_live(ctx, "pine.check", json!({"source": source}), FACADE_CALL).await
    {
        meta_hint = check.get("meta").cloned();
    }
    params["state"] = json!({"inputs": input_ids(&args["inputs"], meta_hint.as_ref())});
    let result = call_live(ctx, "pine.test", params, TEST_CALL).await?;
    let mut display = json!({"kind": "pineTest", "symbol": symbol, "timeframe": tf, "ok": result["ok"]});

    match result["failed"].as_str() {
        Some("compile") => {
            let errors = placed(&result["errors"], &source);
            display["errors"] = errors.clone().into();
            let content = json!({
                "ok": false,
                "stage": "compile",
                "errors": errors,
                "note": "It does not compile. Fix the errors and test again.",
            });
            return Ok(ToolOutput::ok(content.to_string(), display));
        }
        Some(_) => {
            let fault = &result["fault"];
            let mut error = json!({"message": fault["message"]});
            if let Some(line) = fault["line"].as_u64() {
                error["line"] = line.into();
                if let Some(text) = source.split('\n').nth(line as usize - 1) {
                    error["code"] = text.trim_end_matches('\r').into();
                }
            }
            display["fault"] = error.clone();
            let content = json!({
                "ok": false,
                "stage": "runtime",
                "error": error,
                "note": "It compiled but stopped with an error while running on the bars. Fix it and test again.",
            });
            return Ok(ToolOutput::ok(content.to_string(), display));
        }
        None => {}
    }

    let meta = &result["meta"];
    let script_name = meta["name"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or(name);
    let table = values_table(&result);
    let rel = format!(
        "data/pine_{}_{}_{tf}.csv",
        file_safe(&script_name, 40),
        file_safe(symbol, 30)
    );
    // Windows file names ignore case: monthly (1M) would overwrite minute (1m) data.
    let rel = rel.replace("_1M.csv", "_1mo.csv");
    let path = workspace_path(&ctx.workspace, &rel)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create the data folder: {e}"))?;
    }
    std::fs::write(&path, table.csv()).map_err(|e| format!("could not write {rel}: {e}"))?;

    let mut content = json!({
        "ok": true,
        "script": script_name,
        "symbol": result["info"]["symbol"].as_str().unwrap_or(symbol),
        "timeframe": tf,
        "bars": table.times.len(),
        "from": table.times.first().map(|t| iso_s(*t)),
        "to": table.times.last().map(|t| iso_s(*t)),
        "file": rel,
        "inputs": inputs_brief(meta),
        "plots": table.stats(),
        "last": table.tail(TAIL_ROWS),
    });
    let warnings = placed(&result["warnings"], &source);
    if !warnings.is_empty() {
        content["warnings"] = warnings.into();
    }
    if let Some(drawings) = drawings_brief(&result["graphics"]) {
        content["drawings"] = drawings;
    }
    content["note"] = "Every bar's values are in the CSV (time, OHLCV, then one column per plot). Put it on the user's chart with chart_add_indicator.".into();
    display["name"] = script_name.into();
    display["bars"] = table.times.len().into();
    display["file"] = rel.into();
    display["absolute"] = path.to_string_lossy().into_owned().into();
    display["plots"] = content["plots"].clone();
    Ok(ToolOutput::ok(content.to_string(), display))
}

/// A test's bars and the indicator's values on them, by plot.
struct ValuesTable {
    times: Vec<i64>,
    ohlcv: Vec<[Option<f64>; 5]>,
    /// Plot title, its kind, hidden, values per bar.
    plots: Vec<(String, String, bool, Vec<Option<f64>>)>,
}

fn values_table(result: &Value) -> ValuesTable {
    let empty = Vec::new();
    let bars = result["bars"].as_array().unwrap_or(&empty);
    let times: Vec<i64> = bars.iter().filter_map(|b| b["t"].as_i64()).collect();
    let ohlcv = bars
        .iter()
        .filter(|b| b["t"].is_i64())
        .map(|b| ["o", "h", "l", "c", "v"].map(|k| b[k].as_f64()))
        .collect();
    let columns: Vec<&str> = result["meta"]["columns"]
        .as_array()
        .map(|c| c.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let rows: HashMap<i64, &Vec<Value>> = result["rows"]
        .as_array()
        .map(|r| r.iter().filter_map(|row| Some((row.get(0)?.as_i64()?, row.as_array()?))).collect())
        .unwrap_or_default();
    let mut used = HashMap::<String, usize>::new();
    let mut plots = Vec::new();
    for p in result["meta"]["plots"].as_array().unwrap_or(&empty) {
        let Some(col) = p["id"].as_str().and_then(|id| columns.iter().position(|c| *c == id)) else {
            continue;
        };
        let mut title = p["title"].as_str().unwrap_or("Plot").to_string();
        let n = used.entry(title.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            title = format!("{title} ({n})");
        }
        let values = times
            .iter()
            .map(|t| rows.get(t).and_then(|row| row.get(col + 1)).and_then(Value::as_f64))
            .collect();
        plots.push((title, p["kind"].as_str().unwrap_or("line").to_string(), p["hidden"] == true, values));
    }
    ValuesTable { times, ohlcv, plots }
}

fn number(v: f64) -> String {
    let text = format!("{v:.8}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.to_string() }
}

fn csv_field(text: &str) -> String {
    if text.contains([',', '"', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

impl ValuesTable {
    fn csv(&self) -> String {
        let mut out = String::from("time,open,high,low,close,volume");
        for (title, ..) in &self.plots {
            out.push(',');
            out.push_str(&csv_field(title));
        }
        out.push('\n');
        for (i, t) in self.times.iter().enumerate() {
            out.push_str(&iso_s(*t));
            for v in self.ohlcv[i] {
                out.push(',');
                if let Some(v) = v {
                    out.push_str(&number(v));
                }
            }
            for (.., values) in &self.plots {
                out.push(',');
                if let Some(v) = values[i] {
                    out.push_str(&number(v));
                }
            }
            out.push('\n');
        }
        out
    }

    /// Per plot: how many bars have a value, the last one, the range; shapes count their signals.
    fn stats(&self) -> Vec<Value> {
        self.plots
            .iter()
            .map(|(title, kind, hidden, values)| {
                let present: Vec<(usize, f64)> =
                    values.iter().enumerate().filter_map(|(i, v)| v.map(|v| (i, v))).collect();
                let mut out = json!({"title": title, "kind": kind, "bars": present.len()});
                if *hidden {
                    out["hidden"] = true.into();
                }
                if kind == "shapes" {
                    let signals: Vec<i64> =
                        present.iter().filter(|(_, v)| *v != 0.0).map(|(i, _)| self.times[*i]).collect();
                    out["signals"] = signals.len().into();
                    out["lastSignals"] = signals.iter().rev().take(5).rev().map(|t| iso_s(*t)).collect::<Vec<_>>().into();
                } else if let Some(&(i, last)) = present.last() {
                    let (min, max) = present
                        .iter()
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), (_, v)| (lo.min(*v), hi.max(*v)));
                    out["last"] = json!(last);
                    out["lastTime"] = iso_s(self.times[i]).into();
                    out["min"] = json!(min);
                    out["max"] = json!(max);
                }
                out
            })
            .collect()
    }

    /// The newest rows, as the model reads them.
    fn tail(&self, n: usize) -> Vec<Value> {
        let start = self.times.len().saturating_sub(n);
        (start..self.times.len())
            .map(|i| {
                let mut row = Map::new();
                row.insert("time".into(), iso_s(self.times[i]).into());
                row.insert("close".into(), json!(self.ohlcv[i][3]));
                for (title, .., values) in &self.plots {
                    row.insert(title.clone(), json!(values[i]));
                }
                Value::Object(row)
            })
            .collect()
    }
}

/// What the script drew (labels, lines, boxes, tables), briefly.
fn drawings_brief(graphics: &Value) -> Option<Value> {
    let count = |k: &str| graphics[k].as_array().map_or(0, Vec::len);
    if ["labels", "lines", "boxes", "tables"].iter().all(|k| count(k) == 0) {
        return None;
    }
    let mut out = json!({
        "labels": count("labels"),
        "lines": count("lines"),
        "boxes": count("boxes"),
        "tables": count("tables"),
    });
    let mut labels: Vec<&Value> = graphics["labels"].as_array().map(|l| l.iter().collect()).unwrap_or_default();
    labels.sort_by_key(|l| l["t"].as_i64().unwrap_or(0));
    let recent: Vec<Value> = labels
        .iter()
        .rev()
        .take(10)
        .rev()
        .map(|l| json!({"time": l["t"].as_i64().map(iso_s), "price": l["y"], "text": l["text"]}))
        .collect();
    if !recent.is_empty() {
        out["lastLabels"] = recent.into();
    }
    // Tables are usually dashboards: their text says what the script concluded.
    let tables: Vec<Value> = graphics["tables"]
        .as_array()
        .map(|list| {
            list.iter()
                .take(2)
                .map(|t| {
                    let mut grid = vec![vec![String::new(); t["columns"].as_u64().unwrap_or(0).min(20) as usize]; t["rows"].as_u64().unwrap_or(0).min(40) as usize];
                    for c in t["cells"].as_array().into_iter().flatten() {
                        let (r, col) = (c["row"].as_u64().unwrap_or(0) as usize, c["col"].as_u64().unwrap_or(0) as usize);
                        if let Some(cell) = grid.get_mut(r).and_then(|row| row.get_mut(col)) {
                            *cell = c["text"].as_str().unwrap_or_default().to_string();
                        }
                    }
                    let mut text = String::new();
                    for row in grid {
                        let _ = writeln!(text, "{}", row.join(" | "));
                    }
                    Value::String(text)
                })
                .collect()
        })
        .unwrap_or_default();
    if !tables.is_empty() {
        out["tableText"] = tables.into();
    }
    Some(out)
}

// ---------------------------------------------------------------------------------------------
// pine_publish

pub async fn publish(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let id = library_id(require_str(args, "id")?).to_string();
    let s = local(ctx, "pine.get", json!({"id": id})).await?;
    let source = s["source"].as_str().unwrap_or_default();
    // A script that does not compile is not offered: TradingView would keep a broken version.
    let check = call_live(ctx, "pine.check", json!({"source": source}), FACADE_CALL).await?;
    if check["ok"] != true {
        let errors = placed(&check["errors"], source);
        return Ok(ToolOutput {
            ok: false,
            content: json!({"error": "It does not compile, so it was not saved to TradingView. Fix it with pine_edit first.", "errors": errors}).to_string(),
            display: json!({"kind": "pinePublish", "error": "It does not compile.", "errors": errors}),
        });
    }
    let name = arg_str(args, "name").map(str::to_string).unwrap_or_else(|| s["name"].as_str().unwrap_or_default().to_string());
    let linked = s["tradingview"]["id"].as_str().map(str::to_string);
    let card = json!({
        "kind": "pinePublish",
        "name": name,
        "lines": source.split('\n').count(),
        "update": linked.is_some(),
        "source": source,
    });
    let settings = ctx.state.settings.get();
    if !settings.always_allowed_tools.contains("pine_publish") {
        match ctx.request_approval(card.clone()).await {
            Err(Cancelled) => return Err("Cancelled.".into()),
            Ok(Approval::Deny) => {
                let content = json!({"status": "declined", "note": "The user chose not to save it to their TradingView account. Do not retry; the script stays in Demido's library, and runs on the chart from there."});
                return Ok(ToolOutput::ok(content.to_string(), json!({"kind": "pinePublish", "name": name, "denied": true})));
            }
            Ok(Approval::Always) => {
                let _ = ctx.state.settings.update(|s| {
                    s.always_allowed_tools.insert("pine_publish".into());
                });
            }
            Ok(Approval::Once) => {}
        }
    }
    let mut params = json!({"id": id});
    if arg_str(args, "name").is_some() {
        params["name"] = name.clone().into();
    }
    let published = call_live(ctx, "pine.publish", params, FACADE_CALL).await?;
    let tv = &published["tradingview"];
    let created = tv["created"] == true;
    let content = json!({
        "status": "saved",
        "tradingviewId": tv["id"],
        "version": tv["version"],
        "created": created,
        "note": if created {
            "Saved to the user's TradingView account as a new script: it is under My scripts in TradingView's Indicators menu (and Demido's), and opens in TradingView's Pine Editor. Saving it again updates that same script."
        } else {
            "Saved to the user's TradingView account as the next version of the same script."
        },
    });
    Ok(ToolOutput::ok(
        content.to_string(),
        json!({"kind": "pinePublish", "name": name, "created": created, "tradingviewId": tv["id"], "version": tv["version"]}),
    ))
}

// ---------------------------------------------------------------------------------------------
// The chart

fn chart_timeframe(args: &Value) -> Result<Option<&'static str>, String> {
    if arg_str(args, "timeframe").is_none() {
        return Ok(None);
    }
    let tf = timeframe(args)?;
    if matches!(tf, "30m" | "1M") {
        return Err("The chart has no 30m or 1M timeframe; use 1m, 5m, 15m, 1h, 4h, 1d or 1w.".into());
    }
    Ok(Some(tf))
}

pub async fn add_indicator(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let raw = require_str(args, "script")?;
    let tf = chart_timeframe(args)?;
    let (script, name) = if raw.contains(';') && !raw.starts_with(PREFIX) {
        (raw.to_string(), None)
    } else {
        let s = local(ctx, "pine.get", json!({"id": library_id(raw)})).await?;
        (format!("{PREFIX}{}", library_id(raw)), s["name"].as_str().map(str::to_string))
    };
    let command = json!({
        "action": "indicator",
        "script": script,
        "name": name,
        "inputs": args["inputs"].as_object().cloned().unwrap_or_default(),
        "symbol": arg_str(args, "symbol"),
        "timeframe": tf,
    });
    ctx.state.market.chart_command(command);
    let content = json!({
        "status": "added",
        "note": "It is on the chart in the Market window (opened if it was closed). The chart's legend shows it, or why TradingView refused it (such as the plan's limit of indicators per chart). Its values are TradingView's; read them with pine_test.",
    });
    Ok(ToolOutput::ok(
        content.to_string(),
        json!({"kind": "chartIndicator", "script": script, "name": name, "symbol": arg_str(args, "symbol")}),
    ))
}

/// A drawing time: Unix seconds (or milliseconds), or a date and time.
fn time_of(v: &Value) -> Result<i64, String> {
    match v {
        Value::Number(n) => {
            let t = n.as_f64().ok_or("A time must be a number or a date.")? as i64;
            Ok(if t > 100_000_000_000 { t / 1000 } else { t })
        }
        Value::String(s) => parse_date(s, false),
        _ => Err("Each drawing needs its time: an ISO date-time or Unix seconds.".into()),
    }
}

fn price_of(item: &Value, key: &str, kind: &str) -> Result<f64, String> {
    item[key]
        .as_f64()
        .filter(|p| p.is_finite())
        .ok_or_else(|| format!("A {kind} needs \"{key}\" (a number)."))
}

const MAX_ITEMS: usize = 500;
const MAX_POINTS: usize = 10_000;

/// Checks the drawings and puts their times in seconds; what the chart needs, nothing else.
pub(crate) fn normalize_items(items: &[Value]) -> Result<Vec<Value>, String> {
    if items.len() > MAX_ITEMS {
        return Err(format!("At most {MAX_ITEMS} drawings in one set."));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let kind = item["type"].as_str().unwrap_or_default();
        let mut d = Map::new();
        d.insert("type".into(), kind.into());
        for key in ["text", "color", "style", "extend", "position", "shape"] {
            if let Some(v) = item[key].as_str().filter(|v| !v.trim().is_empty()) {
                d.insert(key.into(), v.chars().take(500).collect::<String>().into());
            }
        }
        if let Some(w) = item["width"].as_f64() {
            d.insert("width".into(), (w.round().clamp(1.0, 4.0) as i64).into());
        }
        match kind {
            "hline" => {
                d.insert("price".into(), price_of(item, "price", kind)?.into());
            }
            "label" => {
                d.insert("time".into(), time_of(&item["time"])?.into());
                if item["position"].as_str().is_none() {
                    d.insert("price".into(), price_of(item, "price", kind)?.into());
                }
                if item["text"].as_str().is_none_or(|t| t.trim().is_empty()) {
                    return Err("A label needs its text.".into());
                }
            }
            "marker" => {
                d.insert("time".into(), time_of(&item["time"])?.into());
            }
            "line" | "box" => {
                d.insert("time".into(), time_of(&item["time"])?.into());
                d.insert("price".into(), price_of(item, "price", kind)?.into());
                d.insert("time2".into(), time_of(&item["time2"])?.into());
                d.insert("price2".into(), price_of(item, "price2", kind)?.into());
            }
            "series" => {
                let points = item["points"].as_array().ok_or("A series needs points: [[time, value], …].")?;
                if points.len() > MAX_POINTS {
                    return Err(format!("At most {MAX_POINTS} points in a series."));
                }
                let mut list = Vec::with_capacity(points.len());
                for p in points {
                    let (Some(t), Some(v)) = (p.get(0), p.get(1)) else {
                        return Err("Each point of a series is [time, value].".into());
                    };
                    list.push(json!([time_of(t)?, v.as_f64().filter(|v| v.is_finite())]));
                }
                d.insert("points".into(), list.into());
            }
            other => {
                return Err(format!(
                    "Unknown drawing type \"{other}\". Use hline, line, box, label, marker or series."
                ));
            }
        }
        out.push(Value::Object(d));
    }
    Ok(out)
}

pub async fn draw(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let name = require_str(args, "name")?;
    let tf = chart_timeframe(args)?;
    let empty = Vec::new();
    let items = normalize_items(args["items"].as_array().unwrap_or(&empty))?;
    let symbol = arg_str(args, "symbol");
    if items.is_empty() {
        ctx.state
            .market
            .chart_command(json!({"action": "undraw", "name": name}));
        let content = json!({"status": "removed", "note": if name == "*" { "Every set of drawings was removed from the chart." } else { "That set of drawings was removed from the chart." }});
        return Ok(ToolOutput::ok(content.to_string(), json!({"kind": "chartDraw", "name": name, "removed": true})));
    }
    let count = items.len();
    let pane = if args["pane"] == "separate" { "separate" } else { "overlay" };
    ctx.state.market.chart_command(json!({
        "action": "draw",
        "drawing": {"name": name, "pane": pane, "items": items},
        "symbol": symbol,
        "timeframe": tf,
    }));
    let content = json!({
        "status": "drawn",
        "items": count,
        "note": "Drawn on the chart in the Market window (opened if it was closed). The set stays until the user removes it from the legend, or chart_draw with the same name and no items removes it.",
    });
    Ok(ToolOutput::ok(
        content.to_string(),
        json!({"kind": "chartDraw", "name": name, "items": count, "symbol": symbol}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_ids_accept_the_chart_prefix() {
        assert_eq!(library_id("DEMIDO;abc"), "abc");
        assert_eq!(library_id("abc"), "abc");
    }

    #[test]
    fn compiler_messages_carry_their_line() {
        let source = "//@version=6\nindicator(\"x\")\nplot(ta.sma(close, lenx))\n";
        let placed = placed(
            &json!([{"line": 3, "column": 20, "message": "Undeclared identifier \"lenx\""}]),
            source,
        );
        assert_eq!(placed[0]["code"], "plot(ta.sma(close, lenx))");
        assert_eq!(placed[0]["line"], 3);
    }

    #[test]
    fn edits_replace_exact_text_once() {
        let src = "//@version=6\nindicator(\"x\")\nlen = 14\nplot(ta.sma(close, len))\n";
        let e = apply_edit(src, "len = 14", "len = input.int(14)", false).unwrap();
        assert_eq!(e.source, "//@version=6\nindicator(\"x\")\nlen = input.int(14)\nplot(ta.sma(close, len))\n");
        assert_eq!((e.count, e.line, e.lines), (1, 3, 1));
        let twice = apply_edit(src, "len", "n", false).unwrap_err();
        assert!(twice.contains("occurs 2 times (lines 3, 4)"), "{twice}");
        assert_eq!(apply_edit(src, "len", "n", true).unwrap().count, 2);
        assert!(apply_edit(src, "plot(x)", "plot(y)", false).unwrap_err().contains("not in the script"));
        assert!(apply_edit(src, "len = 14", "len = 14", false).is_err());
        assert!(apply_edit(src, "  ", "x", false).is_err());
    }

    #[test]
    fn edits_drop_the_line_numbers_pine_read_shows() {
        let src = "a = 1\nif a > 0\n    b = 2\n";
        let e = apply_edit(src, " 2| if a > 0\n 3|     b = 2", " 2| if a > 0\n 3|     b = 3", false).unwrap();
        assert_eq!(e.source, "a = 1\nif a > 0\n    b = 3\n");
        assert_eq!(without_numbers("10| x\n\n11|     y"), Some("x\n\n    y".into()));
        assert_eq!(without_numbers("10| x\ny"), None);
        // Windows line endings, blank lines numbered or not.
        let crlf = "a = 1\r\n\r\nb = 2\r\n";
        let e = apply_edit(crlf, "1| a = 1\n2| \n3| b = 2", "1| a = 1\n2| \n3| b = 3", false).unwrap();
        assert_eq!((e.source.as_str(), e.line), ("a = 1\r\n\r\nb = 3\r\n", 1));
        let e = apply_edit(crlf, "1| a = 1\n\n3| b = 2", "1| a = 1\n\n3| b = 4", false).unwrap();
        assert_eq!(e.source, "a = 1\r\n\r\nb = 4\r\n");
    }

    #[test]
    fn edits_take_the_indentation_the_script_has() {
        let src = "if a\n    if b\n        c = 1\n        d = 2\nplot(c)";
        // The model lost the indentation of the block it copied.
        let e = apply_edit(src, "if b\n    c = 1", "if b\n    c := 3", false).unwrap();
        assert_eq!(e.source, "if a\n    if b\n        c := 3\n        d = 2\nplot(c)");
        assert_eq!(e.line, 2);
        assert_eq!(around(&e.source, e.line, e.lines), "1| if a\n2|     if b\n3|         c := 3\n4|         d = 2\n5| plot(c)");
        let crlf = "x = 1\r\n    y = 2\r\nz = 3";
        assert_eq!(apply_edit(crlf, "y = 2\nz = 3", "y = 5\nz = 3", false).unwrap().source, "x = 1\r\n    y = 5\r\nz = 3");
        // All of it flattened: every line keeps its own indentation.
        let e = apply_edit(src, "if b\nc = 1\nd = 2", "if b\nc = 1 // one\nd = 2", false).unwrap();
        assert_eq!(e.source, "if a\n    if b\n        c = 1 // one\n        d = 2\nplot(c)");
        // A line added deeper, and one taken out of the block.
        let e = apply_edit(src, "    if b\n        c = 1", "    if b\n        c = 1\n        if c > 0\n            c := 0\n    e = 1", false).unwrap();
        assert_eq!(e.source, "if a\n    if b\n        c = 1\n        if c > 0\n            c := 0\n    e = 1\n        d = 2\nplot(c)");
        // Indented by 2 where the script indents by 4.
        let e = apply_edit(src, "if b\n  c = 1", "if b\n  c = 1\n      f = 1\ng = 1", false).unwrap();
        assert_eq!(e.source, "if a\n    if b\n        c = 1\n            f = 1\n    g = 1\n        d = 2\nplot(c)");
        let hint = apply_edit(src, "if b\n    e = 1", "if b", false).unwrap_err();
        assert!(hint.contains("first line is on line 2"), "{hint}");
    }

    #[test]
    fn scripts_are_named_by_id_or_name() {
        let list = json!([{"id": "a1", "name": "CSI PRO"}, {"id": "b2", "name": "RSI cross"}, {"id": "c3", "name": "RSI bands"}]);
        assert_eq!(by_name(&list, "csi pro").as_deref(), Some("a1"));
        assert_eq!(by_name(&list, "cross").as_deref(), Some("b2"));
        assert_eq!(by_name(&list, "RSI"), None);
        assert_eq!(by_name(&list, " "), None);
    }

    #[test]
    fn a_long_script_is_read_in_parts_with_its_compiler_messages() {
        let source: String = (1..=800).map(|i| format!("x{i} = ta.sma(close, {i}) // a line of some length\n")).collect();
        let s = json!({"id": "a1", "name": "Long", "kind": "indicator", "revision": 3});
        let check = json!({"ok": true, "errors": [], "warnings": [{"line": 2, "column": 1, "message": "Shadowed"}]});
        let first = read_text(&s, &source, 1, Some(&check), None);
        assert!(first.contains("it compiles, with 1 warning:\n- line 2 (warning): Shadowed\n    x2 = ta.sma(close, 2)"), "{first}");
        assert!(first.chars().count() <= READ_CHARS + 200);
        assert!(first.contains("\n  1| x1 = ta.sma(close, 1)"));
        let next: usize = first.rsplit("start_line ").next().unwrap().trim_end_matches('.').parse().unwrap();
        let second = read_text(&s, &source, next, None, None);
        assert!(second.contains(&format!("Lines {next} to ")), "{second}");
        assert!(second.contains("not signed in"));
        let end = read_text(&s, &source, 795, None, None);
        assert!(end.ends_with("That is the end of the script."), "{end}");
        assert!(read_text(&s, "plot(close)\n", 1, None, None).contains("Its source, 1 line (the line"));
        assert!(read_text(&s, "plot(close)", 9, None, None).ends_with("It has only 1 line."));
    }

    #[test]
    fn input_titles_become_ids() {
        let meta = json!({"inputs": [{"id": "in_0", "name": "Length"}, {"id": "in_1", "name": "Source"}]});
        assert_eq!(
            input_ids(&json!({"length": 50, "in_1": "high", "other": 1}), Some(&meta)),
            json!({"in_0": 50, "in_1": "high", "other": 1})
        );
        assert_eq!(input_ids(&Value::Null, Some(&meta)), json!({}));
    }

    fn test_result() -> Value {
        json!({
            "bars": [
                {"t": 3600, "o": 1.0, "h": 2.0, "l": 0.5, "c": 1.5, "v": 10},
                {"t": 7200, "o": 1.5, "h": 2.5, "l": 1.0, "c": 2.0, "v": 12},
            ],
            "meta": {
                "columns": ["plot_0", "plot_1", "plot_2"],
                "plots": [
                    {"id": "plot_0", "title": "SMA", "kind": "line"},
                    {"id": "plot_1", "title": "Cross", "kind": "shapes"},
                    {"id": "plot_2", "title": "SMA", "kind": "line", "hidden": true},
                ],
            },
            "rows": [[3600, null, 0, 3.0], [7200, 1.75, 1, 4.0]],
        })
    }

    #[test]
    fn a_test_becomes_a_csv_with_one_column_per_plot() {
        let table = values_table(&test_result());
        assert_eq!(
            table.csv(),
            "time,open,high,low,close,volume,SMA,Cross,SMA (2)\n\
             1970-01-01T01:00:00Z,1,2,0.5,1.5,10,,0,3\n\
             1970-01-01T02:00:00Z,1.5,2.5,1,2,12,1.75,1,4\n"
        );
        let stats = table.stats();
        assert_eq!(stats[0]["last"], 1.75);
        assert_eq!(stats[0]["bars"], 1);
        assert_eq!(stats[1]["signals"], 1);
        assert_eq!(stats[1]["lastSignals"], json!(["1970-01-01T02:00:00Z"]));
        assert_eq!(stats[2]["hidden"], true);
        assert_eq!(table.tail(1)[0]["SMA"], 1.75);
    }

    #[test]
    fn script_tables_read_as_text() {
        let g = json!({"labels": [], "lines": [], "boxes": [], "tables": [{
            "rows": 2, "columns": 2,
            "cells": [{"row": 0, "col": 0, "text": "Trend"}, {"row": 0, "col": 1, "text": "Up"}, {"row": 1, "col": 0, "text": "RSI"}],
        }]});
        let brief = drawings_brief(&g).unwrap();
        assert_eq!(brief["tableText"][0], "Trend | Up\nRSI | \n");
        assert!(drawings_brief(&json!({})).is_none());
    }

    #[test]
    fn drawings_are_checked_and_timed_in_seconds() {
        let items = normalize_items(&[
            json!({"type": "hline", "price": 1.1, "color": "#f00", "width": 9}),
            json!({"type": "line", "time": "2026-10-01T14:00:00Z", "price": 1.0, "time2": 1790000000000_i64, "price2": 1.2}),
            json!({"type": "marker", "time": 1790000000, "position": "below", "shape": "arrow_up"}),
            json!({"type": "series", "points": [[1790000000, 1.5], ["2026-10-01", null]]}),
        ])
        .unwrap();
        assert_eq!(items[0]["width"], 4);
        assert_eq!(items[1]["time"], 1_790_863_200);
        assert_eq!(items[1]["time2"], 1_790_000_000);
        assert_eq!(items[3]["points"][1], json!([1_790_812_800, null]));
        assert!(normalize_items(&[json!({"type": "label", "time": 1, "price": 1})]).is_err());
        assert!(normalize_items(&[json!({"type": "box", "time": 1, "price": 1})]).is_err());
        assert!(normalize_items(&[json!({"type": "circle"})]).is_err());
    }
}
