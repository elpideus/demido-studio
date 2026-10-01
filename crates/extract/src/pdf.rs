//! PDF text, page by page, through pdf-extract (MIT, pure Rust, on lopdf). It reads the text layer
//! in content-stream order, decodes the common font encodings, and opens PDFs encrypted with an
//! empty user password, which is how most "protected" PDFs that anyone can open are made. A
//! scanned PDF has no text layer; it gets a note instead of text.

use crate::out::TextOut;
use crate::{Extracted, guarded};

const DAMAGED: &str = "This PDF could not be read. It may be damaged.";
const PROTECTED: &str = "This PDF is password-protected, so its text cannot be read.";
const SCANNED: &str = "No text found. It may be a scanned PDF.";

/// Stack for the parser: deeply nested PDFs recurse further than a blocking thread's default
/// stack allows, and a stack overflow cannot be caught.
const STACK: usize = 64 * 1024 * 1024;

pub(crate) fn read(base: Extracted, bytes: &[u8]) -> Extracted {
    let owned = bytes.to_vec();
    let parsed = std::thread::Builder::new()
        .name("pdf-extract".into())
        .stack_size(STACK)
        .spawn(move || guarded(|| pdf_extract::extract_text_from_mem_by_pages(&owned).map_err(|e| e.to_string())))
        .ok()
        .and_then(|thread| thread.join().ok())
        .flatten();
    let pages = match parsed {
        Some(Ok(pages)) => pages,
        Some(Err(e)) if is_encryption_error(&e) => return base.with_note(PROTECTED),
        _ => return base.with_note(DAMAGED),
    };
    let mut out = TextOut::new();
    for (i, page) in pages.iter().enumerate() {
        if out.is_full() {
            break;
        }
        out.page(&format!("--- Page {} ---", i + 1));
        let text = tidy(page);
        if !text.is_empty() {
            out.paragraph(&text);
        }
    }
    let mut read = base.with_text(out);
    read.pages = Some(u32::try_from(pages.len()).unwrap_or(u32::MAX));
    if read.text.is_none() {
        return read.with_note(SCANNED);
    }
    read
}

fn is_encryption_error(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    m.contains("decrypt") || m.contains("encrypt") || m.contains("password")
}

/// A page's text with its layout noise removed: spaces at line ends, runs of blank lines (the
/// extractor writes one per gap in the layout), and control characters some fonts decode to.
fn tidy(page: &str) -> String {
    let mut out = String::with_capacity(page.len());
    let mut blank_run = 0usize;
    for line in page.lines() {
        let line: String = line
            .chars()
            .filter(|c| !c.is_control() || *c == '\t')
            .collect::<String>()
            .trim_end()
            .to_owned();
        if line.trim().is_empty() {
            blank_run += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank_run > 0 { "\n\n" } else { "\n" });
        }
        blank_run = 0;
        out.push_str(&line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    /// A minimal PDF with one page per text, using the standard Helvetica font. Offsets in the
    /// cross-reference table are computed, so the file is valid.
    pub(crate) fn pdf_with_pages(texts: &[&str]) -> Vec<u8> {
        let n = texts.len();
        let mut objects: Vec<String> = Vec::new();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
        objects.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into());
        for (i, text) in texts.iter().enumerate() {
            let content = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                5 + 2 * i
            ));
            objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
        }
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref = pdf.len();
        let mut table = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for off in offsets {
            table.push_str(&format!("{off:010} 00000 n \n"));
        }
        pdf.extend_from_slice(table.as_bytes());
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn base() -> Extracted {
        Extracted::new(Kind::Document, "application/pdf")
    }

    #[test]
    fn pages_are_read_and_marked() {
        let pdf = pdf_with_pages(&["The quarterly revenue grew.", "Termination needs notice."]);
        let read = read(base(), &pdf);
        let text = read.text.expect("text");
        assert_eq!(read.pages, Some(2));
        assert!(text.starts_with("--- Page 1 ---\n"), "{text}");
        assert!(text.contains("The quarterly revenue grew."), "{text}");
        assert!(text.contains("--- Page 2 ---\nTermination needs notice."), "{text}");
        assert_eq!(read.page_starts.len(), 2);
        assert_eq!(&text[read.page_starts[1]..read.page_starts[1] + 14], "--- Page 2 ---");
    }

    #[test]
    fn pages_without_text_are_a_scanned_pdf() {
        let read = read(base(), &pdf_with_pages(&[""]));
        assert_eq!(read.text, None);
        assert_eq!(read.pages, Some(1));
        assert_eq!(read.note.as_deref(), Some(SCANNED));
    }

    #[test]
    fn garbage_is_damaged_not_a_crash() {
        let read = read(base(), b"%PDF-1.7\n1 0 obj << /Kids [ ( nonsense");
        assert_eq!(read.text, None);
        assert_eq!(read.note.as_deref(), Some(DAMAGED));
    }

    #[test]
    fn layout_noise_is_tidied() {
        assert_eq!(tidy("  a  \n\n\n\nb\u{0}\nc\n\n"), "  a\n\nb\nc");
        assert_eq!(tidy("\n\n"), "");
    }
}
