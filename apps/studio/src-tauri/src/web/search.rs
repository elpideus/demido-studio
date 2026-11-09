//! Web search without an account: engines tried in turn until one answers.
//!
//! Search engines do not like being asked by programs. Each one refuses now and then (a captcha,
//! "unusual traffic", an empty page), and which one refuses changes from day to day, so a search
//! climbs a ladder: the person's own SearXNG server if they set one, Exa's free search service
//! (if allowed), DuckDuckGo's HTML and lite pages, Brave, Bing, and last the search page of the
//! person's chosen engine rendered in the built-in browser, which passes for a person because it
//! is a browser. A rung that answers with results ends the climb; one that answers with none is
//! believed only if no other rung has any; one that fails is reported with the results, so the
//! model and the person can tell "nothing found" from "nobody would answer".
//!
//! The parsers are pure functions of a page, tested against trimmed copies of real result pages
//! in `tests/fixtures/search`.

use std::time::Duration;

use base64::Engine as _;
use dom_query::{Document, Selection};
use serde::Serialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::page::{self, clean_line};
use super::{Web, browser};
use crate::settings::SearchEngine;

/// What one engine asks for at most, however many results the model wants.
const MAX_RESULTS: usize = 10;
const SNIPPET_CHARS: usize = 400;
/// Exa's snippets are passages of the page itself, worth more of them.
const EXA_SNIPPET_CHARS: usize = 1_200;
const TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

/// What a search found, and from where.
#[derive(Clone, Debug, Serialize)]
pub struct Found {
    /// The engine that answered.
    pub engine: &'static str,
    pub hits: Vec<Hit>,
    /// The engines tried before it, with why each gave nothing.
    pub tried: Vec<String>,
}

enum Outcome {
    Hits(Vec<Hit>),
    /// The engine answered, with nothing.
    Empty,
    Failed(String),
}

/// Searches the web for `query`, `count` results at most. `chat` lends its browser tab to the
/// last rung.
pub async fn search(
    web: &Web,
    query: &str,
    count: usize,
    chat: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Found, String> {
    let count = count.clamp(1, MAX_RESULTS);
    let key = format!("{count}\n{}", query.trim().to_lowercase());
    if let Some(kept) = web.searches.lock().get(&key) {
        return Ok(kept);
    }
    let found = climb(web, query, count, chat, cancel).await?;
    if !found.hits.is_empty() {
        web.searches.lock().put(key, found.clone());
    }
    Ok(found)
}

async fn climb(
    web: &Web,
    query: &str,
    count: usize,
    chat: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Found, String> {
    let settings = web.settings.get();
    let mut tried: Vec<String> = Vec::new();
    let mut empty: Option<&'static str> = None;
    let rungs: Vec<&'static str> = [
        settings
            .searxng_url
            .as_ref()
            .filter(|u| !u.trim().is_empty())
            .map(|_| "SearXNG"),
        settings.web_exa.then_some("Exa"),
        Some("DuckDuckGo"),
        Some("DuckDuckGo Lite"),
        Some("Brave"),
        Some("Bing"),
    ]
    .into_iter()
    .flatten()
    .collect();
    for engine in rungs {
        if cancel.is_cancelled() {
            return Err("Stopped.".into());
        }
        let outcome = ask(&web.client, engine, settings.searxng_url.as_deref(), query, count).await;
        match outcome {
            Outcome::Hits(hits) => return Ok(found(engine, hits, count, tried)),
            Outcome::Empty => {
                empty.get_or_insert(engine);
                tried.push(format!("{engine}: no results"));
            }
            Outcome::Failed(why) => {
                tracing::info!(engine, %why, "web search engine refused");
                tried.push(format!("{engine}: {why}"));
            }
        }
    }
    if let Some(engine) = empty {
        return Ok(found(engine, Vec::new(), count, tried));
    }
    // Every engine refused the app: ask in the browser, as a person would.
    if let Some(chat) = chat {
        let engine = settings.browser_search_engine;
        match in_browser(web, chat, query, engine, cancel).await {
            Outcome::Hits(hits) => return Ok(found(engine_name(engine), hits, count, tried)),
            Outcome::Empty => return Ok(found(engine_name(engine), Vec::new(), count, tried)),
            Outcome::Failed(why) => tried.push(format!("{} in the browser: {why}", engine_name(engine))),
        }
    }
    Err(format!("No search engine would answer. {}", tried.join("; ")))
}

/// Asks one engine over plain HTTP.
async fn ask(client: &reqwest::Client, engine: &str, searxng_url: Option<&str>, query: &str, count: usize) -> Outcome {
    let outcome = match engine {
        "SearXNG" => return searxng(client, searxng_url.unwrap_or_default(), query).await,
        "Exa" => return exa(client, query, count).await,
        "DuckDuckGo" => ddg_html(client, query).await,
        "DuckDuckGo Lite" => ddg_lite(client, query).await,
        "Brave" => brave(client, query).await,
        _ => bing(client, query).await,
    };
    // A results page read off a site may be a decoy: Bing answers a program it distrusts with
    // results for something else, or with none at all.
    match outcome {
        Outcome::Hits(hits) if !about(query, &hits) => {
            Outcome::Failed("answered with results for something else".into())
        }
        Outcome::Empty if engine == "Bing" => Outcome::Failed("answered no results, as it does to programs".into()),
        other => other,
    }
}

/// Whether most of `hits` mention a word of `query`.
fn about(query: &str, hits: &[Hit]) -> bool {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 || !w.is_ascii())
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() {
        return true;
    }
    let mentioning = hits
        .iter()
        .filter(|h| {
            let text = format!("{} {} {}", h.title, h.snippet, h.url).to_lowercase();
            words.iter().any(|w| text.contains(w.as_str()))
        })
        .count();
    mentioning * 2 >= hits.len()
}

fn found(engine: &'static str, mut hits: Vec<Hit>, count: usize, tried: Vec<String>) -> Found {
    let mut seen = std::collections::HashSet::new();
    hits.retain(|h| seen.insert(h.url.trim_end_matches('/').to_string()));
    hits.truncate(count);
    Found { engine, hits, tried }
}

pub fn engine_name(engine: SearchEngine) -> &'static str {
    match engine {
        SearchEngine::Duckduckgo => "DuckDuckGo",
        SearchEngine::Brave => "Brave",
        SearchEngine::Bing => "Bing",
        SearchEngine::Google => "Google",
    }
}

// ---- The engines -----------------------------------------------------------------------

/// Reads a result page, refusing what is not one: an error status, a bot check.
async fn result_page(engine: &str, request: reqwest::RequestBuilder) -> Result<String, String> {
    let response = request
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| format!("could not be reached ({})", short_error(&e)))?;
    let status = response.status();
    let server = response
        .headers()
        .get("server")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let mitigated = response.headers().contains_key("cf-mitigated");
    let html = response.text().await.map_err(|e| short_error(&e))?;
    let check = page::check(&page::Response {
        status: status.as_u16(),
        server: &server,
        mitigated,
        html: &html,
    });
    if let Some(check) = check {
        return Err(format!("{engine} asked for {}", check.label()));
    }
    // DuckDuckGo answers 202 with an empty page when it has had enough of a caller.
    if status != reqwest::StatusCode::OK {
        return Err(format!("answered {status}"));
    }
    Ok(html)
}

fn outcome(result: Result<Vec<Hit>, String>) -> Outcome {
    match result {
        Ok(hits) if hits.is_empty() => Outcome::Empty,
        Ok(hits) => Outcome::Hits(hits),
        Err(why) => Outcome::Failed(why),
    }
}

async fn ddg_html(client: &reqwest::Client, query: &str) -> Outcome {
    let request = client
        .post("https://html.duckduckgo.com/html/")
        .header("Referer", "https://html.duckduckgo.com/")
        .form(&[("q", query), ("b", "")]);
    outcome(
        result_page("DuckDuckGo", request)
            .await
            .map(|html| ddg_html_hits(&html)),
    )
}

async fn ddg_lite(client: &reqwest::Client, query: &str) -> Outcome {
    let request = client
        .post("https://lite.duckduckgo.com/lite/")
        .header("Referer", "https://lite.duckduckgo.com/")
        .form(&[("q", query)]);
    outcome(
        result_page("DuckDuckGo", request)
            .await
            .map(|html| ddg_lite_hits(&html)),
    )
}

async fn brave(client: &reqwest::Client, query: &str) -> Outcome {
    let request = client
        .get("https://search.brave.com/search")
        .query(&[("q", query), ("source", "web")]);
    outcome(result_page("Brave", request).await.map(|html| brave_hits(&html)))
}

async fn bing(client: &reqwest::Client, query: &str) -> Outcome {
    let request = client
        .get("https://www.bing.com/search")
        .query(&[("q", query), ("setlang", "en")]);
    outcome(result_page("Bing", request).await.map(|html| bing_hits(&html)))
}

async fn searxng(client: &reqwest::Client, base: &str, query: &str) -> Outcome {
    let base = base.trim().trim_end_matches('/');
    let Ok(url) = Url::parse(&format!("{base}/search")) else {
        return Outcome::Failed("its address in Settings is not a web address".into());
    };
    // JSON when the server allows it, its HTML page when it does not.
    let json = client
        .get(url.clone())
        .query(&[("q", query), ("format", "json")])
        .timeout(TIMEOUT)
        .send()
        .await;
    if let Ok(response) = json
        && response.status().is_success()
        && let Ok(body) = response.json::<Value>().await
    {
        return outcome(Ok(searxng_json_hits(&body)));
    }
    let request = client.get(url).query(&[("q", query)]);
    outcome(
        result_page("SearXNG", request)
            .await
            .map(|html| searxng_html_hits(&html)),
    )
}

/// Exa's search, through its free MCP endpoint: no account, a fair-use limit.
async fn exa(client: &reqwest::Client, query: &str, count: usize) -> Outcome {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "web_search_exa",
            "arguments": {"query": query, "numResults": count.max(5), "objective": query},
        },
    });
    match exa_call(client, &body).await {
        Ok(text) => outcome(Ok(exa_hits(&text))),
        Err(why) => Outcome::Failed(why),
    }
}

pub(super) const EXA_URL: &str = "https://mcp.exa.ai/mcp";

/// Calls one of Exa's MCP tools and returns its text.
pub(super) async fn exa_call(client: &reqwest::Client, body: &Value) -> Result<String, String> {
    let response = client
        .post(EXA_URL)
        .header("Accept", "application/json, text/event-stream")
        .json(body)
        .timeout(Duration::from_secs(40))
        .send()
        .await
        .map_err(|e| format!("could not be reached ({})", short_error(&e)))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| short_error(&e))?;
    if !status.is_success() {
        return Err(match status.as_u16() {
            429 => "is over its free limit for now".into(),
            _ => format!("answered {status}"),
        });
    }
    mcp_text(&text)
}

/// The text an MCP tool answered, from a JSON or event-stream response.
pub(super) fn mcp_text(body: &str) -> Result<String, String> {
    let messages: Vec<Value> = if body.trim_start().starts_with('{') {
        serde_json::from_str(body).into_iter().collect()
    } else {
        body.lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str(d.trim()).ok())
            .collect()
    };
    for m in messages {
        if let Some(e) = m.get("error") {
            return Err(e["message"].as_str().unwrap_or("failed").to_string());
        }
        let Some(result) = m.get("result") else { continue };
        let text: Vec<&str> = result["content"]
            .as_array()
            .map(|c| c.iter().filter_map(|p| p["text"].as_str()).collect())
            .unwrap_or_default();
        let text = text.join("\n");
        if result["isError"] == true {
            return Err(text.chars().take(200).collect());
        }
        return Ok(text);
    }
    Err("answered nothing readable".into())
}

/// The search page of `engine`, rendered in the chat's tab.
async fn in_browser(web: &Web, chat: &str, query: &str, engine: SearchEngine, cancel: &CancellationToken) -> Outcome {
    let b = &web.browser;
    let result: Result<Vec<Hit>, String> = async {
        let tab = b.chat_tab_or_open(chat).await?;
        let _lease = b.lease(tab).await?;
        b.load(tab, &browser::search_url(query, engine), Duration::from_secs(30))
            .await?;
        if b.outwait(tab, Duration::from_secs(8), cancel).await.is_some() {
            let message = format!(
                "{} wants to check that a person is searching. Solve the check, and the assistant goes on by itself.",
                engine_name(engine)
            );
            b.ask(tab, chat, &message, Duration::from_secs(180), true, cancel)
                .await?;
            b.settle(tab, Duration::from_secs(6)).await;
        }
        let page = b.html(tab).await?;
        Ok(hits_for(engine, &page.html))
    }
    .await;
    outcome(result)
}

fn hits_for(engine: SearchEngine, html: &str) -> Vec<Hit> {
    match engine {
        SearchEngine::Duckduckgo => {
            let hits = ddg_js_hits(html);
            if hits.is_empty() { ddg_html_hits(html) } else { hits }
        }
        SearchEngine::Brave => brave_hits(html),
        SearchEngine::Bing => bing_hits(html),
        SearchEngine::Google => google_hits(html),
    }
}

fn short_error(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".into()
    } else if e.is_connect() {
        "no connection".into()
    } else {
        e.to_string().chars().take(120).collect()
    }
}

// ---- Parsers ---------------------------------------------------------------------------

fn text_of(sel: &Selection) -> String {
    clean_line(&sel.text())
}

fn snippet(s: &str, max: usize) -> String {
    let s = clean_line(s);
    if s.chars().count() <= max {
        return s;
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

fn hit(title: String, url: String, snippet_text: &str, date: Option<String>) -> Option<Hit> {
    let parsed = Url::parse(&url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let title = if title.is_empty() { url.clone() } else { title };
    Some(Hit {
        title,
        url,
        snippet: snippet(snippet_text, SNIPPET_CHARS),
        date: date.filter(|d| !d.is_empty()),
    })
}

/// A result link's target, from DuckDuckGo's redirect (`//duckduckgo.com/l/?uddg=…`) when it is
/// one. Its ads go through `y.js`, and are dropped.
fn ddg_target(href: &str) -> Option<String> {
    let href = href.trim();
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let url = Url::parse(&absolute).ok()?;
    if url.host_str().is_some_and(|h| h.ends_with("duckduckgo.com")) {
        if url.path() == "/l/" {
            return url
                .query_pairs()
                .find(|(k, _)| k == "uddg")
                .map(|(_, v)| v.into_owned());
        }
        return None;
    }
    Some(absolute)
}

/// DuckDuckGo's HTML page (html.duckduckgo.com).
pub(crate) fn ddg_html_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("div.result")
        .iter()
        .filter(|r| !r.has_class("result--ad"))
        .filter_map(|r| {
            let a = r.select("a.result__a").first();
            let url = ddg_target(&a.attr("href")?)?;
            hit(text_of(&a), url, &r.select(".result__snippet").first().text(), None)
        })
        .collect()
}

/// DuckDuckGo's lite page: each result is rows of a table, the link's then the snippet's.
pub(crate) fn ddg_lite_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    let mut hits: Vec<Hit> = Vec::new();
    let mut last_kept = false;
    for el in doc.select("a.result-link, td.result-snippet").iter() {
        if el.is("a") {
            let sponsored = el.ancestors(None).is("tr.result-sponsored");
            let kept = (!sponsored)
                .then(|| el.attr("href"))
                .flatten()
                .and_then(|h| ddg_target(&h))
                .and_then(|url| hit(text_of(&el), url, "", None));
            last_kept = kept.is_some();
            hits.extend(kept);
        } else if last_kept && let Some(h) = hits.last_mut().filter(|h| h.snippet.is_empty()) {
            h.snippet = snippet(&el.text(), SNIPPET_CHARS);
        }
    }
    hits
}

/// DuckDuckGo's own page, as the browser renders it.
pub(crate) fn ddg_js_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("article[data-testid=result]")
        .iter()
        .filter_map(|r| {
            let a = r.select("a[data-testid=result-title-a]").first();
            let url = a.attr("href")?.to_string();
            hit(
                text_of(&a),
                url,
                &r.select("[data-result=snippet]").first().text(),
                None,
            )
        })
        .collect()
}

/// Brave's page: a snippet's date leads its text.
pub(crate) fn brave_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("div.snippet[data-type=web]")
        .iter()
        .filter_map(|r| {
            let a = r.select("a[href]").first();
            let url = a.attr("href")?.to_string();
            let title_el = r.select(".title").first();
            let title = title_el
                .attr("title")
                .map(|t| clean_line(&t))
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| text_of(&title_el));
            let content = r.select(".content").first();
            let date_el = content.select(".t-secondary").first();
            let date = text_of(&date_el).trim_end_matches(['-', ' ']).to_string();
            date_el.remove();
            hit(title, url, &content.text(), Some(date))
        })
        .collect()
}

/// A result link's target, from Bing's click-tracking redirect (`/ck/a?…&u=a1<base64>`) when it
/// is one.
fn bing_target(href: &str) -> Option<String> {
    let url = Url::parse(href).ok()?;
    if url.host_str().is_some_and(|h| h.ends_with("bing.com")) && url.path().starts_with("/ck/") {
        let u = url.query_pairs().find(|(k, _)| k == "u")?.1.into_owned();
        let encoded = u.strip_prefix("a1").unwrap_or(&u);
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.trim_end_matches('='))
            .ok()?;
        return String::from_utf8(bytes).ok();
    }
    Some(href.to_string())
}

pub(crate) fn bing_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("a.b_algoReadMore").remove();
    doc.select("li.b_algo")
        .iter()
        .filter_map(|r| {
            let a = r.select("h2 a").first();
            let url = bing_target(&a.attr("href")?)?;
            let caption = r
                .select(".b_caption p, p.b_lineclamp2, p.b_lineclamp3, p.b_lineclamp4, .b_caption")
                .first();
            hit(text_of(&a), url, &caption.text(), None)
        })
        .collect()
}

/// Google's page, as the browser renders it: each result's title is an `h3` inside its link.
pub(crate) fn google_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("#search h3, #rso h3")
        .iter()
        .filter_map(|h3| {
            let a = h3.ancestors(Some(4)).filter("a[href]").first();
            let href = a.attr("href")?.to_string();
            let url = if href.starts_with("/url?") {
                Url::parse(&format!("https://www.google.com{href}"))
                    .ok()?
                    .query_pairs()
                    .find(|(k, _)| k == "q")?
                    .1
                    .into_owned()
            } else {
                href
            };
            let block = h3
                .ancestors(Some(12))
                .filter("div.g, div.MjjYud, div[data-hveid]")
                .first();
            let text = block
                .select(".VwiC3b, [data-sncf], div[style*='-webkit-line-clamp']")
                .first()
                .text();
            hit(text_of(&h3), url, &text, None)
        })
        .collect()
}

pub(crate) fn searxng_json_hits(body: &Value) -> Vec<Hit> {
    body["results"]
        .as_array()
        .map(|results| {
            results
                .iter()
                .filter_map(|r| {
                    hit(
                        clean_line(r["title"].as_str().unwrap_or_default()),
                        r["url"].as_str()?.to_string(),
                        r["content"].as_str().unwrap_or_default(),
                        r["publishedDate"].as_str().map(|d| d.chars().take(10).collect()),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn searxng_html_hits(html: &str) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("article.result")
        .iter()
        .filter_map(|r| {
            let a = r.select("h3 a").first();
            let url = a.attr("href")?.to_string();
            hit(text_of(&a), url, &r.select(".content").first().text(), None)
        })
        .collect()
}

/// Exa's results: blocks of `Title:`, `URL:`, `Published:`, `Author:` and `Highlights:` lines.
pub(crate) fn exa_hits(text: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Title: ") || blocks.is_empty() {
            blocks.push(String::new());
        }
        let block = blocks.last_mut().expect("a block");
        block.push_str(line);
        block.push('\n');
    }
    for block in blocks {
        let field = |name: &str| {
            block
                .lines()
                .find_map(|l| l.strip_prefix(name))
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty() && v != "N/A")
        };
        let (Some(title), Some(url)) = (field("Title: "), field("URL: ")) else {
            continue;
        };
        let highlights = block
            .split_once("\nHighlights:")
            .or_else(|| block.split_once("\nText:"))
            .map(|(_, rest)| rest.trim())
            .unwrap_or_default();
        let date = field("Published: ").map(|d| d.chars().take(10).collect());
        if let Some(mut h) = hit(title, url, "", date) {
            h.snippet = exa_snippet(highlights, &h.title);
            hits.push(h);
        }
    }
    hits
}

/// Exa's highlights, without the title they often repeat, kept to a passage.
fn exa_snippet(highlights: &str, title: &str) -> String {
    let lines: Vec<&str> = highlights
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != title && *l != "...")
        .collect();
    let joined = lines.join(" ");
    let joined = joined.trim_start_matches(title).trim();
    if joined.chars().count() <= EXA_SNIPPET_CHARS {
        return joined.to_string();
    }
    let cut: String = joined.chars().take(EXA_SNIPPET_CHARS).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/search")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    fn urls(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.url.as_str()).collect()
    }

    #[test]
    fn duckduckgo_html_results_skip_ads() {
        let hits = ddg_html_hits(&fixture("ddg.html"));
        assert_eq!(
            urls(&hits),
            [
                "https://doc.rust-lang.org/stable/releases.html",
                "https://releases.rs/",
                "https://blog.rust-lang.org/releases/latest/",
            ]
        );
        assert_eq!(hits[0].title, "Rust Release Notes");
        assert!(
            hits[0].snippet.starts_with("Note that this only stabilizes"),
            "{}",
            hits[0].snippet
        );
    }

    #[test]
    fn duckduckgo_lite_results_pair_links_with_snippets() {
        let hits = ddg_lite_hits(&fixture("ddglite.html"));
        assert_eq!(
            urls(&hits),
            ["https://doc.rust-lang.org/stable/releases.html", "https://releases.rs/"]
        );
        assert_eq!(hits[1].title, "Rust Versions | Rust Changelogs");
        assert!(
            hits[1].snippet.starts_with("relnotes - Marks issues"),
            "{}",
            hits[1].snippet
        );
        assert_eq!(hits[0].snippet, "What changed in each release of Rust.");
    }

    #[test]
    fn brave_results_keep_their_dates_apart() {
        let hits = brave_hits(&fixture("brave.html"));
        assert_eq!(
            urls(&hits),
            ["https://doc.rust-lang.org/beta/releases.html", "https://releases.rs/"]
        );
        assert_eq!(hits[0].title, "Rust Release Notes");
        assert_eq!(hits[0].date.as_deref(), Some("September 3, 2026"));
        assert!(hits[0].snippet.starts_with("On Emscripten"), "{}", hits[0].snippet);
        assert_eq!(hits[1].date, None);
    }

    #[test]
    fn bing_results_lose_their_click_tracking() {
        let hits = bing_hits(&fixture("bing.html"));
        assert_eq!(
            urls(&hits),
            [
                "https://rust-lang.org/",
                "https://en.wikipedia.org/wiki/Rust_(programming_language)"
            ]
        );
        assert_eq!(hits[0].title, "Rust Programming Language");
        assert!(!hits[0].snippet.contains("Read more"), "{}", hits[0].snippet);
    }

    #[test]
    fn duckduckgos_own_page_and_googles_are_read_in_the_browser() {
        let ddg = r#"<div><article data-testid="result"><h2><a data-testid="result-title-a" href="https://a.example/x"><span>Alpha</span></a></h2><div data-result="snippet"><span>About <b>alpha</b>.</span></div></article></div>"#;
        let hits = ddg_js_hits(ddg);
        assert_eq!(urls(&hits), ["https://a.example/x"]);
        assert_eq!(hits[0].snippet, "About alpha.");
        let google = r#"<div id="search"><div class="MjjYud"><div class="g"><a href="https://b.example/"><h3>Beta</h3></a><div class="VwiC3b">Beta things.</div></div></div><div class="g"><a href="/url?q=https://c.example/page&amp;sa=U"><h3>Gamma</h3></a></div></div>"#;
        let hits = google_hits(google);
        assert_eq!(urls(&hits), ["https://b.example/", "https://c.example/page"]);
        assert_eq!(hits[0].snippet, "Beta things.");
    }

    #[test]
    fn searxng_answers_in_json_or_html() {
        let body = json!({"results": [
            {"url": "https://a.example/", "title": "A", "content": "About A", "publishedDate": "2026-10-01T00:00:00"},
            {"url": "javascript:x", "title": "Bad"},
        ]});
        let hits = searxng_json_hits(&body);
        assert_eq!(urls(&hits), ["https://a.example/"]);
        assert_eq!(hits[0].date.as_deref(), Some("2026-10-01"));
        let html = r#"<article class="result"><h3><a href="https://b.example/">B</a></h3><p class="content">About B</p></article>"#;
        assert_eq!(urls(&searxng_html_hits(html)), ["https://b.example/"]);
    }

    #[test]
    fn exa_blocks_are_split_into_results() {
        let text = "Title: Rust Release Notes\nURL: https://doc.rust-lang.org/stable/releases.html\nPublished: N/A\nAuthor: N/A\nHighlights:\nRust Release Notes\n\n# Version 1.99.1 (2026-10-15)\n\n- library: fix missing symbol\n\nTitle: Releases\nURL: https://releases.rs/\nPublished: 2026-10-16T00:00:00.000Z\nAuthor: N/A\nHighlights:\nRust Versions";
        let hits = exa_hits(text);
        assert_eq!(
            urls(&hits),
            ["https://doc.rust-lang.org/stable/releases.html", "https://releases.rs/"]
        );
        assert_eq!(hits[0].date, None);
        assert_eq!(hits[1].date.as_deref(), Some("2026-10-16"));
        assert_eq!(
            hits[0].snippet,
            "# Version 1.99.1 (2026-10-15) - library: fix missing symbol"
        );
    }

    #[test]
    fn mcp_answers_are_read_from_event_streams_and_errors_are_errors() {
        let ok = "event: message\ndata: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]},\"jsonrpc\":\"2.0\",\"id\":1}\n\n";
        assert_eq!(mcp_text(ok).unwrap(), "hello");
        let failed = "data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Error fetching URL(s): x: CRAWL_UNKNOWN_ERROR\"}],\"isError\":true}}\n";
        assert!(mcp_text(failed).unwrap_err().contains("CRAWL_UNKNOWN_ERROR"));
        assert!(
            mcp_text("{\"error\":{\"message\":\"rate limited\"}}")
                .unwrap_err()
                .contains("rate")
        );
    }

    #[test]
    fn results_for_something_else_are_told_apart() {
        let h = |title: &str, url: &str| Hit {
            title: title.into(),
            url: url.into(),
            snippet: String::new(),
            date: None,
        };
        let rust = [
            h("Rust Programming Language", "https://rust-lang.org/"),
            h("The Rust Book", "https://doc.rust-lang.org/book/"),
            h("Learn it", "https://example.com/"),
        ];
        assert!(about("rust programming language", &rust));
        let decoys = [
            h("Ricerca punto vendita | Lidl Italia", "https://www.lidl.it/"),
            h("Store Finder - Lidl US", "https://www.lidl.com/"),
            h(
                "为什么这么多人把「Wikipedia」称作「wiki」？",
                "https://www.zhihu.com/question/19581089",
            ),
        ];
        assert!(!about("rust programming language", &decoys));
        // Words too short to tell anything by say nothing either way.
        assert!(about("C++ vs Go", &decoys));
        assert!(about(
            "维基百科",
            &[h("维基百科，自由的百科全书", "https://zh.wikipedia.org/")]
        ));
    }

    #[test]
    fn duplicate_results_count_once() {
        let h = |u: &str| Hit {
            title: "t".into(),
            url: u.into(),
            snippet: String::new(),
            date: None,
        };
        let f = found(
            "X",
            vec![h("https://a.example/"), h("https://a.example"), h("https://b.example/")],
            5,
            vec![],
        );
        assert_eq!(f.hits.len(), 2);
    }

    /// Asks every engine for real: `cargo test --lib -- --ignored live_engines --nocapture`. An
    /// engine may refuse a program from this network (a bot check, a 202), but one that answers
    /// must yield results: an answer read as none means its page changed and its reader needs
    /// mending.
    #[tokio::test]
    #[ignore]
    async fn live_engines() {
        let client = crate::web::client().unwrap();
        let mut answered = 0;
        for engine in ["Exa", "DuckDuckGo", "DuckDuckGo Lite", "Brave", "Bing"] {
            match ask(&client, engine, None, "rust programming language", 5).await {
                Outcome::Hits(hits) => {
                    answered += 1;
                    println!(
                        "{engine}: {} results, first {} <{}>",
                        hits.len(),
                        hits[0].title,
                        hits[0].url
                    );
                    assert!(
                        hits.iter().all(|h| h.url.starts_with("http") && !h.title.is_empty()),
                        "{engine}: {hits:#?}"
                    );
                }
                Outcome::Empty => panic!("{engine} answered, but no results were read from its page"),
                Outcome::Failed(why) => println!("{engine} refused: {why}"),
            }
        }
        assert!(answered > 0, "no engine answered");
    }
}
