//! Workspace file tools.

use std::io::Read;

use serde_json::{Value, json};

use super::changes::file_kind;
use super::{ToolContext, ToolOutput, arg_str, clip, require_str, workspace_path};
use crate::attachments::meaning;

const MAX_READ: usize = 60_000;

pub fn list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Workspace folder (default: its root)"}
        }
    })
}

pub fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Workspace path, e.g. uploads/report.pdf"},
            "pages": {"type": "string", "description": "PDF pages, slides or spreadsheet sheets, e.g. \"3\" or \"10-14\""},
            "start_line": {"type": "integer", "description": "Default 1"},
            "max_lines": {"type": "integer", "description": "Default 200"}
        },
        "required": ["path"]
    })
}

pub fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "A question, or words the passage would contain"},
            "file": {"type": "string", "description": "Only this file (name or path)"},
            "limit": {"type": "integer", "description": "Default 6, max 20"}
        },
        "required": ["query"]
    })
}

pub fn write_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Workspace path"},
            "content": {"type": "string", "description": "The whole text"}
        },
        "required": ["path", "content"]
    })
}

pub fn list(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let dir = workspace_path(&ctx.workspace, arg_str(args, "path").unwrap_or(""))?;
    if !dir.is_dir() {
        return Ok(ToolOutput::ok(
            json!({"files": [], "note": "The workspace is empty."}).to_string(),
            json!({"kind": "files", "files": []}),
        ));
    }
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(&dir)
        .max_depth(4)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
    {
        let Ok(rel) = entry.path().strip_prefix(&ctx.workspace) else {
            continue;
        };
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.starts_with(".demido") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        files.push(json!({"path": rel, "size": size, "kind": file_kind(entry.path())}));
        if files.len() >= 200 {
            break;
        }
    }
    Ok(ToolOutput::ok(
        json!({"files": files}).to_string(),
        json!({"kind": "files", "files": files}),
    ))
}

/// Reads a workspace file as text. Documents (PDF, Word, PowerPoint, spreadsheets, web pages)
/// are read as the text a model gets from them: an attached file's stored text, any other file's
/// read on the spot.
pub async fn read(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let rel = require_str(args, "path")?;
    let path = workspace_path(&ctx.workspace, rel)?;
    let size = std::fs::metadata(&path)
        .map_err(|_| format!("{rel} does not exist in the workspace."))?
        .len();
    let binary = {
        let mut head = Vec::new();
        std::fs::File::open(&path)
            .and_then(|f| f.take(4096).read_to_end(&mut head))
            .map_err(|_| format!("{rel} could not be read."))?;
        head.contains(&0)
    };
    let text = if is_image(&path) && binary {
        return Err(format!(
            "{rel} is an image, not text. An image the user attaches is shown to you in their message when you can see images."
        ));
    } else if binary || is_document(&path) {
        document_text(ctx, rel, &path, size).await?
    } else {
        let bytes = std::fs::read(&path).map_err(|_| format!("{rel} does not exist in the workspace."))?;
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let text = match arg_str(args, "pages") {
        Some(range) => match page_count(&text) {
            0 => {
                return Err(format!(
                    "{rel} has no pages. Use start_line and max_lines to read a part of it."
                ));
            }
            pages => select_pages(&text, range)
                .ok_or_else(|| format!("{rel} has no pages {range}. It has {pages} pages."))?,
        },
        None => text,
    };
    let start = args["start_line"].as_u64().unwrap_or(1).max(1) as usize;
    let max_lines = args["max_lines"].as_u64().unwrap_or(200).clamp(1, 5000) as usize;
    let total_lines = text.lines().count();
    let shown: String = text
        .lines()
        .skip(start - 1)
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n");
    let mut content = clip(&shown, MAX_READ);
    let last = (start - 1 + max_lines).min(total_lines);
    if start > 1 || last < total_lines {
        content.push_str(&format!("\n… lines {start} to {last} of {total_lines}."));
    }
    Ok(ToolOutput::ok(
        content,
        json!({"kind": "file", "path": rel, "lines": total_lines, "size": size}),
    ))
}

/// Formats read through `demido_extract` even though they are text (any binary file is read
/// through it too). Web pages and other text formats are read as they are, so the model sees,
/// and can edit, their source.
fn is_document(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    ext == "rtf"
}

/// A document's text: the text stored when it was attached while the file is still that one
/// (same size), otherwise read from the file now, since a tool may have changed it.
async fn document_text(ctx: &ToolContext, rel: &str, path: &std::path::Path, size: u64) -> Result<String, String> {
    let stored = rel.trim().replace('\\', "/");
    let stored = stored.trim_start_matches("./");
    if let Ok(Some(a)) = ctx.state.db.attachment_at(&ctx.chat_id, stored)
        && size == a.size
        && let Ok(Some(text)) = ctx.state.db.attachment_text(&a.id)
    {
        return Ok(text);
    }
    if size > crate::attachments::MAX_FILE_BYTES {
        return Err(format!(
            "{rel} is larger than 100 MB, too large to read here. Work on it with run_python instead."
        ));
    }
    let path = path.to_path_buf();
    let read = tokio::task::spawn_blocking(move || demido_extract::extract(&path))
        .await
        .map_err(|e| e.to_string())??;
    read.text.filter(|t| !t.trim().is_empty()).ok_or_else(|| {
        read.note
            .unwrap_or_else(|| format!("No text could be read from {rel}."))
    })
}

fn is_image(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "heic" | "heif" | "avif" | "ico"
    )
}

/// Whether a line starts a page, slide or sheet, as `demido_extract` marks them.
fn is_page_marker(line: &str) -> bool {
    let line = line.trim();
    (line.starts_with("--- Page ") || line.starts_with("--- Slide ") || line.starts_with("--- Sheet "))
        && line.ends_with(" ---")
}

fn page_count(text: &str) -> usize {
    text.lines().filter(|l| is_page_marker(l)).count()
}

/// The pages `range` ("3", "10-14") of a text with page markers; `None` when none of them exist.
fn select_pages(text: &str, range: &str) -> Option<String> {
    let (from, to) = match range.split_once('-') {
        Some((a, b)) => (a.trim().parse::<usize>().ok()?, b.trim().parse::<usize>().ok()?),
        None => {
            let n = range.trim().parse::<usize>().ok()?;
            (n, n)
        }
    };
    let (from, to) = (from.min(to).max(1), from.max(to));
    let mut page = 0usize;
    let mut out = Vec::new();
    for line in text.lines() {
        if is_page_marker(line) {
            page += 1;
        }
        if page >= from && page <= to {
            out.push(line);
        }
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

/// Searches the passages of the chat's attached files: by meaning and words together when the
/// search model has indexed them, by words otherwise.
pub async fn search(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let query = require_str(args, "query")?;
    let limit = args["limit"].as_u64().unwrap_or(6).clamp(1, 20) as usize;
    let all = ctx.state.db.chat_attachments(&ctx.chat_id).map_err(|e| e.to_string())?;
    let readable: Vec<_> = all.into_iter().filter(|a| a.tokens.is_some_and(|t| t > 0)).collect();
    if readable.is_empty() {
        return Err("No file with readable text is attached to this chat.".into());
    }
    let files: Vec<_> = match arg_str(args, "file") {
        Some(wanted) => {
            let wanted = wanted.replace('\\', "/").to_lowercase();
            let wanted = wanted.trim_start_matches("./");
            let matching: Vec<_> = readable
                .iter()
                .filter(|a| {
                    a.name.to_lowercase() == wanted || a.file.as_deref().is_some_and(|f| f.to_lowercase() == wanted)
                })
                .cloned()
                .collect();
            if matching.is_empty() {
                let names: Vec<&str> = readable.iter().map(|a| a.name.as_str()).collect();
                return Err(format!(
                    "No attached file is called {wanted}. The attached files are: {}.",
                    names.join(", ")
                ));
            }
            matching
        }
        None => readable,
    };
    let fts = crate::attachments::search::fts_query(query);
    let ids: Vec<String> = files.iter().map(|a| a.id.clone()).collect();
    let vector = ctx.state.embedder.query(&ids, query, &ctx.cancel).await;
    let by_meaning = vector
        .as_ref()
        .and_then(|q| meaning::find(&ctx.state.db, &ids, q, fts.as_deref(), limit, meaning::Matches::Best));
    let hits = match (by_meaning, fts) {
        (Some((hits, _)), _) => hits,
        (None, Some(fts)) => ctx
            .state
            .db
            .search_passages(&ids, &fts, limit)
            .map_err(|e| e.to_string())?,
        (None, None) => {
            return Err("The query has no words to search for. Use the words the passage would contain.".into());
        }
    };
    let path_of = |id: &str| {
        files
            .iter()
            .find(|a| a.id == id)
            .map(|a| a.file.clone().unwrap_or_else(|| a.name.clone()))
            .unwrap_or_default()
    };
    if hits.is_empty() {
        let names: Vec<String> = files.iter().map(|a| a.name.clone()).collect();
        return Ok(ToolOutput::ok(
            format!(
                "Nothing in {} matches “{query}”. Try other words, synonyms or fewer words, or read the file with read_file.",
                names.join(", ")
            ),
            json!({"kind": "passages", "query": query, "hits": []}),
        ));
    }
    let mut content = format!("{} passages for “{query}”, best first:\n", hits.len());
    let mut shown = Vec::new();
    for hit in &hits {
        let file = path_of(&hit.attachment_id);
        match hit.page {
            Some(page) => content.push_str(&format!("\n[{file}, page {page}]\n")),
            None => content.push_str(&format!("\n[{file}]\n")),
        }
        content.push_str(hit.text.trim());
        content.push('\n');
        shown.push(json!({"file": file, "page": hit.page}));
    }
    Ok(ToolOutput::ok(
        content,
        json!({"kind": "passages", "query": query, "hits": shown}),
    ))
}

pub fn write(ctx: &ToolContext, args: &Value) -> Result<ToolOutput, String> {
    let rel = require_str(args, "path")?;
    let content = args["content"]
        .as_str()
        .ok_or("The \"content\" argument is required.")?;
    let path = workspace_path(&ctx.workspace, rel)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| format!("could not write {rel}: {e}"))?;
    Ok(ToolOutput::ok(
        json!({"written": rel, "bytes": content.len()}).to_string(),
        json!({"kind": "file", "path": rel, "absolute": path.to_string_lossy(), "size": content.len(), "written": true}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_chosen_by_their_markers() {
        let text = "--- Page 1 ---\nintro\n--- Page 2 ---\nmiddle\n--- Page 3 ---\nend";
        assert_eq!(select_pages(text, "2").as_deref(), Some("--- Page 2 ---\nmiddle"));
        assert_eq!(
            select_pages(text, "2-3").as_deref(),
            Some("--- Page 2 ---\nmiddle\n--- Page 3 ---\nend")
        );
        assert_eq!(
            select_pages(text, "3-2").as_deref(),
            select_pages(text, "2-3").as_deref()
        );
        assert_eq!(select_pages(text, "9"), None);
        assert_eq!(select_pages(text, "x"), None);
        assert_eq!(page_count(text), 3);
        let sheets = "--- Sheet \"A\" (2 rows × 1 columns) ---\na\n--- Sheet \"B\" (1 rows × 1 columns) ---\nb";
        assert_eq!(
            select_pages(sheets, "2").as_deref(),
            Some("--- Sheet \"B\" (1 rows × 1 columns) ---\nb")
        );
        assert!(is_document(std::path::Path::new("uploads/Notes.RTF")));
        assert!(
            !is_document(std::path::Path::new("uploads/page.html")),
            "web pages are read as source"
        );
        assert!(!is_document(std::path::Path::new("data.csv")));
    }
}
