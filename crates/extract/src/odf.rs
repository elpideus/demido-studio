//! OpenDocument text (.odt) and presentations (.odp). Spreadsheets (.ods) go through calamine.

use quick_xml::events::Event;

use crate::Extracted;
use crate::out::{Blocks, TextOut};
use crate::xml::{self, attr, event_text, local};

/// Reads `content.xml`: paragraphs, headings, lists and tables, and for a presentation each
/// slide under its marker with its speaker notes.
pub(crate) fn read(base: Extracted, bytes: &[u8], presentation: bool) -> Extracted {
    let what = if presentation { "presentation" } else { "document" };
    let content = xml::open_zip(bytes).and_then(|mut zip| xml::read_entry(&mut zip, "content.xml"));
    let Some(content) = content else {
        return base.with_note(format!("This {what} could not be read. It may be damaged."));
    };
    let mut out = TextOut::new();
    let slides = walk(&content, presentation, &mut out);
    let mut read = base.with_text(out);
    if presentation {
        read.pages = Some(slides);
    }
    if read.text.is_none() {
        return read.with_note(format!("No text found in this {what}."));
    }
    read
}

#[derive(Default)]
struct Paragraph {
    text: String,
    heading: Option<u8>,
    list: Option<u8>,
}

/// Writes the body of `content.xml` and returns the number of slides (draw pages).
fn walk(xml: &str, presentation: bool, out: &mut TextOut) -> u32 {
    let mut blocks = Blocks::new(out);
    // Open paragraphs: a text box's paragraphs sit inside another paragraph.
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut lists = 0u8;
    // Inside footnotes, comments and tracked deletions, which are not part of the text.
    let mut skip = 0u32;
    let mut slides = 0u32;
    // Inside a slide's speaker notes; `notes_said` once "Notes:" is written for this slide.
    let mut in_notes = false;
    let mut notes_said = false;
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
                let empty = matches!(event, Event::Empty(_));
                match local(e).as_slice() {
                    b"note" | b"annotation" | b"tracked-changes" if !empty => skip = 1,
                    b"page" if presentation => {
                        slides += 1;
                        blocks.out.page(&format!("--- Slide {slides} ---"));
                        blocks.separate();
                        notes_said = false;
                    }
                    b"notes" if presentation && !empty => in_notes = true,
                    b"frame" => blocks.separate(),
                    b"list" if !empty => lists = lists.saturating_add(1),
                    b"h" if !empty => paragraphs.push(Paragraph {
                        heading: Some(attr(e, b"outline-level").and_then(|l| l.parse().ok()).unwrap_or(1)),
                        ..Paragraph::default()
                    }),
                    b"p" if !empty => paragraphs.push(Paragraph {
                        list: lists.checked_sub(1),
                        ..Paragraph::default()
                    }),
                    b"table" if !empty => blocks.table_start(),
                    b"table-row" if !empty => blocks.row_start(),
                    b"table-cell" | b"covered-table-cell" => {
                        blocks.cell_start();
                        if empty {
                            blocks.cell_end();
                        }
                    }
                    b"s" => {
                        if let Some(p) = paragraphs.last_mut() {
                            let n = attr(e, b"c").and_then(|c| c.parse::<usize>().ok()).unwrap_or(1);
                            p.text.push_str(&" ".repeat(n.min(100)));
                        }
                    }
                    b"tab" => {
                        if let Some(p) = paragraphs.last_mut() {
                            p.text.push('\t');
                        }
                    }
                    b"line-break" => {
                        if let Some(p) = paragraphs.last_mut() {
                            p.text.push('\n');
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"h" | b"p" => {
                    let Some(mut p) = paragraphs.pop() else { continue };
                    if in_notes && !notes_said && !p.text.trim().is_empty() {
                        p.text = format!("Notes: {}", p.text.trim());
                        notes_said = true;
                    }
                    blocks.paragraph(&p.text, p.heading, p.list);
                }
                b"list" => lists = lists.saturating_sub(1),
                b"table-cell" | b"covered-table-cell" => blocks.cell_end(),
                b"table-row" => blocks.row_end(),
                b"table" => blocks.table_end(),
                b"notes" => in_notes = false,
                b"frame" => blocks.separate(),
                _ => {}
            },
            _ => {
                if let Some(p) = paragraphs.last_mut()
                    && let Some(text) = event_text(&event)
                {
                    p.text.push_str(&text);
                }
            }
        }
    }
    slides
}
