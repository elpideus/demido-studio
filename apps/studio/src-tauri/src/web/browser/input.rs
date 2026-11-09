//! Input to a tab as the person's own mouse and keyboard give it, and a picture of what the tab
//! shows. Events a script sends are marked as not the person's, and some pages heed only real
//! ones (as does every chart drawn on a canvas), so the assistant's clicks at a point, its keys
//! and its typing where the page has the keyboard go to the tab the way the person's would.
//!
//! On Linux they go to the tab's WebKitGTK view as GDK events, as GTK hands it the person's own;
//! on Windows through WebView2's DevTools protocol (`Input.*`, `Page.captureScreenshot`).
//! Elsewhere (macOS) there is none of it ([`NATIVE`] is false): the assistant's clicks and keys go
//! as events a script sends, and it takes no screenshots.

#[cfg(any(target_os = "linux", windows))]
use std::time::Duration;

use serde_json::{Value, json};
use tauri::Webview;
#[cfg(any(target_os = "linux", windows))]
use tokio::sync::oneshot;

/// Whether tabs take input and screenshots as described above on this system.
pub const NATIVE: bool = cfg!(any(target_os = "linux", windows));

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    pub fn parse(name: Option<&str>) -> Result<Button, String> {
        match name.map(|n| n.trim().to_ascii_lowercase()).as_deref() {
            None | Some("") | Some("left") => Ok(Button::Left),
            Some("right") => Ok(Button::Right),
            Some("middle") => Ok(Button::Middle),
            Some(other) => Err(format!("\"{other}\" is not a mouse button: left, right or middle.")),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Button::Left => "left",
            Button::Middle => "middle",
            Button::Right => "right",
        }
    }
}

/// A key to press, as the model names it: "Enter", "Control+a", "Shift+Tab", "ArrowDown", "x".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// The DOM's `key`: "Enter", "a", "ArrowDown".
    pub key: String,
    /// The DOM's `code`: "Enter", "KeyA", "ArrowDown"; empty for a character without one.
    pub code: String,
    /// Windows' virtual-key code, which is also the DOM's `keyCode`; 0 when there is none.
    pub vk: u32,
    /// The X11 keysym of a key with a name (characters get theirs from GDK).
    keysym: u32,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// Keys by name: the names the model may use, the DOM's key and code, the virtual-key code and
/// the X11 keysym.
const NAMED: &[(&[&str], &str, &str, u32, u32)] = &[
    (&["enter", "return"], "Enter", "Enter", 13, 0xff0d),
    (&["tab"], "Tab", "Tab", 9, 0xff09),
    (&["escape", "esc"], "Escape", "Escape", 27, 0xff1b),
    (&["backspace"], "Backspace", "Backspace", 8, 0xff08),
    (&["delete", "del"], "Delete", "Delete", 46, 0xffff),
    (&["space", "spacebar"], " ", "Space", 32, 0x20),
    (&["arrowleft", "left"], "ArrowLeft", "ArrowLeft", 37, 0xff51),
    (&["arrowup", "up"], "ArrowUp", "ArrowUp", 38, 0xff52),
    (&["arrowright", "right"], "ArrowRight", "ArrowRight", 39, 0xff53),
    (&["arrowdown", "down"], "ArrowDown", "ArrowDown", 40, 0xff54),
    (&["home"], "Home", "Home", 36, 0xff50),
    (&["end"], "End", "End", 35, 0xff57),
    (&["pageup"], "PageUp", "PageUp", 33, 0xff55),
    (&["pagedown"], "PageDown", "PageDown", 34, 0xff56),
    (&["insert"], "Insert", "Insert", 45, 0xff63),
];

impl Key {
    pub fn parse(spec: &str) -> Result<Key, String> {
        let spec = spec.trim();
        let (mods, name) = if spec == "+" {
            ("", "+")
        } else if let Some(mods) = spec.strip_suffix("++") {
            (mods, "+")
        } else {
            match spec.rsplit_once('+') {
                Some((mods, name)) => (mods, name),
                None => ("", spec),
            }
        };
        let mut key = Key::named(name.trim())?;
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => key.ctrl = true,
                "shift" => key.shift = true,
                "alt" | "option" => key.alt = true,
                "meta" | "cmd" | "command" | "super" | "win" | "windows" => key.meta = true,
                _ => {
                    return Err(format!("\"{m}\" is not a modifier key: Control, Shift, Alt or Meta."));
                }
            }
        }
        // Shift and a letter make the capital, as a keyboard does.
        if key.shift && key.key.chars().count() == 1 && key.key.chars().all(char::is_alphabetic) {
            key.key = key.key.to_uppercase();
        }
        Ok(key)
    }

    fn named(name: &str) -> Result<Key, String> {
        let lower = name.to_ascii_lowercase();
        let plain = |key: String, code: String, vk: u32, keysym: u32| Key {
            key,
            code,
            vk,
            keysym,
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        };
        if let Some((_, key, code, vk, keysym)) = NAMED.iter().find(|(names, ..)| names.contains(&lower.as_str())) {
            return Ok(plain(key.to_string(), code.to_string(), *vk, *keysym));
        }
        if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u32>().ok())
            && (1..=12).contains(&n)
        {
            return Ok(plain(format!("F{n}"), format!("F{n}"), 111 + n, 0xffbd + n));
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => {
                let (code, vk) = if c.is_ascii_alphabetic() {
                    (format!("Key{}", c.to_ascii_uppercase()), c.to_ascii_uppercase() as u32)
                } else if c.is_ascii_digit() {
                    (format!("Digit{c}"), c as u32)
                } else {
                    (String::new(), 0)
                };
                Ok(plain(c.to_string(), code, vk, 0))
            }
            _ => Err(format!(
                "\"{name}\" is not a key. Name one: Enter, Tab, Escape, Backspace, Delete, Space, ArrowUp, \
                 ArrowDown, ArrowLeft, ArrowRight, Home, End, PageUp, PageDown, F1–F12 or a single character, \
                 with modifiers before it like Control+a or Shift+Tab."
            )),
        }
    }

    /// The key's X11 keysym: a named key's own, or its character's.
    pub fn keysym(&self) -> u32 {
        match (self.keysym, self.key.chars().next()) {
            (0, Some(c)) => crate::desktop::char_keysym(c),
            (keysym, _) => keysym,
        }
    }

    /// Whether the key types its character: one character, with no modifier but Shift.
    pub fn types(&self) -> bool {
        self.key.chars().count() == 1 && !self.ctrl && !self.alt && !self.meta
    }

    /// Whether the key copies, cuts or pastes: Control+C, Shift+Insert and the like. The
    /// keyboard's own keys reach the person's clipboard, which may hold a password.
    pub fn reaches_clipboard(&self) -> bool {
        let letter = self.key.to_ascii_lowercase();
        ((self.ctrl || self.meta) && matches!(letter.as_str(), "c" | "v" | "x"))
            || (self.key == "Insert" && (self.ctrl || self.shift))
            || (self.key == "Delete" && self.shift)
    }

    /// The key for `page.js`'s `press`, which sends it by events a script sends.
    pub fn for_page(&self) -> Value {
        json!({
            "key": self.key,
            "code": self.code,
            "keyCode": self.vk,
            "ctrl": self.ctrl,
            "shift": self.shift,
            "alt": self.alt,
            "meta": self.meta,
        })
    }

    /// How the key reads in a message: "Control+a".
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        for (on, name) in [
            (self.ctrl, "Control"),
            (self.alt, "Alt"),
            (self.shift, "Shift"),
            (self.meta, "Meta"),
        ] {
            if on {
                parts.push(name);
            }
        }
        let key = if self.key == " " { "Space" } else { self.key.as_str() };
        parts.push(key);
        parts.join("+")
    }
}

/// A screenshot as the engine gave it, in device pixels.
pub type Shot = image::RgbImage;

#[cfg(target_os = "linux")]
pub use linux::{click, insert, menus_for_people_only, press, screenshot};
#[cfg(windows)]
pub use windows::{click, insert, menus_for_people_only, press, screenshot};

#[cfg(not(any(target_os = "linux", windows)))]
mod elsewhere {
    use super::{Button, Key, Shot};

    const NONE: &str = "The browser cannot do that on this system.";

    pub async fn click(_tab: &tauri::Webview, _x: f64, _y: f64, _button: Button, _count: u8) -> Result<(), String> {
        Err(NONE.into())
    }

    pub async fn press(_tab: &tauri::Webview, _key: &Key) -> Result<(), String> {
        Err(NONE.into())
    }

    pub async fn insert(_tab: &tauri::Webview, _text: &str) -> Result<(), String> {
        Err(NONE.into())
    }

    pub async fn screenshot(_tab: &tauri::Webview) -> Result<Shot, String> {
        Err(NONE.into())
    }

    /// Events a script sends open no menu here.
    pub fn menus_for_people_only(_tab: &tauri::Webview) {}
}

#[cfg(not(any(target_os = "linux", windows)))]
pub use elsewhere::{click, insert, menus_for_people_only, press, screenshot};

/// Runs `f` on the main thread with the tab's engine view, where `with_webview` runs its
/// closures, and waits for what it sends back, `max` at most.
#[cfg(any(target_os = "linux", windows))]
async fn on_main<T: Send + 'static>(
    tab: &Webview,
    max: Duration,
    f: impl FnOnce(tauri::webview::PlatformWebview, oneshot::Sender<Result<T, String>>) + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = oneshot::channel();
    tab.with_webview(move |w| f(w, tx)).map_err(|e| e.to_string())?;
    tokio::time::timeout(max, rx)
        .await
        .map_err(|_| "The browser did not answer in time.".to_string())?
        .map_err(|_| "That tab was closed.".to_string())?
}

#[cfg(target_os = "linux")]
mod linux {
    use std::time::Duration;

    use gtk::gdk;
    use gtk::glib::translate::{ToGlibPtr, ToGlibPtrMut};
    use gtk::prelude::*;
    use tauri::Webview;
    use webkit2gtk::{SnapshotOptions, SnapshotRegion, WebViewExt};

    use super::{Button, Key, Shot, on_main};

    /// Clicks `count` times with `button` at `x`, `y` of the tab's view (CSS pixels, which are
    /// GTK's at the page's own zoom). WebKit takes the keyboard on a click; it goes back to what had
    /// it, so the person's typing in the app does not land in the tab.
    pub async fn click(tab: &Webview, x: f64, y: f64, button: Button, count: u8) -> Result<(), String> {
        on_main(tab, Duration::from_secs(5), move |w, tx| {
            let view = w.inner();
            let widget: &gtk::Widget = view.upcast_ref();
            let _ = tx.send(click_on(widget, x, y, button, count));
        })
        .await
    }

    fn click_on(widget: &gtk::Widget, x: f64, y: f64, button: Button, count: u8) -> Result<(), String> {
        let window = widget.window().ok_or("The tab is not shown yet.")?;
        let (x, y) = in_window(widget, x, y);
        let device = window.display().default_seat().and_then(|s| s.pointer());
        let button = match button {
            Button::Left => 1,
            Button::Middle => 2,
            Button::Right => 3,
        };
        let held = match button {
            1 => gdk::ModifierType::BUTTON1_MASK,
            2 => gdk::ModifierType::BUTTON2_MASK,
            _ => gdk::ModifierType::BUTTON3_MASK,
        };
        let keyboard = KeyboardHolder::take(widget);
        let mut time = now();
        widget.event(&pointer_event(
            gdk::EventType::MotionNotify,
            &window,
            device.as_ref(),
            x,
            y,
            0,
            time,
            gdk::ModifierType::empty(),
        ));
        // A double click is two clicks close together: WebKit counts them itself.
        for _ in 0..count.max(1) {
            time += 1;
            widget.event(&pointer_event(
                gdk::EventType::ButtonPress,
                &window,
                device.as_ref(),
                x,
                y,
                button,
                time,
                gdk::ModifierType::empty(),
            ));
            time += 1;
            widget.event(&pointer_event(
                gdk::EventType::ButtonRelease,
                &window,
                device.as_ref(),
                x,
                y,
                button,
                time,
                held,
            ));
        }
        keyboard.give_back();
        Ok(())
    }

    /// Keeps the menu a right click opens in `tab` for the person's own clicks. The assistant
    /// cannot see or use a native menu, and on the person's screen it would take their mouse and
    /// keyboard; the events the assistant's input sends are marked as sent, unlike the person's.
    pub fn menus_for_people_only(tab: &Webview) {
        let hooked = tab.with_webview(|w| {
            w.inner().connect_context_menu(|_, _, event, _| event.is_send_event());
        });
        if let Err(e) = hooked {
            tracing::debug!("could not reach a browser tab for its menus: {e}");
        }
    }

    /// Presses and releases `key` where the page has the keyboard.
    pub async fn press(tab: &Webview, key: &Key) -> Result<(), String> {
        let key = key.clone();
        on_main(tab, Duration::from_secs(5), move |w, tx| {
            let view = w.inner();
            let widget: &gtk::Widget = view.upcast_ref();
            let _ = tx.send(press_on(widget, &key));
        })
        .await
    }

    fn press_on(widget: &gtk::Widget, key: &Key) -> Result<(), String> {
        let window = widget.window().ok_or("The tab is not shown yet.")?;
        let keysym = if key.keysym != 0 {
            // Shift+Tab is a key of its own to GTK.
            if key.keysym == 0xff09 && key.shift {
                0xfe20
            } else {
                key.keysym
            }
        } else {
            let c = key.key.chars().next().unwrap_or(' ');
            // SAFETY: a plain function of a number.
            unsafe { gdk::ffi::gdk_unicode_to_keyval(c as u32) }
        };
        let display = window.display();
        let (keycode, group) = gdk::Keymap::for_display(&display)
            .and_then(|map| map.entries_for_keyval(keysym).into_iter().next())
            .map(|k| (k.keycode() as u16, k.group() as u8))
            .unwrap_or((0, 0));
        let device = display.default_seat().and_then(|s| s.keyboard());
        let mut state = gdk::ModifierType::empty();
        for (on, mask) in [
            (key.shift, gdk::ModifierType::SHIFT_MASK),
            (key.ctrl, gdk::ModifierType::CONTROL_MASK),
            (key.alt, gdk::ModifierType::MOD1_MASK),
            (key.meta, gdk::ModifierType::SUPER_MASK),
        ] {
            if on {
                state |= mask;
            }
        }
        let time = now();
        for (kind, time) in [(gdk::EventType::KeyPress, time), (gdk::EventType::KeyRelease, time + 1)] {
            let mut event = gdk::Event::new(kind);
            // SAFETY: a key event, so its `key` member is the one in use; the event owns the
            // reference to the window it is given, and frees it with itself.
            unsafe {
                let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
                let k = &mut (*raw).key;
                k.window = window.to_glib_full();
                k.send_event = 1;
                k.time = time;
                k.state = state.bits();
                k.keyval = keysym;
                k.hardware_keycode = keycode;
                k.group = group;
            }
            event.set_device(device.as_ref());
            widget.event(&event);
        }
        Ok(())
    }

    /// Types `text` where the page has the keyboard, as typing it there would.
    pub async fn insert(tab: &Webview, text: &str) -> Result<(), String> {
        let text = text.to_string();
        on_main(tab, Duration::from_secs(5), move |w, tx| {
            w.inner().execute_editing_command_with_argument("InsertText", &text);
            let _ = tx.send(Ok(()));
        })
        .await
    }

    /// What the tab's view shows, as the engine draws it.
    pub async fn screenshot(tab: &Webview) -> Result<Shot, String> {
        on_main(tab, Duration::from_secs(15), move |w, tx| {
            w.inner().snapshot(
                SnapshotRegion::Visible,
                SnapshotOptions::NONE,
                None::<&webkit2gtk::gio::Cancellable>,
                move |surface| {
                    let shot = surface
                        .map_err(|e| format!("The browser could not take the screenshot: {e}"))
                        .and_then(to_image);
                    let _ = tx.send(shot);
                },
            );
        })
        .await
    }

    /// Cairo's picture (ARGB32: B, G, R, A in memory, premultiplied) as RGB over white.
    fn to_image(surface: gtk::cairo::Surface) -> Result<Shot, String> {
        let surface = gtk::cairo::ImageSurface::try_from(surface)
            .map_err(|_| "The browser gave the screenshot in a form that cannot be read.".to_string())?;
        let (width, height, stride) = (
            surface.width().max(0) as u32,
            surface.height().max(0) as u32,
            surface.stride() as usize,
        );
        // RGB24 leaves the alpha byte unset: every pixel is opaque.
        let opaque = match surface.format() {
            gtk::cairo::Format::ARgb32 => false,
            gtk::cairo::Format::Rgb24 => true,
            _ => return Err("The browser gave the screenshot in a form that cannot be read.".into()),
        };
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        surface
            .with_data(|data| {
                for row in 0..height as usize {
                    for px in data[row * stride..][..width as usize * 4].as_chunks::<4>().0 {
                        let white = if opaque { 0 } else { 255 - px[3] };
                        rgb.extend_from_slice(&[
                            px[2].saturating_add(white),
                            px[1].saturating_add(white),
                            px[0].saturating_add(white),
                        ]);
                    }
                }
            })
            .map_err(|e| format!("The screenshot could not be read: {e}"))?;
        Shot::from_raw(width, height, rgb).ok_or_else(|| "The screenshot came out the wrong size.".to_string())
    }

    /// A point of the view in its GDK window, which is the view's own unless it has none.
    fn in_window(widget: &gtk::Widget, x: f64, y: f64) -> (f64, f64) {
        if widget.has_window() {
            return (x, y);
        }
        let a = widget.allocation();
        (x + f64::from(a.x()), y + f64::from(a.y()))
    }

    #[allow(clippy::too_many_arguments)]
    fn pointer_event(
        kind: gdk::EventType,
        window: &gdk::Window,
        device: Option<&gdk::Device>,
        x: f64,
        y: f64,
        button: u32,
        time: u32,
        state: gdk::ModifierType,
    ) -> gdk::Event {
        let (_, left, top) = window.origin();
        let mut event = gdk::Event::new(kind);
        // SAFETY: `kind` says which member of the event is in use, and that one is filled; the
        // event owns the reference to the window it is given, and frees it with itself.
        unsafe {
            let raw: *mut gdk::ffi::GdkEvent = event.to_glib_none_mut().0;
            if kind == gdk::EventType::MotionNotify {
                let m = &mut (*raw).motion;
                m.window = window.to_glib_full();
                m.send_event = 1;
                m.time = time;
                m.x = x;
                m.y = y;
                m.state = state.bits();
                m.x_root = f64::from(left) + x;
                m.y_root = f64::from(top) + y;
            } else {
                let b = &mut (*raw).button;
                b.window = window.to_glib_full();
                b.send_event = 1;
                b.time = time;
                b.x = x;
                b.y = y;
                b.state = state.bits();
                b.button = button;
                b.x_root = f64::from(left) + x;
                b.y_root = f64::from(top) + y;
            }
        }
        event.set_device(device);
        event
    }

    /// Event times, in milliseconds, as GDK's own count them.
    fn now() -> u32 {
        (gtk::glib::monotonic_time() / 1000) as u32
    }

    /// Where the keyboard was in the app's window before WebKit took it for a click.
    struct KeyboardHolder {
        window: Option<gtk::Window>,
        had: Option<gtk::Widget>,
        tab: gtk::Widget,
    }

    impl KeyboardHolder {
        fn take(tab: &gtk::Widget) -> Self {
            let window = tab.toplevel().and_then(|t| t.downcast::<gtk::Window>().ok());
            let had = window.as_ref().and_then(|w| w.focused_widget());
            KeyboardHolder {
                window,
                had,
                tab: tab.clone(),
            }
        }

        fn give_back(self) {
            match (self.window, self.had) {
                (_, Some(had)) if had != self.tab => had.grab_focus(),
                (Some(window), None) => window.set_focus(None::<&gtk::Widget>),
                _ => {}
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use parking_lot::Mutex;
    use serde_json::{Value, json};
    use tauri::Webview;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_11;
    use webview2_com::{CallDevToolsProtocolMethodCompletedHandler, ContextMenuRequestedEventHandler};
    use windows::core::{HSTRING, Interface};

    use super::{Button, Key, Shot, on_main};

    /// Until when (milliseconds since 1970) a menu a tab asks to open came from the assistant's
    /// input. The DevTools protocol's events look like the person's own to WebView2, so the
    /// moments after the assistant's clicks and keys stand for them.
    static ASSISTANT_UNTIL: AtomicU64 = AtomicU64::new(0);

    fn millis() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Marks the next moments as the assistant's: a menu that opens then is its.
    fn assistant_acts() {
        ASSISTANT_UNTIL.store(millis() + 1500, Ordering::Relaxed);
    }

    /// Keeps the menu a right click opens in `tab` for the person's own clicks. The assistant
    /// cannot see or use a native menu, and on the person's screen it would take their mouse and
    /// keyboard.
    pub fn menus_for_people_only(tab: &Webview) {
        let hooked = tab.with_webview(|w| {
            // SAFETY: WebView2 calls on the window's thread, where `with_webview` runs this; the
            // arguments are valid for the length of the call.
            let result = unsafe {
                w.controller()
                    .CoreWebView2()
                    .and_then(|core| core.cast::<ICoreWebView2_11>())
                    .and_then(|core| {
                        let mut token = Default::default();
                        core.add_ContextMenuRequested(
                            &ContextMenuRequestedEventHandler::create(Box::new(|_, args| {
                                if let Some(args) = args
                                    && millis() < ASSISTANT_UNTIL.load(Ordering::Relaxed)
                                {
                                    args.SetHandled(true)?;
                                }
                                Ok(())
                            })),
                            &mut token,
                        )
                    })
            };
            if let Err(e) = result {
                tracing::debug!("could not keep a browser tab's menus: {e}");
            }
        });
        if let Err(e) = hooked {
            tracing::debug!("could not reach a browser tab for its menus: {e}");
        }
    }

    /// Calls `method` of the DevTools protocol in the tab with `params`, and returns its answer.
    async fn devtools(tab: &Webview, method: &'static str, params: Value) -> Result<Value, String> {
        let params = params.to_string();
        let answer: String = on_main(tab, Duration::from_secs(15), move |w, tx| {
            let tx = Arc::new(Mutex::new(Some(tx)));
            let answer = tx.clone();
            let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |result, json| {
                if let Some(tx) = answer.lock().take() {
                    let _ = tx.send(result.map(|_| json).map_err(|e| e.message()));
                }
                Ok(())
            }));
            // SAFETY: WebView2 calls on the window's thread, where `with_webview` runs this; the
            // strings live for the length of the call.
            let called = unsafe {
                w.controller().CoreWebView2().and_then(|core| {
                    core.CallDevToolsProtocolMethod(&HSTRING::from(method), &HSTRING::from(params), &handler)
                })
            };
            if let Err(e) = called
                && let Some(tx) = tx.lock().take()
            {
                let _ = tx.send(Err(e.message()));
            }
        })
        .await
        .map_err(|e| format!("The browser refused {method}: {e}"))?;
        serde_json::from_str(&answer)
            .map_err(|e| format!("The browser answered {method} with something unreadable: {e}"))
    }

    pub async fn click(tab: &Webview, x: f64, y: f64, button: Button, count: u8) -> Result<(), String> {
        let (name, buttons) = match button {
            Button::Left => ("left", 1),
            Button::Right => ("right", 2),
            Button::Middle => ("middle", 4),
        };
        assistant_acts();
        devtools(
            tab,
            "Input.dispatchMouseEvent",
            json!({"type": "mouseMoved", "x": x, "y": y}),
        )
        .await?;
        for n in 1..=count.max(1) {
            let press =
                json!({"type": "mousePressed", "x": x, "y": y, "button": name, "buttons": buttons, "clickCount": n});
            devtools(tab, "Input.dispatchMouseEvent", press).await?;
            let release =
                json!({"type": "mouseReleased", "x": x, "y": y, "button": name, "buttons": 0, "clickCount": n});
            devtools(tab, "Input.dispatchMouseEvent", release).await?;
        }
        assistant_acts();
        Ok(())
    }

    pub async fn press(tab: &Webview, key: &Key) -> Result<(), String> {
        let modifiers =
            u8::from(key.alt) | u8::from(key.ctrl) << 1 | u8::from(key.meta) << 2 | u8::from(key.shift) << 3;
        // What the key types: its character, and a carriage return for Enter, as Chromium's own
        // keyboard sends.
        let text = match key.key.as_str() {
            "Enter" => "\r".to_string(),
            k if key.types() => k.to_string(),
            _ => String::new(),
        };
        let mut down = json!({
            "type": if text.is_empty() { "rawKeyDown" } else { "keyDown" },
            "key": key.key,
            "code": key.code,
            "windowsVirtualKeyCode": key.vk,
            "nativeVirtualKeyCode": key.vk,
            "modifiers": modifiers,
        });
        if !text.is_empty() {
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
        }
        assistant_acts();
        devtools(tab, "Input.dispatchKeyEvent", down).await?;
        let up = json!({
            "type": "keyUp",
            "key": key.key,
            "code": key.code,
            "windowsVirtualKeyCode": key.vk,
            "nativeVirtualKeyCode": key.vk,
            "modifiers": modifiers,
        });
        devtools(tab, "Input.dispatchKeyEvent", up).await?;
        Ok(())
    }

    pub async fn insert(tab: &Webview, text: &str) -> Result<(), String> {
        devtools(tab, "Input.insertText", json!({"text": text})).await?;
        Ok(())
    }

    pub async fn screenshot(tab: &Webview) -> Result<Shot, String> {
        use base64::Engine as _;
        let answer = devtools(tab, "Page.captureScreenshot", json!({"format": "png"})).await?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(answer["data"].as_str().unwrap_or_default())
            .map_err(|e| format!("The screenshot came back garbled: {e}"))?;
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
            .map_err(|e| format!("The screenshot could not be read: {e}"))?;
        Ok(image.to_rgb8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_read_as_people_name_them() {
        let enter = Key::parse("Enter").unwrap();
        assert_eq!(
            (enter.key.as_str(), enter.code.as_str(), enter.vk),
            ("Enter", "Enter", 13)
        );
        assert_eq!(Key::parse("return").unwrap().key, "Enter");
        let all = Key::parse("Control+a").unwrap();
        assert!(all.ctrl && !all.shift && !all.types());
        assert_eq!((all.key.as_str(), all.code.as_str(), all.vk), ("a", "KeyA", 65));
        let back = Key::parse("shift + tab").unwrap();
        assert!(back.shift && back.key == "Tab");
        assert_eq!(Key::parse("Shift+a").unwrap().key, "A");
        assert_eq!(
            Key::parse("Ctrl+Shift+ArrowDown").unwrap().label(),
            "Control+Shift+ArrowDown"
        );
        assert_eq!(Key::parse("down").unwrap().key, "ArrowDown");
        assert_eq!(Key::parse("F5").unwrap().vk, 116);
        assert_eq!(Key::parse("Space").unwrap().label(), "Space");
        let plus = Key::parse("Control++").unwrap();
        assert!(plus.ctrl && plus.key == "+");
        assert_eq!(Key::parse("+").unwrap().key, "+");
        assert!(Key::parse("é").unwrap().types());
        for k in [
            "Control+v",
            "Ctrl+Shift+V",
            "Meta+c",
            "Control+x",
            "Shift+Insert",
            "Shift+Delete",
        ] {
            assert!(Key::parse(k).unwrap().reaches_clipboard(), "{k}");
        }
        for k in ["v", "Control+a", "Delete", "Insert", "Alt+v"] {
            assert!(!Key::parse(k).unwrap().reaches_clipboard(), "{k}");
        }
        assert!(Key::parse("Hyper+x").is_err());
        assert!(Key::parse("Enterr").is_err());
        assert!(Key::parse("F13").is_err());
    }

    #[test]
    fn buttons_are_named() {
        assert_eq!(Button::parse(None).unwrap(), Button::Left);
        assert_eq!(Button::parse(Some("Right")).unwrap(), Button::Right);
        assert!(Button::parse(Some("back")).is_err());
    }
}
