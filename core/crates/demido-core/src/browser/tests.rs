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
    // It said who it is: its own name with Demido's after it.
    let agent = seen.lock().unwrap().user_agent.clone();
    assert!(agent.contains("HeadlessChrome/") && agent.contains("DemidoStudio/"), "{agent}");

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
