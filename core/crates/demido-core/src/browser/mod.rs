//! The browser a model drives (DECISIONS Z, research note 31).
//!
//! Demido's own copy of Chrome for Testing's headless shell (`copy`), driven through a pipe (`cdp`), with
//! everything it loads going through Demido's own proxy (`proxy`), which lets through only the public
//! internet. Its profile is empty of anything of the person's; each session that browses has a context of its
//! own, with its own cookies, and one page.
//!
//! What a model is given of a page: its text after its scripts ran, chosen as `read_page` chooses it, and a
//! numbered list of what can be pressed or filled (`look.js`, run in a world of Demido's own, apart from the
//! page's scripts). A picture of the page is taken for the person, never given to the model.
//!
//! When a use asks the person is decided where every tool's is (`permissions`): opening an address as
//! `read_page` does; pressing only at Cautious; typing the model's own words at Balanced (Z7). A password
//! field is never typed in.
//!
//! The person uses the same browser from the window (`live`, DECISIONS Z12 to Z16): a session's place has
//! tabs; the window sees the active one as it changes, and its clicks, keys and addresses go to it. The model's
//! tools act on the active tab too.

pub mod cdp;
pub mod copy;
pub mod live;
pub mod proxy;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::web::{Web, guard};

/// The script that numbers what can be pressed or filled, and gives back the page's HTML.
const LOOK: &str = include_str!("look.js");
/// A page left alone this long is closed, and the browser with the last one.
const IDLE: Duration = Duration::from_secs(10 * 60);
/// How long a page may take to load before it is read as it is.
const LOADING_TAKES_AT_MOST: Duration = Duration::from_secs(20);
/// The pictures kept for each session.
const PICTURES_KEPT: usize = 50;
/// The name a page that is only read (`read_page`'s fallback) is kept under: no session's.
const ONLY_READING: &str = "(reading)";
/// The size of the page as a person's window would be.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;

/// One thing on a page that can be pressed or filled, as the model is shown it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Item {
    pub n: u32,
    /// `link`, `button`, `field`, `checkbox`, `radio` or `choice`.
    pub kind: String,
    /// Its words: its label, its text, what it says inside.
    pub words: String,
    #[serde(default)]
    pub href: Option<String>,
    /// A password field: never typed in (Z7).
    #[serde(default)]
    pub password: bool,
    /// The one text field of its form: typing in it sends the form, as Enter would.
    #[serde(default)]
    pub alone: bool,
    /// What a choice offers.
    #[serde(default)]
    pub choices: Vec<String>,
}

/// What the model was given of a page, and the picture taken for the person.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub address: String,
    pub title: String,
    /// The text and the numbered list, as the model is given them.
    pub gave: String,
    /// The picture's name, in the session's pictures.
    pub picture: Option<String>,
}

/// The browser as the window shows it.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Status {
    /// Whether it is offered on this system at all.
    pub offered: bool,
    pub kept: Option<copy::Kept>,
    pub downloading: Option<copy::Downloading>,
    pub running: bool,
    /// The pages open now, by session.
    pub pages: Vec<OpenPage>,
    /// What its proxy let through and refused since it started.
    pub through: u64,
    pub refused: u64,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct OpenPage {
    pub session: String,
    pub address: String,
    pub title: String,
    pub picture: Option<String>,
    /// Milliseconds since it was last used.
    pub idle_ms: u64,
}

/// One tab of a session's place.
struct Tab {
    /// The browser's own name for the page (its target).
    target: String,
    /// The browser's own name for the page's session (the protocol's).
    session: String,
    items: Vec<Item>,
    address: String,
    title: String,
    picture: Option<String>,
}

/// A session's place in the browser: a context of its own (its cookies), its tabs, and which is in front.
struct Place {
    context: String,
    tabs: Vec<Tab>,
    active: usize,
    used: Instant,
    /// The size its pages are laid out at: the window's view of them, or a person's window's.
    size: (u32, u32),
}

impl Place {
    fn front(&mut self) -> Option<&mut Tab> {
        let at = self.active.min(self.tabs.len().saturating_sub(1));
        self.tabs.get_mut(at)
    }
}

struct Running {
    cdp: Arc<cdp::Cdp>,
    proxy: proxy::Proxy,
    agent: String,
    places: HashMap<String, Place>,
}

/// A tab, as the window lists it.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct TabFacts {
    pub id: String,
    pub address: String,
    pub title: String,
}

pub struct Browser {
    copy: copy::Copy,
    web: Arc<Web>,
    events: Arc<EventLog>,
    pictures: PathBuf,
    running: tokio::sync::Mutex<Option<Running>>,
    /// Addresses on this computer that count as the public internet. Only a test fills this.
    stand_ins: Vec<String>,
    /// A program to run in place of the downloaded copy: a test's, or a development build's
    /// (`DEMIDO_DEV_BROWSER`).
    program: Option<PathBuf>,
}

fn stopped(stop: &AtomicBool) -> Result<(), String> {
    if stop.load(Ordering::Relaxed) { Err("it was stopped".into()) } else { Ok(()) }
}

/// What the browser said when a page did not load, in words a model can pass on.
fn why_not_loaded(error: &str) -> String {
    if error.contains("TUNNEL") || error.contains("PROXY") || error.contains("NAME_NOT_RESOLVED") {
        format!("the page could not be opened: its address {} or could not be reached ({error})", guard::NOT_PUBLIC)
    } else if error.contains("TIMED_OUT") {
        "the site did not answer in time".into()
    } else {
        format!("the page could not be opened ({error})")
    }
}

/// The numbered list, as the model is given it.
fn listed(items: &[Item]) -> String {
    if items.is_empty() {
        return "Nothing on the page can be pressed or filled.".into();
    }
    let mut out = String::from("On the page, what can be pressed (press with its number) or filled (type_text with its number):\n");
    for item in items {
        let words = if item.words.is_empty() { "(no words)".to_string() } else { format!("\"{}\"", item.words) };
        out.push_str(&format!("{}. {} {words}", item.n, item.kind));
        if let Some(href) = &item.href {
            out.push_str(&format!(" → {href}"));
        }
        if item.password {
            out.push_str(" (a password: never typed in)");
        }
        if !item.choices.is_empty() {
            out.push_str(&format!(" (choices: {})", item.choices.join(", ")));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

impl Browser {
    pub fn new(home: &std::path::Path, web: Arc<Web>, events: Arc<EventLog>) -> Self {
        #[allow(unused_mut)]
        let mut program = None;
        #[cfg(debug_assertions)]
        if let Ok(path) = std::env::var("DEMIDO_DEV_BROWSER") {
            program = Some(PathBuf::from(path));
        }
        Self { copy: copy::Copy::new(home, events.clone()), web, events, pictures: home.join("browser-pictures"), running: tokio::sync::Mutex::new(None), stand_ins: Vec::new(), program }
    }

    /// The same, with a program of the test's and a stand-in site that counts as public. For tests.
    #[cfg(test)]
    pub(crate) fn for_a_test(mut self, program: PathBuf, stand_ins: Vec<String>) -> Self {
        (self.program, self.stand_ins) = (Some(program), stand_ins);
        self
    }

    pub fn copy(&self) -> &copy::Copy {
        &self.copy
    }

    /// The program that is run: the downloaded copy, or a test's.
    fn program(&self) -> Option<PathBuf> {
        self.program.clone().or_else(|| self.copy.kept().map(|k| PathBuf::from(k.path)))
    }

    /// Whether the browser can be used here: offered on this system, and downloaded.
    pub fn ready(&self) -> bool {
        cfg!(unix) && self.program().is_some()
    }

    pub async fn status(&self) -> Status {
        let running = self.running.lock().await;
        let pages = running
            .as_ref()
            .map(|r| {
                r.places
                    .iter()
                    .filter(|(s, _)| s.as_str() != ONLY_READING)
                    .filter_map(|(s, p)| {
                        let t = p.tabs.get(p.active.min(p.tabs.len().saturating_sub(1)))?;
                        Some(OpenPage { session: s.clone(), address: t.address.clone(), title: t.title.clone(), picture: t.picture.clone(), idle_ms: p.used.elapsed().as_millis() as u64 })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Status {
            offered: copy::platform().is_some() && cfg!(unix),
            kept: self.copy.kept(),
            downloading: self.copy.downloading(),
            running: running.as_ref().is_some_and(|r| r.cdp.running()),
            pages,
            through: running.as_ref().map_or(0, |r| r.proxy.counts.through.load(Ordering::Relaxed)),
            refused: running.as_ref().map_or(0, |r| r.proxy.counts.refused.load(Ordering::Relaxed)),
        }
    }

    /// Starts the browser if it is not running.
    async fn start(&self, running: &mut Option<Running>) -> Result<(), String> {
        if running.as_ref().is_some_and(|r| r.cdp.running()) {
            return Ok(());
        }
        *running = None;
        let program = self.program().ok_or("Demido's browser is not downloaded. It can be downloaded on the Browser screen (about 130 MB), or with: demido browser install")?;
        let proxy = proxy::Proxy::start(self.stand_ins.clone()).await.map_err(|e| format!("the browser's proxy could not start ({e})"))?;
        let profile = self.copy.profile();
        std::fs::create_dir_all(&profile).map_err(|e| e.to_string())?;
        let args: Vec<String> = [
            "--remote-debugging-pipe".to_string(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            format!("--user-data-dir={}", profile.display()),
            // Everything through Demido's proxy, this computer's own addresses too; the browser looks no name up.
            format!("--proxy-server=http://127.0.0.1:{}", proxy.port),
            "--proxy-bypass-list=<-loopback>".into(),
            // The proxy is reached by its number, which the rule would otherwise map to nothing too.
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1".into(),
            // WebRTC: no UDP, and TCP only through the proxy (measured, research note 31).
            "--force-webrtc-ip-handling-policy=disable_non_proxied_udp".into(),
            // Nothing of its own sent to Google, nothing kept that a person would have.
            "--disable-background-networking".into(),
            "--disable-component-update".into(),
            "--disable-sync".into(),
            "--disable-extensions".into(),
            "--disable-default-apps".into(),
            "--disable-domain-reliability".into(),
            "--no-pings".into(),
            "--mute-audio".into(),
            "--password-store=basic".into(),
            "--use-mock-keychain".into(),
            "--disable-features=Translate,OptimizationHints,MediaRouter,AutofillServerCommunication,InterestFeedContentSuggestions".into(),
            format!("--window-size={WIDTH},{HEIGHT}"),
            "--hide-scrollbars".into(),
            "about:blank".into(),
        ]
        .into();
        let cdp = cdp::Cdp::start(&program, &args).map_err(|e| format!("the browser could not start ({e})"))?;
        let version = cdp.call("Browser.getVersion", json!({}), None).await?;
        // Its own name, and Demido's after it: never a browser's name alone (N7).
        let agent = format!("{} {}", version.get("userAgent").and_then(Value::as_str).unwrap_or("Chromium"), crate::web::name());
        let _ = self.events.append("browser.started", None, json!({ "product": version.get("product") }));
        // News of every tab: its address and title as they change, and the tabs a page opens itself.
        let _ = cdp.call("Target.setDiscoverTargets", json!({ "discover": true }), None).await;
        *running = Some(Running { cdp, proxy, agent, places: HashMap::new() });
        Ok(())
    }

    /// A tab of the browser's own, attached and set up as Demido's: its name, its size.
    async fn set_up(cdp: &cdp::Cdp, agent: &str, target: &str, size: (u32, u32)) -> Result<Tab, String> {
        let attached = cdp.call("Target.attachToTarget", json!({ "targetId": target, "flatten": true }), None).await?;
        let page = attached.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string();
        cdp.call("Page.enable", json!({}), Some(&page)).await?;
        cdp.call("Emulation.setUserAgentOverride", json!({ "userAgent": agent }), Some(&page)).await?;
        cdp.call("Emulation.setDeviceMetricsOverride", json!({ "width": size.0, "height": size.1, "deviceScaleFactor": 1, "mobile": false }), Some(&page)).await?;
        Ok(Tab { target: target.to_string(), session: page, items: Vec::new(), address: String::new(), title: String::new(), picture: None })
    }

    /// A new tab in a place, in front.
    async fn new_tab(r: &mut Running, session: &str) -> Result<(), String> {
        let (cdp, agent) = (r.cdp.clone(), r.agent.clone());
        let place = r.places.get_mut(session).ok_or("no place")?;
        let target = cdp.call("Target.createTarget", json!({ "url": "about:blank", "browserContextId": place.context }), None).await?;
        let target = target.get("targetId").and_then(Value::as_str).unwrap_or_default().to_string();
        let tab = Self::set_up(&cdp, &agent, &target, place.size).await?;
        place.tabs.push(tab);
        place.active = place.tabs.len() - 1;
        Ok(())
    }

    /// The place of a session, made with one empty tab when it has none.
    async fn place<'a>(&self, running: &'a mut Option<Running>, session: &str) -> Result<&'a mut Place, String> {
        self.start(running).await?;
        let r = running.as_mut().expect("just started");
        if !r.places.contains_key(session) {
            let context = r.cdp.call("Target.createBrowserContext", json!({ "disposeOnDetach": true }), None).await?;
            let context = context.get("browserContextId").and_then(Value::as_str).unwrap_or_default().to_string();
            r.places.insert(session.to_string(), Place { context, tabs: Vec::new(), active: 0, used: Instant::now(), size: (WIDTH, HEIGHT) });
        }
        if r.places.get(session).is_some_and(|p| p.tabs.is_empty()) {
            Self::new_tab(r, session).await?;
        }
        let place = r.places.get_mut(session).expect("just made");
        place.used = Instant::now();
        Ok(place)
    }

    /// The tab in front of a session's place, made when it has none.
    async fn tab<'a>(&self, running: &'a mut Option<Running>, session: &str) -> Result<&'a mut Tab, String> {
        self.place(running, session).await?.front().ok_or_else(|| "the session has no tab".to_string())
    }

    /// Waits until the page has loaded and its scripts had a moment, or until it has taken long enough.
    async fn settle(cdp: &cdp::Cdp, page: &str, stop: &AtomicBool) -> Result<(), String> {
        let started = Instant::now();
        tokio::time::sleep(Duration::from_millis(300)).await;
        while started.elapsed() < LOADING_TAKES_AT_MOST {
            stopped(stop)?;
            let state = cdp.call("Runtime.evaluate", json!({ "expression": "document.readyState", "returnByValue": true }), Some(page)).await;
            if state.as_ref().ok().and_then(|s| s.pointer("/result/value")).and_then(Value::as_str) == Some("complete") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        // What a page's scripts write after it loaded.
        tokio::time::sleep(Duration::from_millis(700)).await;
        Ok(())
    }

    /// Runs a script in a world of Demido's own on the page, apart from the page's scripts.
    async fn in_our_world(cdp: &cdp::Cdp, page: &str, script: &str) -> Result<Value, String> {
        let frames = cdp.call("Page.getFrameTree", json!({}), Some(page)).await?;
        let frame = frames.pointer("/frameTree/frame/id").and_then(Value::as_str).unwrap_or_default();
        let world = cdp.call("Page.createIsolatedWorld", json!({ "frameId": frame, "worldName": "demido" }), Some(page)).await?;
        let context = world.get("executionContextId").cloned().unwrap_or(Value::Null);
        let got = cdp.call("Runtime.evaluate", json!({ "expression": script, "contextId": context, "returnByValue": true, "awaitPromise": true }), Some(page)).await?;
        if let Some(e) = got.get("exceptionDetails") {
            return Err(format!("the page could not be read ({})", e.pointer("/exception/description").and_then(Value::as_str).unwrap_or("a script failed")));
        }
        Ok(got.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    /// Reads the page as it is now, takes its picture, and remembers what it lists.
    async fn look(&self, running: &mut Option<Running>, session: &str, about: &str, picture: bool) -> Result<Seen, String> {
        let r = running.as_mut().ok_or("the browser has stopped")?;
        let tab = r.places.get_mut(session).and_then(Place::front).ok_or("the session has no page open")?;
        let seen = Self::in_our_world(&r.cdp, &tab.session, LOOK).await?;
        let items: Vec<Item> = serde_json::from_value(seen.get("items").cloned().unwrap_or(Value::Null)).unwrap_or_default();
        let address = seen.get("address").and_then(Value::as_str).unwrap_or_default().to_string();
        let html = seen.get("html").and_then(Value::as_str).unwrap_or_default().to_string();
        let (title, text) = self.web.told_html(&html, &address, about).await;
        let title = if title.is_empty() { seen.get("title").and_then(Value::as_str).unwrap_or_default().to_string() } else { title };
        let mut shot = None;
        if picture && let Ok(taken) = r.cdp.call("Page.captureScreenshot", json!({ "format": "jpeg", "quality": 60 }), Some(&tab.session)).await {
            shot = self.keep_picture(session, taken.get("data").and_then(Value::as_str).unwrap_or_default());
        }
        let gave = format!("{text}\n\n{}", listed(&items));
        (tab.items, tab.address, tab.title, tab.picture) = (items, address.clone(), title.clone(), shot.clone());
        Ok(Seen { address, title, gave, picture: shot })
    }

    /// Writes a picture into the session's pictures, keeping the newest only.
    fn keep_picture(&self, session: &str, base64: &str) -> Option<String> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(base64).ok()?;
        let dir = self.pictures.join(session);
        std::fs::create_dir_all(&dir).ok()?;
        let name = format!("{}.jpg", uuid::Uuid::now_v7());
        std::fs::write(dir.join(&name), bytes).ok()?;
        let mut all: Vec<PathBuf> = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).collect();
        all.sort();
        let over = all.len().saturating_sub(PICTURES_KEPT);
        for old in all.into_iter().take(over) {
            let _ = std::fs::remove_file(old);
        }
        Some(name)
    }

    /// A picture of a session's, by its name: only one Demido took.
    pub fn picture(&self, session: &str, name: &str) -> Option<Vec<u8>> {
        let plain = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') && !s.contains("..");
        (plain(session) && plain(name) && name.ends_with(".jpg")).then(|| std::fs::read(self.pictures.join(session).join(name)).ok()).flatten()
    }

    /// Opens an address in the session's page. Whether it may be opened at all was decided before (N2, N3);
    /// the proxy holds the browser to the public internet whatever the page then loads.
    pub async fn open(&self, session: &str, written: &str, about: &str, stop: &AtomicBool) -> Result<Seen, String> {
        let mut url = guard::address(written)?;
        url.set_fragment(None);
        if guard::is_local(&url) && !self.stand_ins.contains(&format!("{}:{}", url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or(0))) {
            return Err(format!("{} {}, and Demido's browser does not go there", url.host_str().unwrap_or_default(), guard::NOT_PUBLIC));
        }
        if !self.web.allowed(&url, stop).await {
            return Err("the site asks programs not to read this page (its robots.txt), so it was not opened".into());
        }
        let mut running = self.running.lock().await;
        let tab = self.tab(&mut running, session).await?;
        let page = tab.session.clone();
        let cdp = running.as_ref().expect("a tab means it runs").cdp.clone();
        let went = cdp.call("Page.navigate", json!({ "url": url.as_str() }), Some(&page)).await?;
        if let Some(error) = went.get("errorText").and_then(Value::as_str).filter(|e| !e.is_empty()) {
            return Err(why_not_loaded(error));
        }
        Self::settle(&cdp, &page, stop).await?;
        let seen = self.look(&mut running, session, about, true).await?;
        let _ = self.events.append("browser.opened", Some(session), json!({ "address": seen.address }));
        Ok(seen)
    }

    /// What the session's page lists under a number, as it was last read.
    pub async fn item(&self, session: &str, n: u32) -> Option<Item> {
        self.running.lock().await.as_mut()?.places.get_mut(session)?.front()?.items.iter().find(|i| i.n == n).cloned()
    }

    /// The numbered thing, found again on the page as it is now, scrolled into view: its middle, to press.
    async fn middle_of(cdp: &cdp::Cdp, page: &str, n: u32) -> Result<(f64, f64), String> {
        let at = Self::in_our_world(
            cdp,
            page,
            &format!("(() => {{ const el = document.querySelector('[data-demido-n=\"{n}\"]'); if (!el) return null; el.scrollIntoView({{ block: 'center', inline: 'center' }}); const r = el.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]; }})()"),
        )
        .await?;
        let x = at.get(0).and_then(Value::as_f64);
        let y = at.get(1).and_then(Value::as_f64);
        x.zip(y).ok_or_else(|| format!("number {n} is no longer on the page"))
    }

    fn no_such(&self, n: u32, tab: &Tab) -> String {
        if tab.items.is_empty() {
            format!("there is no number {n}: nothing on the page can be pressed or filled")
        } else {
            format!("there is no number {n} on the page: it lists 1 to {}", tab.items.len())
        }
    }

    /// Presses the numbered thing on the session's page, as a person's click would, and reads the page after.
    pub async fn press(&self, session: &str, n: u32, about: &str, stop: &AtomicBool) -> Result<Seen, String> {
        let mut running = self.running.lock().await;
        let r = running.as_mut().ok_or("no page is open in this session: open one first")?;
        let place = r.places.get_mut(session).ok_or("no page is open in this session: open one first")?;
        place.used = Instant::now();
        let tab = place.front().ok_or("no page is open in this session: open one first")?;
        let Some(item) = tab.items.iter().find(|i| i.n == n).cloned() else { return Err(self.no_such(n, tab)) };
        // A link leads to a page of its own: the site's rules for programs hold for it as for any.
        if let Some(href) = item.href.as_deref().and_then(|h| Url::parse(h).ok())
            && matches!(href.scheme(), "http" | "https")
            && !self.web.allowed(&href, stop).await
        {
            return Err("that link leads to a page its site asks programs not to read (its robots.txt), so it was not pressed".into());
        }
        let (cdp, page) = (r.cdp.clone(), tab.session.clone());
        let (x, y) = Self::middle_of(&cdp, &page, n).await?;
        for kind in ["mouseMoved", "mousePressed", "mouseReleased"] {
            cdp.call("Input.dispatchMouseEvent", json!({ "type": kind, "x": x, "y": y, "button": "left", "clickCount": 1 }), Some(&page)).await?;
        }
        Self::settle(&cdp, &page, stop).await?;
        let seen = self.look(&mut running, session, about, true).await?;
        let _ = self.events.append("browser.pressed", Some(session), json!({ "n": n, "kind": item.kind, "address": seen.address }));
        Ok(seen)
    }

    /// Types into the numbered field of the session's page, in place of what it held, and reads the page
    /// after. The one text field of a form is sent, as Enter would send it. A choice is set to the choice
    /// whose words are given. Never a password field.
    pub async fn type_text(&self, session: &str, n: u32, text: &str, about: &str, stop: &AtomicBool) -> Result<Seen, String> {
        let mut running = self.running.lock().await;
        let r = running.as_mut().ok_or("no page is open in this session: open one first")?;
        let place = r.places.get_mut(session).ok_or("no page is open in this session: open one first")?;
        place.used = Instant::now();
        let tab = place.front().ok_or("no page is open in this session: open one first")?;
        let Some(item) = tab.items.iter().find(|i| i.n == n).cloned() else { return Err(self.no_such(n, tab)) };
        if item.password {
            return Err(format!("number {n} is a password field, and Demido never types into one: nothing of the person's is kept for this, and a page that asks a model for a password is a trick"));
        }
        if !matches!(item.kind.as_str(), "field" | "choice") {
            return Err(format!("number {n} is a {} and cannot be typed into; press it instead", item.kind));
        }
        let (cdp, page) = (r.cdp.clone(), tab.session.clone());
        let quoted = serde_json::to_string(text).map_err(|e| e.to_string())?;
        if item.kind == "choice" {
            let chosen = Self::in_our_world(
                &cdp,
                &page,
                &format!("(() => {{ const el = document.querySelector('[data-demido-n=\"{n}\"]'); if (!el) return null; const want = {quoted}.trim().toLowerCase(); const o = [...el.options].find((o) => o.text.trim().toLowerCase() === want) || [...el.options].find((o) => o.text.trim().toLowerCase().includes(want)); if (!o) return false; el.value = o.value; el.dispatchEvent(new Event('input', {{ bubbles: true }})); el.dispatchEvent(new Event('change', {{ bubbles: true }})); return o.text; }})()"),
            )
            .await?;
            if chosen == Value::Bool(false) {
                return Err(format!("number {n} has no choice \"{text}\"; it offers: {}", item.choices.join(", ")));
            }
        } else {
            let focused = Self::in_our_world(
                &cdp,
                &page,
                &format!("(() => {{ const el = document.querySelector('[data-demido-n=\"{n}\"]'); if (!el) return false; el.scrollIntoView({{ block: 'center' }}); el.focus(); if (el.select) el.select(); else if (el.isContentEditable) document.execCommand('selectAll'); return document.activeElement === el; }})()"),
            )
            .await?;
            if focused != Value::Bool(true) {
                return Err(format!("number {n} could not be typed into"));
            }
            cdp.call("Input.insertText", json!({ "text": text }), Some(&page)).await?;
            if item.alone {
                for kind in ["keyDown", "keyUp"] {
                    cdp.call(
                        "Input.dispatchKeyEvent",
                        json!({ "type": kind, "key": "Enter", "code": "Enter", "windowsVirtualKeyCode": 13, "text": if kind == "keyDown" { "\r" } else { "" } }),
                        Some(&page),
                    )
                    .await?;
                }
            }
        }
        Self::settle(&cdp, &page, stop).await?;
        let seen = self.look(&mut running, session, about, true).await?;
        let _ = self.events.append("browser.typed", Some(session), json!({ "n": n, "characters": text.chars().count(), "sent": item.alone, "address": seen.address }));
        Ok(seen)
    }

    /// Reads a page whose text only shows after its scripts ran: `read_page`'s fallback (Z9). In a context of
    /// its own that is closed after, with no picture.
    pub async fn read(&self, address: &str, about: &str, stop: &AtomicBool) -> Result<Seen, String> {
        let seen = self.open(ONLY_READING, address, about, stop).await;
        self.close(ONLY_READING).await;
        seen.map(|s| Seen { picture: None, ..s })
    }

    // ───────── what the person does, from the window (live) ─────────

    /// The tab in front of a session, for the window to show: the browser, its pipe, the page's session and its
    /// target. Started, and the place made, when there is none. The window's view counts as use.
    pub async fn front(&self, session: &str) -> Result<(Arc<cdp::Cdp>, String, String), String> {
        let mut running = self.running.lock().await;
        let tab = self.tab(&mut running, session).await?;
        let (page, target) = (tab.session.clone(), tab.target.clone());
        Ok((running.as_ref().expect("a tab means it runs").cdp.clone(), page, target))
    }

    /// The tab in front, if the session has a place: without starting anything.
    pub async fn front_if_any(&self, session: &str) -> Option<(String, String)> {
        let mut running = self.running.lock().await;
        let place = running.as_mut()?.places.get_mut(session)?;
        place.used = Instant::now();
        place.front().map(|t| (t.session.clone(), t.target.clone()))
    }

    /// The tabs of a session, and which is in front.
    pub async fn tabs(&self, session: &str) -> (Vec<TabFacts>, Option<String>) {
        let mut running = self.running.lock().await;
        let Some(place) = running.as_mut().and_then(|r| r.places.get_mut(session)) else { return (Vec::new(), None) };
        let front = place.front().map(|t| t.target.clone());
        (place.tabs.iter().map(|t| TabFacts { id: t.target.clone(), address: t.address.clone(), title: t.title.clone() }).collect(), front)
    }

    /// What the browser says of a tab now: its address and title, kept for the window and the status.
    pub async fn heard(&self, target: &str, address: &str, title: &str) {
        let mut running = self.running.lock().await;
        if let Some(t) = running.as_mut().into_iter().flat_map(|r| r.places.values_mut()).flat_map(|p| p.tabs.iter_mut()).find(|t| t.target == target) {
            (t.address, t.title) = (address.to_string(), title.to_string());
        }
    }

    /// A new tab, in front, opened at an address the person typed if there is one.
    pub async fn open_tab(&self, session: &str, address: Option<&str>) -> Result<(), String> {
        let url = address.map(|a| self.person_may_go(a)).transpose()?;
        let mut running = self.running.lock().await;
        self.place(&mut running, session).await?;
        let r = running.as_mut().expect("a place means it runs");
        Self::new_tab(r, session).await?;
        if let Some(url) = url {
            let page = r.places.get_mut(session).and_then(Place::front).map(|t| t.session.clone()).unwrap_or_default();
            r.cdp.call("Page.navigate", json!({ "url": url.as_str() }), Some(&page)).await?;
        }
        Ok(())
    }

    /// Puts a tab in front.
    pub async fn activate(&self, session: &str, target: &str) -> bool {
        let mut running = self.running.lock().await;
        let Some(place) = running.as_mut().and_then(|r| r.places.get_mut(session)) else { return false };
        match place.tabs.iter().position(|t| t.target == target) {
            Some(at) => {
                place.active = at;
                place.used = Instant::now();
                true
            }
            None => false,
        }
    }

    /// Closes one tab; the last one closed closes the place, with its cookies.
    pub async fn close_tab(&self, session: &str, target: &str) {
        let last = {
            let mut running = self.running.lock().await;
            let Some(r) = running.as_mut() else { return };
            let Some(place) = r.places.get_mut(session) else { return };
            let Some(at) = place.tabs.iter().position(|t| t.target == target) else { return };
            place.tabs.remove(at);
            if place.active >= at && place.active > 0 {
                place.active -= 1;
            }
            let _ = r.cdp.call("Target.closeTarget", json!({ "targetId": target }), None).await;
            r.places.get(session).is_some_and(|p| p.tabs.is_empty())
        };
        if last {
            self.close(session).await;
        }
    }

    /// A tab a page of the session's opened itself (a link to a new window): taken in, and put in front.
    pub async fn adopt(&self, session: &str, target: &str) -> Result<(), String> {
        let mut running = self.running.lock().await;
        let r = running.as_mut().ok_or("the browser has stopped")?;
        let (cdp, agent) = (r.cdp.clone(), r.agent.clone());
        let place = r.places.get_mut(session).ok_or("no place")?;
        if place.tabs.iter().any(|t| t.target == target) {
            return Ok(());
        }
        let tab = Self::set_up(&cdp, &agent, target, place.size).await?;
        place.tabs.push(tab);
        place.active = place.tabs.len() - 1;
        Ok(())
    }

    /// Whether a target is one of a session's tabs, or was opened by one of them: (its own, opened by it).
    pub async fn knows(&self, session: &str, target: &str, opener: Option<&str>, context: Option<&str>) -> (bool, bool) {
        let running = self.running.lock().await;
        let Some(place) = running.as_ref().and_then(|r| r.places.get(session)) else { return (false, false) };
        let own = place.tabs.iter().any(|t| t.target == target);
        let opened = !own && context == Some(place.context.as_str()) && opener.is_some_and(|o| place.tabs.iter().any(|t| t.target == o));
        (own, opened)
    }

    /// The size the session's pages are laid out at: set by the window's view of them.
    pub async fn resize(&self, session: &str, width: u32, height: u32) {
        let size = (width.clamp(320, 3840), height.clamp(240, 2400));
        let mut running = self.running.lock().await;
        let Some(r) = running.as_mut() else { return };
        let cdp = r.cdp.clone();
        if let Some(place) = r.places.get_mut(session) {
            place.size = size;
            for t in &place.tabs {
                let _ = cdp.call("Emulation.setDeviceMetricsOverride", json!({ "width": size.0, "height": size.1, "deviceScaleFactor": 1, "mobile": false }), Some(&t.session)).await;
            }
        }
    }

    /// Where the person may send a tab: an address of the public internet (N3), as the proxy holds it to anyway.
    /// No robots.txt: the person is browsing, not a program reading.
    pub fn person_may_go(&self, written: &str) -> Result<Url, String> {
        let written = written.trim();
        // Words that are not an address are a search, on the search engine Demido asks without a key.
        let an_address = written.contains("://") || (!written.contains(char::is_whitespace) && (written.contains('.') || written.contains(':') || written.eq_ignore_ascii_case("localhost")));
        if !an_address {
            return Url::parse_with_params("https://duckduckgo.com/", &[("q", written)]).map_err(|e| e.to_string());
        }
        let with_scheme = if written.contains("://") { written.to_string() } else { format!("https://{written}") };
        let url = guard::address(&with_scheme)?;
        if guard::is_local(&url) && !self.stand_ins.contains(&format!("{}:{}", url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or(0))) {
            return Err(format!("{} {}, and Demido's browser does not go there", url.host_str().unwrap_or_default(), guard::NOT_PUBLIC));
        }
        Ok(url)
    }

    /// Describes what is on the page at a point, or the smallest thing that holds a box: for the person's
    /// marks, sent to the conversation as words.
    pub async fn describe(cdp: &cdp::Cdp, page: &str, x: f64, y: f64, w: f64, h: f64) -> Result<Value, String> {
        Self::in_our_world(cdp, page, &format!("({})({x}, {y}, {w}, {h})", live::DESCRIBE)).await
    }

    /// Closes a session's page and forgets its cookies.
    pub async fn close(&self, session: &str) {
        let mut running = self.running.lock().await;
        if let Some(r) = running.as_mut()
            && let Some(place) = r.places.remove(session)
        {
            let _ = r.cdp.call("Target.disposeBrowserContext", json!({ "browserContextId": place.context }), None).await;
        }
    }

    /// Closes what was left alone too long, and the browser when nothing is open: called now and then.
    pub async fn tidy(&self) {
        let mut running = self.running.lock().await;
        let Some(r) = running.as_mut() else { return };
        let old: Vec<String> = r.places.iter().filter(|(_, p)| p.used.elapsed() > IDLE).map(|(s, _)| s.clone()).collect();
        for session in old {
            if let Some(place) = r.places.remove(&session) {
                let _ = r.cdp.call("Target.disposeBrowserContext", json!({ "browserContextId": place.context }), None).await;
            }
        }
        if r.places.is_empty() || !r.cdp.running() {
            let r = running.take().expect("just looked");
            r.cdp.end().await;
            let _ = self.events.append("browser.stopped", None, json!({}));
        }
    }

    /// Ends the browser: when Demido stops.
    pub async fn end(&self) {
        if let Some(r) = self.running.lock().await.take() {
            r.cdp.end().await;
        }
    }

    /// Forgets a session's pictures: when the session is deleted.
    pub async fn forget(&self, session: &str) {
        self.close(session).await;
        let _ = std::fs::remove_dir_all(self.pictures.join(session));
    }
}

#[cfg(test)]
mod tests;
