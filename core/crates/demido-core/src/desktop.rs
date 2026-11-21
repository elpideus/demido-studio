//! What the desktop looks like: for now, its accent colour.
//!
//! "Automatic" in the Accent setting means "use my system's accent". A browser cannot read that; the core
//! can, when it runs on the same computer as the window.
//!
//! On Linux the desktop's portal answers (`org.freedesktop.appearance`, `accent-color`: KDE Plasma 6 and
//! GNOME 47 and later). The core asks through `busctl`, and through `gdbus` if that is missing: both come
//! with nearly every Linux desktop. A D-Bus library would add some thirty packages to answer one question,
//! so it waits until a second thing needs D-Bus (DECISIONS K14).
//!
//! Windows and macOS: not built. The answer says so (`supported: false`) instead of pretending there is no accent.

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Appearance {
    /// The system's accent colour as `#rrggbb`. `null` when the system has none or will not say.
    pub accent: Option<String>,
    /// Where the answer came from: `portal`. `null` when there is no answer.
    pub source: Option<String>,
    /// False where reading the system accent is not built yet (Windows, macOS).
    pub supported: bool,
}

/// Asks the desktop. Takes up to a few seconds when the desktop does not answer; call it off the request threads.
pub fn read() -> Appearance {
    #[cfg(target_os = "linux")]
    {
        let accent = by_portal();
        Appearance { source: accent.as_ref().map(|_| "portal".to_string()), accent, supported: true }
    }
    #[cfg(not(target_os = "linux"))]
    {
        Appearance { accent: None, source: None, supported: false }
    }
}

#[cfg(target_os = "linux")]
fn by_portal() -> Option<String> {
    use std::process::Command;
    const DEST: &str = "org.freedesktop.portal.Desktop";
    const PATH: &str = "/org/freedesktop/portal/desktop";
    const SPACE: &str = "org.freedesktop.appearance";
    const KEY: &str = "accent-color";
    // LC_ALL=C: numbers with a point, whatever the system's language.
    let ask = |program: &str, args: &[&str]| -> Option<String> {
        let out = Command::new(program).args(args).env("LC_ALL", "C").output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let said = ask("busctl", &["--user", "--timeout=2", "call", DEST, PATH, "org.freedesktop.portal.Settings", "ReadOne", "ss", SPACE, KEY])
        .or_else(|| ask("gdbus", &["call", "--session", "--timeout", "2", "--dest", DEST, "--object-path", PATH, "--method", "org.freedesktop.portal.Settings.ReadOne", SPACE, KEY]))?;
    hex(rgb(&said)?)
}

/// Finds the three numbers in the portal's answer, however the tool wrote them:
/// `v (ddd) 0.901961 0.647059 0.85098` (busctl) or `(<(0.9019, 0.6470, 0.8509)>,)` (gdbus).
/// Anything else (two numbers, four, words) is not an answer.
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn rgb(text: &str) -> Option<[f64; 3]> {
    let numbers: Vec<f64> = text
        .split(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | 'e' | 'E' | '+')))
        .filter(|part| part.bytes().any(|b| b.is_ascii_digit()))
        .map(str::parse::<f64>)
        .collect::<Result<_, _>>()
        .ok()?;
    <[f64; 3]>::try_from(numbers).ok()
}

/// The portal gives each part from 0 to 1. A part outside that range is its way of saying "no accent is set".
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn hex(rgb: [f64; 3]) -> Option<String> {
    if rgb.iter().any(|c| !(0.0..=1.0).contains(c)) {
        return None;
    }
    let part = |c: f64| (c * 255.0).round() as u8;
    Some(format!("#{:02x}{:02x}{:02x}", part(rgb[0]), part(rgb[1]), part(rgb[2])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_tools_answers_are_read() {
        // What KDE Plasma 6 answered on the computer this was written on.
        assert_eq!(rgb("v (ddd) 0.901961 0.647059 0.85098\n").and_then(hex).as_deref(), Some("#e6a5d9"));
        assert_eq!(rgb("(<(0.90196079015731812, 0.64705884456634521, 0.85098040103912354)>,)\n").and_then(hex).as_deref(), Some("#e6a5d9"));
        assert_eq!(rgb("v (ddd) 0 0 0").and_then(hex).as_deref(), Some("#000000"));
        assert_eq!(rgb("v (ddd) 1 1 1").and_then(hex).as_deref(), Some("#ffffff"));
    }

    #[test]
    fn no_accent_and_nonsense_are_not_a_colour() {
        // Out of range is the portal's "none set".
        assert_eq!(rgb("v (ddd) -1 -1 -1").and_then(hex), None);
        assert_eq!(rgb("v (ddd) 2 0.5 0.5").and_then(hex), None);
        // Not three numbers.
        assert_eq!(rgb("v u 1"), None);
        assert_eq!(rgb("v (dddd) 0.1 0.2 0.3 0.4"), None);
        assert_eq!(rgb(""), None);
        assert_eq!(rgb("Call failed: No such interface"), None);
    }
}
