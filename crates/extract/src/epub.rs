//! EPUB books: the chapters in reading order (the package's spine), each read as HTML.

use std::collections::HashMap;

use quick_xml::events::Event;

use crate::Extracted;
use crate::out::TextOut;
use crate::xml::{self, attr, local};

pub(crate) fn read(base: Extracted, bytes: &[u8]) -> Extracted {
    const DAMAGED: &str = "This e-book could not be read. It may be damaged.";
    let Some(mut zip) = xml::open_zip(bytes) else {
        return base.with_note(DAMAGED);
    };
    let package = xml::read_entry(&mut zip, "META-INF/container.xml").and_then(|container| first_rootfile(&container));
    let mut chapters = package
        .as_deref()
        .and_then(|opf| Some((opf, xml::read_entry(&mut zip, opf)?)))
        .map(|(opf, xml)| spine(opf, &xml))
        .unwrap_or_default();
    if chapters.is_empty() {
        // No readable package: every XHTML file, by name.
        chapters = zip
            .file_names()
            .filter(|n| n.ends_with(".xhtml") || n.ends_with(".html") || n.ends_with(".htm"))
            .map(str::to_owned)
            .collect();
        chapters.sort();
    }
    if chapters.is_empty() {
        return base.with_note(DAMAGED);
    }
    // Each chapter is read once, however often the spine names it.
    let mut seen = std::collections::HashSet::new();
    chapters.retain(|c| seen.insert(c.clone()));
    let mut out = TextOut::new();
    for chapter in &chapters {
        if out.is_full() {
            break;
        }
        if let Some(html) = xml::read_entry(&mut zip, chapter) {
            out.paragraph(&crate::html::to_text(&html));
        }
    }
    let read = base.with_text(out);
    if read.text.is_none() {
        return read.with_note("No text found in this e-book.");
    }
    read
}

/// The package document (`content.opf`) named in `META-INF/container.xml`.
fn first_rootfile(container: &str) -> Option<String> {
    let mut reader = xml::reader(container);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) if local(&e) == b"rootfile" => {
                if let Some(path) = attr(&e, b"full-path") {
                    return Some(path);
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

/// The chapter files in reading order: the spine's item references, looked up in the manifest.
fn spine(opf_path: &str, opf: &str) -> Vec<String> {
    let mut manifest: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut reader = xml::reader(opf);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match local(&e).as_slice() {
                b"item" => {
                    let media = attr(&e, b"media-type").unwrap_or_default();
                    if media.contains("html")
                        && let (Some(id), Some(href)) = (attr(&e, b"id"), attr(&e, b"href"))
                    {
                        manifest.insert(id, xml::resolve(opf_path, &href));
                    }
                }
                b"itemref" => {
                    if let Some(id) = attr(&e, b"idref") {
                        order.push(id);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    order.iter().filter_map(|id| manifest.get(id).cloned()).collect()
}
