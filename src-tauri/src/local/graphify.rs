//! Bundled Graphify: a code knowledge-graph builder run through the portable Python
//! runtime (`python.rs`). Turns a working folder into a queryable graph under
//! `<folder>/graphify-out/` and renders its `graph.html` visualisation.
//!
//! **No server, no port.** Graphify is a plain CLI — every operation is a short-lived
//! `python -m graphify` child whose stdout we capture. Nothing binds a socket. The
//! `graph.html` visualisation normally pulls `vis-network` off a CDN; we cache that one
//! script into app-data once and inline it into the HTML we hand the webview, so the
//! rendered graph makes no network request either.
//!
//! Graphify itself is installed with `pip install graphifyy` (PyPI package name; the
//! importable module + `python -m graphify` entry point are both `graphify`). A marker
//! file records a successful install so status checks stay cheap (no subprocess per poll).

use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter, Manager};

use crate::local::python;

const PYPI_PACKAGE: &str = "graphifyy";
const VIS_NETWORK_URL: &str =
    "https://unpkg.com/vis-network@9.1.6/standalone/umd/vis-network.min.js";

fn base_dir(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("app data dir")
        .join("graphify")
}

/// Marker written after a successful `pip install graphifyy`, so `installed` is a cheap
/// file-existence check rather than spawning Python to probe the import on every poll.
fn install_marker(app: &AppHandle) -> PathBuf {
    base_dir(app).join("installed")
}

fn vis_network_path(app: &AppHandle) -> PathBuf {
    base_dir(app).join("vis-network.min.js")
}

/// Whether the graphify package is installed into the portable runtime.
pub fn installed(app: &AppHandle) -> bool {
    install_marker(app).exists()
}

/// The `graphify-out` directory graphify writes into, under a given working folder.
fn out_dir(folder: &str) -> PathBuf {
    Path::new(folder).join("graphify-out")
}

/// Whether a built graph exists for `folder` (its `graph.html` is present).
pub fn graph_built(folder: &str) -> bool {
    out_dir(folder).join("graph.html").exists()
}

/// Source-code file extensions that count toward staleness. Only files whose changes would alter
/// graphify's structural (code-only) graph belong here — not docs, assets, lockfiles, or data.
/// Deliberately an allowlist: an unknown extension is treated as "not source" so a tool dropping,
/// say, a `.log` or generated `.min.js` next to the code never marks the graph stale. Kept flat +
/// lowercase; `ext_is_source` lowercases the candidate before lookup.
const SOURCE_EXTS: &[&str] = &[
    // systems / compiled
    "rs", "go", "c", "h", "cc", "cpp", "cxx", "hpp", "hh", "cs", "swift", "kt", "kts", "java",
    "scala", "m", "mm", "zig", // scripting / dynamic
    "py", "rb", "php", "lua", "pl", "pm", "sh", "bash", "zsh", "r", // web / js-ts family
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue", "svelte", "astro",
    // other real code
    "dart", "ex", "exs", "erl", "clj", "cljs", "hs", "ml", "mli", "fs", "fsx", "sql", "gd",
];

fn ext_is_source(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| SOURCE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// Well-known generated / vendored directory names, always skipped even when no `.gitignore` lists
/// them. The `ignore` crate honours `.gitignore` but has no built-in notion of these (that is
/// ripgrep's own default, not the library's) — so a repo that simply forgot to ignore its
/// `node_modules` would otherwise get its vendored `.js` stat-walked. Belt-and-suspenders on top of
/// the gitignore walk, not a replacement for it.
const JUNK_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    "out",
    ".venv",
    "venv",
    "__pycache__",
    "graphify-out",
    "vendor",
    ".next",
    ".nuxt",
    ".svelte-kit",
];

/// Whether the on-disk graph is out of date: some **source-code** file under `folder` has a
/// modification time newer than `graph.html` (rewritten on every build, so its mtime = last build).
///
/// "Smart" scoping, so only actual code is weighed — never libraries or tool-generated folders:
///   1. **`.gitignore`-aware walk** (`ignore` crate) → skips whatever the repo already marks as
///      generated/vendored (`node_modules`, `target`, `dist`, `.venv`, …) plus hidden dirs and
///      `.git`, using the project's own `.gitignore`/`.ignore` rather than a guessed denylist.
///   2. **Source-extension allowlist** ([`SOURCE_EXTS`]) → a tracked README, image, or lockfile
///      changing does not count; only files graphify's code-only build would actually re-ingest.
///
/// Returns `false` when no graph exists (nothing to be stale against). Staleness is a best-effort
/// nudge, never a hard gate → any walk/stat error is ignored, and the walk is capped at
/// `MAX_STALE_FILES` source files so a pathological tree can't stall a query.
pub fn graph_stale(folder: &str) -> bool {
    const MAX_STALE_FILES: usize = 50_000;
    let graph = out_dir(folder).join("graph.html");
    let Ok(built_at) = graph.metadata().and_then(|m| m.modified()) else {
        return false; // no graph, or can't read its mtime → not "stale"
    };

    // `ignore::WalkBuilder` honours .gitignore/.ignore, skips hidden entries and .git by default,
    // and reads parent ignores — the same file selection ripgrep uses. `require_git(false)` lets
    // the ignore rules apply even in a folder that is not a git repo (a bare .gitignore still wins).
    let mut seen = 0usize;
    let walker = ignore::WalkBuilder::new(folder)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .require_git(false)
        .filter_entry(|e| {
            // Prune well-known junk dirs regardless of .gitignore. Applies to directories only;
            // files fall through so the extension filter below decides them.
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = e.file_name().to_str() {
                    return !JUNK_DIRS.contains(&name);
                }
            }
            true
        })
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if !ext_is_source(path) {
            continue;
        }
        seen += 1;
        if seen > MAX_STALE_FILES {
            return false; // too big to judge cheaply — don't guess stale
        }
        if let Some(m) = entry.metadata().ok().and_then(|md| md.modified().ok()) {
            if m > built_at {
                return true;
            }
        }
    }
    false
}

/// Per-folder preference file: `{ "<folder path>": <auto-build bool> }`. Lives next to the
/// install marker in app-data (not in the user's folder), so toggling it leaves no trace in the
/// project and survives a `graphify-out` delete. Read backend-side by `send_message` to decide
/// whether to tell the model to build the graph before working — so it cannot live only in the
/// frontend's `prefs.json` like the skills toggle does.
fn prefs_path(app: &AppHandle) -> PathBuf {
    base_dir(app).join("prefs.json")
}

fn read_prefs(app: &AppHandle) -> HashMap<String, bool> {
    std::fs::read_to_string(prefs_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Whether automatic graph building is enabled for `folder`. **Defaults to `true`** — a folder
/// Demido has never seen has no entry, and the feature is on by default for new projects. Only an
/// explicit toggle-off writes `false`.
pub fn auto_build_enabled(app: &AppHandle, folder: &str) -> bool {
    read_prefs(app).get(folder).copied().unwrap_or(true)
}

/// Persist the auto-build preference for `folder`.
pub fn set_auto_build(app: &AppHandle, folder: &str, enabled: bool) -> Result<(), String> {
    std::fs::create_dir_all(base_dir(app)).map_err(|e| e.to_string())?;
    let mut prefs = read_prefs(app);
    prefs.insert(folder.to_string(), enabled);
    let json = serde_json::to_string(&prefs).map_err(|e| e.to_string())?;
    std::fs::write(prefs_path(app), json).map_err(|e| e.to_string())
}

/// Per-folder settled node-position cache: `{ "<folder path>": { "<node id>": {"x":.., "y":..} } }`.
/// Lives in app-data next to the prefs (not in the user's folder), so it survives an app restart —
/// the whole point: without disk persistence the layout is recomputed by a ~1.5s physics stabilization
/// on the first graph open of every session. Positions are passed through as opaque `Value` (the
/// frontend and vis-network agree on the `{id:{x,y}}` shape); this layer only stores and returns them.
fn positions_path(app: &AppHandle) -> PathBuf {
    base_dir(app).join("positions.json")
}

fn read_all_positions(app: &AppHandle) -> HashMap<String, serde_json::Value> {
    std::fs::read_to_string(positions_path(app))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Cached node positions for `folder`, or `None` when none have been captured yet.
pub fn get_positions(app: &AppHandle, folder: &str) -> Option<serde_json::Value> {
    read_all_positions(app).remove(folder)
}

/// Persist the settled node positions for `folder`.
pub fn set_positions(
    app: &AppHandle,
    folder: &str,
    positions: serde_json::Value,
) -> Result<(), String> {
    std::fs::create_dir_all(base_dir(app)).map_err(|e| e.to_string())?;
    let mut all = read_all_positions(app);
    all.insert(folder.to_string(), positions);
    let json = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    std::fs::write(positions_path(app), json).map_err(|e| e.to_string())
}

/// Drop the cached positions for `folder`. Called after a (re)build — the graph's node ids and
/// natural layout may have changed, so a stale cache would misplace nodes.
pub fn clear_positions(app: &AppHandle, folder: &str) -> Result<(), String> {
    let mut all = read_all_positions(app);
    if all.remove(folder).is_none() {
        return Ok(());
    }
    let json = serde_json::to_string(&all).map_err(|e| e.to_string())?;
    std::fs::write(positions_path(app), json).map_err(|e| e.to_string())
}

/// A guidance note appended to the system prompt so Demido navigates via the graph and honours the
/// auto-build preference. Returns `None` when there is nothing to say (feature off for a folder
/// with no graph). Caller only invokes this when agent mode is on and a working folder is set —
/// the graphify tools are unavailable otherwise.
pub fn prompt_note(app: &AppHandle, folder: &str) -> Option<String> {
    note_for(graph_built(folder), auto_build_enabled(app, folder)).map(str::to_string)
}

/// Pure note-selection: which guidance (if any) applies given whether a graph exists and whether
/// auto-build is on. Split out so the branching is unit-testable without a Tauri `AppHandle`.
fn note_for(graph_built: bool, auto_build: bool) -> Option<&'static str> {
    if graph_built {
        Some(
            "A code knowledge graph is available for the working folder. To learn the codebase's \
             structure, relationships, or where something lives, prefer the graphify_query tool \
             (kinds: query / path / explain) over reading files one at a time. After large \
             changes, refresh it by calling graphify_build with update=true.",
        )
    } else if auto_build {
        Some(
            "This working folder has no code knowledge graph yet, and automatic graph building is \
             enabled for it. Before exploring the code, build the graph once with the \
             graphify_build tool so you can navigate it with graphify_query. Exception: if the \
             folder is empty or nearly empty (a brand-new project with no real source yet), do the \
             requested initial work first, then call graphify_build once actual files exist.",
        )
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{graph_stale, note_for};

    #[test]
    fn graph_stale_is_false_when_no_graph_exists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        // No graphify-out/graph.html → nothing to be stale against.
        assert!(!graph_stale(dir.path().to_str().unwrap()));
    }

    #[test]
    fn graph_stale_ignores_junk_dirs_and_non_source_files() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().to_str().unwrap().to_string();
        let out = dir.path().join("graphify-out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("graph.html"), "<html></html>").unwrap();

        // All of these are created *after* graph.html, so they are newer — yet none may mark the
        // graph stale: node_modules is a junk dir (skipped without any .gitignore); README.md and
        // pnpm-lock.yaml are not source extensions; a .gitignore'd build/ dir is honoured.
        std::thread::sleep(std::time::Duration::from_millis(30));
        let nm = dir.path().join("node_modules");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::write(nm.join("dep.js"), "module.exports = {}").unwrap();
        std::fs::write(dir.path().join("README.md"), "# docs").unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "ignored_out/\n").unwrap();
        let ig = dir.path().join("ignored_out");
        std::fs::create_dir_all(&ig).unwrap();
        std::fs::write(ig.join("gen.rs"), "// generated").unwrap();
        assert!(
            !graph_stale(&folder),
            "junk dirs, non-source files, and gitignored dirs must not mark the graph stale"
        );

        // A newer real source file DOES mark the graph stale. Poll to defeat coarse mtime clocks.
        let src = dir.path().join("main.rs");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            std::fs::write(&src, "fn main() { /* edited */ }").unwrap();
            if graph_stale(&folder) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "edited source never registered as stale"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn note_prefers_navigation_when_a_graph_exists() {
        // Graph present → navigation note, regardless of the auto-build toggle.
        for auto in [true, false] {
            let n = note_for(true, auto).expect("graph present should always yield a note");
            assert!(n.contains("graphify_query"));
            assert!(n.contains("prefer"));
        }
    }

    #[test]
    fn note_tells_the_model_to_build_first_when_auto_build_on_and_no_graph() {
        let n = note_for(false, true).expect("auto-build on with no graph should yield a note");
        assert!(n.contains("graphify_build"));
        // The empty-folder judgment must survive — it is the whole point of the new-project case.
        assert!(n.contains("empty or nearly empty"));
    }

    #[test]
    fn note_is_silent_when_auto_build_off_and_no_graph() {
        assert!(note_for(false, false).is_none());
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallProgress {
    stage: String,
}

fn emit_stage(app: &AppHandle, stage: &str) {
    let _ = app.emit(
        "graphify_install_progress",
        InstallProgress {
            stage: stage.into(),
        },
    );
}

#[derive(Clone, Serialize)]
struct BuildProgress {
    line: String,
}

/// Download the `vis-network` UMD bundle once, so the rendered `graph.html` never reaches
/// out to a CDN. A no-op if already cached.
async fn ensure_vis_network(app: &AppHandle, client: &reqwest::Client) -> Result<(), String> {
    let dest = vis_network_path(app);
    if dest.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(base_dir(app)).map_err(|e| e.to_string())?;
    let bytes = client
        .get(VIS_NETWORK_URL)
        .header("User-Agent", "DemidoStudio")
        .send()
        .await
        .map_err(|e| format!("vis-network download failed: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("vis-network stream error: {}", e))?;
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())
}

/// Install (or repair) the portable Python runtime + the graphify package. Progress via
/// `python_install_progress` / `graphify_install_progress`. Heavy the first time (pip
/// resolves tree-sitter wheels for ~30 languages); a no-op once the marker is present.
pub async fn install(app: &AppHandle, client: &reqwest::Client) -> Result<(), String> {
    if !python::python_ready(app) {
        emit_stage(app, "installing portable Python runtime");
        python::ensure_python(app, client).await?;
    }
    // Fetch the visualisation script alongside the package so the first graph render is offline.
    emit_stage(app, "downloading graph renderer");
    ensure_vis_network(app, client).await?;

    if installed(app) {
        emit_stage(app, "ready");
        return Ok(());
    }

    std::fs::create_dir_all(base_dir(app)).map_err(|e| e.to_string())?;
    emit_stage(
        app,
        "installing graphify (this can take a few minutes the first time)",
    );

    let app2 = app.clone();
    // pip resolves + builds wheels synchronously for minutes; keep it off the async pool.
    tokio::task::spawn_blocking(move || pip_install(&app2))
        .await
        .map_err(|e| format!("graphify install task failed: {}", e))??;

    std::fs::write(install_marker(app), PYPI_PACKAGE).map_err(|e| e.to_string())?;
    emit_stage(app, "ready");
    Ok(())
}

fn pip_install(app: &AppHandle) -> Result<(), String> {
    let out = Command::new(python::python_bin(app))
        .args([
            "-m",
            "pip",
            "install",
            "--disable-pip-version-check",
            "--no-warn-script-location",
            "--upgrade",
            PYPI_PACKAGE,
        ])
        .output()
        .map_err(|e| format!("Failed to run pip: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "pip install failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// Remove the graphify install marker (and cached renderer). The Python runtime is generic
/// and left in place; its pip-installed graphify package is left behind for a fast reinstall.
pub fn uninstall(app: &AppHandle) -> Result<(), String> {
    let _ = std::fs::remove_file(install_marker(app));
    let _ = std::fs::remove_file(vis_network_path(app));
    Ok(())
}

/// Build (or, with `update`, refresh) the graph for `folder`. Runs `python -m graphify
/// <folder> [--update]` with the folder as cwd, streaming each output line as a
/// `graphify_build_progress` event. Returns once the child exits; errors carry the tail.
pub async fn build(
    app: &AppHandle,
    client: &reqwest::Client,
    folder: String,
    update: bool,
) -> Result<(), String> {
    install(app, client).await?;
    let app2 = app.clone();
    let folder2 = folder.clone();
    tokio::task::spawn_blocking(move || run_build(&app2, &folder2, update))
        .await
        .map_err(|e| format!("graphify build task failed: {}", e))??;
    // Graph rewritten — node ids / natural layout may differ, so any cached layout is now stale.
    let _ = clear_positions(app, &folder);
    Ok(())
}

fn run_build(app: &AppHandle, folder: &str, update: bool) -> Result<(), String> {
    let mut cmd = Command::new(python::python_bin(app));
    cmd.arg("-m").arg("graphify").arg(folder);
    if update {
        cmd.arg("--update");
    }
    // `--code-only` builds the structural graph from local AST alone, no LLM key. Without it,
    // graphify tries semantic extraction on every doc/paper/image in the folder and hard-errors
    // ("no LLM API key found (N doc/paper/image file(s) need semantic extraction)") on any repo
    // that has a README, docs, or images — i.e. all of them. Demido wires no key into graphify
    // (enrichment is opt-in via env), so the graph is code-only by design; make the build match.
    cmd.arg("--code-only");
    let mut child = cmd
        .current_dir(folder)
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start graphify: {}", e))?;

    // Drain stderr to a buffer so a full pipe can never deadlock the child; surface it on failure.
    let stderr = child.stderr.take();
    let err_handle = stderr.map(|s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let mut reader = BufReader::new(s);
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap_or(0) > 0 {
                buf.push_str(&line);
                line.clear();
            }
            buf
        })
    });

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let _ = app.emit("graphify_build_progress", BuildProgress { line });
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("graphify wait failed: {}", e))?;
    let stderr_tail = err_handle.and_then(|h| h.join().ok()).unwrap_or_default();
    if !status.success() {
        let tail = stderr_tail.trim();
        return Err(format!(
            "graphify build failed{}",
            if tail.is_empty() {
                String::new()
            } else {
                format!(":\n{}", tail)
            }
        ));
    }
    Ok(())
}

/// Run a read query (`query` / `path` / `explain`) against the built graph. `args` are the
/// positional arguments for that subcommand. cwd is `folder` so graphify finds its
/// `graphify-out`. Returns the command's stdout text.
pub async fn query(
    app: &AppHandle,
    folder: String,
    kind: String,
    args: Vec<String>,
) -> Result<String, String> {
    let app = app.clone();
    tokio::task::spawn_blocking(move || run_query(&app, &folder, &kind, &args))
        .await
        .map_err(|e| format!("graphify query task failed: {}", e))?
}

/// Synchronous query for the tool executor, which already runs inside `spawn_blocking`. Same as
/// [`query`] without the extra task hop. cwd is `folder` so graphify finds its `graphify-out`.
pub fn query_blocking(
    app: &AppHandle,
    folder: &str,
    kind: &str,
    args: &[String],
) -> Result<String, String> {
    run_query(app, folder, kind, args)
}

fn run_query(app: &AppHandle, folder: &str, kind: &str, args: &[String]) -> Result<String, String> {
    if !matches!(kind, "query" | "path" | "explain") {
        return Err(format!("unknown graphify query kind: {}", kind));
    }
    let out = Command::new(python::python_bin(app))
        .arg("-m")
        .arg("graphify")
        .arg(kind)
        .args(args)
        .current_dir(folder)
        .env("PYTHONUNBUFFERED", "1")
        .env("GRAPHIFY_NO_TIPS", "1")
        .output()
        .map_err(|e| format!("Failed to run graphify {}: {}", kind, e))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("graphify {} failed: {}", kind, err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A CSS override injected into `graph.html` so its chrome matches Demido Studio's theme. Only
/// the shell (body, sidebar, search, legend, checkboxes, scrollbars) is re-skinned — graphify's
/// per-node colours are community-coded *data* and are left untouched. Appended *after* graphify's
/// own `<style>`, so equal-specificity rules win by cascade order. Values are the exact `oklch()`
/// tokens from `src/index.css`; the webview is Chromium (WebView2/WebKit) so `oklch()` resolves —
/// reusing the tokens verbatim means the graph never drifts from the app if the theme changes.
fn demido_theme_css() -> &'static str {
    "<style>\
:root{--d-bg:oklch(0.14 0 0);--d-fg:oklch(0.91 0 0);--d-primary:oklch(0.77 0.12 152);\
--d-primary-fg:oklch(0.14 0 0);--d-muted-fg:oklch(0.65 0.008 230);--d-accent:oklch(0.27 0 0);\
--d-border:oklch(0.28 0 0);--d-sidebar:oklch(0.16 0 0);}\
body{background:var(--d-bg);color:var(--d-fg);\
font-family:Inter,-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;}\
#graph{background:var(--d-bg);}\
#sidebar{background:var(--d-sidebar);border-left:1px solid var(--d-border);}\
#search-wrap,#search-results,#info-panel,#stats{border-color:var(--d-border);}\
#search{background:var(--d-bg);border:1px solid var(--d-border);color:var(--d-fg);}\
#search:focus{border-color:var(--d-primary);}\
.search-item:hover,.neighbor-link:hover,.legend-item:hover{background:var(--d-accent);}\
#info-panel h3,#legend-wrap h3{color:var(--d-muted-fg);}\
#info-content{color:var(--d-fg);}#info-content .field b{color:var(--d-fg);}\
#stats{color:var(--d-muted-fg);}\
#legend-controls label{color:var(--d-muted-fg);}#legend-controls label:hover{color:var(--d-fg);}\
.legend-count{color:var(--d-muted-fg);}\
.legend-cb,#select-all-cb{border:1.5px solid var(--d-border);background:var(--d-bg);}\
.legend-cb:checked,#select-all-cb:checked,#select-all-cb:indeterminate\
{background:var(--d-primary);border-color:var(--d-primary);}\
.legend-cb:checked::after,#select-all-cb:checked::after{border-color:var(--d-primary-fg);}\
#select-all-cb:indeterminate::after{background:var(--d-primary-fg);}\
::-webkit-scrollbar{width:8px;height:8px;}::-webkit-scrollbar-track{background:transparent;}\
::-webkit-scrollbar-thumb{background-color:var(--d-border);border-radius:4px;}\
::-webkit-scrollbar-thumb:hover{background-color:var(--d-muted-fg);}\
</style></head>"
}

/// Read `folder/graphify-out/graph.html` and inline the cached `vis-network` bundle in place
/// of its CDN `<script src>`, so the returned HTML renders in a webview with no network
/// access. Handed to the frontend as an iframe `srcdoc`.
pub async fn graph_html(
    app: &AppHandle,
    client: &reqwest::Client,
    folder: String,
) -> Result<String, String> {
    ensure_vis_network(app, client).await?;
    let html_path = out_dir(&folder).join("graph.html");
    let html = std::fs::read_to_string(&html_path)
        .map_err(|e| format!("No graph for this folder yet ({}).", e))?;
    let js = std::fs::read_to_string(vis_network_path(app))
        .map_err(|e| format!("graph renderer missing: {}", e))?;

    // Replace the whole (possibly multi-line) vis-network <script src=...></script> tag with
    // an inline copy. `[^>]*` spans newlines (negated class matches them); the tag holds no
    // '>' until its own close, so this consumes exactly that one element. No backreference —
    // Rust's `regex` rejects those.
    //
    // `NoExpand` is load-bearing: the replacement string is the *entire minified bundle*, which
    // contains `$g`, `$1`, `${...}` identifiers. A plain `&str` replacement makes the `regex`
    // crate read those as capture-group references and splice them out (undefined group → ""),
    // silently deleting ~330 chars of JS → `Unexpected token '='` → `vis` undefined → blank graph.
    let re = regex::Regex::new(r"(?s)<script\b[^>]*vis-network[^>]*></script>").unwrap();
    let inline = format!("<script>{}</script>", js);
    let replaced = re.replace(&html, regex::NoExpand(inline.as_str()));

    // Skin the chrome to Demido's theme by inserting our override <style> just before </head>,
    // after graphify's own styles. `replacen(.., 1)` touches only the document head. Plain string
    // replace (no regex) → the CSS's own `:` / braces carry no capture-group meaning.
    let themed = replaced.replacen("</head>", demido_theme_css(), 1);

    // Inject the position-cache hook just before </body>, after graphify's own scripts (so `network`
    // and `nodesDS` — top-level `const`s in classic scripts — are in scope). The leading marker is a
    // placeholder the frontend swaps for a `window.__GRAPHIFY_POS__` script when it has cached
    // positions for this folder; without it, the hook only reports positions upward.
    Ok(themed.replacen(
        "</body>",
        &format!("{}{}</body>", POS_MARKER, position_cache_hook()),
        1,
    ))
}

/// Placeholder inserted before the hook. The frontend replaces it with a
/// `<script>window.__GRAPHIFY_POS__={...}</script>` block on reopen, or with nothing on first view.
pub const POS_MARKER: &str = "<!--GRAPHIFY_POS-->";

/// A classic `<script>` appended after graphify's own scripts. Two jobs:
/// 1. If `window.__GRAPHIFY_POS__` holds cached node positions, place the nodes there and disable
///    physics *before* stabilization runs → the graph paints instantly, no 2s settle animation.
/// 2. Report positions to the host (`postMessage`) so it can persist them for the next open.
///
/// The report fires on more than `stabilizationIterationsDone`: also on `dragEnd` (manual layout
/// refinements survive) and via a timeout fallback (so closing the window *before* stabilization
/// completes still captures a usable layout — otherwise the cache would never populate and every
/// reopen would re-run the full ~1.5s settle).
fn position_cache_hook() -> &'static str {
    "<script>\
(function(){\
  if(typeof network==='undefined'||typeof nodesDS==='undefined')return;\
  var cached=window.__GRAPHIFY_POS__;\
  var sent=false;\
  var report=function(){try{parent.postMessage({__graphify:'positions',positions:network.getPositions()},'*');sent=true;}catch(e){}};\
  if(cached){\
    try{\
      network.setOptions({physics:{enabled:false}});\
      var ups=[];var ids=nodesDS.getIds();\
      for(var i=0;i<ids.length;i++){var p=cached[String(ids[i])];if(p)ups.push({id:ids[i],x:p.x,y:p.y});}\
      nodesDS.update(ups);network.fit();\
    }catch(e){}\
    report();\
  }else{\
    network.once('stabilizationIterationsDone',report);\
    setTimeout(function(){if(!sent)report();},4000);\
  }\
  network.on('dragEnd',report);\
})();\
</script>"
}
