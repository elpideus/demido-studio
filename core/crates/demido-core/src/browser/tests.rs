//! Demido's browser, for real, against a stand-in site on this computer (DECISIONS Z). These need the headless
//! shell: they run when `DEMIDO_TRY_BROWSER` names it (`demido browser install` puts one in
//! `programs/browser/versions/<v>/chrome-headless-shell/…`), and say so and pass when it does not.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::*;

static GO_ON: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Seen {
    asked: Vec<String>,
    user_agent: String,
}

/// A page whose text is written by its scripts, with a link, a search form of one box, a button that writes,
/// and a password box. `{leak}` is an address on this computer that is not the stand-in.
const APP: &str = r#"<!doctype html><html><head><title>Heron watch</title></head><body>
<div id="root"></div>
<a href="/second">The second page</a>
<form action="/search" method="get"><input name="q" placeholder="Search herons"></form>
<button id="more" onclick="document.getElementById('root').insertAdjacentHTML('beforeend', '<p>The button was pressed, and this sentence came after it.</p>')">Show more</button>
<form action="/login" method="post"><input name="user" placeholder="Name"><input type="password" name="pw" placeholder="Password"></form>
<script>
  document.getElementById('root').innerHTML = '<h1>Heron watch</h1><p>' + 'Grey herons nest high in the reed beds of the northern lagoon, where foxes cannot reach them. '.repeat(4) + '</p>';
  // Things a page might try: a request to this computer, and WebRTC to a listener here.
  fetch('{leak}/fetched').catch(() => {});
  try { const pc = new RTCPeerConnection({ iceServers: [{ urls: 'stun:{stun}' }] }); pc.createDataChannel('x'); pc.createOffer().then((o) => pc.setLocalDescription(o)); } catch (e) {}
</script></body></html>"#;

async fn site(State(seen): State<Arc<Mutex<Seen>>>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or_default().to_string();
    {
        let mut s = seen.lock().unwrap();
        s.asked.push(path.clone());
        if path == "/app" {
            s.user_agent = request.headers().get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
        }
    }
    let html = |body: String| ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response();
    match path.as_str() {
        "/robots.txt" => ([(header::CONTENT_TYPE, "text/plain")], "User-agent: *\nDisallow: /private\n").into_response(),
        "/second" => html("<html><head><title>The second page</title></head><body><p>This is the second page, reached by its link.</p></body></html>".into()),
        "/search" => html(format!(
            "<html><head><title>Results</title></head><body><p>You searched for: {}</p></body></html>",
            url::form_urlencoded::parse(query.as_bytes()).find(|(k, _)| k == "q").map(|(_, v)| v.into_owned()).unwrap_or_default()
        )),
        "/private" => html("<html><body>Not for programs.</body></html>".into()),
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Where the headless shell is, when this test is to run.
fn shell() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var("DEMIDO_TRY_BROWSER").ok()?);
    path.is_file().then_some(path)
}

#[tokio::test]
async fn a_model_opens_a_page_after_its_scripts_ran_presses_and_types_and_only_the_public_internet_is_reached() {
    let Some(program) = shell() else {
        eprintln!("Skipped: DEMIDO_TRY_BROWSER does not name a headless shell.");
        return;
    };
    let seen = Arc::new(Mutex::new(Seen::default()));
    // The stand-in site, and another door on this computer that is not it: nothing may reach that one.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let door = listener.local_addr().unwrap();
    let app = Router::new().fallback(site).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let other = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let other_at = other.local_addr().unwrap();
    let other_hits = Arc::new(AtomicUsize::new(0));
    let hits = other_hits.clone();
    tokio::spawn(async move {
        while let Ok(_c) = other.accept().await {
            hits.fetch_add(1, Ordering::Relaxed);
        }
    });
    let stun = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    stun.set_nonblocking(true).unwrap();
    let stun_at = stun.local_addr().unwrap();
    // The page is served from the stand-in, with the two addresses written into it.
    let page = APP.replace("{leak}", &format!("http://{other_at}")).replace("{stun}", &stun_at.to_string());
    let page_door = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let page_at = page_door.local_addr().unwrap();
    let (robots_seen, app_seen) = (seen.clone(), seen.clone());
    tokio::spawn(async move {
        let app = Router::new()
            .route(
                "/app",
                axum::routing::get(move |headers: axum::http::HeaderMap| async move {
                    app_seen.lock().unwrap().user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
                    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page.clone()).into_response()
                }),
            )
            .fallback(site)
            .with_state(robots_seen);
        axum::serve(page_door, app).await.unwrap()
    });

    let home = crate::testing::TempDir::new("demido-browser");
    let db = Arc::new(crate::db::Db::memory().unwrap());
    let events = Arc::new(EventLog::new(db));
    let stand_ins = vec![door.to_string(), page_at.to_string()];
    let web = Arc::new(Web::with(None, crate::web::Sources::default(), stand_ins.clone()));
    let browser = Browser::new(&home, web, events).for_a_test(program, stand_ins);

    // Opened: its text as its scripts wrote it, and the numbered list.
    let opened = browser.open("s1", &format!("http://{page_at}/app"), "where do herons nest?", &GO_ON).await.unwrap();
    assert_eq!(opened.title, "Heron watch");
    assert!(opened.gave.contains("Grey herons nest high in the reed beds"), "the scripts' text: {}", opened.gave);
    assert!(opened.gave.contains(&format!("1. link \"The second page\" → http://{page_at}/second")), "{}", opened.gave);
    assert!(opened.gave.contains("2. field \"Search herons\""), "{}", opened.gave);
    assert!(opened.gave.contains("3. button \"Show more\""), "{}", opened.gave);
    assert!(opened.gave.contains("5. field \"Password\" (a password: never typed in)"), "{}", opened.gave);
    let picture = opened.picture.clone().expect("a picture for the person");
    assert!(browser.picture("s1", &picture).is_some_and(|b| b.starts_with(&[0xFF, 0xD8])), "a JPEG");
    // An ordinary Chrome's name (Z18).
    let agent = seen.lock().unwrap().user_agent.clone();
    assert!(agent.contains(" Chrome/") && !agent.contains("Headless") && !agent.contains("DemidoStudio"), "{agent}");

    // Pressing a button: the page as it is after.
    let pressed = browser.press("s1", 3, "", &GO_ON).await.unwrap();
    assert!(pressed.gave.contains("The button was pressed"), "{}", pressed.gave);
    // Typing into a form's only box sends it.
    let typed = browser.type_text("s1", 2, "grey heron", "", &GO_ON).await.unwrap();
    assert!(typed.gave.contains("You searched for: grey heron"), "{}", typed.gave);
    assert!(typed.address.ends_with("/search?q=grey+heron"), "{}", typed.address);
    // A link, pressed: the page it leads to.
    browser.open("s1", &format!("http://{page_at}/app"), "", &GO_ON).await.unwrap();
    let followed = browser.press("s1", 1, "", &GO_ON).await.unwrap();
    assert!(followed.gave.contains("This is the second page"), "{}", followed.gave);

    // Never a password box, and never a number the page does not have.
    browser.open("s1", &format!("http://{page_at}/app"), "", &GO_ON).await.unwrap();
    let said = browser.type_text("s1", 5, "hunter2", "", &GO_ON).await.unwrap_err();
    assert!(said.contains("password field"), "{said}");
    let said = browser.press("s1", 99, "", &GO_ON).await.unwrap_err();
    assert!(said.contains("there is no number 99"), "{said}");

    // A page the site asks programs not to read is not opened; this computer is not reached at all.
    let said = browser.open("s1", &format!("http://{page_at}/private"), "", &GO_ON).await.unwrap_err();
    assert!(said.contains("robots.txt"), "{said}");
    let said = browser.open("s1", &format!("http://{other_at}/"), "", &GO_ON).await.unwrap_err();
    assert!(said.contains(guard::NOT_PUBLIC), "{said}");
    assert_eq!(other_hits.load(Ordering::Relaxed), 0, "the page's own request to this computer went nowhere");
    let mut b = [0u8; 64];
    assert!(stun.recv_from(&mut b).is_err(), "WebRTC sent nothing");

    // Each session has a page and cookies of its own; a closed one is gone.
    browser.open("s2", &format!("http://{page_at}/second"), "", &GO_ON).await.unwrap();
    assert_eq!(browser.status().await.pages.len(), 2);
    browser.close("s2").await;
    assert_eq!(browser.status().await.pages.len(), 1);
    // Reading a page for read_page leaves no page open and keeps no picture.
    let read = browser.read(&format!("http://{page_at}/app"), "herons", &GO_ON).await.unwrap();
    assert!(read.gave.contains("Grey herons nest") && read.picture.is_none());
    assert_eq!(browser.status().await.pages.len(), 1);
    browser.end().await;
}

#[test]
fn the_list_a_model_is_given_says_what_each_thing_is() {
    let items = vec![
        Item { n: 1, kind: "link".into(), words: "About".into(), href: Some("https://example.org/about".into()), ..Item::default() },
        Item { n: 2, kind: "choice".into(), words: "Size".into(), choices: vec!["Small".into(), "Large".into()], ..Item::default() },
        Item { n: 3, kind: "field".into(), words: "".into(), password: true, ..Item::default() },
    ];
    assert_eq!(
        listed(&items),
        "On the page, what can be pressed (press with its number) or filled (type_text with its number):\n1. link \"About\" → https://example.org/about\n2. choice \"Size\" (choices: Small, Large)\n3. field (no words) (a password: never typed in)"
    );
    assert_eq!(listed(&[]), "Nothing on the page can be pressed or filled.");
}

/// The person's side (DECISIONS Z12 to Z16): tabs, any public address, and what is at a point, in words.
#[tokio::test]
async fn the_person_has_tabs_goes_to_public_addresses_and_a_point_is_described() {
    let Some(program) = shell() else {
        eprintln!("Skipped: DEMIDO_TRY_BROWSER does not name a headless shell.");
        return;
    };
    let page = r#"<!doctype html><html><head><title>Two buttons</title></head><body style="margin:0">
<button id="more" style="position:absolute;left:40px;top:40px;width:120px;height:40px">Show more</button>
<div class="card" style="position:absolute;left:300px;top:200px;width:300px;height:200px"><p>Grey herons nest in reeds.</p></div>
<svg style="position:absolute;left:40px;top:120px" width="40" height="40"><title>A heron</title><path d="M0 0H40V40H0Z"/></svg>
<a href="/x" style="position:absolute;left:40px;top:200px;font-size:20px">A link</a>
<p style="position:absolute;left:40px;top:300px;margin:0;font-size:20px">Herons</p>
</body></html>"#;
    let door = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = door.local_addr().unwrap();
    tokio::spawn(async move {
        let app = Router::new().route("/", axum::routing::get(move || async move { ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page) }));
        axum::serve(door, app).await.unwrap()
    });
    let home = crate::testing::TempDir::new("demido-browser-person");
    let events = Arc::new(EventLog::new(Arc::new(crate::db::Db::memory().unwrap())));
    let stand_ins = vec![at.to_string()];
    let web = Arc::new(Web::with(None, crate::web::Sources::default(), stand_ins.clone()));
    let browser = Browser::new(&home, web, events).for_a_test(program, stand_ins);

    // Where the person may go: a name alone is https; this computer is not the public internet.
    assert_eq!(browser.person_may_go("example.org").unwrap().as_str(), "https://example.org/");
    assert!(browser.person_may_go("127.0.0.1:9").unwrap_err().contains(guard::NOT_PUBLIC));
    assert!(browser.person_may_go("file:///etc/passwd").is_err());
    assert_eq!(browser.person_may_go("grey heron").unwrap().as_str(), "https://duckduckgo.com/?q=grey+heron");

    // A tab, opened at an address; its place is made with it.
    browser.open_tab("p1", Some(&format!("http://{at}/"))).await.unwrap();
    let (cdp, page_session, first) = browser.front("p1").await.unwrap();
    for _ in 0..40 {
        let state = cdp.call("Runtime.evaluate", json!({ "expression": "document.readyState", "returnByValue": true }), Some(&page_session)).await.unwrap();
        if state.pointer("/result/value").and_then(Value::as_str) == Some("complete") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // The empty tab the place was made with, and the one opened: the second is in front.
    let (tabs, front) = browser.tabs("p1").await;
    assert_eq!((tabs.len(), front.as_deref()), (2, Some(first.as_str())));

    // What is at a point, and what holds a box, in words.
    let button = Browser::describe(&cdp, &page_session, 100.0, 60.0, 0.0, 0.0).await.unwrap();
    assert_eq!((button["tag"].as_str(), button["text"].as_str(), button["selector"].as_str()), (Some("button"), Some("Show more"), Some("button#more")));
    let card = Browser::describe(&cdp, &page_session, 310.0, 210.0, 200.0, 40.0).await.unwrap();
    assert_eq!(card["tag"].as_str(), Some("div"), "{card}");
    assert!(card["text"].as_str().unwrap().contains("Grey herons"), "{card}");
    let drawing = Browser::describe(&cdp, &page_session, 60.0, 140.0, 0.0, 0.0).await.unwrap();
    assert_eq!((drawing["tag"].as_str(), drawing["label"].as_str()), (Some("svg"), Some("A heron")), "{drawing}");
    // The cursor a browser would show there: the pointer on a link, the text bar on words, the arrow elsewhere.
    let cursor = |x: f64, y: f64| {
        let (cdp, page_session) = (cdp.clone(), page_session.clone());
        async move { Browser::in_our_world(&cdp, &page_session, &format!("({})({x}, {y})", super::live::CURSOR)).await.unwrap() }
    };
    assert_eq!(cursor(50.0, 210.0).await, json!("pointer"));
    assert_eq!(cursor(50.0, 310.0).await, json!("text"));
    assert_eq!(cursor(900.0, 700.0).await, json!("default"));

    // Tabs: another in front, the first again, and one closed.
    let other = tabs[0].id.clone();
    assert!(browser.activate("p1", &other).await);
    assert_eq!(browser.tabs("p1").await.1.as_deref(), Some(other.as_str()));
    browser.close_tab("p1", &other).await;
    let (tabs, front) = browser.tabs("p1").await;
    assert_eq!((tabs.len(), front.as_deref()), (1, Some(first.as_str())));
    // The last one closed closes the place.
    browser.close_tab("p1", &first).await;
    assert!(browser.tabs("p1").await.0.is_empty());
    browser.end().await;
}

/// Chrome's own brands, in Chrome's own order, as Chromium 158 gives them (measured: the headless shell, with
/// its own name in Google Chrome's place); the person's language from the system's.
#[test]
fn an_ordinary_chrome_is_named_as_chrome_names_itself() {
    let o = Ordinary::of("158.0.8127.88");
    let names: Vec<(&str, &str)> = o.brands.iter().map(|(b, v)| (b.as_str(), v.as_str())).collect();
    assert_eq!(names, [("Chromium", "158"), ("Not.A/Brand", "24"), ("Google Chrome", "158")]);
    assert_eq!(o.full[1], ("Not.A/Brand".to_string(), "24.0.0.0".to_string()));
    assert!(o.agent.ends_with("(KHTML, like Gecko) Chrome/158.0.0.0 Safari/537.36"), "{}", o.agent);
    assert_eq!(languages_of(Some("en_US.UTF-8")), "en-US,en");
    assert_eq!(languages_of(Some("it_IT@euro")), "it-IT,it");
    assert_eq!(languages_of(Some("de")), "de");
    assert_eq!(languages_of(None), "en-US,en");
}

/// What a site is told and what its scripts read: an ordinary Chrome's, nothing that says a program drives it
/// (DECISIONS Z18). The page's own requests, a worker's and a service worker's are heard by the site.
#[tokio::test]
async fn the_browser_says_and_sends_what_an_ordinary_chrome_does() {
    let Some(program) = shell() else {
        eprintln!("Skipped: DEMIDO_TRY_BROWSER does not name a headless shell.");
        return;
    };
    let page = r#"<!doctype html><html><head><title>Who</title></head><body><pre id="out"></pre><script>
const worker = new Worker('/worker.js');
const fromWorker = new Promise((done) => { worker.onmessage = (e) => done(e.data); });
navigator.serviceWorker.register('/sw.js').then(() => navigator.serviceWorker.ready).then(() => fetch('/after-sw')).catch(() => {});
Promise.all([navigator.userAgentData.getHighEntropyValues(['platformVersion', 'fullVersionList', 'architecture']), fromWorker]).then(([high, worker]) => {
  document.getElementById('out').textContent = JSON.stringify({ webdriver: navigator.webdriver, agent: navigator.userAgent, brands: navigator.userAgentData.brands, mobile: navigator.userAgentData.mobile, platform: navigator.userAgentData.platform, navigatorPlatform: navigator.platform, high, languages: navigator.languages, worker });
});
</script></body></html>"#;
    let worker = "fetch('/from-worker').then(() => postMessage(navigator.userAgent));";
    let service = "self.addEventListener('install', (e) => { self.skipWaiting(); e.waitUntil(fetch('/from-sw')); }); self.addEventListener('activate', (e) => e.waitUntil(clients.claim()));";
    let heard = Arc::new(Mutex::new(HashMap::<String, Vec<(String, String)>>::new()));
    let kept = heard.clone();
    let door = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = door.local_addr().unwrap();
    tokio::spawn(async move {
        let app = Router::new().route("/robots.txt", axum::routing::get(|| async { "User-agent: *\nAllow: /\n" })).fallback(move |request: Request| async move {
            let path = request.uri().path().to_string();
            kept.lock().unwrap().insert(path.clone(), request.headers().iter().map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string())).collect());
            match path.as_str() {
                "/who" => ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], page).into_response(),
                "/worker.js" => ([(header::CONTENT_TYPE, "text/javascript")], worker).into_response(),
                "/sw.js" => ([(header::CONTENT_TYPE, "text/javascript")], service).into_response(),
                _ => "ok".into_response(),
            }
        });
        axum::serve(door, app).await.unwrap()
    });
    let home = crate::testing::TempDir::new("demido-browser-who");
    let events = Arc::new(EventLog::new(Arc::new(crate::db::Db::memory().unwrap())));
    let stand_ins = vec![at.to_string()];
    let web = Arc::new(Web::with(None, crate::web::Sources::default(), stand_ins.clone()));
    let browser = Browser::new(&home, web, events).for_a_test(program, stand_ins);
    let opened = browser.open("w1", &format!("http://{at}/who"), "", &GO_ON).await.unwrap();
    // The service worker's requests come when it is installed: a little after the page.
    for _ in 0..50 {
        if heard.lock().unwrap().contains_key("/from-sw") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let heard = heard.lock().unwrap().clone();
    let header_of = |path: &str, name: &str| heard.get(path).and_then(|h| h.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()));

    let ordinary = |agent: &str| agent.contains("Chrome/") && agent.ends_with(" Safari/537.36") && !agent.contains("Headless") && !agent.contains("DemidoStudio") && agent.contains(".0.0.0 ");
    for path in ["/who", "/worker.js", "/from-worker", "/sw.js", "/from-sw", "/after-sw"] {
        let agent = header_of(path, "user-agent").unwrap_or_else(|| panic!("{path} was not asked for"));
        assert!(ordinary(&agent), "{path}: {agent}");
    }
    let hints = header_of("/who", "sec-ch-ua").expect("the client hints");
    assert!(hints.contains("\"Google Chrome\"") && hints.contains("\"Chromium\"") && !hints.contains("Headless"), "{hints}");
    assert_eq!(header_of("/who", "sec-ch-ua-mobile").as_deref(), Some("?0"));
    assert!(header_of("/who", "accept-language").is_some_and(|l| !l.is_empty()), "a language");
    // What the page's scripts read.
    let said: Value = serde_json::from_str(opened.gave.lines().find(|l| l.starts_with('{')).expect("the page wrote what it read")).unwrap();
    assert_eq!(said["webdriver"], json!(false), "{said}");
    assert!(ordinary(said["agent"].as_str().unwrap()) && ordinary(said["worker"].as_str().unwrap()), "{said}");
    let brands: Vec<&str> = said["brands"].as_array().unwrap().iter().filter_map(|b| b["brand"].as_str()).collect();
    assert!(brands.contains(&"Google Chrome") && brands.contains(&"Chromium") && !brands.iter().any(|b| b.contains("Headless")), "{said}");
}
