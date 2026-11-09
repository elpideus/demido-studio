//! The desktop on Wayland, through the desktop portals (xdg-desktop-portal) over D-Bus.
//!
//! Screenshots come from the Screenshot portal, taken without its dialog (`interactive` off); the
//! portal asks the person once whether Demido may. Some desktops (KDE) save the picture in the
//! person's Pictures folder: it is read and deleted there.
//!
//! The pointer and keys go through a RemoteDesktop session with the screens shared too (a
//! ScreenCast source), since a pointer is placed on a screen of the session: the portal checks
//! every absolute position against the shared screens. The desktop shows its dialog when the
//! session starts; the restore token it gives back (persist mode 2, until revoked) starts the next
//! one without it.
//!
//! The portals know a program by its app id: a sandboxed one by its sandbox, a program of the host
//! (Demido) by registering one, before anything else on its connection. Demido registers its
//! desktop entry's, so the desktop's dialogs name it and what the person allowed is kept for it:
//! the package's (`demido-studio`), else the entry a tool that installs AppImages wrote for it
//! (`demido-studio-0.6.1-x86_64`). The portal takes an entry only when the program it runs is
//! there; with none, Demido is an app it cannot name, which still works.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use image::RgbImage;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::{Rect, Step, char_keysym};
use crate::settings::SettingsStore;
use crate::web::browser::input::Button;

const DESTINATION: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";
const REMOTE: &str = "org.freedesktop.portal.RemoteDesktop";
const CAST: &str = "org.freedesktop.portal.ScreenCast";
const SCREENSHOT: &str = "org.freedesktop.portal.Screenshot";
const SESSION: &str = "org.freedesktop.portal.Session";
const REGISTRY: &str = "org.freedesktop.host.portal.Registry";
/// The desktop entry Demido is installed with (packaging/linux/demido-studio.desktop).
const APP_ID: &str = "demido-studio";

/// Device types of a RemoteDesktop session.
const KEYBOARD: u32 = 1;
const POINTER: u32 = 2;
/// The ScreenCast source type of whole monitors.
const MONITOR: u32 = 1;
/// Remembered until the person revokes it.
const PERSIST: u32 = 2;
/// A session idle this long ends; the next use starts another, by its token without the dialog.
const IDLE: Duration = Duration::from_secs(180);
/// Time for the program under the pointer to take in each key.
const BETWEEN_KEYS: Duration = Duration::from_millis(8);
/// Linux's button codes (input-event-codes.h).
const BTN_LEFT: i32 = 0x110;
const BTN_RIGHT: i32 = 0x111;
const BTN_MIDDLE: i32 = 0x112;

type Options<'a> = HashMap<&'static str, Value<'a>>;
type Results = HashMap<String, OwnedValue>;

/// How a portal request ended without its results.
enum Refused {
    /// The person said no, or closed the dialog.
    Declined,
    Failed(String),
}

/// The control session the tools share: started when first needed, kept while in use.
#[derive(Default)]
pub struct Shared(Arc<Mutex<Option<Session>>>);

struct Session {
    remote: zbus::Proxy<'static>,
    handle: OwnedObjectPath,
    devices: u32,
    streams: Vec<Stream>,
    used: Instant,
    /// Cancelled when the session is closed from here, which ends its watcher.
    done: CancellationToken,
}

/// A screen shared in the session, and where it is on the desktop.
struct Stream {
    node: u32,
    rect: Rect,
}

impl Shared {
    pub async fn area(
        &self,
        settings: &Arc<SettingsStore>,
        cancel: &CancellationToken,
        asking: &(dyn Fn() + Sync),
    ) -> Result<Rect, String> {
        let mut guard = self.0.lock().await;
        let session = self.ready(&mut guard, settings, cancel, asking).await?;
        Ok(session.area())
    }

    pub async fn perform(
        &self,
        settings: &Arc<SettingsStore>,
        steps: &[Step],
        cancel: &CancellationToken,
        asking: &(dyn Fn() + Sync),
    ) -> Result<(), String> {
        let mut guard = self.0.lock().await;
        let session = self.ready(&mut guard, settings, cancel, asking).await?;
        let done = session.perform(steps).await;
        if done.is_err() {
            // A session the desktop ended, most likely: the next call starts another.
            if let Some(session) = guard.take() {
                session.close().await;
            }
        }
        done
    }

    /// The session, started first when there is none.
    async fn ready<'a>(
        &self,
        guard: &'a mut Option<Session>,
        settings: &Arc<SettingsStore>,
        cancel: &CancellationToken,
        asking: &(dyn Fn() + Sync),
    ) -> Result<&'a mut Session, String> {
        if guard.is_none() {
            asking();
            let token = settings.get().remote_desktop_token;
            let (session, conn, token) = Session::start(token, cancel).await?;
            if token.is_some() {
                let _ = settings.update(|s| s.remote_desktop_token = token);
            }
            watch(self.0.clone(), settings.clone(), &conn, &session).await;
            *guard = Some(session);
        }
        let session = guard.as_mut().expect("started above");
        session.used = Instant::now();
        Ok(session)
    }
}

/// Ends the session after [`IDLE`], and forgets its token when the desktop ends it (the person
/// stopped it there), so the next one asks again.
async fn watch(
    shared: Arc<Mutex<Option<Session>>>,
    settings: Arc<SettingsStore>,
    conn: &zbus::Connection,
    session: &Session,
) {
    let closed = match zbus::Proxy::new(conn, DESTINATION, session.handle.clone(), SESSION).await {
        Ok(proxy) => proxy.receive_signal("Closed").await.ok(),
        Err(_) => None,
    };
    let (handle, done) = (session.handle.clone(), session.done.clone());
    tokio::spawn(async move {
        let mut closed = closed;
        let ours = |guard: &Option<Session>| guard.as_ref().is_some_and(|s| s.handle == handle);
        loop {
            let ended = async {
                match closed.as_mut() {
                    Some(signals) => {
                        signals.next().await;
                    }
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                _ = done.cancelled() => return,
                _ = ended => {
                    let _ = settings.update(|s| s.remote_desktop_token = None);
                    let mut guard = shared.lock().await;
                    if ours(&guard) {
                        *guard = None;
                    }
                    tracing::info!("the desktop ended the remote control session");
                    return;
                }
                _ = tokio::time::sleep(Duration::from_secs(20)) => {
                    let mut guard = shared.lock().await;
                    if !ours(&guard) {
                        return;
                    }
                    if guard.as_ref().is_some_and(|s| s.used.elapsed() >= IDLE)
                        && let Some(session) = guard.take()
                    {
                        session.close().await;
                        return;
                    }
                }
            }
        }
    });
}

impl Session {
    /// Starts a session with the keyboard, the pointer and every screen, by `token` when the
    /// person allowed one before. Returns it, its connection, and the token for the next.
    async fn start(
        token: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<(Session, zbus::Connection, Option<String>), String> {
        let conn = bus().await?;
        let remote = zbus::Proxy::new(&conn, DESTINATION, PATH, REMOTE)
            .await
            .map_err(|e| e.to_string())?;
        if remote.get_property::<u32>("version").await.is_err() {
            return Err(
                "This desktop does not let programs use the mouse and keyboard: it has no remote desktop portal \
                 (xdg-desktop-portal with a backend for it, such as KDE's or GNOME's)."
                    .into(),
            );
        }
        let refused = |r: Refused| match r {
            Refused::Declined => "You did not allow Demido to control the mouse and keyboard.".to_string(),
            Refused::Failed(e) => format!("The mouse and keyboard could not be reached: {e}"),
        };

        let (t, session_token) = (name(), name());
        let options = Options::from([
            ("handle_token", Value::from(t.as_str())),
            ("session_handle_token", Value::from(session_token.as_str())),
        ]);
        let created = request(&conn, REMOTE, "CreateSession", &(options,), &t, cancel)
            .await
            .map_err(refused)?;
        let handle = text(&created, "session_handle").ok_or("The desktop started no session.")?;
        let handle = OwnedObjectPath::try_from(handle).map_err(|e| e.to_string())?;
        let mut session = Session {
            remote,
            handle,
            devices: 0,
            streams: Vec::new(),
            used: Instant::now(),
            done: CancellationToken::new(),
        };
        match session.choose(&conn, token, cancel).await {
            Ok(token) => Ok((session, conn, token)),
            Err(r) => {
                session.close().await;
                Err(refused(r))
            }
        }
    }

    /// Asks for the devices and screens, and starts the session (the desktop's dialog).
    async fn choose(
        &mut self,
        conn: &zbus::Connection,
        token: Option<String>,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, Refused> {
        let t = name();
        let mut options = Options::from([
            ("handle_token", Value::from(t.as_str())),
            ("types", Value::from(KEYBOARD | POINTER)),
            ("persist_mode", Value::from(PERSIST)),
        ]);
        if let Some(token) = &token {
            options.insert("restore_token", Value::from(token.as_str()));
        }
        request(conn, REMOTE, "SelectDevices", &(&self.handle, options), &t, cancel).await?;

        let t = name();
        let options = Options::from([
            ("handle_token", Value::from(t.as_str())),
            ("types", Value::from(MONITOR)),
            ("multiple", Value::from(true)),
        ]);
        request(conn, CAST, "SelectSources", &(&self.handle, options), &t, cancel).await?;

        let t = name();
        let options = Options::from([("handle_token", Value::from(t.as_str()))]);
        let started = request(conn, REMOTE, "Start", &(&self.handle, "", options), &t, cancel).await?;
        self.devices = started.get("devices").and_then(|v| u32::try_from(v).ok()).unwrap_or(0);
        self.streams = started.get("streams").map(streams).unwrap_or_default();
        if self.streams.is_empty() {
            return Err(Refused::Failed(
                "the desktop shared no screen to place the pointer on".into(),
            ));
        }
        Ok(text(&started, "restore_token"))
    }

    /// Every shared screen together.
    fn area(&self) -> Rect {
        let mut rects = self.streams.iter().map(|s| s.rect);
        let first = rects.next().unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        });
        rects.fold(first, |all, r| all.union(&r))
    }

    async fn perform(&self, steps: &[Step]) -> Result<(), String> {
        for step in steps {
            self.step(step).await?;
        }
        Ok(())
    }

    async fn step(&self, step: &Step) -> Result<(), String> {
        let pointer = || {
            if self.devices & POINTER == 0 {
                Err("You allowed Demido the keyboard but not the mouse.".to_string())
            } else {
                Ok(())
            }
        };
        let keyboard = || {
            if self.devices & KEYBOARD == 0 {
                Err("You allowed Demido the mouse but not the keyboard.".to_string())
            } else {
                Ok(())
            }
        };
        match step {
            Step::MoveTo { x, y } => {
                pointer()?;
                let (stream, x, y) = self.place(*x, *y);
                self.notify("NotifyPointerMotionAbsolute", &(&self.handle, none(), stream, x, y))
                    .await
            }
            Step::Press(button) | Step::Release(button) => {
                pointer()?;
                let pressed = u32::from(matches!(step, Step::Press(_)));
                self.notify("NotifyPointerButton", &(&self.handle, none(), code(*button), pressed))
                    .await
            }
            Step::Scroll { dx, dy } => {
                pointer()?;
                let unit = scroll_unit();
                for (axis, steps) in [(0u32, *dy), (1u32, *dx)] {
                    if steps != 0 {
                        self.notify("NotifyPointerAxisDiscrete", &(&self.handle, none(), axis, steps * unit))
                            .await?;
                    }
                }
                Ok(())
            }
            Step::KeyDown(keysym) | Step::KeyUp(keysym) => {
                keyboard()?;
                let pressed = u32::from(matches!(step, Step::KeyDown(_)));
                self.notify("NotifyKeyboardKeysym", &(&self.handle, none(), *keysym as i32, pressed))
                    .await
            }
            Step::Type(text) => {
                keyboard()?;
                for c in text.chars().filter(|&c| c != '\r') {
                    let keysym = char_keysym(c) as i32;
                    self.notify("NotifyKeyboardKeysym", &(&self.handle, none(), keysym, 1u32))
                        .await?;
                    self.notify("NotifyKeyboardKeysym", &(&self.handle, none(), keysym, 0u32))
                        .await?;
                    tokio::time::sleep(BETWEEN_KEYS).await;
                }
                Ok(())
            }
            Step::Tap(code) => {
                keyboard()?;
                // The desktop's keycodes are the kernel's: X11's less 8.
                let code = i32::from(*code) - 8;
                for pressed in [1u32, 0] {
                    self.notify("NotifyKeyboardKeycode", &(&self.handle, none(), code, pressed))
                        .await?;
                }
                tokio::time::sleep(BETWEEN_KEYS).await;
                Ok(())
            }
            Step::Pause(time) => {
                tokio::time::sleep(*time).await;
                Ok(())
            }
        }
    }

    /// The screen a point of the desktop is on, and the point on it; a point off every screen
    /// goes to the nearest.
    fn place(&self, x: f64, y: f64) -> (u32, f64, f64) {
        let distance = |r: &Rect| {
            let dx = (r.x - x).max(x - (r.x + r.width - 1.0)).max(0.0);
            let dy = (r.y - y).max(y - (r.y + r.height - 1.0)).max(0.0);
            dx * dx + dy * dy
        };
        let stream = self
            .streams
            .iter()
            .find(|s| s.rect.contains(x, y))
            .or_else(|| {
                self.streams
                    .iter()
                    .min_by(|a, b| distance(&a.rect).total_cmp(&distance(&b.rect)))
            })
            .expect("a session has a screen");
        let r = stream.rect;
        (
            stream.node,
            (x - r.x).clamp(0.0, r.width - 1.0),
            (y - r.y).clamp(0.0, r.height - 1.0),
        )
    }

    async fn notify<B>(&self, method: &str, body: &B) -> Result<(), String>
    where
        B: serde::Serialize + zbus::zvariant::DynamicType,
    {
        self.remote
            .call_method(method, body)
            .await
            .map(drop)
            .map_err(|e| format!("The desktop did not take the input: {e}"))
    }

    async fn close(&self) {
        self.done.cancel();
        if let Ok(proxy) = zbus::Proxy::new(self.remote.connection(), DESTINATION, self.handle.clone(), SESSION).await {
            let _ = proxy.call_method("Close", &()).await;
        }
    }
}

/// A picture of the whole desktop, from the Screenshot portal.
pub async fn screenshot(cancel: &CancellationToken) -> Result<RgbImage, String> {
    // KDE's portal saves the picture in the pictures folder and fails without one.
    if let Some(pictures) = dirs::picture_dir().or_else(|| dirs::home_dir().map(|home| home.join("Pictures"))) {
        let _ = tokio::fs::create_dir_all(pictures).await;
    }
    let conn = bus().await?;
    let t = name();
    let options = Options::from([
        ("handle_token", Value::from(t.as_str())),
        ("interactive", Value::from(false)),
        ("modal", Value::from(false)),
    ]);
    let results = request(&conn, SCREENSHOT, "Screenshot", &("", options), &t, cancel)
        .await
        .map_err(|r| match r {
            Refused::Declined => "You did not allow Demido to take screenshots of the screen.".to_string(),
            Refused::Failed(e) => format!("The screen could not be captured: {e}"),
        })?;
    let path = text(&results, "uri")
        .and_then(|uri| url::Url::parse(&uri).ok())
        .and_then(|url| url.to_file_path().ok())
        .ok_or("The desktop gave no screenshot.")?;
    tokio::task::spawn_blocking(move || {
        let read = image::open(&path);
        // The portal saved it for this request alone, where the person keeps their pictures.
        let _ = std::fs::remove_file(&path);
        read.map(|i| i.to_rgb8())
            .map_err(|e| format!("The screenshot could not be read: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The connection to the session bus the portals are reached on, registered as Demido.
async fn bus() -> Result<zbus::Connection, String> {
    static BUS: tokio::sync::OnceCell<zbus::Connection> = tokio::sync::OnceCell::const_new();
    BUS.get_or_try_init(|| async {
        let conn = zbus::Connection::session()
            .await
            .map_err(|e| format!("The desktop's message bus could not be reached: {e}"))?;
        // Portals older than 1.19 have no registry, and know Demido by no app id: they still work.
        let ids = tokio::task::spawn_blocking(app_ids).await.unwrap_or_default();
        for id in ids {
            let registered = conn
                .call_method(
                    Some(DESTINATION),
                    PATH,
                    Some(REGISTRY),
                    "Register",
                    &(id.as_str(), none()),
                )
                .await;
            match registered {
                Ok(_) => {
                    tracing::info!("registered with the desktop portal as {id}");
                    break;
                }
                Err(e) => tracing::info!("the desktop portal did not take {id} as Demido's app id: {e}"),
            }
        }
        Ok(conn)
    })
    .await
    .cloned()
}

/// The app ids Demido may be installed under, best first: the package's, then the other desktop
/// entries named for Demido, those that run this AppImage first.
fn app_ids() -> Vec<String> {
    let env = |key: &str| std::env::var_os(key).filter(|v| !v.is_empty());
    let home = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system = env("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let dirs = home.into_iter().chain(std::env::split_paths(&system));
    let appimage = env("APPIMAGE").and_then(|p| p.into_string().ok());
    let mut found: Vec<(bool, String)> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir.join("applications")) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = name.strip_suffix(".desktop") else {
                continue;
            };
            if id == APP_ID || !id.to_ascii_lowercase().starts_with("demido") || found.iter().any(|(_, f)| f == id) {
                continue;
            }
            let runs_this = appimage
                .as_deref()
                .is_some_and(|path| std::fs::read_to_string(entry.path()).is_ok_and(|text| text.contains(path)));
            found.push((runs_this, id.to_string()));
        }
    }
    found.sort_by_key(|(runs_this, _)| !runs_this);
    std::iter::once(APP_ID.to_string())
        .chain(found.into_iter().map(|(_, id)| id))
        .collect()
}

/// Calls `method` of the portal's `interface` with `body`, which names `token` as its
/// `handle_token`, and waits for the request's answer.
async fn request<B>(
    conn: &zbus::Connection,
    interface: &str,
    method: &str,
    body: &B,
    token: &str,
    cancel: &CancellationToken,
) -> Result<Results, Refused>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let failed = |e: zbus::Error| Refused::Failed(e.to_string());
    let sender = conn
        .unique_name()
        .ok_or_else(|| Refused::Failed("no name on the bus".into()))?
        .trim_start_matches(':')
        .replace('.', "_");
    // The request's path is known before the call, so its answer cannot be missed.
    let path = format!("{PATH}/request/{sender}/{token}");
    let request = zbus::Proxy::new(conn, DESTINATION, path, "org.freedesktop.portal.Request")
        .await
        .map_err(failed)?;
    let mut answers = request.receive_signal("Response").await.map_err(failed)?;
    let portal = zbus::Proxy::new(conn, DESTINATION, PATH, interface)
        .await
        .map_err(failed)?;
    // The answer may wait for the person (a dialog); the call itself is taken at once.
    match tokio::time::timeout(Duration::from_secs(30), portal.call_method(method, body)).await {
        Ok(called) => called.map_err(failed)?,
        Err(_) => return Err(Refused::Failed("the desktop portal did not answer".into())),
    };
    let answer = tokio::select! {
        answer = answers.next() => answer,
        _ = cancel.cancelled() => {
            let _ = request.call_method("Close", &()).await;
            return Err(Refused::Failed("stopped".into()));
        }
    };
    let answer = answer.ok_or_else(|| Refused::Failed("the portal did not answer".into()))?;
    let (code, results): (u32, Results) = answer.body().deserialize().map_err(failed)?;
    match code {
        0 => Ok(results),
        1 => Err(Refused::Declined),
        _ => Err(Refused::Failed(format!("the portal refused the {method} request"))),
    }
}

/// A name for a request or session, unique in this process.
fn name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("demido{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn none() -> Options<'static> {
    Options::new()
}

fn text(results: &Results, key: &str) -> Option<String> {
    results
        .get(key)
        .and_then(|v| <&str>::try_from(v).ok())
        .map(str::to_string)
}

/// The shared screens: `a(ua{sv})`, each a PipeWire node with its position and size.
fn streams(value: &OwnedValue) -> Vec<Stream> {
    let Ok(Value::Array(list)) = value.try_clone().map(Value::from) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|item| {
            let Value::Structure(fields) = item else {
                return None;
            };
            let [Value::U32(node), Value::Dict(props)] = fields.fields() else {
                return None;
            };
            let pair = |key: &str| -> Option<(f64, f64)> {
                let value: Value = props.get(&key).ok()??;
                let value = match value {
                    Value::Value(inner) => *inner,
                    other => other,
                };
                let Value::Structure(pair) = value else {
                    return None;
                };
                match pair.fields() {
                    [Value::I32(a), Value::I32(b)] => Some((f64::from(*a), f64::from(*b))),
                    _ => None,
                }
            };
            let (width, height) = pair("size")?;
            let (x, y) = pair("position").unwrap_or((0.0, 0.0));
            Some(Stream {
                node: *node,
                rect: Rect { x, y, width, height },
            })
        })
        .collect()
}

fn code(button: Button) -> i32 {
    match button {
        Button::Left => BTN_LEFT,
        Button::Middle => BTN_MIDDLE,
        Button::Right => BTN_RIGHT,
    }
}

/// What one wheel notch is to the portal. KDE's takes the scroll distance, of which a notch is
/// 15; the others take notches.
fn scroll_unit() -> i32 {
    let kde = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("KDE")));
    if kde { 15 } else { 1 }
}
