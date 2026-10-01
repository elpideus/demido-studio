//! Word (.docx) and PowerPoint (.pptx): zipped XML, read as a stream of events.

use std::collections::HashMap;

use quick_xml::events::Event;

use crate::Extracted;
use crate::out::{Blocks, TextOut};
use crate::xml::{self, Zip, attr, event_text, local};

/// The package's main part (`word/document.xml`, `ppt/presentation.xml`), from `_rels/.rels`.
fn main_part(zip: &mut Zip<'_>, usual: &str) -> String {
    xml::related(zip, "", "/officeDocument")
        .filter(|part| zip.index_for_name(part).is_some())
        .unwrap_or_else(|| usual.to_owned())
}

pub(crate) fn docx(base: Extracted, bytes: &[u8]) -> Extracted {
    const DAMAGED: &str = "This Word document could not be read. It may be damaged.";
    let Some(mut zip) = xml::open_zip(bytes) else {
        return base.with_note(DAMAGED);
    };
    let main = main_part(&mut zip, "word/document.xml");
    let Some(document) = xml::read_entry(&mut zip, &main) else {
        return base.with_note(DAMAGED);
    };
    let styles_part = xml::related(&mut zip, &main, "/styles").unwrap_or_else(|| xml::resolve(&main, "styles.xml"));
    let styles = xml::read_entry(&mut zip, &styles_part)
        .map(|s| heading_styles(&s))
        .unwrap_or_default();
    let mut out = TextOut::new();
    word_body(&document, &styles, &mut out);
    let read = base.with_text(out);
    if read.text.is_none() {
        return read.with_note("No text found in this document.");
    }
    read
}

#[derive(Default)]
struct Paragraph {
    text: String,
    heading: Option<u8>,
    list: Option<u8>,
}

/// Paragraphs, headings, list items and tables of `word/document.xml`.
fn word_body(xml: &str, styles: &HashMap<String, u8>, out: &mut TextOut) {
    let mut blocks = Blocks::new(out);
    // Open paragraphs: a text box's paragraphs sit inside a paragraph of the body.
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut in_text = false;
    // Inside paragraph properties, where `w:tab` is a tab stop rather than a tab.
    let mut in_properties = 0u32;
    // Inside `mc:Fallback`, a second copy of a drawing's text for old readers.
    let mut skip = 0u32;
    let mut reader = xml::reader(xml);
    while !blocks.out.is_full() {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        if skip > 0 {
            match event {
                Event::Start(_) => skip += 1,
                Event::End(_) => skip -= 1,
                _ => {}
            }
            continue;
        }
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let name = local(e);
                if matches!(event, Event::Start(_)) {
                    match name.as_slice() {
                        b"Fallback" => skip = 1,
                        b"p" => paragraphs.push(Paragraph::default()),
                        b"pPr" => in_properties += 1,
                        b"t" => in_text = true,
                        b"tbl" => blocks.table_start(),
                        b"tr" => blocks.row_start(),
                        b"tc" => blocks.cell_start(),
                        _ => {}
                    }
                }
                let Some(p) = paragraphs.last_mut() else {
                    continue;
                };
                match name.as_slice() {
                    b"tab" | b"ptab" if in_properties == 0 => p.text.push('\t'),
                    b"br" | b"cr" => p.text.push('\n'),
                    b"noBreakHyphen" => p.text.push('-'),
                    b"pStyle" => {
                        if let Some(id) = attr(e, b"val") {
                            p.heading = styles.get(&id).copied().or_else(|| level_from_style_id(&id));
                        }
                    }
                    b"outlineLvl" if in_properties > 0 => {
                        if let Some(level) = attr(e, b"val").and_then(|v| v.parse::<u8>().ok()).filter(|&l| l < 9) {
                            p.heading = Some(level + 1);
                        }
                    }
                    b"numPr" if in_properties > 0 => p.list = Some(0),
                    b"ilvl" if in_properties > 0 => {
                        p.list = Some(attr(e, b"val").and_then(|v| v.parse().ok()).unwrap_or(0));
                    }
                    // Numbering id 0 takes a style's numbering off.
                    b"numId" if attr(e, b"val").as_deref() == Some("0") => p.list = None,
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"p" => {
                    if let Some(p) = paragraphs.pop() {
                        blocks.paragraph(&p.text, p.heading, p.list);
                    }
                }
                b"pPr" => in_properties = in_properties.saturating_sub(1),
                b"t" => in_text = false,
                b"tc" => blocks.cell_end(),
                b"tr" => blocks.row_end(),
                b"tbl" => blocks.table_end(),
                _ => {}
            },
            _ => {
                if in_text
                    && let Some(text) = event_text(&event)
                    && let Some(p) = paragraphs.last_mut()
                {
                    p.text.push_str(&text);
                }
            }
        }
    }
}

/// The heading level of every paragraph style that is a heading.
///
/// Built-in headings are recognized by their name ("heading 1", "Title"), which stays English in
/// every language, while the style id is translated (Italian Word writes "Titolo1"). Styles with
/// an outline level, and styles based on a heading, count too.
fn heading_styles(xml: &str) -> HashMap<String, u8> {
    #[derive(Default)]
    struct Style {
        level: Option<u8>,
        based_on: Option<String>,
    }
    let mut styles: HashMap<String, Style> = HashMap::new();
    let mut current: Option<(String, Style)> = None;
    let mut reader = xml::reader(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => match local(&e).as_slice() {
                b"style" => {
                    if let Some((id, style)) = current.take() {
                        styles.insert(id, style);
                    }
                    current = attr(&e, b"styleId").map(|id| (id, Style::default()));
                }
                b"name" => {
                    if let Some((_, style)) = current.as_mut()
                        && let Some(level) = attr(&e, b"val").and_then(|n| level_from_name(&n))
                    {
                        style.level = Some(level);
                    }
                }
                b"outlineLvl" => {
                    if let Some((_, style)) = current.as_mut()
                        && style.level.is_none()
                        && let Some(level) = attr(&e, b"val").and_then(|v| v.parse::<u8>().ok()).filter(|&l| l < 9)
                    {
                        style.level = Some(level + 1);
                    }
                }
                b"basedOn" => {
                    if let Some((_, style)) = current.as_mut() {
                        style.based_on = attr(&e, b"val");
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if e.local_name().as_ref() == b"style" => {
                if let Some((id, style)) = current.take() {
                    styles.insert(id, style);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut levels = HashMap::new();
    for id in styles.keys() {
        let mut style = styles.get(id);
        // Follow `basedOn` a few steps; a loop in a broken file ends here too.
        for _ in 0..10 {
            let Some(s) = style else { break };
            if let Some(level) = s.level {
                levels.insert(id.clone(), level);
                break;
            }
            style = s.based_on.as_ref().and_then(|b| styles.get(b));
        }
    }
    levels
}

/// "heading 2" is 2, "Title" is 1.
fn level_from_name(name: &str) -> Option<u8> {
    let name = name.trim().to_ascii_lowercase();
    if name == "title" {
        return Some(1);
    }
    name.strip_prefix("heading ")?
        .trim()
        .parse()
        .ok()
        .filter(|l| (1..=9).contains(l))
}

/// A heading style id when the file has no styles part: "Heading2" is 2.
fn level_from_style_id(id: &str) -> Option<u8> {
    let id = id.to_ascii_lowercase();
    if id == "title" {
        return Some(1);
    }
    id.strip_prefix("heading")?.parse().ok().filter(|l| (1..=9).contains(l))
}

pub(crate) fn pptx(base: Extracted, bytes: &[u8]) -> Extracted {
    const DAMAGED: &str = "This PowerPoint file could not be read. It may be damaged.";
    let Some(mut zip) = xml::open_zip(bytes) else {
        return base.with_note(DAMAGED);
    };
    let main = main_part(&mut zip, "ppt/presentation.xml");
    let mut slides = slide_parts(&mut zip, &main);
    // Each slide is read once, however often the list names it.
    let mut seen = std::collections::HashSet::new();
    slides.retain(|s| seen.insert(s.clone()));
    if slides.is_empty() {
        return base.with_note(if zip.index_for_name(&main).is_some() {
            "This presentation has no slides."
        } else {
            DAMAGED
        });
    }
    let mut out = TextOut::new();
    for (i, slide) in slides.iter().enumerate() {
        if out.is_full() {
            break;
        }
        out.page(&format!("--- Slide {} ---", i + 1));
        if let Some(xml) = xml::read_entry(&mut zip, slide) {
            slide_text(&xml, false, &mut out);
        }
        if let Some(notes) = xml::related(&mut zip, slide, "/notesSlide")
            && let Some(xml) = xml::read_entry(&mut zip, &notes)
        {
            slide_text(&xml, true, &mut out);
        }
    }
    let mut read = base.with_text(out);
    read.pages = Some(u32::try_from(slides.len()).unwrap_or(u32::MAX));
    if read.text.is_none() {
        return read.with_note("No text found in the slides.");
    }
    read
}

/// The slide parts in show order: `p:sldIdLst` in the presentation, through its relationships.
/// A file without that list falls back on `ppt/slides/slideN.xml` by number.
fn slide_parts(zip: &mut Zip<'_>, main: &str) -> Vec<String> {
    let targets: HashMap<String, String> = xml::relationships(zip, &xml::rels_part_of(main), main)
        .into_iter()
        .map(|(id, _, target)| (id, target))
        .collect();
    let mut slides = Vec::new();
    if let Some(presentation) = xml::read_entry(zip, main) {
        let mut reader = xml::reader(&presentation);
        loop {
            match reader.read_event() {
                Ok(Event::Start(e) | Event::Empty(e)) if local(&e) == b"sldId" => {
                    if let Some(target) = xml::prefixed_attr(&e, b"id").and_then(|id| targets.get(&id))
                        && zip.index_for_name(target).is_some()
                    {
                        slides.push(target.clone());
                    }
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
    }
    if slides.is_empty() {
        let mut numbered: Vec<(u32, String)> = zip
            .file_names()
            .filter_map(|name| {
                let n = name
                    .strip_prefix("ppt/slides/slide")?
                    .strip_suffix(".xml")?
                    .parse()
                    .ok()?;
                Some((n, name.to_owned()))
            })
            .collect();
        numbered.sort();
        slides = numbered.into_iter().map(|(_, name)| name).collect();
    }
    slides
}

/// The text of a slide's shapes (one paragraph per shape, a line per text paragraph) and tables.
/// For speaker notes (`notes`), only the notes body, after "Notes:".
fn slide_text(xml: &str, notes: bool, out: &mut TextOut) {
    struct Shape {
        placeholder: Option<String>,
        lines: Vec<String>,
    }
    let mut blocks = Blocks::new(out);
    blocks.separate();
    let mut shape: Option<Shape> = None;
    let mut paragraph: Option<String> = None;
    let mut in_text = false;
    let mut tables = 0u32;
    let mut reader = xml::reader(xml);
    while !blocks.out.is_full() {
        let event = match reader.read_event() {
            Ok(Event::Eof) | Err(_) => break,
            Ok(event) => event,
        };
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                match local(e).as_slice() {
                    b"sp" if !empty => {
                        shape = Some(Shape {
                            placeholder: None,
                            lines: Vec::new(),
                        });
                    }
                    b"ph" => {
                        if let Some(s) = shape.as_mut() {
                            // A placeholder without a type is a content ("obj") placeholder.
                            s.placeholder = Some(attr(e, b"type").unwrap_or_else(|| "obj".to_owned()));
                        }
                    }
                    b"p" if !empty => paragraph = Some(String::new()),
                    b"t" if !empty => in_text = true,
                    b"br" => {
                        if let Some(p) = paragraph.as_mut() {
                            p.push('\n');
                        }
                    }
                    b"tbl" if !empty => {
                        tables += 1;
                        blocks.table_start();
                    }
                    b"tr" if !empty => blocks.row_start(),
                    b"tc" if !empty => blocks.cell_start(),
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    let text = paragraph.take().unwrap_or_default();
                    match shape.as_mut() {
                        Some(s) if tables == 0 => s.lines.push(text),
                        _ => blocks.paragraph(&text, None, None),
                    }
                }
                b"tc" => blocks.cell_end(),
                b"tr" => blocks.row_end(),
                b"tbl" => {
                    tables = tables.saturating_sub(1);
                    blocks.table_end();
                }
                b"sp" => {
                    let Some(s) = shape.take() else { continue };
                    let kind = s.placeholder.as_deref();
                    let wanted = if notes {
                        kind == Some("body")
                    } else {
                        // Slide numbers, dates and footers repeat on every slide.
                        !matches!(kind, Some("sldNum" | "dt" | "ftr"))
                    };
                    let lines: Vec<&str> = s.lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
                    if wanted && !lines.is_empty() {
                        let text = lines.join("\n");
                        blocks.separate();
                        blocks
                            .out
                            .paragraph(&if notes { format!("Notes: {text}") } else { text });
                    }
                }
                _ => {}
            },
            _ => {
                if in_text
                    && let Some(text) = event_text(&event)
                    && let Some(p) = paragraph.as_mut()
                {
                    p.push_str(&text);
                }
            }
        }
    }
}
