//! The key that every request needs, even from this computer.
//!
//! On first start the core makes a long random key and writes it to a file only the user can read.
//! Programs on this computer (the command line, the window) read that file and send the key with each
//! request. A browser must not carry the key in an address, where it would end up in history and logs.
//! So a browser gets in through a one-use link: the link is traded for a session, and the session lives in
//! a cookie that scripts cannot read.
//!
//! This is for one user on one computer. Use from other devices adds passkeys and encryption later.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::Result;

/// How long a one-use link stays good.
pub const LINK_LIFETIME: Duration = Duration::from_secs(60);
pub const COOKIE: &str = "demido_session";

pub struct Auth {
    key: String,
    links: Mutex<HashMap<String, Instant>>,
    sessions: Mutex<HashSet<String>>,
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
        Ok(Self::with_key(key))
    }

    pub fn with_key(key: String) -> Self {
        Self { key, links: Mutex::new(HashMap::new()), sessions: Mutex::new(HashSet::new()) }
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
        self.sessions.lock().unwrap_or_else(|e| e.into_inner()).insert(session.clone());
        Some(session)
    }

    /// True when the `Cookie` header carries a live session.
    pub fn cookie_ok(&self, header: Option<&str>) -> bool {
        let Some(header) = header else { return false };
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        header.split(';').filter_map(|p| p.trim().strip_prefix(COOKIE)?.strip_prefix('=')).any(|v| sessions.contains(v))
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
