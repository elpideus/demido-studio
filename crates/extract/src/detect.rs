//! What a file is: its content's signature first, then its extension, then whether it reads as
//! text.

use std::io::{Cursor, Read};

use crate::{Kind, guarded, text};

pub(crate) const PDF: &str = "application/pdf";
pub(crate) const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
pub(crate) const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
pub(crate) const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const XLSM: &str = "application/vnd.ms-excel.sheet.macroEnabled.12";
const XLSB: &str = "application/vnd.ms-excel.sheet.binary.macroEnabled.12";
const XLS: &str = "application/vnd.ms-excel";
const ODT: &str = "application/vnd.oasis.opendocument.text";
const ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";
const ODP: &str = "application/vnd.oasis.opendocument.presentation";
const EPUB: &str = "application/epub+zip";
const DOC: &str = "application/msword";
const PPT: &str = "application/vnd.ms-powerpoint";

const OLD_WORD: &str = "Old Word documents (.doc) cannot be read. Save it as .docx and attach it again.";
const OLD_POWERPOINT: &str = "Old PowerPoint files (.ppt) cannot be read. Save it as .pptx and attach it again.";
const PROTECTED: &str = "This file is password-protected, so its text cannot be read.";

/// How a file's text is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// Text as it is: plain text, code, Markdown, CSV, JSON, XML.
    Text,
    Html,
    Pdf,
    Docx,
    Pptx,
    /// xlsx, xlsm, xlsb, xls, ods.
    Spreadsheet,
    Odt,
    Odp,
    Epub,
    Rtf,
    Image,
    Audio,
    Other,
}

pub(crate) struct Detected {
    pub kind: Kind,
    pub mime: String,
    pub format: Format,
    /// Known before reading: an old binary Office file, a password-protected one.
    pub note: Option<&'static str>,
}

fn found(kind: Kind, mime: &str, format: Format) -> Detected {
    Detected {
        kind,
        mime: mime.to_owned(),
        format,
        note: None,
    }
}

fn noted(kind: Kind, mime: &str, format: Format, note: &'static str) -> Detected {
    Detected {
        note: Some(note),
        ..found(kind, mime, format)
    }
}

/// What `bytes` is. `ext` is the file's lowercase extension, or empty.
pub(crate) fn detect(bytes: &[u8], ext: &str) -> Detected {
    if let Some(d) = by_signature(bytes, ext) {
        return d;
    }
    if let Some(d) = by_extension(ext) {
        // A real Office, OpenDocument or EPUB file always starts with its ZIP or OLE signature.
        // One that reads as text is a text export given that name, as banks and ERP systems
        // save HTML tables or tab-separated rows as ".xls".
        let container = matches!(
            d.format,
            Format::Docx | Format::Pptx | Format::Spreadsheet | Format::Odt | Format::Odp | Format::Epub
        ) || matches!(d.mime.as_str(), DOC | PPT);
        if container && !bytes.is_empty() && text::looks_like_text(bytes) {
            return sniff_text(bytes);
        }
        return d;
    }
    if text::looks_like_text(bytes) {
        return sniff_text(bytes);
    }
    found(Kind::Other, "application/octet-stream", Format::Other)
}

/// The magic bytes formats start with.
fn by_signature(b: &[u8], ext: &str) -> Option<Detected> {
    use Format as F;
    use Kind as K;

    // A byte order mark means text, and a UTF-16 one would pass for an MP3 frame below.
    if text::has_bom(b) {
        return None;
    }
    let at = |offset: usize, sig: &[u8]| b.get(offset..offset + sig.len()) == Some(sig);

    // PDF readers accept the header anywhere in the first kilobyte, but a text file that only
    // mentions it (a script writing PDFs, notes about the format) is still text: past the start,
    // the header counts for a ".pdf" file or a binary one.
    let header_at_start = at(0, b"%PDF-");
    let header_near_start = || b[..b.len().min(1024)].windows(5).any(|w| w == b"%PDF-");
    if header_at_start || ((ext == "pdf" || !text::looks_like_text(b)) && header_near_start()) {
        return Some(found(K::Document, PDF, F::Pdf));
    }
    let image = |mime: &str| Some(found(K::Image, mime, F::Image));
    let audio = |mime: &str| Some(found(K::Audio, mime, F::Audio));
    let other = |mime: &str| Some(found(K::Other, mime, F::Other));

    if at(0, b"\x89PNG\r\n\x1a\n") {
        return image("image/png");
    }
    if at(0, &[0xFF, 0xD8, 0xFF]) {
        return image("image/jpeg");
    }
    if at(0, b"GIF87a") || at(0, b"GIF89a") {
        return image("image/gif");
    }
    if at(0, b"RIFF") {
        if at(8, b"WEBP") {
            return image("image/webp");
        }
        if at(8, b"WAVE") {
            return audio("audio/wav");
        }
        if at(8, b"AVI ") {
            return other("video/x-msvideo");
        }
    }
    if at(0, b"II*\0") || at(0, b"MM\0*") {
        return image("image/tiff");
    }
    if at(0, b"BM")
        && b.len() >= 18
        && matches!(
            u32::from_le_bytes([b[14], b[15], b[16], b[17]]),
            12 | 40 | 52 | 56 | 64 | 108 | 124
        )
    {
        return image("image/bmp");
    }
    if at(0, &[0, 0, 1, 0]) && ext == "ico" {
        return image("image/x-icon");
    }
    if at(4, b"ftyp") {
        return Some(iso_media(b, ext));
    }
    if at(0, b"fLaC") {
        return audio("audio/flac");
    }
    if at(0, b"OggS") {
        return if ext == "ogv" {
            other("video/ogg")
        } else {
            audio("audio/ogg")
        };
    }
    if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
        return audio("audio/aiff");
    }
    if at(0, b"ID3") {
        return audio(if ext == "aac" { "audio/aac" } else { "audio/mpeg" });
    }
    if let Some(mime) = mpeg_audio(b) {
        return audio(mime);
    }
    if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        return other(if ext == "mkv" { "video/x-matroska" } else { "video/webm" });
    }
    if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") {
        return zipped(b, ext);
    }
    if at(0, &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some(compound_file(ext));
    }
    if at(0, b"{\\rtf") {
        return Some(found(K::Document, "application/rtf", F::Rtf));
    }
    if at(0, &[0x1F, 0x8B]) {
        return other("application/gzip");
    }
    if at(0, b"7z\xBC\xAF\x27\x1C") {
        return other("application/x-7z-compressed");
    }
    if at(0, b"Rar!\x1A\x07") {
        return other("application/vnd.rar");
    }
    if at(0, b"SQLite format 3\0") {
        return other("application/vnd.sqlite3");
    }
    None
}

/// An MPEG audio frame (MP3) or an AAC ADTS frame at the start of the file.
fn mpeg_audio(b: &[u8]) -> Option<&'static str> {
    let &[0xFF, b1, b2, ..] = b else {
        return None;
    };
    if b1 & 0xF6 == 0xF0 {
        return Some("audio/aac");
    }
    let version = (b1 >> 3) & 3;
    let layer = (b1 >> 1) & 3;
    let bitrate = b2 >> 4;
    let rate = (b2 >> 2) & 3;
    (b1 & 0xE0 == 0xE0 && version != 1 && layer != 0 && bitrate != 0xF && rate != 3).then_some("audio/mpeg")
}

/// The ISO media family (`ftyp` box): MP4 audio and video, QuickTime, HEIC, AVIF.
fn iso_media(b: &[u8], ext: &str) -> Detected {
    use Format as F;
    use Kind as K;
    let brand = &b[8..b.len().min(12)];
    match brand {
        b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" => found(K::Image, "image/heic", F::Image),
        b"mif1" | b"msf1" if ext == "avif" => found(K::Image, "image/avif", F::Image),
        b"mif1" | b"msf1" => found(K::Image, "image/heif", F::Image),
        b"avif" | b"avis" => found(K::Image, "image/avif", F::Image),
        b"M4A " | b"M4B " | b"M4P " | b"F4A " => found(K::Audio, "audio/mp4", F::Audio),
        b"qt  " => found(K::Other, "video/quicktime", F::Other),
        _ if ext == "m4a" => found(K::Audio, "audio/mp4", F::Audio),
        _ => found(K::Other, "video/mp4", F::Other),
    }
}

/// A ZIP container: Office Open XML, OpenDocument, EPUB, or a plain archive.
fn zipped(b: &[u8], ext: &str) -> Option<Detected> {
    use Format as F;
    use Kind as K;
    // An archive whose directory cannot be read falls back on its extension.
    let (names, mimetype) = guarded(|| zip_listing(b)).flatten()?;
    let has = |prefix: &str| names.iter().any(|n| n.starts_with(prefix));
    if has("word/") {
        return Some(found(K::Document, DOCX, F::Docx));
    }
    if has("ppt/") {
        return Some(found(K::Document, PPTX, F::Pptx));
    }
    if has("xl/") {
        let mime = if names.iter().any(|n| n == "xl/workbook.bin") {
            XLSB
        } else if ext == "xlsm" {
            XLSM
        } else {
            XLSX
        };
        return Some(found(K::Data, mime, F::Spreadsheet));
    }
    let mimetype = mimetype.unwrap_or_default();
    let mimetype = mimetype.trim();
    Some(if mimetype.starts_with(ODT) {
        found(K::Document, ODT, F::Odt)
    } else if mimetype.starts_with(ODS) {
        found(K::Data, ODS, F::Spreadsheet)
    } else if mimetype.starts_with(ODP) {
        found(K::Document, ODP, F::Odp)
    } else if mimetype == EPUB {
        found(K::Document, EPUB, F::Epub)
    } else {
        found(K::Other, "application/zip", F::Other)
    })
}

/// The entry names of a ZIP archive, and its `mimetype` entry (OpenDocument, EPUB).
fn zip_listing(b: &[u8]) -> Option<(Vec<String>, Option<String>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(b)).ok()?;
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    let mimetype = zip.by_name("mimetype").ok().and_then(|entry| {
        let mut s = String::new();
        entry.take(200).read_to_string(&mut s).ok()?;
        Some(s)
    });
    Some((names, mimetype))
}

/// An OLE compound file: old Office formats, and Office files encrypted with a password.
fn compound_file(ext: &str) -> Detected {
    use Format as F;
    use Kind as K;
    match ext {
        "xls" | "xlt" => found(K::Data, XLS, F::Spreadsheet),
        "doc" | "dot" => noted(K::Document, DOC, F::Other, OLD_WORD),
        "ppt" | "pps" | "pot" => noted(K::Document, PPT, F::Other, OLD_POWERPOINT),
        "docx" | "docm" => noted(K::Document, DOCX, F::Other, PROTECTED),
        "pptx" | "pptm" => noted(K::Document, PPTX, F::Other, PROTECTED),
        "xlsx" | "xlsm" | "xlsb" => noted(K::Data, XLSX, F::Other, PROTECTED),
        "msg" => found(K::Other, "application/vnd.ms-outlook", F::Other),
        _ => found(K::Other, "application/x-ole-storage", F::Other),
    }
}

/// What a file is from its extension alone.
fn by_extension(ext: &str) -> Option<Detected> {
    use Format as F;
    use Kind as K;
    let d = match ext {
        "pdf" => found(K::Document, PDF, F::Pdf),
        "docx" | "docm" | "dotx" | "dotm" => found(K::Document, DOCX, F::Docx),
        "pptx" | "pptm" | "potx" | "ppsx" => found(K::Document, PPTX, F::Pptx),
        "xlsx" | "xltx" => found(K::Data, XLSX, F::Spreadsheet),
        "xlsm" => found(K::Data, XLSM, F::Spreadsheet),
        "xlsb" => found(K::Data, XLSB, F::Spreadsheet),
        "xls" => found(K::Data, XLS, F::Spreadsheet),
        "ods" => found(K::Data, ODS, F::Spreadsheet),
        "odt" | "ott" => found(K::Document, ODT, F::Odt),
        "odp" | "otp" => found(K::Document, ODP, F::Odp),
        "epub" => found(K::Document, EPUB, F::Epub),
        "rtf" => found(K::Document, "application/rtf", F::Rtf),
        "doc" | "dot" => noted(K::Document, DOC, F::Other, OLD_WORD),
        "ppt" | "pps" => noted(K::Document, PPT, F::Other, OLD_POWERPOINT),
        "html" | "htm" | "xhtml" => found(K::Document, "text/html", F::Html),

        "csv" => found(K::Data, "text/csv", F::Text),
        "tsv" | "tab" => found(K::Data, "text/tab-separated-values", F::Text),
        "json" | "geojson" | "ipynb" => found(K::Data, "application/json", F::Text),
        "jsonl" | "ndjson" => found(K::Data, "application/x-ndjson", F::Text),
        "xml" => found(K::Data, "application/xml", F::Text),

        "md" | "markdown" => found(K::Text, "text/markdown", F::Text),
        "css" | "scss" | "sass" | "less" => found(K::Text, "text/css", F::Text),
        "js" | "mjs" | "cjs" | "jsx" => found(K::Text, "text/javascript", F::Text),
        "yaml" | "yml" => found(K::Text, "application/yaml", F::Text),
        "toml" => found(K::Text, "application/toml", F::Text),
        "sql" => found(K::Text, "application/sql", F::Text),
        "svg" => found(K::Text, "image/svg+xml", F::Text),
        "txt" | "text" | "log" | "rst" | "adoc" | "org" | "tex" | "bib" | "srt" | "vtt" | "ini" | "cfg" | "conf"
        | "env" | "properties" | "rs" | "py" | "pyw" | "ts" | "tsx" | "java" | "kt" | "kts" | "c" | "h" | "cpp"
        | "cc" | "cxx" | "hpp" | "hh" | "cs" | "go" | "rb" | "php" | "swift" | "m" | "mm" | "scala" | "dart"
        | "lua" | "r" | "pl" | "pm" | "sh" | "bash" | "zsh" | "fish" | "ps1" | "psm1" | "bat" | "cmd" | "vue"
        | "svelte" | "gradle" | "cmake" | "mk" | "diff" | "patch" | "graphql" | "proto" | "tf" | "nix" | "zig"
        | "ex" | "exs" | "erl" | "hs" | "clj" | "fs" | "vb" | "asm" => found(K::Text, "text/plain", F::Text),

        "png" => found(K::Image, "image/png", F::Image),
        "jpg" | "jpeg" | "jfif" => found(K::Image, "image/jpeg", F::Image),
        "gif" => found(K::Image, "image/gif", F::Image),
        "webp" => found(K::Image, "image/webp", F::Image),
        "bmp" => found(K::Image, "image/bmp", F::Image),
        "tif" | "tiff" => found(K::Image, "image/tiff", F::Image),
        "heic" => found(K::Image, "image/heic", F::Image),
        "heif" => found(K::Image, "image/heif", F::Image),
        "avif" => found(K::Image, "image/avif", F::Image),
        "ico" => found(K::Image, "image/x-icon", F::Image),

        "wav" => found(K::Audio, "audio/wav", F::Audio),
        "mp3" => found(K::Audio, "audio/mpeg", F::Audio),
        "flac" => found(K::Audio, "audio/flac", F::Audio),
        "ogg" | "oga" | "opus" => found(K::Audio, "audio/ogg", F::Audio),
        "m4a" => found(K::Audio, "audio/mp4", F::Audio),
        "aac" => found(K::Audio, "audio/aac", F::Audio),
        "aif" | "aiff" => found(K::Audio, "audio/aiff", F::Audio),

        "mp4" | "m4v" => found(K::Other, "video/mp4", F::Other),
        "mov" => found(K::Other, "video/quicktime", F::Other),
        "webm" => found(K::Other, "video/webm", F::Other),
        "zip" => found(K::Other, "application/zip", F::Other),
        _ => return None,
    };
    Some(d)
}

/// A text file whose extension says nothing: HTML and XML by their first tag, plain text otherwise.
fn sniff_text(b: &[u8]) -> Detected {
    use Format as F;
    use Kind as K;
    let head = String::from_utf8_lossy(&b[..b.len().min(512)])
        .trim_start()
        .to_ascii_lowercase();
    if head.starts_with("<!doctype html") || head.starts_with("<html") {
        found(K::Document, "text/html", F::Html)
    } else if head.starts_with("<?xml") && head.contains("<svg") {
        found(K::Text, "image/svg+xml", F::Text)
    } else if head.starts_with("<?xml") {
        found(K::Data, "application/xml", F::Text)
    } else {
        found(K::Text, "text/plain", F::Text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mime(bytes: &[u8], ext: &str) -> String {
        detect(bytes, ext).mime
    }

    #[test]
    fn signatures_win_over_extensions() {
        assert_eq!(mime(b"%PDF-1.7\n", "txt"), PDF);
        assert_eq!(mime(b"\x89PNG\r\n\x1a\n....", "jpg"), "image/png");
        assert_eq!(mime(b"RIFF\0\0\0\0WAVEfmt ", ""), "audio/wav");
        assert_eq!(mime(b"ID3\x04\0\0\0\0\0\0", ""), "audio/mpeg");
        assert_eq!(mime(&[0xFF, 0xFB, 0x90, 0x44], ""), "audio/mpeg");
        assert_eq!(mime(&[0xFF, 0xF1, 0x50, 0x80], ""), "audio/aac");
        assert_eq!(mime(b"fLaC\0\0\0\x22", ""), "audio/flac");
        assert_eq!(mime(b"OggS\0\x02", ""), "audio/ogg");
        assert_eq!(mime(b"\0\0\0\x20ftypM4A \0\0\0\0", ""), "audio/mp4");
        assert_eq!(mime(b"\0\0\0\x18ftypheic\0\0\0\0", "jpg"), "image/heic");
        assert_eq!(mime(b"\0\0\0\x18ftypisom\0\0\0\0", ""), "video/mp4");
        assert_eq!(mime(b"{\\rtf1\\ansi hello}", ""), "application/rtf");
    }

    #[test]
    fn text_by_content() {
        assert_eq!(detect(b"just some notes", "").kind, Kind::Text);
        assert_eq!(detect(b"just some notes", "weird").mime, "text/plain");
        assert_eq!(detect(b"<!DOCTYPE html><p>x", "").format, Format::Html);
        assert_eq!(detect(b"a,b\n1,2", "csv").kind, Kind::Data);
        assert_eq!(detect(b"\x01\x02\0\x03binary", "bin").kind, Kind::Other);
        // "BM" alone is not a bitmap.
        assert_eq!(detect(b"BMW and Audi", "").kind, Kind::Text);
        // A UTF-16 text file is not an MP3.
        assert_eq!(detect(&[0xFF, 0xFE, b'h', 0], "").kind, Kind::Text);
    }

    #[test]
    fn text_that_mentions_a_format_is_still_text() {
        // A script that writes PDFs mentions the header; only a .pdf or a binary file is one.
        let script = b"with open('a.pdf', 'wb') as f:\n    f.write(b'%PDF-1.4\\n')\n";
        assert_eq!(detect(script, "py").kind, Kind::Text);
        assert_eq!(mime(b"junk before %PDF-1.7\n", "pdf"), PDF);
        // Bank and ERP exports: HTML or tab-separated rows saved as ".xls".
        assert_eq!(
            detect(b"<html><table><tr><td>1</td></tr></table>", "xls").format,
            Format::Html
        );
        assert_eq!(detect(b"Date\tAmount\n2026-01-02\t10.50\n", "xls").format, Format::Text);
        // A real workbook keeps its extension's meaning.
        assert_eq!(
            detect(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0], "xls").mime,
            XLS
        );
    }
}
