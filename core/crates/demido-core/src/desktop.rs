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
//! On macOS the system itself is asked for its accent colour (`NSColor.controlAccentColor`, in sRGB),
//! through `osascript`'s JavaScript, which every Mac has: the colour itself, not a name looked up in a table.
//!
//! On Windows it is the accent the person chose, as Windows keeps it (`AccentColor` under
//! `HKCU\Software\Microsoft\Windows\DWM`, a number in the order alpha, blue, green, red), read with `reg`,
//! which every Windows has.

use serde::Serialize;
use utoipa::ToSchema;

#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Appearance {
    /// The system's accent colour as `#rrggbb`. `null` when the system has none or will not say.
    pub accent: Option<String>,
    /// Where the answer came from: `portal` (Linux), `appkit` (macOS), `registry` (Windows). `null` when
    /// there is no answer.
    pub source: Option<String>,
    /// False where reading the system accent is not built (a system other than these three).
    pub supported: bool,
}

/// Asks the desktop. Takes up to a few seconds when the desktop does not answer; call it off the request threads.
pub fn read() -> Appearance {
    #[cfg(target_os = "linux")]
    {
        let accent = by_portal();
        Appearance { source: accent.as_ref().map(|_| "portal".to_string()), accent, supported: true }
    }
    #[cfg(target_os = "macos")]
    {
        let accent = by_appkit();
        Appearance { source: accent.as_ref().map(|_| "appkit".to_string()), accent, supported: true }
    }
    #[cfg(windows)]
    {
        let accent = by_registry();
        Appearance { source: accent.as_ref().map(|_| "registry".to_string()), accent, supported: true }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        Appearance { accent: None, source: None, supported: false }
    }
}

/// What a program printed, if it ended well within a few seconds.
#[cfg(any(target_os = "macos", windows))]
fn said(program: &str, args: &[&str]) -> Option<String> {
    let mut child = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn().ok()?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() > std::time::Duration::from_secs(5) => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(30)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

/// The accent macOS draws controls with, in sRGB, as three numbers from 0 to 1.
#[cfg(target_os = "macos")]
fn by_appkit() -> Option<String> {
    let script = "ObjC.import('AppKit'); var c = $.NSColor.controlAccentColor.colorUsingColorSpace($.NSColorSpace.sRGBColorSpace); [c.redComponent, c.greenComponent, c.blueComponent].join(' ')";
    said("/usr/bin/osascript", &["-l", "JavaScript", "-e", script]).as_deref().and_then(rgb).and_then(hex)
}

/// The accent the person chose on Windows.
#[cfg(windows)]
fn by_registry() -> Option<String> {
    said("reg", &["query", r"HKCU\Software\Microsoft\Windows\DWM", "/v", "AccentColor"]).as_deref().and_then(from_registry)
}

/// `AccentColor    REG_DWORD    0xff9c5a00`: alpha, blue, green, red, as one number. As `#rrggbb`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn from_registry(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains("AccentColor") && l.contains("REG_DWORD"))?;
    let number = line.split_whitespace().last()?.trim_start_matches("0x").trim_start_matches("0X");
    let v = u32::from_str_radix(number, 16).ok()?;
    let (r, g, b) = (v & 0xff, (v >> 8) & 0xff, (v >> 16) & 0xff);
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
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
#[cfg_attr(not(any(target_os = "linux", target_os = "macos", test)), allow(dead_code))]
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
#[cfg_attr(not(any(target_os = "linux", target_os = "macos", test)), allow(dead_code))]
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
    fn macos_and_windows_answers_are_read() {
        // osascript prints AppKit's components with a space between them: the same reader as the portal's.
        assert_eq!(rgb("0 0.47843137383461 1\n").and_then(hex).as_deref(), Some("#007aff"));
        // Windows keeps alpha, blue, green, red.
        assert_eq!(from_registry("\r\nHKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\DWM\r\n    AccentColor    REG_DWORD    0xff9c5a00\r\n").as_deref(), Some("#005a9c"));
        assert_eq!(from_registry("ERROR: The system was unable to find the specified registry key or value."), None);
    }

    /// Where the checks run on macOS and Windows, the real system is asked: it answers, or says it has none.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn the_system_is_asked_for_its_accent() {
        let seen = read();
        assert!(seen.supported);
        assert!(seen.accent.as_deref().is_none_or(|a| a.len() == 7 && a.starts_with('#')), "{seen:?}");
        eprintln!("this system's accent: {seen:?}");
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
