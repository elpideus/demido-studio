//! Serving the web UI from the same address as the API.
//!
//! The files come from a folder on disk. A release carries them in a folder named `ui` beside the program;
//! while developing they are in `apps/web/dist`. Carrying them inside the program itself is not built yet.
//! When no folder is found, the page below says so instead of failing quietly.

use std::path::{Component, Path, PathBuf};

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

#[derive(Clone, Debug, Default)]
pub struct Ui {
    /// The folder holding `index.html`, if there is one.
    pub dir: Option<PathBuf>,
}

impl Ui {
    /// `given`, else `DEMIDO_UI_DIR`, else a folder named `ui` where a release or an installer puts it (beside
    /// the program; `../lib/Demido Studio/ui` from Linux's packages; `../Resources/ui` in macOS's app), else
    /// `apps/web/dist` under where the program was started (the repository).
    pub fn find(given: Option<PathBuf>) -> Self {
        let has_page = |p: &PathBuf| p.join("index.html").is_file();
        let installed = || {
            let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
            [dir.join("ui"), dir.join("../lib/Demido Studio/ui"), dir.join("../Resources/ui")].into_iter().find(has_page)
        };
        let dir = given
            .or_else(|| std::env::var_os("DEMIDO_UI_DIR").map(PathBuf::from))
            .or_else(installed)
            .or_else(|| ["apps/web/dist", "../apps/web/dist"].iter().map(PathBuf::from).find(has_page))
            .filter(has_page);
        Self { dir }
    }

    /// Serves one file. `has` is the mark of the copy the browser already holds (`If-None-Match`), if any.
    ///
    /// Two kinds of file, two ways of not sending them twice:
    ///
    /// - Under `assets/` the name carries a fingerprint of the content (the build puts it there). Such a
    ///   file never changes, so the browser is told to keep it for good and never asks again.
    /// - Everything else (`index.html` above all) keeps its name when it changes. The browser asks each time,
    ///   but with the mark of its copy; when that still fits, the answer is "not changed" and no bytes.
    pub async fn serve(&self, path: &str, has: Option<&str>) -> Response {
        let Some(dir) = &self.dir else {
            return (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], MISSING).into_response();
        };
        let wanted = Path::new(path.trim_start_matches('/'));
        // Only plain names: no "..", no absolute paths. A request can never leave the folder.
        if wanted.components().any(|c| !matches!(c, Component::Normal(_))) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let file = if wanted.as_os_str().is_empty() { dir.join("index.html") } else { dir.join(wanted) };
        let Ok(about) = tokio::fs::metadata(&file).await else { return StatusCode::NOT_FOUND.into_response() };
        if !about.is_file() {
            return StatusCode::NOT_FOUND.into_response();
        }
        let keep = if wanted.starts_with("assets") { "public, max-age=31536000, immutable" } else { "no-cache" };
        let kind = kind(&file);
        let page = kind.starts_with("text/html");
        // Size and time of last change: enough to tell one build of a file from the next. A page's mark also
        // carries its policy: a browser told "not changed" keeps the headers it had, the old policy with them.
        let changed = about.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
        let mark = if page { format!("\"{:x}-{:x}-{:x}\"", about.len(), changed, policy_mark()) } else { format!("\"{:x}-{:x}\"", about.len(), changed) };
        let walls = |res: &mut Response| {
            let headers = res.headers_mut();
            headers.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
            if page {
                headers.insert(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static(POLICY));
                // A link followed from an answer does not tell the site where it was clicked.
                headers.insert(header::REFERRER_POLICY, header::HeaderValue::from_static("no-referrer"));
            }
        };
        let mut res = if has.is_some_and(|theirs| theirs.split(',').any(|m| m.trim().trim_start_matches("W/") == mark)) {
            (StatusCode::NOT_MODIFIED, [(header::ETAG, mark), (header::CACHE_CONTROL, keep.to_string())]).into_response()
        } else {
            match tokio::fs::read(&file).await {
                Ok(bytes) => ([(header::CONTENT_TYPE, kind.to_string()), (header::CACHE_CONTROL, keep.to_string()), (header::ETAG, mark)], bytes).into_response(),
                Err(_) => return StatusCode::NOT_FOUND.into_response(),
            }
        };
        walls(&mut res);
        res
    }
}

/// What the window's own pages may load and run (IDEAS 136): Demido's own files, and nothing from anywhere
/// else. Nothing a model writes is shown as a page with this address (Markdown is drawn as text; a page a
/// model gave runs in `/page-frame`, of no address and with its own policy; the folder's pages and files are
/// in frames of no address that run nothing), so this is a second wall: what got past the first could
/// neither run a script nor send anything out. A page shown in a frame of its own (`srcdoc`) is under this
/// policy too, so a picture it names on the internet is not fetched either; and `frame-src 'self'` keeps a
/// frame from sending itself to another address (measured, DECISIONS Q15). Styles may be written in
/// the page, as React and the editor write them; scripts may not. Small fonts come inside the styles
/// (`data:`), as the build puts them there.
const POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; media-src 'self'; worker-src 'self'; frame-src 'self'; object-src 'none'; base-uri 'self'; form-action 'self'; frame-ancestors 'none'";

/// What a page a model gave may do while it runs (DECISIONS Q15). `sandbox allow-scripts` without
/// `allow-same-origin`: a document of no address, which cannot act on Demido as the person or reach its
/// storage, even if it is opened by itself; no forms, no windows of its own, no moving the window. Its
/// scripts and styles only from the page itself, pictures and sounds only from inside it (`data:`,
/// `blob:`), and every connection shut (`default-src 'none'` covers fetch, sockets, beacons, frames and
/// workers). Only Demido's own window may hold it in a frame.
pub const PAGE_FRAME_POLICY: &str = "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; form-action 'none'; base-uri 'none'; frame-ancestors 'self'";

/// The frame's document: it holds nothing of its own. The window hands it the page's text once it has
/// loaded, and it becomes that page; it takes the text from the window that holds it and from nowhere else,
/// and only once.
const PAGE_FRAME: &str = "<!doctype html>\n<html><head><meta charset=\"utf-8\"><title>A page a model gave</title></head><body><script>\n\"use strict\";\naddEventListener(\"message\", function take(e) {\n  if (e.source !== parent || typeof e.data !== \"string\") return;\n  removeEventListener(\"message\", take);\n  document.open();\n  document.write(e.data);\n  document.close();\n});\n</script></body></html>\n";

/// The frame a page a model gave runs in (`/page-frame`), with its own policy. Open: it holds nothing, and
/// the window that frames it is the one that is signed in.
pub fn page_frame() -> Response {
    let mut res = ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], PAGE_FRAME).into_response();
    let headers = res.headers_mut();
    headers.insert(header::CONTENT_SECURITY_POLICY, header::HeaderValue::from_static(PAGE_FRAME_POLICY));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, header::HeaderValue::from_static("nosniff"));
    headers.insert(header::REFERRER_POLICY, header::HeaderValue::from_static("no-referrer"));
    res
}

/// A short mark of the policy (FNV-1a), for the page's own mark.
fn policy_mark() -> u32 {
    POLICY.bytes().fold(0x811c_9dc5_u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

const MISSING: &str = "Demido's core is running, but the web UI is not here.\n\nA release keeps the UI in a folder named `ui` beside this program. That folder was not found.\nFrom the repository, build it and point the server at it:\n\n    pnpm app:build\n    demido serve --ui-dir apps/web/dist\n\nThe API is at /api/v1 and its description at /api/v1/openapi.json.\n";

fn kind(file: &Path) -> &'static str {
    match file.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn head(res: &Response, name: header::HeaderName) -> String {
        res.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
    }

    #[test]
    fn a_page_a_model_gave_runs_with_no_address_and_no_way_out() {
        let res = page_frame();
        let policy = head(&res, header::CONTENT_SECURITY_POLICY);
        let directives: Vec<&str> = policy.split(';').map(str::trim).collect();
        assert_eq!(directives[0], "sandbox allow-scripts", "scripts, and nothing else the sandbox could allow: {policy}");
        assert!(!policy.contains("allow-same-origin") && !policy.contains("allow-top-navigation") && !policy.contains("allow-popups") && !policy.contains("allow-forms"), "{policy}");
        assert!(directives.contains(&"default-src 'none'") && !policy.contains("connect-src"), "nothing may connect: {policy}");
        for d in &directives {
            assert!(!d.contains("http") && !d.contains('*') && !d.contains("'self'") || d.starts_with("frame-ancestors"), "no address to load from: {d}");
        }
        assert!(directives.contains(&"frame-ancestors 'self'"), "{policy}");
        assert_eq!((head(&res, header::X_CONTENT_TYPE_OPTIONS).as_str(), head(&res, header::REFERRER_POLICY).as_str()), ("nosniff", "no-referrer"));
        assert!(head(&res, header::CONTENT_TYPE).starts_with("text/html"));
    }

    #[tokio::test]
    async fn a_browser_is_never_sent_the_same_file_twice() {
        let dir = std::env::temp_dir().join(format!("demido-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html><title>Demido</title>").unwrap();
        std::fs::write(dir.join("assets/index-abc123.js"), "console.log(1)").unwrap();
        let ui = Ui::find(Some(dir.clone()));

        // The page keeps its name when it changes: the browser asks each time, and is told "not changed".
        let page = ui.serve("/", None).await;
        assert_eq!((page.status(), head(&page, header::CACHE_CONTROL).as_str()), (StatusCode::OK, "no-cache"));
        let mark = head(&page, header::ETAG);
        assert!(mark.starts_with('"'));
        // The page runs no script but Demido's own files, and loads nothing from elsewhere.
        let policy = head(&page, header::CONTENT_SECURITY_POLICY);
        assert!(policy.contains("script-src 'self';") && policy.contains("connect-src 'self';") && policy.contains("frame-ancestors 'none'"), "{policy}");
        assert_eq!((head(&page, header::REFERRER_POLICY).as_str(), head(&page, header::X_CONTENT_TYPE_OPTIONS).as_str()), ("no-referrer", "nosniff"));
        let again = ui.serve("/", Some(&mark)).await;
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(head(&again, header::CONTENT_SECURITY_POLICY), POLICY, "a page told \"not changed\" is told its policy again");
        assert!(mark.contains(&format!("{:x}", policy_mark())), "a new policy is a new mark");
        assert_eq!(axum::body::to_bytes(again.into_body(), 1024).await.unwrap().len(), 0);
        assert_eq!(ui.serve("/", Some(&format!("W/{mark}"))).await.status(), StatusCode::NOT_MODIFIED, "a weak mark is the same mark");
        assert_eq!(ui.serve("/", Some("\"some-other-build\"")).await.status(), StatusCode::OK);

        // A file whose name carries its fingerprint never changes: kept for good, never asked for again.
        let script = ui.serve("/assets/index-abc123.js", None).await;
        assert_eq!(script.status(), StatusCode::OK);
        assert!(head(&script, header::CACHE_CONTROL).contains("immutable"));

        // When the page is rebuilt, its mark changes and the old mark no longer fits.
        std::fs::write(dir.join("index.html"), "<!doctype html><title>Demido, rebuilt and longer</title>").unwrap();
        assert_eq!(ui.serve("/", Some(&mark)).await.status(), StatusCode::OK);

        assert_eq!(ui.serve("/../secret", None).await.status(), StatusCode::NOT_FOUND);
        assert_eq!(ui.serve("/assets", None).await.status(), StatusCode::NOT_FOUND, "a folder is not a file");
        std::fs::remove_dir_all(dir).ok();
    }
}
