//! The key that every request needs, even from this computer.
//!
//! On first start the core makes a long random key and writes it to a file only the user can read.
//! Programs on this computer (the command line, the window) read that file and send the key with each
//! request. A browser must not carry the key in an address, where it would end up in history and logs.
//! So a browser gets in through a one-use link: the link is traded for a session, and the session lives in
//! a cookie that scripts cannot read.
//!
//! A browser that got in stays in when the server is restarted: the server keeps a fingerprint of each
//! session beside the key (a SHA-256 of it, not the session itself, so the file lets nobody in). A session
//! is good for thirty days from the day it was made.
//!
//! This is for one user on one computer. Use from other devices adds passkeys and encryption later.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::{Result, now_ms};

/// How long a one-use link stays good.
pub const LINK_LIFETIME: Duration = Duration::from_secs(60);
pub const COOKIE: &str = "demido_session";

/// The name of the session cookie for the address a browser used: `demido_session_7341`. Named after the
/// port, because a browser keeps one cookie jar for a host whatever the port.
pub fn cookie_name(host: Option<&str>) -> String {
    match host.and_then(|h| h.rsplit_once(':')).map(|(_, port)| port).filter(|port| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())) {
        Some(port) => format!("{COOKIE}_{port}"),
        None => COOKIE.to_string(),
    }
}
/// How long a browser stays signed in.
const SESSION_LIFETIME_MS: i64 = 30 * 86_400_000;
/// More browsers than this are not remembered; the oldest are forgotten first.
const MOST_SESSIONS: usize = 50;

pub struct Auth {
    key: String,
    links: Mutex<HashMap<String, Instant>>,
    /// The fingerprint of each live session, and when it was made (milliseconds since 1970).
    sessions: Mutex<HashMap<String, i64>>,
    /// Where the fingerprints are kept, so a restart does not sign every browser out. `None` in tests.
    kept: Option<PathBuf>,
}

/// What is kept of a session: enough to recognise it, not enough to use it.
fn fingerprint(session: &str) -> String {
    Sha256::digest(session.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn random() -> String {
    // Two random UUIDs: 244 bits from the system's generator.
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

/// Compares without stopping at the first difference, so timing says nothing about the key.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Auth {
    /// Reads the key file, or makes it on first start.
    pub fn open(path: &Path) -> Result<Self> {
        let key = match std::fs::read_to_string(path) {
            Ok(k) if k.trim().len() >= 32 => k.trim().to_string(),
            _ => {
                let k = random();
                write_private(path, &k)?;
                k
            }
        };
        // The browsers that were signed in when the server last ran, less the ones that are too old by now.
        let kept = path.with_file_name("browsers");
        let sessions = std::fs::read_to_string(&kept)
            .unwrap_or_default()
            .lines()
            .filter_map(|line| line.split_once(' ').and_then(|(print, made)| Some((print.to_string(), made.parse::<i64>().ok()?))))
            .filter(|(print, made)| print.len() == 64 && now_ms() - made < SESSION_LIFETIME_MS)
            .collect();
        Ok(Self { key, links: Mutex::new(HashMap::new()), sessions: Mutex::new(sessions), kept: Some(kept) })
    }

    pub fn with_key(key: String) -> Self {
        Self { key, links: Mutex::new(HashMap::new()), sessions: Mutex::new(HashMap::new()), kept: None }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// True for `Bearer <the key>`.
    pub fn bearer_ok(&self, header: Option<&str>) -> bool {
        header.and_then(|h| h.strip_prefix("Bearer ")).is_some_and(|k| same(k.trim(), &self.key))
    }

    /// Makes a link that works once, for a minute.
    pub fn new_link(&self) -> String {
        let code = random();
        let mut links = self.links.lock().unwrap_or_else(|e| e.into_inner());
        links.retain(|_, made| made.elapsed() < LINK_LIFETIME);
        links.insert(code.clone(), Instant::now());
        code
    }

    /// Trades a link for a session. A link works once.
    pub fn redeem(&self, code: &str) -> Option<String> {
        let made = self.links.lock().unwrap_or_else(|e| e.into_inner()).remove(code)?;
        if made.elapsed() >= LINK_LIFETIME {
            return None;
        }
        let session = random();
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sessions.insert(fingerprint(&session), now_ms());
        sessions.retain(|_, made| now_ms() - *made < SESSION_LIFETIME_MS);
        while sessions.len() > MOST_SESSIONS {
            let oldest = sessions.iter().min_by_key(|(_, made)| **made).map(|(print, _)| print.clone()).expect("not empty");
            sessions.remove(&oldest);
        }
        if let Some(kept) = &self.kept {
            let lines: String = sessions.iter().map(|(print, made)| format!("{print} {made}\n")).collect();
            if let Err(e) = write_private(kept, &lines) {
                tracing::warn!("The sign-in could not be kept on disk, so a restart will ask for it again: {e}");
            }
        }
        Some(session)
    }

    /// True when the `Cookie` header carries a live session.
    pub fn cookie_ok(&self, header: Option<&str>) -> bool {
        let Some(header) = header else { return false };
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        // The cookie is named after the port it was given on (`cookie_name`): a browser keeps one jar for a
        // host whatever the port, so two servers on one computer would otherwise take each other's place.
        header
            .split(';')
            .filter_map(|p| p.trim().split_once('='))
            .filter(|(name, _)| {
                name.strip_prefix(COOKIE).is_some_and(|rest| rest.is_empty() || rest.strip_prefix('_').is_some_and(|port| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())))
            })
            .any(|(_, v)| sessions.get(&fingerprint(v)).is_some_and(|made| now_ms() - *made < SESSION_LIFETIME_MS))
    }
}

/// Writes a file that only the user can read.
pub(crate) fn write_private(path: &Path, text: &str) -> Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(text.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        // On Windows the user's data folder is already private to the user.
        std::fs::write(path, text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_right_key_gets_in() {
        let a = Auth::with_key("k".repeat(64));
        assert!(a.bearer_ok(Some(&format!("Bearer {}", "k".repeat(64)))));
        assert!(!a.bearer_ok(Some("Bearer wrong")));
        assert!(!a.bearer_ok(Some(&"k".repeat(64))));
        assert!(!a.bearer_ok(None));
    }

    #[test]
    fn a_link_works_once_and_gives_a_session() {
        let a = Auth::with_key(random());
        let link = a.new_link();
        let session = a.redeem(&link).expect("the link is fresh");
        assert!(a.redeem(&link).is_none(), "a link must not work twice");
        assert!(a.cookie_ok(Some(&format!("other=1; {COOKIE}={session}"))));
        assert!(!a.cookie_ok(Some(&format!("{COOKIE}=made-up"))));
        // Named after the port it was given on, beside another server's cookie for the same host.
        assert_eq!(
            (cookie_name(Some("127.0.0.1:7341")).as_str(), cookie_name(Some("demido.example")).as_str(), cookie_name(None).as_str()),
            ("demido_session_7341", "demido_session", "demido_session")
        );
        assert!(a.cookie_ok(Some(&format!("demido_session_7399=someone-elses; demido_session_7341={session}"))));
        assert!(!a.cookie_ok(Some(&format!("demido_session_evil={session}"))) && !a.cookie_ok(Some(&format!("demido_sessions={session}"))));
    }

    #[test]
    fn a_browser_stays_signed_in_when_the_server_is_started_again() {
        let dir = std::env::temp_dir().join(format!("demido-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        let first = Auth::open(&path).unwrap();
        let session = first.redeem(&first.new_link()).unwrap();
        let cookie = format!("{COOKIE}={session}");
        drop(first);

        // A new start: the same browser is still let in, and a made-up session is not.
        let again = Auth::open(&path).unwrap();
        assert!(again.cookie_ok(Some(&cookie)));
        assert!(!again.cookie_ok(Some(&format!("{COOKIE}={}", "f".repeat(64)))));

        // What is on disk recognises the session and cannot be used as one.
        let on_disk = std::fs::read_to_string(dir.join("browsers")).unwrap();
        assert!(!on_disk.contains(&session), "the session itself is never written down");
        let print = on_disk.split(' ').next().unwrap().to_string();
        assert!(!again.cookie_ok(Some(&format!("{COOKIE}={print}"))), "the fingerprint does not get anyone in");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("browsers")).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // A session made more than thirty days ago is forgotten at the next start.
        std::fs::write(dir.join("browsers"), format!("{print} {}\n", now_ms() - SESSION_LIFETIME_MS - 1)).unwrap();
        assert!(!Auth::open(&path).unwrap().cookie_ok(Some(&cookie)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn the_key_file_is_made_once_and_then_reused() {
        let dir = std::env::temp_dir().join(format!("demido-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("key");
        let first = Auth::open(&path).unwrap().key().to_string();
        let second = Auth::open(&path).unwrap().key().to_string();
        assert_eq!(first, second);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(dir).ok();
    }
}
