//! Portable Python runtime, downloaded on demand (same shape as `engine.rs`'s llama-server
//! runtime): a prebuilt, self-contained CPython from `astral-sh/python-build-standalone`
//! (the distribution `uv`/`rye` use) extracted into app-data. No system Python required.
//!
//! This is intentionally generic — SearXNG (`searxng.rs`) is the first thing installed
//! through it, but any future Python-based tool can reuse `ensure_python`/`python_bin`.

use futures_util::StreamExt;
use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

const RELEASE_API: &str =
    "https://api.github.com/repos/astral-sh/python-build-standalone/releases/latest";

fn runtime_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("python")
}

/// The `install_only` build extracts a fixed `python/` top-level directory.
pub fn install_dir(app: &AppHandle) -> PathBuf {
    runtime_dir(app).join("python")
}

/// `Lib/site-packages` inside the portable install (Windows layout only — the only platform
/// that currently needs shim modules written directly into site-packages).
pub fn windows_site_packages(app: &AppHandle) -> PathBuf {
    install_dir(app).join("Lib").join("site-packages")
}

pub fn python_bin(app: &AppHandle) -> PathBuf {
    let base = install_dir(app);
    if cfg!(windows) {
        base.join("python.exe")
    } else {
        base.join("bin").join("python3")
    }
}

pub fn python_ready(app: &AppHandle) -> bool {
    python_bin(app).exists()
}

/// Version string of the installed runtime (e.g. "3.12.7"), asked of the binary itself
/// rather than derived from the asset name, so it stays true after any manual swap.
pub fn python_version(app: &AppHandle) -> Option<String> {
    if !python_ready(app) {
        return None;
    }
    let out = std::process::Command::new(python_bin(app))
        .args(["-c", "import platform; print(platform.python_version())"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

/// Remove the whole portable runtime. Anything installed into it (SearXNG's deps) dies
/// with it — callers that care must stop dependent processes first.
pub fn uninstall_python(app: &AppHandle) -> Result<(), String> {
    let dir = runtime_dir(app);
    if !dir.exists() {
        return Ok(());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("Failed to remove Python runtime: {}", e))
}

// Pinned major.minor: needs to be new enough for stdlib `tomllib` (3.11+), which SearXNG's
// bot-detection config loader imports unconditionally. The release ships several CPython
// versions side by side, so the OS/arch suffix alone isn't enough to pick one.
const PYTHON_MAJOR_MINOR: &str = "3.12";

/// The exact `-install_only.tar.gz` suffix for this OS/arch, or None if unsupported.
fn asset_suffix() -> Option<&'static str> {
    let win = cfg!(windows);
    let mac = cfg!(target_os = "macos");
    let arm = std::env::consts::ARCH == "aarch64";
    if win {
        Some("-x86_64-pc-windows-msvc-install_only.tar.gz")
    } else if mac {
        Some(if arm {
            "-aarch64-apple-darwin-install_only.tar.gz"
        } else {
            "-x86_64-apple-darwin-install_only.tar.gz"
        })
    } else {
        Some(if arm {
            "-aarch64-unknown-linux-gnu-install_only.tar.gz"
        } else {
            "-x86_64-unknown-linux-gnu-install_only.tar.gz"
        })
    }
}

async fn find_asset(client: &reqwest::Client) -> Result<(String, String, i64), String> {
    let suffix = asset_suffix().ok_or("No portable Python build for this platform")?;
    let prefix = format!("cpython-{}.", PYTHON_MAJOR_MINOR);
    let rel: serde_json::Value = client
        .get(RELEASE_API)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("GitHub API failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Bad GitHub response: {}", e))?;
    let assets = rel
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or("No release assets")?;
    let asset = assets
        .iter()
        .find(|a| {
            a.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(suffix))
                .unwrap_or(false)
        })
        .ok_or_else(|| format!("No '{}*{}' build in the latest release", prefix, suffix))?;
    let name = asset
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("python.tar.gz")
        .to_string();
    let url = asset
        .get("browser_download_url")
        .and_then(|u| u.as_str())
        .ok_or("Asset missing download URL")?
        .to_string();
    let size = asset.get("size").and_then(|s| s.as_i64()).unwrap_or(0);
    Ok((name, url, size))
}

/// The version that *would* be installed, from the asset name (`cpython-3.12.7+2024…`).
/// Used by the setup wizard, which must name a version before anything is on disk.
pub async fn available_version(client: &reqwest::Client) -> Result<String, String> {
    let (name, _, _) = find_asset(client).await?;
    name.strip_prefix("cpython-")
        .and_then(|rest| rest.split(['+', '-']).next())
        .filter(|v| !v.is_empty())
        .map(String::from)
        .ok_or_else(|| format!("Could not read a version out of asset name '{}'", name))
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallProgress {
    downloaded: i64,
    total: i64,
    stage: String,
}

/// Download + extract the portable Python runtime, replacing any previous install.
pub async fn install_python(app: &AppHandle, client: &reqwest::Client) -> Result<(), String> {
    let (name, url, total) = find_asset(client).await?;

    let dir = runtime_dir(app);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let archive = dir.join(&name);
    let resp = client
        .get(&url)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("Python download failed: {}", e))?;
    let mut f = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    let mut got: i64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        f.write_all(&chunk).map_err(|e| e.to_string())?;
        got += chunk.len() as i64;
        let _ = app.emit(
            "python_install_progress",
            InstallProgress {
                downloaded: got,
                total,
                stage: "download".into(),
            },
        );
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);

    let _ = app.emit(
        "python_install_progress",
        InstallProgress {
            downloaded: total,
            total,
            stage: "extract".into(),
        },
    );
    extract_targz(&archive, &dir)?;
    let _ = std::fs::remove_file(&archive);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = python_bin(app);
        if let Ok(meta) = std::fs::metadata(&bin) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&bin, perms);
        }
    }

    if !python_bin(app).exists() {
        return Err("Extraction succeeded but python binary not found at expected path".into());
    }

    let _ = app.emit(
        "python_install_progress",
        InstallProgress {
            downloaded: total,
            total,
            stage: "ensurepip".into(),
        },
    );
    let _ = std::process::Command::new(python_bin(app))
        .args(["-m", "ensurepip", "--upgrade"])
        .output();

    Ok(())
}

pub async fn ensure_python(app: &AppHandle, client: &reqwest::Client) -> Result<PathBuf, String> {
    if python_ready(app) {
        return Ok(python_bin(app));
    }
    install_python(app, client).await?;
    Ok(python_bin(app))
}

fn extract_targz(archive: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let rel = entry.path().map_err(|e| e.to_string())?.into_owned();
        let out = dest.join(&rel);
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
    }
    Ok(())
}
