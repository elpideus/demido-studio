//! Searching the internet without an account: sources tried in turn until one answers.
//!
//! The shape is the previous Demido's, which the owner asked for (DECISIONS N13): search services refuse
//! programs now and then, and which one refuses changes from day to day, so a search climbs a ladder.
//! A rung that answers with results ends the climb. One that answers with none is believed only when no
//! later rung has any. One that fails is named with the results, so the model and the person can tell
//! "nothing was found" from "nobody would answer".
//!
//! The rungs, in order:
//!
//! 1. **The person's own SearXNG server**, when its address is set.
//! 2. **Exa**, through the service it offers assistants: no account, a free limit. It searches the whole
//!    web and answers with passages of the pages it found, which is what a small model needs.
//! 3. **DuckDuckGo**, through the page it serves to browsers without scripts, and
//! 4. its lighter page. Both are asked as their own form asks: the form's page is loaded, then the form is
//!    sent from it. Asked any other way DuckDuckGo answers a program with a puzzle for a person (tried,
//!    research note 15). When it does that all the same, Demido leaves it alone for a while.
//! 5. **Wikipedia and Mwmbl** together: the two that have an interface for programs and need nothing.
//!
//! Every rung is asked under Demido's own name. Not here, though the previous Demido had them: reading
//! Brave's and Bing's result pages (their own `robots.txt` says not to), a browser's name in place of
//! Demido's, and the built-in browser as the last rung (there is no built-in browser yet).
//!
//! The words of a search are the one thing here that a model writes and that leaves the computer, so they
//! are kept short, and they go to the rungs that are turned on and nowhere else.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dom_smoothie::dom_query::Document;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

use super::{ACCEPT_A_PAGE, Ask, SEARCH_IS_FRESH, Sent, Through, Web, guard, page};

/// A search is a few words. Longer than this is not a search but a text being sent somewhere.
pub const MOST_OF_A_QUERY: usize = 200;
/// How many results a model is given.
const MOST_RESULTS: usize = 6;
/// How much of what a result is about.
const MOST_ABOUT: usize = 260;
/// Exa's are passages of the page itself, worth more of them; and then fewer results.
const MOST_ABOUT_FROM_EXA: usize = 800;
const MOST_FROM_EXA: usize = 5;
/// How much of the first result's own page is given with the results.
const MOST_OF_THE_FIRST: usize = 2_500;
/// Mwmbl allows one search a second without a key. A little slower than that, to be sure.
const BETWEEN_TWO_MWMBL: Duration = Duration::from_millis(1200);
const BETWEEN_TWO_WIKIPEDIA: Duration = Duration::from_secs(1);
const BETWEEN_TWO_EXA: Duration = Duration::from_secs(1);
const BETWEEN_TWO_DUCKDUCKGO: Duration = Duration::from_secs(2);
/// The person's own server is asked as fast as they ask.
const BETWEEN_TWO_OF_THEIR_OWN: Duration = Duration::from_millis(100);
/// How long a source is left alone after it said it is being asked too much, or asked for a person.
const LEFT_ALONE_FOR: Duration = Duration::from_secs(15 * 60);
/// The page a form stands on is kept this long before it is loaded again.
const A_FORM_IS_FRESH: Duration = Duration::from_secs(24 * 60 * 60);

const SEARXNG: &str = "Your SearXNG";
const EXA: &str = "Exa";
const DUCKDUCKGO: &str = "DuckDuckGo";
const DUCKDUCKGO_LITE: &str = "DuckDuckGo Lite";
const WIKIPEDIA: &str = "Wikipedia";
const MWMBL: &str = "Mwmbl";
const WIKIPEDIA_AND_MWMBL: &str = "Wikipedia and Mwmbl";

/// One page that a search found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub title: String,
    pub address: String,
    /// A line or two of what it says. From Exa, a passage of the page.
    pub about: String,
    /// Which sources found it.
    pub from: Vec<String>,
    /// The day it was published, when the source says: `2026-11-18`.
    #[serde(default)]
    pub date: Option<String>,
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub query: String,
    /// The rung that answered.
    #[serde(default)]
    pub engine: String,
    pub hits: Vec<Hit>,
    /// The parts of the first result's page that match what was asked. A small model asked whether it
    /// needs another tool mostly says no, so the page it would have had to open is opened for it.
    #[serde(default)]
    pub first: Option<String>,
    /// The rungs tried before the one that answered, with why each gave nothing.
    pub notes: Vec<String>,
    /// Nothing was asked of anyone: the search was kept from before.
    #[serde(skip)]
    pub kept: bool,
}

impl Found {
    /// As a model is given it.
    pub fn told(&self) -> String {
        let from = if self.engine.is_empty() { String::new() } else { format!(", from {}", self.engine) };
        let mut out = if self.hits.is_empty() { format!("Nothing was found for \"{}\".", self.query) } else { format!("Results for \"{}\"{from}:\n", self.query) };
        for (n, hit) in self.hits.iter().enumerate() {
            out.push_str(&format!("\n{}. {}\n   {}\n", n + 1, hit.title, hit.address));
            if let Some(date) = &hit.date {
                out.push_str(&format!("   Published {date}\n"));
            }
            if !hit.about.is_empty() {
                out.push_str(&format!("   {}\n", hit.about));
            }
        }
        if let (Some(first), Some(hit)) = (&self.first, self.hits.first()) {
            out.push_str(&format!("\nFrom the first of them, {}:\n\n{first}\n", hit.title));
        }
        for note in &self.notes {
            out.push_str(&format!("\n({note})"));
        }
        out.trim_end().to_string()
    }
}

/// Which sources to ask: the person's settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Which {
    /// The address of the person's own SearXNG server, when they have one.
    pub searxng: Option<String>,
    pub exa: bool,
    pub duckduckgo: bool,
    pub wikipedia: bool,
    pub mwmbl: bool,
    /// The Wikipedia to ask: `en`, `it`, `de`…
    pub language: String,
}

impl Default for Which {
    fn default() -> Self {
        Self { searxng: None, exa: true, duckduckgo: true, wikipedia: true, mwmbl: true, language: "en".into() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rung {
    Searxng,
    Exa,
    DuckDuckGo,
    DuckDuckGoLite,
    WikipediaAndMwmbl,
}

impl Rung {
    fn name(self) -> &'static str {
        match self {
            Rung::Searxng => SEARXNG,
            Rung::Exa => EXA,
            Rung::DuckDuckGo => DUCKDUCKGO,
            Rung::DuckDuckGoLite => DUCKDUCKGO_LITE,
            Rung::WikipediaAndMwmbl => WIKIPEDIA_AND_MWMBL,
        }
    }
}

/// What a rung gave: its results, and what went wrong on the way without stopping it.
type Gave = Result<(Vec<Hit>, Vec<String>), String>;

fn short(text: &str, at_most: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= at_most { text } else { format!("{}…", text.chars().take(at_most).collect::<String>().trim_end()) }
}

/// A result, when it is one: a title, and an address on the public internet.
fn hit(title: &str, address: &str, about: &str, most_about: usize, from: &str, date: Option<String>, is_local: &dyn Fn(&Url) -> bool) -> Option<Hit> {
    let address = guard::address(address).ok().filter(|u| !is_local(u))?;
    let title = short(title, 120);
    (!title.is_empty()).then(|| Hit { title, address: address.to_string(), about: short(about, most_about), from: vec![from.into()], date })
}

/// Whether most of what was found mentions a word of what was asked. A results page read off a site may be
/// a decoy: a site that distrusts a program sometimes answers it with results for something else.
fn about(query: &str, hits: &[Hit]) -> bool {
    // Words too short to tell anything by say nothing either way; a word that is not Latin letters may be short.
    let words: Vec<String> = query.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3 || (!w.is_empty() && !w.is_ascii())).map(str::to_lowercase).collect();
    if words.is_empty() {
        return true;
    }
    let mentioning = hits.iter().filter(|h| words.iter().any(|w| format!("{} {} {}", h.title, h.about, h.address).to_lowercase().contains(w.as_str()))).count();
    mentioning * 2 >= hits.len()
}

// ----- reading what each source answers ---------------------------------------------------------------------

/// What Wikipedia's interface answered, as hits in its own order.
fn wikipedia_hits(answer: &Value) -> Vec<Hit> {
    let mut pages: Vec<&Value> = answer.pointer("/query/pages").and_then(Value::as_array).map(|p| p.iter().collect()).unwrap_or_default();
    pages.sort_by_key(|p| p.get("index").and_then(Value::as_u64).unwrap_or(u64::MAX));
    pages
        .into_iter()
        .filter_map(|p| {
            let text = |name: &str| p.get(name).and_then(Value::as_str).unwrap_or_default();
            (!text("title").is_empty() && !text("fullurl").is_empty()).then(|| Hit {
                title: format!("{} (Wikipedia)", text("title")),
                address: text("fullurl").to_string(),
                about: short(text("extract"), MOST_ABOUT),
                from: vec![WIKIPEDIA.into()],
                date: None,
            })
        })
        .collect()
}

/// What Mwmbl answered, as hits in its own order.
fn mwmbl_hits(answer: &Value, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let results = answer.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    let text = |r: &Value, name: &str| r.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    results.iter().filter_map(|r| hit(&text(r, "title"), &text(r, "url"), &text(r, "content"), MOST_ABOUT, MWMBL, None, is_local)).collect()
}

/// What a SearXNG server answered in JSON.
fn searxng_hits(answer: &Value, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let results = answer.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    let text = |r: &Value, name: &str| r.get(name).and_then(Value::as_str).unwrap_or_default().to_string();
    results
        .iter()
        .filter_map(|r| {
            let date = Some(text(r, "publishedDate").chars().take(10).collect::<String>()).filter(|d| d.len() == 10);
            hit(&text(r, "title"), &text(r, "url"), &text(r, "content"), MOST_ABOUT, SEARXNG, date, is_local)
        })
        .collect()
}

/// A SearXNG server's page of results for people: what it gives when its JSON is turned off.
fn searxng_page_hits(html: &str, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select("article.result")
        .iter()
        .filter_map(|result| {
            let link = result.select_single("h3 a");
            hit(&link.text(), &link.attr("href")?, &result.select_single(".content").text(), MOST_ABOUT, SEARXNG, None, is_local)
        })
        .collect()
}

/// The text a tool of an MCP service answered: from a JSON answer, or from a stream of events.
fn mcp_text(body: &str) -> Result<String, String> {
    let messages: Vec<Value> = if body.trim_start().starts_with('{') {
        serde_json::from_str(body).into_iter().collect()
    } else {
        body.lines().filter_map(|line| line.strip_prefix("data:")).filter_map(|data| serde_json::from_str(data.trim()).ok()).collect()
    };
    for message in messages {
        if let Some(error) = message.get("error") {
            return Err(format!("said: {}", short(error.get("message").and_then(Value::as_str).unwrap_or("it failed"), 200)));
        }
        let Some(result) = message.get("result") else { continue };
        let text = result.get("content").and_then(Value::as_array).map(|parts| parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        if result.get("isError") == Some(&Value::Bool(true)) {
            return Err(format!("said: {}", short(&text, 200)));
        }
        return Ok(text);
    }
    Err("answered nothing that can be read".into())
}

/// Exa's results. It answers in text: for each page a block of `Title:`, `URL:`, `Published:`, `Author:`
/// lines and then `Highlights:`, the passages of the page that match.
fn exa_hits(text: &str, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Title: ") || blocks.is_empty() {
            blocks.push(Vec::new());
        }
        blocks.last_mut().expect("just pushed").push(line);
    }
    blocks
        .iter()
        .filter_map(|block| {
            let field = |name: &str| block.iter().find_map(|line| line.strip_prefix(name)).map(str::trim).filter(|v| !v.is_empty() && *v != "N/A");
            let title = field("Title: ")?;
            let passages_from = block.iter().position(|line| line.starts_with("Highlights:") || line.starts_with("Text:")).map_or(block.len(), |at| at + 1);
            // Without the rules between results, the "..." between passages, the marks of headings, and the
            // title said again.
            let passages: Vec<&str> =
                block[passages_from..].iter().map(|line| line.trim().trim_start_matches('#').trim()).filter(|line| !line.is_empty() && *line != "..." && *line != "---" && *line != title).collect();
            let date = field("Published: ").map(|d| d.chars().take(10).collect::<String>()).filter(|d| d.len() == 10);
            hit(title, field("URL: ")?, &passages.join(" "), MOST_ABOUT_FROM_EXA, EXA, date, is_local)
        })
        .collect()
}

/// The real address behind a link of DuckDuckGo's: it sometimes leads through DuckDuckGo, with the address
/// inside. `None` for a link that is DuckDuckGo's own (an advertisement goes through it too).
fn duckduckgo_target(href: &str) -> Option<String> {
    let href = if href.starts_with("//") { format!("https:{href}") } else { href.to_string() };
    match Url::parse(&href) {
        Ok(url) if url.path() == "/l/" => url.query_pairs().find(|(name, _)| name == "uddg").map(|(_, address)| address.into_owned()),
        Ok(url) if url.host_str().is_some_and(|host| host == "duckduckgo.com" || host.ends_with(".duckduckgo.com")) => None,
        Ok(_) => Some(href),
        Err(_) => None,
    }
}

/// DuckDuckGo's page of results for browsers without scripts.
fn duckduckgo_hits(html: &str, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let doc = Document::from(html);
    doc.select(".result")
        .iter()
        // What is paid for is not a result.
        .filter(|result| !result.is(".result--ad"))
        .filter_map(|result| {
            let link = result.select_single("a.result__a");
            hit(&link.text(), &duckduckgo_target(&link.attr("href")?)?, &result.select_single(".result__snippet").text(), MOST_ABOUT, DUCKDUCKGO, None, is_local)
        })
        .collect()
}

/// DuckDuckGo's lighter page: a table, a row for each link and a row under it for what the page is about.
fn duckduckgo_lite_hits(html: &str, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let doc = Document::from(html);
    let mut hits: Vec<Hit> = Vec::new();
    // Whether the row of words that comes next belongs to the last link: not when that link was no result.
    let mut open = false;
    for cell in doc.select("a.result-link, td.result-snippet").iter() {
        if cell.is("a.result-link") {
            let found = cell.attr("href").and_then(|href| duckduckgo_target(&href)).and_then(|address| hit(&cell.text(), &address, "", MOST_ABOUT, DUCKDUCKGO_LITE, None, is_local));
            open = found.is_some();
            hits.extend(found);
        } else if open && let Some(last) = hits.last_mut() {
            last.about = short(&cell.text(), MOST_ABOUT);
            open = false;
        }
    }
    hits
}

/// Several lists of hits as one: a page found by more than one source, or high in a list, comes first.
/// (Reciprocal rank fusion: each list gives a page `weight / (60 + its place)`.)
fn merged(lists: Vec<(f64, Vec<Hit>)>) -> Vec<Hit> {
    let mut all: Vec<(f64, usize, Hit)> = Vec::new();
    for (weight, hits) in lists {
        for (place, hit) in hits.into_iter().enumerate() {
            let score = weight / (60.0 + place as f64 + 1.0);
            let same = guard::address(&hit.address).map(|u| guard::same_as(&u)).unwrap_or_else(|_| hit.address.clone());
            match all.iter_mut().find(|(_, _, h)| guard::address(&h.address).map(|u| guard::same_as(&u)).is_ok_and(|s| s == same)) {
                Some((total, _, known)) => {
                    *total += score;
                    known.from.extend(hit.from);
                    if known.about.is_empty() {
                        known.about = hit.about;
                    }
                }
                None => {
                    let order = all.len();
                    all.push((score, order, hit));
                }
            }
        }
    }
    all.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    all.into_iter().map(|(_, _, hit)| hit).collect()
}

/// A list without the same page twice, and no longer than a model is given.
fn once_each(hits: Vec<Hit>, at_most: usize) -> Vec<Hit> {
    let mut seen = std::collections::HashSet::new();
    hits.into_iter().filter(|h| seen.insert(guard::address(&h.address).map(|u| guard::same_as(&u)).unwrap_or_else(|_| h.address.clone()))).take(at_most).collect()
}

impl Web {
    /// What a source said when it asked to be left alone, while that still holds.
    fn left_alone(&self, source: &'static str) -> Option<String> {
        self.paused.lock().expect("not poisoned").get(source).filter(|(until, _)| *until > Instant::now()).map(|(_, said)| said.clone())
    }

    /// Leaves a source alone for a while, and gives back the words that say why.
    fn leave_alone(&self, source: &'static str, said: String) -> String {
        self.paused.lock().expect("not poisoned").insert(source, (Instant::now() + LEFT_ALONE_FOR, said.clone()));
        said
    }

    // ----- the rungs --------------------------------------------------------------------------------------

    /// The person's own SearXNG server: its JSON when the server allows that, its page for people when not.
    async fn ask_searxng(&self, base: &str, query: &str, stop: &AtomicBool) -> Result<Vec<Hit>, String> {
        let wrong = || "its address in the settings is not a web address".to_string();
        let base = guard::address(base.trim()).map_err(|_| wrong())?;
        let mut url = Url::parse(&format!("{}/search", base.as_str().trim_end_matches('/'))).map_err(|_| wrong())?;
        url.query_pairs_mut().append_pair("q", query).append_pair("format", "json");
        // The person named this server themselves, so it may be on this computer or their network.
        let got = self.send(Ask { url: &url, through: Through::Open, if_changed: None, gap: BETWEEN_TWO_OF_THEIR_OWN, sent: None, accept: "application/json" }, stop).await?;
        if got.status == 200
            && let Ok(answer) = serde_json::from_slice::<Value>(&got.body)
        {
            return Ok(searxng_hits(&answer, &|url| self.is_local(url)));
        }
        url.query_pairs_mut().clear().append_pair("q", query);
        let got = self.send(Ask { url: &url, through: Through::Open, if_changed: None, gap: BETWEEN_TWO_OF_THEIR_OWN, sent: None, accept: ACCEPT_A_PAGE }, stop).await?;
        if got.status != 200 {
            return Err(format!("answered {}", got.status));
        }
        Ok(searxng_page_hits(&String::from_utf8_lossy(&got.body), &|url| self.is_local(url)))
    }

    /// Exa, through the service it offers assistants (`https://mcp.exa.ai/mcp`): its search tool is called as
    /// any assistant would call it. Without a key it has a free limit, and answers only a direct IPv4
    /// connection.
    async fn ask_exa(&self, query: &str, stop: &AtomicBool) -> Result<Vec<Hit>, String> {
        if let Some(said) = self.left_alone(EXA) {
            return Err(said);
        }
        let url = Url::parse(&self.sources.exa).map_err(|_| "Exa's address is wrong".to_string())?;
        // What is sent is the search and nothing else. Its "objective" is the search said again: the person's
        // own words stay here.
        let call =
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "web_search_exa", "arguments": { "query": query, "numResults": MOST_FROM_EXA, "objective": query } } });
        let got = self
            .send(Ask { url: &url, through: Through::StrictV4, if_changed: None, gap: BETWEEN_TWO_EXA, sent: Some(Sent::Json(call.to_string())), accept: "application/json, text/event-stream" }, stop)
            .await?;
        match got.status {
            200 => Ok(exa_hits(&mcp_text(&String::from_utf8_lossy(&got.body))?, &|url| self.is_local(url))),
            429 => Err(self.leave_alone(EXA, "Exa says its free searches are used up for now (429). Demido leaves it alone for a quarter of an hour.".into())),
            403 => Err(self.leave_alone(EXA, "Exa refused (403): without a key it answers only a direct connection over IPv4. Demido leaves it alone for a quarter of an hour.".into())),
            other => Err(format!("answered {other}")),
        }
    }

    /// DuckDuckGo, asked as its own form asks. The page the form stands on is loaded first (and kept for a
    /// day), then the form is sent from that page: what a browser without scripts does.
    async fn ask_duckduckgo(&self, lite: bool, query: &str, stop: &AtomicBool) -> Result<Vec<Hit>, String> {
        if let Some(said) = self.left_alone(DUCKDUCKGO) {
            return Err(said);
        }
        let puzzle = |html: &str| html.contains("bots use DuckDuckGo too") || html.contains("anomaly-modal");
        let refused =
            || self.leave_alone(DUCKDUCKGO, "DuckDuckGo asked for proof that a person is searching. Demido does not get round that: it leaves DuckDuckGo alone for a quarter of an hour.".into());
        let form = Url::parse(&if lite { format!("{}/lite/", self.sources.duckduckgo_lite) } else { format!("{}/html/", self.sources.duckduckgo) })
            .map_err(|_| "DuckDuckGo's address is wrong".to_string())?;
        let from = match self.kept::<String>("form", form.as_str()) {
            Some((age, page)) if age < A_FORM_IS_FRESH => page,
            _ => {
                let got = self.get(&form, false, None, BETWEEN_TWO_DUCKDUCKGO, stop).await?;
                let html = String::from_utf8_lossy(&got.body);
                if puzzle(&html) {
                    return Err(refused());
                }
                // DuckDuckGo sends its empty search page with "202 Accepted" (seen on 2026-11-21): the form
                // is what counts here, not the number.
                if !html.contains("<form") {
                    return Err(format!("answered {} with a page that has no search form", got.status));
                }
                let page = got.address.to_string();
                self.keep("form", form.as_str(), &page);
                page
            }
        };
        let fields = if lite { vec![("q".to_string(), query.to_string())] } else { vec![("q".to_string(), query.to_string()), ("b".to_string(), String::new())] };
        let got = self.send(Ask { url: &form, through: Through::Strict, if_changed: None, gap: BETWEEN_TWO_DUCKDUCKGO, sent: Some(Sent::Form { fields, from }), accept: ACCEPT_A_PAGE }, stop).await?;
        let html = String::from_utf8_lossy(&got.body).into_owned();
        // Sent the form, a 202 is DuckDuckGo having had enough of a caller: the page holds no results.
        if got.status == 202 || puzzle(&html) {
            return Err(refused());
        }
        if got.status != 200 {
            return Err(format!("answered {}", got.status));
        }
        let stand_ins = self.stand_ins.clone();
        let hits = tokio::task::spawn_blocking(move || {
            let is_local = |url: &Url| guard::is_local(url) && !stand_ins.contains(&super::authority(url));
            if lite { duckduckgo_lite_hits(&html, &is_local) } else { duckduckgo_hits(&html, &is_local) }
        })
        .await
        .map_err(|_| "its answer could not be read".to_string())?;
        if !about(query, &hits) {
            return Err("answered with results for something else".into());
        }
        Ok(hits)
    }

    async fn ask_wikipedia(&self, query: &str, language: &str, stop: &AtomicBool) -> Result<Vec<Hit>, String> {
        let language: String = language.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(12).collect();
        let base = self.sources.wikipedia.replace("{lang}", if language.is_empty() { "en" } else { &language });
        let mut url = Url::parse(&format!("{base}/w/api.php")).map_err(|_| "Wikipedia's address is wrong".to_string())?;
        url.query_pairs_mut().extend_pairs([
            ("action", "query"),
            ("format", "json"),
            ("formatversion", "2"),
            ("generator", "search"),
            ("gsrsearch", query),
            ("gsrlimit", "4"),
            ("prop", "extracts|info"),
            ("exintro", "1"),
            ("explaintext", "1"),
            ("exchars", "320"),
            ("exlimit", "4"),
            ("inprop", "url"),
            ("redirects", "1"),
        ]);
        let got = self.get(&url, false, None, BETWEEN_TWO_WIKIPEDIA, stop).await?;
        if got.status != 200 {
            return Err(format!("Wikipedia answered {}", got.status));
        }
        let answer: Value = serde_json::from_slice(&got.body).map_err(|_| "Wikipedia's answer could not be read".to_string())?;
        Ok(wikipedia_hits(&answer))
    }

    async fn ask_mwmbl(&self, query: &str, stop: &AtomicBool) -> Result<Vec<Hit>, String> {
        if let Some(said) = self.left_alone(MWMBL) {
            return Err(said);
        }
        let mut url = Url::parse(&format!("{}/api/v2/search/", self.sources.mwmbl)).map_err(|_| "Mwmbl's address is wrong".to_string())?;
        // Mwmbl matches the words as they come: asked "how wide are a heron's wings" it finds pages that
        // start with "How wide" (tried). So it is asked with the words that say what the search is about.
        let words = page::meaning_words(query).join(" ");
        url.query_pairs_mut().append_pair("q", if words.is_empty() { query } else { &words });
        let got = self.get(&url, false, None, BETWEEN_TWO_MWMBL, stop).await?;
        match got.status {
            200 => Ok(mwmbl_hits(&serde_json::from_slice(&got.body).map_err(|_| "Mwmbl's answer could not be read".to_string())?, &|url| self.is_local(url))),
            // Its limit without a key: a thousand searches a month, one a second. Demido does not look for a way round.
            402 | 403 | 429 => Err(self.leave_alone(
                MWMBL,
                format!("Mwmbl says it has been asked too much ({}): without a key it allows a thousand searches a month. Demido leaves it alone for a quarter of an hour.", got.status),
            )),
            other => Err(format!("Mwmbl answered {other}")),
        }
    }

    /// The two that have an interface for programs, asked at once and merged: what both found comes first,
    /// then they take turns, Wikipedia leading (its first article is more often to the point than Mwmbl's
    /// first page, and the first page is the one that is read).
    async fn ask_wikipedia_and_mwmbl(&self, query: &str, which: &Which, stop: &AtomicBool) -> Gave {
        let (from_wikipedia, from_mwmbl) = tokio::join!(async { if which.wikipedia { Some(self.ask_wikipedia(query, &which.language, stop).await) } else { None } }, async {
            if which.mwmbl { Some(self.ask_mwmbl(query, stop).await) } else { None }
        },);
        let (mut lists, mut notes, mut answered) = (Vec::new(), Vec::new(), false);
        for (name, came) in [(WIKIPEDIA, from_wikipedia), (MWMBL, from_mwmbl)] {
            match came {
                Some(Ok(hits)) => {
                    answered = true;
                    lists.push((1.0, hits));
                }
                Some(Err(why)) => notes.push(if why.starts_with(name) { why } else { format!("{name} could not be asked: {why}.") }),
                None => {}
            }
        }
        if answered { Ok((merged(lists), notes)) } else { Err(notes.join(" ")) }
    }

    async fn ask(&self, rung: Rung, query: &str, which: &Which, stop: &AtomicBool) -> Gave {
        let alone = |hits: Vec<Hit>| (hits, Vec::new());
        match rung {
            Rung::Searxng => self.ask_searxng(which.searxng.as_deref().unwrap_or_default(), query, stop).await.map(alone),
            Rung::Exa => self.ask_exa(query, stop).await.map(alone),
            Rung::DuckDuckGo => self.ask_duckduckgo(false, query, stop).await.map(alone),
            Rung::DuckDuckGoLite => self.ask_duckduckgo(true, query, stop).await.map(alone),
            Rung::WikipediaAndMwmbl => self.ask_wikipedia_and_mwmbl(query, which, stop).await,
        }
    }

    /// Searches: climbs the ladder of sources that are turned on, and reads the first page that was found
    /// when the source gave only a line about it. `about` is what the person asked, in their words: the
    /// parts of that page are chosen by it and by the search. `Err` when nobody would answer.
    pub async fn search(&self, query: &str, about: &str, which: &Which, stop: &AtomicBool) -> Result<Found, String> {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.is_empty() {
            return Err("there is nothing to search for".into());
        }
        if query.chars().count() > MOST_OF_A_QUERY {
            return Err(format!("a search is a few words, {MOST_OF_A_QUERY} characters at most. This one has {}", query.chars().count()));
        }
        let searxng = which.searxng.as_deref().map(str::trim).filter(|address| !address.is_empty());
        let rungs: Vec<Rung> = [
            searxng.map(|_| Rung::Searxng),
            which.exa.then_some(Rung::Exa),
            which.duckduckgo.then_some(Rung::DuckDuckGo),
            which.duckduckgo.then_some(Rung::DuckDuckGoLite),
            (which.wikipedia || which.mwmbl).then_some(Rung::WikipediaAndMwmbl),
        ]
        .into_iter()
        .flatten()
        .collect();
        if rungs.is_empty() {
            return Err("every search source is turned off in the settings".into());
        }
        let key = format!("{}|{}|{}|{}|{}|{}|{}", query.to_lowercase(), searxng.unwrap_or_default(), which.exa, which.duckduckgo, which.wikipedia, which.mwmbl, which.language);
        if let Some((age, mut found)) = self.kept::<Found>("search", &key)
            && age < SEARCH_IS_FRESH
        {
            found.kept = true;
            return Ok(found);
        }
        let (mut notes, mut found_nothing, mut answer) = (Vec::new(), None, None);
        for rung in rungs {
            if stop.load(Ordering::Relaxed) {
                return Err("it was stopped".into());
            }
            let name = rung.name();
            match self.ask(rung, &query, which, stop).await {
                Ok((hits, on_the_way)) => {
                    notes.extend(on_the_way);
                    if hits.is_empty() {
                        // Believed only if no later rung has anything.
                        found_nothing.get_or_insert(name);
                        notes.push(format!("{name} found nothing."));
                    } else {
                        answer = Some((rung, hits));
                        break;
                    }
                }
                Err(why) if why == "it was stopped" => return Err(why),
                Err(why) => {
                    let note = if why.starts_with(name) || why.contains(". ") { why } else { format!("{name} {}.", why.trim_end_matches('.')) };
                    // DuckDuckGo's two pages are left alone together: said once.
                    if !notes.contains(&note) {
                        notes.push(note);
                    }
                }
            }
        }
        let Some((rung, hits)) = answer else {
            return match found_nothing {
                Some(name) => Ok(Found { query, engine: name.into(), hits: Vec::new(), first: None, notes, kept: false }),
                None => Err(format!("No search service would answer. {}", notes.join(" "))),
            };
        };
        let mut found = Found { query, engine: rung.name().into(), hits: once_each(hits, if rung == Rung::Exa { MOST_FROM_EXA } else { MOST_RESULTS }), first: None, notes, kept: false };
        // Exa answered with passages of the pages already. From the others there is a line about each page,
        // so the first page is read too, as far as it matches. When it cannot be read, the list is still there.
        if rung != Rung::Exa
            && let Some(hit) = found.hits.first()
            && let Ok((page, _)) = self.page(&hit.address, false, stop).await
        {
            let given = page::passages(&page.text, &format!("{} {about}", found.query), MOST_OF_THE_FIRST);
            found.first = (!given.text.trim().is_empty()).then_some(given.text);
        }
        // Kept without the story of who refused: that is news for one search, not for an hour.
        self.keep("search", &key, &Found { notes: Vec::new(), ..found.clone() });
        Ok(found)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn addresses(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.address.as_str()).collect()
    }

    /// What Mwmbl answers, cut down. The form of the real one was read on 2026-11-21.
    pub const MWMBL_ANSWER: &str = r#"{"query":"grey heron","number_of_results":101,"results":[
        {"url":"https://en.wikipedia.org/wiki/Grey_heron","title":"Grey heron","content":"The grey heron (Ardea cinerea) is a long-legged wading bird of the heron family.","engine":"wikipedia","score":1.0},
        {"url":"https://birds.example/herons?page=2","title":"Herons of the lagoon","content":"","engine":"mwmbl","score":0.5},
        {"url":"http://192.168.1.1/admin","title":"A router","content":"","engine":"mwmbl","score":0.4},
        {"url":"not an address","title":"Nothing","content":"","engine":"mwmbl","score":0.3},
        {"url":"https://untitled.example/","title":"","content":"","engine":"mwmbl","score":0.2}
    ],"images":[],"widgets":[],"monthly_usage":null,"monthly_limit":null}"#;

    /// What Exa's search tool answers, cut down from a real answer of 2026-11-21, as the stream of events
    /// its service sends. `{here}` is where a test wants the first page to be.
    pub const EXA_ANSWER: &str = "event: message\ndata: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Title: Grey heron\\nURL: {here}\\nPublished: 2026-11-18T19:39:24.000Z\\nAuthor: N/A\\nHighlights:\\n## Description\\n\\n...\\n\\nThe grey heron is a large bird, standing up to 100 cm (40 in) tall and measuring 84–102 cm (33–40 in) long with a 155–195 cm (61–77 in) wingspan. [10]\\n\\n---\\n\\nTitle: Heron | Waterway wildlife guide | Canal & River Trust\\nURL: https://canalrivertrust.org.uk/heron\\nPublished: N/A\\nAuthor: N/A\\nHighlights:\\n## Heron facts\\n\\nSize: 84-102cm in length, 155-175cm wingspan\\n\\nWeight: 1.5-2kg\\n\\n---\\n\\nTitle: A router\\nURL: http://192.168.1.1/admin\\nPublished: N/A\\nHighlights:\\nNot a page on the internet.\",\"_meta\":{\"searchTime\":217.9}}]},\"jsonrpc\":\"2.0\",\"id\":1}\n\n";

    /// DuckDuckGo's page for browsers without scripts, cut down from a real one of 2026-11-21, with an
    /// advertisement and a link through DuckDuckGo added as they were seen on other days.
    pub const DUCKDUCKGO_PAGE: &str = r#"<html><body><form action="/html/" method="post"><input name="q"></form><div class="serp__results"><div id="links" class="results">
        <div class="result results_links results_links_deep result--ad"><h2 class="result__title"><a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=shop.example">Buy herons cheap</a></h2><a class="result__snippet">An advertisement.</a></div>
        <div class="result results_links results_links_deep web-result "><div class="links_main links_deep result__body">
          <h2 class="result__title"><a rel="nofollow" class="result__a" href="{here}">How Long Do Grey Herons Live? - Global Animal Guide</a></h2>
          <a class="result__snippet" href="{here}"><b>Grey</b> <b>Heron</b> lifespan in the wild: records place most <b>grey</b> <b>herons</b> near 15-25 years.</a></div></div>
        <div class="result results_links results_links_deep web-result "><div class="links_main links_deep result__body">
          <h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fbirds.example%2Fherons%3Fpage%3D2&amp;rut=def">Herons of the lagoon</a></h2>
          <a class="result__snippet">Where to see herons, and when.</a></div></div>
        <div class="result"><h2><a class="result__a" href="//duckduckgo.com/l/?uddg=http%3A%2F%2F192.168.1.1%2Fadmin">A heron's router</a></h2></div>
        </div></div></body></html>"#;

    /// DuckDuckGo's lighter page, cut down from a real one of 2026-11-21.
    pub const DUCKDUCKGO_LITE_PAGE: &str = r#"<html><body><form action="/lite/" method="post"><input class='query' type="text" name="q" value="grey heron"></form><table>
        <tr><td valign="top">1.&nbsp;</td><td><a rel="nofollow" href="{here}" class='result-link'>Grey heron - Wikipedia</a></td></tr>
        <tr><td>&nbsp;&nbsp;&nbsp;</td><td class='result-snippet'>The <b>grey heron</b> is a long-legged wading bird of the heron family.</td></tr>
        <tr><td>&nbsp;&nbsp;&nbsp;</td><td><span class='link-text'>en.wikipedia.org</span></td></tr>
        <tr><td valign="top">2.&nbsp;</td><td><a rel="nofollow" href="https://duckduckgo.com/y.js?ad_domain=shop.example" class='result-link'>Buy herons cheap</a></td></tr>
        <tr><td>&nbsp;&nbsp;&nbsp;</td><td class='result-snippet'>An advertisement about a grey heron.</td></tr>
        <tr><td valign="top">3.&nbsp;</td><td><a rel="nofollow" href="https://birds.example/herons" class='result-link'>Herons of the lagoon</a></td></tr>
        </table></body></html>"#;

    #[test]
    fn mwmbls_answer_is_read_and_what_is_not_a_page_on_the_internet_is_left_out() {
        let hits = mwmbl_hits(&serde_json::from_str(MWMBL_ANSWER).unwrap(), &guard::is_local);
        assert_eq!(hits.len(), 2, "the address on a private network, the one that is none and the one without a title are not results: {hits:?}");
        assert_eq!((hits[0].title.as_str(), hits[0].address.as_str()), ("Grey heron", "https://en.wikipedia.org/wiki/Grey_heron"));
        assert_eq!(hits[0].about, "The grey heron (Ardea cinerea) is a long-legged wading bird of the heron family.");
        assert_eq!(hits[1].address, "https://birds.example/herons?page=2");
        assert!(mwmbl_hits(&json!({ "results": [] }), &guard::is_local).is_empty() && mwmbl_hits(&json!({}), &guard::is_local).is_empty());
    }

    #[test]
    fn wikipedias_answer_is_read_in_its_own_order() {
        let answer = json!({ "query": { "pages": [
            { "pageid": 2, "title": "Heron", "index": 2, "extract": "Herons are long-legged birds.", "fullurl": "https://en.wikipedia.org/wiki/Heron" },
            { "pageid": 1, "title": "Grey heron", "index": 1, "extract": "The grey heron (Ardea cinerea) is a wading bird.", "fullurl": "https://en.wikipedia.org/wiki/Grey_heron" },
        ] } });
        let hits = wikipedia_hits(&answer);
        assert_eq!(hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Grey heron (Wikipedia)", "Heron (Wikipedia)"]);
        assert!(wikipedia_hits(&json!({ "batchcomplete": true })).is_empty(), "nothing found is not an error");
    }

    #[test]
    fn exas_answer_is_read_from_its_stream_with_the_passages_and_the_day() {
        let text = mcp_text(&EXA_ANSWER.replace("{here}", "https://en.wikipedia.org/wiki/Grey_heron")).unwrap();
        let hits = exa_hits(&text, &guard::is_local);
        assert_eq!(addresses(&hits), ["https://en.wikipedia.org/wiki/Grey_heron", "https://canalrivertrust.org.uk/heron"], "the address on a private network is not a result");
        assert_eq!(hits[0].date.as_deref(), Some("2026-11-18"));
        assert_eq!(hits[0].about, "Description The grey heron is a large bird, standing up to 100 cm (40 in) tall and measuring 84–102 cm (33–40 in) long with a 155–195 cm (61–77 in) wingspan. [10]");
        assert_eq!((hits[1].date.as_deref(), hits[1].about.as_str()), (None, "Heron facts Size: 84-102cm in length, 155-175cm wingspan Weight: 1.5-2kg"));

        // The same service answering in plain JSON, saying no, and saying nothing.
        assert_eq!(mcp_text(r#"{"result":{"content":[{"type":"text","text":"hello"}]},"jsonrpc":"2.0","id":1}"#).unwrap(), "hello");
        assert!(mcp_text("data: {\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"Search failed: upstream\"}],\"isError\":true}}\n").unwrap_err().contains("Search failed"));
        assert!(mcp_text(r#"{"error":{"message":"rate limited"}}"#).unwrap_err().contains("rate limited"));
        assert!(mcp_text("<html>not it</html>").is_err());
    }

    #[test]
    fn duckduckgos_two_pages_are_read_and_what_is_paid_for_is_left_out() {
        let hits = duckduckgo_hits(&DUCKDUCKGO_PAGE.replace("{here}", "https://globalanimalguide.org/herons/"), &guard::is_local);
        assert_eq!(addresses(&hits), ["https://globalanimalguide.org/herons/", "https://birds.example/herons?page=2"], "the advertisement and the address on a private network are not results");
        assert_eq!(hits[0].title, "How Long Do Grey Herons Live? - Global Animal Guide");
        assert_eq!(hits[0].about, "Grey Heron lifespan in the wild: records place most grey herons near 15-25 years.");
        assert!(duckduckgo_hits("<html><body><div class=\"no-results\">No results.</div></body></html>", &guard::is_local).is_empty());

        let hits = duckduckgo_lite_hits(&DUCKDUCKGO_LITE_PAGE.replace("{here}", "https://en.wikipedia.org/wiki/Grey_heron"), &guard::is_local);
        assert_eq!(addresses(&hits), ["https://en.wikipedia.org/wiki/Grey_heron", "https://birds.example/herons"]);
        assert_eq!(hits[0].about, "The grey heron is a long-legged wading bird of the heron family.");
        assert_eq!(hits[1].about, "", "the words under an advertisement are not given to the result after it");
    }

    #[test]
    fn a_searxng_server_is_read_in_json_and_as_its_page() {
        let answer = json!({ "results": [
            { "title": "Grey heron", "url": "https://en.wikipedia.org/wiki/Grey_heron", "content": "A wading bird.", "publishedDate": "2026-11-18T19:39:24" },
            { "title": "No address" },
        ] });
        let hits = searxng_hits(&answer, &guard::is_local);
        assert_eq!((addresses(&hits), hits[0].date.as_deref()), (vec!["https://en.wikipedia.org/wiki/Grey_heron"], Some("2026-11-18")));
        let page = r#"<html><body><article class="result"><h3><a href="https://birds.example/">Birds</a></h3><p class="content">All about birds.</p></article><article class="result"><h3>No link</h3></article></body></html>"#;
        let hits = searxng_page_hits(page, &guard::is_local);
        assert_eq!((addresses(&hits), hits[0].about.as_str()), (vec!["https://birds.example/"], "All about birds."));
    }

    #[test]
    fn results_for_something_else_are_told_apart() {
        let h = |title: &str, address: &str| Hit { title: title.into(), address: address.into(), ..Hit::default() };
        let herons = [h("Grey heron - Wikipedia", "https://en.wikipedia.org/wiki/Grey_heron"), h("Birds of the marsh", "https://birds.example/heron"), h("Something else", "https://example.com/")];
        assert!(about("grey heron lifespan", &herons));
        let decoys = [h("Store finder", "https://shop.example/"), h("Cheap flights", "https://fly.example/"), h("Weather today", "https://weather.example/")];
        assert!(!about("grey heron lifespan", &decoys));
        assert!(about("C++ vs Go", &decoys), "words too short to tell anything by say nothing either way");
        assert!(about("鷺", &[h("鷺 - Wikipedia", "https://ja.wikipedia.org/wiki/%E3%82%B5%E3%82%AE")]), "a word that is not Latin letters may be short");
        assert!(about("grey heron", &[]));
    }

    #[test]
    fn what_two_sources_both_found_comes_first() {
        let hit = |title: &str, address: &str, from: &str| Hit { title: title.into(), address: address.into(), from: vec![from.into()], ..Hit::default() };
        let all = merged(vec![
            (1.0, vec![hit("Birds", "https://birds.example/", MWMBL), hit("Grey heron", "https://en.wikipedia.org/wiki/Grey_heron", MWMBL)]),
            (1.0, vec![hit("Grey heron (Wikipedia)", "https://en.wikipedia.org/wiki/Grey_heron/", WIKIPEDIA), hit("Heron (Wikipedia)", "https://en.wikipedia.org/wiki/Heron", WIKIPEDIA)]),
        ]);
        assert_eq!(addresses(&all), ["https://en.wikipedia.org/wiki/Grey_heron", "https://birds.example/", "https://en.wikipedia.org/wiki/Heron"]);
        assert_eq!(all[0].from, [MWMBL, WIKIPEDIA]);
        let twice = vec![hit("A", "https://a.example/", EXA), hit("A again", "http://www.a.example", EXA), hit("B", "https://b.example/", EXA)];
        assert_eq!(addresses(&once_each(twice, 5)), ["https://a.example/", "https://b.example/"]);
    }

    #[test]
    fn a_model_is_given_results_as_a_short_numbered_list() {
        let found = Found {
            query: "grey heron".into(),
            engine: "Exa".into(),
            hits: vec![Hit { title: "Grey heron".into(), address: "https://en.wikipedia.org/wiki/Grey_heron".into(), about: "A wading bird.".into(), from: vec![], date: Some("2026-11-18".into()) }],
            first: Some("It lives for about 5 years.".into()),
            notes: vec!["Your SearXNG could not be reached.".into()],
            kept: false,
        };
        assert_eq!(
            found.told(),
            "Results for \"grey heron\", from Exa:\n\n1. Grey heron\n   https://en.wikipedia.org/wiki/Grey_heron\n   Published 2026-11-18\n   A wading bird.\n\nFrom the first of them, Grey heron:\n\nIt lives for about 5 years.\n\n(Your SearXNG could not be reached.)"
        );
        assert_eq!(Found { query: "zzz".into(), ..Found::default() }.told(), "Nothing was found for \"zzz\".");
    }
}
