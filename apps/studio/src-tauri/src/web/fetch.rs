//! Reading a web page, or a document on the web, as text.
//!
//! Plain HTTP first: an HTML page is boiled down to its article (or its whole text, see
//! [`page::readable`]) as Markdown, a text file is read as it is, and a PDF or Office file is read
//! like an attached one ([`demido_extract`]). A page that refuses a program, or is empty until
//! its scripts run, climbs on:
//!
//! 1. the chat's tab in the built-in browser, which runs the page's scripts, waits out checks
//!    that pass by themselves (Cloudflare's "just a moment") and reads what it shows;
//! 2. Exa's free crawler, when the browser too is stopped at a check (if allowed in Settings);
//! 3. the person, who is shown the tab with the check to solve, after which the browser reads on.
//!
//! Pages are kept a while ([`super::KEEP`]), so reading on through a long one does not fetch it
//! again.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::browser::Answer;
use super::page::{self, MAX_HTML_BYTES};
use super::{Web, search};

/// Documents (PDFs, Office files) are read up to this size.
const MAX_DOCUMENT_BYTES: usize = 30 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long the browser waits for a check that passes by itself.
const OUTWAIT: Duration = Duration::from_secs(10);
/// How long the person has to solve a check.
const ASK_FOR: Duration = Duration::from_secs(180);
/// What Exa is asked for at most, in characters.
const EXA_CHARS: usize = 60_000;

/// How a page was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    Http,
    Browser,
    Exa,
}

impl Via {
    pub fn label(self) -> &'static str {
        match self {
            Via::Http => "read directly",
            Via::Browser => "read in the built-in browser",
            Via::Exa => "read through Exa",
        }
    }
}

/// A page read as text.
#[derive(Clone, Debug, Serialize)]
pub struct Fetched {
    /// Where it ended up, after redirects.
    pub url: String,
    pub title: String,
    pub text: String,
    pub site: Option<String>,
    pub byline: Option<String>,
    pub published: Option<String>,
    /// `page`, `document` (PDF, Office) or `text`.
    pub kind: &'static str,
    pub via: Via,
    /// What the reader should know: a page that shows little, a document cut short.
    pub note: Option<String>,
}

/// What a plain request got.
enum Got {
    Read(Fetched),
    /// The site refused a program, or the page needs its scripts: worth the browser. `partial`
    /// is what it did show.
    Refused {
        why: String,
        partial: Option<Fetched>,
    },
}

/// Reads `url`. `chat` lends its browser tab when plain HTTP is refused; `progress` says what is
/// going on, for the call's card.
pub async fn fetch(
    web: &Web,
    url: &Url,
    chat: Option<&str>,
    cancel: &CancellationToken,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Arc<Fetched>, String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "Only web pages (http and https) can be read, not {}: links.",
            url.scheme()
        ));
    }
    super::browser::allowed(url)?;
    let key = key(url);
    if let Some(kept) = web.pages.lock().get(&key) {
        return Ok(kept);
    }
    let read = read(web, url, chat, cancel, progress).await?;
    let read = Arc::new(read);
    web.pages.lock().put(key, read.clone());
    if read.url != url.as_str() {
        web.pages.lock().put(read.url.clone(), read.clone());
    }
    Ok(read)
}

/// The page kept for `url` from an earlier read, if any.
pub fn kept(web: &Web, url: &Url) -> Option<Arc<Fetched>> {
    web.pages.lock().get(&key(url))
}

/// A page's key: its address without the fragment, which names a place in the same page.
fn key(url: &Url) -> String {
    let mut url = url.clone();
    url.set_fragment(None);
    url.to_string()
}

async fn read(
    web: &Web,
    url: &Url,
    chat: Option<&str>,
    cancel: &CancellationToken,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Fetched, String> {
    let (why, partial) = match http(&web.client, &web.cache_dir, url).await? {
        Got::Read(f) => return Ok(f),
        Got::Refused { why, partial } => (why, partial),
    };
    tracing::info!(%url, %why, "plain request refused; trying the browser");
    let Some(chat) = chat else {
        return partial.ok_or(why);
    };
    if cancel.is_cancelled() {
        return Err("Stopped.".into());
    }
    match in_browser(web, url, chat, cancel, progress).await {
        Ok(f) => Ok(f),
        Err(e) => match partial {
            Some(mut p) => {
                p.note = Some(format!("{why}; the browser could not do better: {e}"));
                Ok(p)
            }
            None => Err(format!("{why}. {e}")),
        },
    }
}

// ---- Plain HTTP ------------------------------------------------------------------------

async fn http(client: &reqwest::Client, cache_dir: &Path, url: &Url) -> Result<Got, String> {
    let response = match client.get(url.clone()).timeout(TIMEOUT).send().await {
        Ok(r) => r,
        // A site that drops programs mid-handshake, or keeps them waiting, may still serve a browser.
        Err(e) if e.is_timeout() => {
            return Ok(Got::Refused {
                why: "The site did not answer in time".into(),
                partial: None,
            });
        }
        Err(e) => return Err(unreachable(url, &e)),
    };
    let status = response.status();
    let final_url = response.url().clone();
    let headers = response.headers().clone();
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    let content_type = header("content-type");
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    let server = header("server");
    let mitigated = headers.contains_key("cf-mitigated");
    let kind = kind_of(&mime, &final_url);
    if let Kind::Media(what) = kind {
        return Err(format!(
            "{final_url} is {what} ({mime}), not a page; web_fetch reads text only."
        ));
    }
    let limit = if kind == Kind::Document {
        MAX_DOCUMENT_BYTES
    } else {
        MAX_HTML_BYTES
    };
    let (bytes, cut) = body(response, limit).await?;

    let refused_status =
        matches!(status.as_u16(), 401 | 403 | 429 | 503) || (status.as_u16() == 202 && bytes.is_empty());
    if !status.is_success() && !refused_status {
        let reason = status.canonical_reason().unwrap_or("an error");
        return Err(format!("{final_url} answered {} {reason}.", status.as_u16()));
    }
    // What is sent as text/plain or octet-stream may still be a document or a page.
    let kind = match kind {
        Kind::Unknown => sniff(&bytes),
        k => k,
    };
    match kind {
        Kind::Html => {
            let html = decode(&bytes, &content_type, true);
            if let Some(check) = page::check(&page::Response {
                status: status.as_u16(),
                server: &server,
                mitigated,
                html: &html,
            }) {
                return Ok(Got::Refused {
                    why: format!("The site asked for {}", check.label()),
                    partial: None,
                });
            }
            let page = page::readable(final_url.as_str(), &html);
            let shell = page.shell;
            let fetched = from_page(page, cut);
            // An error page is no stand-in for the page, however much it says.
            if refused_status {
                return Ok(Got::Refused {
                    why: format!("The site answered {}", status.as_u16()),
                    partial: None,
                });
            }
            if shell {
                return Ok(Got::Refused {
                    why: "The page shows next to nothing until its scripts run".into(),
                    partial: Some(fetched),
                });
            }
            Ok(Got::Read(fetched))
        }
        _ if refused_status => Ok(Got::Refused {
            why: format!("The site answered {}", status.as_u16()),
            partial: None,
        }),
        Kind::Document => Ok(Got::Read(document(cache_dir, &final_url, &mime, &bytes).await?)),
        Kind::Text | Kind::Unknown => {
            let text = decode(&bytes, &content_type, false);
            if text.contains('\u{FFFD}') && text.matches('\u{FFFD}').count() > text.len() / 50 {
                return Err(format!("{final_url} is not text ({mime}); web_fetch reads text only."));
            }
            Ok(Got::Read(Fetched {
                title: final_url
                    .path_segments()
                    .and_then(|mut s| s.next_back())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| final_url.host_str().unwrap_or_default())
                    .to_string(),
                url: final_url.to_string(),
                text: pretty(&mime, text),
                site: final_url.host_str().map(str::to_string),
                byline: None,
                published: None,
                kind: "text",
                via: Via::Http,
                note: cut.then(|| "Only the start of the file was read; it is larger.".to_string()),
            }))
        }
        Kind::Media(_) => unreachable!("refused above"),
    }
}

fn unreachable(url: &Url, e: &reqwest::Error) -> String {
    let host = url.host_str().unwrap_or("the site");
    if e.is_connect() {
        let detail = std::error::Error::source(e).map(|s| s.to_string()).unwrap_or_default();
        if detail.contains("dns") || detail.contains("resolve") || detail.contains("lookup") {
            return format!("{host} was not found (no such site, or no connection to the internet).");
        }
        return format!("Could not connect to {host}.");
    }
    if e.is_redirect() {
        return format!("{url} redirects too many times.");
    }
    format!("Could not read {url}: {e}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Html,
    Text,
    /// PDF, Office, EPUB: read as attached files are.
    Document,
    /// An image, a sound, a video, a font: not for reading.
    Media(&'static str),
    Unknown,
}

/// Whether something of type `mime` at `url` is a document (a PDF, an office file), whose text is
/// read from its file. A browser shows a PDF in a viewer of its own, with no text in the page.
pub fn is_document(mime: &str, url: &Url) -> bool {
    kind_of(&mime.to_ascii_lowercase(), url) == Kind::Document
}

fn kind_of(mime: &str, url: &Url) -> Kind {
    let ext = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match mime {
        "text/html" | "application/xhtml+xml" => Kind::Html,
        "application/pdf" | "application/rtf" | "application/epub+zip" | "application/msword" => Kind::Document,
        m if m.starts_with("application/vnd.openxmlformats-officedocument")
            || m.starts_with("application/vnd.oasis.opendocument")
            || m.starts_with("application/vnd.ms-") =>
        {
            Kind::Document
        }
        "image/svg+xml" => Kind::Text,
        m if m.starts_with("image/") => Kind::Media("an image"),
        m if m.starts_with("audio/") => Kind::Media("audio"),
        m if m.starts_with("video/") => Kind::Media("a video"),
        m if m.starts_with("font/") => Kind::Media("a font"),
        m if m.starts_with("text/")
            || m.ends_with("+json")
            || m.ends_with("+xml")
            || matches!(
                m,
                "application/json"
                    | "application/xml"
                    | "application/javascript"
                    | "application/x-javascript"
                    | "application/yaml"
                    | "application/x-yaml"
                    | "application/toml"
                    | "application/x-ndjson"
                    | "application/sql"
            ) =>
        {
            Kind::Text
        }
        _ if DOCUMENT_EXTENSIONS.contains(&ext.as_str()) => Kind::Document,
        "application/zip"
        | "application/gzip"
        | "application/x-tar"
        | "application/x-7z-compressed"
        | "application/vnd.rar"
        | "application/x-msdownload"
        | "application/wasm" => Kind::Media("an archive or a program"),
        _ => Kind::Unknown,
    }
}

const DOCUMENT_EXTENSIONS: [&str; 13] = [
    "pdf", "docx", "doc", "pptx", "ppt", "xlsx", "xls", "odt", "odp", "ods", "epub", "rtf", "xlsm",
];

/// What a body sent without a useful type is, from its first bytes.
fn sniff(bytes: &[u8]) -> Kind {
    let start = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]).to_ascii_lowercase();
    let start = start.trim_start_matches('\u{feff}').trim_start();
    if bytes.starts_with(b"%PDF") || bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        Kind::Document
    } else if start.starts_with("<!doctype html") || start.starts_with("<html") || start.starts_with("<head") {
        Kind::Html
    } else {
        Kind::Text
    }
}

/// Reads a body, `limit` bytes at most. Returns whether it was cut there.
async fn body(response: reqwest::Response, limit: usize) -> Result<(Vec<u8>, bool), String> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("The page stopped loading: {e}"))?;
        let room = limit - bytes.len();
        if chunk.len() >= room {
            bytes.extend_from_slice(&chunk[..room]);
            return Ok((bytes, true));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok((bytes, false))
}

/// Text in the character set the server or the page names, UTF-8 when neither does.
fn decode(bytes: &[u8], content_type: &str, html: bool) -> String {
    let named = content_type
        .split(';')
        .filter_map(|p| p.trim().strip_prefix("charset="))
        .next()
        .map(|c| c.trim_matches(['"', '\'']).to_string())
        .or_else(|| html.then(|| meta_charset(bytes)).flatten());
    let encoding = named
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

/// The character set a page names in its head (`<meta charset>`).
fn meta_charset(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    let at = head.find("charset=")? + "charset=".len();
    let value: String = head[at..]
        .trim_start_matches(['"', '\''])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    (!value.is_empty()).then_some(value)
}

/// JSON laid out to be read; anything else as it came.
fn pretty(mime: &str, text: String) -> String {
    if (mime == "application/json" || mime.ends_with("+json"))
        && text.len() < 2_000_000
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        && let Ok(laid_out) = serde_json::to_string_pretty(&value)
    {
        return laid_out;
    }
    text
}

fn from_page(p: page::Page, cut: bool) -> Fetched {
    let mut note = p.shell.then(|| "The page shows little text.".to_string());
    if cut {
        note = Some("Only the start of the page was read; it is very large.".into());
    }
    Fetched {
        url: p.url,
        title: p.title,
        text: p.text,
        site: p.site,
        byline: p.byline,
        published: p.published,
        kind: "page",
        via: Via::Http,
        note,
    }
}

/// A PDF or Office file, saved where the app keeps its cache and read as an attached file is.
async fn document(cache_dir: &Path, url: &Url, mime: &str, bytes: &[u8]) -> Result<Fetched, String> {
    use sha2::Digest;
    let ext = match mime {
        "application/pdf" => "pdf",
        "application/rtf" => "rtf",
        "application/epub+zip" => "epub",
        "application/msword" => "doc",
        m if m.ends_with("wordprocessingml.document") => "docx",
        m if m.ends_with("presentationml.presentation") => "pptx",
        m if m.ends_with("spreadsheetml.sheet") => "xlsx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.oasis.opendocument.text" => "odt",
        "application/vnd.oasis.opendocument.presentation" => "odp",
        "application/vnd.oasis.opendocument.spreadsheet" => "ods",
        _ => url
            .path_segments()
            .and_then(|mut s| s.next_back())
            .and_then(|n| n.rsplit_once('.'))
            .map(|(_, e)| e)
            .filter(|e| DOCUMENT_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or("pdf"),
    };
    let dir = cache_dir.to_path_buf();
    let name = format!("{:x}.{ext}", sha2::Sha256::digest(url.as_str().as_bytes()));
    let path = dir.join(name);
    let owned = bytes.to_vec();
    let extracted = tokio::task::spawn_blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(&path, owned).map_err(|e| e.to_string())?;
        let read = demido_extract::extract(&path);
        let _ = std::fs::remove_file(&path);
        read
    })
    .await
    .map_err(|e| e.to_string())??;
    let text = extracted.text.unwrap_or_default();
    if text.trim().is_empty() {
        return Err(extracted
            .note
            .unwrap_or_else(|| format!("No text could be read from {url}.")));
    }
    let title = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .map(|s| percent_encoding::percent_decode_str(s).decode_utf8_lossy().into_owned())
        .unwrap_or_else(|| url.to_string());
    let mut note = extracted.note;
    if bytes.len() >= MAX_DOCUMENT_BYTES {
        note = Some("The file is larger than 30 MB; only its start was read.".into());
    }
    if let Some(pages) = extracted.pages {
        let pages = format!("{pages} pages, each marked \"--- Page N ---\".");
        note = Some(match note {
            Some(n) => format!("{pages} {n}"),
            None => pages,
        });
    }
    Ok(Fetched {
        url: url.to_string(),
        title,
        text,
        site: url.host_str().map(str::to_string),
        byline: None,
        published: None,
        kind: "document",
        via: Via::Http,
        note,
    })
}

// ---- The browser, Exa, the person -------------------------------------------------------

async fn in_browser(
    web: &Web,
    url: &Url,
    chat: &str,
    cancel: &CancellationToken,
    progress: &(dyn Fn(&str) + Send + Sync),
) -> Result<Fetched, String> {
    let b = &web.browser;
    let tab = b.chat_tab_or_open(chat).await?;
    let _lease = b.lease(tab).await?;
    progress("Opening the page in the built-in browser");
    b.load(tab, url, TIMEOUT).await?;
    if let Some(check) = b.outwait(tab, OUTWAIT, cancel).await {
        tracing::info!(%url, check = check.label(), "the browser was stopped at a check");
        // Exa's crawler cannot reach the person's own network, and is not told its addresses.
        if web.settings.get().web_exa && !super::on_this_network(url) {
            progress("Asking Exa for the page");
            match exa(&web.client, url).await {
                Ok(f) => return Ok(f),
                Err(e) => tracing::info!(%url, "Exa could not read the page: {e}"),
            }
        }
        progress("Waiting for you to solve a check in the Browser window");
        let host = url.host_str().unwrap_or("A site");
        let message = format!(
            "{host} wants to check that a person is here. Solve the check, and the assistant reads on by itself."
        );
        match b.ask(tab, chat, &message, ASK_FOR, true, cancel).await? {
            Answer::Cleared | Answer::Done => b.settle(tab, Duration::from_secs(6)).await,
            Answer::Skipped => return Err(format!("The page asks for {}, and the user skipped it.", check.label())),
            Answer::TimedOut => {
                return Err(format!(
                    "The page asks for {}, which nobody solved in time.",
                    check.label()
                ));
            }
        }
        if let Some(check) = b.state(tab).await?.check {
            return Err(format!("The page still asks for {}.", check.label()));
        }
    }
    let rendered = b.html(tab).await?;
    let shown = Url::parse(&rendered.url).unwrap_or_else(|_| url.clone());
    if is_document(&rendered.kind, &shown) {
        progress("Reading the document");
        let (kind, bytes) = b.file(tab, MAX_DOCUMENT_BYTES).await?;
        let kind = if kind.is_empty() {
            rendered.kind.to_ascii_lowercase()
        } else {
            kind
        };
        let mut fetched = document(&web.cache_dir, &shown, &kind, &bytes).await?;
        fetched.via = Via::Browser;
        return Ok(fetched);
    }
    let mut page = page::readable(&rendered.url, &rendered.html);
    if page.title.is_empty() {
        page.title = rendered.title;
    }
    let mut fetched = from_page(page, false);
    fetched.via = Via::Browser;
    Ok(fetched)
}

/// The page as Exa's crawler read it.
async fn exa(client: &reqwest::Client, url: &Url) -> Result<Fetched, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "web_fetch_exa",
            "arguments": {"urls": [url.as_str()], "maxCharacters": EXA_CHARS},
        },
    });
    let text = search::exa_call(client, &body).await.map_err(|e| format!("Exa {e}"))?;
    let read = exa_page(&text).ok_or("Exa answered without the page")?;
    Ok(Fetched {
        url: read.url.unwrap_or_else(|| url.to_string()),
        title: read.title.unwrap_or_default(),
        text: page::tidy(&read.text),
        site: url.host_str().map(str::to_string),
        byline: read.author,
        published: read.published,
        kind: "page",
        via: Via::Exa,
        note: None,
    })
}

struct ExaPage {
    title: Option<String>,
    url: Option<String>,
    published: Option<String>,
    author: Option<String>,
    text: String,
}

/// Exa's page: a few `Name: value` lines (or a `# Title` heading over the `URL:` line), then the
/// text (after `Text:` when it says so).
fn exa_page(answer: &str) -> Option<ExaPage> {
    let mut page = ExaPage {
        title: None,
        url: None,
        published: None,
        author: None,
        text: String::new(),
    };
    let mut lines = answer.lines().peekable();
    let value = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty() && v != "N/A");
    if let Some(title) = answer.lines().next().and_then(|l| l.trim().strip_prefix("# "))
        && answer
            .lines()
            .nth(1)
            .is_some_and(|l| l.trim_start().starts_with("URL:"))
    {
        page.title = value(title);
        lines.next();
    }
    while let Some(line) = lines.peek() {
        let line = line.trim_start();
        if let Some(v) = line.strip_prefix("Title:") {
            page.title = value(v);
        } else if let Some(v) = line.strip_prefix("URL:") {
            page.url = value(v);
        } else if let Some(v) = line
            .strip_prefix("Published:")
            .or_else(|| line.strip_prefix("Published Date:"))
        {
            page.published = value(v).map(|d| d.chars().take(10).collect());
        } else if let Some(v) = line.strip_prefix("Author:") {
            page.author = value(v);
        } else if let Some(v) = line.strip_prefix("Text:") {
            lines.next();
            page.text = std::iter::once(v.trim_start())
                .chain(lines)
                .collect::<Vec<_>>()
                .join("\n");
            break;
        } else if !["Highlights:", "ID:", "Image:", "Favicon:"]
            .iter()
            .any(|k| line.starts_with(k))
        {
            page.text = lines.collect::<Vec<_>>().join("\n");
            break;
        }
        lines.next();
    }
    let text = page.text.trim();
    (text.chars().count() >= 40).then(|| ExaPage {
        text: text.to_string(),
        ..page
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn content_types_say_how_to_read() {
        assert_eq!(kind_of("text/html", &url("https://a.example/")), Kind::Html);
        assert_eq!(kind_of("application/pdf", &url("https://a.example/x")), Kind::Document);
        assert_eq!(
            kind_of(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                &url("https://a.example/x")
            ),
            Kind::Document
        );
        assert_eq!(
            kind_of("application/octet-stream", &url("https://a.example/paper.PDF")),
            Kind::Document
        );
        assert_eq!(kind_of("application/json", &url("https://a.example/")), Kind::Text);
        assert_eq!(kind_of("application/ld+json", &url("https://a.example/")), Kind::Text);
        assert!(matches!(
            kind_of("image/png", &url("https://a.example/")),
            Kind::Media(_)
        ));
        assert!(matches!(
            kind_of("application/zip", &url("https://a.example/a.zip")),
            Kind::Media(_)
        ));
        assert_eq!(kind_of("", &url("https://a.example/")), Kind::Unknown);
    }

    #[test]
    fn a_tab_can_show_a_document() {
        // As a page gives `document.contentType`, which need not be lower case.
        assert!(is_document("application/PDF", &url("https://a.example/x")));
        assert!(is_document("", &url("https://a.example/paper.pdf")));
        assert!(!is_document("text/html", &url("https://a.example/paper.pdf")));
        assert!(!is_document("text/html", &url("https://a.example/")));
    }

    #[test]
    fn bodies_without_a_type_are_sniffed() {
        assert_eq!(sniff(b"%PDF-1.7\n..."), Kind::Document);
        assert_eq!(sniff(b"\n  <!DOCTYPE html><html>"), Kind::Html);
        assert_eq!(sniff(b"just words"), Kind::Text);
    }

    #[test]
    fn character_sets_come_from_the_header_or_the_page() {
        let latin1 = b"caf\xe9";
        assert_eq!(decode(latin1, "text/plain; charset=ISO-8859-1", false), "café");
        let page = b"<html><head><meta charset=\"windows-1252\"></head><body>caf\xe9</body></html>";
        assert!(decode(page, "text/html", true).contains("café"));
        assert_eq!(decode("ok ✓".as_bytes(), "", false), "ok ✓");
    }

    #[test]
    fn json_is_laid_out() {
        assert_eq!(pretty("application/json", "{\"a\":1}".into()), "{\n  \"a\": 1\n}");
        assert_eq!(pretty("text/plain", "{\"a\":1}".into()), "{\"a\":1}");
    }

    #[test]
    fn exa_pages_lose_their_header() {
        let answer = "Title: Example Domain\nURL: https://example.com/\nPublished: N/A\nAuthor: N/A\nText: This domain is for use in documentation examples without needing permission.\n\nMore here.";
        let p = exa_page(answer).unwrap();
        assert_eq!(p.title.as_deref(), Some("Example Domain"));
        assert_eq!(p.published, None);
        assert!(p.text.starts_with("This domain is for use"), "{}", p.text);
        assert!(p.text.ends_with("More here."));
        let bare =
            "Title: X\nURL: https://x.example/\n\n# Heading\n\nA paragraph long enough to count as a page of text.";
        assert!(exa_page(bare).unwrap().text.starts_with("# Heading"));
        assert!(exa_page("Title: X\nURL: https://x.example/\nText: ").is_none());
        let heading = "# Example Domain\nURL: https://example.com/\n\nExample Domain\n\nThis domain is for use in documentation examples.";
        let p = exa_page(heading).unwrap();
        assert_eq!(p.title.as_deref(), Some("Example Domain"));
        assert_eq!(p.url.as_deref(), Some("https://example.com/"));
        assert!(p.text.starts_with("Example Domain\n\nThis domain"), "{}", p.text);
        // A page that opens with a heading of its own keeps it.
        assert!(exa_page(bare).unwrap().text.starts_with("# Heading"));
    }

    #[test]
    fn fragments_name_the_same_page() {
        assert_eq!(key(&url("https://a.example/p#part")), "https://a.example/p");
    }

    /// Reads real pages over plain HTTP, and one through Exa:
    /// `cargo test --lib -- --ignored live_reads --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_reads() {
        let client = crate::web::client().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let cases = [
            (
                "https://en.wikipedia.org/wiki/Rust_(programming_language)",
                "page",
                "Rust",
            ),
            ("https://www.rfc-editor.org/rfc/rfc9110.txt", "text", "HTTP Semantics"),
            (
                "https://www.rfc-editor.org/rfc/rfc9110.pdf",
                "document",
                "HTTP Semantics",
            ),
            (
                "https://api.github.com/repos/rust-lang/rust",
                "text",
                "\"full_name\": \"rust-lang/rust\"",
            ),
        ];
        for (address, kind, says) in cases {
            match http(&client, cache.path(), &url(address)).await {
                Ok(Got::Read(f)) => {
                    println!(
                        "{address}: {} {:?}, {} chars, note {:?}",
                        f.kind,
                        f.title,
                        f.text.len(),
                        f.note
                    );
                    assert_eq!(f.kind, kind, "{address}");
                    assert!(f.text.contains(says), "{address}: {}", &f.text[..f.text.len().min(600)]);
                }
                Ok(Got::Refused { why, .. }) => panic!("{address} was refused: {why}"),
                Err(e) => panic!("{address}: {e}"),
            }
        }
        match exa(&client, &url("https://example.com/")).await {
            Ok(f) => {
                println!("Exa: {:?}, {} chars", f.title, f.text.len());
                assert_eq!(f.title, "Example Domain");
                assert!(f.text.contains("documentation"), "{}", f.text);
            }
            Err(e) => println!("Exa refused: {e}"),
        }
    }
}
