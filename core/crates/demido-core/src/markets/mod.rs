//! Markets: prices from public feeds, indicators worked out here, and a Pine-compatible engine of Demido's own
//! (DECISIONS M2, M3, M7, M9, M15 to M19; research note 33). Nothing here places an order, and nothing reads
//! TradingView.
//!
//! Crypto comes from Kraken's public candles, and from Coinbase's when Kraken does not answer; currencies from the
//! European Central Bank's daily reference rates, through Frankfurter. No key, and only what the person asks
//! for, kept for a while (the caching rule, P): a candle list for a quarter of its interval, and at least 15 s.

pub mod pine;
pub mod ta;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{Error, Result};

/// The built-in indicators: each a script of the engine's (M2, M17): an id, its name, its source.
pub const BUILT_IN: &[(&str, &str, &str)] = &[
    ("ema20", "EMA 20", "//@version=6\nindicator(\"EMA 20\", overlay=true)\nplot(ta.ema(close, 20), \"EMA 20\", color=color.orange)"),
    ("sma50", "SMA 50", "//@version=6\nindicator(\"SMA 50\", overlay=true)\nplot(ta.sma(close, 50), \"SMA 50\", color=color.blue)"),
    (
        "bb20",
        "Bollinger Bands 20, 2",
        "//@version=6\nindicator(\"Bollinger Bands\", overlay=true)\nbasis = ta.sma(close, 20)\ndev = 2 * ta.stdev(close, 20)\nplot(basis, \"Basis\", color=color.gray)\nplot(basis + dev, \"Upper\", color=color.teal)\nplot(basis - dev, \"Lower\", color=color.teal)",
    ),
    ("rsi14", "RSI 14", "//@version=6\nindicator(\"RSI 14\")\nplot(ta.rsi(close, 14), \"RSI\", color=color.purple)\nhline(70, \"Overbought\")\nhline(30, \"Oversold\")"),
    (
        "macd",
        "MACD 12, 26, 9",
        "//@version=6\nindicator(\"MACD\")\nmacd = ta.ema(close, 12) - ta.ema(close, 26)\nsignal = ta.ema(macd, 9)\nplot(macd, \"MACD\", color=color.blue)\nplot(signal, \"Signal\", color=color.orange)\nplot(macd - signal, \"Histogram\", color=color.gray)\nhline(0)",
    ),
];

/// One candle: when it opened (milliseconds since 1970), its prices, and how much was traded.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Candle {
    pub t: i64,
    pub o: f64,
    pub h: f64,
    pub l: f64,
    pub c: f64,
    pub v: f64,
}

/// A symbol Demido can show, and where its prices come from.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Symbol {
    /// `BTC/EUR`.
    pub id: String,
    pub name: String,
    /// `crypto` or `currency`.
    pub kind: String,
    /// Who gives its prices: `Kraken (Coinbase if not)`, `the European Central Bank`.
    pub from: String,
    /// The timeframes it has, in minutes.
    pub intervals: Vec<u32>,
}

/// Crypto pairs: Demido's name, Kraken's, Coinbase's.
const CRYPTO: &[(&str, &str, &str, &str)] = &[
    ("BTC/EUR", "Bitcoin in euros", "XBTEUR", "BTC-EUR"),
    ("BTC/USD", "Bitcoin in dollars", "XBTUSD", "BTC-USD"),
    ("ETH/EUR", "Ether in euros", "ETHEUR", "ETH-EUR"),
    ("ETH/USD", "Ether in dollars", "ETHUSD", "ETH-USD"),
    ("SOL/EUR", "Solana in euros", "SOLEUR", "SOL-EUR"),
    ("SOL/USD", "Solana in dollars", "SOLUSD", "SOL-USD"),
    ("XRP/EUR", "XRP in euros", "XRPEUR", "XRP-EUR"),
    ("ADA/EUR", "Cardano in euros", "ADAEUR", "ADA-EUR"),
    ("DOGE/USD", "Dogecoin in dollars", "XDGUSD", "DOGE-USD"),
];
/// Currencies against the euro and the dollar, from the ECB's daily rates.
const CURRENCIES: &[(&str, &str)] =
    &[("EUR/USD", "Euro in dollars"), ("EUR/GBP", "Euro in pounds"), ("EUR/CHF", "Euro in Swiss francs"), ("EUR/JPY", "Euro in yen"), ("GBP/USD", "Pound in dollars"), ("USD/JPY", "Dollar in yen")];
/// Kraken's intervals, in minutes; Coinbase has 1, 5, 15, 60, 360 and 1440 of them.
const KRAKEN_INTERVALS: &[u32] = &[1, 5, 15, 30, 60, 240, 1440, 10080];

pub fn symbols() -> Vec<Symbol> {
    let mut out: Vec<Symbol> = CRYPTO
        .iter()
        .map(|(id, name, _, _)| Symbol {
            id: (*id).into(),
            name: (*name).into(),
            kind: "crypto".into(),
            from: "Kraken (Coinbase when Kraken does not answer)".into(),
            intervals: KRAKEN_INTERVALS.to_vec(),
        })
        .collect();
    out.extend(CURRENCIES.iter().map(|(id, name)| Symbol {
        id: (*id).into(),
        name: (*name).into(),
        kind: "currency".into(),
        from: "the European Central Bank's daily reference rates, through Frankfurter".into(),
        intervals: vec![1440],
    }));
    out
}

/// Where the feeds are: a test points them at a stand-in.
#[derive(Clone, Debug)]
pub struct Feeds {
    pub kraken: String,
    pub coinbase: String,
    pub frankfurter: String,
}

impl Default for Feeds {
    fn default() -> Self {
        Self { kraken: "https://api.kraken.com".into(), coinbase: "https://api.exchange.coinbase.com".into(), frankfurter: "https://api.frankfurter.dev".into() }
    }
}

/// Candles as given, with where they came from and how old the answer is.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Candles {
    pub symbol: String,
    pub interval: u32,
    pub from: String,
    pub candles: Vec<Candle>,
    /// Milliseconds since the feed was asked.
    pub age_ms: u64,
}

/// Candles kept: when they were asked for, from whom, and what came.
type Kept = (Instant, String, Vec<Candle>);

pub struct Markets {
    http: reqwest::Client,
    feeds: Feeds,
    kept: Mutex<HashMap<(String, u32), Kept>>,
    pub(crate) db: std::sync::Arc<crate::db::Db>,
}

fn net(who: &str) -> impl Fn(reqwest::Error) -> Error + '_ {
    move |e| Error::Net(format!("{who} could not be reached ({}).", e.without_url()))
}

impl Markets {
    pub fn new(feeds: Feeds, db: std::sync::Arc<crate::db::Db>) -> Self {
        let http = reqwest::Client::builder().user_agent(crate::web::name()).connect_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(20)).build().expect("a client can be made");
        Self { http, feeds, kept: Mutex::default(), db }
    }

    /// Runs a script on a symbol's candles.
    pub async fn run(&self, source: &str, symbol: &str, interval: u32) -> Result<(Candles, std::result::Result<pine::Ran, pine::PineError>)> {
        let candles = self.candles(symbol, interval, false).await?;
        let ran = pine::run(source, &candles.candles);
        Ok((candles, ran))
    }

    /// Has a session's model write a script for what the person asked, runs it, and hands any error back to be
    /// mended, three tries at most (M3). Every try is in the session, for the person to read.
    pub async fn write(&self, chat: &std::sync::Arc<crate::chat::Chat>, ask: &Write) -> Result<Written> {
        let session = match &ask.session {
            Some(id) => id.clone(),
            None => chat.start(Some("Pine scripts"), ask.model.as_deref()).await?.id,
        };
        let candles = self.candles(&ask.symbol, ask.interval, false).await?;
        let mut source = ask.source.clone().unwrap_or_default();
        let mut error: Option<String> = None;
        for tries in 1..=3 {
            let said = asked_to_write(&ask.request, Some(&source), error.as_deref());
            chat.say(&session, &said)?;
            // The answer, once it is written.
            let mut waited = 0;
            while chat.answering(&session) && waited < 20 * 60 * 10 {
                tokio::time::sleep(Duration::from_millis(100)).await;
                waited += 1;
            }
            let answer = chat.sessions().messages(&session)?.into_iter().rev().find(|m| m.role == "assistant").map(|m| m.content).unwrap_or_default();
            source = script_in(&answer);
            match pine::run(&source, &candles.candles) {
                Ok(ran) => return Ok(Written { session, source, tries, ran: Some(ran), error: None }),
                Err(e) => error = Some(e.to_string()),
            }
        }
        Ok(Written { session, source, tries: 3, ran: None, error })
    }

    /// The candles of a symbol at an interval: kept from before while fresh, otherwise asked of its feed.
    pub async fn candles(&self, symbol: &str, interval: u32, fresh: bool) -> Result<Candles> {
        let key = (symbol.to_string(), interval);
        let fresh_for = Duration::from_secs((interval as u64 * 60 / 4).max(15));
        if !fresh
            && let Some((at, from, candles)) = self.kept.lock().unwrap_or_else(|e| e.into_inner()).get(&key).cloned()
            && at.elapsed() < fresh_for
        {
            return Ok(Candles { symbol: symbol.into(), interval, from, candles, age_ms: at.elapsed().as_millis() as u64 });
        }
        let (from, candles) = if let Some((_, _, kraken, coinbase)) = CRYPTO.iter().find(|c| c.0 == symbol) {
            if !KRAKEN_INTERVALS.contains(&interval) {
                return Err(Error::Invalid(format!("{symbol} has candles of 1, 5, 15, 30, 60, 240, 1440 and 10080 minutes, not {interval}.")));
            }
            match self.kraken(kraken, interval).await {
                Ok(c) => ("Kraken".to_string(), c),
                Err(first) => match self.coinbase(coinbase, interval).await {
                    Ok(c) => ("Coinbase".to_string(), c),
                    Err(_) => return Err(first),
                },
            }
        } else if CURRENCIES.iter().any(|c| c.0 == symbol) {
            if interval != 1440 {
                return Err(Error::Invalid(format!("{symbol} comes from the ECB's daily rates: only daily candles.")));
            }
            ("the European Central Bank".to_string(), self.ecb(symbol).await?)
        } else {
            return Err(Error::NotFound(format!("Demido has no prices for \"{symbol}\".")));
        };
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), from.clone(), candles.clone()));
        Ok(Candles { symbol: symbol.into(), interval, from, candles, age_ms: 0 })
    }

    async fn kraken(&self, pair: &str, interval: u32) -> Result<Vec<Candle>> {
        let mut url = url::Url::parse(&format!("{}/0/public/OHLC", self.feeds.kraken)).map_err(|_| Error::Other("Kraken's address is wrong".into()))?;
        url.query_pairs_mut().append_pair("pair", pair).append_pair("interval", &interval.to_string());
        let got: Value = self.http.get(url).send().await.map_err(net("Kraken"))?.json().await.map_err(net("Kraken"))?;
        if let Some(e) = got["error"].as_array().and_then(|e| e.first()) {
            return Err(Error::Net(format!("Kraken said: {}", e.as_str().unwrap_or("an error"))));
        }
        let rows = got["result"].as_object().and_then(|r| r.iter().find(|(k, _)| *k != "last").map(|(_, v)| v.clone())).unwrap_or(Value::Null);
        let num = |v: &Value| v.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| v.as_f64()).unwrap_or(f64::NAN);
        Ok(rows
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let r = r.as_array()?;
                Some(Candle { t: r.first()?.as_i64()? * 1000, o: num(&r[1]), h: num(&r[2]), l: num(&r[3]), c: num(&r[4]), v: num(r.get(6)?) })
            })
            .collect())
    }

    async fn coinbase(&self, product: &str, interval: u32) -> Result<Vec<Candle>> {
        let granularity = match interval {
            1 => 60,
            5 => 300,
            15 => 900,
            60 => 3600,
            1440 => 86400,
            _ => return Err(Error::Invalid("Coinbase has no candles of that size".into())),
        };
        let mut url = url::Url::parse(&format!("{}/products/{product}/candles", self.feeds.coinbase)).map_err(|_| Error::Other("Coinbase's address is wrong".into()))?;
        url.query_pairs_mut().append_pair("granularity", &granularity.to_string());
        let got: Value = self.http.get(url).send().await.map_err(net("Coinbase"))?.json().await.map_err(net("Coinbase"))?;
        // Coinbase: time, low, high, open, close, volume; the newest first.
        let mut out: Vec<Candle> = got
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let r = r.as_array()?;
                let n = |i: usize| r.get(i).and_then(Value::as_f64).unwrap_or(f64::NAN);
                Some(Candle { t: r.first()?.as_i64()? * 1000, l: n(1), h: n(2), o: n(3), c: n(4), v: n(5) })
            })
            .collect();
        out.sort_by_key(|c| c.t);
        Ok(out)
    }

    /// A year of the ECB's daily rates for a pair, as candles whose four prices are the day's rate.
    async fn ecb(&self, symbol: &str) -> Result<Vec<Candle>> {
        let (base, quote) = symbol.split_once('/').ok_or_else(|| Error::Invalid("a currency pair is two codes".into()))?;
        let since = jiff::Timestamp::from_millisecond(crate::now_ms() - 365 * 86_400_000)
            .map(|t| t.to_zoned_datetime(jiff::tz::TimeZone::UTC).strftime("%Y-%m-%d").to_string())
            .unwrap_or_else(|_| "2025-01-01".into());
        let mut url = url::Url::parse(&format!("{}/v1/{since}..", self.feeds.frankfurter)).map_err(|_| Error::Other("Frankfurter's address is wrong".into()))?;
        url.query_pairs_mut().append_pair("base", base).append_pair("symbols", quote);
        let got: Value = self.http.get(url).send().await.map_err(net("Frankfurter"))?.json().await.map_err(net("Frankfurter"))?;
        let mut out: Vec<Candle> = got["rates"]
            .as_object()
            .into_iter()
            .flatten()
            .filter_map(|(day, rates)| {
                let rate = rates.get(quote)?.as_f64()?;
                let t = format!("{day}T00:00:00").parse::<jiff::civil::DateTime>().ok()?.to_zoned_datetime(jiff::tz::TimeZone::UTC).ok()?.timestamp().as_millisecond();
                Some(Candle { t, o: rate, h: rate, l: rate, c: rate, v: 0.0 })
            })
            .collect();
        out.sort_by_key(|c| c.t);
        Ok(out)
    }
}

/// What the person asks a model to write, on which chart.
#[derive(Clone, Debug, Deserialize, ToSchema)]
pub struct Write {
    pub request: String,
    pub symbol: String,
    pub interval: u32,
    /// The script to start from, when there is one.
    #[serde(default)]
    pub source: Option<String>,
    /// The session to write it in; a new one, "Pine scripts", when left out.
    #[serde(default)]
    pub session: Option<String>,
    /// The model, for a new session.
    #[serde(default)]
    pub model: Option<String>,
}

/// What came of it: the last script, how many tries, and what it gave or the error it still has.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Written {
    pub session: String,
    pub source: String,
    pub tries: u32,
    pub ran: Option<pine::Ran>,
    pub error: Option<String>,
}

/// A script of the person's, kept on this computer (M12).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Script {
    pub id: String,
    pub name: String,
    pub source: String,
    pub updated_at: i64,
}

/// The scripts kept, the newest first.
pub fn scripts(db: &crate::db::Db) -> Result<Vec<Script>> {
    db.with(|c| {
        c.prepare("SELECT id, name, source, updated_at FROM pine_scripts ORDER BY updated_at DESC")?
            .query_map([], |r| Ok(Script { id: r.get(0)?, name: r.get(1)?, source: r.get(2)?, updated_at: r.get(3)? }))?
            .collect()
    })
}

/// Keeps a script: a new one without an id, or in place of the one with it.
pub fn keep(db: &crate::db::Db, id: Option<&str>, name: &str, source: &str) -> Result<Script> {
    if source.len() > 100_000 {
        return Err(Error::Invalid("A script is kept up to 100,000 characters.".into()));
    }
    let name = if name.trim().is_empty() { "Script".to_string() } else { name.trim().chars().take(80).collect() };
    let id = id.map(str::to_string).unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()[..8].to_string());
    let at = crate::now_ms();
    db.with(|c| {
        c.execute(
            "INSERT INTO pine_scripts (id, name, source, updated_at) VALUES (?1, ?2, ?3, ?4) ON CONFLICT (id) DO UPDATE SET name = ?2, source = ?3, updated_at = ?4",
            rusqlite::params![id, name, source, at],
        )
    })?;
    Ok(Script { id, name, source: source.into(), updated_at: at })
}

pub fn forget(db: &crate::db::Db, id: &str) -> Result<()> {
    db.with(|c| c.execute("DELETE FROM pine_scripts WHERE id = ?1", [id]))?;
    Ok(())
}

/// The script in a model's answer: its one block of code when it wrote one, or else the whole of it.
pub fn script_in(answer: &str) -> String {
    let mut blocks = answer.split("```").skip(1).step_by(2);
    match blocks.next() {
        Some(block) => block.trim_start_matches(|c: char| c.is_alphanumeric() || c == '-').trim_start_matches('\n').trim().to_string(),
        None => answer.trim().to_string(),
    }
}

/// What a model is asked, to write a script for the person (M3): the engine's part of the language, said
/// shortly, the request, and the script it is to mend when there is one. Material first, the ask last (L13).
pub fn asked_to_write(request: &str, current: Option<&str>, error: Option<&str>) -> String {
    let mut out = String::from(
        "Demido's chart runs a part of Pine Script v6: indicator(title, overlay=true|false); name = expression; input.int(default) and input.float(default); the series open, high, low, close, volume, hl2, hlc3, ohlc4; x[n]; + - * / %; < > <= >= == !=; and, or, not; cond ? a : b; ta.sma, ta.ema, ta.rma, ta.rsi, ta.stdev, ta.highest, ta.lowest, ta.change, ta.crossover, ta.crossunder, ta.atr; math.abs, math.max, math.min, math.sqrt, math.pow, math.log, math.round; nz; plot(series, \"title\", color=color.green); hline(price, \"title\"). No if, for, while, functions of your own, request.* or strategies.\n\n",
    );
    if let Some(current) = current.filter(|c| !c.trim().is_empty()) {
        out.push_str(&format!("The script so far:\n```\n{}\n```\n\n", current.trim()));
    }
    if let Some(error) = error {
        out.push_str(&format!("When it was run, Demido's chart said: {error}\n\n"));
    }
    out.push_str(&format!("The person asked: \"{}\"\n\n", request.trim()));
    out.push_str(if error.is_some() { "Write the whole script again, mended, in one block of code, and nothing else." } else { "Write the whole script, in one block of code, and nothing else." });
    out
}

#[cfg(test)]
mod tests;
