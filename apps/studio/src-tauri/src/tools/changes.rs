//! Which workspace files a run created or changed, so its card can show them (a chart Python
//! drew, a video a command downloaded).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{Value, json};

pub type Snapshot = HashMap<PathBuf, (u64, SystemTime)>;

pub fn snapshot(dir: &Path) -> Snapshot {
    walkdir::WalkDir::new(dir)
        .max_depth(6)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((e.path().to_path_buf(), (m.len(), m.modified().ok()?)))
        })
        .collect()
}

pub fn changed_files(dir: &Path, before: &Snapshot) -> Vec<Value> {
    let mut out: Vec<Value> = snapshot(dir)
        .into_iter()
        .filter(|(p, meta)| before.get(p) != Some(meta))
        .filter_map(|(p, (size, _))| {
            let rel = p.strip_prefix(dir).ok()?.to_string_lossy().replace('\\', "/");
            if rel.starts_with(".demido") {
                return None;
            }
            Some(json!({
                "path": rel,
                "absolute": p.to_string_lossy(),
                "size": size,
                "kind": file_kind(&p),
            }))
        })
        .collect();
    out.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    out.truncate(40);
    out
}

pub fn file_kind(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => "image",
        Some("csv" | "tsv") => "table",
        Some("json" | "md" | "txt" | "py" | "log" | "yaml" | "yml" | "html" | "xml") => "text",
        _ => "other",
    }
}
