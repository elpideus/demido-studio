//! Enclosed local inference engine.
//!
//! Demido downloads a prebuilt `llama-server` (llama.cpp) into app-data (this is the
//! runtime manager), then spawns it as a child process bound to `127.0.0.1` on a random
//! free port, gated by a per-session API key. Other local apps can see the port but can't
//! use it without the key, and the child is killed on app exit (see `RunEvent::Exit` in
//! `lib.rs`). It speaks the OpenAI-compatible API, so the rest of Demido routes to it
//! through the normal `openai_compat` provider path — the live base_url is written into
//! the provider row at spawn time.
//!
//! ponytail: one server, one model at a time — swapping model kills+respawns. Add
//! llama-swap / concurrent models only if users actually need them.

use futures_util::StreamExt;
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;
use crate::db::{self, LOCAL_PROVIDER_ID, LOCAL_PROVIDER_KEY_REF};

const RELEASE_API: &str = "https://api.github.com/repos/ggml-org/llama.cpp/releases/latest";

/// A currently-running llama-server child plus how to reach it.
pub struct Running {
    child: Child,
    port: u16,
    key: String,
    model_id: String,
}

#[derive(Default)]
pub struct LocalEngine {
    running: Mutex<Option<Running>>,
}

impl LocalEngine {
    pub fn current_model(&self) -> Option<String> {
        self.running.lock().unwrap().as_ref().map(|r| r.model_id.clone())
    }

    /// Kill the running server, if any. Called on model swap and app exit.
    pub fn stop(&self) {
        if let Some(mut r) = self.running.lock().unwrap().take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn runtime_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("app data dir").join("runtime")
}

fn binpath_marker(app: &AppHandle) -> PathBuf {
    runtime_dir(app).join("binpath.txt")
}

/// Resolved path to the llama-server binary. The extracted layout varies (some builds nest
/// it under `build/bin/`), so install records the real path in `binpath.txt` and we use
/// that; otherwise fall back to the runtime-dir root.
pub fn runtime_bin(app: &AppHandle) -> PathBuf {
    if let Ok(p) = std::fs::read_to_string(binpath_marker(app)) {
        let pb = PathBuf::from(p.trim());
        if pb.exists() {
            return pb;
        }
    }
    let name = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    runtime_dir(app).join(name)
}

pub fn models_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("app data dir").join("models")
}

pub fn runtime_ready(app: &AppHandle) -> bool {
    runtime_bin(app).exists()
}

fn variant_marker(app: &AppHandle) -> PathBuf {
    runtime_dir(app).join("variant.txt")
}

/// Id of the currently-installed runtime variant, if any.
pub fn installed_variant(app: &AppHandle) -> Option<String> {
    std::fs::read_to_string(variant_marker(app)).ok().map(|s| s.trim().to_string())
}

// ---------------------------------------------------------------------------
// Runtime variants (NVIDIA / AMD / Apple Silicon / CPU)
// ---------------------------------------------------------------------------

/// The four backends surfaced in the UI. Availability is decided at runtime by matching
/// against the actual release assets, so unsupported combos (e.g. CUDA on Linux, Metal on
/// Windows) simply come back `available: false`.
pub const VARIANT_IDS: [&str; 4] = ["cuda", "hip", "metal", "cpu"];

fn variant_label(id: &str) -> &'static str {
    match id {
        "cuda" => "NVIDIA (CUDA)",
        "hip" => "AMD (ROCm / HIP)",
        "metal" => "Apple Silicon (Metal)",
        _ => "CPU",
    }
}

/// All lowercase substrings that must appear in an asset name for it to match this variant
/// on the current OS. Returns None if the variant is meaningless on this OS.
fn variant_tokens(id: &str) -> Option<Vec<&'static str>> {
    let win = cfg!(windows);
    let mac = cfg!(target_os = "macos");
    let arch_arm = std::env::consts::ARCH == "aarch64";
    match id {
        "cpu" => Some(if win {
            vec!["win-cpu-x64", ".zip"]
        } else if mac {
            if arch_arm { vec!["macos-arm64", ".tar.gz"] } else { vec!["macos-x64", ".tar.gz"] }
        } else {
            vec!["ubuntu-x64.tar.gz"]
        }),
        "cuda" => {
            if win {
                Some(vec!["win-cuda", "x64", ".zip"])
            } else if !mac {
                // No Ubuntu CUDA build is published — leave it to match nothing.
                Some(vec!["ubuntu-cuda", "x64", ".tar.gz"])
            } else {
                None
            }
        }
        "hip" => {
            if win {
                Some(vec!["win-hip", "x64", ".zip"])
            } else if !mac {
                Some(vec!["ubuntu-rocm", "x64", ".tar.gz"])
            } else {
                None
            }
        }
        "metal" => {
            if mac && arch_arm {
                Some(vec!["macos-arm64", ".tar.gz"])
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Extra companion assets a variant needs (CUDA needs the cudart DLLs on Windows).
fn variant_extra_tokens(id: &str, main_asset: &str) -> Vec<String> {
    if id == "cuda" && cfg!(windows) {
        // Match the cudart zip to the same CUDA version as the chosen main asset.
        let ver = ["12.4", "13.3"]
            .iter()
            .find(|v| main_asset.contains(*v))
            .copied()
            .unwrap_or("12.4");
        return vec![format!("cudart"), format!("cuda-{}", ver), "win".into(), ".zip".into()];
    }
    Vec::new()
}

fn asset_matches(name: &str, tokens: &[impl AsRef<str>]) -> bool {
    let n = name.to_lowercase();
    tokens.iter().all(|t| n.contains(t.as_ref()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub os: String,
    pub arch: String,
    pub gpus: Vec<String>,
    /// Recommended variant id for this machine.
    pub recommended: String,
}

/// Best-effort hardware probe to recommend a runtime and flag likely-wrong ones.
pub fn detect_hardware() -> Hardware {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let gpus = detect_gpus();
    let g = gpus.join(" ").to_lowercase();
    let recommended = if os == "macos" && arch == "aarch64" {
        "metal"
    } else if g.contains("nvidia") {
        "cuda"
    } else if g.contains("amd") || g.contains("radeon") {
        "hip"
    } else {
        "cpu"
    }
    .to_string();
    Hardware { os, arch, gpus, recommended }
}

fn detect_gpus() -> Vec<String> {
    #[cfg(windows)]
    {
        let out = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
            ])
            .output();
        if let Ok(o) = out {
            return String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
        }
    }
    #[cfg(target_os = "macos")]
    {
        if std::env::consts::ARCH == "aarch64" {
            return vec!["Apple Silicon GPU".to_string()];
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(o) = Command::new("sh").args(["-c", "lspci | grep -i vga"]).output() {
            return String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
        }
    }
    Vec::new()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariantInfo {
    pub id: String,
    pub label: String,
    pub available: bool,
    pub installed: bool,
    pub recommended: bool,
    /// Human note: recommendation or a warning that it likely won't fit this machine.
    pub note: String,
}

async fn fetch_release_assets(client: &reqwest::Client) -> Result<Vec<serde_json::Value>, String> {
    let rel: serde_json::Value = client
        .get(RELEASE_API)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("GitHub API failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Bad GitHub response: {}", e))?;
    rel.get("assets")
        .and_then(|a| a.as_array())
        .cloned()
        .ok_or_else(|| "No release assets".into())
}

fn find_asset<'a>(assets: &'a [serde_json::Value], tokens: &[&str]) -> Option<&'a serde_json::Value> {
    // The CUDA main build tokens ("win-cuda") also match the `cudart-*` companion zip (DLLs
    // only, no binary), which is listed first. Exclude cudart unless we're explicitly
    // querying for it.
    let want_cudart = tokens.iter().any(|t| t.contains("cudart"));
    assets.iter().find(|a| {
        let Some(name) = a.get("name").and_then(|n| n.as_str()) else { return false };
        if !want_cudart && name.to_lowercase().contains("cudart") {
            return false;
        }
        asset_matches(name, tokens)
    })
}

/// Describe the four runtime variants for the current machine: which is recommended,
/// `std::env::consts::OS` spelled the way a user would recognise it.
fn os_label(os: &str) -> &str {
    match os {
        "windows" => "Microsoft Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    }
}

/// which is installed, which are downloadable, and a note for the wrong ones.
pub async fn list_variants(
    app: &AppHandle,
    client: &reqwest::Client,
) -> Result<(Hardware, Vec<VariantInfo>), String> {
    let hw = detect_hardware();
    let assets = fetch_release_assets(client).await.unwrap_or_default();
    let installed = installed_variant(app);

    let infos = VARIANT_IDS
        .iter()
        .map(|&id| {
            let tokens = variant_tokens(id);
            let available = tokens
                .as_ref()
                .map(|t| find_asset(&assets, &t.iter().map(|s| *s).collect::<Vec<_>>()).is_some())
                .unwrap_or(false);
            let recommended = hw.recommended == id;
            let is_installed = installed.as_deref() == Some(id);
            let note = if !available {
                format!("Not available on {} ({}).", os_label(&hw.os), hw.arch)
            } else if recommended {
                "Recommended based on your PC specs.".into()
            } else if id == "cpu" {
                "Always works, but is extremely slow on most systems. Not recommended.".into()
            } else {
                "No matching GPU detected — may fail to load or fall back to CPU".into()
            };
            VariantInfo {
                id: id.to_string(),
                label: variant_label(id).to_string(),
                available,
                installed: is_installed,
                recommended,
                note,
            }
        })
        .collect();
    Ok((hw, infos))
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeProgress {
    downloaded: i64,
    total: i64,
    stage: String,
}

/// Download + install a specific runtime variant, replacing any currently-installed one.
pub async fn install_variant(
    app: &AppHandle,
    client: &reqwest::Client,
    id: &str,
) -> Result<(), String> {
    let tokens = variant_tokens(id)
        .ok_or_else(|| format!("Variant '{}' is not valid on this platform", id))?;
    let assets = fetch_release_assets(client).await?;
    let main = find_asset(&assets, &tokens.iter().map(|s| *s).collect::<Vec<_>>())
        .ok_or_else(|| format!("No '{}' build in the latest release for this platform", id))?;
    let main_name = main.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();

    let dir = runtime_dir(app);
    // Clear any previous runtime so leftovers from another variant don't linger.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Main archive.
    download_and_extract(app, client, main, &dir, "download").await?;

    // Companion archives (cudart for CUDA on Windows).
    let extras = variant_extra_tokens(id, &main_name);
    if !extras.is_empty() {
        if let Some(extra) = find_asset(&assets, &extras.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
            download_and_extract(app, client, extra, &dir, "runtime-libs").await?;
        }
    }

    // Locate the binary at whatever depth it extracted to, and record its real path.
    let found = find_binary(&dir).ok_or_else(|| {
        let files = list_extracted(&dir);
        format!("llama-server not found in extracted runtime. Got: {}", files.join(", "))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&found).map_err(|e| e.to_string())?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&found, perms).map_err(|e| e.to_string())?;
    }
    std::fs::write(binpath_marker(app), found.to_string_lossy().as_ref())
        .map_err(|e| e.to_string())?;
    std::fs::write(variant_marker(app), id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Basenames of everything extracted (for a helpful error if the binary is missing).
fn list_extracted(dir: &std::path::Path) -> Vec<String> {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .take(40)
        .collect()
}

async fn download_and_extract(
    app: &AppHandle,
    client: &reqwest::Client,
    asset: &serde_json::Value,
    dir: &std::path::Path,
    stage: &str,
) -> Result<(), String> {
    let url = asset
        .get("browser_download_url")
        .and_then(|u| u.as_str())
        .ok_or("Asset missing download URL")?;
    let name = asset.get("name").and_then(|n| n.as_str()).unwrap_or("archive");
    let total = asset.get("size").and_then(|s| s.as_i64()).unwrap_or(0);

    let archive = dir.join(name);
    let resp = client
        .get(url)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("Runtime download failed: {}", e))?;
    let mut f = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let mut stream = resp.bytes_stream();
    let mut got: i64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
        f.write_all(&chunk).map_err(|e| e.to_string())?;
        got += chunk.len() as i64;
        let _ = app.emit(
            "local_runtime_progress",
            RuntimeProgress { downloaded: got, total, stage: stage.into() },
        );
    }
    f.flush().map_err(|e| e.to_string())?;
    drop(f);

    let _ = app.emit(
        "local_runtime_progress",
        RuntimeProgress { downloaded: total, total, stage: "extract".into() },
    );
    if name.to_lowercase().ends_with(".zip") {
        extract_zip(&archive, dir)?;
    } else {
        extract_targz(&archive, dir)?;
    }
    let _ = std::fs::remove_file(&archive);
    Ok(())
}

/// Ensure *some* runtime binary exists (used lazily before a spawn). Installs the
/// recommended variant, falling back to CPU.
pub async fn ensure_runtime(
    app: &AppHandle,
    client: &reqwest::Client,
) -> Result<PathBuf, String> {
    let bin = runtime_bin(app);
    if bin.exists() {
        return Ok(bin);
    }
    let hw = detect_hardware();
    if install_variant(app, client, &hw.recommended).await.is_err() {
        install_variant(app, client, "cpu").await?;
    }
    Ok(runtime_bin(app))
}

fn extract_zip(archive: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        // Preserve the archive's layout so the binary keeps its sibling DLLs.
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(&rel);
        if entry.is_dir() {
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

fn find_binary(dir: &std::path::Path) -> Option<PathBuf> {
    let target = if cfg!(windows) { "llama-server.exe" } else { "llama-server" };
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.file_name().to_string_lossy().eq_ignore_ascii_case(target)
        {
            return Some(entry.path().to_path_buf());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Serving
// ---------------------------------------------------------------------------

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    Ok(port)
}

/// Ensure the given local model is being served, spawning or swapping the server as
/// needed. Returns (port, api_key). Also writes the live base_url into the provider row
/// and the key into secrets so the normal openai_compat path works.
pub async fn ensure_model(
    app: &AppHandle,
    state: &AppState,
    model_id: &str,
) -> Result<(u16, String), String> {
    {
        let mut guard = state.local_engine.running.lock().unwrap();
        if let Some(r) = guard.as_mut() {
            let alive = matches!(r.child.try_wait(), Ok(None));
            if alive && r.model_id == model_id {
                return Ok((r.port, r.key.clone()));
            }
        }
    }
    state.local_engine.stop();

    // The load reads gigabytes off disk and blocks the first send; tell the UI so the
    // wait isn't silent. Paired emit: every exit path below reports through `load_model`.
    let _ = app.emit(
        "local_engine_status",
        serde_json::json!({ "model_id": model_id, "loading": true }),
    );
    let result = load_model(app, state, model_id).await;
    let _ = app.emit(
        "local_engine_status",
        serde_json::json!({
            "model_id": model_id,
            "loading": false,
            "error": result.as_ref().err(),
        }),
    );
    result
}

/// Spawn the server for `model_id` and wait until it answers /health. Assumes any previous
/// server is already stopped.
async fn load_model(
    app: &AppHandle,
    state: &AppState,
    model_id: &str,
) -> Result<(u16, String), String> {
    let bin = ensure_runtime(app, &state.http_client).await?;

    let model = {
        let conn = state.conn.lock().unwrap();
        db::local_models::find_by_id(&conn, model_id).map_err(|e| e.to_string())?
    }
    .ok_or_else(|| format!("Local model '{}' not found", model_id))?;

    let port = free_port()?;
    let key = uuid::Uuid::new_v4().simple().to_string();

    let mut cmd = Command::new(&bin);
    cmd.arg("-m").arg(&model.file_path);
    // Vision projector: enables multimodal input when the model ships an mmproj.
    if let Some(mp) = &model.mmproj_path {
        if std::path::Path::new(mp).exists() {
            cmd.arg("--mmproj").arg(mp);
        }
    }
    let mut child = cmd
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .arg("--api-key")
        .arg(&key)
        .arg("--jinja")
        .stdout(Stdio::null())
        // llama-server reports on stderr, and the report is the only place some of it exists:
        // the per-request `Chat format:` line (which template llama.cpp matched, and so which
        // tool-call parser is live) is never exposed over HTTP — `/props` describes the model,
        // not the request. Dropping this stream meant a model whose tool calls stopped being
        // parsed failed silently, looking like the model had merely finished its turn.
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start llama-server: {}", e))?;

    if let Some(stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("[llama-server] {}", line);
            }
        });
    }

    {
        let mut guard = state.local_engine.running.lock().unwrap();
        *guard = Some(Running { child, port, key: key.clone(), model_id: model_id.to_string() });
    }

    let health = format!("http://127.0.0.1:{}/health", port);
    let mut ready = false;
    for _ in 0..240 {
        {
            let mut guard = state.local_engine.running.lock().unwrap();
            if let Some(r) = guard.as_mut() {
                if !matches!(r.child.try_wait(), Ok(None)) {
                    *guard = None;
                    return Err("llama-server exited during startup (check the model file)".into());
                }
            } else {
                return Err("Engine was stopped during startup".into());
            }
        }
        if let Ok(resp) = state.http_client.get(&health).send().await {
            if resp.status().is_success() {
                ready = true;
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    if !ready {
        state.local_engine.stop();
        return Err("llama-server did not become ready in time".into());
    }

    {
        let conn = state.conn.lock().unwrap();
        conn.execute(
            "UPDATE providers SET base_url = ?1 WHERE id = ?2",
            rusqlite::params![format!("http://127.0.0.1:{}/v1", port), LOCAL_PROVIDER_ID],
        )
        .map_err(|e| e.to_string())?;
    }
    state.secrets.set(LOCAL_PROVIDER_KEY_REF, &key).map_err(|e| e.to_string())?;

    // The model is loaded, so llama.cpp can now tell us what it actually supports —
    // `/props` only ever describes the loaded model, so this is our one chance. Cache it;
    // capability lookups for un-loaded models fall back to the models.dev registry.
    if let Some(caps) = crate::caps::probe_llama_server(&state.http_client, port, &key).await {
        let conn = state.conn.lock().unwrap();
        if let Err(e) = db::local_models::set_caps(&conn, model_id, &caps) {
            eprintln!("[engine] failed to cache caps for {}: {}", model_id, e);
        }
    }

    Ok((port, key))
}
