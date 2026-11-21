//! Searching the internet, with the two sources that need no key and that allow programs.
//!
//! - **Wikipedia**, through the interface it offers programs. One request gives the pages that match and
//!   the opening lines of each.
//! - **Mwmbl**, an independent, non-profit search engine with its own index of the web. Without a key it
//!   allows a thousand searches a month from one connection, one a second (research note 15). Its results
//!   are published under CC BY-NC-SA 4.0.
//!
//! DuckDuckGo is not one of them. Its page for browsers without scripts answered `curl`, and answered
//! Demido's own client, every time, with a puzzle meant for a person (tried on 2026-11-21). Getting past
//! that would mean making Demido look like another program, and Demido does not do that.
//!
//! Both are asked at once, and what they found is merged: a page that both found comes first. The words of
//! a search are the one thing here that a model writes and that leaves the computer, so they are kept
//! short, and they go to these two and nowhere else.

use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

use super::{SEARCH_IS_FRESH, Web, guard, page};

/// A search is a few words. Longer than this is not a search but a text being sent somewhere.
pub const MOST_OF_A_QUERY: usize = 200;
/// How many results a model is given.
const MOST_RESULTS: usize = 6;
/// How much of what a result is about.
const MOST_ABOUT: usize = 260;
/// How much of the first result's own page is given with the results.
const MOST_OF_THE_FIRST: usize = 2_500;
/// Mwmbl allows one search a second without a key. A little slower than that, to be sure.
const BETWEEN_TWO_MWMBL: Duration = Duration::from_millis(1200);
const BETWEEN_TWO_WIKIPEDIA: Duration = Duration::from_secs(1);
/// How long a source is left alone after it said it is being asked too much.
const LEFT_ALONE_FOR: Duration = Duration::from_secs(15 * 60);

const WIKIPEDIA: &str = "Wikipedia";
const MWMBL: &str = "Mwmbl";

/// One page that a search found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hit {
    pub title: String,
    pub address: String,
    /// A line or two of what it says.
    pub about: String,
    /// Which sources found it.
    pub from: Vec<String>,
}

/// What a search found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Found {
    pub query: String,
    pub hits: Vec<Hit>,
    /// The parts of the first result's page that match what was asked. A small model asked whether it
    /// needs another tool mostly says no, so the page it would have had to open is opened for it.
    #[serde(default)]
    pub first: Option<String>,
    /// What went wrong with a source, in words for the person.
    pub notes: Vec<String>,
    /// Nothing was asked of anyone: the search was kept from before.
    #[serde(skip)]
    pub kept: bool,
}

impl Found {
    /// As a model is given it.
    pub fn told(&self) -> String {
        let mut out = if self.hits.is_empty() { format!("Nothing was found for \"{}\".", self.query) } else { format!("Results for \"{}\":\n", self.query) };
        for (n, hit) in self.hits.iter().enumerate() {
            out.push_str(&format!("\n{}. {}\n   {}\n", n + 1, hit.title, hit.address));
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

/// Which sources to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct Which {
    pub wikipedia: bool,
    pub mwmbl: bool,
    /// The Wikipedia to ask: `en`, `it`, `de`…
    pub language: String,
}

impl Default for Which {
    fn default() -> Self {
        Self { wikipedia: true, mwmbl: true, language: "en".into() }
    }
}

fn short(text: &str, at_most: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= at_most { text } else { format!("{}…", text.chars().take(at_most).collect::<String>().trim_end()) }
}

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
            })
        })
        .collect()
}

/// What Mwmbl answered, as hits in its own order. `is_local` says whether an address is on this computer or
/// its network.
fn mwmbl_hits(answer: &Value, is_local: &dyn Fn(&Url) -> bool) -> Vec<Hit> {
    let results = answer.get("results").and_then(Value::as_array).cloned().unwrap_or_default();
    results
        .iter()
        .filter_map(|r| {
            let text = |name: &str| r.get(name).and_then(Value::as_str).unwrap_or_default();
            // An address that is not one, or that leads into a private network, is not a result.
            let address = guard::address(text("url")).ok().filter(|u| !is_local(u))?;
            (!text("title").is_empty()).then(|| Hit { title: short(text("title"), 120), address: address.to_string(), about: short(text("content"), MOST_ABOUT), from: vec![MWMBL.into()] })
        })
        .collect()
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
    all.into_iter().take(MOST_RESULTS).map(|(_, _, hit)| hit).collect()
}

impl Web {
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
        if let Some((until, said)) = self.paused.lock().expect("not poisoned").get(MWMBL).cloned()
            && until > Instant::now()
        {
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
            402 | 403 | 429 => {
                let said = format!("Mwmbl says it has been asked too much ({}): without a key it allows a thousand searches a month. Demido leaves it alone for a quarter of an hour.", got.status);
                self.paused.lock().expect("not poisoned").insert(MWMBL, (Instant::now() + LEFT_ALONE_FOR, said.clone()));
                Err(said)
            }
            other => Err(format!("Mwmbl answered {other}")),
        }
    }

    /// Searches with the sources that are turned on, and reads the first page that was found. `about` is
    /// what the person asked, in their words: the parts of that page are chosen by it and by the search.
    /// `Err` only when the search could not be made at all.
    pub async fn search(&self, query: &str, about: &str, which: &Which, stop: &AtomicBool) -> Result<Found, String> {
        let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if query.is_empty() {
            return Err("there is nothing to search for".into());
        }
        if query.chars().count() > MOST_OF_A_QUERY {
            return Err(format!("a search is a few words, {MOST_OF_A_QUERY} characters at most. This one has {}", query.chars().count()));
        }
        if !which.wikipedia && !which.mwmbl {
            return Err("every search source is turned off in the settings".into());
        }
        let key = format!("{}|{}|{}|{}", query.to_lowercase(), which.wikipedia, which.mwmbl, which.language);
        if let Some((age, mut found)) = self.kept::<Found>("search", &key)
            && age < SEARCH_IS_FRESH
        {
            found.kept = true;
            return Ok(found);
        }
        let (from_wikipedia, from_mwmbl) = tokio::join!(async { if which.wikipedia { Some(self.ask_wikipedia(&query, &which.language, stop).await) } else { None } }, async {
            if which.mwmbl { Some(self.ask_mwmbl(&query, stop).await) } else { None }
        },);
        let (mut lists, mut notes, mut answered) = (Vec::new(), Vec::new(), false);
        // They take turns: Wikipedia's first, Mwmbl's first, Wikipedia's second… A page both found comes
        // before all of those. Wikipedia leads because its first article is more often to the point than
        // Mwmbl's first page (tried: asked for a heron's lifespan, Mwmbl's first was somebody's blog), and
        // the first page is the one that is read.
        for (weight, name, came) in [(1.0, WIKIPEDIA, from_wikipedia), (1.0, MWMBL, from_mwmbl)] {
            match came {
                Some(Ok(hits)) => {
                    answered = true;
                    lists.push((weight, hits));
                }
                Some(Err(why)) => notes.push(if why.starts_with(name) { why } else { format!("{name} could not be asked: {why}.") }),
                None => {}
            }
        }
        if !answered {
            return Err(notes.join(" "));
        }
        let mut found = Found { query, hits: merged(lists), first: None, notes, kept: false };
        // The first page found is read too, as far as it matches. When it cannot be read, the list is still there.
        if let Some(hit) = found.hits.first()
            && let Ok((page, _)) = self.page(&hit.address, false, stop).await
        {
            let given = page::passages(&page.text, &format!("{} {about}", found.query), MOST_OF_THE_FIRST);
            found.first = (!given.text.trim().is_empty()).then_some(given.text);
        }
        // What is kept is a search that was answered in full: one with a source missing is asked again.
        if found.notes.is_empty() {
            self.keep("search", &key, &found);
        }
        Ok(found)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// What Mwmbl answers, cut down. The form of the real one was read on 2026-11-21.
    pub const RESULTS: &str = r#"{"query":"grey heron","number_of_results":101,"results":[
        {"url":"https://en.wikipedia.org/wiki/Grey_heron","title":"Grey heron","content":"The grey heron (Ardea cinerea) is a long-legged wading bird of the heron family.","engine":"wikipedia","score":1.0},
        {"url":"https://birds.example/herons?page=2","title":"Herons of the lagoon","content":"","engine":"mwmbl","score":0.5},
        {"url":"http://192.168.1.1/admin","title":"A router","content":"","engine":"mwmbl","score":0.4},
        {"url":"not an address","title":"Nothing","content":"","engine":"mwmbl","score":0.3},
        {"url":"https://untitled.example/","title":"","content":"","engine":"mwmbl","score":0.2}
    ],"images":[],"widgets":[],"monthly_usage":null,"monthly_limit":null}"#;

    #[test]
    fn mwmbls_answer_is_read_and_what_is_not_a_page_on_the_internet_is_left_out() {
        let hits = mwmbl_hits(&serde_json::from_str(RESULTS).unwrap(), &guard::is_local);
        assert_eq!(hits.len(), 2, "the address on a private network, the one that is none and the one without a title are not results: {hits:?}");
        assert_eq!((hits[0].title.as_str(), hits[0].address.as_str()), ("Grey heron", "https://en.wikipedia.org/wiki/Grey_heron"));
        assert_eq!(hits[0].about, "The grey heron (Ardea cinerea) is a long-legged wading bird of the heron family.");
        assert_eq!(hits[1].address, "https://birds.example/herons?page=2");
        assert!(mwmbl_hits(&serde_json::json!({ "results": [] }), &guard::is_local).is_empty() && mwmbl_hits(&serde_json::json!({}), &guard::is_local).is_empty());
    }

    #[test]
    fn wikipedias_answer_is_read_in_its_own_order() {
        let answer = serde_json::json!({ "query": { "pages": [
            { "pageid": 2, "title": "Heron", "index": 2, "extract": "Herons are long-legged birds.", "fullurl": "https://en.wikipedia.org/wiki/Heron" },
            { "pageid": 1, "title": "Grey heron", "index": 1, "extract": "The grey heron (Ardea cinerea) is a wading bird.", "fullurl": "https://en.wikipedia.org/wiki/Grey_heron" },
        ] } });
        let hits = wikipedia_hits(&answer);
        assert_eq!(hits.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Grey heron (Wikipedia)", "Heron (Wikipedia)"]);
        assert!(wikipedia_hits(&serde_json::json!({ "batchcomplete": true })).is_empty(), "nothing found is not an error");
    }

    #[test]
    fn what_two_sources_both_found_comes_first() {
        let hit = |title: &str, address: &str, from: &str| Hit { title: title.into(), address: address.into(), about: String::new(), from: vec![from.into()] };
        let all = merged(vec![
            (1.0, vec![hit("Birds", "https://birds.example/", MWMBL), hit("Grey heron", "https://en.wikipedia.org/wiki/Grey_heron", MWMBL)]),
            (0.8, vec![hit("Grey heron (Wikipedia)", "https://en.wikipedia.org/wiki/Grey_heron/", WIKIPEDIA), hit("Heron (Wikipedia)", "https://en.wikipedia.org/wiki/Heron", WIKIPEDIA)]),
        ]);
        assert_eq!(all.iter().map(|h| h.address.as_str()).collect::<Vec<_>>(), ["https://en.wikipedia.org/wiki/Grey_heron", "https://birds.example/", "https://en.wikipedia.org/wiki/Heron"]);
        assert_eq!(all[0].from, [MWMBL, WIKIPEDIA]);
    }

    #[test]
    fn a_model_is_given_results_as_a_short_numbered_list() {
        let found = Found {
            query: "grey heron".into(),
            hits: vec![Hit { title: "Grey heron".into(), address: "https://en.wikipedia.org/wiki/Grey_heron".into(), about: "A wading bird.".into(), from: vec![] }],
            first: Some("It lives for about 5 years.".into()),
            notes: vec!["Mwmbl could not be asked: the site did not answer in time.".into()],
            kept: false,
        };
        assert_eq!(
            found.told(),
            "Results for \"grey heron\":\n\n1. Grey heron\n   https://en.wikipedia.org/wiki/Grey_heron\n   A wading bird.\n\nFrom the first of them, Grey heron:\n\nIt lives for about 5 years.\n\n(Mwmbl could not be asked: the site did not answer in time.)"
        );
        assert_eq!(Found { query: "zzz".into(), ..Found::default() }.told(), "Nothing was found for \"zzz\".");
    }
}
