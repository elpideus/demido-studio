//! Reading and searching, against a stand-in site on this computer. Nothing here touches the internet.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::*;

const ARTICLE: &str = r#"<!doctype html><html><head><title>Heron counts rise on the lagoon</title><script>var x = 1;</script></head><body>
    <nav><a href="/">Home</a></nav><article><h1>Heron counts rise on the lagoon</h1>
    <p>Volunteers counted four hundred and twelve grey herons on the northern lagoon this spring, the highest number since counting began in 1988. The count is made on one morning in April, from six boats that cross the lagoon side by side.</p>
    <p>Wardens put the rise down to three mild winters in a row and to the reed beds that were replanted after the flood of 2019. Reed beds give herons somewhere to nest that foxes cannot reach.</p>
    <p>The next count will be made in April of next year. Anyone who can sit still in a boat for three hours is welcome, the wardens say.</p></article>
    <footer>Copyright The Marsh Gazette</footer></body></html>"#;

/// What the stand-in remembers of what it was asked.
#[derive(Default)]
struct Seen {
    /// How often each path was asked for.
    asked: HashMap<String, usize>,
    /// The headers of the last request.
    last: HashMap<String, String>,
    /// Mwmbl's stand-in says it has been asked too much.
    too_much: bool,
    /// Exa's stand-in says its free searches are used up.
    exa_used_up: bool,
    /// DuckDuckGo's stand-in answers with a puzzle for a person.
    wants_a_person: bool,
    /// DuckDuckGo's stand-in answers with results for something else.
    decoy: bool,
    /// DuckDuckGo's stand-in finds nothing.
    nothing: bool,
    /// The SearXNG stand-in has its JSON turned off.
    no_json: bool,
    /// What the last form or call that was posted held, and the headers it came with.
    posted: String,
    posted_with: HashMap<String, String>,
}

type Shared = Arc<Mutex<Seen>>;

async fn site(State(seen): State<Shared>, headers: HeaderMap, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let asked_for = request.uri().query().unwrap_or_default().to_string();
    let posting = request.method() == axum::http::Method::POST;
    let posted = String::from_utf8_lossy(&axum::body::to_bytes(request.into_body(), 100_000).await.unwrap_or_default()).into_owned();
    let (too_much, exa_used_up, wants_a_person, decoy, nothing, no_json) = {
        let mut seen = seen.lock().unwrap();
        // A form's page being loaded and the form being sent are counted apart.
        *seen.asked.entry(if posting { format!("POST {path}") } else { path.clone() }).or_default() += 1;
        seen.last = headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string())).collect();
        if posting {
            seen.posted = posted;
            seen.posted_with = seen.last.clone();
        }
        (seen.too_much, seen.exa_used_up, seen.wants_a_person, seen.decoy, seen.nothing, seen.no_json)
    };
    let puzzle = || {
        (
            StatusCode::ACCEPTED,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<html><body><div class=\"anomaly-modal\">Unfortunately, bots use DuckDuckGo too. Select all squares containing a duck.</div></body></html>",
        )
            .into_response()
    };
    let html = |body: &str| ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body.to_string()).into_response();
    let here = format!("http://{}", headers.get("host").and_then(|h| h.to_str().ok()).unwrap_or_default());
    match path.as_str() {
        "/robots.txt" => ([(header::CONTENT_TYPE, "text/plain")], "User-agent: *\nDisallow: /private/\n").into_response(),
        "/article" => {
            if headers.get("if-none-match").is_some_and(|v| v == "\"v1\"") {
                return StatusCode::NOT_MODIFIED.into_response();
            }
            ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::ETAG, "\"v1\"")], ARTICLE).into_response()
        }
        "/private/diary" => html("<html><body><p>Not for programs.</p></body></html>"),
        "/to-article" => (StatusCode::FOUND, [(header::LOCATION, "/article")]).into_response(),
        // Another door on this computer: not the stand-in, so not allowed.
        "/to-inside" => (StatusCode::FOUND, [(header::LOCATION, "http://127.0.0.1:1/keys")]).into_response(),
        "/to-keys" => (StatusCode::FOUND, [(header::LOCATION, "http://169.254.169.254/latest/meta-data/")]).into_response(),
        "/big" => ([(header::CONTENT_TYPE, "text/plain")], "0123456789\n".repeat(300_000)).into_response(),
        "/picture" => ([(header::CONTENT_TYPE, "image/png")], vec![0x89u8, b'P', b'N', b'G']).into_response(),
        "/paper" => ([(header::CONTENT_TYPE, "application/pdf")], "%PDF-1.7 ...").into_response(),
        "/latin" => ([(header::CONTENT_TYPE, "text/plain; charset=iso-8859-1")], vec![b'c', b'a', b'f', 0xE9]).into_response(),
        "/app" => html("<html><head><script src=a.js></script><script src=b.js></script><script>boot()</script></head><body><div id=root></div></body></html>"),
        "/slow" => {
            tokio::time::sleep(Duration::from_secs(20)).await;
            html("<html><body>late</body></html>")
        }
        // Exa's service for assistants: a call of its search tool, answered as a stream of events.
        "/mcp" if exa_used_up => StatusCode::TOO_MANY_REQUESTS.into_response(),
        "/mcp" if posting => ([(header::CONTENT_TYPE, "text/event-stream")], search::tests::EXA_ANSWER.replace("{here}", &format!("{here}/article"))).into_response(),
        // DuckDuckGo's two pages: loaded, they are the form; sent the form, they are the results.
        "/html/" | "/lite/" if wants_a_person => puzzle(),
        // Loaded, the real one answers "202 Accepted" with its form (seen on 2026-11-21).
        "/html/" | "/lite/" if !posting => (StatusCode::ACCEPTED, html("<html><body><form method=\"post\"><input name=\"q\"></form></body></html>")).into_response(),
        "/html/" | "/lite/" if nothing => html("<html><body><div class=\"no-results\">No results.</div></body></html>"),
        "/html/" if decoy => html(
            "<html><body><div class=\"result\"><a class=\"result__a\" href=\"https://shop.example/\">Store finder</a></div><div class=\"result\"><a class=\"result__a\" href=\"https://fly.example/\">Cheap flights</a></div></body></html>",
        ),
        "/html/" => html(&search::tests::DUCKDUCKGO_PAGE.replace("{here}", &format!("{here}/article"))),
        "/lite/" => html(&search::tests::DUCKDUCKGO_LITE_PAGE.replace("{here}", &format!("{here}/article"))),
        // A SearXNG server of the person's own.
        "/search" if asked_for.contains("format=json") && no_json => StatusCode::FORBIDDEN.into_response(),
        "/search" if asked_for.contains("format=json") => (
            [(header::CONTENT_TYPE, "application/json")],
            format!(r#"{{"results":[{{"title":"Grey heron","url":"{here}/article","content":"From your own server.","publishedDate":"2026-11-18T19:39:24"}}]}}"#),
        )
            .into_response(),
        "/search" => {
            html(&format!("<html><body><article class=\"result\"><h3><a href=\"{here}/article\">Grey heron</a></h3><p class=\"content\">From your own server's page.</p></article></body></html>"))
        }
        // Both search sources find pages on the stand-in itself, so that reading what was found stays here.
        "/w/api.php" => {
            let pages = format!(
                r#"{{"query":{{"pages":[{{"pageid":1,"title":"Grey heron","index":1,"extract":"The grey heron (Ardea cinerea) is a long-legged wading bird.","fullurl":"{here}/article"}},{{"pageid":2,"title":"Heron","index":2,"extract":"Herons are long-legged, long-necked birds.","fullurl":"{here}/wiki/Heron"}}]}}}}"#
            );
            ([(header::CONTENT_TYPE, "application/json")], pages).into_response()
        }
        "/api/v2/search/" if too_much => StatusCode::TOO_MANY_REQUESTS.into_response(),
        "/api/v2/search/" => {
            let results = format!(
                r#"{{"results":[{{"url":"{here}/article","title":"Grey heron","content":"The grey heron (Ardea cinerea) is a long-legged wading bird of the heron family."}},{{"url":"{here}/herons?page=2","title":"Herons of the lagoon","content":""}},{{"url":"http://192.168.1.1/admin","title":"A router","content":""}}]}}"#
            );
            ([(header::CONTENT_TYPE, "application/json")], results).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) struct World {
    pub web: Web,
    pub base: String,
    seen: Shared,
    pub kept_in: std::path::PathBuf,
}

/// Only the last rung: the two sources that have an interface for programs.
fn the_two() -> Which {
    Which { exa: false, duckduckgo: false, ..Which::default() }
}

impl World {
    pub fn asked(&self, path: &str) -> usize {
        self.seen.lock().unwrap().asked.get(path).copied().unwrap_or(0)
    }
    pub fn at(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
    /// Makes everything that is kept look older.
    fn age_what_is_kept(&self, by: Duration) {
        for entry in std::fs::read_dir(&self.kept_in).unwrap().filter_map(|e| e.ok()) {
            let file = std::fs::File::options().write(true).open(entry.path()).unwrap();
            file.set_modified(SystemTime::now() - by).unwrap();
        }
    }
}

pub(crate) async fn world() -> World {
    let seen = Shared::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let door = listener.local_addr().unwrap();
    let app = Router::new().fallback(site).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let kept_in = std::env::temp_dir().join(format!("demido-web-{}", uuid::Uuid::new_v4()));
    let base = format!("http://{door}");
    // The stand-in, and only it, counts as the public internet.
    let web = Web::with(
        Some(kept_in.clone()),
        Sources { wikipedia: base.clone(), mwmbl: base.clone(), exa: format!("{base}/mcp"), duckduckgo: base.clone(), duckduckgo_lite: base.clone() },
        vec![door.to_string()],
    );
    World { web, base, seen, kept_in }
}

static GO_ON: AtomicBool = AtomicBool::new(false);

#[tokio::test]
async fn a_page_is_read_as_text_under_demidos_own_name_and_kept() {
    let w = world().await;
    let read = w.web.read(&w.at("/article#top"), "how many herons were counted?", false, &GO_ON).await.unwrap();
    assert_eq!(read.title, "Heron counts rise on the lagoon");
    assert!(read.gave.starts_with(&format!("Page: Heron counts rise on the lagoon\nAddress: {}\n\n", w.at("/article"))), "{}", read.gave);
    assert!(read.gave.contains("four hundred and twelve grey herons") && !read.gave.contains("var x") && !read.gave.contains("Copyright"), "{}", read.gave);
    assert!(!read.kept);

    // What the site saw: who is asking, and nothing about the person.
    let last = w.seen.lock().unwrap().last.clone();
    assert!(last["user-agent"].starts_with("DemidoStudio/") && last["user-agent"].contains("github.com/elpideus/demido-studio"), "{last:?}");
    for never in ["cookie", "referer", "authorization", "accept-language"] {
        assert!(!last.contains_key(never), "{never} is not sent: {last:?}");
    }

    // Asked again: the site is not.
    let again = w.web.read(&w.at("/article"), "something else", false, &GO_ON).await.unwrap();
    assert!(again.kept && again.gave.contains("four hundred and twelve"));
    assert_eq!((w.asked("/article"), w.asked("/robots.txt")), (1, 1));

    // After a quarter of an hour the site is asked whether the page changed, and says no.
    w.age_what_is_kept(Duration::from_secs(16 * 60));
    let later = w.web.read(&w.at("/article"), "", false, &GO_ON).await.unwrap();
    assert!(later.kept && later.gave.contains("four hundred and twelve"));
    assert_eq!((w.asked("/article"), w.asked("/robots.txt")), (2, 1), "one short question to the site; its rules are kept for a day");
    assert_eq!(w.seen.lock().unwrap().last.get("if-none-match").map(String::as_str), Some("\"v1\""));
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn what_a_site_asks_programs_not_to_read_is_not_read() {
    let w = world().await;
    let said = w.web.read(&w.at("/private/diary"), "", false, &GO_ON).await.unwrap_err();
    assert!(said.contains("robots.txt"), "{said}");
    assert_eq!(w.asked("/private/diary"), 0, "the page itself was never asked for");
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn a_redirect_is_followed_on_the_internet_and_never_into_this_computer() {
    let w = world().await;
    let read = w.web.read(&w.at("/to-article"), "", false, &GO_ON).await.unwrap();
    assert_eq!(read.address, w.at("/article"), "the address given is where the page really is");
    for inside in ["/to-inside", "/to-keys"] {
        let said = w.web.read(&w.at(inside), "", false, &GO_ON).await.unwrap_err();
        assert!(said.contains("a redirect was not followed") && said.contains(guard::NOT_PUBLIC), "{inside}: {said}");
    }
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn this_computer_and_its_network_are_not_read_unless_the_person_wrote_the_address_and_allowed_it() {
    let w = world().await;
    // The real rules: no stand-in counts as public.
    let web = Web::new(None);
    for inside in [
        w.at("/article"),
        "http://localhost:7341/api/v1/sessions".into(),
        "http://169.254.169.254/latest/meta-data/".into(),
        "http://[::1]/".into(),
        "http://192.168.1.1/".into(),
        "http://2130706433/".into(),
        "http://nas.local/".into(),
    ] {
        let said = web.read(&inside, "", false, &GO_ON).await.unwrap_err();
        assert!(said.contains(guard::NOT_PUBLIC), "{inside}: {said}");
    }
    assert_eq!(w.asked("/article"), 0);
    for bad in ["file:///etc/passwd", "ftp://example.com/x", "https://user:secret@example.com/"] {
        assert!(web.read(bad, "", false, &GO_ON).await.is_err(), "{bad}");
    }
    // Allowed by the person, for an address the person wrote: read, and the site's rules for programs are
    // not asked for (it is the person's own).
    let read = web.read(&w.at("/article"), "", true, &GO_ON).await.unwrap();
    assert!(read.gave.contains("four hundred and twelve"));
    assert_eq!((w.asked("/article"), w.asked("/robots.txt")), (1, 0));
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn what_is_not_a_page_of_text_is_said_in_words() {
    let w = world().await;
    for (path, why) in [("/picture", "not a page of text (it is image/png)"), ("/paper", "PDF"), ("/nothing-here", "no page at that address (404)")] {
        let said = w.web.read(&w.at(path), "", false, &GO_ON).await.unwrap_err();
        assert!(said.contains(why), "{path}: {said}");
    }
    let big = w.web.read(&w.at("/big"), "", false, &GO_ON).await.unwrap();
    assert!(big.gave.contains("only the first 2 MB were read") && big.gave.contains("This is its start") && big.gave.chars().count() < MOST_OF_A_PAGE + 400, "{}", &big.gave[big.gave.len() - 300..]);
    assert!(w.web.read(&w.at("/latin"), "", false, &GO_ON).await.unwrap().gave.contains("café"), "a page says how it is written, and is read that way");
    let app = w.web.read(&w.at("/app"), "", false, &GO_ON).await.unwrap();
    assert!(app.gave.contains("almost nothing until its scripts have run"), "{}", app.gave);
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn reading_stops_when_the_person_stops_the_answer() {
    let w = world().await;
    let stop = Arc::new(AtomicBool::new(false));
    let later = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        later.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    assert_eq!(w.web.read(&w.at("/slow"), "", false, &stop).await.unwrap_err(), "it was stopped");
    assert!(started.elapsed() < Duration::from_secs(5));
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn a_search_climbs_a_ladder_and_the_first_source_that_answers_ends_it() {
    let w = world().await;
    // With everything turned on and no server of the person's own, Exa is the first rung.
    let found = w.web.search("how wide are a grey heron's wings", "", &Which::default(), &GO_ON).await.unwrap();
    assert_eq!((found.engine.as_str(), found.hits.len()), ("Exa", 2), "{found:?}");
    assert_eq!((found.hits[0].address.clone(), found.hits[0].date.as_deref()), (w.at("/article"), Some("2026-11-18")));
    assert!(found.hits[0].about.contains("155–195 cm (61–77 in) wingspan"), "Exa answers with passages of the pages: {}", found.hits[0].about);
    assert!(found.first.is_none() && found.notes.is_empty(), "with passages in hand, no page is read on top");
    assert!(
        found.told().starts_with(&format!(
            "Results for \"how wide are a grey heron's wings\", from Exa:\n\n1. Grey heron\n   {}\n   Published 2026-11-18\n   Description The grey heron is a large bird",
            w.at("/article")
        )),
        "{}",
        found.told()
    );
    // Exa was called as an assistant calls it, with the search and nothing else, and no other rung was asked.
    let call: serde_json::Value = serde_json::from_str(&w.seen.lock().unwrap().posted).unwrap();
    assert_eq!(call["method"], "tools/call");
    assert_eq!(
        call["params"],
        serde_json::json!({ "name": "web_search_exa", "arguments": { "query": "how wide are a grey heron's wings", "numResults": 5, "objective": "how wide are a grey heron's wings" } })
    );
    assert!(w.seen.lock().unwrap().posted_with["user-agent"].starts_with("DemidoStudio/"));
    assert_eq!((w.asked("POST /mcp"), w.asked("POST /html/"), w.asked("/w/api.php"), w.asked("/article")), (1, 0, 0, 0));

    // Exa says its free searches are used up: the search goes on to DuckDuckGo, and says why.
    w.seen.lock().unwrap().exa_used_up = true;
    let found = w.web.search("how long do grey herons live", "", &Which::default(), &GO_ON).await.unwrap();
    assert_eq!((found.engine.as_str(), found.hits.len()), ("DuckDuckGo", 2), "{found:?}");
    assert_eq!(found.hits[0].address, w.at("/article"));
    assert!(found.notes.len() == 1 && found.notes[0].starts_with("Exa says its free searches are used up for now (429)"), "{:?}", found.notes);
    assert!(found.first.as_deref().is_some_and(|first| first.contains("four hundred and twelve grey herons")), "a line about each page is not much, so the first page is read too");
    // DuckDuckGo was asked as its own form asks: the form's page was loaded, then the form sent from it.
    assert_eq!((w.asked("/html/"), w.asked("POST /html/"), w.asked("POST /lite/")), (1, 1, 0));
    let (posted, last) = {
        let seen = w.seen.lock().unwrap();
        (seen.posted.clone(), seen.posted_with.clone())
    };
    assert_eq!(posted, "q=how+long+do+grey+herons+live&b=");
    assert_eq!(last["referer"], w.at("/html/"), "the page the form stands on, which was really loaded");
    assert!(last["user-agent"].starts_with("DemidoStudio/"), "under Demido's own name");

    // While Exa is left alone it is not asked, and the form's page is not loaded a second time.
    w.web.search("grey heron nest", "", &Which::default(), &GO_ON).await.unwrap();
    assert_eq!((w.asked("POST /mcp"), w.asked("/html/"), w.asked("POST /html/")), (2, 1, 2));
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn a_source_that_asks_for_a_person_is_left_alone_and_the_search_goes_on() {
    let w = world().await;
    w.seen.lock().unwrap().wants_a_person = true;
    let which = Which { exa: false, ..Which::default() };
    let found = w.web.search("grey heron", "", &which, &GO_ON).await.unwrap();
    assert_eq!(found.engine, "Wikipedia and Mwmbl", "{found:?}");
    assert!(
        found.notes.len() == 1 && found.notes[0].contains("proof that a person is searching") && found.notes[0].contains("does not get round that"),
        "said once for both of DuckDuckGo's pages: {:?}",
        found.notes
    );
    assert_eq!((w.asked("/html/"), w.asked("POST /html/"), w.asked("/lite/"), w.asked("POST /lite/")), (1, 0, 0, 0), "refused at the first page, the second was not even tried");

    // Results for something else are not results, and the lighter page is asked instead.
    let w = world().await;
    w.seen.lock().unwrap().decoy = true;
    let found = w.web.search("grey heron", "", &which, &GO_ON).await.unwrap();
    assert_eq!(found.engine, "DuckDuckGo Lite", "{found:?}");
    assert_eq!(found.notes, ["DuckDuckGo answered with results for something else."]);
    assert_eq!(found.hits.iter().map(|h| h.address.clone()).collect::<Vec<_>>(), [w.at("/article"), "https://birds.example/herons".to_string()]);
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn nothing_found_is_believed_only_when_no_later_source_finds_anything() {
    let w = world().await;
    w.seen.lock().unwrap().nothing = true;
    // DuckDuckGo finds nothing; the two after it do.
    let found = w.web.search("grey heron", "", &Which { exa: false, ..Which::default() }, &GO_ON).await.unwrap();
    assert_eq!(found.engine, "Wikipedia and Mwmbl");
    assert_eq!(found.notes, ["DuckDuckGo found nothing.", "DuckDuckGo Lite found nothing."]);
    // With nobody after it, nothing is the answer, and it is an answer: not an error.
    let found = w.web.search("grey heron", "", &Which { exa: false, wikipedia: false, mwmbl: false, ..Which::default() }, &GO_ON).await.unwrap();
    assert!(found.hits.is_empty() && found.engine == "DuckDuckGo" && found.told().starts_with("Nothing was found for \"grey heron\"."), "{found:?}");

    // Nobody would answer at all: that is said, with why, so that it is not taken for "nothing was found".
    let w = world().await;
    {
        let mut seen = w.seen.lock().unwrap();
        (seen.exa_used_up, seen.wants_a_person) = (true, true);
    }
    let said = w.web.search("grey heron", "", &Which { wikipedia: false, mwmbl: false, ..Which::default() }, &GO_ON).await.unwrap_err();
    assert!(said.starts_with("No search service would answer. Exa says its free searches are used up") && said.contains("DuckDuckGo asked for proof"), "{said}");
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn the_persons_own_searxng_server_is_asked_first() {
    let w = world().await;
    let which = Which { searxng: Some(format!(" {}/ ", w.base)), ..Which::default() };
    let found = w.web.search("grey heron", "", &which, &GO_ON).await.unwrap();
    assert_eq!((found.engine.as_str(), found.hits[0].about.as_str(), found.hits[0].date.as_deref()), ("Your SearXNG", "From your own server.", Some("2026-11-18")));
    assert_eq!((w.asked("/search"), w.asked("POST /mcp")), (1, 0));
    // Many servers have their JSON turned off: then the page for people is read.
    w.seen.lock().unwrap().no_json = true;
    let found = w.web.search("herons", "", &which, &GO_ON).await.unwrap();
    assert_eq!((found.engine.as_str(), found.hits[0].about.as_str()), ("Your SearXNG", "From your own server's page."));
    // An address that is none is said to be none, and the search goes on.
    let found = w.web.search("heron", "", &Which { searxng: Some("not an address at all".into()), ..Which::default() }, &GO_ON).await.unwrap();
    assert_eq!((found.engine.as_str(), found.notes.as_slice()), ("Exa", ["Your SearXNG its address in the settings is not a web address.".to_string()].as_slice()));
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn a_search_asks_both_sources_merges_them_and_is_kept() {
    let w = world().await;
    let found = w.web.search("  grey   heron ", "", &the_two(), &GO_ON).await.unwrap();
    assert_eq!(found.query, "grey heron");
    assert_eq!(found.hits.iter().map(|h| h.address.clone()).collect::<Vec<_>>(), [w.at("/article"), w.at("/wiki/Heron"), w.at("/herons?page=2")], "the address on a private network is not a result");
    assert_eq!(found.hits[0].from, ["Wikipedia", "Mwmbl"], "found by both, so first");
    assert!(found.notes.is_empty() && !found.kept);
    let told = found.told();
    assert!(
        told.starts_with(&format!(
            "Results for \"grey heron\", from Wikipedia and Mwmbl:\n\n1. Grey heron (Wikipedia)\n   {}\n   The grey heron (Ardea cinerea) is a long-legged wading bird.\n",
            w.at("/article")
        )),
        "{told}"
    );
    // The first page found was read too: a small model rarely asks for a second tool by itself.
    assert!(told.contains("\nFrom the first of them, Grey heron (Wikipedia):\n\n") && told.contains("four hundred and twelve grey herons"), "{told}");
    assert_eq!((w.asked("/article"), w.asked("/robots.txt")), (1, 1));

    let again = w.web.search("Grey Heron", "", &the_two(), &GO_ON).await.unwrap();
    assert!(again.kept && again.hits == found.hits);
    assert_eq!((w.asked("/api/v2/search/"), w.asked("/w/api.php"), w.asked("/article")), (1, 1, 1), "the same search within the hour asks nobody");

    // One source turned off: only the other is asked.
    let only = w.web.search("herons", "", &Which { mwmbl: false, ..the_two() }, &GO_ON).await.unwrap();
    assert!(only.hits.iter().all(|h| h.from == ["Wikipedia"]) && only.hits.len() == 2);
    assert_eq!(w.asked("/api/v2/search/"), 1);

    for (query, which, why) in
        [("", the_two(), "nothing to search for"), (&"word ".repeat(60), the_two(), "a few words"), ("herons", Which { wikipedia: false, mwmbl: false, ..the_two() }, "turned off")]
    {
        let said = w.web.search(query, "", &which, &GO_ON).await.unwrap_err();
        assert!(said.contains(why), "{said}");
    }
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn a_source_that_says_it_is_asked_too_much_is_left_alone_and_the_other_still_answers() {
    let w = world().await;
    w.seen.lock().unwrap().too_much = true;
    let found = w.web.search("grey heron", "", &the_two(), &GO_ON).await.unwrap();
    assert!(found.hits.iter().all(|h| h.from == ["Wikipedia"]) && found.hits.len() == 2, "{found:?}");
    assert!(found.notes.len() == 1 && found.notes[0].contains("asked too much (429)") && found.notes[0].contains("leaves it alone"), "{:?}", found.notes);
    assert!(found.told().ends_with("quarter of an hour.)"));

    // What was found is kept for the hour, without the story of who refused: the same search asks nobody.
    w.seen.lock().unwrap().too_much = false;
    let second = w.web.search("grey heron", "", &the_two(), &GO_ON).await.unwrap();
    assert!(second.kept && second.notes.is_empty() && second.hits == found.hits);
    // Another search within the quarter of an hour: Mwmbl would answer again, and is not asked.
    let third = w.web.search("herons", "", &the_two(), &GO_ON).await.unwrap();
    assert_eq!(third.notes.len(), 1);
    assert_eq!((w.asked("/api/v2/search/"), w.asked("/w/api.php")), (1, 2));

    // With only that source turned on, the search says why it found nothing.
    let said = w.web.search("grey heron", "", &Which { wikipedia: false, ..the_two() }, &GO_ON).await.unwrap_err();
    assert!(said.contains("asked too much"), "{said}");
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn what_is_kept_does_not_grow_without_end() {
    let dir = std::env::temp_dir().join(format!("demido-web-sweep-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    // Nine files of 30 MB: more than is kept. Sparse, so the test writes nothing to speak of.
    for i in 0..9u64 {
        let file = std::fs::File::create(dir.join(format!("page-{i}.json"))).unwrap();
        file.set_len(30_000_000).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(1000 - i * 100)).unwrap();
    }
    let _web = Web::new(Some(dir.clone()));
    let left: Vec<String> = {
        let mut names: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    };
    assert_eq!(left, ["page-4.json", "page-5.json", "page-6.json", "page-7.json", "page-8.json"], "the oldest went, down to three quarters of the limit");
    std::fs::remove_dir_all(dir).ok();
}
