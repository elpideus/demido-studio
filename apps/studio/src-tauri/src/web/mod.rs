//! The web: searching it, reading its pages, and the built-in browser.
//!
//! Search and reading go over plain HTTP first, as a browser asks (its user agent and headers),
//! which answers most pages in well under a second. Pages that refuse a program (a bot check, a
//! "too many requests"), or that are empty until their scripts run, are read in the built-in
//! browser instead, in the chat's own tab; a check only a person can pass (a captcha) is shown to
//! the person there to solve ([`browser::Browser::ask`]). See [`search`] and [`fetch`].

pub mod browser;
pub mod fetch;
pub mod page;
pub mod search;

use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, HeaderMap, HeaderValue};
use url::{Host, Url};

use crate::settings::SettingsStore;
use browser::Browser;

/// The browser plain requests pass for. Sites that serve a program less, or nothing, serve a
/// browser its page.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
                          Chrome/144.0.0.0 Safari/537.36";

/// Pages and searches are kept this long, so reading on through a long page, or a search asked
/// twice, does not go back to the site.
const KEEP: Duration = Duration::from_secs(15 * 60);
const KEEP_COUNT: usize = 16;

pub struct Web {
    client: reqwest::Client,
    settings: Arc<SettingsStore>,
    pub browser: Arc<Browser>,
    /// Where documents read from the web (PDFs, Office files) are put to be read.
    cache_dir: PathBuf,
    pages: Mutex<Recent<Arc<fetch::Fetched>>>,
    searches: Mutex<Recent<search::Found>>,
}

impl Web {
    pub fn new(
        app: tauri::AppHandle,
        settings: Arc<SettingsStore>,
        profile: PathBuf,
        cache_dir: PathBuf,
    ) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Web {
            client: client()?,
            settings,
            browser: Browser::new(app, profile),
            cache_dir,
            pages: Mutex::new(Recent::default()),
            searches: Mutex::new(Recent::default()),
        }))
    }
}

/// Whether `url` is on the person's own computer or local network, where a page could reach
/// what the internet cannot.
pub fn on_this_network(url: &Url) -> bool {
    let ip = match url.host() {
        None => return true,
        Some(Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            return !d.contains('.')
                || [
                    ".localhost",
                    ".local",
                    ".lan",
                    ".internal",
                    ".home.arpa",
                    ".intranet",
                    ".corp",
                ]
                .iter()
                .any(|s| d.ends_with(s));
        }
        Some(Host::Ipv4(ip)) => IpAddr::V4(ip),
        Some(Host::Ipv6(ip)) => ip.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(ip)),
    };
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback()
                || ip.is_private()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                // Carrier-grade NAT, where some home networks and VPNs live.
                || (a == 100 && (b & 0xC0) == 64)
        }
        IpAddr::V6(ip) => {
            let first = ip.segments()[0];
            ip.is_loopback() || ip.is_unspecified() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// The client plain requests go out on, asking as a browser does.
fn client() -> reqwest::Result<reqwest::Client> {
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"),
    );
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("en-US,en;q=0.9"));
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
}

/// A few things fetched lately, forgotten after [`KEEP`].
struct Recent<T> {
    items: VecDeque<(String, Instant, T)>,
}

impl<T> Default for Recent<T> {
    fn default() -> Self {
        Recent { items: VecDeque::new() }
    }
}

impl<T: Clone> Recent<T> {
    fn get(&mut self, key: &str) -> Option<T> {
        self.items.retain(|(_, at, _)| at.elapsed() < KEEP);
        self.items.iter().find(|(k, _, _)| k == key).map(|(_, _, v)| v.clone())
    }

    fn put(&mut self, key: String, value: T) {
        self.items.retain(|(k, _, _)| *k != key);
        self.items.push_back((key, Instant::now(), value));
        while self.items.len() > KEEP_COUNT {
            self.items.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_things_are_kept_a_while_and_only_a_few() {
        let mut r: Recent<u32> = Recent::default();
        for i in 0..20 {
            r.put(format!("k{i}"), i);
        }
        assert_eq!(r.get("k0"), None);
        assert_eq!(r.get("k19"), Some(19));
        r.put("k19".into(), 7);
        assert_eq!(r.get("k19"), Some(7));
        assert_eq!(r.items.len(), KEEP_COUNT);
    }
}
