//! Reading attached files: what a file is, the text a model can read from it, passages of that
//! text for retrieval, and a model-ready copy of an image.
//!
//! [`extract`] tells what a file is from its first bytes (then its extension) and reads the text
//! out of documents, spreadsheets and text files. Paged formats mark every page, slide or sheet on
//! a line of its own, and [`Extracted::page_starts`] says where each marker is, so [`chunks`] can
//! tell which page a passage comes from. [`model_image`] turns a photo or screenshot into a PNG or
//! JPEG that local and cloud models can read.
//!
//! Attachments come from anywhere, so every third-party parser runs behind `catch_unwind`: a
//! damaged or hostile file becomes a note for the person, never a crash. Some failures cannot be
//! caught inside a process at all (a stack overflow, an allocation the system refuses, a parser
//! that never finishes), so an application can have every file read in a child process instead
//! ([`isolate`], [`child_main`]).

use std::ffi::OsString;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

mod chunk;
mod detect;
mod epub;
mod html;
mod images;
mod odf;
mod ooxml;
mod out;
mod pdf;
mod rtf;
mod sheet;
mod text;
mod xml;

pub use chunk::{Chunk, chunks};
pub use images::{ModelImage, model_image};

use detect::Format;
use out::TextOut;

/// What kind of file an attachment is, which decides how a model receives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// png, jpeg, gif, webp, bmp, tiff, heic...
    Image,
    /// wav, mp3, flac, ogg, m4a, aac...
    Audio,
    /// pdf, docx, odt, pptx, odp, html, epub, rtf: prose with a layout
    Document,
    /// plain text, markdown, code, logs, config
    Text,
    /// csv, tsv, json, jsonl, xml, xlsx, xls, ods: tables and records
    Data,
    /// anything else (archives, executables, video): the model is told the file exists
    Other,
}

/// What [`extract`] found in a file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Extracted {
    pub kind: Kind,
    /// Detected from the content first (magic bytes), then the extension.
    pub mime: String,
    /// The text a model reads. None for images, audio, and files with no readable text.
    /// Paged formats mark each page on a line of its own: `--- Page 3 ---` (PDF),
    /// `--- Slide 3 ---` (presentations), `--- Sheet "Sales" (1,204 rows × 8 columns) ---`
    /// (spreadsheets, each sheet as CSV rows below its marker).
    pub text: Option<String>,
    /// PDFs: pages; presentations: slides; spreadsheets: sheets. None otherwise.
    pub pages: Option<u32>,
    /// Byte offsets in `text` where page (slide, sheet) 1, 2, 3... starts (its marker line).
    /// Empty when the format has no pages. Shorter than `pages` when the text was cut at
    /// [`MAX_TEXT_CHARS`] before the last pages.
    pub page_starts: Vec<usize>,
    /// What the person should know, shown on the file's chip. For example
    /// "No text found. It may be a scanned PDF." or "Only the first 4,000,000 characters are read."
    pub note: Option<String>,
    /// Pixel size of an image, as it is shown (EXIF rotation applied).
    pub dimensions: Option<(u32, u32)>,
}

/// The most text read from one file. A longer text is cut here, with a note.
pub const MAX_TEXT_CHARS: usize = 4_000_000;

/// Reads `path`. Never panics: parser panics are caught and become an Err or a note.
/// Err only when the file cannot be read at all (missing, permission denied, a folder).
///
/// After [`isolate`], the file is read in a child process; otherwise the whole file is read into
/// memory and parsed on the calling thread (a PDF on a helper thread with a larger stack). Either
/// way call it from a blocking task: a long PDF or a big spreadsheet takes seconds.
pub fn extract(path: &Path) -> Result<Extracted, String> {
    match ISOLATION.get() {
        Some(isolation) => isolation.extract(path),
        None => extract_here(path),
    }
}

/// Reads `path` in this process.
fn extract_here(path: &Path) -> Result<Extracted, String> {
    let meta = std::fs::metadata(path).map_err(|e| read_error(&e))?;
    if meta.is_dir() {
        return Err("This is a folder, not a file.".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|e| read_error(&e))?;
    Ok(extract_bytes(&bytes, &extension(path)))
}

/// The argument an application can start itself with to be the child of [`isolate`]: it is
/// followed by the file's path, and the application then only calls [`child_main`].
pub const CHILD_ARG: &str = "--demido-extract";

/// Prefix of the line a child writes its answer on: anything a parser prints is ignored.
const CHILD_ANSWER: &str = "DEMIDO-EXTRACT:";

/// How files are read in a child process.
struct Isolation {
    program: PathBuf,
    args: Vec<OsString>,
    timeout: Duration,
    on_spawn: Option<fn(&std::process::Child)>,
}

static ISOLATION: OnceLock<Isolation> = OnceLock::new();

/// From now on [`extract`] reads every file in a child process: `program` started with `args`
/// and then the file's path, which must call [`child_main`] with that path. A child that dies
/// or runs past `timeout` costs only that file its text: the file is reported with a note.
/// `on_spawn` sees each child as it starts (to tie it to the application's lifetime). Only the
/// first call counts.
pub fn isolate(program: PathBuf, args: Vec<OsString>, timeout: Duration, on_spawn: Option<fn(&std::process::Child)>) {
    let _ = ISOLATION.set(Isolation {
        program,
        args,
        timeout,
        on_spawn,
    });
}

/// The child's side of [`isolate`]: reads `path` here and writes the result to stdout. Returns
/// the process's exit code.
pub fn child_main(path: &Path) -> i32 {
    let result = extract_here(path);
    match serde_json::to_string(&result) {
        Ok(json) => {
            println!("{CHILD_ANSWER}{json}");
            0
        }
        Err(_) => 1,
    }
}

impl Isolation {
    fn extract(&self, path: &Path) -> Result<Extracted, String> {
        let meta = std::fs::metadata(path).map_err(|e| read_error(&e))?;
        if meta.is_dir() {
            return Err("This is a folder, not a file.".to_owned());
        }
        match self.run(path) {
            Ok(result) => result,
            Err(why) => {
                // What the file is, from its start and its name, since the child cannot say.
                let mut head = Vec::new();
                let _ = std::fs::File::open(path).and_then(|f| f.take(64 * 1024).read_to_end(&mut head));
                let found = detect::detect(&head, &extension(path));
                Ok(Extracted::new(found.kind, found.mime).with_note(why))
            }
        }
    }

    /// Runs the child; Err with a sentence for the person when it fails or takes too long.
    fn run(&self, path: &Path) -> Result<Result<Extracted, String>, &'static str> {
        const DAMAGED: &str = "This file could not be read. It may be damaged.";
        let mut command = std::process::Command::new(&self.program);
        command
            .args(&self.args)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|_| DAMAGED)?;
        if let Some(on_spawn) = self.on_spawn {
            on_spawn(&child);
        }
        // Read the answer on another thread, so a large one never fills the pipe and stalls the
        // child, while this one keeps the time.
        let mut stdout = child.stdout.take().ok_or(DAMAGED)?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            let _ = stdout.read_to_end(&mut out);
            let _ = tx.send(out);
        });
        let Ok(out) = rx.recv_timeout(self.timeout) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Reading this file took too long. It may be damaged.");
        };
        let _ = child.wait();
        let out = String::from_utf8_lossy(&out);
        let answer = out
            .lines()
            .rev()
            .find_map(|line| line.strip_prefix(CHILD_ANSWER))
            .ok_or(DAMAGED)?;
        serde_json::from_str(answer).map_err(|_| DAMAGED)
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Detects and reads a file's content; `ext` is its lowercase extension, or empty.
fn extract_bytes(bytes: &[u8], ext: &str) -> Extracted {
    let found = detect::detect(bytes, ext);
    let (kind, mime) = (found.kind, found.mime.clone());
    guarded(|| read(bytes, found))
        .unwrap_or_else(|| Extracted::new(kind, mime).with_note("This file could not be read. It may be damaged."))
}

fn read(bytes: &[u8], found: detect::Detected) -> Extracted {
    xml::reset_budget();
    let base = Extracted::new(found.kind, found.mime);
    let read = match found.format {
        Format::Text => text_file(base, bytes),
        Format::Html => {
            let mut out = TextOut::new();
            out.push(&html::to_text(&text::decode(bytes)));
            base.with_text(out)
        }
        Format::Pdf => pdf::read(base, bytes),
        Format::Docx => ooxml::docx(base, bytes),
        Format::Pptx => ooxml::pptx(base, bytes),
        Format::Spreadsheet => sheet::read(base, bytes),
        Format::Odt => odf::read(base, bytes, false),
        Format::Odp => odf::read(base, bytes, true),
        Format::Epub => epub::read(base, bytes),
        Format::Rtf => {
            let mut out = TextOut::new();
            out.push(&rtf::to_text(bytes));
            base.with_text(out)
        }
        Format::Image => images::describe(base, bytes),
        Format::Audio | Format::Other => base,
    };
    match found.note {
        Some(note) => read.with_note(note),
        None => read,
    }
}

fn text_file(base: Extracted, bytes: &[u8]) -> Extracted {
    if bytes.is_empty() {
        return base.with_note("The file is empty.");
    }
    let mut out = TextOut::new();
    out.push(&text::decode(bytes));
    base.with_text(out)
}

impl Extracted {
    fn new(kind: Kind, mime: impl Into<String>) -> Self {
        Self {
            kind,
            mime: mime.into(),
            text: None,
            pages: None,
            page_starts: Vec::new(),
            note: None,
            dimensions: None,
        }
    }

    /// Adds a sentence to the note, after any already there.
    fn with_note(mut self, note: impl AsRef<str>) -> Self {
        let note = note.as_ref();
        self.note = Some(match self.note.take() {
            Some(existing) => format!("{existing} {note}"),
            None => note.to_owned(),
        });
        self
    }

    /// Takes the text (and page markers) that was read, with a note when it was cut short.
    fn with_text(mut self, out: TextOut) -> Self {
        let done = out.finish();
        self.text = done.text;
        self.page_starts = if self.text.is_some() {
            done.page_starts
        } else {
            Vec::new()
        };
        if done.truncated {
            self = self.with_note(format!("Only the first {} characters are read.", group(MAX_TEXT_CHARS)));
        }
        self
    }
}

/// Runs a parser, turning a panic into `None`.
///
/// The default panic hook still prints the panic message to stderr; this crate leaves the hook
/// alone because it belongs to the application.
fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

/// A sentence for the person about a file that could not be opened.
fn read_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "The file does not exist any more.".to_owned(),
        std::io::ErrorKind::PermissionDenied => "Demido Studio is not allowed to read this file.".to_owned(),
        _ => format!("The file could not be read ({e})."),
    }
}

/// `1204` as `1,204`.
fn group(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "1 page", "3 pages".
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", group(n), if n == 1 { one } else { many })
}

#[cfg(test)]
mod isolation_tests {
    use super::*;

    /// Not a test of its own: the isolation tests start this test binary as the child, running
    /// only this function, with the file's path as the last argument.
    #[test]
    #[ignore = "run as a child process by the isolation tests"]
    fn child_entry() {
        let path = PathBuf::from(std::env::args().next_back().unwrap());
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("crash") {
            std::process::abort();
        }
        if name.starts_with("hang") {
            std::thread::sleep(Duration::from_secs(30));
        }
        // Noise a parser might print does not get in the way of the answer.
        println!("some library chatter");
        std::process::exit(child_main(&path));
    }

    fn isolation(timeout: Duration) -> Isolation {
        Isolation {
            program: std::env::current_exe().unwrap(),
            args: [
                "--exact",
                "isolation_tests::child_entry",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ]
            .map(OsString::from)
            .into(),
            timeout,
            on_spawn: None,
        }
    }

    #[test]
    fn a_child_reads_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, "# Plan\nShip it.").unwrap();
        let read = isolation(Duration::from_secs(60)).extract(&path).unwrap();
        assert_eq!(read, extract_here(&path).unwrap());
        assert_eq!(read.text.as_deref(), Some("# Plan\nShip it."));
    }

    #[test]
    fn a_child_that_dies_or_hangs_costs_only_the_text() {
        let dir = tempfile::tempdir().unwrap();
        let crash = dir.path().join("crash.pdf");
        std::fs::write(&crash, b"%PDF-1.7 anything").unwrap();
        let read = isolation(Duration::from_secs(60)).extract(&crash).unwrap();
        assert_eq!((read.kind, read.mime.as_str()), (Kind::Document, "application/pdf"));
        assert_eq!(read.text, None);
        assert!(read.note.unwrap().contains("could not be read"));

        let hang = dir.path().join("hang.txt");
        std::fs::write(&hang, "words").unwrap();
        let started = std::time::Instant::now();
        let read = isolation(Duration::from_secs(3)).extract(&hang).unwrap();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(read.note.unwrap().contains("took too long"));

        assert!(
            isolation(Duration::from_secs(5)).extract(dir.path()).is_err(),
            "a folder"
        );
    }
}
