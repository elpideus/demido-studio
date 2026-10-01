//! `extract` end to end, one file per format, built here so the tests need no fixtures on disk.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use demido_extract::{Kind, MAX_TEXT_CHARS, extract};
use zip::write::SimpleFileOptions;

fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

#[test]
fn word_documents() {
    let dir = tempfile::tempdir().unwrap();
    let document = format!(
        r#"<?xml version="1.0"?><w:document {W}><w:body>
        <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Service agreement</w:t></w:r></w:p>
        <w:p><w:r><w:t xml:space="preserve">Either party may </w:t></w:r><w:r><w:t>terminate &amp; leave.</w:t></w:r></w:p>
        <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First item</w:t></w:r></w:p>
        <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Fee</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>100 EUR</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
        </w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0"?><w:styles {W}><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#
    );
    let path = file(
        dir.path(),
        "contract.docx",
        &zip_of(&[("word/document.xml", &document), ("word/styles.xml", &styles)]),
    );
    let read = extract(&path).unwrap();
    assert_eq!(read.kind, Kind::Document);
    assert!(read.mime.contains("wordprocessingml"));
    let text = read.text.unwrap();
    assert!(text.contains("# Service agreement"), "{text}");
    assert!(text.contains("Either party may terminate & leave."), "{text}");
    assert!(text.contains("- First item"), "{text}");
    assert!(text.contains("Fee | 100 EUR"), "{text}");
}

#[test]
fn presentations_are_read_slide_by_slide() {
    let dir = tempfile::tempdir().unwrap();
    let slide = |t: &str| {
        format!(
            r#"<?xml version="1.0"?><p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:txBody><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#
        )
    };
    let path = file(
        dir.path(),
        "deck.pptx",
        &zip_of(&[
            ("ppt/slides/slide1.xml", &slide("Quarterly results")),
            ("ppt/slides/slide2.xml", &slide("Revenue grew 12%")),
        ]),
    );
    let read = extract(&path).unwrap();
    assert_eq!(read.kind, Kind::Document);
    assert_eq!(read.pages, Some(2));
    let text = read.text.unwrap();
    assert!(text.starts_with("--- Slide 1 ---\nQuarterly results"), "{text}");
    assert!(text.contains("--- Slide 2 ---\nRevenue grew 12%"), "{text}");
    assert_eq!(read.page_starts.len(), 2);
}

#[test]
fn opendocument_text_and_epub() {
    let dir = tempfile::tempdir().unwrap();
    let content = r#"<?xml version="1.0"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text><text:h text:outline-level="2">Minutes</text:h><text:p>We agreed on Friday.</text:p></office:text></office:body></office:document-content>"#;
    let path = file(
        dir.path(),
        "minutes.odt",
        &zip_of(&[
            ("mimetype", "application/vnd.oasis.opendocument.text"),
            ("content.xml", content),
        ]),
    );
    let read = extract(&path).unwrap();
    assert_eq!(read.kind, Kind::Document);
    let text = read.text.unwrap();
    assert!(text.contains("Minutes"), "{text}");
    assert!(text.contains("We agreed on Friday."), "{text}");

    let container = r#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/book.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
    let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf"><manifest><item id="c2" href="two.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="one.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;
    let chapter = |t: &str| format!("<html><body><h1>{t}</h1><p>Text of {t}.</p></body></html>");
    let path = file(
        dir.path(),
        "book.epub",
        &zip_of(&[
            ("mimetype", "application/epub+zip"),
            ("META-INF/container.xml", container),
            ("OEBPS/book.opf", opf),
            ("OEBPS/one.xhtml", &chapter("Chapter One")),
            ("OEBPS/two.xhtml", &chapter("Chapter Two")),
        ]),
    );
    let read = extract(&path).unwrap();
    assert_eq!(read.mime, "application/epub+zip");
    let text = read.text.unwrap();
    let (one, two) = (text.find("Chapter One").unwrap(), text.find("Chapter Two").unwrap());
    assert!(one < two, "spine order: {text}");
}

#[test]
fn web_pages_text_and_data() {
    let dir = tempfile::tempdir().unwrap();
    let html = file(
        dir.path(),
        "page.html",
        b"<html><head><style>p{color:red}</style><script>var x = 1;</script></head><body><h1>Title</h1><p>Fish &amp; chips</p></body></html>",
    );
    let read = extract(&html).unwrap();
    let text = read.text.unwrap();
    assert!(text.contains("Title") && text.contains("Fish & chips"), "{text}");
    assert!(!text.contains("color:red") && !text.contains("var x"), "{text}");

    let csv = extract(&file(dir.path(), "sales.csv", b"region,revenue\nNorth,10\n")).unwrap();
    assert_eq!(csv.kind, Kind::Data);
    assert_eq!(csv.text.as_deref(), Some("region,revenue\nNorth,10"));

    let mut utf16 = vec![0xFF, 0xFE];
    for unit in "città".encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    let read = extract(&file(dir.path(), "notes.txt", &utf16)).unwrap();
    assert_eq!(read.kind, Kind::Text);
    assert_eq!(read.text.as_deref(), Some("città"));

    // No extension, but plainly text.
    let read = extract(&file(dir.path(), "README", b"just words\n")).unwrap();
    assert_eq!(read.kind, Kind::Text);
}

#[test]
fn images_and_sound_are_recognised() {
    let dir = tempfile::tempdir().unwrap();
    let img = image::DynamicImage::ImageRgb8(image::ImageBuffer::from_pixel(32, 16, image::Rgb([1, 2, 3])));
    let mut png = Vec::new();
    img.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    // The content decides, not the name.
    let read = extract(&file(dir.path(), "photo.jpg", &png)).unwrap();
    assert_eq!((read.kind, read.mime.as_str()), (Kind::Image, "image/png"));
    assert_eq!(read.dimensions, Some((32, 16)));
    assert!(read.text.is_none());

    let mut wav = b"RIFF\x24\0\0\0WAVEfmt ".to_vec();
    wav.extend_from_slice(&[16, 0, 0, 0, 1, 0, 1, 0, 0x40, 0x1F, 0, 0, 0x80, 0x3E, 0, 0, 2, 0, 16, 0]);
    let read = extract(&file(dir.path(), "memo.wav", &wav)).unwrap();
    assert_eq!((read.kind, read.mime.as_str()), (Kind::Audio, "audio/wav"));
}

#[test]
fn damaged_files_are_notes_not_crashes() {
    let dir = tempfile::tempdir().unwrap();
    let garbage: Vec<u8> = (0..4096u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    for name in ["x.pdf", "x.docx", "x.pptx", "x.xlsx", "x.odt", "x.epub", "x.png"] {
        let read = extract(&file(dir.path(), name, &garbage)).unwrap();
        assert!(read.text.is_none() || read.note.is_some(), "{name}");
    }
    let mut broken_pdf = b"%PDF-1.7\n".to_vec();
    broken_pdf.extend_from_slice(&garbage);
    let read = extract(&file(dir.path(), "broken.pdf", &broken_pdf)).unwrap();
    assert_eq!(read.kind, Kind::Document);
    assert!(read.note.is_some());

    assert!(extract(dir.path()).is_err(), "a folder is not a file");
    assert!(extract(&dir.path().join("missing.txt")).is_err());
}

#[test]
fn long_text_is_cut_with_a_note() {
    let dir = tempfile::tempdir().unwrap();
    let text = "word ".repeat(MAX_TEXT_CHARS / 5 + 10);
    let read = extract(&file(dir.path(), "long.txt", text.as_bytes())).unwrap();
    assert_eq!(
        read.text.unwrap().chars().count(),
        MAX_TEXT_CHARS - 1,
        "the cut drops the trailing space"
    );
    assert!(read.note.unwrap().contains("4,000,000"));
}
