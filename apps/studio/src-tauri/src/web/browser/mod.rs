//! The built-in browser: web pages in tabs inside the app's window, for the person and for the
//! assistant.
//!
//! A tab is a webview of its own, laid over the app's page where the Browser window's content
//! area is (see [`place`]); the Browser window itself is part of the page and draws the tab strip,
//! the address bar and the frame around it. Every tab shares one browser profile of its own
//! (`webview-browser` in the data folder), apart from the app's page: a sign-in the person makes
//! in any tab holds in all of them, the assistant's included.
//!
//! The person's tabs are theirs. Each chat that browses gets one tab of its own, which web
//! searches and fetches render pages in when plain HTTP is refused, and which the `browser_*`
//! tools drive; it shows in the tab strip like any other, so the person can watch it, take over,
//! or solve a check the assistant cannot ([`Browser::ask`]).
//!
//! A tab may not go to the app's own pages, which the app's commands answer, nor anywhere but the
//! web: navigation to any other scheme is refused.

pub mod input;
mod place;

use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{Value, json};
#[cfg(not(target_os = "linux"))]
use tauri::webview::DownloadEvent;
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, Manager, Webview, WebviewBuilder, WebviewUrl};
use tokio::sync::{oneshot, watch};
use tokio_util::sync::CancellationToken;
use url::Url;

pub use place::Rect;

use self::input::{Button, Key};
use super::page::{self, Check};

/// The tab list, sent to the UI whenever it changes.
pub const TABS_EVENT: &str = "browser://tabs";
/// A check or sign-in the assistant needs the person for, or `null` when there is none.
pub const ATTENTION_EVENT: &str = "browser://attention";
/// A download a tab finished.
pub const DOWNLOAD_EVENT: &str = "browser://download";

const PAGE_JS: &str = include_str!("page.js");

/// A call of one of `page.js`'s actions. The file is one function, which a formatter ends with a
/// `;` that would end the statement before the call.
fn page_call(action: &str, args: &Value) -> String {
    format!(
        "{}({}, {args})",
        PAGE_JS.trim_end().trim_end_matches(';'),
        json!(action)
    )
}

/// A call of `page.js`'s `run` with a script of the assistant's, `code`: an async function.
fn script_call(args: &Value, code: &str) -> String {
    format!("{}(\"run\", {args}, {code})", PAGE_JS.trim_end().trim_end_matches(';'))
}

/// Why the assistant does not type where the page has the keyboard: as `page.js` says it.
const SECRET: &str = "The field with the keyboard asks for a password or payment details, which only the user types: \
                      ask them with browser_ask_user.";

/// The size of an assistant's tab nobody has looked at: a desktop page.
const DEFAULT_SIZE: (f64, f64) = (1280.0, 900.0);
/// Chats browsing at once. The tab of the chat that browsed longest ago closes for a new one.
const MAX_CHAT_TABS: usize = 4;
/// A chat's tab nobody uses for this long closes when another chat starts browsing.
const IDLE: Duration = Duration::from_secs(20 * 60);
/// How long a chat's tab out of sight keeps getting frames after the assistant last used it: longer
/// than a model takes to think between two calls, not so long that a video on the page goes on
/// costing the person's computer (see [`place`]).
const IN_USE: Duration = Duration::from_secs(60);
/// How long a page that went without frames gets to catch up before the assistant reads it.
const CATCH_UP: Duration = Duration::from_secs(3);

/// Who a tab is for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Owner {
    User,
    Chat {
        #[serde(rename = "chatId")]
        chat_id: String,
    },
}

/// A tab as the UI shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabInfo {
    pub id: u64,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub owner: Owner,
    pub active: bool,
}

/// What the assistant asks the person for, as the UI shows it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttentionInfo {
    pub tab: u64,
    pub chat_id: String,
    pub message: String,
    /// When the assistant stops waiting, in milliseconds since 1970.
    pub until: i64,
}

/// How a request for the person ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The person said they are done.
    Done,
    /// The person said to go on without them.
    Skipped,
    /// The check went away (the person solved it, or it passed by itself).
    Cleared,
    TimedOut,
}

#[derive(Clone, Debug, Default)]
struct Load {
    /// Page loads started in the tab.
    started: u64,
    loading: bool,
    /// What the last page turned out to be when it was a file to download, which the assistant's
    /// tabs leave alone: its type or name.
    file: Option<String>,
}

struct Tab {
    id: u64,
    owner: Owner,
    webview: Webview,
    url: String,
    title: String,
    load: watch::Sender<Load>,
    /// The size it last showed at, kept while parked.
    size: (f64, f64),
    /// Where it is now, to place it only when that changes.
    at: Option<Rect>,
    used: Instant,
    /// One action of the assistant's at a time in this tab.
    lease: Arc<tokio::sync::Mutex<()>>,
    /// CSS pixels of the page per pixel of the last screenshot the assistant took of it, by which
    /// a point it names in that screenshot is found in the page.
    shot_scale: f64,
}

impl Tab {
    fn info(&self, active: Option<u64>) -> TabInfo {
        TabInfo {
            id: self.id,
            url: self.url.clone(),
            title: self.title.clone(),
            loading: self.load.borrow().loading,
            owner: self.owner.clone(),
            active: active == Some(self.id),
        }
    }

    /// Whether the assistant is using this tab, or was a moment ago.
    fn in_use(&self) -> bool {
        matches!(self.owner, Owner::Chat { .. }) && (self.used.elapsed() < IN_USE || self.lease.try_lock().is_err())
    }

    /// Whether it is where it gets no frames.
    fn without_frames(&self) -> bool {
        self.at.is_some_and(|at| at.x == place::PARKED_X)
    }
}

struct Attention {
    info: AttentionInfo,
    answer: Option<oneshot::Sender<bool>>,
}

#[derive(Default)]
struct Inner {
    tabs: Vec<Tab>,
    /// The tab the Browser window shows.
    active: Option<u64>,
    /// Where the Browser window shows it; `None` while the window is closed, minimized or covered.
    place: Option<Rect>,
    next: u64,
    attention: Option<Attention>,
    /// The app's page is in its overlay (Linux, see [`place`]).
    adopted: bool,
    /// A task is parking the tabs the assistant stopped using ([`Browser::park_when_unused`]).
    parking: bool,
    /// Where each chat's tab was when it was closed, by chat, until the chat opens another: for
    /// the assistant to go back there, as the page numbers it had are gone with the tab.
    closed: HashMap<String, String>,
}

/// A page the browser rendered, for reading.
pub struct Rendered {
    pub url: String,
    pub title: String,
    /// The page's type, such as `text/html`, or `application/pdf` for a PDF in the browser's viewer.
    pub kind: String,
    pub html: String,
}

/// The state of a tab's page.
pub struct PageState {
    /// The bot check that holds it, if one does.
    pub check: Option<Check>,
}

pub struct Browser {
    app: AppHandle,
    profile: PathBuf,
    downloads: PathBuf,
    me: Weak<Browser>,
    inner: Mutex<Inner>,
    /// Tabs are made one at a time.
    creating: tokio::sync::Mutex<()>,
    /// One request for the person at a time.
    asking: tokio::sync::Mutex<()>,
    /// Whether the web context the tabs share has the download handler (see [`handle_downloads`]).
    #[cfg(target_os = "linux")]
    downloads_handled: AtomicBool,
}

impl Browser {
    pub fn new(app: AppHandle, profile: PathBuf) -> Arc<Self> {
        let downloads = dirs::download_dir().unwrap_or_else(|| profile.join("downloads"));
        Arc::new_cyclic(|me| Browser {
            app,
            profile,
            downloads,
            me: me.clone(),
            inner: Mutex::new(Inner::default()),
            creating: tokio::sync::Mutex::new(()),
            asking: tokio::sync::Mutex::new(()),
            #[cfg(target_os = "linux")]
            downloads_handled: AtomicBool::new(false),
        })
    }

    /// The app's own page. By webview: once tabs are in its window, Tauri no longer calls that
    /// window a "webview window" (`get_webview_window` finds nothing).
    fn page(&self) -> Option<Webview> {
        self.app.get_webview("main")
    }

    pub fn tabs(&self) -> Vec<TabInfo> {
        let inner = self.inner.lock();
        inner.tabs.iter().map(|t| t.info(inner.active)).collect()
    }

    pub fn attention(&self) -> Option<AttentionInfo> {
        self.inner.lock().attention.as_ref().map(|a| a.info.clone())
    }

    fn changed(&self) {
        let _ = self.app.emit(TABS_EVENT, self.tabs());
    }

    /// Opens a tab for `owner` at `url` (a blank one without), shown in the Browser window if
    /// `activate`.
    pub async fn open(&self, owner: Owner, url: Option<Url>, activate: bool) -> Result<u64, String> {
        if let Some(url) = &url {
            allowed(url)?;
        }
        let _one = self.creating.lock().await;
        let page = self.page().ok_or("The app's window is not open.")?;
        let window = page.window();
        let id = {
            let mut inner = self.inner.lock();
            inner.next += 1;
            if !inner.adopted {
                place::adopt_page(&page);
                inner.adopted = true;
            }
            inner.next
        };
        let (load, _) = watch::channel(Load::default());
        // The person's tabs download files; the assistant's note what the file was and leave it.
        let refuse = (!matches!(owner, Owner::User)).then(|| load.clone());
        let start = url
            .clone()
            .unwrap_or_else(|| Url::parse("about:blank").expect("a valid URL"));
        let builder = self.builder(id, start, refuse.clone());
        let created = tokio::task::spawn_blocking(move || {
            window.add_child(
                builder,
                tauri::LogicalPosition::new(place::PARKED_X, 0.0),
                tauri::LogicalSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1),
            )
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("The browser could not open a tab: {e}"))?;
        let parked = Rect {
            x: place::PARKED_X,
            y: 0.0,
            width: DEFAULT_SIZE.0,
            height: DEFAULT_SIZE.1,
        };
        place::adopt(&created, parked);
        input::menus_for_people_only(&created);
        #[cfg(target_os = "linux")]
        {
            download_what_cannot_show(&created, refuse);
            if !self.downloads_handled.swap(true, Ordering::Relaxed) {
                handle_downloads(&created, self.me.clone());
            }
        }
        {
            let mut inner = self.inner.lock();
            inner.tabs.push(Tab {
                id,
                owner,
                webview: created,
                url: url.map(String::from).unwrap_or_default(),
                title: String::new(),
                load,
                size: DEFAULT_SIZE,
                at: Some(parked),
                used: Instant::now(),
                lease: Arc::new(tokio::sync::Mutex::new(())),
                shot_scale: 1.0,
            });
            if activate || inner.active.is_none() {
                inner.active = Some(id);
            }
            self.arrange(&mut inner);
            self.park_when_unused(&mut inner);
        }
        self.changed();
        Ok(id)
    }

    fn builder(
        &self,
        id: u64,
        start: Url,
        #[cfg_attr(target_os = "linux", allow(unused_variables))] refuse: Option<watch::Sender<Load>>,
    ) -> WebviewBuilder<tauri::Wry> {
        let label = format!("browser-{id}");
        let (loads, titles, popups) = (self.me.clone(), self.me.clone(), self.me.clone());
        let builder = WebviewBuilder::new(label, WebviewUrl::External(start))
            .data_directory(self.profile.clone())
            .focused(false)
            .on_navigation(|url| {
                let ok = allowed(url).is_ok() || url.as_str() == "about:blank";
                if !ok {
                    tracing::info!(%url, "a browser tab may not go there; refused");
                }
                ok
            })
            .on_page_load(move |webview, payload| {
                if let Some(b) = loads.upgrade() {
                    let url = webview
                        .url()
                        .map(String::from)
                        .unwrap_or_else(|_| payload.url().to_string());
                    b.loaded(id, url, payload.event() == PageLoadEvent::Started);
                }
            })
            .on_document_title_changed(move |webview, title| {
                if let Some(b) = titles.upgrade() {
                    b.titled(id, title, webview.url().ok().map(String::from));
                }
            })
            .on_new_window(move |url, _| {
                if let Some(b) = popups.upgrade() {
                    b.popup(id, url);
                }
                NewWindowResponse::Deny
            });
        // On Linux the tabs' downloads are handled where they share them (see `handle_downloads`).
        #[cfg(not(target_os = "linux"))]
        let builder = {
            let downloads = self.me.clone();
            builder.on_download(move |_, event| match event {
                DownloadEvent::Requested { url, destination } => {
                    if let Some(load) = &refuse {
                        let name = destination.file_name().map(|n| n.to_string_lossy().into_owned());
                        load.send_modify(|l| l.file = Some(name.unwrap_or_else(|| "a file".into())));
                        return false;
                    }
                    let Some(b) = downloads.upgrade() else { return false };
                    *destination = b.download_path(&url, destination);
                    tracing::info!(%url, to = %destination.display(), "browser download");
                    true
                }
                DownloadEvent::Finished { url, path, success } => {
                    if let Some(b) = downloads.upgrade() {
                        b.download_finished(&url, path, success);
                    }
                    true
                }
                _ => true,
            })
        };
        builder
    }

    fn loaded(&self, id: u64, url: String, started: bool) {
        {
            let mut inner = self.inner.lock();
            let Some(tab) = inner.tabs.iter_mut().find(|t| t.id == id) else {
                return;
            };
            tab.url = url;
            tab.load.send_modify(|l| {
                if started {
                    l.started += 1;
                    l.file = None;
                }
                l.loading = started;
            });
        }
        self.changed();
    }

    fn titled(&self, id: u64, title: String, url: Option<String>) {
        {
            let mut inner = self.inner.lock();
            let Some(tab) = inner.tabs.iter_mut().find(|t| t.id == id) else {
                return;
            };
            tab.title = title;
            if let Some(url) = url {
                tab.url = url;
            }
        }
        self.changed();
    }

    /// A page asked for a new window: the assistant's tab goes there itself; the person gets a
    /// new tab.
    fn popup(&self, from: u64, url: Url) {
        if allowed(&url).is_err() {
            return;
        }
        let Some(me) = self.me.upgrade() else { return };
        let owner = self
            .inner
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == from)
            .map(|t| t.owner.clone());
        tauri::async_runtime::spawn(async move {
            match owner {
                Some(Owner::Chat { .. }) => {
                    let _ = me.navigate(from, url);
                }
                _ => {
                    if let Err(e) = me.open(Owner::User, Some(url), true).await {
                        tracing::warn!("{e}");
                    }
                }
            }
        });
    }

    fn download_finished(&self, url: &Url, path: Option<PathBuf>, success: bool) {
        let _ = self.app.emit(
            DOWNLOAD_EVENT,
            json!({"url": url.as_str(), "path": path, "success": success}),
        );
    }

    /// Whether `path` is a file the browser downloaded (in the folder it downloads to).
    pub fn downloaded(&self, path: &std::path::Path) -> bool {
        match (path.canonicalize(), self.downloads.canonicalize()) {
            (Ok(file), Ok(dir)) => file.starts_with(dir) && file.is_file(),
            _ => false,
        }
    }

    /// Where a download goes: the Downloads folder, under a name no other file there has.
    fn download_path(&self, url: &Url, suggested: &std::path::Path) -> PathBuf {
        let name = suggested
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .map(str::to_string)
            .or_else(|| url.path_segments().and_then(|mut s| s.next_back()).map(str::to_string))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "download".into());
        let name: String = name
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
            .collect();
        let _ = std::fs::create_dir_all(&self.downloads);
        let path = std::path::Path::new(&name);
        let (stem, ext) = (
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("download")
                .to_string(),
            path.extension()
                .and_then(|s| s.to_str())
                .map(|e| format!(".{e}"))
                .unwrap_or_default(),
        );
        let mut candidate = self.downloads.join(&name);
        let mut n = 0;
        while candidate.exists() {
            n += 1;
            candidate = self.downloads.join(format!("{stem} ({n}){ext}"));
        }
        candidate
    }

    pub fn close(&self, id: u64) {
        let closed = {
            let mut inner = self.inner.lock();
            let Some(i) = inner.tabs.iter().position(|t| t.id == id) else {
                return;
            };
            let tab = inner.tabs.remove(i);
            if let Owner::Chat { chat_id } = &tab.owner
                && !tab.url.is_empty()
            {
                inner.closed.insert(chat_id.clone(), tab.url.clone());
            }
            if inner.active == Some(id) {
                // The neighbour of the closed tab, the person's own first.
                let users: Vec<u64> = inner
                    .tabs
                    .iter()
                    .filter(|t| t.owner == Owner::User)
                    .map(|t| t.id)
                    .collect();
                inner.active = users
                    .get(i.min(users.len().saturating_sub(1)))
                    .copied()
                    .or_else(|| inner.tabs.first().map(|t| t.id));
            }
            if inner.attention.as_ref().is_some_and(|a| a.info.tab == id)
                && let Some(mut a) = inner.attention.take()
                && let Some(answer) = a.answer.take()
            {
                let _ = answer.send(false);
            }
            self.arrange(&mut inner);
            tab
        };
        place::forget(&closed.webview);
        if let Err(e) = closed.webview.close() {
            tracing::warn!("could not close a browser tab: {e}");
        }
        self.changed();
    }

    /// Closes the tab of `chat`, when the chat goes.
    pub fn close_chat(&self, chat: &str) {
        let id = self.tab_of(chat);
        if let Some(id) = id {
            self.close(id);
        }
        self.inner.lock().closed.remove(chat);
    }

    /// Where the tab of `chat` was when it was closed, while the chat has none.
    pub fn closed_at(&self, chat: &str) -> Option<String> {
        self.inner.lock().closed.get(chat).cloned()
    }

    /// The tab of `chat`, if it has one.
    pub fn tab_of(&self, chat: &str) -> Option<u64> {
        self.inner
            .lock()
            .tabs
            .iter()
            .find(|t| matches!(&t.owner, Owner::Chat { chat_id } if chat_id == chat))
            .map(|t| t.id)
    }

    pub fn activate(&self, id: u64) {
        {
            let mut inner = self.inner.lock();
            if !inner.tabs.iter().any(|t| t.id == id) {
                return;
            }
            inner.active = Some(id);
            self.arrange(&mut inner);
        }
        self.changed();
    }

    /// Where the Browser window shows its tab, or `None` to show none.
    pub fn place(&self, at: Option<Rect>) {
        let mut inner = self.inner.lock();
        let at = at.filter(|r| r.width >= 1.0 && r.height >= 1.0);
        if inner.place == at {
            return;
        }
        let hiding = inner.place.is_some() && at.is_none();
        inner.place = at;
        self.arrange(&mut inner);
        if hiding && let Some(page) = self.page() {
            place::focus_page(&page);
        }
    }

    /// Puts the active tab where the window shows it and every other out of sight, with frames if
    /// the assistant is using it.
    fn arrange(&self, inner: &mut Inner) {
        let (active, shown) = (inner.active, inner.place);
        for tab in &mut inner.tabs {
            let at = match shown {
                Some(r) if active == Some(tab.id) => {
                    tab.size = (r.width, r.height);
                    r
                }
                _ => {
                    let size = Rect {
                        x: 0.0,
                        y: 0.0,
                        width: tab.size.0,
                        height: tab.size.1,
                    };
                    if tab.in_use() { size.awake() } else { size.parked() }
                }
            };
            if tab.at != Some(at) {
                place::set(&tab.webview, at);
                tab.at = Some(at);
            }
        }
    }

    /// Sees that tabs the assistant stopped using go without frames again.
    fn park_when_unused(&self, inner: &mut Inner) {
        if inner.parking || !inner.tabs.iter().any(Tab::in_use) {
            return;
        }
        inner.parking = true;
        let me = self.me.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let Some(b) = me.upgrade() else {
                    return;
                };
                let mut inner = b.inner.lock();
                b.arrange(&mut inner);
                if !inner.tabs.iter().any(Tab::in_use) {
                    inner.parking = false;
                    return;
                }
            }
        });
    }

    fn webview(&self, id: u64) -> Result<Webview, String> {
        self.inner
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.webview.clone())
            .ok_or_else(|| "That tab was closed.".to_string())
    }

    /// Goes to `url` in tab `id`, without waiting.
    pub fn navigate(&self, id: u64, url: Url) -> Result<(), String> {
        allowed(&url)?;
        self.webview(id)?.navigate(url).map_err(|e| e.to_string())
    }

    pub fn reload(&self, id: u64) -> Result<(), String> {
        self.webview(id)?.reload().map_err(|e| e.to_string())
    }

    /// Back, forward or stop, by the page's own history.
    pub fn history(&self, id: u64, action: &str) -> Result<(), String> {
        let js = page_call(action, &json!({}));
        self.webview(id)?.eval(js).map_err(|e| e.to_string())
    }

    /// Forgets every cookie, sign-in and cached file of the browser's profile.
    pub async fn clear_data(&self) -> Result<(), String> {
        let webview = self.inner.lock().tabs.first().map(|t| t.webview.clone());
        if let Some(w) = webview {
            return w.clear_all_browsing_data().map_err(|e| e.to_string());
        }
        // The profile is reached through a tab of it: a blank one, closed again.
        let id = self.open(Owner::User, None, false).await?;
        let cleared = self
            .webview(id)
            .and_then(|w| w.clear_all_browsing_data().map_err(|e| e.to_string()));
        self.close(id);
        cleared
    }

    // ---- The assistant's tabs ------------------------------------------------------------

    /// The tab of `chat`, opened if it has none.
    pub async fn chat_tab_or_open(&self, chat: &str) -> Result<u64, String> {
        if let Some(id) = self.tab_of(chat) {
            self.touch(id);
            return Ok(id);
        }
        for id in self.to_retire() {
            self.close(id);
        }
        self.inner.lock().closed.remove(chat);
        self.open(
            Owner::Chat {
                chat_id: chat.to_string(),
            },
            None,
            false,
        )
        .await
    }

    /// The chats' tabs to close for a new one: those idle too long, and the oldest beyond the
    /// limit; never one the window shows or the person is asked about.
    fn to_retire(&self) -> Vec<u64> {
        let inner = self.inner.lock();
        let busy = |t: &&Tab| {
            (inner.active == Some(t.id) && inner.place.is_some())
                || inner.attention.as_ref().is_some_and(|a| a.info.tab == t.id)
        };
        let mut chats: Vec<&Tab> = inner
            .tabs
            .iter()
            .filter(|t| matches!(t.owner, Owner::Chat { .. }))
            .filter(|t| !busy(t))
            .collect();
        chats.sort_by_key(|t| t.used);
        let mut out: Vec<u64> = chats.iter().filter(|t| t.used.elapsed() > IDLE).map(|t| t.id).collect();
        let total = inner
            .tabs
            .iter()
            .filter(|t| matches!(t.owner, Owner::Chat { .. }))
            .count();
        let mut over = (total + 1).saturating_sub(MAX_CHAT_TABS).saturating_sub(out.len());
        for t in chats {
            if over == 0 {
                break;
            }
            if !out.contains(&t.id) {
                out.push(t.id);
                over -= 1;
            }
        }
        out
    }

    /// Marks tab `id` as used now, which gives it frames where a tab out of sight can have them.
    /// Whether this is what gave them.
    fn touch(&self, id: u64) -> bool {
        let mut inner = self.inner.lock();
        let Some(t) = inner.tabs.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        let without = t.without_frames();
        t.used = Instant::now();
        self.arrange(&mut inner);
        self.park_when_unused(&mut inner);
        without && inner.tabs.iter().any(|t| t.id == id && !t.without_frames())
    }

    /// Holds tab `id` for one of the assistant's actions.
    pub async fn lease(&self, id: u64) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
        let lease = self
            .inner
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.lease.clone())
            .ok_or("That tab was closed.")?;
        let woken = self.touch(id);
        let held = lease.lock_owned().await;
        if woken {
            self.settle(id, CATCH_UP).await;
        }
        Ok(held)
    }

    /// Goes to `url` in tab `id` and waits for the page to load and settle.
    pub async fn load(&self, id: u64, url: &Url, timeout: Duration) -> Result<(), String> {
        allowed(url)?;
        let mut load = self.load_watch(id)?;
        let before = load.borrow().started;
        self.webview(id)?.navigate(url.clone()).map_err(|e| e.to_string())?;
        wait_loaded(&mut load, before, Duration::from_secs(8), timeout).await;
        if let Some(file) = load.borrow().file.clone() {
            return Err(format!(
                "{url} is a file ({file}), not a page, and the assistant's browser tabs do not download files."
            ));
        }
        self.settle(id, Duration::from_secs(5)).await;
        Ok(())
    }

    fn load_watch(&self, id: u64) -> Result<watch::Receiver<Load>, String> {
        self.inner
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.load.subscribe())
            .ok_or_else(|| "That tab was closed.".to_string())
    }

    /// Runs one of `page.js`'s actions in tab `id`, then waits for what it set off: a new page
    /// to load, or the page to stop changing.
    pub async fn act(&self, id: u64, action: &str, args: Value) -> Result<Value, String> {
        let Acted { mut done, file, .. } = self
            .acting(id, self.run(id, action, args, Duration::from_secs(10)))
            .await?;
        if let (Some(file), Some(o)) = (file, done.as_object_mut()) {
            o.insert("file".into(), file.into());
        }
        Ok(done)
    }

    /// Does `action` in tab `id`, then waits for what it set off: a new page to load, or the page
    /// to stop changing.
    async fn acting<T>(&self, id: u64, action: impl Future<Output = Result<T, String>>) -> Result<Acted<T>, String> {
        let mut load = self.load_watch(id)?;
        let before = load.borrow().started;
        let done = action.await?;
        let loaded = wait_loaded(&mut load, before, Duration::from_millis(700), Duration::from_secs(20)).await;
        let file = if loaded { load.borrow().file.clone() } else { None };
        self.settle(id, Duration::from_secs(if loaded { 5 } else { 3 })).await;
        Ok(Acted { done, loaded, file })
    }

    /// Clicks `count` times with `button` at `x`, `y` of tab `id`'s view, in CSS pixels, as the
    /// person's mouse would, and waits for what it set off. What a new page turned out to be when
    /// it was a file.
    pub async fn click_at(&self, id: u64, x: f64, y: f64, button: Button, count: u8) -> Result<Option<String>, String> {
        // On Linux the mouse's middle button pastes the text the person last selected anywhere on
        // their desktop; a script's middle click does all else it does, without that.
        if !input::NATIVE || button == Button::Middle {
            let args = json!({"x": x, "y": y, "button": button.name(), "double": count > 1});
            let clicked = self.run(id, "clickAt", args, Duration::from_secs(10));
            return Ok(self.acting(id, clicked).await?.file);
        }
        let webview = self.webview(id)?;
        self.keep_focus(id).await;
        Ok(self.acting(id, input::click(&webview, x, y, button, count)).await?.file)
    }

    /// Presses `keys` one after another where tab `id`'s page has the keyboard, as the person's
    /// keyboard would, and waits for what they set off. What a new page turned out to be when it
    /// was a file.
    pub async fn press(&self, id: u64, keys: &[Key]) -> Result<Option<String>, String> {
        let webview = self.webview(id)?;
        if input::NATIVE {
            self.keep_focus(id).await;
            // The keyboard's own keys type into a password field as well; so they are not given
            // the keys that would.
            if keys.iter().any(Key::types)
                && self.run(id, "focused", json!({}), Duration::from_secs(5)).await?["secret"] == true
            {
                return Err(SECRET.into());
            }
        }
        let pressed = async {
            for key in keys {
                if input::NATIVE {
                    input::press(&webview, key).await?;
                } else {
                    self.run(id, "press", key.for_page(), Duration::from_secs(10)).await?;
                }
            }
            Ok(())
        };
        Ok(self.acting(id, pressed).await?.file)
    }

    /// Types `text` where tab `id`'s page has the keyboard, as typing it there would, and waits for
    /// the page to settle. Never in a password field.
    pub async fn insert(&self, id: u64, text: &str) -> Result<(), String> {
        if !input::NATIVE {
            let typed = self.run(id, "insert", json!({"text": text}), Duration::from_secs(10));
            return self.acting(id, typed).await.map(|_| ());
        }
        let webview = self.webview(id)?;
        self.keep_focus(id).await;
        let target = self.run(id, "focused", json!({}), Duration::from_secs(5)).await?;
        if target["secret"] == true {
            return Err(SECRET.into());
        }
        if target["text"] != true {
            return Err(
                "Nothing that takes text has the keyboard: click the field first, or pass its reference.".into(),
            );
        }
        self.acting(id, input::insert(&webview, text)).await.map(|_| ())
    }

    /// A screenshot of what tab `id` shows, as the engine draws it.
    pub async fn screenshot(&self, id: u64) -> Result<input::Shot, String> {
        input::screenshot(&self.webview(id)?).await
    }

    /// CSS pixels of tab `id`'s page per pixel of the last screenshot the assistant took of it.
    pub fn shot_scale(&self, id: u64) -> f64 {
        self.inner
            .lock()
            .tabs
            .iter()
            .find(|t| t.id == id)
            .map_or(1.0, |t| t.shot_scale)
    }

    pub fn set_shot_scale(&self, id: u64, scale: f64) {
        if let Some(t) = self.inner.lock().tabs.iter_mut().find(|t| t.id == id) {
            t.shot_scale = scale;
        }
    }

    /// Runs a script of the assistant's, `code`, in tab `id`'s page, waits up to `max` for what it
    /// gives back, then for what it set off. `code` is an expression or the body of an async
    /// function: `document.title`, or `const n = 1; return n + 1;`.
    pub async fn script(
        &self,
        id: u64,
        code: &str,
        max: Duration,
        cancel: &CancellationToken,
    ) -> Result<Acted<Ran>, String> {
        let run = uuid::Uuid::new_v4().simple().to_string();
        let ran = async {
            let [expression, body] = script_forms(code);
            if self.start_script(id, &run, &expression).await.is_err()
                && self.start_script(id, &run, &body).await.is_err()
            {
                let parsed = self
                    .run(id, "parse", json!({"code": code}), Duration::from_secs(5))
                    .await;
                return Err(match parsed {
                    Err(e) => format!("The script does not parse as JavaScript: {e}"),
                    Ok(_) => "The script did not start: it does not parse as JavaScript, or the page is still loading."
                        .into(),
                });
            }
            let start = Instant::now();
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return Ok(Ran::Stopped),
                    _ = tokio::time::sleep(Duration::from_millis(100)) => {}
                }
                // An error is a page between two loads; it is asked again.
                if let Ok(v) = self.run(id, "ran", json!({"id": run}), Duration::from_secs(5)).await {
                    if v["gone"] == true {
                        return Ok(Ran::Left);
                    }
                    if v["done"] == true {
                        return Ok(match v["thrown"].as_str() {
                            Some(e) => Ran::Threw(e.to_string()),
                            None => Ran::Gave(v["value"].as_str().unwrap_or("undefined").to_string()),
                        });
                    }
                }
                if start.elapsed() >= max {
                    return Ok(Ran::TimedOut);
                }
            }
        };
        self.acting(id, ran).await
    }

    /// Starts a script of the assistant's, `code` (an async function), in tab `id`'s page, which
    /// keeps its outcome under `run` for `page.js`'s `ran`. Err when it does not even start, as when
    /// it does not parse.
    async fn start_script(&self, id: u64, run: &str, code: &str) -> Result<(), String> {
        let webview = self.webview(id)?;
        let js = script_call(&json!({"id": run}), code);
        let (tx, rx) = oneshot::channel::<String>();
        let tx = Mutex::new(Some(tx));
        webview
            .eval_with_callback(js, move |result| {
                if let Some(tx) = tx.lock().take() {
                    let _ = tx.send(result);
                }
            })
            .map_err(|e| e.to_string())?;
        let raw = tokio::time::timeout(Duration::from_secs(10), rx)
            .await
            .map_err(|_| "The page did not answer in time; it may still be loading.".to_string())?
            .map_err(|_| "That tab was closed.".to_string())?;
        let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        match value["ok"] == true {
            true => Ok(()),
            false => Err(value["error"]
                .as_str()
                .unwrap_or("The script did not start: it does not parse.")
                .to_string()),
        }
    }

    /// Lets the page in tab `id` keep the keyboard as it sees it while the app's window has it
    /// back after one of the assistant's clicks (Linux, see [`input::click`]): pages close their
    /// menus and suggestions when the keyboard leaves them.
    async fn keep_focus(&self, id: u64) {
        if cfg!(target_os = "linux") {
            let _ = self.run(id, "keepFocus", json!({}), Duration::from_secs(5)).await;
        }
    }

    /// Runs one of `page.js`'s actions in tab `id` and returns what it answered.
    pub async fn run(&self, id: u64, action: &str, args: Value, timeout: Duration) -> Result<Value, String> {
        let webview = self.webview(id)?;
        let js = page_call(action, &args);
        let (tx, rx) = oneshot::channel::<String>();
        let tx = Mutex::new(Some(tx));
        webview
            .eval_with_callback(js, move |result| {
                if let Some(tx) = tx.lock().take() {
                    let _ = tx.send(result);
                }
            })
            .map_err(|e| e.to_string())?;
        let raw = tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| "The page did not answer in time; it may still be loading.".to_string())?
            .map_err(|_| "That tab was closed.".to_string())?;
        let value: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        // WebKitGTK hands back an empty answer for a script that threw or a page that is gone.
        if !value.is_object() {
            return Err("The page could not be read; it may still be loading.".into());
        }
        if let Some(e) = value["error"].as_str() {
            return Err(e.to_string());
        }
        if let Some(url) = value["url"].as_str() {
            self.at_url(id, url);
        }
        Ok(value)
    }

    /// Notes where the page of tab `id` says it is. A site that changes its page by script, as
    /// YouTube does from a channel to its Videos, loads nothing and may keep its title, so no
    /// event of the webview's tells.
    fn at_url(&self, id: u64, url: &str) {
        {
            let mut inner = self.inner.lock();
            let Some(tab) = inner.tabs.iter_mut().find(|t| t.id == id) else {
                return;
            };
            if tab.url == url {
                return;
            }
            tab.url = url.to_string();
        }
        self.changed();
    }

    /// Waits for the page in tab `id` to stop changing, `max` at most.
    pub async fn settle(&self, id: u64, max: Duration) {
        let start = Instant::now();
        let mut last: Option<u64> = None;
        let mut quiet = 0;
        while start.elapsed() < max {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let Ok(state) = self.run(id, "state", json!({}), Duration::from_secs(2)).await else {
                continue;
            };
            let changes = state["changes"].as_u64().unwrap_or(0);
            let ready = state["ready"].as_str() == Some("complete");
            let has_text = state["textLength"].as_u64().unwrap_or(0) > 0;
            quiet = if last == Some(changes) { quiet + 1 } else { 0 };
            last = Some(changes);
            if quiet >= 2 && (ready || start.elapsed() > Duration::from_secs(2)) && has_text {
                return;
            }
        }
    }

    /// Whether a bot check holds tab `id`'s page.
    pub async fn state(&self, id: u64) -> Result<PageState, String> {
        let s = self.run(id, "state", json!({}), Duration::from_secs(5)).await?;
        let title = s["title"].as_str().unwrap_or_default().to_string();
        let small = s["small"].as_str().map(str::to_string);
        let html = small.unwrap_or_else(|| format!("<title>{title}</title>"));
        let check = page::check(&page::Response {
            status: 200,
            server: "",
            mitigated: false,
            html: &html,
        });
        Ok(PageState { check })
    }

    /// The page in tab `id` as HTML, for reading.
    pub async fn html(&self, id: u64) -> Result<Rendered, String> {
        let v = self.run(id, "html", json!({}), Duration::from_secs(15)).await?;
        Ok(Rendered {
            url: v["url"].as_str().unwrap_or_default().to_string(),
            title: v["title"].as_str().unwrap_or_default().to_string(),
            kind: v["type"].as_str().unwrap_or_default().to_string(),
            html: v["html"].as_str().unwrap_or_default().to_string(),
        })
    }

    /// The file tab `id` shows, at most `max` bytes: for a document the browser shows in a viewer
    /// of its own, such as a PDF, whose text is not in the page. Its type and its bytes.
    pub async fn file(&self, id: u64, max: usize) -> Result<(String, Vec<u8>), String> {
        // WebKitGTK hands over the file as it loaded it. Asked for again, even from the page, it
        // would go without what a browser's own request carries, and a guarded site turns it away.
        #[cfg(target_os = "linux")]
        let (kind, mut bytes) = loaded_file(&self.webview(id)?).await?;
        // Elsewhere the page asks for it again, with the tab's cookies.
        #[cfg(not(target_os = "linux"))]
        let (kind, mut bytes) = {
            use base64::Engine as _;
            let v = self
                .run(id, "bytes", json!({"max": max}), Duration::from_secs(60))
                .await?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(v["base64"].as_str().unwrap_or_default())
                .map_err(|e| format!("The file came back garbled: {e}"))?;
            (v["type"].as_str().unwrap_or_default().to_string(), bytes)
        };
        bytes.truncate(max);
        let kind = kind.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
        Ok((kind, bytes))
    }

    /// Waits up to `max` for a check that passes by itself (Cloudflare's, say) to pass in tab
    /// `id`. Returns the check still there, if any.
    pub async fn outwait(&self, id: u64, max: Duration, cancel: &CancellationToken) -> Option<Check> {
        let start = Instant::now();
        loop {
            let check = self.state(id).await.ok().and_then(|s| s.check);
            if check.is_none() || matches!(check, Some(Check::Captcha)) || start.elapsed() >= max {
                return check;
            }
            tokio::select! {
                _ = cancel.cancelled() => return check,
                _ = tokio::time::sleep(Duration::from_millis(700)) => {}
            }
        }
    }

    /// Shows tab `id` to the person with `message` (a check to solve, a sign-in to make) and waits
    /// until they say they are done or to go on, or `until_clear` and the check is gone, for
    /// `max` at most.
    pub async fn ask(
        &self,
        id: u64,
        chat: &str,
        message: &str,
        max: Duration,
        until_clear: bool,
        cancel: &CancellationToken,
    ) -> Result<Answer, String> {
        let _one = tokio::select! {
            one = self.asking.lock() => one,
            _ = cancel.cancelled() => return Err("Stopped.".into()),
        };
        self.webview(id)?;
        let (tx, mut rx) = oneshot::channel();
        let until = chrono::Utc::now().timestamp_millis() + max.as_millis() as i64;
        let info = AttentionInfo {
            tab: id,
            chat_id: chat.to_string(),
            message: message.to_string(),
            until,
        };
        {
            let mut inner = self.inner.lock();
            inner.attention = Some(Attention {
                info: info.clone(),
                answer: Some(tx),
            });
            inner.active = Some(id);
            self.arrange(&mut inner);
        }
        self.changed();
        let _ = self.app.emit(ATTENTION_EVENT, Some(&info));
        let start = Instant::now();
        let answer = loop {
            tokio::select! {
                said = &mut rx => break match said {
                    Ok(true) => Answer::Done,
                    _ => Answer::Skipped,
                },
                _ = cancel.cancelled() => break Answer::Skipped,
                _ = tokio::time::sleep(Duration::from_millis(1200)) => {
                    if start.elapsed() >= max {
                        break Answer::TimedOut;
                    }
                    if until_clear
                        && let Ok(state) = self.state(id).await
                        && state.check.is_none()
                    {
                        break Answer::Cleared;
                    }
                }
            }
        };
        self.inner.lock().attention = None;
        let _ = self.app.emit(ATTENTION_EVENT, None::<AttentionInfo>);
        Ok(answer)
    }

    /// The person's answer to [`Browser::ask`]: done, or go on without them.
    pub fn answer(&self, done: bool) {
        if let Some(a) = self.inner.lock().attention.as_mut()
            && let Some(tx) = a.answer.take()
        {
            let _ = tx.send(done);
        }
    }
}

/// What one of the assistant's actions did in a tab.
pub struct Acted<T> {
    pub done: T,
    /// A new page loaded with it.
    pub loaded: bool,
    /// What the new page turned out to be when it was a file, which the tab left alone.
    pub file: Option<String>,
}

/// How a script of the assistant's ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ran {
    /// It gave back this: a string as it is, anything else as JSON.
    Gave(String),
    /// It threw this.
    Threw(String),
    /// The page it ran in went away before it finished: another page loaded.
    Left,
    TimedOut,
    Stopped,
}

/// A script of the assistant's as an async function, first as an expression, whose value it gives
/// back, then as the function's body. Only one of the two parses: a body of statements is no
/// expression, and an expression gives nothing back as a body.
fn script_forms(code: &str) -> [String; 2] {
    let code = code.trim().trim_end_matches([';', ' ', '\n', '\t', '\r']);
    [
        format!("async () => {{ return (\n{code}\n); }}"),
        format!("async () => {{\n{code}\n}}"),
    ]
}

/// Waits for a page load after `before` to start (within `start`) and finish (within `finish`).
/// Returns whether one started.
async fn wait_loaded(load: &mut watch::Receiver<Load>, before: u64, start: Duration, finish: Duration) -> bool {
    let started = matches!(
        tokio::time::timeout(start, load.wait_for(|l| l.started > before)).await,
        Ok(Ok(_))
    );
    if !started {
        return false;
    }
    let _ = tokio::time::timeout(finish, load.wait_for(|l| l.started > before && !l.loading)).await;
    true
}

/// Whether a tab may go to `url`: the web only, and none of the app's own pages, which answer the
/// app's commands.
pub fn allowed(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https" | "blob") {
        return Err(format!(
            "The browser opens only web pages (http and https), not {}: links.",
            url.scheme()
        ));
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    // On Windows the app's own pages and files are served from http://<scheme>.localhost.
    let app_host = host.ends_with(".localhost") && url.port().is_none();
    if app_host || crate::speech::microphone::is_app_origin(url.as_str()) {
        return Err("The browser cannot open the app's own pages.".into());
    }
    Ok(())
}

/// What the person typed in the address bar, as a page to go to: an address, or a search with
/// `engine`.
pub fn address(input: &str, engine: crate::settings::SearchEngine) -> Option<Url> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Ok(url) = Url::parse(input)
        && matches!(url.scheme(), "http" | "https")
    {
        return Some(url);
    }
    let looks_like_host = !input.contains(char::is_whitespace)
        && (input.starts_with("localhost") || input.contains('.'))
        && !input.starts_with('.')
        && !input.ends_with('.');
    if looks_like_host {
        let local = input.starts_with("localhost") || input.starts_with("127.") || input.starts_with("[::1]");
        let scheme = if local { "http" } else { "https" };
        if let Ok(url) = Url::parse(&format!("{scheme}://{input}"))
            && url
                .host_str()
                .is_some_and(|h| h.contains('.') || h == "localhost" || h.starts_with('['))
        {
            return Some(url);
        }
    }
    Some(search_url(input, engine))
}

/// A search for `query` on `engine`'s own page.
pub fn search_url(query: &str, engine: crate::settings::SearchEngine) -> Url {
    use crate::settings::SearchEngine;
    let base = match engine {
        SearchEngine::Duckduckgo => "https://duckduckgo.com/",
        SearchEngine::Brave => "https://search.brave.com/search",
        SearchEngine::Bing => "https://www.bing.com/search",
        SearchEngine::Google => "https://www.google.com/search",
    };
    Url::parse_with_params(base, &[("q", query)]).expect("a valid URL")
}

/// Downloads what a tab cannot show, as browsers do, or, in the assistant's tabs (`refuse`), notes
/// what the file was and leaves it. WebKitGTK on its own drops a page it cannot show (a zip, an
/// installer) unless the site marks it an attachment, and the click does nothing.
#[cfg(target_os = "linux")]
fn download_what_cannot_show(tab: &Webview, refuse: Option<watch::Sender<Load>>) {
    use webkit2gtk::glib::prelude::Cast;
    use webkit2gtk::{
        PolicyDecisionExt, PolicyDecisionType, ResponsePolicyDecision, ResponsePolicyDecisionExt, URIResponseExt,
        WebViewExt,
    };
    let done = tab.with_webview(move |webview| {
        webview.inner().connect_decide_policy(move |_, decision, kind| {
            if kind != PolicyDecisionType::Response {
                return false;
            }
            let Some(response) = decision.downcast_ref::<ResponsePolicyDecision>() else {
                return false;
            };
            if !response.is_main_frame_main_resource() {
                return false;
            }
            let answer = response.response();
            let attachment = answer
                .as_ref()
                .and_then(|a| a.http_headers())
                .and_then(|h| h.one("Content-Disposition"))
                .is_some_and(|d| d.trim_start().to_ascii_lowercase().starts_with("attachment"));
            if !attachment && response.is_mime_type_supported() {
                return false;
            }
            match &refuse {
                Some(load) => {
                    let kind = answer.and_then(|a| a.mime_type()).map(String::from);
                    load.send_modify(|l| l.file = Some(kind.unwrap_or_else(|| "a file".into())));
                    decision.ignore();
                }
                None => decision.download(),
            }
            true
        });
    });
    if let Err(e) = done {
        tracing::debug!("could not reach a browser tab for its downloads: {e}");
    }
}

/// The file `tab` shows as WebKit loaded it, and its type.
#[cfg(target_os = "linux")]
async fn loaded_file(tab: &Webview) -> Result<(String, Vec<u8>), String> {
    use webkit2gtk::{URIResponseExt, WebResourceExt, WebViewExt, gio};
    let (tx, rx) = oneshot::channel();
    tab.with_webview(move |webview| {
        let Some(resource) = webview.inner().main_resource() else {
            let _ = tx.send(Err("The tab shows nothing yet.".to_string()));
            return;
        };
        let kind = resource.response().and_then(|r| r.mime_type()).map(String::from);
        resource.data(None::<&gio::Cancellable>, move |data| {
            let got = data
                .map(|bytes| (kind.unwrap_or_default(), bytes))
                .map_err(|e| format!("The browser could not hand over the file: {e}"));
            let _ = tx.send(got);
        });
    })
    .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(60), rx)
        .await
        .map_err(|_| "The browser did not hand over the file in time.".to_string())?
        .map_err(|_| "That tab was closed.".to_string())?
}

/// Handles the downloads of every tab, from the web context they share on Linux, which lives as
/// long as the app. Done once: each tab's `on_download` would add its handler to that context
/// again, so each download would be told of once per tab.
#[cfg(target_os = "linux")]
fn handle_downloads(tab: &Webview, browser: Weak<Browser>) {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use webkit2gtk::glib::prelude::{Cast, StaticType};
    use webkit2gtk::glib::translate::{IntoGlib, ToGlibPtr};
    use webkit2gtk::glib::{self, gobject_ffi};
    use webkit2gtk::{DownloadExt, URIRequestExt, WebContextExt, WebViewExt};
    fn url_of(download: &webkit2gtk::Download) -> Option<Url> {
        download.request()?.uri().and_then(|u| Url::parse(&u).ok())
    }
    /// Blocks the handlers `download` already has for choosing where it goes. wry gives every tab
    /// one even without `on_download`, which saves the file where wry likes and answers before
    /// this one is asked.
    fn set_aside_others(download: &webkit2gtk::Download) {
        let instance: *mut gobject_ffi::GObject = download.upcast_ref::<glib::Object>().to_glib_none().0;
        // SAFETY: the download is alive for the call, the signal is looked up on its own type,
        // and matching by signal alone needs no closure, function or data.
        unsafe {
            let signal = gobject_ffi::g_signal_lookup(
                c"decide-destination".as_ptr(),
                webkit2gtk::Download::static_type().into_glib(),
            );
            gobject_ffi::g_signal_handlers_block_matched(
                instance.cast(),
                gobject_ffi::G_SIGNAL_MATCH_ID,
                signal,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }
    }
    let done = tab.with_webview(move |webview| {
        let Some(context) = webview.inner().context() else {
            tracing::warn!("a browser tab has no web context; its downloads will not be saved");
            return;
        };
        context.connect_download_started(move |_, download| {
            set_aside_others(download);
            let path = Rc::new(RefCell::new(None::<PathBuf>));
            let failed = Rc::new(Cell::new(false));
            let (b, to) = (browser.clone(), path.clone());
            download.connect_decide_destination(move |download, suggested| {
                let (Some(b), Some(url)) = (b.upgrade(), url_of(download)) else {
                    download.cancel();
                    return true;
                };
                let destination = b.download_path(&url, std::path::Path::new(suggested));
                tracing::info!(%url, to = %destination.display(), "browser download");
                download.set_destination(&destination.to_string_lossy());
                *to.borrow_mut() = Some(destination);
                true
            });
            let failing = failed.clone();
            download.connect_failed(move |_, _| failing.set(true));
            let b = browser.clone();
            download.connect_finished(move |download| {
                if let (Some(b), Some(url)) = (b.upgrade(), url_of(download)) {
                    let ok = !failed.get();
                    b.download_finished(&url, path.borrow().clone().filter(|_| ok), ok);
                }
            });
        });
    });
    if let Err(e) = done {
        tracing::warn!("could not reach the browser's web context for its downloads: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SearchEngine;

    #[test]
    fn page_js_is_called_as_one_expression() {
        let js = page_call("snapshot", &json!({"max": 3}));
        assert!(js.starts_with("// "), "the file's own header comment comes first");
        assert!(js.ends_with(r#"})("snapshot", {"max":3})"#), "{}", &js[js.len() - 40..]);
    }

    #[test]
    fn scripts_are_tried_as_an_expression_then_as_a_body() {
        let [expression, body] = script_forms("  document.title;  \n");
        assert_eq!(expression, "async () => { return (\ndocument.title\n); }");
        assert_eq!(body, "async () => {\ndocument.title\n}");
        // A comment at the end does not swallow what closes the function.
        let [expression, _] = script_forms("1 + 1 // two");
        assert!(expression.ends_with("// two\n); }"));
        let js = script_call(&json!({"id": "r1"}), &body);
        assert!(js.ends_with("})(\"run\", {\"id\":\"r1\"}, async () => {\ndocument.title\n})"));
    }

    #[test]
    fn tabs_stay_on_the_web() {
        let ok = |s: &str| allowed(&Url::parse(s).unwrap()).is_ok();
        assert!(ok("https://example.com/"));
        assert!(ok("http://localhost:8080/"));
        assert!(ok("http://192.168.1.10/"));
        assert!(!ok("file:///etc/passwd"));
        assert!(!ok("tauri://localhost/index.html"));
        assert!(!ok("asset://localhost/x"));
        assert!(!ok("http://asset.localhost/C:/Users/x"));
        assert!(!ok("http://tauri.localhost/"));
        assert!(!ok("https://ipc.localhost/plugin:x"));
        assert!(!ok("javascript:alert(1)"));
        assert!(!ok("data:text/html,hi"));
        assert_eq!(ok("http://localhost:1420/"), !cfg!(debug_assertions));
    }

    #[test]
    fn the_address_bar_tells_addresses_from_searches() {
        let go = |s: &str| address(s, SearchEngine::Duckduckgo).map(String::from);
        assert_eq!(go("example.com").as_deref(), Some("https://example.com/"));
        assert_eq!(
            go("https://example.com/a?b=1").as_deref(),
            Some("https://example.com/a?b=1")
        );
        assert_eq!(go("localhost:3000/x").as_deref(), Some("http://localhost:3000/x"));
        assert_eq!(
            go("rust lifetimes").as_deref(),
            Some("https://duckduckgo.com/?q=rust+lifetimes")
        );
        assert_eq!(
            go("what is 2.5 kg in lb").as_deref(),
            Some("https://duckduckgo.com/?q=what+is+2.5+kg+in+lb")
        );
        assert_eq!(go("   "), None);
        assert_eq!(
            address("otters", SearchEngine::Google).map(String::from).as_deref(),
            Some("https://www.google.com/search?q=otters")
        );
    }
}
