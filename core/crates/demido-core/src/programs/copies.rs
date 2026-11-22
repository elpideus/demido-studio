//! Demido's own copies of Claude Code and OpenCode: downloaded from their makers when the person asks, kept
//! unchanged in Demido's data folder, run there with a profile of their own (DECISIONS U26–U31).
//!
//! Nothing is installed on the system: no program is put on the `PATH`, no shell is changed, Claude Code's
//! own `claude install` (which sets those up) is never run. What a copy keeps (its settings, history and,
//! for Claude Code, the login it keeps itself) stays in its profile folder, beside it.
//!
//! - **Claude Code** comes from where Anthropic's own installers take it: `downloads.claude.ai/claude-code-
//!   releases`: `latest` names the version, `<version>/manifest.json` lists each platform's file with its
//!   SHA-256 and size, and the file is `<version>/<platform>/claude` (`claude.exe` on Windows). The file is
//!   checked against that SHA-256 before it gets its name, as Anthropic's installers check it.
//! - **OpenCode** comes from where its own installer takes it: `opencode.ai/update/api/latest/cli/npm` names
//!   the version and package, and the program is in the npm package `@opencode/cli-<target>`, checked
//!   against the SHA-512 npm lists for it, and unpacked from `package/bin/opencode`.
//!
//! Each runs with its self-update off (`DISABLE_AUTOUPDATER`, `OPENCODE_DISABLE_AUTOUPDATE`): it would update
//! itself into its usual place in the person's home. Demido updates its copy instead, when asked.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::ToSchema;

use crate::events::EventLog;
use crate::runtime::RemoteFile;
use crate::{Error, Result, now_ms};

/// Where Anthropic publishes Claude Code.
const CLAUDE_CODE_FROM: &str = "https://downloads.claude.ai/claude-code-releases";
/// Where OpenCode says which version is the latest, and what its package is called.
const OPENCODE_LATEST: &str = "https://opencode.ai/update/api/latest/cli/npm";
/// Where npm keeps packages.
const NPM: &str = "https://registry.npmjs.org";

/// A copy Demido keeps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Kept {
    pub provider: String,
    pub version: String,
    /// The program's full path.
    pub path: String,
    /// Bytes on disk.
    pub size: u64,
    /// How it was checked, in words: "SHA-256 from Anthropic's manifest".
    pub checked: String,
    /// Where it was downloaded from.
    pub from: String,
    /// Milliseconds since 1970.
    pub kept_at: i64,
}

/// The newest version a maker offers.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Latest {
    pub provider: String,
    pub version: String,
    /// Bytes to download, when the maker says.
    pub size: Option<u64>,
}

/// A download under way.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Downloading {
    pub version: String,
    pub done: u64,
    pub total: Option<u64>,
}

pub struct Copies {
    /// `<home>/programs`.
    root: PathBuf,
    http: reqwest::Client,
    events: Arc<EventLog>,
    claude_code_from: String,
    opencode_latest: String,
    npm: String,
    /// The downloads under way, by provider, with what stops each.
    running: std::sync::Mutex<HashMap<String, (Arc<AtomicBool>, Downloading)>>,
}

/// Claude Code's name for this computer's platform, as in Anthropic's manifest.
pub(crate) fn claude_code_platform() -> Option<String> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        return None;
    };
    let musl = if os == "linux" && cfg!(target_env = "musl") { "-musl" } else { "" };
    Some(format!("{os}-{arch}{musl}"))
}

/// OpenCode's name for this computer, as in its package names: `linux-x64`, with `-baseline` for a processor
/// without AVX2 and `-musl` on a musl Linux, as its own installer decides.
pub(crate) fn opencode_target() -> Option<String> {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "windows"
    } else {
        return None;
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        return None;
    };
    #[cfg(target_arch = "x86_64")]
    let baseline = if std::arch::is_x86_feature_detected!("avx2") { "" } else { "-baseline" };
    #[cfg(not(target_arch = "x86_64"))]
    let baseline = "";
    let musl = if os == "linux" && cfg!(target_env = "musl") { "-musl" } else { "" };
    Some(format!("{os}-{arch}{baseline}{musl}"))
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

fn named(provider: &str) -> &'static str {
    if provider == "claude-code" { "Claude Code" } else { "OpenCode" }
}

/// SHA-512 of a file, as npm writes it: `sha512-<base64>`.
fn npm_integrity_of(path: &Path) -> Result<String> {
    use base64::Engine;
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = sha2::Sha512::new();
    let mut piece = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut piece)?;
        if n == 0 {
            break;
        }
        hash.update(&piece[..n]);
    }
    Ok(format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode(hash.finalize())))
}

impl Copies {
    pub fn new(home: &Path, events: Arc<EventLog>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!("DemidoStudio/{} (+https://github.com/elpideus/demido-studio)", crate::VERSION))
            .connect_timeout(std::time::Duration::from_secs(20))
            .read_timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("an HTTP client with these settings can always be made");
        Self { root: home.join("programs"), http, events, claude_code_from: CLAUDE_CODE_FROM.into(), opencode_latest: OPENCODE_LATEST.into(), npm: NPM.into(), running: std::sync::Mutex::default() }
    }

    /// The same, downloading from stand-ins at `base` (`base/claude`, `base/opencode-latest`, `base/npm`). For tests.
    #[cfg(test)]
    pub(crate) fn from(mut self, base: &str) -> Self {
        self.claude_code_from = format!("{base}/claude");
        self.opencode_latest = format!("{base}/opencode-latest");
        self.npm = format!("{base}/npm");
        self
    }

    fn dir(&self, provider: &str) -> PathBuf {
        self.root.join(provider)
    }

    fn note(&self, provider: &str) -> PathBuf {
        self.dir(provider).join("kept.json")
    }

    /// The folder a copy keeps what it keeps in for one account: its profile, with its own settings,
    /// history and, for Claude Code, its own login. One for each account (DECISIONS U33). Profiles stay when
    /// the copy is removed, so that a copy downloaded again finds them.
    pub fn profile(&self, provider: &str, account: &str) -> PathBuf {
        self.dir(provider).join("profiles").join(account)
    }

    /// Where a copy kept its one profile before there could be several accounts. Adopted by `Providers`.
    pub(crate) fn old_profile(&self, provider: &str) -> PathBuf {
        self.dir(provider).join("profile")
    }

    /// Deletes an account's profile: its settings, its history and its login.
    pub fn remove_profile(&self, provider: &str, account: &str) -> Result<()> {
        let place = self.profile(provider, account);
        if place.exists() {
            std::fs::remove_dir_all(place)?;
        }
        Ok(())
    }

    /// The copy Demido keeps of a program, if it keeps one and its file is there.
    pub fn kept(&self, provider: &str) -> Option<Kept> {
        let kept: Kept = std::fs::read_to_string(self.note(provider)).ok().and_then(|t| serde_json::from_str(&t).ok())?;
        Path::new(&kept.path).is_file().then_some(kept)
    }

    /// The download under way for a program, if one is.
    pub fn downloading(&self, provider: &str) -> Option<Downloading> {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).get(provider).map(|(_, d)| d.clone())
    }

    /// What a copy is run with for one account, so that it keeps to that account's folder and does not
    /// update itself.
    pub fn env(&self, provider: &str, account: &str) -> Vec<(&'static str, String)> {
        let profile = self.profile(provider, account);
        match provider {
            "claude-code" => {
                let _ = std::fs::create_dir_all(&profile);
                vec![("CLAUDE_CONFIG_DIR", profile.display().to_string()), ("DISABLE_AUTOUPDATER", "1".into())]
            }
            _ => {
                let mut env = vec![("OPENCODE_DISABLE_AUTOUPDATE", "1".to_string())];
                for (name, part) in [("XDG_CONFIG_HOME", "config"), ("XDG_DATA_HOME", "data"), ("XDG_CACHE_HOME", "cache"), ("XDG_STATE_HOME", "state")] {
                    let place = profile.join(part);
                    let _ = std::fs::create_dir_all(&place);
                    env.push((name, place.display().to_string()));
                }
                env
            }
        }
    }

    async fn get_json(&self, url: &str, what: &str) -> Result<Value> {
        let res = self.http.get(url).send().await.map_err(|e| Error::Net(format!("{what} could not be asked: {}", crate::runtime::root_cause(&e))))?;
        if !res.status().is_success() {
            return Err(Error::Net(format!("{what}: the answer was {}.", res.status())));
        }
        res.json().await.map_err(|e| Error::Net(format!("{what} could not be read: {e}")))
    }

    /// The newest version the maker offers, with what is to be downloaded.
    async fn offer(&self, provider: &str) -> Result<(Latest, RemoteFile, String)> {
        match provider {
            "claude-code" => {
                let platform = claude_code_platform().ok_or_else(|| Error::Invalid("Anthropic offers no Claude Code for this kind of computer.".into()))?;
                let version = self
                    .http
                    .get(format!("{}/latest", self.claude_code_from))
                    .send()
                    .await
                    .map_err(|e| Error::Net(format!("Anthropic could not be asked for Claude Code's version: {}", crate::runtime::root_cause(&e))))?
                    .text()
                    .await
                    .map_err(|e| Error::Net(e.to_string()))?
                    .trim()
                    .to_string();
                let looks_right = version.split('.').count() >= 3 && version.split('.').take(3).all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
                if !looks_right {
                    return Err(Error::Net("Anthropic's download service did not answer with a version. It may not be offered where this computer is (anthropic.com/supported-countries).".into()));
                }
                let manifest = self.get_json(&format!("{}/{version}/manifest.json", self.claude_code_from), "Anthropic's list of Claude Code's files").await?;
                let entry = &manifest["platforms"][platform.as_str()];
                let checksum = entry["checksum"].as_str().filter(|c| c.len() == 64 && c.chars().all(|ch| ch.is_ascii_hexdigit()));
                let Some(checksum) = checksum else {
                    return Err(Error::Invalid(format!("Anthropic's list of Claude Code's files has none for {platform}.")));
                };
                let binary = entry["binary"].as_str().map(str::to_string).unwrap_or_else(|| exe("claude"));
                let size = entry["size"].as_u64();
                let url = format!("{}/{version}/{platform}/{binary}", self.claude_code_from);
                let file = RemoteFile { name: format!("Claude Code {version}"), url, size: size.unwrap_or(0), sha256: Some(checksum.to_lowercase()) };
                Ok((Latest { provider: provider.into(), version, size }, file, binary))
            }
            _ => {
                let target = opencode_target().ok_or_else(|| Error::Invalid("OpenCode offers no program for this kind of computer.".into()))?;
                let latest = self.get_json(&self.opencode_latest, "OpenCode's latest version").await?;
                let version = latest["version"].as_str().ok_or_else(|| Error::Net("OpenCode did not say its latest version.".into()))?.to_string();
                let package = latest["metadata"]["package"].as_str().or(latest["package"].as_str()).unwrap_or("@opencode/cli");
                let name = format!("{package}-{target}");
                let listed = self.get_json(&format!("{}/{}/{version}", self.npm, name.replacen('/', "%2f", 1)), "OpenCode's package").await?;
                let tarball = listed["dist"]["tarball"].as_str().ok_or_else(|| Error::Net(format!("npm lists no file for {name} {version}.")))?.to_string();
                let integrity = listed["dist"]["integrity"]
                    .as_str()
                    .filter(|i| i.starts_with("sha512-"))
                    .ok_or_else(|| Error::Net(format!("npm lists no SHA-512 for {name} {version}, so it cannot be checked.")))?
                    .to_string();
                let file = RemoteFile { name: format!("OpenCode {version}"), url: tarball, size: 0, sha256: None };
                Ok((Latest { provider: provider.into(), version, size: None }, file, integrity))
            }
        }
    }

    /// The newest version the maker offers.
    pub async fn latest(&self, provider: &str) -> Result<Latest> {
        Ok(self.offer(provider).await?.0)
    }

    /// Downloads the newest version, checks it, and keeps it in place of the one kept before. Told as it goes
    /// (`program.download`); the end is in the log (`program.downloaded`, `program.download.failed`).
    pub async fn download(&self, provider: &str) -> Result<Kept> {
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
            if running.contains_key(provider) {
                return Err(Error::Busy(format!("{} is being downloaded already.", named(provider))));
            }
            running.insert(provider.to_string(), (stop.clone(), Downloading { version: String::new(), done: 0, total: None }));
        }
        let got = self.download_now(provider, &stop).await;
        self.running.lock().unwrap_or_else(|e| e.into_inner()).remove(provider);
        match &got {
            Ok(kept) => {
                let _ = self.events.append("program.downloaded", Some(provider), json!({ "version": kept.version, "size": kept.size, "checked": kept.checked, "from": kept.from }));
            }
            Err(e) => {
                let _ = self.events.append("program.download.failed", Some(provider), json!({ "problem": e.to_string() }));
            }
        }
        got
    }

    async fn download_now(&self, provider: &str, stop: &AtomicBool) -> Result<Kept> {
        let (latest, file, check) = self.offer(provider).await?;
        if let Some(kept) = self.kept(provider).filter(|k| k.version == latest.version) {
            return Ok(kept);
        }
        let place = self.dir(provider).join("versions").join(&latest.version);
        tokio::fs::create_dir_all(&place).await?;
        if let Some(free) = crate::system::free_bytes(&place)
            && free < file.size.max(200_000_000) + 100_000_000
        {
            return Err(Error::Invalid(format!("There is not enough room for {}: {} MB are free where Demido keeps its files.", file.name, free >> 20)));
        }
        let told = |done: u64| {
            if let Some((_, d)) = self.running.lock().unwrap_or_else(|e| e.into_inner()).get_mut(provider) {
                (d.version, d.done, d.total) = (latest.version.clone(), done, latest.size);
            }
            self.events.tell("program.download", Some(provider), json!({ "version": latest.version, "done": done, "total": latest.size }));
        };
        let (program, checked) = match provider {
            "claude-code" => {
                let to = place.join(&check);
                crate::runtime::download_file(&self.http, &file, &to, stop, told).await?;
                (to, "SHA-256 from Anthropic's manifest".to_string())
            }
            _ => {
                let archive = place.join("opencode.tgz");
                crate::runtime::download_file(&self.http, &file, &archive, stop, told).await?;
                let (from, want) = (archive.clone(), check.clone());
                let found = tokio::task::spawn_blocking(move || npm_integrity_of(&from)).await.map_err(|e| Error::Other(e.to_string()))??;
                if found != want {
                    let _ = tokio::fs::remove_file(&archive).await;
                    return Err(Error::Invalid(format!("{} is not the file npm lists: its SHA-512 does not match. It was deleted and nothing was kept.", file.name)));
                }
                let to = place.join(exe("opencode"));
                let (from, into) = (archive.clone(), to.clone());
                tokio::task::spawn_blocking(move || unpack_opencode(&from, &into)).await.map_err(|e| Error::Other(e.to_string()))??;
                let _ = tokio::fs::remove_file(&archive).await;
                (to, "SHA-512 from npm".to_string())
            }
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
        }
        let size = std::fs::metadata(&program)?.len();
        let kept = Kept { provider: provider.into(), version: latest.version.clone(), path: program.display().to_string(), size, checked, from: file.url.clone(), kept_at: now_ms() };
        crate::auth::write_private(&self.note(provider), &serde_json::to_string_pretty(&kept)?)?;
        // The versions kept before go: one copy is kept.
        if let Ok(entries) = std::fs::read_dir(self.dir(provider).join("versions")) {
            for e in entries.flatten().filter(|e| e.file_name() != latest.version.as_str()) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
        Ok(kept)
    }

    /// Stops a download. What was downloaded is kept, so the next try goes on from there.
    pub fn stop(&self, provider: &str) -> bool {
        match self.running.lock().unwrap_or_else(|e| e.into_inner()).get(provider) {
            Some((stop, _)) => {
                stop.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// Deletes Demido's copy of a program. Its profiles (their settings and logins) stay.
    pub fn remove(&self, provider: &str) -> Result<Kept> {
        let Some(kept) = self.kept(provider) else {
            return Err(Error::NotFound(format!("Demido keeps no copy of {}.", named(provider))));
        };
        std::fs::remove_dir_all(self.dir(provider).join("versions"))?;
        std::fs::remove_file(self.note(provider))?;
        let _ = self.events.append("program.removed", Some(provider), json!({ "version": kept.version }));
        Ok(kept)
    }
}

/// Takes the program out of OpenCode's package: `package/bin/opencode`.
fn unpack_opencode(archive: &Path, to: &Path) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let wanted = format!("package/bin/{}", exe("opencode"));
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.to_string_lossy() == wanted {
            let mut out = std::fs::File::create(to)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    Err(Error::Invalid(format!("OpenCode's package has no {wanted}: it is not laid out as its own installer expects.")))
}
