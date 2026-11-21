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
    /// `given`, else `DEMIDO_UI_DIR`, else a folder named `ui` beside the program (how a release is laid
    /// out), else `apps/web/dist` under where the program was started (the repository).
    pub fn find(given: Option<PathBuf>) -> Self {
        let has_page = |p: &PathBuf| p.join("index.html").is_file();
        let beside_program = || std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join("ui")));
        let dir = given
            .or_else(|| std::env::var_os("DEMIDO_UI_DIR").map(PathBuf::from))
            .or_else(|| beside_program().filter(has_page))
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
        // Size and time of last change: enough to tell one build of a file from the next.
        let changed = about.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis());
        let mark = format!("\"{:x}-{:x}\"", about.len(), changed);
        if has.is_some_and(|theirs| theirs.split(',').any(|m| m.trim().trim_start_matches("W/") == mark)) {
            return (StatusCode::NOT_MODIFIED, [(header::ETAG, mark), (header::CACHE_CONTROL, keep.to_string())]).into_response();
        }
        match tokio::fs::read(&file).await {
            Ok(bytes) => ([(header::CONTENT_TYPE, kind(&file).to_string()), (header::CACHE_CONTROL, keep.to_string()), (header::ETAG, mark)], bytes).into_response(),
            Err(_) => StatusCode::NOT_FOUND.into_response(),
        }
    }
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
        let again = ui.serve("/", Some(&mark)).await;
        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
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
