//! The text of a PDF that `read_page` fetched (DECISIONS N15).
//!
//! A PDF from the internet is someone else's file, and reading one is work that can go wrong in ways a page
//! cannot: a malformed file can make the reader panic, a small file can unpack into a very large one, and a
//! strange one can take a long time. So in Demido the text is taken out in a process of its own (the same
//! program, `demido pdf-text`), given a few seconds and, on Linux, a limit on memory; when it misbehaves, that
//! process ends and the core goes on. Tests take it out in this process, with the same limits on what is read.

use std::time::Duration;

/// How long taking the text out may take.
const MOST_SECONDS: u64 = 20;
/// The most memory the process that takes it out may use (Linux).
#[cfg(target_os = "linux")]
const MOST_MEMORY: u64 = 1 << 30;
/// The most pages read; a longer PDF says how many more it has.
pub const MOST_PAGES: usize = 300;

/// Where the text is taken out.
#[derive(Clone, Debug, Default)]
pub enum Reader {
    /// In this process: for tests.
    #[default]
    Here,
    /// In a process of its own: this program, run as `<program> pdf-text`.
    Apart(std::path::PathBuf),
}

/// The text of a PDF, page by page, with a line between pages: what `demido pdf-text` prints. A panic in the
/// reader is an error, not a crash.
pub fn text_of(bytes: &[u8]) -> Result<String, String> {
    let pages = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(bytes))
        .map_err(|_| "the PDF could not be read: it is malformed".to_string())?
        .map_err(|e| format!("the PDF could not be read: {e}"))?;
    let count = pages.len();
    let mut out = String::new();
    for (i, page) in pages.into_iter().take(MOST_PAGES).enumerate() {
        let text = tidy(&page);
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("— page {} —\n{text}", i + 1));
    }
    if count > MOST_PAGES {
        out.push_str(&format!("\n\n(The PDF has {count} pages; the first {MOST_PAGES} were read.)"));
    }
    if out.trim().is_empty() {
        return Err("the PDF has no text to read: it may be pictures of pages (scanned), which Demido does not read".into());
    }
    Ok(out)
}

/// A page's text with its runs of blank lines and spaces made short.
fn tidy(page: &str) -> String {
    let mut out = String::new();
    let mut blank = 0;
    for line in page.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")) {
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(&line);
    }
    out
}

/// The text of a PDF, taken out where `reader` says, within the limits.
pub async fn read(bytes: Vec<u8>, reader: &Reader) -> Result<String, String> {
    let too_long = || format!("the PDF took more than {MOST_SECONDS} seconds to read, and was left");
    match reader {
        Reader::Here => {
            let work = tokio::task::spawn_blocking(move || text_of(&bytes));
            match tokio::time::timeout(Duration::from_secs(MOST_SECONDS), work).await {
                Ok(Ok(read)) => read,
                Ok(Err(_)) => Err("the PDF could not be read".into()),
                Err(_) => Err(too_long()),
            }
        }
        Reader::Apart(program) => {
            use tokio::io::AsyncWriteExt;
            let mut command = tokio::process::Command::new(program);
            command.arg("pdf-text").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
            #[cfg(target_os = "linux")]
            // SAFETY: only setrlimit, which is safe to call between fork and exec. A reader that runs out of
            // memory ends at once (tried: a PDF that unpacks to 1.5 GB), and leaves no core dump behind.
            unsafe {
                command.pre_exec(|| {
                    let memory = libc::rlimit { rlim_cur: MOST_MEMORY, rlim_max: MOST_MEMORY };
                    let no_dump = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
                    libc::setrlimit(libc::RLIMIT_AS, &memory);
                    libc::setrlimit(libc::RLIMIT_CORE, &no_dump);
                    Ok(())
                });
            }
            let mut child = command.spawn().map_err(|e| format!("the PDF reader could not be started: {e}"))?;
            let mut stdin = child.stdin.take().expect("piped");
            let feed = async move {
                let _ = stdin.write_all(&bytes).await;
            };
            let run = async {
                feed.await;
                child.wait_with_output().await
            };
            match tokio::time::timeout(Duration::from_secs(MOST_SECONDS), run).await {
                Err(_) => Err(too_long()),
                Ok(Err(e)) => Err(format!("the PDF reader failed: {e}")),
                Ok(Ok(out)) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
                Ok(Ok(out)) => {
                    let said = String::from_utf8_lossy(&out.stderr);
                    let said = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped").trim().to_string();
                    Err(if said.starts_with("the PDF") { said } else { "the PDF could not be read: the reader stopped on it".into() })
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A one-page PDF that says "Hello from a PDF", written the way a PDF is: objects, a table of where each
    /// starts, and a trailer.
    pub(crate) fn hello() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 144] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            {
                let stream = "BT /F1 18 Tf 36 72 Td (Hello from a PDF) Tj ET";
                format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len())
            },
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut starts = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            starts.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let table = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for start in starts {
            out.extend_from_slice(format!("{start:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{table}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }

    #[test]
    fn a_pdfs_text_is_read_page_by_page_and_a_broken_one_is_an_error() {
        let read = text_of(&hello()).unwrap();
        assert!(read.starts_with("— page 1 —\n") && read.contains("Hello from a PDF"), "{read:?}");
        assert!(text_of(b"%PDF-1.4\nnot a real PDF at all").is_err());
        assert!(text_of(b"").is_err());
        assert_eq!(tidy("  one   two \n\n\n three\n"), "one two\n\nthree");
    }

    #[tokio::test]
    async fn here_it_is_read_within_the_limit() {
        assert!(read(hello(), &Reader::Here).await.unwrap().contains("Hello from a PDF"));
        assert!(read(b"%PDF-1.7 broken".to_vec(), &Reader::Here).await.is_err());
    }
}
