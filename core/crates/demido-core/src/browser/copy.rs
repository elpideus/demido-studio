//! Demido's own copy of the browser (DECISIONS Z1): Chrome for Testing's `chrome-headless-shell`, stable
//! channel, downloaded from Google's storage when the person asks, and kept unchanged in
//! `programs/browser/versions/<version>/`.
//!
//! Chrome for Testing lists its builds in JSON, with no checksum. Google's storage gives each file's MD5 with
//! it (`x-goog-hash`): the download is checked against that and its size, which proves it arrived whole as the
//! storage holds it; that it is Google's rests on the connection being to Google's own storage over HTTPS.
//! Said so in the window. A list is kept for an hour (the caching rule, P).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::{Error, Result, now_ms};

/// Where Chrome for Testing lists its newest builds of each channel.
pub const LIST: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";
/// The only place a build is taken from, whatever the list says.
const STORAGE: &str = "https://storage.googleapis.com/chrome-for-testing-public/";
const LIST_IS_FRESH: Duration = Duration::from_secs(60 * 60);

/// Chrome for Testing's name for this computer's platform. `None` where Demido's browser is not offered.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "aarch64") => Some("mac-arm64"),
        ("macos", "x86_64") => Some("mac-x64"),
        // On Windows the debugging pipe is given as handles: not built (Z2).
        _ => None,
    }
}

/// The copy Demido keeps, as its note says (`browser.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Kept {
    pub version: String,
    /// The program itself.
    pub path: String,
    /// What was downloaded, in bytes.
    pub size: u64,
    /// How it was checked, in words.
    pub checked: String,
    pub from: String,
    pub kept_at: i64,
}

/// The newest stable build for this computer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Latest {
    pub version: String,
    pub url: String,
    pub size: Option<u64>,
    /// As Google's storage gives it, in base64.
    #[serde(skip)]
    pub md5: Option<String>,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Downloading {
    pub version: String,
    pub done: u64,
    pub total: Option<u64>,
}

pub struct Copy {
    dir: PathBuf,
    http: reqwest::Client,
    list: String,
    events: Arc<EventLog>,
    running: Mutex<Option<(Arc<AtomicBool>, Downloading)>>,
    listed: Mutex<Option<(Instant, Latest)>>,
}

impl Copy {
    pub fn new(home: &Path, events: Arc<EventLog>) -> Self {
        let http = reqwest::Client::builder().user_agent(crate::web::name()).connect_timeout(Duration::from_secs(15)).build().expect("a client can be made");
        #[allow(unused_mut)]
        let mut list = LIST.to_string();
        // A development build can be pointed at a stand-in list on this computer. A release build cannot: where
        // the browser comes from is not something to leave open.
        #[cfg(debug_assertions)]
        if let Ok(stand_in) = std::env::var("DEMIDO_DEV_BROWSER_LIST") {
            list = stand_in;
        }
        Self { dir: home.join("programs").join("browser"), http, list, events, running: Mutex::default(), listed: Mutex::default() }
    }

    /// The same, with the list at `list`. For tests.
    #[cfg(test)]
    pub(crate) fn listed_at(mut self, list: &str) -> Self {
        self.list = list.into();
        self
    }

    fn note(&self) -> PathBuf {
        self.dir.join("browser.json")
    }

    /// The copy Demido keeps, when it is there.
    pub fn kept(&self) -> Option<Kept> {
        let kept: Kept = serde_json::from_slice(&std::fs::read(self.note()).ok()?).ok()?;
        Path::new(&kept.path).is_file().then_some(kept)
    }

    pub fn downloading(&self) -> Option<Downloading> {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|(_, d)| d.clone())
    }

    /// The newest stable build for this computer, from Chrome for Testing's list, with its size and MD5 from
    /// Google's storage.
    pub async fn latest(&self, fresh: bool) -> Result<Latest> {
        if !fresh
            && let Some((at, latest)) = self.listed.lock().unwrap_or_else(|e| e.into_inner()).clone()
            && at.elapsed() < LIST_IS_FRESH
        {
            return Ok(latest);
        }
        let platform = platform().ok_or_else(|| Error::Invalid("Demido's browser is not offered on this system yet.".into()))?;
        let list: Value = self
            .http
            .get(&self.list)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| Error::Net(format!("Chrome for Testing's list could not be read ({}).", crate::runtime::root_cause(&e))))?
            .json()
            .await
            .map_err(|_| Error::Net("Chrome for Testing's list could not be read.".into()))?;
        let stable = list.pointer("/channels/Stable").ok_or_else(|| Error::Net("Chrome for Testing's list has no stable channel.".into()))?;
        let version = stable.get("version").and_then(Value::as_str).unwrap_or_default().to_string();
        let url = stable
            .pointer("/downloads/chrome-headless-shell")
            .and_then(Value::as_array)
            .and_then(|all| all.iter().find(|d| d.get("platform").and_then(Value::as_str) == Some(platform)))
            .and_then(|d| d.get("url").and_then(Value::as_str))
            .ok_or_else(|| Error::Net(format!("Chrome for Testing's list has no headless shell for {platform}.")))?
            .to_string();
        let ours = url.starts_with(STORAGE) || (cfg!(debug_assertions) && self.list != LIST);
        if !ours || !version.chars().all(|c| c.is_ascii_digit() || c == '.') || version.is_empty() {
            return Err(Error::Invalid("Chrome for Testing's list names a file that is not on Google's storage; nothing is downloaded from it.".into()));
        }
        // The size and the MD5, as the storage gives them with the file.
        let head = self.http.head(&url).send().await.map_err(|e| Error::Net(format!("Google's storage could not be asked ({}).", crate::runtime::root_cause(&e))))?;
        let size = head.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok());
        let md5 = head.headers().get_all("x-goog-hash").iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(',')).find_map(|v| v.trim().strip_prefix("md5=").map(str::to_string));
        let latest = Latest { version, url, size, md5 };
        *self.listed.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), latest.clone()));
        Ok(latest)
    }

    /// Downloads the newest stable build, checks it, unpacks it, and keeps it in place of the one before.
    pub async fn install(&self) -> Result<Kept> {
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            if running.is_some() {
                return Err(Error::Busy("Demido's browser is being downloaded already.".into()));
            }
            *running = Some((stop.clone(), Downloading { version: String::new(), done: 0, total: None }));
        }
        let got = self.install_now(&stop).await;
        *self.running.lock().unwrap_or_else(|e| e.into_inner()) = None;
        match &got {
            Ok(kept) => {
                let _ = self.events.append("browser.downloaded", None, json!({ "version": kept.version, "size": kept.size, "checked": kept.checked }));
            }
            Err(e) => {
                let _ = self.events.append("browser.download.failed", None, json!({ "problem": e.to_string() }));
            }
        }
        got
    }

    async fn install_now(&self, stop: &AtomicBool) -> Result<Kept> {
        let latest = self.latest(false).await?;
        if let Some(kept) = self.kept().filter(|k| k.version == latest.version) {
            return Ok(kept);
        }
        let place = self.dir.join("versions").join(&latest.version);
        tokio::fs::create_dir_all(&place).await?;
        if let Some(free) = crate::system::free_bytes(&place)
            && free < latest.size.unwrap_or(150_000_000) * 4
        {
            return Err(Error::Invalid(format!(
                "There is not enough room for Demido's browser: {} MB are free where Demido keeps its files, and it needs about {} MB.",
                free >> 20,
                (latest.size.unwrap_or(150_000_000) * 3) >> 20
            )));
        }
        let name = latest.url.rsplit('/').next().unwrap_or("chrome-headless-shell.zip").to_string();
        let archive = place.join(&name);
        let file = crate::runtime::RemoteFile { name: name.clone(), url: latest.url.clone(), size: latest.size.unwrap_or(0), sha256: None };
        let told = |done: u64| {
            if let Some((_, d)) = self.running.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                (d.version, d.done, d.total) = (latest.version.clone(), done, latest.size);
            }
            self.events.tell("browser.download", None, json!({ "version": latest.version, "done": done, "total": latest.size }));
        };
        crate::runtime::download_file(&self.http, &file, &archive, stop, told).await?;
        // The MD5 the storage gave with it.
        let checked = match &latest.md5 {
            Some(want) => {
                let from = archive.clone();
                let found = tokio::task::spawn_blocking(move || md5_of(&from)).await.map_err(|e| Error::Other(e.to_string()))??;
                if &found != want {
                    let _ = tokio::fs::remove_file(&archive).await;
                    return Err(Error::Invalid(format!("{name} is not the file Google's storage holds: its MD5 does not match. It was deleted and nothing was kept.")));
                }
                "its size and the MD5 Google's storage gives with it (Chrome for Testing publishes no checksum)".to_string()
            }
            None => "its size only: Google's storage gave no MD5 for it".to_string(),
        };
        let (from, into) = (archive.clone(), place.join("unpacking"));
        let _ = std::fs::remove_dir_all(&into);
        let stop_unpacking = Arc::new(AtomicBool::new(stop.load(Ordering::Relaxed)));
        tokio::task::spawn_blocking(move || crate::runtime::unpack_archive(&from, &into, &stop_unpacking)).await.map_err(|e| Error::Other(e.to_string()))??;
        let unpacked = place.join("chrome-headless-shell");
        let _ = std::fs::remove_dir_all(&unpacked);
        std::fs::rename(place.join("unpacking"), &unpacked)?;
        let _ = tokio::fs::remove_file(&archive).await;
        let program = find_program(&unpacked).ok_or_else(|| Error::Invalid("The download holds no chrome-headless-shell program.".into()))?;
        let kept = Kept { version: latest.version.clone(), path: program.display().to_string(), size: file.size, checked, from: latest.url.clone(), kept_at: now_ms() };
        crate::auth::write_private(&self.note(), &serde_json::to_string_pretty(&kept)?)?;
        // The versions kept before go: one copy is kept.
        if let Ok(entries) = std::fs::read_dir(self.dir.join("versions")) {
            for e in entries.flatten().filter(|e| e.file_name() != latest.version.as_str()) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        Ok(kept)
    }

    /// Stops a download. What was downloaded is kept, so the next try goes on from there.
    pub fn stop(&self) -> bool {
        match self.running.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some((stop, _)) => {
                stop.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Deletes Demido's copy of the browser. Its profile (empty of anything of the person's) goes with it.
    pub fn remove(&self) -> Result<Kept> {
        let kept = self.kept().ok_or_else(|| Error::NotFound("Demido keeps no copy of its browser.".into()))?;
        std::fs::remove_dir_all(&self.dir)?;
        let _ = self.events.append("browser.removed", None, json!({ "version": kept.version }));
        Ok(kept)
    }

    /// Where the browser's profile is: empty of anything of the person's (Z3).
    pub fn profile(&self) -> PathBuf {
        self.dir.join("profile")
    }
}

/// The program in an unpacked headless shell: `chrome-headless-shell-<platform>/chrome-headless-shell`.
fn find_program(unpacked: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "chrome-headless-shell.exe" } else { "chrome-headless-shell" };
    std::fs::read_dir(unpacked).ok()?.flatten().map(|e| e.path().join(name)).find(|p| p.is_file())
}

/// A file's MD5, in base64, as Google's storage writes it.
fn md5_of(path: &Path) -> Result<String> {
    use md5::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = md5::Md5::new();
    let mut piece = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut piece)?;
        if n == 0 {
            break;
        }
        hash.update(&piece[..n]);
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::{Request, State};
    use axum::http::{StatusCode, header};
    use axum::response::{IntoResponse, Response};

    /// A zip with a headless shell in it, as Chrome for Testing lays it out: a folder named for the platform.
    fn shell_zip(platform: &str) -> Vec<u8> {
        use std::io::Write;
        let mut zip = ::zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = ::zip::write::SimpleFileOptions::default().unix_permissions(0o755);
        zip.start_file(format!("chrome-headless-shell-{platform}/chrome-headless-shell"), options).unwrap();
        zip.write_all(b"#!/bin/sh\necho not a real browser\n").unwrap();
        zip.start_file(format!("chrome-headless-shell-{platform}/ABOUT"), options).unwrap();
        zip.write_all(b"Google Chrome\n").unwrap();
        zip.finish().unwrap().into_inner()
    }

    struct Stand {
        zip: Vec<u8>,
        md5: String,
        base: String,
    }

    async fn storage(State(stand): State<Arc<Mutex<Stand>>>, request: Request) -> Response {
        let stand = stand.lock().unwrap();
        let platform = platform().unwrap();
        match request.uri().path() {
            "/list.json" => axum::Json(json!({ "channels": { "Stable": { "version": "158.0.8127.88", "downloads": { "chrome-headless-shell": [
                { "platform": "win64", "url": "https://example.org/wrong.zip" },
                { "platform": platform, "url": format!("{}/158.0.8127.88/{platform}/chrome-headless-shell-{platform}.zip", stand.base) }
            ] } } } }))
            .into_response(),
            p if p.ends_with(".zip") => {
                ([(header::CONTENT_TYPE, "application/zip".to_string()), (header::HeaderName::from_static("x-goog-hash"), format!("crc32c=AAAAAA==,md5={}", stand.md5))], stand.zip.clone())
                    .into_response()
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        }
    }

    #[tokio::test]
    async fn the_browser_is_downloaded_from_the_list_checked_against_the_storages_md5_and_unpacked() {
        let Some(platform) = platform() else { return };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let zip = shell_zip(platform);
        let md5 = {
            use md5::Digest;
            base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(&zip))
        };
        let stand = Arc::new(Mutex::new(Stand { zip, md5, base: base.clone() }));
        let app = Router::new().fallback(storage).with_state(stand.clone());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let home = crate::testing::TempDir::new("demido-browser-copy");
        let events = Arc::new(EventLog::new(Arc::new(crate::db::Db::memory().unwrap())));
        let copy = Copy::new(&home, events).listed_at(&format!("{base}/list.json"));
        let latest = copy.latest(false).await.unwrap();
        assert_eq!((latest.version.as_str(), latest.size.is_some(), latest.md5.is_some()), ("158.0.8127.88", true, true));
        let kept = copy.install().await.unwrap();
        assert!(kept.path.ends_with(&format!("chrome-headless-shell-{platform}/chrome-headless-shell")), "{}", kept.path);
        assert!(kept.checked.starts_with("its size and the MD5"));
        assert_eq!(copy.kept(), Some(kept.clone()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&kept.path).unwrap().permissions().mode() & 0o111, 0o111, "it can be run");
        }
        // A file that is not what the storage says it is: deleted, and nothing kept.
        copy.remove().unwrap();
        stand.lock().unwrap().md5 = "AAAAAAAAAAAAAAAAAAAAAA==".into();
        let copy = Copy::new(&home, Arc::new(EventLog::new(Arc::new(crate::db::Db::memory().unwrap())))).listed_at(&format!("{base}/list.json"));
        let said = copy.install().await.unwrap_err().to_string();
        assert!(said.contains("MD5 does not match"), "{said}");
        assert!(copy.kept().is_none());
    }

    #[test]
    fn an_md5_is_written_as_googles_storage_writes_it() {
        let dir = crate::testing::TempDir::new("demido-md5");
        let file = dir.join("hello.txt");
        std::fs::write(&file, b"hello").unwrap();
        // The MD5 of "hello", in base64.
        assert_eq!(md5_of(&file).unwrap(), "XUFAKrxLKna5cZ2REBfFkg==");
    }
}
