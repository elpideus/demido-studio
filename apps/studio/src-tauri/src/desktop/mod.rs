//! The person's own screen, mouse and keyboard, for the computer-use tools (`tools::computer`).
//!
//! On Linux there are two ways in, by the kind of session:
//!
//! - X11 ([`x11`]): the X server's XTEST extension moves the pointer, clicks and presses keys as
//!   the devices would, and the screen is read from the root window. Nothing is asked: any X client
//!   may.
//! - Wayland ([`portal`]): the compositor lets no program do either on its own, so the desktop's
//!   portals ask the person. The Screenshot portal takes pictures (asking once whether Demido may),
//!   and a RemoteDesktop session, which the person allows in the desktop's own dialog, carries the
//!   pointer and keys. The session is remembered (a restore token in the settings), so the dialog
//!   comes once, and it ends after a few idle minutes; when the person ends it from the desktop,
//!   the next use asks again. For an X11 app, characters the keyboard layout lacks are lent
//!   spare keys of Xwayland's keymap first ([`Lent`]).
//!
//! Positions are in the desktop's coordinates (logical pixels); `tools::computer` turns the
//! screenshot pixels the model gives into them.

use std::sync::Arc;
use std::time::Duration;

use image::RgbImage;
use tokio_util::sync::CancellationToken;

use crate::settings::SettingsStore;
use crate::web::browser::input::Button;

#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "linux")]
mod x11;

/// Whether this system's screen, mouse and keyboard can be used.
pub const SUPPORTED: bool = cfg!(target_os = "linux");

/// The X11 keysyms of the modifier keys (the left ones).
pub const CONTROL: u32 = 0xffe3;
pub const SHIFT: u32 = 0xffe1;
pub const ALT: u32 = 0xffe9;
pub const SUPER: u32 = 0xffeb;

/// A rectangle of the desktop, in its coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// Only Linux's desktop is reached yet (`portal`); the tests use these everywhere.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl Rect {
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Rect) -> Rect {
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        Rect {
            x,
            y,
            width: (self.x + self.width).max(other.x + other.width) - x,
            height: (self.y + self.height).max(other.y + other.height) - y,
        }
    }
}

/// One thing done with the mouse or the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// The pointer to a point of the desktop.
    MoveTo {
        x: f64,
        y: f64,
    },
    Press(Button),
    Release(Button),
    /// Wheel notches: positive is right and down.
    Scroll {
        dx: i32,
        dy: i32,
    },
    /// A key by its X11 keysym.
    KeyDown(u32),
    KeyUp(u32),
    /// Text, typed a character at a time.
    Type(String),
    /// A key by its X11 keycode, pressed and released: on Wayland, a key of Xwayland's keymap
    /// lent a character (see `Lent`).
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Tap(u8),
    Pause(Duration),
}

/// The X11 keysym that types `c`: Latin-1 characters are their own, others are Unicode's
/// (0x01000000 and the code point), and a line break, tab and backspace are those keys.
pub fn char_keysym(c: char) -> u32 {
    match c {
        '\n' | '\r' => 0xff0d,
        '\t' => 0xff09,
        '\u{8}' => 0xff08,
        ' '..='~' | '\u{a0}'..='\u{ff}' => c as u32,
        _ => 0x0100_0000 | c as u32,
    }
}

/// What kind of session the desktop is.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Server {
    X11,
    Wayland,
}

#[cfg(target_os = "linux")]
fn server() -> Result<Server, String> {
    let set = |key: &str| std::env::var_os(key).is_some_and(|v| !v.is_empty());
    if set("WAYLAND_DISPLAY") {
        Ok(Server::Wayland)
    } else if set("DISPLAY") {
        Ok(Server::X11)
    } else {
        Err("There is no desktop to use here: Demido is not running in a graphical session.".into())
    }
}

/// The desktop as the computer-use tools reach it.
pub struct Desktop {
    settings: Arc<SettingsStore>,
    #[cfg(target_os = "linux")]
    session: portal::Shared,
}

impl Desktop {
    pub fn new(settings: Arc<SettingsStore>) -> Self {
        Desktop {
            settings,
            #[cfg(target_os = "linux")]
            session: portal::Shared::default(),
        }
    }

    /// A picture of the whole desktop. `asking` is told when the desktop may show the person a
    /// dialog first.
    #[cfg(target_os = "linux")]
    pub async fn screenshot(&self, cancel: &CancellationToken, asking: &(dyn Fn() + Sync)) -> Result<RgbImage, String> {
        match server()? {
            Server::X11 => blocking(|x| x.capture()).await,
            Server::Wayland => {
                asking();
                portal::screenshot(cancel).await
            }
        }
    }

    /// The part of the desktop the pointer can reach. On Wayland it starts the control session,
    /// which may ask the person first (`asking` is told so).
    #[cfg(target_os = "linux")]
    pub async fn area(&self, cancel: &CancellationToken, asking: &(dyn Fn() + Sync)) -> Result<Rect, String> {
        match server()? {
            Server::X11 => blocking(|x| Ok(x.area())).await,
            Server::Wayland => self.session.area(&self.settings, cancel, asking).await,
        }
    }

    /// Does `steps` in order.
    #[cfg(target_os = "linux")]
    pub async fn perform(
        &self,
        steps: Vec<Step>,
        cancel: &CancellationToken,
        asking: &(dyn Fn() + Sync),
    ) -> Result<(), String> {
        match server()? {
            Server::X11 => {
                blocking(move |x| {
                    for step in &steps {
                        x.step(step)?;
                    }
                    Ok(())
                })
                .await
            }
            Server::Wayland => {
                if let [Step::Type(text)] = steps.as_slice()
                    && let Some(lent) = Lent::for_xwayland(text).await
                {
                    let steps = lent_steps(text, &lent.0);
                    // The keys are given back once typed (when `lent` goes).
                    return self.session.perform(&self.settings, &steps, cancel, asking).await;
                }
                self.session.perform(&self.settings, &steps, cancel, asking).await
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn screenshot(
        &self,
        _cancel: &CancellationToken,
        _asking: &(dyn Fn() + Sync),
    ) -> Result<RgbImage, String> {
        let _ = &self.settings;
        Err(ELSEWHERE.into())
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn area(&self, _cancel: &CancellationToken, _asking: &(dyn Fn() + Sync)) -> Result<Rect, String> {
        Err(ELSEWHERE.into())
    }

    #[cfg(not(target_os = "linux"))]
    pub async fn perform(
        &self,
        _steps: Vec<Step>,
        _cancel: &CancellationToken,
        _asking: &(dyn Fn() + Sync),
    ) -> Result<(), String> {
        Err(ELSEWHERE.into())
    }
}

#[cfg(not(target_os = "linux"))]
const ELSEWHERE: &str = "The screen, mouse and keyboard can be used on Linux only for now.";

/// Characters lent spare keys of Xwayland's keymap, given back when dropped.
///
/// On Wayland the desktop types a character its keyboard layout lacks by making a keymap that
/// has it (KWin does), which reaches Wayland apps; but an X11 keymap cannot hold that keymap's
/// keys, so X11 apps would get nothing. Xwayland's keymap is its own to change, though, and a
/// key pressed through the portal reaches an X11 app by it. (Xwayland's XTEST would ask the
/// person again: KWin lets X11 apps send input only once allowed.)
#[cfg(target_os = "linux")]
struct Lent(Vec<(char, u8)>);

#[cfg(target_os = "linux")]
impl Lent {
    /// For `text` typed into the active app: the characters the layout lacks, each on a spare
    /// key, when that app is an X11 one (running through Xwayland). None when it is not, or the
    /// layout has every character.
    async fn for_xwayland(text: &str) -> Option<Lent> {
        if std::env::var_os("DISPLAY").is_none_or(|display| display.is_empty()) {
            return None;
        }
        let text = text.to_string();
        blocking(move |x| {
            if !x.client_active() {
                return Ok(None);
            }
            let mut missing: Vec<char> = Vec::new();
            for c in text.chars().filter(|&c| c != '\r') {
                if !missing.contains(&c) && !x.has_key(char_keysym(c)) {
                    missing.push(c);
                }
            }
            if missing.is_empty() {
                return Ok(None);
            }
            let keysyms: Vec<u32> = missing.iter().map(|&c| char_keysym(c)).collect();
            // With too few spare keys, the rest go as the desktop types them.
            let codes = x.lend_all(&keysyms);
            if codes.is_empty() {
                return Ok(None);
            }
            // Programs reread the keyboard when it changes, before the keys reach them.
            std::thread::sleep(Duration::from_millis(50));
            Ok(Some(Lent(missing.into_iter().zip(codes).collect())))
        })
        .await
        .ok()
        .flatten()
    }
}

#[cfg(target_os = "linux")]
impl Drop for Lent {
    fn drop(&mut self) {
        let codes: Vec<u8> = self.0.iter().map(|&(_, code)| code).collect();
        std::thread::spawn(move || {
            // After the last key reaches its program, which reads it by the keymap of then.
            std::thread::sleep(Duration::from_millis(300));
            if let Ok(mut x) = x11::X::open() {
                x.give_back(&codes);
            }
        });
    }
}

/// `text` as steps: the `lent` characters by their keys, the rest as text.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn lent_steps(text: &str, lent: &[(char, u8)]) -> Vec<Step> {
    let mut steps = Vec::new();
    let mut run = String::new();
    for c in text.chars() {
        match lent.iter().find(|&&(lent, _)| lent == c) {
            Some(&(_, code)) => {
                if !run.is_empty() {
                    steps.push(Step::Type(std::mem::take(&mut run)));
                }
                steps.push(Step::Tap(code));
            }
            None => run.push(c),
        }
    }
    if !run.is_empty() {
        steps.push(Step::Type(run));
    }
    steps
}

/// Runs `work` with a connection to the X server of its own, on a thread that may block.
#[cfg(target_os = "linux")]
async fn blocking<T: Send + 'static>(
    work: impl FnOnce(&mut x11::X) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(move || {
        let mut x = x11::X::open()?;
        work(&mut x)
    })
    .await
    .map_err(|e| format!("The desktop could not be reached: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn characters_have_their_keysyms() {
        assert_eq!(char_keysym('a'), 0x61);
        assert_eq!(char_keysym('A'), 0x41);
        assert_eq!(char_keysym(' '), 0x20);
        assert_eq!(char_keysym('é'), 0xe9);
        assert_eq!(char_keysym('€'), 0x0100_20ac);
        assert_eq!(char_keysym('\n'), 0xff0d);
        assert_eq!(char_keysym('\t'), 0xff09);
    }

    #[test]
    fn rectangles_join() {
        let left = Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        };
        let right = Rect {
            x: 1920.0,
            y: -200.0,
            width: 1440.0,
            height: 2560.0,
        };
        let both = left.union(&right);
        assert_eq!(
            both,
            Rect {
                x: 0.0,
                y: -200.0,
                width: 3360.0,
                height: 2560.0
            }
        );
        assert!(right.contains(1920.0, 0.0));
        assert!(!left.contains(1920.0, 0.0));
    }

    #[test]
    fn lent_characters_go_by_their_keys() {
        let lent = [('é', 248), ('ß', 230)];
        assert_eq!(
            lent_steps("Hé, ßé!", &lent),
            vec![
                Step::Type("H".into()),
                Step::Tap(248),
                Step::Type(", ".into()),
                Step::Tap(230),
                Step::Tap(248),
                Step::Type("!".into()),
            ]
        );
        assert_eq!(lent_steps("plain", &lent), vec![Step::Type("plain".into())]);
    }
}
