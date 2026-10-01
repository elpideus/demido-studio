//! Reading XML parts out of ZIP containers (Office Open XML, OpenDocument, EPUB).

use std::borrow::Cow;
use std::io::{Cursor, Read};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// The most one XML part may inflate to. Real documents are far below it; a ZIP bomb is not.
const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;

pub(crate) type Zip<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

pub(crate) fn open_zip(bytes: &[u8]) -> Option<Zip<'_>> {
    zip::ZipArchive::new(Cursor::new(bytes)).ok()
}

/// The most all the parts read from one file may inflate to together: a small file can list one
/// huge part thousands of times, and each would be inflated again.
const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024;

thread_local! {
    /// Bytes inflated for the file being read on this thread.
    static INFLATED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Starts the inflate budget of a new file.
pub(crate) fn reset_budget() {
    INFLATED.with(|n| n.set(0));
}

/// An entry's content as text, or None when it is missing, unreadable, too big, or the file's
/// budget is spent.
pub(crate) fn read_entry(zip: &mut Zip<'_>, name: &str) -> Option<String> {
    let used = INFLATED.with(std::cell::Cell::get);
    let room = MAX_TOTAL_BYTES.saturating_sub(used).min(MAX_PART_BYTES);
    let entry = zip.by_name(name).ok()?;
    if entry.size() > room {
        return None;
    }
    let mut bytes = Vec::new();
    entry.take(room).read_to_end(&mut bytes).ok()?;
    INFLATED.with(|n| n.set(used + bytes.len() as u64));
    Some(crate::text::decode(&bytes))
}

/// A lenient reader: Office files from odd producers still parse.
pub(crate) fn reader(xml: &str) -> Reader<&[u8]> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;
    reader
}

/// An element's name without its namespace prefix: `w:p` is `p`.
pub(crate) fn local(e: &BytesStart<'_>) -> Vec<u8> {
    e.local_name().as_ref().to_vec()
}

/// The value of the attribute with local name `name` (`w:val` matches `val`).
pub(crate) fn attr(e: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| value(&a.value))
}

/// The value of a namespaced attribute with local name `name`: `r:id`, not the plain `id` that
/// PowerPoint puts on the same element.
pub(crate) fn prefixed_attr(e: &BytesStart<'_>, name: &[u8]) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.prefix().is_some() && a.key.local_name().as_ref() == name)
        .and_then(|a| value(&a.value))
}

/// An attribute's raw value, unescaped. Every part is decoded to UTF-8 before it is parsed
/// (`read_entry`), so the bytes are UTF-8.
fn value(raw: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(raw).ok()?;
    Some(quick_xml::escape::unescape(s).map_or_else(|_| s.to_owned(), Cow::into_owned))
}

/// The text an event carries: text, CDATA, or a character or entity reference.
pub(crate) fn event_text<'a>(event: &'a Event<'_>) -> Option<Cow<'a, str>> {
    match event {
        Event::Text(t) => t.xml10_content().ok(),
        Event::CData(t) => t.xml10_content().ok(),
        Event::GeneralRef(r) => {
            if let Ok(Some(c)) = r.resolve_char_ref() {
                return Some(Cow::Owned(c.to_string()));
            }
            let name = r.xml10_content().ok()?;
            quick_xml::escape::resolve_predefined_entity(&name)
                .or_else(|| quick_xml::escape::resolve_html5_entity(&name))
                .map(Cow::Borrowed)
        }
        _ => None,
    }
}

/// Resolves a relationship or manifest `target` against the folder of the part that names it:
/// `("word/document.xml", "media/a.png")` is `word/media/a.png`, and `/ppt/x.xml` is `ppt/x.xml`.
pub(crate) fn resolve(from_part: &str, target: &str) -> String {
    let target = percent_decode(target.split('#').next().unwrap_or_default());
    if let Some(absolute) = target.strip_prefix('/') {
        return normalize(absolute.split('/').collect());
    }
    let mut parts: Vec<&str> = from_part.split('/').collect();
    parts.pop();
    parts.extend(target.split('/'));
    normalize(parts)
}

fn normalize(parts: Vec<&str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    for part in parts {
        match part {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out.join("/")
}

/// `%20` and friends in hrefs.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_owned();
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The relationships in `rels_part`, as (id, type, target part). Targets resolve against
/// `source_part`, the part the relationships belong to (empty for the package's own `_rels/.rels`).
pub(crate) fn relationships(zip: &mut Zip<'_>, rels_part: &str, source_part: &str) -> Vec<(String, String, String)> {
    let Some(xml) = read_entry(zip, rels_part) else {
        return Vec::new();
    };
    let mut reader = reader(&xml);
    let mut rels = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if local(&e) == b"Relationship" => {
                if attr(&e, b"TargetMode").as_deref() == Some("External") {
                    continue;
                }
                let (Some(id), Some(target)) = (attr(&e, b"Id"), attr(&e, b"Target")) else {
                    continue;
                };
                let kind = attr(&e, b"Type").unwrap_or_default();
                rels.push((id, kind, resolve(source_part, &target)));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    rels
}

/// The first part `part` relates to with a relationship type ending in `kind` (`/styles`,
/// `/notesSlide`). `part` is empty for the package itself.
pub(crate) fn related(zip: &mut Zip<'_>, part: &str, kind: &str) -> Option<String> {
    let rels = if part.is_empty() {
        "_rels/.rels".to_owned()
    } else {
        rels_part_of(part)
    };
    relationships(zip, &rels, part)
        .into_iter()
        .find(|(_, k, _)| k.ends_with(kind))
        .map(|(_, _, target)| target)
}

/// The `.rels` part that holds `part`'s relationships: `word/document.xml` has
/// `word/_rels/document.xml.rels`.
pub(crate) fn rels_part_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_resolve_against_their_part() {
        assert_eq!(resolve("word/document.xml", "styles.xml"), "word/styles.xml");
        assert_eq!(
            resolve("ppt/slides/slide1.xml", "../notesSlides/notesSlide1.xml"),
            "ppt/notesSlides/notesSlide1.xml"
        );
        assert_eq!(resolve("_rels/.rels", "/word/document.xml"), "word/document.xml");
        assert_eq!(
            resolve("OEBPS/content.opf", "Text/chapter%201.xhtml#top"),
            "OEBPS/Text/chapter 1.xhtml"
        );
        assert_eq!(rels_part_of("ppt/presentation.xml"), "ppt/_rels/presentation.xml.rels");
    }
}
