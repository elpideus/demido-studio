//! MetaTrader 5 as a second source of 1-minute history: the fallback while Dukascopy refuses or
//! fails, or the first source when the person prefers it. Nothing here touches MetaTrader until the
//! person accepts the notice in the Data window and picks a terminal and an account.
//!
//! The market service does the downloading (`sidecars/market/src/metatrader`) through a small
//! Python bridge on the `MetaTrader5` package, which exists for Windows only. This module finds the
//! terminals and the accounts they logged into ([`detect`]), installs the bridge with uv the first
//! time it is needed (a Windows CPython and the package, in `<data>/tools/metatrader`), and tells
//! the service how to start it: on Windows that Python; on Linux Wine running it in the terminal's
//! own prefix, with Wine's menu builder off so nothing is added to the desktop.

pub mod detect;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{AppError, CmdResult};
use crate::settings::{MarketSource, Settings, SettingsStore};

/// Version of the MetaTrader notice. Bumping it asks everyone again.
pub const CONSENT_VERSION: &str = "2026-10";

/// What `ready.json` must say; bumping it installs the bridge again.
const SETUP_VERSION: u32 = 1;
/// The Python the package is built for (uv's name for it, on every platform).
const PYTHON: &str = "cpython-3.13-windows-x86_64-none";
const PYTHON_VERSION: &str = "3.13";
const PACKAGE: &str = "MetaTrader5";
const BRIDGE: &str = include_str!("../../../../../../sidecars/market/src/metatrader/bridge.py");

/// The person's MetaTrader choices from the Data window; what is left out stays as it was.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Choice {
    /// Accepts (or withdraws) the notice.
    pub consent: Option<bool>,
    pub source: Option<MarketSource>,
    pub fallback: Option<bool>,
    pub dukascopy_fallback: Option<bool>,
    /// A terminal64.exe that [`MetaTrader::detect`] found.
    pub terminal: Option<PathBuf>,
    pub login: Option<u64>,
    pub server: Option<String>,
    pub switch_account: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    /// MetaTrader can be used on this computer.
    pub supported: bool,
    /// Why not, when it cannot.
    pub reason: Option<String>,
    pub terminals: Vec<detect::Terminal>,
    pub setup: SetupState,
    /// The notice version a consent must name.
    pub consent_version: &'static str,
}

/// The bridge's installation.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SetupState {
    pub installed: bool,
    pub installing: bool,
    /// The last line uv printed while installing.
    pub message: Option<String>,
    /// Why the last installation failed; it is not tried again on its own until the person
    /// saves their choices again.
    pub error: Option<String>,
}

/// `ready.json`: written once the bridge is complete.
#[derive(Serialize, Deserialize)]
struct Ready {
    version: u32,
    /// python.exe, relative to the bridge's folder.
    python: PathBuf,
    package: String,
}

pub struct MetaTrader {
    settings: Arc<SettingsStore>,
    /// `<data>/tools/metatrader`: Python, the package and the bridge script.
    dir: PathBuf,
    uv: Option<PathBuf>,
    /// The app's own Python, which uv resolves the package with on Linux (it cannot run the
    /// Windows one there).
    host_python: Option<PathBuf>,
    uv_cache: PathBuf,
    setup: RwLock<SetupState>,
    installing: tokio::sync::Mutex<()>,
}

impl MetaTrader {
    pub fn new(
        settings: Arc<SettingsStore>,
        dir: PathBuf,
        uv: Option<PathBuf>,
        host_python: Option<PathBuf>,
        uv_cache: PathBuf,
    ) -> Self {
        let me = Self {
            settings,
            dir,
            uv,
            host_python,
            uv_cache,
            setup: RwLock::new(SetupState::default()),
            installing: tokio::sync::Mutex::new(()),
        };
        me.setup.write().installed = me.ready().is_some();
        me
    }

    pub fn setup_state(&self) -> SetupState {
        self.setup.read().clone()
    }

    /// The terminals on this computer (reads their files; starts nothing).
    pub fn detect(&self) -> Detection {
        let reason = unsupported(self.uv.is_some());
        Detection {
            supported: reason.is_none(),
            reason,
            terminals: detect::terminals(),
            setup: self.setup_state(),
            consent_version: CONSENT_VERSION,
        }
    }

    /// Saves `choice`. A terminal must be one [`detect`] finds; choosing another terminal forgets
    /// the account unless one is chosen with it.
    pub fn choose(&self, choice: Choice) -> CmdResult<Settings> {
        let terminal = match &choice.terminal {
            Some(path) => Some(
                detect::terminal_at(path)
                    .ok_or_else(|| AppError::msg("That MetaTrader terminal is not on this computer any more"))?,
            ),
            None => None,
        };
        if let Some(server) = &choice.server
            && server.trim().is_empty()
        {
            return Err(AppError::msg("Choose a MetaTrader account"));
        }
        let saved = self.settings.update(|s| {
            if let Some(consent) = choice.consent {
                s.metatrader_consent = consent.then(|| CONSENT_VERSION.to_string());
            }
            if let Some(source) = choice.source {
                s.market_source = source;
            }
            if let Some(fallback) = choice.fallback {
                s.metatrader_fallback = fallback;
            }
            if let Some(fallback) = choice.dukascopy_fallback {
                s.dukascopy_fallback = fallback;
            }
            if let Some(terminal) = terminal {
                if s.metatrader_terminal.as_deref() != Some(terminal.path.as_path()) && choice.login.is_none() {
                    s.metatrader_login = None;
                    s.metatrader_server = None;
                }
                s.metatrader_terminal = Some(terminal.path);
            }
            if let Some(login) = choice.login {
                s.metatrader_login = Some(login).filter(|l| *l > 0);
            }
            if let Some(server) = &choice.server {
                s.metatrader_server = Some(server.trim().to_string());
            }
            if let Some(switch) = choice.switch_account {
                s.metatrader_switch = switch;
            }
        })?;
        // Saving again is the person's way to retry an installation that failed.
        self.setup.write().error = None;
        Ok(saved)
    }

    /// What the market service is configured with (`metatrader.configure`), or null before the
    /// person consented. Writes the bridge script when it changed.
    pub fn config(&self) -> Value {
        config_for(&self.settings.get(), |terminal| self.launch(terminal))
    }

    /// The person consented and chose a terminal, but the bridge is missing, and no installation
    /// is running or failed already.
    pub fn needs_install(&self) -> bool {
        let s = self.settings.get();
        let setup = self.setup_state();
        consented(&s)
            && s.metatrader_terminal.is_some()
            && !setup.installed
            && !setup.installing
            && setup.error.is_none()
            && unsupported(self.uv.is_some()).is_none()
    }

    /// Installs the bridge (nothing when it is installed), reporting uv's lines to `progress`.
    pub async fn install(&self, progress: &(dyn Fn(&SetupState) + Send + Sync)) -> Result<(), String> {
        let _one = self.installing.lock().await;
        if self.ready().is_some() {
            self.update_setup(progress, |s| s.installed = true);
            return Ok(());
        }
        self.update_setup(progress, |s| {
            s.installing = true;
            s.error = None;
            s.message = Some("Installing the MetaTrader bridge".into());
        });
        let result = self.install_now(progress).await;
        self.update_setup(progress, |s| {
            s.installing = false;
            s.installed = result.is_ok();
            s.message = None;
            s.error = result.as_ref().err().cloned();
        });
        result
    }

    fn update_setup(&self, progress: &(dyn Fn(&SetupState) + Send + Sync), f: impl FnOnce(&mut SetupState)) {
        let snapshot = {
            let mut s = self.setup.write();
            f(&mut s);
            s.clone()
        };
        progress(&snapshot);
    }

    async fn install_now(&self, progress: &(dyn Fn(&SetupState) + Send + Sync)) -> Result<(), String> {
        let Some(uv) = self.uv.clone() else {
            return Err(format!("uv is not installed. {}.", crate::setup::how_to_add()));
        };
        let line = |text: &str| self.update_setup(progress, |s| s.message = Some(text.to_string()));
        let python_dir = self.dir.join("python");
        std::fs::create_dir_all(&python_dir).map_err(|e| format!("could not create {}: {e}", python_dir.display()))?;

        line("Downloading Python for the MetaTrader bridge");
        // --no-bin and --no-registry: nothing outside this folder, no python3.13 on the PATH.
        let args: Vec<OsString> = vec![
            "python".into(),
            "install".into(),
            "--install-dir".into(),
            python_dir.clone().into(),
            "--no-bin".into(),
            "--no-registry".into(),
            PYTHON.into(),
        ];
        run(&uv, &args, &self.uv_env(), Duration::from_secs(15 * 60), &line).await?;
        let python = windows_python(&python_dir).ok_or("uv did not install Python")?;

        line("Downloading the MetaTrader5 package");
        let site = self.dir.join("site");
        let staging = self.dir.join("site.new");
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| format!("could not clear {}: {e}", staging.display()))?;
        }
        let mut args: Vec<OsString> = vec![
            "pip".into(),
            "install".into(),
            "--target".into(),
            staging.clone().into(),
            "--python-platform".into(),
            "x86_64-pc-windows-msvc".into(),
            "--python-version".into(),
            PYTHON_VERSION.into(),
            "--only-binary".into(),
            ":all:".into(),
        ];
        let resolver = if cfg!(windows) {
            Some(python.clone())
        } else {
            self.host_python.clone()
        };
        if let Some(resolver) = resolver {
            args.extend(["--python".into(), resolver.into()]);
        }
        args.push(PACKAGE.into());
        run(&uv, &args, &self.uv_env(), Duration::from_secs(15 * 60), &line).await?;
        if site.exists() {
            std::fs::remove_dir_all(&site).map_err(|e| format!("could not replace {}: {e}", site.display()))?;
        }
        std::fs::rename(&staging, &site).map_err(|e| format!("could not move the package into place: {e}"))?;
        let package = package_version(&site).ok_or("uv did not install the MetaTrader5 package")?;

        self.bridge_script()
            .map_err(|e| format!("could not write the bridge: {e:#}"))?;
        let ready = Ready {
            version: SETUP_VERSION,
            python: python.strip_prefix(&self.dir).unwrap_or(&python).to_path_buf(),
            package,
        };
        demido_core::fsx::write_json(&self.dir.join("ready.json"), &ready)
            .map_err(|e| format!("could not finish the installation: {e:#}"))?;
        Ok(())
    }

    fn uv_env(&self) -> Vec<(&'static str, OsString)> {
        vec![
            // An interpreter uv fetches for itself stays here too.
            ("UV_PYTHON_INSTALL_DIR", self.dir.join("python").into()),
            ("UV_CACHE_DIR", self.uv_cache.clone().into()),
            ("UV_LINK_MODE", "copy".into()),
            ("UV_NO_CONFIG", "1".into()),
            ("UV_NO_PROGRESS", "1".into()),
            ("UV_PYTHON_DOWNLOADS", "automatic".into()),
            ("NO_COLOR", "1".into()),
        ]
    }

    /// python.exe when the installation is complete.
    fn ready(&self) -> Option<PathBuf> {
        let ready: Ready = demido_core::fsx::read_json(&self.dir.join("ready.json")).ok()?;
        let python = self.dir.join(&ready.python);
        let complete = ready.version == SETUP_VERSION
            && python.is_file()
            && detect_package(&self.dir.join("site"))
            && !ready.package.is_empty();
        complete.then_some(python)
    }

    /// `bridge.py` as this build ships it.
    fn bridge_script(&self) -> anyhow::Result<PathBuf> {
        let path = self.dir.join("bridge.py");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(BRIDGE) {
            std::fs::create_dir_all(&self.dir)?;
            demido_core::fsx::write_atomic(&path, BRIDGE.as_bytes())?;
        }
        Ok(path)
    }

    /// How the service starts the bridge for `terminal`; None until it is installed.
    fn launch(&self, terminal: &detect::Terminal) -> Option<Value> {
        let python = self.ready()?;
        let bridge = match self.bridge_script() {
            Ok(path) => path,
            Err(e) => {
                tracing::warn!("the MetaTrader bridge could not be written: {e:#}");
                return None;
            }
        };
        let site = self.dir.join("site");
        let Some(prefix) = &terminal.prefix else {
            return Some(json!({
                "program": python,
                "args": ["-I", bridge, "--site", site],
            }));
        };
        let wine = find_wine()?;
        let inside = |p: &Path| detect::wine_path(prefix, p);
        Some(json!({
            "program": wine,
            "args": [inside(&python)?, "-I", inside(&bridge)?, "--site", inside(&site)?],
            "env": wine_env(prefix, std::env::var("WINEDLLOVERRIDES").ok()),
        }))
    }
}

/// The person accepted the current notice.
pub fn consented(s: &Settings) -> bool {
    s.metatrader_consent.as_deref() == Some(CONSENT_VERSION)
}

/// The service's configuration from the settings; `launch` gives the bridge's command.
fn config_for(s: &Settings, launch: impl FnOnce(&detect::Terminal) -> Option<Value>) -> Value {
    if !consented(s) {
        return Value::Null;
    }
    let mode = match s.market_source {
        MarketSource::Metatrader if s.dukascopy_fallback => "preferred",
        MarketSource::Metatrader => "only",
        MarketSource::Dukascopy if s.metatrader_fallback => "fallback",
        MarketSource::Dukascopy => "off",
    };
    let terminal = s.metatrader_terminal.as_deref().and_then(detect::terminal_at);
    let bridge = terminal.as_ref().and_then(launch);
    json!({
        "mode": mode,
        "bridge": bridge,
        "terminal": terminal.as_ref().map(|t| t.windows_path.clone()).unwrap_or_default(),
        "terminalArgs": if terminal.as_ref().is_some_and(|t| t.portable) { vec!["/portable"] } else { vec![] },
        "login": s.metatrader_login.unwrap_or(0),
        "server": s.metatrader_server.clone().unwrap_or_default(),
        "allowSwitch": s.metatrader_switch,
    })
}

/// Why MetaTrader cannot be used on this computer, if it cannot.
fn unsupported(has_uv: bool) -> Option<String> {
    if cfg!(target_os = "macos") {
        return Some("Downloading from MetaTrader works on Windows and Linux.".into());
    }
    if cfg!(not(windows)) && find_wine().is_none() {
        return Some(
            "Wine is not installed. On Linux MetaTrader runs under Wine, and so does Demido's bridge to it.".into(),
        );
    }
    if !has_uv {
        return Some(format!(
            "uv, which installs Demido's bridge to MetaTrader, is missing. {}.",
            crate::setup::how_to_add()
        ));
    }
    None
}

/// The environment Wine runs the bridge in: the terminal's prefix, quiet, and no menu entries or
/// file associations (the person's own overrides kept).
fn wine_env(prefix: &Path, overrides: Option<String>) -> Value {
    let overrides = match overrides.filter(|o| !o.trim().is_empty()) {
        Some(theirs) => format!("{theirs};winemenubuilder.exe=d"),
        None => "winemenubuilder.exe=d".into(),
    };
    json!({
        "WINEPREFIX": prefix,
        "WINEDEBUG": "-all",
        "WINEDLLOVERRIDES": overrides,
    })
}

fn find_wine() -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .flat_map(|dir| [dir.join("wine"), dir.join("wine64")]);
    // WineHQ's packages, when their links are not on the PATH.
    let winehq = ["stable", "devel", "staging"].map(|b| PathBuf::from(format!("/opt/wine-{b}/bin/wine")));
    on_path.chain(winehq).find(|p| p.is_file())
}

/// The newest `cpython-3.13.*-windows-x86_64-none/python.exe` uv installed.
fn windows_python(dir: &Path) -> Option<PathBuf> {
    let patch = |name: &str| -> Option<u32> {
        let rest = name.strip_prefix("cpython-3.13.")?;
        rest.strip_suffix("-windows-x86_64-none")?.parse().ok()
    };
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let python = e.path().join("python.exe");
            Some((patch(&name)?, python)).filter(|(_, p)| p.is_file())
        })
        .max_by_key(|(patch, _)| *patch)
        .map(|(_, python)| python)
}

fn detect_package(site: &Path) -> bool {
    site.join("MetaTrader5").join("__init__.py").is_file()
}

/// The installed package's version, from its `metatrader5-<version>.dist-info` folder.
fn package_version(site: &Path) -> Option<String> {
    if !detect_package(site) {
        return None;
    }
    std::fs::read_dir(site).ok()?.flatten().find_map(|e| {
        let name = e.file_name().to_string_lossy().to_ascii_lowercase();
        name.strip_prefix("metatrader5-")?
            .strip_suffix(".dist-info")
            .map(str::to_string)
    })
}

/// Runs `program` to its end, passing each line it prints to `on_line`; the error carries its last
/// lines.
async fn run(
    program: &Path,
    args: &[OsString],
    envs: &[(&'static str, OsString)],
    timeout: Duration,
    on_line: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .envs(envs.iter().map(|(k, v)| (*k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let name = program.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let mut child = crate::runtime::job::spawn(&mut cmd).map_err(|e| format!("could not start {name}: {e}"))?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let stdout = child
        .stdout
        .take()
        .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>);
    let stderr = child
        .stderr
        .take()
        .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>);
    for reader in [stdout, stderr].into_iter().flatten() {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(reader).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    }
    drop(tx);
    let mut tail: Vec<String> = Vec::new();
    let finished = tokio::time::timeout(timeout, async {
        while let Some(line) = rx.recv().await {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            on_line(line);
            tail.push(line.to_string());
            if tail.len() > 6 {
                tail.remove(0);
            }
        }
        child.wait().await
    })
    .await;
    match finished {
        Err(_) => {
            let _ = child.start_kill();
            Err(format!(
                "{name} did not finish within {} minutes",
                timeout.as_secs() / 60
            ))
        }
        Ok(Err(e)) => Err(format!("{name} failed: {e}")),
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => Err(format!("{name} failed ({status}): {}", tail.join(" | "))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consented_settings() -> Settings {
        Settings {
            metatrader_consent: Some(CONSENT_VERSION.into()),
            ..Settings::default()
        }
    }

    #[test]
    fn nothing_is_configured_without_consent() {
        let mut s = Settings {
            market_source: MarketSource::Metatrader,
            metatrader_login: Some(1),
            metatrader_server: Some("X".into()),
            ..Settings::default()
        };
        assert_eq!(config_for(&s, |_| panic!("no bridge without consent")), Value::Null);
        // A consent to an older notice is no consent.
        s.metatrader_consent = Some("2020-01".into());
        assert_eq!(config_for(&s, |_| None), Value::Null);
    }

    #[test]
    fn the_source_and_the_fallback_pick_the_mode() {
        let mut s = consented_settings();
        assert_eq!(
            config_for(&s, |_| None)["mode"],
            "fallback",
            "the fallback is on once consented"
        );
        s.metatrader_fallback = false;
        assert_eq!(config_for(&s, |_| None)["mode"], "off");
        s.market_source = MarketSource::Metatrader;
        assert_eq!(config_for(&s, |_| None)["mode"], "preferred");
        s.dukascopy_fallback = false;
        assert_eq!(config_for(&s, |_| None)["mode"], "only", "MetaTrader alone");
        s.metatrader_fallback = true;
        assert_eq!(
            config_for(&s, |_| None)["mode"],
            "only",
            "the other fallback does not matter"
        );
        s.dukascopy_fallback = true;
        let config = config_for(&s, |_| None);
        assert_eq!(config["bridge"], Value::Null);
        assert_eq!(config["terminal"], "", "no terminal chosen");
        assert_eq!(config["allowSwitch"], true);
    }

    #[cfg(unix)]
    #[test]
    fn a_chosen_terminal_under_wine_is_named_as_windows_sees_it() {
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("prefix");
        let install = prefix.join("drive_c/Program Files/MetaTrader 5");
        std::fs::create_dir_all(install.join("Config")).unwrap();
        std::fs::create_dir_all(prefix.join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("../drive_c", prefix.join("dosdevices/c:")).unwrap();
        std::fs::write(install.join("terminal64.exe"), b"MZ").unwrap();
        let s = Settings {
            metatrader_terminal: Some(install.join("terminal64.exe")),
            metatrader_login: Some(42),
            metatrader_server: Some("Broker-Demo".into()),
            metatrader_switch: false,
            ..consented_settings()
        };
        let config = config_for(&s, |t| Some(json!({"program": "wine", "for": t.windows_path})));
        assert_eq!(config["terminal"], "C:\\Program Files\\MetaTrader 5\\terminal64.exe");
        assert_eq!(config["bridge"]["for"], config["terminal"]);
        assert_eq!(
            config["terminalArgs"],
            json!(["/portable"]),
            "its data is in its install folder"
        );
        assert_eq!(
            (config["login"].as_u64(), config["server"].as_str()),
            (Some(42), Some("Broker-Demo"))
        );
        assert_eq!(config["allowSwitch"], false);
    }

    #[test]
    fn wine_runs_the_bridge_quietly_in_the_terminals_prefix() {
        let env = wine_env(Path::new("/home/a/.mt5"), None);
        assert_eq!(
            env,
            json!({"WINEPREFIX": "/home/a/.mt5", "WINEDEBUG": "-all", "WINEDLLOVERRIDES": "winemenubuilder.exe=d"})
        );
        let env = wine_env(Path::new("/p"), Some("d3d11=n".into()));
        assert_eq!(env["WINEDLLOVERRIDES"], "d3d11=n;winemenubuilder.exe=d");
    }

    #[test]
    fn the_newest_python_and_the_package_version_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("python");
        for name in [
            "cpython-3.13.9-windows-x86_64-none",
            "cpython-3.13.15-windows-x86_64-none",
            "cpython-3.14.1-windows-x86_64-none",
        ] {
            std::fs::create_dir_all(py.join(name)).unwrap();
            std::fs::write(py.join(name).join("python.exe"), b"MZ").unwrap();
        }
        std::fs::create_dir_all(py.join("cpython-3.13.20-linux-x86_64-gnu")).unwrap();
        assert_eq!(
            windows_python(&py),
            Some(py.join("cpython-3.13.15-windows-x86_64-none/python.exe"))
        );

        let site = dir.path().join("site");
        std::fs::create_dir_all(site.join("metatrader5-5.0.6231.dist-info")).unwrap();
        assert_eq!(package_version(&site), None, "the package itself is missing");
        std::fs::create_dir_all(site.join("MetaTrader5")).unwrap();
        std::fs::write(site.join("MetaTrader5/__init__.py"), b"").unwrap();
        assert_eq!(package_version(&site).as_deref(), Some("5.0.6231"));
    }

    #[test]
    fn the_bridge_is_ready_only_when_complete_and_its_script_follows_the_build() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Arc::new(SettingsStore::load(dir.path().join("settings.json")));
        let tools = dir.path().join("tools");
        let mt = MetaTrader::new(settings, tools.clone(), None, None, dir.path().join("uv"));
        assert!(!mt.setup_state().installed);
        assert_eq!(mt.ready(), None);

        let python = tools.join("python/cpython-3.13.15-windows-x86_64-none/python.exe");
        std::fs::create_dir_all(python.parent().unwrap()).unwrap();
        std::fs::write(&python, b"MZ").unwrap();
        std::fs::create_dir_all(tools.join("site/MetaTrader5")).unwrap();
        std::fs::write(tools.join("site/MetaTrader5/__init__.py"), b"").unwrap();
        let ready = Ready {
            version: SETUP_VERSION,
            python: PathBuf::from("python/cpython-3.13.15-windows-x86_64-none/python.exe"),
            package: "5.0.6231".into(),
        };
        demido_core::fsx::write_json(&tools.join("ready.json"), &ready).unwrap();
        assert_eq!(mt.ready(), Some(python.clone()));
        assert!(
            MetaTrader::new(mt.settings.clone(), tools.clone(), None, None, dir.path().join("uv"))
                .setup_state()
                .installed
        );

        std::fs::write(tools.join("bridge.py"), "old").unwrap();
        let script = mt.bridge_script().unwrap();
        assert_eq!(std::fs::read_to_string(script).unwrap(), BRIDGE);

        let stale = Ready {
            version: SETUP_VERSION + 1,
            ..ready
        };
        demido_core::fsx::write_json(&tools.join("ready.json"), &stale).unwrap();
        assert_eq!(mt.ready(), None, "another setup version installs again");
    }

    #[test]
    fn choices_are_saved_and_another_terminal_forgets_the_account() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Arc::new(SettingsStore::load(dir.path().join("settings.json")));
        let mt = MetaTrader::new(
            settings.clone(),
            dir.path().join("tools"),
            None,
            None,
            dir.path().join("uv"),
        );
        let saved = mt
            .choose(Choice {
                consent: Some(true),
                login: Some(42),
                server: Some(" Broker-Demo ".into()),
                ..Choice::default()
            })
            .unwrap();
        assert_eq!(saved.metatrader_consent.as_deref(), Some(CONSENT_VERSION));
        assert_eq!(
            (saved.metatrader_login, saved.metatrader_server.as_deref()),
            (Some(42), Some("Broker-Demo"))
        );
        assert!(
            mt.choose(Choice {
                terminal: Some(dir.path().join("evil.exe")),
                ..Choice::default()
            })
            .is_err(),
            "only a terminal64.exe that exists"
        );
        let saved = mt
            .choose(Choice {
                consent: Some(false),
                source: Some(MarketSource::Metatrader),
                dukascopy_fallback: Some(false),
                ..Choice::default()
            })
            .unwrap();
        assert_eq!(saved.metatrader_consent, None);
        assert_eq!(saved.market_source, MarketSource::Metatrader);
        assert!(!saved.dukascopy_fallback);
        assert!(saved.metatrader_fallback, "left out, so kept");
        assert_eq!(
            saved.metatrader_login,
            Some(42),
            "the account is kept for when they consent again"
        );
        assert_eq!(mt.config(), Value::Null);
    }
}
