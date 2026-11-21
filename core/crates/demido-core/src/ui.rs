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

    pub async fn serve(&self, path: &str) -> Response {
        let Some(dir) = &self.dir else {
            return (StatusCode::OK, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], MISSING).into_response();
        };
        let wanted = Path::new(path.trim_start_matches('/'));
        // Only plain names: no "..", no absolute paths. A request can never leave the folder.
        if wanted.components().any(|c| !matches!(c, Component::Normal(_))) {
            return StatusCode::NOT_FOUND.into_response();
        }
        let file = if wanted.as_os_str().is_empty() { dir.join("index.html") } else { dir.join(wanted) };
        match tokio::fs::read(&file).await {
            Ok(bytes) => ([(header::CONTENT_TYPE, kind(&file)), (header::CACHE_CONTROL, if file.ends_with("index.html") { "no-cache" } else { "public, max-age=3600" })], bytes).into_response(),
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
