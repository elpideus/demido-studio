//! A session's folder, as the person sees it in the window (brief 4.24): what is in it, and each file, to
//! look at (DECISIONS F1 to F4).
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

/// What a file is shown as: by its name, and for a name that says nothing, by whether its start is text.
pub fn kind_of(path: &Path) -> Kind {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "mdx" => Kind::Markdown,
        "html" | "htm" | "xhtml" => Kind::Html,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "ico" => Kind::Image,
        "svg" => Kind::Svg,
        "pdf" => Kind::Pdf,
        "mp4" | "m4v" | "webm" | "mov" | "ogv" => Kind::Video,
        "mp3" | "wav" | "ogg" | "oga" | "opus" | "flac" | "m4a" | "aac" => Kind::Audio,
        _ => {
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
            let modified = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
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
    Ok(Opened { size: meta.len(), content_type: kind.content_type(&ext), kind, path: file })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("demido-folder-{}", uuid::Uuid::new_v4()));
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
        std::fs::remove_dir_all(dir).ok();
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
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_page_is_sent_as_text_and_a_picture_as_itself() {
        let dir = folder();
        assert_eq!(open(&dir, "page.html").unwrap().content_type, "text/plain; charset=utf-8", "never as a page that would run");
        assert_eq!(open(&dir, "README.md").unwrap().content_type, "text/plain; charset=utf-8");
        assert_eq!(open(&dir, "photo.png").unwrap().content_type, "image/png");
        assert_eq!(open(&dir, "blob.bin").unwrap().content_type, "application/octet-stream");
        assert!(open(&dir, "src").is_err());
        std::fs::remove_dir_all(dir).ok();
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
}
