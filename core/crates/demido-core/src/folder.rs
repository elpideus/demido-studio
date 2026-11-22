//! A session's folder, as the person sees it in the window (brief 4.24): what is in it, and each file, to
//! look at and, for text, to save (DECISIONS F1 to F6).
//!
//! Only inside the session's folder: every path is checked the way the tools check theirs
//! (`tools::files::inside`), so `..`, an absolute path elsewhere or a link that leads out is refused before
//! anything is opened.
//!
//! A file is given back as what it is, with one exception made for safety: whatever a browser would run as
//! a page (HTML, XML, JavaScript) is sent as plain text, and every file goes with `nosniff` and a `sandbox`
//! policy. An SVG is sent as a picture: in an `<img>` it runs nothing, and opened by itself the policy holds
//! it. The files are served from the same address as Demido's own API, which the person's sign-in opens; a
//! page in their folder that ran there could act on Demido as them.

use std::path::{Path, PathBuf};

use serde::Serialize;
use utoipa::ToSchema;

use crate::{Error, Result};

/// A folder is listed up to this many entries.
const MOST_LISTED: usize = 2_000;

/// The largest text file that has a version and can be saved from the window: 8 MB.
pub const MOST_SAVED: u64 = 8_000_000;

/// What the window shows a file as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Markdown,
    Html,
    Text,
    Image,
    Svg,
    Pdf,
    Video,
    Audio,
    Other,
}

impl Kind {
    /// Markdown, a page or other text: what can be edited.
    pub fn is_text(self) -> bool {
        matches!(self, Kind::Markdown | Kind::Html | Kind::Text)
    }

    pub fn word(self) -> &'static str {
        match self {
            Kind::Markdown => "markdown",
            Kind::Html => "html",
            Kind::Text => "text",
            Kind::Image => "image",
            Kind::Svg => "svg",
            Kind::Pdf => "pdf",
            Kind::Video => "video",
            Kind::Audio => "audio",
            Kind::Other => "other",
        }
    }

    /// The type it is sent with. Nothing a browser would run as a page.
    pub fn content_type(self, ext: &str) -> &'static str {
        match self {
            Kind::Markdown | Kind::Html | Kind::Text => "text/plain; charset=utf-8",
            Kind::Svg => "image/svg+xml",
            Kind::Pdf => "application/pdf",
            Kind::Image => match ext {
                "png" => "image/png",
                "jpg" | "jpeg" => "image/jpeg",
                "gif" => "image/gif",
                "webp" => "image/webp",
                "avif" => "image/avif",
                "bmp" => "image/bmp",
                "ico" => "image/x-icon",
                _ => "application/octet-stream",
            },
            Kind::Video => match ext {
                "mp4" | "m4v" => "video/mp4",
                "webm" => "video/webm",
                "mov" => "video/quicktime",
                "ogv" => "video/ogg",
                _ => "application/octet-stream",
            },
            Kind::Audio => match ext {
                "mp3" => "audio/mpeg",
                "wav" => "audio/wav",
                "ogg" | "oga" => "audio/ogg",
                "opus" => "audio/opus",
                "flac" => "audio/flac",
                "m4a" | "aac" => "audio/mp4",
                _ => "application/octet-stream",
            },
            Kind::Other => "application/octet-stream",
        }
    }
}

/// What a file is shown as, by its name alone. `None`: its name does not say.
pub fn kind_by_name(path: &Path) -> Option<Kind> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    Some(match ext.as_str() {
        "md" | "markdown" | "mdx" => Kind::Markdown,
        "html" | "htm" | "xhtml" => Kind::Html,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "ico" => Kind::Image,
        "svg" => Kind::Svg,
        "pdf" => Kind::Pdf,
        "mp4" | "m4v" | "webm" | "mov" | "ogv" => Kind::Video,
        "mp3" | "wav" | "ogg" | "oga" | "opus" | "flac" | "m4a" | "aac" => Kind::Audio,
        _ => return None,
    })
}

/// What a file is shown as: by its name, and for a name that says nothing, by whether its start is text.
pub fn kind_of(path: &Path) -> Kind {
    match kind_by_name(path) {
        Some(kind) => kind,
        None => {
            // Text is valid UTF-8 without a zero byte, in its first 8 kB.
            let mut start = vec![0u8; 8192];
            let read = std::fs::File::open(path).and_then(|mut f| std::io::Read::read(&mut f, &mut start)).unwrap_or(0);
            start.truncate(read);
            let text = !start.contains(&0) && (std::str::from_utf8(&start).is_ok() || std::str::from_utf8(&start[..start.len().saturating_sub(3)]).is_ok());
            if text { Kind::Text } else { Kind::Other }
        }
    }
}

/// One thing in a folder.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Entry {
    pub name: String,
    /// From the session's folder, with `/` between the parts.
    pub path: String,
    /// `folder` or `file`.
    pub kind: String,
    /// For a file: what the window shows it as (`markdown`, `html`, `text`, `image`, `svg`, `pdf`, `video`,
    /// `audio`, `other`).
    pub shown_as: Option<String>,
    /// Bytes, for a file.
    pub size: u64,
    /// When it last changed, in milliseconds since 1970.
    pub modified: Option<i64>,
}

/// What is in one folder of a session's folder.
#[derive(Clone, Debug, PartialEq, Serialize, ToSchema)]
pub struct Listing {
    /// The session's folder, its full path.
    pub folder: String,
    /// The folder listed, from the session's folder (`` for itself).
    pub path: String,
    /// Folders first, then files, each by name.
    pub entries: Vec<Entry>,
    /// How many more there are than were listed.
    pub more: usize,
}

/// A path inside the folder, as given in a request: `` is the folder itself.
fn resolved(folder: &Path, path: &str) -> Result<PathBuf> {
    let path = path.trim().trim_start_matches('/');
    if path.is_empty() {
        return folder.canonicalize().map_err(|e| Error::NotFound(format!("The session's folder cannot be opened: {e}")));
    }
    crate::tools::files::inside(folder, path).map_err(|why| Error::Invalid(format!("{why}.")))
}

/// When a file last changed, in milliseconds since 1970.
fn modified_of(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64)
}

/// A path as the window is given it: from the folder, with `/`.
fn shown(folder: &Path, path: &Path) -> String {
    let within = path.strip_prefix(folder).unwrap_or(path);
    within.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// What is in a folder of the session's folder.
pub fn list(folder: &Path, path: &str) -> Result<Listing> {
    let base = folder.canonicalize().map_err(|e| Error::NotFound(format!("The session's folder cannot be opened: {e}")))?;
    let dir = resolved(folder, path)?;
    if !dir.is_dir() {
        return Err(Error::NotFound(format!("\"{path}\" is not a folder in the session's folder.")));
    }
    let mut entries: Vec<Entry> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            // A link is shown for what it leads to, and only if that is inside the folder.
            let real = p.canonicalize().ok()?;
            if !real.starts_with(&base) {
                return None;
            }
            let meta = std::fs::metadata(&real).ok()?;
            let modified = modified_of(&meta);
            Some(Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: shown(&base, &p),
                kind: if meta.is_dir() { "folder" } else { "file" }.into(),
                shown_as: (!meta.is_dir()).then(|| kind_of(&real).word().to_string()),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified,
            })
        })
        .collect();
    entries.sort_by(|a, b| (a.kind != "folder").cmp(&(b.kind != "folder")).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    let more = entries.len().saturating_sub(MOST_LISTED);
    entries.truncate(MOST_LISTED);
    Ok(Listing { folder: base.display().to_string(), path: shown(&base, &dir), entries, more })
}

/// A file of the session's folder, opened to be sent.
pub struct Opened {
    pub path: PathBuf,
    pub size: u64,
    pub kind: Kind,
    pub content_type: &'static str,
    pub modified: Option<std::time::SystemTime>,
}

/// Opens a file of the session's folder to be looked at.
pub fn open(folder: &Path, path: &str) -> Result<Opened> {
    let file = resolved(folder, path)?;
    let meta = std::fs::metadata(&file).map_err(|_| Error::NotFound(format!("There is no file \"{path}\" in the session's folder.")))?;
    if meta.is_dir() {
        return Err(Error::Invalid(format!("\"{path}\" is a folder.")));
    }
    let kind = kind_of(&file);
    let ext = file.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    Ok(Opened { size: meta.len(), content_type: kind.content_type(&ext), kind, modified: meta.modified().ok(), path: file })
}

/// A `Range` that asks for bytes the file does not have, or that cannot be read.
#[derive(Debug, PartialEq, Eq)]
pub struct NotInFile;

/// The part of a file a `Range` header asks for (`bytes=start-end`, one range), as start and length.
/// `None`: the whole file.
pub fn range(header: Option<&str>, size: u64) -> std::result::Result<Option<(u64, u64)>, NotInFile> {
    let Some(spec) = header.and_then(|h| h.trim().strip_prefix("bytes=")) else { return Ok(None) };
    if spec.contains(',') {
        return Ok(None);
    }
    let number = |s: &str| s.parse::<u64>().map_err(|_| NotInFile);
    let (from, to) = spec.split_once('-').ok_or(NotInFile)?;
    let (start, end) = match (from.trim(), to.trim()) {
        ("", last) => (size.saturating_sub(number(last)?), size.saturating_sub(1)),
        (from, "") => (number(from)?, size.saturating_sub(1)),
        (from, to) => (number(from)?, number(to)?.min(size.saturating_sub(1))),
    };
    if size == 0 || start > end || start >= size {
        return Err(NotInFile);
    }
    Ok(Some((start, end - start + 1)))
}

/// A mark for what a file holds, which changes whenever it does: the start of its SHA-256, as an HTTP
/// entity tag. Not the time it changed: two saves in one second, on a disk that keeps whole seconds, would
/// have the same time and the same size.
pub fn version_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(bytes);
    format!("\"{}\"", hash[..16].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// A text file, once saved.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Saved {
    pub path: String,
    pub size: u64,
    /// When it last changed, in milliseconds since 1970.
    pub modified: Option<i64>,
    /// What it holds now, as `ETag` gives it: the next save says it replaces this.
    pub version: String,
}

/// Writes `text` over a text file of the session's folder, if it still holds what the person opened.
///
/// `replaces` is the `If-Match` of the request: the version the person opened, or `*` for whatever is
/// there. When the file has changed since (the model or another program wrote it), nothing is written.
/// The text is written beside the file and then moved into place, so that a file is never left half
/// written; it keeps its permissions.
pub fn save(folder: &Path, path: &str, text: &[u8], replaces: &str) -> Result<Saved> {
    let file = resolved(folder, path)?;
    let meta = std::fs::metadata(&file).map_err(|_| Error::NotFound(format!("There is no file \"{path}\" in the session's folder.")))?;
    if meta.is_dir() {
        return Err(Error::Invalid(format!("\"{path}\" is a folder.")));
    }
    if !kind_of(&file).is_text() {
        return Err(Error::Invalid(format!("\"{path}\" is not a text file. Only text is edited here.")));
    }
    if text.len() as u64 > MOST_SAVED {
        return Err(Error::Invalid(format!("That is more than {} MB, the most saved from here.", MOST_SAVED / 1_000_000)));
    }
    if std::str::from_utf8(text).is_err() {
        return Err(Error::Invalid("That text is not UTF-8, so it was not saved.".into()));
    }
    let any = replaces.split(',').any(|v| v.trim() == "*");
    if !any {
        if meta.len() > MOST_SAVED {
            return Err(Error::Invalid(format!("\"{path}\" is larger than {} MB, so it is not edited here.", MOST_SAVED / 1_000_000)));
        }
        let now = version_of(&std::fs::read(&file)?);
        if !replaces.split(',').any(|v| v.trim() == now) {
            return Err(Error::Changed(format!("\"{path}\" changed after it was opened (the model or another program wrote it), so it was not saved. Open it again to see what changed.")));
        }
    }
    write_whole(&file, text, Some(meta.permissions()))?;
    let meta = std::fs::metadata(&file)?;
    Ok(Saved { path: path.trim().trim_start_matches('/').to_string(), size: meta.len(), modified: modified_of(&meta), version: version_of(text) })
}

/// Writes `text` beside `file` and then moves it into place, so that a file is never left half written.
/// It keeps the permissions of the file it replaces.
fn write_whole(file: &Path, text: &[u8], keep: Option<std::fs::Permissions>) -> std::io::Result<()> {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let beside = file.with_file_name(format!(".{name}.demido-{}", uuid::Uuid::new_v4().simple()));
    let written = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut out = std::fs::File::create(&beside)?;
        out.write_all(text)?;
        if let Some(keep) = keep {
            out.set_permissions(keep)?;
        }
        out.sync_all()?;
        drop(out);
        std::fs::rename(&beside, file)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&beside);
    }
    written
}

/// Puts text into the session's folder as a file, as the person asks: a file a model gave (an artifact),
/// kept on disk. A file that is there is replaced only when `replace` says so; the folders on the way are
/// made. Only inside the session's folder.
pub fn put(folder: &Path, path: &str, text: &[u8], replace: bool) -> Result<Entry> {
    let path = path.trim().trim_start_matches('/');
    if path.is_empty() {
        return Err(Error::Invalid("Name the file it goes in.".into()));
    }
    let base = folder.canonicalize().map_err(|e| Error::NotFound(format!("The session's folder cannot be opened: {e}")))?;
    let file = resolved(folder, path)?;
    if file.is_dir() {
        return Err(Error::Invalid(format!("\"{path}\" is a folder.")));
    }
    let there = file.exists();
    if there && !replace {
        return Err(Error::Busy(format!("There is a file \"{path}\" in the folder already.")));
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let keep = if there { Some(std::fs::metadata(&file)?.permissions()) } else { None };
    write_whole(&file, text, keep)?;
    let meta = std::fs::metadata(&file)?;
    Ok(Entry {
        name: file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: shown(&base, &file),
        kind: "file".into(),
        shown_as: Some(kind_of(&file).word().to_string()),
        size: meta.len(),
        modified: modified_of(&meta),
    })
}

/// Renames a file or a folder of the session's folder, as the person asks: both names inside it, and the new
/// one not taken. The folders on the way to the new name are made.
pub fn rename(folder: &Path, from: &str, to: &str) -> Result<Entry> {
    let base = folder.canonicalize().map_err(|e| Error::NotFound(format!("The session's folder cannot be opened: {e}")))?;
    let (from, to) = (from.trim().trim_start_matches('/'), to.trim().trim_start_matches('/'));
    if from.is_empty() || to.is_empty() {
        return Err(Error::Invalid("Name what is renamed, and its new name.".into()));
    }
    let old = resolved(folder, from)?;
    if !old.exists() || old == base {
        return Err(Error::NotFound(format!("There is no \"{from}\" in the session's folder.")));
    }
    let new = resolved(folder, to)?;
    if new.exists() {
        return Err(Error::Busy(format!("There is a \"{to}\" in the folder already.")));
    }
    if new.starts_with(&old) {
        return Err(Error::Invalid(format!("\"{from}\" cannot go inside itself.")));
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&old, &new)?;
    let meta = std::fs::metadata(&new)?;
    Ok(Entry {
        name: new.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        path: shown(&base, &new),
        kind: if meta.is_dir() { "folder" } else { "file" }.into(),
        shown_as: (!meta.is_dir()).then(|| kind_of(&new).word().to_string()),
        size: if meta.is_dir() { 0 } else { meta.len() },
        modified: modified_of(&meta),
    })
}

/// What is different between a text file as it is now and the person's edit of it, as a unified diff:
/// `-` for lines only the file has, `+` for lines only the edit has.
#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Compared {
    pub diff: String,
    /// In a few characters: `+3 −1`.
    pub stat: String,
}

/// Compares a text file of the session's folder with `mine`, the person's edit of it.
pub fn compare(folder: &Path, path: &str, mine: &[u8]) -> Result<Compared> {
    let file = resolved(folder, path)?;
    if !file.is_file() || !kind_of(&file).is_text() {
        return Err(Error::Invalid(format!("\"{path}\" is not a text file in the session's folder.")));
    }
    let now = std::fs::read(&file)?;
    let shown = crate::tools::diff::of_file(path.trim().trim_start_matches('/'), Some(&now), Some(mine));
    let stat = shown.stat();
    Ok(Compared { diff: shown.text, stat })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> crate::testing::TempDir {
        let dir = crate::testing::TempDir::new("demido-folder");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("README.md"), "# Hello\n").unwrap();
        std::fs::write(dir.join("page.html"), "<script>alert(1)</script>").unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(dir.join("Makefile"), "all:\n\techo hi\n").unwrap();
        std::fs::write(dir.join("blob.bin"), [0u8, 1, 2, 3]).unwrap();
        std::fs::write(dir.join("photo.png"), [0x89, b'P', b'N', b'G']).unwrap();
        dir
    }

    #[test]
    fn a_folder_is_listed_folders_first_with_what_each_file_is_shown_as() {
        let dir = folder();
        let l = list(&dir, "").unwrap();
        let names: Vec<(&str, &str, Option<&str>)> = l.entries.iter().map(|e| (e.name.as_str(), e.kind.as_str(), e.shown_as.as_deref())).collect();
        assert_eq!(
            names,
            [
                ("src", "folder", None),
                ("blob.bin", "file", Some("other")),
                ("Makefile", "file", Some("text")),
                ("page.html", "file", Some("html")),
                ("photo.png", "file", Some("image")),
                ("README.md", "file", Some("markdown"))
            ]
        );
        assert_eq!(list(&dir, "src").unwrap().entries[0].path, "src/main.rs");
    }

    #[test]
    fn nothing_outside_the_folder_is_listed_or_opened() {
        let dir = folder();
        let beside = dir.parent().unwrap().join(format!("beside-{}", uuid::Uuid::new_v4()));
        std::fs::write(&beside, "not yours").unwrap();
        assert!(list(&dir, "..").is_err());
        let name = beside.file_name().unwrap().to_str().unwrap();
        assert!(open(&dir, &format!("../{name}")).is_err(), "a file that is there, beside the folder");
        assert!(open(&dir, beside.to_str().unwrap()).is_err(), "an absolute path elsewhere");
        #[cfg(unix)]
        {
            // A link inside that leads out is neither listed nor opened.
            std::os::unix::fs::symlink(&beside, dir.join("way-out")).unwrap();
            assert!(!list(&dir, "").unwrap().entries.iter().any(|e| e.name == "way-out"));
            assert!(open(&dir, "way-out").is_err());
        }
        std::fs::remove_file(beside).ok();
    }

    #[test]
    fn a_page_is_sent_as_text_and_a_picture_as_itself() {
        let dir = folder();
        assert_eq!(open(&dir, "page.html").unwrap().content_type, "text/plain; charset=utf-8", "never as a page that would run");
        assert_eq!(open(&dir, "README.md").unwrap().content_type, "text/plain; charset=utf-8");
        assert_eq!(open(&dir, "photo.png").unwrap().content_type, "image/png");
        assert_eq!(open(&dir, "blob.bin").unwrap().content_type, "application/octet-stream");
        assert!(open(&dir, "src").is_err());
    }

    #[test]
    fn a_range_is_read_as_browsers_ask_for_one() {
        assert_eq!(range(None, 100), Ok(None));
        assert_eq!(range(Some("bytes=0-"), 100), Ok(Some((0, 100))));
        assert_eq!(range(Some("bytes=10-19"), 100), Ok(Some((10, 10))));
        assert_eq!(range(Some("bytes=-10"), 100), Ok(Some((90, 10))));
        assert_eq!(range(Some("bytes=90-500"), 100), Ok(Some((90, 10))));
        assert_eq!(range(Some("bytes=100-"), 100), Err(NotInFile));
        assert_eq!(range(Some("bytes=0-1,5-6"), 100), Ok(None), "several ranges: the whole file");
    }

    #[test]
    fn a_file_is_put_in_the_folder_and_one_that_is_there_replaced_only_when_asked() {
        let dir = folder();
        let put_in = put(&dir, "pages/sol.html", b"<h1>Sol</h1>", false).unwrap();
        assert_eq!((put_in.path.as_str(), put_in.shown_as.as_deref(), put_in.size), ("pages/sol.html", Some("html"), 12));
        assert_eq!(std::fs::read_to_string(dir.join("pages/sol.html")).unwrap(), "<h1>Sol</h1>");
        assert!(matches!(put(&dir, "README.md", b"# Mine", false), Err(Error::Busy(_))), "a file that is there is not replaced unasked");
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "# Hello\n");
        put(&dir, "README.md", b"# Mine", true).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "# Mine");
        assert!(put(&dir, "../outside.md", b"x", false).is_err());
        assert!(put(&dir, "src", b"x", true).is_err(), "not over a folder");
        assert!(put(&dir, "  ", b"x", false).is_err());
    }

    #[test]
    fn a_file_is_made_and_renamed_inside_the_folder_and_nothing_is_overwritten() {
        let dir = folder();
        // A new file: `put` without replacing.
        assert_eq!(put(&dir, "notes/new.md", b"", false).unwrap().path, "notes/new.md");
        let moved = rename(&dir, "notes/new.md", "drafts/idea.md").unwrap();
        assert_eq!((moved.path.as_str(), moved.kind.as_str()), ("drafts/idea.md", "file"));
        assert!(dir.join("drafts/idea.md").is_file() && !dir.join("notes/new.md").exists());
        assert!(matches!(rename(&dir, "drafts/idea.md", "README.md"), Err(Error::Busy(_))), "not over a file that is there");
        assert!(rename(&dir, "README.md", "../README.md").is_err());
        assert!(rename(&dir, "../elsewhere", "here.md").is_err());
        assert!(matches!(rename(&dir, "missing.md", "x.md"), Err(Error::NotFound(_))));
        assert!(matches!(rename(&dir, "src", "src/inner"), Err(Error::Invalid(_))), "not into itself");
        assert_eq!(rename(&dir, "src", "code").unwrap().kind, "folder");
    }

    #[test]
    fn an_edit_is_compared_with_the_file_as_it_is_now() {
        let dir = folder();
        let compared = compare(&dir, "README.md", b"# Hello\n\nMine.\n").unwrap();
        assert_eq!(compared.stat, "+2 −0");
        assert!(compared.diff.contains("+Mine.") && compared.diff.contains("--- a/README.md"), "{}", compared.diff);
        assert!(compare(&dir, "photo.png", b"x").is_err());
        assert!(compare(&dir, "../elsewhere.md", b"x").is_err());
    }

    #[test]
    fn a_text_file_is_saved_only_over_what_was_opened_and_never_half_written() {
        let dir = folder();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.join("README.md"), std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let opened = version_of(&std::fs::read(dir.join("README.md")).unwrap());
        let saved = save(&dir, "README.md", b"# Changed\n", &opened).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "# Changed\n");
        assert_eq!((saved.size, saved.version.as_str()), (10, version_of(b"# Changed\n").as_str()));
        assert_ne!(saved.version, opened, "a new version");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join("README.md")).unwrap().permissions().mode() & 0o777, 0o640, "it keeps its permissions");
        }

        // Saved over the version first opened: the file changed since, and nothing is written.
        assert!(matches!(save(&dir, "README.md", b"# Mine\n", &opened), Err(Error::Changed(_))));
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "# Changed\n");
        // Over whatever is there, when the person says so.
        save(&dir, "README.md", b"# Mine\n", "*").unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "# Mine\n");
        // Nothing is left beside it.
        assert!(!std::fs::read_dir(&*dir).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().contains(".demido-")));

        // Only text, only inside, only UTF-8, only a file that is there.
        assert!(matches!(save(&dir, "photo.png", b"x", "*"), Err(Error::Invalid(_))));
        assert!(save(&dir, "../outside.md", b"x", "*").is_err());
        assert!(matches!(save(&dir, "README.md", &[0xff, 0xfe, 0x00], "*"), Err(Error::Invalid(_))));
        assert!(matches!(save(&dir, "src", b"x", "*"), Err(Error::Invalid(_))));
        assert!(save(&dir, "new.md", b"x", "*").is_err(), "a new file is not made here");
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "# Mine\n");
    }
}
