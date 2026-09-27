use std::time::Duration;

use serde_json::{Value, json};

use super::St;
use crate::error::{AppError, CmdResult};
use crate::market::{MarketStatus, auth};

fn rpc(e: crate::market::RpcError) -> AppError {
    AppError::msg(e.message)
}

#[tauri::command]
pub fn market_status(state: St<'_>) -> MarketStatus {
    state.market.status()
}

/// Opens the TradingView sign-in window. Progress arrives as `market://status` events.
#[tauri::command]
pub async fn market_login(state: St<'_>) -> CmdResult<()> {
    auth::begin(&state.market).await.map_err(AppError::msg)?;
    Ok(())
}

#[tauri::command]
pub async fn market_logout(state: St<'_>) -> CmdResult<MarketStatus> {
    state.market.logout().await;
    Ok(state.market.status())
}

#[tauri::command]
pub async fn market_search(state: St<'_>, query: String, kind: Option<String>) -> CmdResult<Value> {
    state
        .market
        .call("search", json!({"query": query, "type": kind}), Duration::from_secs(20))
        .await
        .map_err(rpc)
}

#[tauri::command]
pub async fn market_quote(state: St<'_>, symbols: Vec<String>) -> CmdResult<Value> {
    state
        .market
        .call("quote", json!({"symbols": symbols}), Duration::from_secs(30))
        .await
        .map_err(rpc)
}

/// Opens a live chart stream: returns the first bars, then `market://event` updates follow.
#[tauri::command]
pub async fn market_open_stream(
    state: St<'_>,
    symbol: String,
    timeframe: String,
    bars: Option<u32>,
) -> CmdResult<Value> {
    state
        .market
        .call(
            "stream.open",
            json!({"symbol": symbol, "timeframe": timeframe, "bars": bars.unwrap_or(500)}),
            Duration::from_secs(90),
        )
        .await
        .map_err(rpc)
}

#[tauri::command]
pub async fn market_close_stream(state: St<'_>, stream_id: String) -> CmdResult<()> {
    let _ = state
        .market
        .call("stream.close", json!({"id": stream_id}), Duration::from_secs(10))
        .await;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The market data store: reads, downloads that fill it, and what it holds. Times are seconds;
// results pass through as the service sends them (see `sidecars/market/src/protocol.ts`).

/// Whole seconds. Times arrive as plain numbers, and one computed as `Date.now() / 1000` has a
/// fraction an integer parameter would reject.
fn secs(t: f64) -> i64 {
    t.floor() as i64
}

/// `{symbol, ...}` plus the optional range fields that are set; unset ones are left out.
fn range_params(symbol: String, from: Option<f64>, to: Option<f64>, tiers: Option<Vec<String>>) -> Value {
    let mut params = json!({"symbol": symbol});
    if let Some(from) = from {
        params["from"] = secs(from).into();
    }
    if let Some(to) = to {
        params["to"] = secs(to).into();
    }
    if let Some(tiers) = tiers.filter(|t| !t.is_empty()) {
        params["tiers"] = json!(tiers);
    }
    params
}

async fn store_call(state: &St<'_>, method: &str, params: Value, secs: u64) -> CmdResult<Value> {
    state
        .market
        .call(method, params, Duration::from_secs(secs))
        .await
        .map_err(rpc)
}

/// The newest bars for the signed-out chart; fetches the few missing buckets first.
#[tauri::command]
pub async fn market_bars_latest(state: St<'_>, symbol: String, timeframe: String, count: u32) -> CmdResult<Value> {
    let params = json!({"symbol": symbol, "timeframe": timeframe, "count": count});
    store_call(&state, "bars.latest", params, 120).await
}

/// Older bars from the store only (chart paging); never touches the network.
#[tauri::command]
pub async fn market_bars_older(
    state: St<'_>,
    symbol: String,
    timeframe: String,
    before: f64,
    count: u32,
) -> CmdResult<Value> {
    let params = json!({"symbol": symbol, "timeframe": timeframe, "before": secs(before), "count": count});
    store_call(&state, "bars.older", params, 30).await
}

/// Queues the buckets from the last stored time to now; the chart refreshes on `store.updated`.
#[tauri::command]
pub async fn market_bars_freshen(state: St<'_>, symbol: String, timeframe: String) -> CmdResult<Value> {
    let params = json!({"symbol": symbol, "timeframe": timeframe});
    store_call(&state, "bars.freshen", params, 30).await
}

#[tauri::command]
pub async fn market_download_plan(
    state: St<'_>,
    symbol: String,
    from: Option<f64>,
    to: Option<f64>,
    tiers: Option<Vec<String>>,
) -> CmdResult<Value> {
    store_call(&state, "download.plan", range_params(symbol, from, to, tiers), 60).await
}

/// Starts (or joins) a download. Progress arrives as `market://event` events
/// (`download.progress` / `download.done` / `download.error`).
#[tauri::command]
pub async fn market_download_start(
    state: St<'_>,
    symbol: String,
    from: Option<f64>,
    to: Option<f64>,
    tiers: Option<Vec<String>>,
    origin: String,
) -> CmdResult<Value> {
    let mut params = range_params(symbol, from, to, tiers);
    params["origin"] = origin.into();
    store_call(&state, "download.start", params, 60).await
}

#[tauri::command]
pub async fn market_download_pause(state: St<'_>, job_id: String) -> CmdResult<()> {
    store_call(&state, "download.pause", json!({"jobId": job_id}), 15).await?;
    Ok(())
}

#[tauri::command]
pub async fn market_download_resume(state: St<'_>, job_id: String) -> CmdResult<()> {
    store_call(&state, "download.resume", json!({"jobId": job_id}), 15).await?;
    Ok(())
}

/// Removes the job record; the data it fetched stays.
#[tauri::command]
pub async fn market_download_cancel(state: St<'_>, job_id: String) -> CmdResult<()> {
    store_call(&state, "download.cancel", json!({"jobId": job_id}), 15).await?;
    Ok(())
}

/// The job, or null once it was cancelled or removed.
#[tauri::command]
pub async fn market_download_status(state: St<'_>, job_id: String) -> CmdResult<Value> {
    store_call(&state, "download.status", json!({"jobId": job_id}), 15).await
}

#[tauri::command]
pub async fn market_download_list(state: St<'_>, symbol: Option<String>) -> CmdResult<Value> {
    let params = match symbol {
        Some(symbol) => json!({"symbol": symbol}),
        None => json!({}),
    };
    store_call(&state, "download.list", params, 15).await
}

#[tauri::command]
pub async fn market_cache_summary(state: St<'_>, symbol: Option<String>) -> CmdResult<Value> {
    let params = match symbol {
        Some(symbol) => json!({"symbol": symbol}),
        None => json!({}),
    };
    store_call(&state, "cache.summary", params, 60).await
}

/// Deletes a market's stored data; the service refuses while a job for it runs.
#[tauri::command]
pub async fn market_cache_delete(state: St<'_>, market: String) -> CmdResult<Value> {
    store_call(&state, "cache.delete", json!({"market": market}), 120).await
}

/// Forgets learned starts and "unavailable" answers, so older data is looked for again.
#[tauri::command]
pub async fn market_cache_recheck(state: St<'_>, market: String) -> CmdResult<()> {
    store_call(&state, "cache.recheck", json!({"market": market}), 15).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_range_fields_are_left_out() {
        assert_eq!(
            range_params("EURUSD".into(), None, None, None),
            json!({"symbol": "EURUSD"})
        );
        assert_eq!(
            range_params("EURUSD".into(), None, None, Some(vec![])),
            json!({"symbol": "EURUSD"})
        );
        assert_eq!(
            range_params(
                "EURUSD".into(),
                Some(10.0),
                Some(20.0),
                Some(vec!["h1".into(), "m1".into()])
            ),
            json!({"symbol": "EURUSD", "from": 10, "to": 20, "tiers": ["h1", "m1"]})
        );
    }

    #[test]
    fn fractional_seconds_become_whole() {
        let params = range_params("EURUSD".into(), Some(1_700_000_000.75), Some(1_800_000_000.0), None);
        assert_eq!(
            params,
            json!({"symbol": "EURUSD", "from": 1_700_000_000, "to": 1_800_000_000})
        );
        assert_eq!(secs(-0.5), -1);
    }
}
