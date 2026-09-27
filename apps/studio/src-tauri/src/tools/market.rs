//! Market data tools. Live data needs a TradingView sign-in: when the person is not signed in,
//! the sign-in window opens and the tool waits for it; if it is not completed in time, the tool
//! tells the model to ask the person to sign in.
//!
//! History comes from the market service's local store. The one way a tool adds to that store
//! is [`ensure_downloaded`]: plan the missing part, ask the person when it would take long,
//! start (or reuse) a download job, show its progress on the tool's card and wait a while.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::{ToolContext, ToolOutput, arg_str, require_str};
use crate::agent::{Approval, Cancelled};
use crate::market::auth::{self, LoginState};

const LOGIN_WAIT: Duration = Duration::from_secs(120);
const TIMEFRAMES: &[&str] = &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M"];
/// How long a tool waits for a download before letting it finish in the background.
const DOWNLOAD_WAIT: Duration = Duration::from_secs(90);
/// The most rows the store writes into one CSV.
const EXPORT_CAP: i64 = 2_000_000;
/// Quick store calls: plans, job control, summaries.
const STORE_CALL: Duration = Duration::from_secs(60);
/// Reads that write a CSV (and may refresh the newest bucket first).
const READ_CALL: Duration = Duration::from_secs(300);
const DAY: i64 = 86_400;

pub fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Company, ticker or pair, e.g. \"apple\", \"EURUSD\", \"bitcoin\", \"S&P 500\""},
            "type": {"type": "string", "enum": ["stock", "forex", "crypto", "index", "futures", "cfd", "fund", "etf", "mutual_fund", "bond"], "description": "Optional asset class filter. Gold, silver and oil are listed as cfd. fund covers ETFs and mutual funds; etf and mutual_fund narrow to one kind"}
        },
        "required": ["query"]
    })
}

pub fn quote_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbols": {"type": "array", "items": {"type": "string"}, "description": "TradingView symbols such as FX:EURUSD, NASDAQ:AAPL, BINANCE:BTCUSDT"}
        },
        "required": ["symbols"]
    })
}

pub fn candles_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": {"type": "string", "description": "TradingView symbol such as FX:EURUSD or NASDAQ:AAPL"},
            "timeframe": {"type": "string", "enum": TIMEFRAMES},
            "bars": {"type": "integer", "description": "Number of candles back from now (default 300, max 5000). Ignored when from is given."},
            "from": {"type": "string", "description": "Optional start date, YYYY-MM-DD"},
            "to": {"type": "string", "description": "Optional end date, YYYY-MM-DD (default: now)"}
        },
        "required": ["symbol", "timeframe"]
    })
}

pub fn history_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "instrument": {"type": "string", "description": "Instrument, e.g. EURUSD, XAUUSD, BTCUSD, US500, AAPL"},
            "timeframe": {"type": "string", "enum": TIMEFRAMES},
            "from": {"type": "string", "description": "Start date, YYYY-MM-DD"},
            "to": {"type": "string", "description": "End date, YYYY-MM-DD (default: today)"}
        },
        "required": ["instrument", "timeframe", "from"]
    })
}

pub fn download_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": {"type": "string", "description": "Dukascopy instrument (EURUSD, XAUUSD, US500) or TradingView symbol (FX:EURUSD, NASDAQ:AAPL)"},
            "from": {"type": "string", "description": "Optional start date, YYYY-MM-DD (default: the earliest data the source has)"},
            "to": {"type": "string", "description": "Optional end date, YYYY-MM-DD (default: now)"},
            "timeframe": {"type": "string", "enum": TIMEFRAMES, "description": "Optional: the timeframe the user needs. Lets them choose a smaller download with only the detail it needs."}
        },
        "required": ["symbol"]
    })
}

pub fn data_status_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "symbol": {"type": "string", "description": "Optional symbol or instrument to check (EURUSD, FX:EURUSD); every stored market when omitted"}
        }
    })
}

/// Makes sure a TradingView session exists, opening the sign-in window when it does not.
async fn ensure_login(ctx: &ToolContext) -> Result<(), String> {
    let market = &ctx.state.market;
    if market.logged_in() {
        return Ok(());
    }
    let rx = auth::begin(market).await?;
    ctx.state
        .notice(&ctx.chat_id, "tradingviewLogin", "Sign in to TradingView to continue");
    tokio::select! {
        _ = ctx.cancel.cancelled() => Err("Cancelled.".into()),
        outcome = auth::wait(rx, LOGIN_WAIT) => match outcome {
            Ok(_) => Ok(()),
            Err(LoginState::Closed) => Err(
                "The user closed the TradingView sign-in window without signing in. Live market data needs a TradingView account: tell the user to sign in (Market window, Sign in button) and try again. Historical data without signing in is available through market_history.".into()
            ),
            Err(_) => Err(
                "The user is not signed in to TradingView, so live market data is unavailable. A sign-in window was opened but not completed in time. Tell the user to sign in to TradingView (Market window, Sign in button) and then ask again. Historical data without signing in is available through market_history.".into()
            ),
        }
    }
}

/// Calls a TradingView-backed method, signing in first and once more if the session expired.
async fn call_live(ctx: &ToolContext, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
    ensure_login(ctx).await?;
    match ctx.state.market.call(method, params.clone(), timeout).await {
        Err(e) if e.is_auth() => {
            ensure_login(ctx).await?;
            ctx.state
                .market
                .call(method, params, timeout)
                .await
                .map_err(|e| e.message)
        }
        other => other.map_err(|e| e.message),
    }
}

async fn store(ctx: &ToolContext, method: &str, params: Value) -> Result<Value, String> {
    ctx.state
        .market
        .call(method, params, STORE_CALL)
        .await
        .map_err(|e| e.message)
}

async fn search_symbols(ctx: &ToolContext, query: &str, kind: Option<&str>) -> Result<Vec<Value>, String> {
    let result = ctx
        .state
        .market
        .call("search", json!({"query": query, "type": kind}), Duration::from_secs(20))
        .await
        .map_err(|e| e.message)?;
    Ok(result
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .take(12)
        .collect())
}

pub async fn search(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let query = require_str(args, "query")?;
    let kind = arg_str(args, "type");
    let mut list = search_symbols(ctx, query, kind).await?;
    let mut note = None;
    if let Some(kind) = kind
        && list.is_empty()
    {
        // Models often guess the asset class wrong: TradingView lists gold as a commodity CFD,
        // not forex. Searching every class beats reporting nothing.
        list = search_symbols(ctx, query, None).await?;
        if !list.is_empty() {
            note = Some(format!(
                "Nothing matched as {kind}; these results cover every asset class."
            ));
        }
    }
    if list.is_empty() {
        return Ok(ToolOutput::ok(
            json!({"results": [], "note": format!("No symbols match \"{query}\". Try another spelling or the ticker.")}).to_string(),
            json!({"kind": "search", "query": query, "results": []}),
        ));
    }
    let compact: Vec<Value> = list
        .iter()
        .map(|s| json!({"symbol": s["symbol"], "description": s["description"], "type": s["type"], "exchange": s["exchange"]}))
        .collect();
    let mut content = json!({"results": compact});
    if let Some(note) = note {
        content["note"] = note.into();
    }
    Ok(ToolOutput::ok(
        content.to_string(),
        json!({"kind": "search", "query": query, "results": list}),
    ))
}

pub async fn quote(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let symbols: Vec<String> = match &args["symbols"] {
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Value::String(s) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        _ => Vec::new(),
    };
    if symbols.is_empty() {
        return Err("Give at least one symbol, e.g. [\"FX:EURUSD\"].".into());
    }
    let result = call_live(ctx, "quote", json!({"symbols": symbols}), Duration::from_secs(30)).await?;
    let quotes = result.as_array().cloned().unwrap_or_default();
    let compact: Vec<Value> = quotes
        .iter()
        .map(|q| {
            json!({
                "symbol": q["symbol"], "description": q["description"], "price": q["price"],
                "change": q["change"], "changePercent": q["changePercent"], "bid": q["bid"], "ask": q["ask"],
                "high": q["high"], "low": q["low"], "open": q["open"], "previousClose": q["prevClose"],
                "currency": q["currency"], "time": q["time"], "error": q["error"],
            })
        })
        .collect();
    Ok(ToolOutput::ok(
        json!({"quotes": compact}).to_string(),
        json!({"kind": "quotes", "quotes": quotes}),
    ))
}

pub async fn candles(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let symbol = require_str(args, "symbol")?;
    let timeframe = timeframe(args)?;
    let bars = arg_u64(args, "bars").unwrap_or(300).clamp(10, 5000);
    let (from, to) = parse_range(args)?;
    if let Some(from) = from {
        check_export_cap(timeframe, from, to.unwrap_or_else(now).min(now()))?;
    }
    let file = csv_file(&ctx.workspace, symbol, timeframe, from, to, bars)?;
    let mut params = json!({
        "symbol": symbol,
        "timeframe": timeframe,
        "bars": bars,
        "csvPath": file.path.to_string_lossy(),
    });
    if let Some(from) = from {
        params["from"] = from.into();
    }
    if let Some(to) = to {
        params["to"] = (to - 1).into();
    }
    read_and_fill(ctx, Read::Candles(params), symbol, timeframe, file).await
}

pub async fn history(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let instrument = require_str(args, "instrument")?;
    let timeframe = timeframe(args)?;
    let from = parse_date(require_str(args, "from")?, false)?;
    let to = arg_str(args, "to").map(|s| parse_date(s, true)).transpose()?;
    check_range(Some(from), to)?;
    check_export_cap(timeframe, from, to.unwrap_or_else(now).min(now()))?;
    let file = csv_file(&ctx.workspace, instrument, timeframe, Some(from), to, 0)?;
    let mut params = json!({
        "instrument": instrument,
        "timeframe": timeframe,
        "from": from,
        "csvPath": file.path.to_string_lossy(),
    });
    if let Some(to) = to {
        // The service's `to` is the last second included.
        params["to"] = (to - 1).into();
    }
    read_and_fill(ctx, Read::History(params), instrument, timeframe, file).await
}

/// Downloads history into the store at the best detail, waiting up to 90 s for it.
pub async fn download(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let symbol = require_str(args, "symbol")?;
    let timeframe = match arg_str(args, "timeframe") {
        Some(_) => Some(timeframe(args)?),
        None => None,
    };
    let (from, to) = parse_range(args)?;
    let d = ensure_downloaded(ctx, symbol, timeframe, from, to, true).await?;
    let (ok, content) = download_result(symbol, &d);
    Ok(ToolOutput {
        ok,
        content: content.to_string(),
        display: d.display(),
    })
}

/// What the model reads after `market_download`: the job's own counts, and when it stopped short,
/// who stopped it and what it takes to go on. Only a job that says `done` is reported as stored.
fn download_result(symbol: &str, d: &Downloaded) -> (bool, Value) {
    let plan = &d.plan;
    let mut content = json!({
        "symbol": symbol,
        "name": plan["name"],
        "source": plan["source"],
        "from": iso(&plan["from"]),
        "to": iso(&plan["to"]),
    });
    if let Some(tf) = d.only {
        content["detail"] = format!("only what {tf} and larger timeframes need").into();
    }
    if d.denied {
        content["status"] = "declined".into();
        content["note"] = "The user chose not to download this. Do not retry it; continue with what is stored (market_data_status shows it) or ask the user.".into();
        return (true, content);
    }
    let Some(job_id) = &d.job_id else {
        content["status"] = "done".into();
        content["note"] = match d.only {
            Some(tf) => format!(
                "Everything {tf} needs in this range is already stored; nothing was downloaded. Read it with market_history or market_candles at {tf} or a larger timeframe (smaller ones need the full download)."
            ),
            None => "Everything in this range is already stored; nothing was downloaded. Read it with market_history or market_candles at any timeframe.".into(),
        }
        .into();
        return (true, content);
    };
    content["jobId"] = job_id.as_str().into();
    let status = d.job_status();
    content["status"] = status.into();
    d.put_counts(content.as_object_mut().expect("object"));
    if let Some(error) = &d.job_error {
        content["error"] = error.as_str().into();
    }
    content["note"] = match (d.stopped_note(), status) {
        (Some(note), _) => note,
        (None, "done") => match d.only {
            Some(tf) => format!(
                "The download finished: everything {tf} needs is stored. Read it with market_history or market_candles at {tf} or a larger timeframe."
            ),
            None => "The download finished: the history is stored. Read it with market_history or market_candles at any timeframe.".into(),
        },
        (None, _) => {
            content["etaSeconds"] = json!(d.eta_seconds);
            background_note(d.eta_seconds)
        }
    }
    .into();
    (d.job_error.is_none(), content)
}

/// A compact view of what is stored, for deciding what (not) to download.
pub async fn data_status(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let symbol = arg_str(args, "symbol");
    let params = match symbol {
        Some(symbol) => json!({"symbol": symbol}),
        None => json!({}),
    };
    let summary = store(ctx, "cache.summary", params).await?;
    let items = summary["items"].as_array().cloned().unwrap_or_default();
    let markets: Vec<Value> = items.iter().map(compact_market).collect();
    let note = match (markets.is_empty(), symbol) {
        (true, Some(s)) => format!("Nothing is stored for {s} yet."),
        (true, None) => "No market history is stored yet.".to_string(),
        _ => "Times are UTC and ranges are [from, to): the end is excluded. gaps: periods the source has that are not stored. noData: periods the source has no candles for (weekends, holidays).".to_string(),
    };
    let content = json!({"totalSize": fmt_bytes(num(&summary, "bytes")), "markets": markets, "note": note});
    Ok(ToolOutput::ok(
        content.to_string(),
        json!({"kind": "dataStatus", "symbol": symbol, "bytes": summary["bytes"], "items": items}),
    ))
}

// ---------------------------------------------------------------------------------------------
// Downloads

/// What to do about a plan.
#[derive(Debug, PartialEq)]
enum Next {
    /// Nothing is missing.
    Covered,
    /// An unfinished job already covers the range: it was approved when it started. `resume` when
    /// it is paused or stopped by an error, so asking for the data again restarts it.
    Reuse {
        job_id: String,
        resume: bool,
    },
    /// Long enough to ask the person first.
    Ask,
    Start,
}

fn next_step(plan: &Value, approval_seconds: u32) -> Next {
    if plan["complete"].as_bool() == Some(true) {
        return Next::Covered;
    }
    if let Some(id) = plan["job"]["id"].as_str() {
        return Next::Reuse {
            job_id: id.to_string(),
            resume: matches!(plan["job"]["status"].as_str(), Some("paused" | "error")),
        };
    }
    if num(plan, "seconds") > f64::from(approval_seconds) {
        Next::Ask
    } else {
        Next::Start
    }
}

/// A full download long enough to ask about may already be settled by the smaller "only this
/// timeframe" plan: stored for that timeframe (`Covered`), or fetched by an unfinished job, the one
/// the person started by choosing "Only {timeframe}" earlier (`Reuse`). That job was approved, so
/// asking again would offer only the full download they turned down, or nothing.
fn settled_by(minimal: &Value) -> Option<Next> {
    match next_step(minimal, u32::MAX) {
        step @ (Next::Covered | Next::Reuse { .. }) => Some(step),
        Next::Ask | Next::Start => None,
    }
}

/// The "only this timeframe" choice is worth showing when it saves real work (a tenth or more).
/// Buckets another job already queued count as work: they are what this timeframe waits on.
fn offer_minimal(full: &Value, minimal: &Value) -> bool {
    let work = |plan: &Value| num(plan, "requests") + num(plan, "queuedAhead");
    minimal["complete"].as_bool() != Some(true) && work(minimal) > 0.0 && work(minimal) <= work(full) * 0.9
}

/// The timeframes a TradingView download fetches (the chart's).
const TV_DOWNLOAD_TIMEFRAMES: &[&str] = &["1m", "5m", "15m", "1h", "4h", "1d", "1w"];

/// What "only this timeframe" downloads: the coarsest Dukascopy tier that serves it, or the
/// TradingView timeframe itself. One-minute data would serve it too, but a list including it
/// costs as much as the full download, so below an hour there is no smaller choice.
fn minimal_tiers(source: &str, timeframe: &str) -> Option<Vec<&'static str>> {
    if source == "tradingview" {
        return TV_DOWNLOAD_TIMEFRAMES
            .iter()
            .find(|t| **t == timeframe)
            .map(|t| vec![*t]);
    }
    match timeframe {
        "1d" | "1w" | "1M" => Some(vec!["d1"]),
        "1h" | "4h" => Some(vec!["h1"]),
        _ => None,
    }
}

fn range_params(symbol: &str, from: Option<i64>, to: Option<i64>, tiers: Option<&[&str]>) -> Value {
    let mut params = json!({"symbol": symbol});
    if let Some(from) = from {
        params["from"] = from.into();
    }
    if let Some(to) = to {
        params["to"] = to.into();
    }
    if let Some(tiers) = tiers {
        params["tiers"] = json!(tiers);
    }
    params
}

/// What [`ensure_downloaded`] did.
#[derive(Debug, Default)]
struct Downloaded {
    /// The plan shown and started (the smaller one when the person chose it).
    plan: Value,
    /// The timeframe whose smaller download `plan` is (chosen, reused or already stored).
    only: Option<&'static str>,
    job_id: Option<String>,
    /// The range is stored for the timeframe that was waited on.
    covered: bool,
    /// The job's status when the wait ended; `cancelled` once its record is gone.
    status: Option<String>,
    eta_seconds: Option<f64>,
    /// The job's own counts when last seen: files done and in total, bytes stored.
    files: Option<(u64, u64)>,
    bytes: Option<f64>,
    /// What the job says about itself, e.g. why it is waiting.
    message: Option<String>,
    /// The person chose not to download.
    denied: bool,
    /// Why the job failed (or why following it did).
    job_error: Option<String>,
}

impl Downloaded {
    /// A job that exists and has not finished (paused counts: it can be resumed).
    fn unfinished(&self) -> bool {
        self.job_id.is_some()
            && self.job_error.is_none()
            && (is_active(self.status.as_deref()) || self.status.as_deref() == Some("paused"))
    }

    /// Takes in what `download.status` said about the job; `null` means its record is gone, which
    /// only a cancel does. The counts are the job's, never the plan's: a plan counts only the files
    /// its call adds.
    fn note_job(&mut self, job: &Value) {
        if job.is_null() {
            self.status = Some("cancelled".into());
            return;
        }
        let count = |key: &str| job[key].as_f64().map(|n| n.max(0.0) as u64);
        self.status = job["status"].as_str().map(str::to_string);
        self.eta_seconds = job["etaSeconds"].as_f64();
        if let (Some(done), Some(total)) = (count("done"), count("total")) {
            self.files = Some((done, total));
        }
        self.bytes = job["bytes"].as_f64().or(self.bytes);
        self.message = job["message"].as_str().filter(|m| !m.is_empty()).map(str::to_string);
        // A fresh status replaces an older failure: "Try again" may have restarted the job.
        self.job_error = (self.status.as_deref() == Some("error"))
            .then(|| self.message.clone().unwrap_or_else(|| "The download failed.".into()));
    }

    /// The job's status for the model; `unknown` when the service could not say.
    fn job_status(&self) -> &str {
        match (self.status.as_deref(), &self.job_error) {
            (Some(status), _) => status,
            (None, Some(_)) => "unknown",
            (None, None) => "running",
        }
    }

    /// `filesDone`, `filesTotal` and `size` from the job's counts, when known.
    fn put_counts(&self, into: &mut Map<String, Value>) {
        if let Some((done, total)) = self.files {
            into.insert("filesDone".into(), done.into());
            into.insert("filesTotal".into(), total.into());
        }
        if let Some(bytes) = self.bytes {
            into.insert("size".into(), fmt_bytes(bytes).into());
        }
        if let Some(message) = &self.message {
            into.insert("message".into(), message.as_str().into());
        }
    }

    /// Why the job will not go on by itself, in words the model can pass on: someone has to act.
    /// `None` while it runs (or waits out a throttle) and once it is done.
    fn stopped_note(&self) -> Option<String> {
        let at = match self.files {
            Some((done, total)) if total > 0 => {
                format!(" at {} of {} files", thousands(done as i64), thousands(total as i64))
            }
            _ => String::new(),
        };
        match self.status.as_deref() {
            Some("paused") => Some(format!(
                "The user paused the download{at}. It will not continue until they press Resume on its card in the chat or in the Data tab. Asking for this data again resumes it, so do that only if the user wants it to go on."
            )),
            Some("cancelled") => Some(format!(
                "The user cancelled the download{at}, so it will not continue; what it downloaded stays stored. Do not start it again unless the user asks."
            )),
            Some("error") => Some(format!(
                "The download stopped with an error{at} ({}). It will not continue until the user presses Try again on its card in the chat or in the Data tab.",
                self.job_error.as_deref().unwrap_or("no reason given")
            )),
            None => self
                .job_error
                .as_ref()
                .map(|e| format!("Could not check on the download ({e}); its card in the chat shows how it is going.")),
            Some(_) => None,
        }
    }

    /// The card's download display, `{kind, jobId, plan, status?, jobError?, note?, denied?}`.
    fn display(&self) -> Value {
        let mut d = json!({"kind": "download", "jobId": self.job_id, "plan": self.plan});
        let already_stored = self.job_id.is_none() && self.covered;
        let status = match () {
            _ if self.denied => Some("denied"),
            _ if already_stored => Some("done"),
            _ => self.status.as_deref(),
        };
        if let Some(status) = status {
            d["status"] = status.into();
        }
        if let Some(error) = &self.job_error {
            d["jobError"] = error.as_str().into();
        }
        if self.denied {
            // The chat's cards test this flag, like other declined tools' results.
            d["denied"] = true.into();
            d["note"] = "You chose not to download this.".into();
        } else if already_stored {
            d["note"] = match self.only {
                Some(tf) => format!("Everything {tf} needs is already stored: nothing to download."),
                None => "Already stored: nothing to download.".into(),
            }
            .into();
        }
        d
    }

    /// Facts about the download for the model reading a data result, and a note to add.
    fn facts(&self) -> (Map<String, Value>, Option<String>) {
        let mut facts = Map::new();
        if self.denied {
            facts.insert("download".into(), "declined".into());
            return (facts, Some(DECLINED_NOTE.into()));
        }
        // With the range covered the job no longer matters to this read; once it is done, what it
        // could not get is what `missing` lists.
        let status = self.job_status();
        if self.covered || self.job_id.is_none() || status == "done" {
            return (facts, None);
        }
        facts.insert("status".into(), status.into());
        facts.insert("jobId".into(), json!(self.job_id));
        self.put_counts(&mut facts);
        if let Some(error) = &self.job_error {
            facts.insert("downloadError".into(), error.as_str().into());
        }
        if let Some(note) = self.stopped_note() {
            return (facts, Some(note));
        }
        facts.insert("etaSeconds".into(), json!(self.eta_seconds));
        (facts, Some(background_note(self.eta_seconds)))
    }
}

const DECLINED_NOTE: &str = "The user chose not to download the missing history, so this has only what was already stored (see missing). Do not retry the download; answer with this data or ask the user.";

fn background_note(eta_seconds: Option<f64>) -> String {
    let lead = "The download continues in the background; the progress bar in the chat shows it.";
    match eta_seconds.filter(|s| s.is_finite() && *s > 0.0) {
        Some(s) => {
            let minutes = (s / 60.0).ceil().max(1.0) as u64;
            let unit = if minutes == 1 { "minute" } else { "minutes" };
            format!("{lead} Tell the user the full data will be ready in about {minutes} {unit}.")
        }
        None => format!("{lead} Tell the user the full data will be ready when it finishes."),
    }
}

/// Makes sure `[from, to)` of `symbol` is in the store: plan it (all tiers), sign in when the
/// data comes from TradingView, reuse an unfinished job (also one fetching just what `timeframe`
/// needs) or ask when the estimate is over the person's limit, start, show the job on the card
/// and wait. `whole_job` waits for the whole
/// download instead of just `timeframe`'s data. Once a job exists, failures are reported in
/// `job_error` rather than as an error, so the card keeps its progress bar.
async fn ensure_downloaded(
    ctx: &ToolContext,
    symbol: &str,
    timeframe: Option<&'static str>,
    from: Option<i64>,
    to: Option<i64>,
    whole_job: bool,
) -> Result<Downloaded, String> {
    let plan = store(ctx, "download.plan", range_params(symbol, from, to, None)).await?;
    let source = plan["source"].as_str().unwrap_or("dukascopy").to_string();
    let mut out = Downloaded {
        plan: plan.clone(),
        ..Downloaded::default()
    };
    let mut next = next_step(&plan, ctx.state.settings.get().download_approval_seconds);
    let smaller = timeframe.and_then(|tf| minimal_tiers(&source, tf));
    let mut tiers = None;
    let mut offer = None;
    // Before asking about the full download, see whether the smaller one settles it.
    if next == Next::Ask
        && let Some(list) = &smaller
    {
        let minimal = store(ctx, "download.plan", range_params(symbol, from, to, Some(list))).await?;
        match settled_by(&minimal) {
            Some(step) => {
                next = step;
                tiers = smaller.clone();
                out.plan = minimal;
                out.only = timeframe;
            }
            None => offer = Some(minimal).filter(|m| offer_minimal(&plan, m)),
        }
    }
    if next == Next::Covered {
        out.covered = true;
        return Ok(out);
    }
    if source == "tradingview" {
        ensure_login(ctx).await?;
    }
    let job_id = match next {
        Next::Reuse { job_id, resume } => {
            if resume {
                store(ctx, "download.resume", json!({"jobId": job_id})).await?;
            }
            job_id
        }
        next => {
            if next == Next::Ask {
                let card = json!({"kind": "download", "plan": plan, "minimal": offer, "timeframe": timeframe});
                match ctx.request_approval(card).await {
                    Err(Cancelled) => return Err("Cancelled.".into()),
                    Ok(Approval::Deny) => {
                        out.denied = true;
                        return Ok(out);
                    }
                    Ok(Approval::Minimal) => {
                        // Only offered with a smaller plan; an answer without one gets everything.
                        if let Some(minimal) = offer {
                            tiers = smaller;
                            out.plan = minimal;
                            out.only = timeframe;
                        }
                    }
                    Ok(Approval::Once | Approval::Always) => {}
                }
            }
            start_download(ctx, symbol, from, to, tiers.as_deref()).await?
        }
    };
    out.job_id = Some(job_id.clone());
    ctx.set_display(out.display());
    // A download of every tier is complete once its finest detail is.
    let wait_on = match (whole_job && tiers.is_none(), timeframe) {
        (false, Some(tf)) => tf,
        _ => "1m",
    };
    follow(ctx, &mut out, &job_id, symbol, wait_on, (from, to), whole_job).await?;
    Ok(out)
}

async fn start_download(
    ctx: &ToolContext,
    symbol: &str,
    from: Option<i64>,
    to: Option<i64>,
    tiers: Option<&[&str]>,
) -> Result<String, String> {
    let mut params = range_params(symbol, from, to, tiers);
    params["origin"] = "chat".into();
    let started = match ctx
        .state
        .market
        .call("download.start", params.clone(), STORE_CALL)
        .await
    {
        Err(e) if e.is_auth() => {
            ensure_login(ctx).await?;
            store(ctx, "download.start", params).await?
        }
        other => other.map_err(|e| e.message)?,
    };
    started["jobId"]
        .as_str()
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "The download could not be started.".to_string())
}

fn is_active(status: Option<&str>) -> bool {
    matches!(status, Some("queued" | "running" | "waiting"))
}

/// Waits, cancellably and at most [`DOWNLOAD_WAIT`], until the range is stored for `timeframe`
/// (with `whole_job`, until the job ends), then records the job's state. `Err` only when the turn
/// is stopped.
async fn follow(
    ctx: &ToolContext,
    out: &mut Downloaded,
    job_id: &str,
    symbol: &str,
    timeframe: &str,
    (from, to): (Option<i64>, Option<i64>),
    whole_job: bool,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + DOWNLOAD_WAIT;
    let now = now();
    let seconds = |v: &Value| v.as_f64().map(|s| s as i64);
    let to = seconds(&out.plan["to"]).or(to).unwrap_or(now).min(now);
    let from = seconds(&out.plan["from"]).or(from).unwrap_or(0);
    let market = &ctx.state.market;
    let params = wait_params(symbol, timeframe, (from, to), job_id, whole_job);
    let wait = async {
        if from >= to {
            return std::future::pending().await;
        }
        market
            .call("download.wait", params, DOWNLOAD_WAIT + Duration::from_secs(15))
            .await
    };
    // The job as last seen, so its counts survive a cancel that removes it.
    let seen = parking_lot::Mutex::new(None::<Value>);
    // A job that ends (done, failed, paused, removed) without covering the range, say over a
    // hole at the source, must not hold the tool until the timeout.
    let ended = async {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            match market
                .call("download.status", json!({"jobId": job_id}), STORE_CALL)
                .await
            {
                Ok(job) if !job.is_null() => {
                    let moving = is_active(job["status"].as_str());
                    *seen.lock() = Some(job);
                    if !moving {
                        return;
                    }
                }
                _ => return,
            }
        }
    };
    {
        tokio::pin!(wait, ended);
        let mut waiting = true;
        loop {
            tokio::select! {
                _ = ctx.cancel.cancelled() => return Err("Cancelled.".into()),
                waited = &mut wait, if waiting => {
                    waiting = false;
                    match waited {
                        Ok(w) => {
                            out.covered = w["covered"].as_bool().unwrap_or(false);
                            out.status = w["status"].as_str().map(str::to_string);
                            if !whole_job {
                                break;
                            }
                        }
                        // Following the job still works without it.
                        Err(e) => tracing::warn!("waiting for download {job_id} failed: {}", e.message),
                    }
                }
                _ = &mut ended => break,
                _ = tokio::time::sleep_until(deadline) => break,
            }
        }
    }
    if let Some(job) = seen.into_inner() {
        out.note_job(&job);
    }
    record_job(ctx, out, job_id).await;
    Ok(())
}

/// `download.wait`'s params. Waiting on part of a job (a timeframe's data) moves that part into
/// the service's waiting lane; a whole-job wait must not, or it would override the job's own
/// order (hourly and daily first) and the fair share between jobs.
fn wait_params(symbol: &str, timeframe: &str, (from, to): (i64, i64), job_id: &str, whole_job: bool) -> Value {
    json!({
        "symbol": symbol,
        "timeframe": timeframe,
        "from": from,
        "to": to,
        "jobId": job_id,
        "timeoutMs": DOWNLOAD_WAIT.as_millis() as u64,
        "boost": !whole_job,
    })
}

/// Reads the job's state after a wait: its status and counts, `cancelled` when it is gone.
async fn record_job(ctx: &ToolContext, out: &mut Downloaded, job_id: &str) {
    match ctx
        .state
        .market
        .call("download.status", json!({"jobId": job_id}), STORE_CALL)
        .await
    {
        Ok(job) => out.note_job(&job),
        Err(e) => {
            if out.status.is_none() {
                out.job_error = Some(e.message);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reading data into the workspace

/// A read of the store that also writes the workspace CSV.
enum Read {
    History(Value),
    Candles(Value),
}

impl Read {
    async fn run(&self, ctx: &ToolContext) -> Result<Value, String> {
        match self {
            Read::History(p) => ctx
                .state
                .market
                .call("history", p.clone(), READ_CALL)
                .await
                .map_err(|e| e.message),
            Read::Candles(p) => call_live(ctx, "candles", p.clone(), READ_CALL).await,
        }
    }
}

/// Reads what is stored; when part of the range is missing, downloads it (asking when long)
/// and reads again.
async fn read_and_fill(
    ctx: &ToolContext,
    read: Read,
    symbol: &str,
    timeframe: &'static str,
    file: CsvFile,
) -> Result<ToolOutput, String> {
    let mut result = read.run(ctx).await?;
    let mut download = None;
    let mut download_error = None;
    if let Some((from, to)) = extent(&ranges(&result["missing"])) {
        match ensure_downloaded(ctx, symbol, Some(timeframe), Some(from), Some(to), false).await {
            Ok(d) => {
                if d.job_id.is_some() {
                    match read.run(ctx).await {
                        Ok(again) => result = again,
                        Err(e) => tracing::warn!("reading {symbol} again after its download failed: {e}"),
                    }
                }
                download = Some(d);
            }
            Err(e) if ctx.cancel.is_cancelled() => return Err(e),
            // What is stored is still worth returning.
            Err(e) => download_error = Some(e),
        }
    }
    summarize(&result, symbol, timeframe, &file, download.as_ref(), download_error)
}

/// Builds the model's summary and the UI's candles preview from the service's CSV summary.
fn summarize(
    result: &Value,
    requested: &str,
    timeframe: &str,
    file: &CsvFile,
    download: Option<&Downloaded>,
    download_error: Option<String>,
) -> Result<ToolOutput, String> {
    let count = result["count"].as_u64().unwrap_or(0);
    let symbol = result["symbol"].as_str().unwrap_or(requested).to_string();
    let description = result["info"]["description"].as_str().unwrap_or_default().to_string();
    let (facts, download_note) = download.map(Downloaded::facts).unwrap_or_default();

    if count == 0 {
        let _ = std::fs::remove_file(&file.path);
        let Some(d) = download.filter(|d| d.job_id.is_some() || d.denied) else {
            return Err(match download_error {
                Some(e) => format!(
                    "No candles are stored for {symbol} ({timeframe}) in that range, and the missing part could not be downloaded: {e}"
                ),
                None => format!(
                    "No candles were found for {symbol} ({timeframe}). Check the symbol with market_search or pick another range."
                ),
            });
        };
        let mut content = json!({"symbol": symbol, "timeframe": timeframe, "candles": 0});
        content.as_object_mut().expect("object").extend(facts);
        if let Some(note) = download_note {
            content["note"] = note.into();
        }
        return Ok(ToolOutput {
            ok: d.job_error.is_none(),
            content: content.to_string(),
            display: d.display(),
        });
    }

    let missing = ranges(&result["missing"]);
    let truncated = result["truncated"].as_bool().unwrap_or(false);
    let spans = result["spans"].as_array().cloned().unwrap_or_default();
    let first_close = num(result, "firstClose");
    let last_close = num(result, "lastClose");
    let change_pct = if first_close != 0.0 {
        (last_close - first_close) / first_close * 100.0
    } else {
        0.0
    };
    let mut notes = vec![
        "The full data is in the CSV file (with a source column); load it with pandas in run_python to analyse it."
            .to_string(),
    ];
    let mut summary = json!({
        "symbol": symbol,
        "description": description,
        "timeframe": timeframe,
        "candles": count,
        "from": iso(&result["from"]),
        "to": iso(&result["to"]),
        "file": file.rel,
        "firstClose": result["firstClose"],
        "lastClose": result["lastClose"],
        "changePercent": (change_pct * 100.0).round() / 100.0,
        "high": result["high"],
        "low": result["low"],
        "sources": spans
            .iter()
            .map(|s| json!({"source": s["source"], "from": iso(&s["from"]), "to": iso(&s["to"]), "count": s["count"]}))
            .collect::<Vec<_>>(),
        "lastRows": last_rows_text(&result["lastRows"]),
    });
    if !missing.is_empty() {
        summary["missing"] = json!(fmt_ranges(&missing, 12));
        notes.push("Parts of the range are not stored; they are listed in missing.".into());
    }
    if truncated {
        summary["truncated"] = true.into();
        notes.push(format!(
            "Only the newest {count} candles fit; ask for a shorter range or a larger timeframe for the rest."
        ));
    }
    summary.as_object_mut().expect("object").extend(facts);
    if let Some(error) = &download_error {
        summary["downloadError"] = error.as_str().into();
    }
    notes.extend(download_note);
    summary["note"] = notes.join(" ").into();

    let mut display = json!({
        "kind": "candles",
        "symbol": symbol,
        "description": description,
        "timeframe": timeframe,
        "count": count,
        "from": iso(&result["from"]),
        "to": iso(&result["to"]),
        "file": file.rel,
        "path": file.path.to_string_lossy(),
        "lastClose": result["lastClose"],
        "changePercent": change_pct,
        "high": result["high"],
        "low": result["low"],
        "closes": result["closes"],
        "sources": sources_by_source(&spans),
        "spans": spans,
        "missing": missing.iter().map(|&(f, t)| json!([f, t])).collect::<Vec<_>>(),
        "truncated": truncated,
    });
    if let Some(d) = download {
        // A job still going (or failed) keeps its progress bar under the preview.
        if d.unfinished() || (d.job_id.is_some() && d.job_error.is_some()) {
            display["download"] = d.display();
        }
        if d.denied {
            display["denied"] = true.into();
            display["note"] = "You chose not to download the missing history; this is what was already stored.".into();
        }
    }
    Ok(ToolOutput::ok(summary.to_string(), display))
}

/// One count per source, in the order the sources first appear (what the preview's chips show).
fn sources_by_source(spans: &[Value]) -> Vec<Value> {
    let mut totals: Vec<(String, u64)> = Vec::new();
    for span in spans {
        let source = span["source"].as_str().unwrap_or("dukascopy");
        let count = span["count"].as_u64().unwrap_or(0);
        match totals.iter_mut().find(|(s, _)| s == source) {
            Some((_, total)) => *total += count,
            None => totals.push((source.to_string(), count)),
        }
    }
    totals
        .into_iter()
        .map(|(source, count)| json!({"source": source, "count": count}))
        .collect()
}

/// The service's last rows as CSV text, whether it sent text, lines, arrays or bar objects.
fn last_rows_text(rows: &Value) -> String {
    const HEADER: &str = "time,open,high,low,close,volume,source";
    let cell = |v: &Value| match v {
        Value::Number(n) => n.as_f64().map(fmt_num).unwrap_or_default(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    };
    match rows {
        Value::String(s) => s.clone(),
        Value::Array(list) => {
            let mut lines: Vec<String> = Vec::new();
            for row in list {
                let line = match row {
                    Value::String(s) => s.clone(),
                    Value::Array(cells) => cells
                        .iter()
                        .enumerate()
                        .map(|(i, c)| if i == 0 { iso_or_text(c) } else { cell(c) })
                        .collect::<Vec<_>>()
                        .join(","),
                    Value::Object(_) => {
                        let time = if row["time"].is_null() { &row["t"] } else { &row["time"] };
                        let pick = |long: &str, short: &str| {
                            if row[long].is_null() {
                                cell(&row[short])
                            } else {
                                cell(&row[long])
                            }
                        };
                        [
                            iso_or_text(time),
                            pick("open", "o"),
                            pick("high", "h"),
                            pick("low", "l"),
                            pick("close", "c"),
                            pick("volume", "v"),
                            cell(&row["source"]),
                        ]
                        .join(",")
                    }
                    _ => continue,
                };
                lines.push(line);
            }
            if lines.first().is_some_and(|l| !l.starts_with("time")) && !list.iter().all(Value::is_string) {
                lines.insert(0, HEADER.to_string());
            }
            lines.join("\n")
        }
        _ => String::new(),
    }
}

/// Where a read's CSV goes: named after the symbol, timeframe and requested range, so the
/// same question (and the read after a download) overwrites the same file.
struct CsvFile {
    rel: String,
    path: PathBuf,
}

fn csv_file(
    workspace: &Path,
    symbol: &str,
    timeframe: &str,
    from: Option<i64>,
    to: Option<i64>,
    bars: u64,
) -> Result<CsvFile, String> {
    let safe: String = symbol
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    // Windows file names ignore case: monthly (1M) would overwrite minute (1m) data.
    let tf = if timeframe == "1M" { "1mo" } else { timeframe };
    let last_day = compact_day(to.map(|t| t - 1).unwrap_or_else(now).min(now()));
    let name = match from {
        Some(from) => format!("{safe}_{tf}_{}_{last_day}.csv", compact_day(from)),
        None => format!("{safe}_{tf}_last{bars}_{last_day}.csv"),
    };
    let dir = workspace.join("data");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create the data folder: {e}"))?;
    Ok(CsvFile {
        rel: format!("data/{name}"),
        path: dir.join(name),
    })
}

// ---------------------------------------------------------------------------------------------
// Stored-data summaries

fn compact_market(item: &Value) -> Value {
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let sources: Vec<Value> = list(&item["sources"])
        .iter()
        .map(|s| {
            json!({
                "source": s["source"],
                "key": s["key"],
                "size": fmt_bytes(num(s, "bytes")),
                "detail": list(&s["tiers"]).iter().map(compact_tier).collect::<Vec<_>>(),
            })
        })
        .collect();
    let downloads: Vec<Value> = list(&item["jobs"])
        .iter()
        .map(|j| {
            json!({
                "jobId": j["id"], "status": j["status"], "done": j["done"], "total": j["total"],
                "from": iso(&j["from"]), "to": iso(&j["to"]), "etaSeconds": j["etaSeconds"], "message": j["message"],
            })
        })
        .collect();
    json!({
        "market": item["market"],
        "name": item["name"],
        "symbols": item["symbols"],
        "sources": sources,
        "downloads": downloads,
    })
}

fn compact_tier(tier: &Value) -> Value {
    let id = tier["tier"].as_str().unwrap_or_default();
    let stored = ranges(&tier["intervals"]);
    let empty = ranges(&tier["empty"]);
    let mut out = json!({
        "tier": id,
        "detail": tier_label(id),
        "size": fmt_bytes(num(tier, "bytes")),
        "stored": fmt_ranges(&stored, 12),
    });
    if let Some((start, end)) = pair(&tier["available"]) {
        out["available"] = fmt_range(start, end).into();
        let mut holes = gaps((start, end), &[stored, empty.clone()].concat());
        // The newest hours are fetched by the next read anyway; they are not worth a download.
        holes.retain(|&(f, t)| t < end || t - f >= DAY);
        out["gaps"] = json!(fmt_ranges(&holes, 12));
    }
    if !empty.is_empty() {
        out["noData"] = json!(fmt_ranges(&empty, 6));
    }
    if let Some(start) = tier["learnedStart"].as_f64() {
        out["learnedStart"] = fmt_time(start as i64).into();
    }
    out
}

fn tier_label(tier: &str) -> String {
    match tier {
        "m1" => "1-minute".into(),
        "h1" => "hourly".into(),
        "d1" => "daily".into(),
        other => format!("{other} candles"),
    }
}

/// `available` minus `covered`: the stretches nothing covers.
fn gaps(available: (i64, i64), covered: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut sorted = covered.to_vec();
    sorted.sort_unstable();
    let (mut cursor, end) = available;
    let mut out = Vec::new();
    for (f, t) in sorted {
        if t <= cursor {
            continue;
        }
        if f >= end {
            break;
        }
        if f > cursor {
            out.push((cursor, f));
        }
        cursor = cursor.max(t);
    }
    if cursor < end {
        out.push((cursor, end));
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Arguments and formatting

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn timeframe(args: &Value) -> Result<&'static str, String> {
    let raw = require_str(args, "timeframe")?;
    let normalized = match raw.trim() {
        "1M" | "1mo" | "1month" | "monthly" | "M" => "1M",
        other => match other.to_ascii_lowercase().as_str() {
            "1m" | "1min" | "m1" => "1m",
            "5m" | "5min" | "m5" => "5m",
            "15m" | "15min" | "m15" => "15m",
            "30m" | "30min" | "m30" => "30m",
            "1h" | "60m" | "h1" | "60" => "1h",
            "4h" | "h4" | "240" => "4h",
            "1d" | "d" | "d1" | "daily" | "1day" => "1d",
            "1w" | "w" | "w1" | "weekly" | "1week" => "1w",
            _ => {
                return Err(format!(
                    "Unknown timeframe {raw}. Use one of {}.",
                    TIMEFRAMES.join(", ")
                ));
            }
        },
    };
    Ok(TIMEFRAMES.iter().find(|t| **t == normalized).copied().unwrap_or("1d"))
}

fn timeframe_seconds(timeframe: &str) -> i64 {
    match timeframe {
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "30m" => 1800,
        "1h" => 3600,
        "4h" => 14_400,
        "1d" => DAY,
        "1w" => 7 * DAY,
        _ => 30 * DAY,
    }
}

/// A whole number argument, also when the model sends it as a string.
fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    match &args[key] {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// A date argument in seconds: `YYYY-MM-DD` (or `YYYYMMDD`, `YYYY-MM`, `YYYY`, an ISO time, Unix
/// seconds or milliseconds). With `end`, a bare date means the end of that period: `to: 2024-03-31`
/// includes the 31st.
fn parse_date(raw: &str, end: bool) -> Result<i64, String> {
    use chrono::{Months, NaiveDate, NaiveDateTime};
    let text = raw.trim();
    let bad = || format!("Could not read the date \"{text}\". Use YYYY-MM-DD.");
    let midnight = |d: NaiveDate| d.and_hms_opt(0, 0, 0).map(|t| t.and_utc().timestamp());
    if text.len() > 4 && text.bytes().all(|b| b.is_ascii_digit()) {
        // Every Unix time since March 1973 has nine or more digits; eight are a compact day. Five
        // to seven (YYYYMM or YYMMDD?) are too ambiguous to guess at.
        if text.len() == 8 {
            let start = NaiveDate::parse_from_str(text, "%Y%m%d").map_err(|_| bad())?;
            let day = if end { start.succ_opt().ok_or_else(bad)? } else { start };
            return midnight(day).ok_or_else(bad);
        }
        if text.len() < 9 {
            return Err(bad());
        }
        let n: i64 = text.parse().map_err(|_| bad())?;
        return Ok(if n > 100_000_000_000 { n / 1000 } else { n });
    }
    let int = |s: &str| s.parse::<u32>().ok();
    let parts: Vec<&str> = text.split('-').collect();
    let period = match parts.as_slice() {
        [y] if y.len() == 4 => {
            let start = NaiveDate::from_ymd_opt(y.parse().map_err(|_| bad())?, 1, 1).ok_or_else(bad)?;
            Some((start, start.checked_add_months(Months::new(12))))
        }
        [y, m] if y.len() == 4 && m.len() <= 2 => {
            let start =
                NaiveDate::from_ymd_opt(y.parse().map_err(|_| bad())?, int(m).ok_or_else(bad)?, 1).ok_or_else(bad)?;
            Some((start, start.checked_add_months(Months::new(1))))
        }
        [y, m, d] if y.len() == 4 && m.len() <= 2 && d.len() <= 2 => {
            let start = NaiveDate::from_ymd_opt(
                y.parse().map_err(|_| bad())?,
                int(m).ok_or_else(bad)?,
                int(d).ok_or_else(bad)?,
            )
            .ok_or_else(bad)?;
            Some((start, start.succ_opt()))
        }
        _ => None,
    };
    if let Some((start, next)) = period {
        let day = if end { next.ok_or_else(bad)? } else { start };
        return midnight(day).ok_or_else(bad);
    }
    if let Ok(t) = chrono::DateTime::parse_from_rfc3339(text) {
        return Ok(t.timestamp());
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(t) = NaiveDateTime::parse_from_str(text, format) {
            return Ok(t.and_utc().timestamp());
        }
    }
    Err(bad())
}

/// The `from`/`to` arguments as a half-open range in seconds.
fn parse_range(args: &Value) -> Result<(Option<i64>, Option<i64>), String> {
    let from = arg_str(args, "from").map(|s| parse_date(s, false)).transpose()?;
    let to = arg_str(args, "to").map(|s| parse_date(s, true)).transpose()?;
    check_range(from, to)?;
    Ok((from, to))
}

fn check_range(from: Option<i64>, to: Option<i64>) -> Result<(), String> {
    if let Some(from) = from
        && from >= now()
    {
        return Err("The start date is in the future.".into());
    }
    if let (Some(from), Some(to)) = (from, to)
        && to <= from
    {
        return Err("The end date must be after the start date.".into());
    }
    Ok(())
}

/// Refuses a range whose CSV would pass the export cap, before anything is planned.
fn check_export_cap(timeframe: &str, from: i64, to: i64) -> Result<(), String> {
    // Markets that close at weekends have about 5/7 of the calendar's candles; a 24/7 market
    // that still slips past comes back marked `truncated`.
    let candles = (to - from).max(0) / timeframe_seconds(timeframe) * 5 / 7;
    if candles > EXPORT_CAP {
        return Err(format!(
            "{} to {} is about {} {timeframe} candles, more than the {} one file can hold. Use a larger timeframe or a shorter range.",
            fmt_time(from),
            fmt_time(to),
            thousands(candles),
            thousands(EXPORT_CAP)
        ));
    }
    Ok(())
}

fn num(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or(0.0)
}

/// `[from, to)` second ranges, sent either as `[[f, t], ...]` or `[{from, to}, ...]`.
fn ranges(v: &Value) -> Vec<(i64, i64)> {
    v.as_array()
        .map(|list| list.iter().filter_map(pair).collect())
        .unwrap_or_default()
}

fn pair(v: &Value) -> Option<(i64, i64)> {
    let (f, t) = match v {
        Value::Array(a) if a.len() == 2 => (&a[0], &a[1]),
        Value::Object(o) => (o.get("from")?, o.get("to")?),
        _ => return None,
    };
    let (f, t) = (f.as_f64()? as i64, t.as_f64()? as i64);
    (t > f).then_some((f, t))
}

/// The span from the first range's start to the last one's end.
fn extent(ranges: &[(i64, i64)]) -> Option<(i64, i64)> {
    let from = ranges.iter().map(|r| r.0).min()?;
    let to = ranges.iter().map(|r| r.1).max()?;
    Some((from, to))
}

fn iso(v: &Value) -> String {
    match v {
        Value::Number(n) => n
            .as_f64()
            .and_then(|s| chrono::DateTime::from_timestamp(s as i64, 0))
            .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_default(),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

fn iso_or_text(v: &Value) -> String {
    match v {
        Value::Number(_) => iso(v),
        Value::String(s) => s.clone(),
        _ => String::new(),
    }
}

/// A date, with the time only when it is not midnight UTC.
fn fmt_time(t: i64) -> String {
    match chrono::DateTime::from_timestamp(t, 0) {
        Some(d) if t % DAY == 0 => d.format("%Y-%m-%d").to_string(),
        Some(d) => d.format("%Y-%m-%d %H:%M").to_string(),
        None => t.to_string(),
    }
}

fn compact_day(t: i64) -> String {
    chrono::DateTime::from_timestamp(t, 0)
        .map(|d| d.format("%Y%m%d").to_string())
        .unwrap_or_default()
}

fn fmt_range(from: i64, to: i64) -> String {
    format!("{} → {}", fmt_time(from), fmt_time(to))
}

/// At most `max` ranges, keeping the oldest and newest and counting what is left out.
fn fmt_ranges(ranges: &[(i64, i64)], max: usize) -> Vec<String> {
    let all: Vec<String> = ranges.iter().map(|&(f, t)| fmt_range(f, t)).collect();
    if all.len() <= max {
        return all;
    }
    let head = max / 2;
    let tail = max - head - 1;
    let mut out = all[..head].to_vec();
    out.push(format!("… {} more …", all.len() - head - tail));
    out.extend_from_slice(&all[all.len() - tail..]);
    out
}

fn fmt_bytes(bytes: f64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes.max(0.0);
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let s = format!("{v:.8}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    #[test]
    fn timeframes_are_normalized() {
        assert_eq!(timeframe(&json!({"timeframe": "H1"})).unwrap(), "1h");
        assert_eq!(timeframe(&json!({"timeframe": "daily"})).unwrap(), "1d");
        assert_eq!(timeframe(&json!({"timeframe": "1M"})).unwrap(), "1M");
        assert_eq!(timeframe(&json!({"timeframe": "1m"})).unwrap(), "1m");
        assert!(timeframe(&json!({"timeframe": "7x"})).is_err());
    }

    #[test]
    fn numbers_are_printed_compactly() {
        assert_eq!(fmt_num(1.17340000), "1.1734");
        assert_eq!(fmt_num(100.0), "100");
        assert_eq!(fmt_num(f64::NAN), "");
        assert_eq!(thousands(2_000_000), "2,000,000");
        assert_eq!(thousands(999), "999");
        assert_eq!(fmt_bytes(512.0), "512 B");
        assert_eq!(fmt_bytes(75.0 * 1024.0 * 1024.0), "75.0 MB");
    }

    #[test]
    fn dates_parse_as_periods() {
        assert_eq!(parse_date("2024-03-05", false).unwrap(), day(2024, 3, 5));
        assert_eq!(parse_date("2024-03-05", true).unwrap(), day(2024, 3, 6));
        assert_eq!(parse_date("2024-3-5", false).unwrap(), day(2024, 3, 5));
        assert_eq!(parse_date("2024-12", true).unwrap(), day(2025, 1, 1));
        assert_eq!(parse_date("2024-02", false).unwrap(), day(2024, 2, 1));
        assert_eq!(parse_date("2019", false).unwrap(), day(2019, 1, 1));
        assert_eq!(parse_date("2019", true).unwrap(), day(2020, 1, 1));
        assert_eq!(
            parse_date("2024-03-05T12:00:00Z", true).unwrap(),
            day(2024, 3, 5) + 12 * 3600
        );
        assert_eq!(
            parse_date("2024-03-05 06:30", false).unwrap(),
            day(2024, 3, 5) + 6 * 3600 + 1800
        );
        assert_eq!(parse_date("1700000000", false).unwrap(), 1_700_000_000);
        assert_eq!(parse_date("1700000000000", false).unwrap(), 1_700_000_000);
        assert!(parse_date("yesterday", false).is_err());
        assert!(parse_date("2024-13-01", false).is_err());
    }

    #[test]
    fn compact_dates_are_days_not_unix_times() {
        assert_eq!(parse_date("20240101", false).unwrap(), day(2024, 1, 1));
        assert_eq!(parse_date("20240101", true).unwrap(), day(2024, 1, 2));
        assert_eq!(parse_date("20240229", true).unwrap(), day(2024, 3, 1));
        assert!(parse_date("20241301", false).is_err(), "no 13th month");
        assert!(parse_date("01312024", false).is_err(), "not YYYYMMDD");
        // Six digits could be YYYYMM or YYMMDD: refusing beats guessing.
        assert!(parse_date("202401", false).is_err());
        assert!(parse_date("12345", false).is_err());
        assert!(parse_date("1234567", false).is_err());
        assert_eq!(
            parse_date("100000000", false).unwrap(),
            100_000_000,
            "nine digits are Unix seconds"
        );
    }

    #[test]
    fn ranges_are_checked() {
        let (from, to) = parse_range(&json!({"from": "2020-01-01", "to": "2020-12-31"})).unwrap();
        assert_eq!((from, to), (Some(day(2020, 1, 1)), Some(day(2021, 1, 1))));
        assert_eq!(parse_range(&json!({"from": " "})).unwrap(), (None, None));
        assert!(parse_range(&json!({"from": "2021-01-01", "to": "2020-01-01"})).is_err());
        assert!(parse_range(&json!({"from": "2999-01-01"})).is_err());
    }

    #[test]
    fn counts_are_read_from_numbers_or_text() {
        assert_eq!(arg_u64(&json!({"bars": 500}), "bars"), Some(500));
        assert_eq!(arg_u64(&json!({"bars": "250"}), "bars"), Some(250));
        assert_eq!(arg_u64(&json!({"bars": 120.0}), "bars"), Some(120));
        assert_eq!(arg_u64(&json!({"bars": "many"}), "bars"), None);
        assert_eq!(arg_u64(&json!({}), "bars"), None);
    }

    #[test]
    fn long_downloads_ask_and_approved_jobs_do_not() {
        let plan = |seconds: f64| json!({"complete": false, "seconds": seconds, "requests": 100, "job": null});
        assert_eq!(next_step(&plan(61.0), 60), Next::Ask);
        assert_eq!(next_step(&plan(60.0), 60), Next::Start, "the limit itself does not ask");
        assert_eq!(next_step(&plan(5.0), 60), Next::Start);
        assert_eq!(next_step(&plan(5.0), 0), Next::Ask);
        assert_eq!(next_step(&json!({"complete": true, "seconds": 900}), 60), Next::Covered);
        let running = json!({"complete": false, "seconds": 9000, "job": {"id": "j1", "status": "running"}});
        assert_eq!(
            next_step(&running, 60),
            Next::Reuse {
                job_id: "j1".into(),
                resume: false
            }
        );
        for status in ["paused", "error"] {
            let stopped = json!({"complete": false, "seconds": 9000, "job": {"id": "j2", "status": status}});
            assert_eq!(
                next_step(&stopped, 60),
                Next::Reuse {
                    job_id: "j2".into(),
                    resume: true
                },
                "{status}"
            );
        }
    }

    #[test]
    fn the_smaller_download_is_offered_only_when_it_saves_work() {
        let full = json!({"requests": 8400, "complete": false});
        assert!(offer_minimal(&full, &json!({"requests": 280, "complete": false})));
        assert!(!offer_minimal(&full, &json!({"requests": 8400, "complete": false})));
        assert!(
            !offer_minimal(&full, &json!({"requests": 8350, "complete": false})),
            "a sliver is not worth a button"
        );
        assert!(!offer_minimal(&full, &json!({"requests": 0, "complete": true})));
        // Everything this timeframe needs is already queued by another job: still the cheaper
        // choice, and picking it joins that work instead of fetching 1-minute data.
        assert!(offer_minimal(
            &full,
            &json!({"requests": 0, "queuedAhead": 201, "complete": false})
        ));
        assert!(!offer_minimal(
            &json!({"requests": 100, "queuedAhead": 0, "complete": false}),
            &json!({"requests": 0, "queuedAhead": 95, "complete": false})
        ));
    }

    #[test]
    fn an_only_this_timeframe_job_is_reused_instead_of_asking_again() {
        // "Only 1h" started the h1-only job j1. The next ask for the same range plans every tier:
        // no job contains that, so it would ask, while j1 already has every h1 bucket queued.
        let full = json!({"complete": false, "seconds": 2111, "requests": 6100, "queuedAhead": 201, "job": null});
        assert_eq!(next_step(&full, 60), Next::Ask);
        let minimal = |status: &str| json!({"complete": false, "seconds": 0, "requests": 0, "queuedAhead": 201, "job": {"id": "j1", "status": status}});
        for (status, resume) in [
            ("running", false),
            ("waiting", false),
            ("paused", true),
            ("error", true),
        ] {
            assert_eq!(
                settled_by(&minimal(status)),
                Some(Next::Reuse {
                    job_id: "j1".into(),
                    resume
                }),
                "{status}"
            );
        }
        let stored = json!({"complete": true, "seconds": 0, "requests": 0, "queuedAhead": 0, "job": null});
        assert_eq!(settled_by(&stored), Some(Next::Covered));
        let fresh = json!({"complete": false, "seconds": 99_999, "requests": 129, "queuedAhead": 0, "job": null});
        assert_eq!(
            settled_by(&fresh),
            None,
            "a smaller download still to start is offered, not started"
        );
    }

    #[test]
    fn whole_job_waits_leave_the_job_order_alone() {
        let whole = wait_params("EURUSD", "1m", (0, 100), "j1", true);
        assert_eq!(whole["boost"], false);
        assert_eq!(whole["jobId"], "j1");
        let part = wait_params("EURUSD", "1h", (0, 100), "j1", false);
        assert_eq!(part["boost"], true, "a timeframe's data rides the waiting lane");
        assert_eq!((part["from"].clone(), part["to"].clone()), (json!(0), json!(100)));
        assert_eq!(part["timeoutMs"], 90_000);
    }

    fn job(id: &str) -> Downloaded {
        Downloaded {
            plan: plan(),
            job_id: Some(id.into()),
            ..Downloaded::default()
        }
    }

    #[test]
    fn a_paused_download_says_it_will_not_continue() {
        let paused = Downloaded {
            status: Some("paused".into()),
            ..job("j1")
        };
        let (facts, note) = paused.facts();
        let note = note.unwrap();
        assert!(note.contains("The user paused the download"), "{note}");
        assert!(note.contains("will not continue until they press Resume"), "{note}");
        assert!(!note.contains("continues in the background"), "{note}");
        assert_eq!(facts["status"], "paused");
    }

    #[test]
    fn stopped_downloads_report_the_jobs_own_counts() {
        let mut d = job("j1");
        d.note_job(&json!({"id": "j1", "status": "running", "done": 120, "total": 8400, "bytes": 1_048_576, "etaSeconds": 600}));
        d.note_job(&json!({"id": "j1", "status": "paused", "done": 130, "total": 8400, "bytes": 1_100_000}));
        let (ok, c) = download_result("EURUSD", &d);
        assert!(ok, "pausing is the user's choice, not a failure");
        assert_eq!(c["status"], "paused");
        assert_eq!(
            (c["filesDone"].clone(), c["filesTotal"].clone()),
            (json!(130), json!(8400))
        );
        let note = c["note"].as_str().unwrap();
        assert!(note.contains("paused the download at 130 of 8,400 files"), "{note}");
        assert!(note.contains("Resume") && note.contains("Data tab"), "{note}");
        let (facts, note) = d.facts();
        assert_eq!(facts["filesDone"], 130);
        assert!(note.unwrap().contains("at 130 of 8,400 files"));

        // A cancel removes the record: the last counts seen stand.
        d.note_job(&Value::Null);
        let (ok, c) = download_result("EURUSD", &d);
        assert!(ok);
        assert_eq!(c["status"], "cancelled");
        let note = c["note"].as_str().unwrap();
        assert!(
            note.contains("The user cancelled the download at 130 of 8,400 files"),
            "{note}"
        );
        assert!(note.contains("will not continue"), "{note}");
        let display = d.display();
        assert_eq!(display["status"], "cancelled");
        assert!(
            display.get("jobError").is_none(),
            "its card already says it was cancelled"
        );
        assert!(d.facts().1.unwrap().contains("cancelled"));

        let mut failed = job("j2");
        failed
            .note_job(&json!({"status": "error", "done": 5, "total": 83, "message": "Dukascopy refused the request"}));
        let (ok, c) = download_result("EURUSD", &failed);
        assert!(!ok);
        assert_eq!(c["error"], "Dukascopy refused the request");
        let note = c["note"].as_str().unwrap();
        assert!(
            note.contains("stopped with an error at 5 of 83 files (Dukascopy refused the request)"),
            "{note}"
        );
        assert!(note.contains("Try again"), "{note}");
        assert_eq!(failed.display()["jobError"], "Dukascopy refused the request");

        // "Try again" restarted it: a fresh status replaces the old failure.
        failed.note_job(&json!({"status": "running", "done": 6, "total": 83}));
        assert!(failed.job_error.is_none() && failed.stopped_note().is_none());
    }

    #[test]
    fn only_a_finished_job_is_reported_as_stored() {
        // The plan counted 8,400 files; the job's own counts are what the model is told.
        let mut running = job("j1");
        running.note_job(&json!({"status": "running", "done": 5, "total": 83, "etaSeconds": 30}));
        let (ok, c) = download_result("EURUSD", &running);
        assert!(ok);
        assert_eq!(c["status"], "running");
        assert_eq!(c["filesTotal"], 83);
        let note = c["note"].as_str().unwrap();
        assert!(
            note.contains("continues in the background") && !note.contains("finished"),
            "{note}"
        );

        let mut done = job("j1");
        done.note_job(&json!({"status": "done", "done": 83, "total": 83, "bytes": 690_000}));
        let (_, c) = download_result("EURUSD", &done);
        assert_eq!(
            (c["status"].clone(), c["filesDone"].clone()),
            (json!("done"), json!(83))
        );
        assert_eq!(c["size"], "673.8 KB");
        assert!(c["note"].as_str().unwrap().contains("finished"));

        // Without word from the service, nothing is claimed.
        let unknown = Downloaded {
            job_error: Some("the market service stopped".into()),
            ..job("j1")
        };
        let (ok, c) = download_result("EURUSD", &unknown);
        assert!(!ok);
        assert_eq!(c["status"], "unknown");
        assert!(
            c["note"]
                .as_str()
                .unwrap()
                .starts_with("Could not check on the download")
        );
    }

    #[test]
    fn the_smaller_download_says_what_it_covers() {
        let stored = Downloaded {
            plan: plan(),
            only: Some("1h"),
            covered: true,
            ..Downloaded::default()
        };
        let (ok, c) = download_result("EURUSD", &stored);
        assert!(ok);
        assert_eq!(c["status"], "done");
        let note = c["note"].as_str().unwrap();
        assert!(
            note.contains("Everything 1h needs") && !note.contains("any timeframe"),
            "{note}"
        );
        assert!(stored.display()["note"].as_str().unwrap().contains("1h"));

        let mut finished = Downloaded {
            only: Some("1d"),
            ..job("j1")
        };
        finished.note_job(&json!({"status": "done", "done": 24, "total": 24}));
        let (_, c) = download_result("EURUSD", &finished);
        let note = c["note"].as_str().unwrap();
        assert!(
            note.contains("everything 1d needs is stored") && !note.contains("any timeframe"),
            "{note}"
        );
    }

    #[test]
    fn a_paused_download_reaches_the_read_summary() {
        let dir = tempfile::tempdir().unwrap();
        let result = json!({
            "symbol": "EURUSD", "count": 24, "from": 0, "to": 82_800, "firstClose": 1.0, "lastClose": 1.0,
            "spans": [{"source": "dukascopy", "from": 0, "to": 82_800, "count": 24}], "missing": [[86_400, 172_800]],
        });
        let mut paused = job("j4");
        paused.note_job(&json!({"status": "paused", "done": 1, "total": 8767}));
        let out = summarize(&result, "EURUSD", "1h", &csv(dir.path()), Some(&paused), None).unwrap();
        let content: Value = serde_json::from_str(&out.content).unwrap();
        let note = content["note"].as_str().unwrap();
        assert!(note.contains("paused the download at 1 of 8,767 files"), "{note}");
        assert!(!note.contains("continues in the background"), "{note}");
        assert_eq!(
            out.display["download"]["status"], "paused",
            "the card keeps its Resume button"
        );
    }

    #[test]
    fn only_this_timeframe_means_its_coarsest_tier() {
        assert_eq!(minimal_tiers("dukascopy", "1h"), Some(vec!["h1"]));
        assert_eq!(minimal_tiers("dukascopy", "4h"), Some(vec!["h1"]));
        assert_eq!(minimal_tiers("dukascopy", "1d"), Some(vec!["d1"]));
        assert_eq!(minimal_tiers("dukascopy", "1M"), Some(vec!["d1"]));
        assert_eq!(
            minimal_tiers("dukascopy", "15m"),
            None,
            "minute data is the whole cost anyway"
        );
        assert_eq!(minimal_tiers("tradingview", "1h"), Some(vec!["1h"]));
        assert_eq!(
            minimal_tiers("tradingview", "30m"),
            None,
            "not a timeframe TradingView downloads"
        );
    }

    #[test]
    fn exports_past_the_cap_are_refused_up_front() {
        let from = day(2010, 1, 1);
        let to = day(2020, 1, 1);
        let refused = check_export_cap("1m", from, to).unwrap_err();
        assert!(refused.contains("2,000,000"), "{refused}");
        assert!(check_export_cap("1h", from, to).is_ok());
        assert!(check_export_cap("1m", day(2023, 1, 1), day(2024, 1, 1)).is_ok());
    }

    #[test]
    fn ranges_read_both_shapes() {
        let list = json!([[10, 20], {"from": 30, "to": 50}, [5, 5], "junk"]);
        assert_eq!(ranges(&list), vec![(10, 20), (30, 50)]);
        assert_eq!(extent(&ranges(&list)), Some((10, 50)));
        assert_eq!(extent(&[]), None);
    }

    #[test]
    fn gaps_are_what_nothing_covers() {
        assert_eq!(
            gaps((0, 100), &[(10, 20), (15, 30), (60, 70)]),
            vec![(0, 10), (30, 60), (70, 100)]
        );
        assert_eq!(gaps((0, 100), &[(0, 100)]), vec![]);
        assert_eq!(gaps((50, 100), &[(0, 60), (90, 200)]), vec![(60, 90)]);
    }

    #[test]
    fn long_range_lists_keep_both_ends() {
        let list: Vec<(i64, i64)> = (0..30).map(|i| (i * DAY * 2, i * DAY * 2 + DAY)).collect();
        let shown = fmt_ranges(&list, 12);
        assert_eq!(shown.len(), 12);
        assert!(shown[6].contains("19 more"), "{shown:?}");
        assert_eq!(shown[0], "1970-01-01 → 1970-01-02");
    }

    #[test]
    fn last_rows_become_csv_text() {
        assert_eq!(last_rows_text(&json!("time,open\n1,2")), "time,open\n1,2");
        let rows = json!([{"t": 0, "o": 1.5, "h": 2, "l": 1, "c": 1.75, "v": 10, "source": "dukascopy"}]);
        assert_eq!(
            last_rows_text(&rows),
            "time,open,high,low,close,volume,source\n1970-01-01T00:00:00Z,1.5,2,1,1.75,10,dukascopy"
        );
        let arrays = json!([[60, 1, 2, 0.5, 1.5, 0, "tradingview"]]);
        assert!(last_rows_text(&arrays).ends_with("1970-01-01T00:01:00Z,1,2,0.5,1.5,0,tradingview"));
        assert_eq!(last_rows_text(&json!(["time,open", "x,1"])), "time,open\nx,1");
    }

    #[test]
    fn sources_add_up_per_source() {
        let spans = [
            json!({"source": "dukascopy", "count": 100}),
            json!({"source": "tradingview", "count": 20}),
            json!({"source": "dukascopy", "count": 5}),
        ];
        assert_eq!(
            sources_by_source(&spans),
            vec![
                json!({"source": "dukascopy", "count": 105}),
                json!({"source": "tradingview", "count": 20})
            ]
        );
    }

    #[test]
    fn csv_names_follow_the_request() {
        let dir = tempfile::tempdir().unwrap();
        let f = csv_file(
            dir.path(),
            "FX:EURUSD",
            "1M",
            Some(day(2020, 1, 1)),
            Some(day(2021, 1, 1)),
            0,
        )
        .unwrap();
        assert_eq!(f.rel, "data/FX_EURUSD_1mo_20200101_20201231.csv");
        assert!(dir.path().join("data").is_dir());
        let latest = csv_file(dir.path(), "EURUSD", "1h", None, None, 300).unwrap();
        assert!(latest.rel.starts_with("data/EURUSD_1h_last300_"), "{}", latest.rel);
    }

    fn plan() -> Value {
        json!({"source": "dukascopy", "key": "eurusd", "name": "EUR/USD", "from": 0, "to": 100, "requests": 8400, "seconds": 2400, "complete": false, "job": null})
    }

    #[test]
    fn download_displays_have_the_card_shape() {
        let fresh = Downloaded {
            plan: plan(),
            job_id: Some("j1".into()),
            ..Downloaded::default()
        };
        assert_eq!(
            fresh.display(),
            json!({"kind": "download", "jobId": "j1", "plan": plan()})
        );

        let denied = Downloaded {
            plan: plan(),
            denied: true,
            ..Downloaded::default()
        };
        let d = denied.display();
        assert_eq!(
            (d["jobId"].clone(), d["status"].clone()),
            (Value::Null, json!("denied"))
        );
        assert_eq!(d["denied"], true);

        let failed = Downloaded {
            plan: plan(),
            job_id: Some("j1".into()),
            status: Some("error".into()),
            job_error: Some("Offline".into()),
            ..Downloaded::default()
        };
        let d = failed.display();
        assert_eq!((d["jobError"].clone(), d.get("error")), (json!("Offline"), None));

        let stored = Downloaded {
            plan: plan(),
            covered: true,
            ..Downloaded::default()
        };
        assert_eq!(stored.display()["status"], "done");
    }

    fn csv(dir: &Path) -> CsvFile {
        CsvFile {
            rel: "data/EURUSD_1h.csv".into(),
            path: dir.join("EURUSD_1h.csv"),
        }
    }

    #[test]
    fn a_read_becomes_a_summary_and_a_preview() {
        let dir = tempfile::tempdir().unwrap();
        let result = json!({
            "symbol": "EURUSD", "info": {"description": "Euro vs US Dollar"}, "timeframe": "1h",
            "count": 1200, "from": 0, "to": 3600, "firstClose": 1.0, "lastClose": 1.1, "high": 1.2, "low": 0.9,
            "spans": [{"source": "dukascopy", "from": 0, "to": 3600, "count": 1200}],
            "lastRows": "time,open,high,low,close,volume,source\n1970-01-01T01:00:00Z,1,1,1,1.1,0,dukascopy",
            "closes": [1.0, 1.1], "missing": [[7200, 10800]], "truncated": false,
        });
        let running = Downloaded {
            plan: plan(),
            job_id: Some("j9".into()),
            status: Some("running".into()),
            eta_seconds: Some(600.0),
            ..Downloaded::default()
        };
        let out = summarize(&result, "EURUSD", "1h", &csv(dir.path()), Some(&running), None).unwrap();
        assert!(out.ok);
        let content: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(content["candles"], 1200);
        assert_eq!(content["status"], "running");
        assert_eq!(content["jobId"], "j9");
        assert!(content["note"].as_str().unwrap().contains("about 10 minutes"));
        assert_eq!(content["missing"][0], "1970-01-01 02:00 → 1970-01-01 03:00");
        assert_eq!(out.display["kind"], "candles");
        assert_eq!(out.display["sources"], json!([{"source": "dukascopy", "count": 1200}]));
        assert_eq!(out.display["download"]["jobId"], "j9");
        assert_eq!(out.display["from"], "1970-01-01T00:00:00Z");
        assert!((out.display["changePercent"].as_f64().unwrap() - 10.0).abs() < 1e-9);

        let done = Downloaded {
            status: Some("done".into()),
            covered: true,
            ..running
        };
        let out = summarize(&result, "EURUSD", "1h", &csv(dir.path()), Some(&done), None).unwrap();
        assert!(
            out.display.get("download").is_none(),
            "a finished job needs no progress bar"
        );
        assert!(
            serde_json::from_str::<Value>(&out.content)
                .unwrap()
                .get("status")
                .is_none()
        );
    }

    #[test]
    fn an_empty_read_without_a_download_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let empty = json!({"symbol": "EURUSD", "count": 0, "missing": []});
        assert!(summarize(&empty, "EURUSD", "1h", &csv(dir.path()), None, None).is_err());
        let declined = Downloaded {
            plan: plan(),
            denied: true,
            ..Downloaded::default()
        };
        let out = summarize(&empty, "EURUSD", "1h", &csv(dir.path()), Some(&declined), None).unwrap();
        assert_eq!(out.display["kind"], "download");
        assert_eq!(out.display["status"], "denied");
        let content: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(content["download"], "declined");
    }

    #[test]
    fn a_declined_download_still_returns_what_is_stored() {
        let dir = tempfile::tempdir().unwrap();
        let result = json!({
            "symbol": "EURUSD", "count": 24, "from": 0, "to": 82_800, "firstClose": 1.0, "lastClose": 1.0,
            "spans": [{"source": "dukascopy", "from": 0, "to": 82_800, "count": 24}],
            "missing": [[86_400, 172_800]],
        });
        let declined = Downloaded {
            plan: plan(),
            denied: true,
            ..Downloaded::default()
        };
        let out = summarize(&result, "EURUSD", "1h", &csv(dir.path()), Some(&declined), None).unwrap();
        assert!(out.ok);
        assert_eq!(out.display["kind"], "candles");
        assert_eq!(out.display["denied"], true);
        assert!(out.display.get("download").is_none(), "no job, no progress bar");
        let content: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(content["download"], "declined");
        assert_eq!(content["missing"][0], "1970-01-02 → 1970-01-03");
        assert!(content["note"].as_str().unwrap().contains("Do not retry"));
    }

    #[test]
    fn a_failed_job_keeps_its_card_and_reports_job_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = json!({"symbol": "EURUSD", "count": 5, "from": 0, "to": 14_400, "spans": [], "missing": []});
        let failed = Downloaded {
            plan: plan(),
            job_id: Some("j3".into()),
            status: Some("error".into()),
            job_error: Some("Offline".into()),
            ..Downloaded::default()
        };
        let out = summarize(&result, "EURUSD", "1h", &csv(dir.path()), Some(&failed), None).unwrap();
        assert_eq!(out.display["download"]["jobId"], "j3");
        assert_eq!(out.display["download"]["jobError"], "Offline");
        assert!(out.display.get("error").is_none(), "an error would hide the preview");
        let content: Value = serde_json::from_str(&out.content).unwrap();
        assert_eq!(content["downloadError"], "Offline");
    }
}
