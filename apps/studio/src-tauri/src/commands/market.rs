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

/// Older bars for a stream (scrolling back in time).
#[tauri::command]
pub async fn market_stream_more(
    state: St<'_>,
    stream_id: String,
    count: Option<u32>,
    before: Option<i64>,
) -> CmdResult<Value> {
    state
        .market
        .call(
            "stream.more",
            json!({"id": stream_id, "count": count.unwrap_or(500), "before": before}),
            Duration::from_secs(180),
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

/// Dukascopy history without a TradingView session (the chart's fallback when signed out).
#[tauri::command]
pub async fn market_history(
    state: St<'_>,
    instrument: String,
    timeframe: String,
    from: String,
    to: Option<String>,
) -> CmdResult<Value> {
    state
        .market
        .call(
            "history",
            json!({"instrument": instrument, "timeframe": timeframe, "from": from, "to": to}),
            Duration::from_secs(240),
        )
        .await
        .map_err(rpc)
}

#[tauri::command]
pub async fn market_resolve_dukascopy(state: St<'_>, symbol: String) -> CmdResult<Value> {
    state
        .market
        .call("dukascopy.resolve", json!({"symbol": symbol}), Duration::from_secs(15))
        .await
        .map_err(rpc)
}
