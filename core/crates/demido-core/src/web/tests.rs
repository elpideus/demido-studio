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
}

type Shared = Arc<Mutex<Seen>>;

async fn site(State(seen): State<Shared>, headers: HeaderMap, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let too_much = {
        let mut seen = seen.lock().unwrap();
        *seen.asked.entry(path.clone()).or_default() += 1;
        seen.last = headers.iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string())).collect();
        seen.too_much
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
    let web = Web::with(Some(kept_in.clone()), Sources { wikipedia: base.clone(), mwmbl: base.clone() }, vec![door.to_string()]);
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
async fn a_search_asks_both_sources_merges_them_and_is_kept() {
    let w = world().await;
    let found = w.web.search("  grey   heron ", "", &Which::default(), &GO_ON).await.unwrap();
    assert_eq!(found.query, "grey heron");
    assert_eq!(found.hits.iter().map(|h| h.address.clone()).collect::<Vec<_>>(), [w.at("/article"), w.at("/wiki/Heron"), w.at("/herons?page=2")], "the address on a private network is not a result");
    assert_eq!(found.hits[0].from, ["Wikipedia", "Mwmbl"], "found by both, so first");
    assert!(found.notes.is_empty() && !found.kept);
    let told = found.told();
    assert!(
        told.starts_with(&format!("Results for \"grey heron\":\n\n1. Grey heron (Wikipedia)\n   {}\n   The grey heron (Ardea cinerea) is a long-legged wading bird.\n", w.at("/article"))),
        "{told}"
    );
    // The first page found was read too: a small model rarely asks for a second tool by itself.
    assert!(told.contains("\nFrom the first of them, Grey heron (Wikipedia):\n\n") && told.contains("four hundred and twelve grey herons"), "{told}");
    assert_eq!((w.asked("/article"), w.asked("/robots.txt")), (1, 1));

    let again = w.web.search("Grey Heron", "", &Which::default(), &GO_ON).await.unwrap();
    assert!(again.kept && again.hits == found.hits);
    assert_eq!((w.asked("/api/v2/search/"), w.asked("/w/api.php"), w.asked("/article")), (1, 1, 1), "the same search within the hour asks nobody");

    // One source turned off: only the other is asked.
    let only = w.web.search("herons", "", &Which { mwmbl: false, ..Which::default() }, &GO_ON).await.unwrap();
    assert!(only.hits.iter().all(|h| h.from == ["Wikipedia"]) && only.hits.len() == 2);
    assert_eq!(w.asked("/api/v2/search/"), 1);

    for (query, which, why) in [
        ("", Which::default(), "nothing to search for"),
        (&"word ".repeat(60), Which::default(), "a few words"),
        ("herons", Which { wikipedia: false, mwmbl: false, ..Which::default() }, "turned off"),
    ] {
        let said = w.web.search(query, "", &which, &GO_ON).await.unwrap_err();
        assert!(said.contains(why), "{said}");
    }
    std::fs::remove_dir_all(&w.kept_in).ok();
}

#[tokio::test]
async fn a_source_that_says_it_is_asked_too_much_is_left_alone_and_the_other_still_answers() {
    let w = world().await;
    w.seen.lock().unwrap().too_much = true;
    let found = w.web.search("grey heron", "", &Which::default(), &GO_ON).await.unwrap();
    assert!(found.hits.iter().all(|h| h.from == ["Wikipedia"]) && found.hits.len() == 2, "{found:?}");
    assert!(found.notes.len() == 1 && found.notes[0].contains("asked too much (429)") && found.notes[0].contains("leaves it alone"), "{:?}", found.notes);
    assert!(found.told().ends_with("quarter of an hour.)"));

    // Even when it would answer again, it is not asked for a quarter of an hour. A search with a source
    // missing is not kept, so Wikipedia is asked again.
    w.seen.lock().unwrap().too_much = false;
    let second = w.web.search("grey heron", "", &Which::default(), &GO_ON).await.unwrap();
    assert_eq!(second.notes.len(), 1);
    assert_eq!((w.asked("/api/v2/search/"), w.asked("/w/api.php")), (1, 2));

    // With only that source turned on, the search says why it found nothing.
    let said = w.web.search("grey heron", "", &Which { wikipedia: false, ..Which::default() }, &GO_ON).await.unwrap_err();
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
