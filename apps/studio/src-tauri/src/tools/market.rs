//! Market data tools. Live data needs a TradingView sign-in: when the person is not signed in,
//! the sign-in window opens and the tool waits for it; if it is not completed in time, the tool
//! tells the model to ask the person to sign in.

use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value, json};

use super::{ToolContext, ToolOutput, arg_str, require_str};
use crate::market::auth::{self, LoginState};

const LOGIN_WAIT: Duration = Duration::from_secs(120);
const TIMEFRAMES: &[&str] = &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w", "1M"];

pub fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "Company, ticker or pair, e.g. \"apple\", \"EURUSD\", \"bitcoin\", \"S&P 500\""},
            "type": {"type": "string", "enum": ["stock", "forex", "crypto", "index", "futures", "cfd", "fund"], "description": "Optional asset class filter"}
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
            "instrument": {"type": "string", "description": "Dukascopy instrument, e.g. EURUSD, XAUUSD, BTCUSD, US500, AAPL"},
            "timeframe": {"type": "string", "enum": ["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1M"]},
            "from": {"type": "string", "description": "Start date, YYYY-MM-DD"},
            "to": {"type": "string", "description": "End date, YYYY-MM-DD (default: today)"}
        },
        "required": ["instrument", "timeframe", "from"]
    })
}

/// Makes sure a TradingView session exists, opening the sign-in window when it does not.
async fn ensure_login(ctx: &ToolContext) -> Result<(), String> {
    let market = &ctx.state.market;
    if market.logged_in() {
        return Ok(());
    }
    let rx = auth::begin(market).await?;
    ctx.state.notice(&ctx.chat_id, "tradingviewLogin", "Sign in to TradingView to continue");
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
            ctx.state.market.call(method, params, timeout).await.map_err(|e| e.message)
        }
        other => other.map_err(|e| e.message),
    }
}

pub async fn search(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let query = require_str(args, "query")?;
    let params = json!({"query": query, "type": arg_str(args, "type")});
    let result = ctx
        .state
        .market
        .call("search", params, Duration::from_secs(20))
        .await
        .map_err(|e| e.message)?;
    let list: Vec<Value> = result.as_array().cloned().unwrap_or_default().into_iter().take(12).collect();
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
    Ok(ToolOutput::ok(
        json!({"results": compact}).to_string(),
        json!({"kind": "search", "query": query, "results": list}),
    ))
}

pub async fn quote(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let symbols: Vec<String> = match &args["symbols"] {
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Value::String(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
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
    let bars = args["bars"].as_u64().unwrap_or(300).clamp(10, 5000);
    let params = json!({
        "symbol": symbol,
        "timeframe": timeframe,
        "bars": bars,
        "from": arg_str(args, "from"),
        "to": arg_str(args, "to"),
    });
    let result = call_live(ctx, "candles", params, Duration::from_secs(240)).await?;
    finish(ctx, result, symbol, timeframe)
}

pub async fn history(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let instrument = require_str(args, "instrument")?;
    let timeframe = timeframe(args)?;
    let from = require_str(args, "from")?;
    let params = json!({
        "instrument": instrument,
        "timeframe": timeframe,
        "from": from,
        "to": arg_str(args, "to"),
    });
    let result = ctx
        .state
        .market
        .call("history", params, Duration::from_secs(300))
        .await
        .map_err(|e| e.message)?;
    finish(ctx, result, instrument, timeframe)
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
            _ => return Err(format!("Unknown timeframe {raw}. Use one of {}.", TIMEFRAMES.join(", "))),
        },
    };
    Ok(TIMEFRAMES.iter().find(|t| **t == normalized).copied().unwrap_or("1d"))
}

/// Saves candles as CSV and builds the model's summary and the UI's preview.
fn finish(ctx: &ToolContext, result: Value, requested: &str, timeframe: &str) -> Result<ToolOutput, String> {
    let bars = result["bars"].as_array().cloned().unwrap_or_default();
    let symbol = result["symbol"].as_str().unwrap_or(requested).to_string();
    let description = result["info"]["description"].as_str().unwrap_or_default().to_string();
    if bars.is_empty() {
        return Err(format!("No candles were returned for {symbol} ({timeframe}). Check the symbol with market_search or pick another range."));
    }
    let get = |b: &Value, k: &str| b[k].as_f64().unwrap_or(f64::NAN);
    let time = |b: &Value| {
        chrono::DateTime::from_timestamp(b["t"].as_i64().unwrap_or(0), 0)
            .map(|d| d.format("%Y-%m-%dT%H:%M:%SZ").to_string())
            .unwrap_or_default()
    };
    let first = &bars[0];
    let last = &bars[bars.len() - 1];

    let mut csv = String::from("time,open,high,low,close,volume\n");
    for b in &bars {
        let _ = writeln!(
            csv,
            "{},{},{},{},{},{}",
            time(b),
            fmt_num(get(b, "o")),
            fmt_num(get(b, "h")),
            fmt_num(get(b, "l")),
            fmt_num(get(b, "c")),
            fmt_num(get(b, "v"))
        );
    }
    let safe_symbol: String = symbol
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let day = |s: &str| s.get(..10).unwrap_or(s).replace('-', "");
    let rel = format!(
        "data/{}_{}_{}_{}.csv",
        safe_symbol,
        timeframe,
        day(&time(first)),
        day(&time(last))
    );
    let path = ctx.workspace.join(&rel);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, &csv).map_err(|e| format!("could not save the data: {e}"))?;

    let closes: Vec<f64> = bars.iter().map(|b| get(b, "c")).collect();
    let high = bars.iter().map(|b| get(b, "h")).fold(f64::MIN, f64::max);
    let low = bars.iter().map(|b| get(b, "l")).fold(f64::MAX, f64::min);
    let first_close = closes[0];
    let last_close = closes[closes.len() - 1];
    let change_pct = if first_close != 0.0 {
        (last_close - first_close) / first_close * 100.0
    } else {
        0.0
    };
    // The header and the last eight rows.
    let lines: Vec<&str> = csv.lines().collect();
    let tail = std::iter::once(lines[0])
        .chain(lines[1..].iter().copied().skip(lines.len().saturating_sub(9)))
        .collect::<Vec<_>>()
        .join("\n");
    let sources = result["sources"].clone();
    let summary = json!({
        "symbol": symbol,
        "description": description,
        "timeframe": timeframe,
        "candles": bars.len(),
        "from": time(first),
        "to": time(last),
        "file": rel,
        "firstClose": first_close,
        "lastClose": last_close,
        "changePercent": (change_pct * 100.0).round() / 100.0,
        "high": high,
        "low": low,
        "sources": sources,
        "lastRows": tail,
        "note": "The full data is in the CSV file; load it with pandas in run_python to analyse it.",
    });
    // Up to 160 closes for the UI's sparkline.
    let step = (closes.len() / 160).max(1);
    let spark: Vec<f64> = closes.iter().step_by(step).copied().collect();
    let display = json!({
        "kind": "candles",
        "symbol": symbol,
        "description": description,
        "timeframe": timeframe,
        "count": bars.len(),
        "from": time(first),
        "to": time(last),
        "file": rel,
        "path": path.to_string_lossy(),
        "lastClose": last_close,
        "changePercent": change_pct,
        "high": high,
        "low": low,
        "closes": spark,
        "sources": sources,
    });
    Ok(ToolOutput::ok(summary.to_string(), display))
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
    }
}
