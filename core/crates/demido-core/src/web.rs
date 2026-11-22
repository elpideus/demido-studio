//! Reading from the internet: one page by its address, and searching.
//!
//! This is the only part of Demido through which a model reaches the network, and it can only read. It
//! sends a `GET` and nothing else: no form, no body, no cookie, no password, no header that says who the
//! person is. What can leave the computer through it is therefore exactly this: the address itself (and
//! the address can carry text, which is why `guard` decides which addresses a model may open unasked), and
//! the words of a search.
//!
//! What it does for every request:
//!
//! - says who it is: `DemidoStudio/<version>` with the project's address. It never passes as a browser;
//! - goes only to the public internet (`guard`), checked again at every redirect;
//! - follows the site's `robots.txt` (`robots`);
//! - asks a site one thing at a time, with a pause between;
//! - keeps what it got (the caching rule, DECISIONS P): a page for a quarter of an hour before it asks the
//!   site whether it changed, a search for an hour, a `robots.txt` for a day;
//! - stops reading at two megabytes, and after twenty-five seconds.
//!
//! A PDF is read for its text, in a process of its own (`pdf`).
//!
//! Not built: pages that only show their text after running scripts (that needs the browser),
//! and anything that needs a key (that needs the place where keys are kept).

pub mod guard;
pub mod page;
pub mod pdf;
pub mod robots;
mod search;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use url::Url;

pub use search::{Found, Hit, Which};

/// How many characters of a page a model is given. About 1,300 tokens: a small model has few to spare.
pub const MOST_OF_A_PAGE: usize = 5_000;
/// No more of a page than this is downloaded.
const MOST_BYTES: usize = 2_000_000;
/// A PDF may be larger: one cut short cannot be read at all (its table of where everything is comes last).
const MOST_PDF_BYTES: usize = 20_000_000;
/// No more of a page's text than this is kept.
const MOST_TEXT: usize = 400_000;
const PAGE_IS_FRESH: Duration = Duration::from_secs(15 * 60);
const SEARCH_IS_FRESH: Duration = Duration::from_secs(60 * 60);
const ROBOTS_ARE_FRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// What is kept on disk never grows past this; the oldest goes first.
const KEPT_AT_MOST: u64 = 200_000_000;
/// The pause between two requests to one site.
const BETWEEN_TWO: Duration = Duration::from_millis(500);

/// Who Demido says it is, to every site.
pub fn name() -> String {
    format!("DemidoStudio/{} (+https://github.com/elpideus/demido-studio)", crate::VERSION)
}

/// Where the built-in search sources are. A test points them at a stand-in.
#[derive(Clone, Debug)]
pub struct Sources {
    /// With `{lang}` where the language goes.
    pub wikipedia: String,
    pub mwmbl: String,
    /// Exa's service for assistants: the whole address.
    pub exa: String,
    /// DuckDuckGo's page for browsers without scripts, and its lighter one.
    pub duckduckgo: String,
    pub duckduckgo_lite: String,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            wikipedia: "https://{lang}.wikipedia.org".into(),
            mwmbl: "https://api.mwmbl.org".into(),
            exa: "https://mcp.exa.ai/mcp".into(),
            duckduckgo: "https://html.duckduckgo.com".into(),
            duckduckgo_lite: "https://lite.duckduckgo.com".into(),
        }
    }
}

/// Which of the clients a request goes out on.
#[derive(Clone, Copy, PartialEq)]
enum Through {
    /// Only to the public internet.
    Strict,
    /// Also to this computer and its network: for what the person named themselves.
    Open,
    /// Only to the public internet, and only over IPv4: Exa's free service asks for that.
    StrictV4,
}

/// What is sent with a request that is not a plain `GET`.
enum Sent {
    /// A form, as a page's own form would send it. `from` is the page the form stands on.
    Form {
        fields: Vec<(String, String)>,
        from: String,
    },
    Json(String),
}

/// One request.
struct Ask<'a> {
    url: &'a Url,
    through: Through,
    /// What the site said last time, so that it can answer "unchanged".
    if_changed: Option<(&'a Option<String>, &'a Option<String>)>,
    /// The pause between two requests to this site.
    gap: Duration,
    sent: Option<Sent>,
    accept: &'static str,
}

const ACCEPT_A_PAGE: &str = "text/html,application/xhtml+xml,text/plain,application/json;q=0.9,*/*;q=0.5";

/// What a site sent back.
struct Got {
    status: u16,
    address: Url,
    kind: String,
    etag: Option<String>,
    changed: Option<String>,
    body: Vec<u8>,
    /// The site sent more than is read.
    cut: bool,
}

/// A page as it is kept.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct KeptPage {
    address: String,
    title: String,
    text: String,
    etag: Option<String>,
    changed: Option<String>,
    /// Said under the text: "the site sent more than is read", "almost nothing without scripts".
    notes: Vec<String>,
    /// The shop's own description of what the page sells, from its product data (`page::product_data`).
    #[serde(default)]
    product: Vec<String>,
}

/// A page as a model is given it.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// Where the page really was, after redirects.
    pub address: String,
    pub title: String,
    /// The words handed to the model.
    pub gave: String,
    /// Nothing was asked of the site: the page was kept from before.
    pub kept: bool,
}

pub struct Web {
    strict: reqwest::Client,
    /// Also reaches this computer and its network. Used only for an address the person wrote, and only
    /// when the person turned that on.
    open: reqwest::Client,
    /// As `strict`, over IPv4 only.
    strict_v4: reqwest::Client,
    kept_in: Option<PathBuf>,
    /// When each site may next be asked.
    next: Mutex<HashMap<String, Instant>>,
    /// A search source that said it is being asked too much: until when it is left alone, and what it said.
    paused: Mutex<HashMap<&'static str, (Instant, String)>>,
    pub(crate) sources: Sources,
    /// Addresses on this computer that count as the public internet. Only a test fills this, for its stand-in.
    stand_ins: Arc<Vec<String>>,
    /// Where a PDF's text is taken out.
    pdf: pdf::Reader,
    puts: AtomicUsize,
}

fn authority(url: &Url) -> String {
    format!("{}:{}", url.host_str().unwrap_or_default(), url.port_or_known_default().unwrap_or(0))
}

/// Why a request failed, in words a model can pass on.
fn why(error: &reqwest::Error) -> String {
    let mut source: Option<&dyn std::error::Error> = Some(error);
    while let Some(e) = source {
        let said = e.to_string();
        if said.contains(guard::NOT_PUBLIC) || said.contains("redirect") && said.contains("not followed") {
            return said;
        }
        source = e.source();
    }
    if error.is_timeout() {
        "the site did not answer in time".into()
    } else if error.is_connect() {
        "the site could not be reached".into()
    } else if error.is_redirect() {
        "the site sent the request round in circles".into()
    } else {
        format!("the page could not be read ({})", error.without_url_ref())
    }
}

trait WithoutUrl {
    fn without_url_ref(&self) -> String;
}
impl WithoutUrl for reqwest::Error {
    /// The error's words without the address in them: an address can be long, and it is already known.
    fn without_url_ref(&self) -> String {
        let said = self.to_string();
        match self.url() {
            Some(url) => said.replace(&format!(" for url ({url})"), "").replace(url.as_str(), "the address"),
            None => said,
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Web {
    /// `kept_in`: where pages and searches are kept. `None` keeps nothing.
    pub fn new(kept_in: Option<PathBuf>) -> Self {
        Self::with(kept_in, Sources::default(), Vec::new())
    }

    /// The same, taking a PDF's text out in a process of its own, `<program> pdf-text`, when `program` is
    /// Demido's own (in a test it is the test, and stays here).
    pub fn reading_pdfs_apart(mut self, program: Option<PathBuf>) -> Self {
        if let Some(program) = program.filter(|p| p.file_stem().is_some_and(|s| s == "demido")) {
            self.pdf = pdf::Reader::Apart(program);
        }
        self
    }

    /// With the search sources somewhere else, and with addresses on this computer that count as public: for
    /// a test and its stand-in.
    pub fn with(kept_in: Option<PathBuf>, sources: Sources, stand_ins: Vec<String>) -> Self {
        let stand_ins = Arc::new(stand_ins);
        let checked = stand_ins.clone();
        let builder = || reqwest::Client::builder().user_agent(name()).connect_timeout(Duration::from_secs(8)).timeout(Duration::from_secs(25)).referer(false);
        // The same rules for both strict clients: every redirect is checked like a first address.
        let checking = |checked: Arc<Vec<String>>| {
            reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 {
                    return attempt.error("a redirect was not followed: the site sent the request on more than five times");
                }
                let next = attempt.url();
                if !matches!(next.scheme(), "http" | "https") || !next.username().is_empty() || next.password().is_some() {
                    return attempt.error("a redirect was not followed: it led to an address that is not read");
                }
                if guard::is_local(next) && !checked.contains(&authority(next)) {
                    let said = format!("a redirect was not followed: it led to {}, which {}", next.host_str().unwrap_or_default(), guard::NOT_PUBLIC);
                    return attempt.error(said);
                }
                // A test stays with its stand-in.
                if !checked.is_empty() && !checked.contains(&authority(next)) {
                    return attempt.error("a redirect was not followed: a test does not reach the internet");
                }
                attempt.follow()
            })
        };
        // A name is looked up into public numbers only. The stand-in of a test is on this computer, so a
        // test's client looks names up as usual; every other check stays.
        let public_only = |client: reqwest::ClientBuilder| if stand_ins.is_empty() { client.dns_resolver(Arc::new(guard::PublicOnly)) } else { client };
        let strict = public_only(builder().redirect(checking(checked.clone())));
        let strict_v4 = public_only(builder().redirect(checking(checked)).local_address(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)));
        let open = builder().redirect(reqwest::redirect::Policy::limited(5));
        let web = Self {
            strict: strict.build().expect("a client can be made"),
            open: open.build().expect("a client can be made"),
            strict_v4: strict_v4.build().expect("a client can be made"),
            kept_in,
            next: Mutex::default(),
            paused: Mutex::default(),
            sources,
            stand_ins,
            pdf: pdf::Reader::Here,
            puts: AtomicUsize::new(0),
        };
        web.sweep();
        web
    }

    /// Whether an address is refused for being on this computer or its network.
    fn is_local(&self, url: &Url) -> bool {
        guard::is_local(url) && !self.stand_ins.contains(&authority(url))
    }

    // ----- what is kept ---------------------------------------------------------------------------------

    fn file_of(&self, kind: &str, key: &str) -> Option<PathBuf> {
        let name = hex(&Sha256::digest(key.as_bytes())[..16]);
        self.kept_in.as_ref().map(|dir| dir.join(format!("{kind}-{name}.json")))
    }

    /// What was kept under a key, and how old it is.
    fn kept<T: DeserializeOwned>(&self, kind: &str, key: &str) -> Option<(Duration, T)> {
        let file = self.file_of(kind, key)?;
        let age = std::fs::metadata(&file).ok()?.modified().ok().and_then(|at| SystemTime::now().duration_since(at).ok()).unwrap_or(Duration::MAX);
        Some((age, serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?))
    }

    fn keep<T: Serialize>(&self, kind: &str, key: &str, value: &T) {
        let Some(file) = self.file_of(kind, key) else { return };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Written beside and then moved into place, so that a half-written file is never read.
        let beside = file.with_extension("part");
        if serde_json::to_vec(value).ok().and_then(|bytes| std::fs::write(&beside, bytes).ok()).is_some() {
            let _ = std::fs::rename(&beside, &file);
        }
        if self.puts.fetch_add(1, Ordering::Relaxed) % 100 == 99 {
            self.sweep();
        }
    }

    /// Removes the oldest of what is kept until it is well under the limit.
    fn sweep(&self) {
        let Some(dir) = &self.kept_in else { return };
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut files: Vec<(SystemTime, u64, PathBuf)> =
            entries.filter_map(|e| e.ok()).filter_map(|e| e.metadata().ok().filter(|m| m.is_file()).map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len(), e.path()))).collect();
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        if total <= KEPT_AT_MOST {
            return;
        }
        files.sort();
        for (_, size, path) in files {
            if total <= KEPT_AT_MOST * 3 / 4 {
                break;
            }
            if std::fs::remove_file(path).is_ok() {
                total -= size;
            }
        }
    }

    // ----- asking a site --------------------------------------------------------------------------------

    /// Waits until this site may be asked again, and books the next turn.
    async fn turn_for(&self, url: &Url, gap: Duration) {
        let at = {
            let mut next = self.next.lock().expect("not poisoned");
            let now = Instant::now();
            let at = next.get(&authority(url)).copied().filter(|at| *at > now).unwrap_or(now);
            next.insert(authority(url), at + gap);
            at
        };
        tokio::time::sleep_until(at.into()).await;
    }

    /// One `GET`. `if_changed`: what the site said last time, so that it can answer "unchanged".
    async fn get(&self, url: &Url, open: bool, if_changed: Option<(&Option<String>, &Option<String>)>, gap: Duration, stop: &AtomicBool) -> Result<Got, String> {
        self.send(Ask { url, through: if open { Through::Open } else { Through::Strict }, if_changed, gap, sent: None, accept: ACCEPT_A_PAGE }, stop).await
    }

    /// One request, of whatever kind, and what came back.
    async fn send(&self, ask: Ask<'_>, stop: &AtomicBool) -> Result<Got, String> {
        let url = ask.url;
        // A test stays with its stand-in, whatever address turns up in what it reads.
        if ask.through != Through::Open && !self.stand_ins.is_empty() && !self.stand_ins.contains(&authority(url)) {
            return Err("a test does not reach the internet".into());
        }
        self.turn_for(url, ask.gap).await;
        let client = match ask.through {
            Through::Strict => &self.strict,
            Through::Open => &self.open,
            Through::StrictV4 => &self.strict_v4,
        };
        let mut request = match ask.sent {
            None => client.get(url.clone()),
            Some(Sent::Json(body)) => client.post(url.clone()).header("content-type", "application/json").body(body),
            Some(Sent::Form { fields, from }) => {
                let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(fields).finish();
                client.post(url.clone()).header("content-type", "application/x-www-form-urlencoded").header("referer", from).body(body)
            }
        }
        .header("accept", ask.accept);
        if let Some((etag, changed)) = ask.if_changed {
            if let Some(etag) = etag {
                request = request.header("if-none-match", etag);
            }
            if let Some(changed) = changed {
                request = request.header("if-modified-since", changed);
            }
        }
        let stopped = async {
            while !stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        let read = async {
            let mut response = request.send().await.map_err(|e| why(&e))?;
            let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
            let (status, address, kind, etag, changed) =
                (response.status().as_u16(), response.url().clone(), header("content-type").unwrap_or_default().to_ascii_lowercase(), header("etag"), header("last-modified"));
            let (mut body, mut cut) = (Vec::new(), false);
            let most = if kind.starts_with("application/pdf") { MOST_PDF_BYTES } else { MOST_BYTES };
            while let Some(chunk) = response.chunk().await.map_err(|e| why(&e))? {
                let room = most - body.len();
                if chunk.len() > room {
                    body.extend_from_slice(&chunk[..room]);
                    cut = true;
                    break;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Got { status, address, kind, etag, changed, body, cut })
        };
        tokio::select! {
            got = read => got,
            _ = stopped => Err("it was stopped".into()),
        }
    }

    /// Whether the site lets programs read this page. A site whose `robots.txt` cannot be had does not
    /// keep one page from being read for a person who asked for it.
    async fn allowed_by_site(&self, url: &Url, open: bool, stop: &AtomicBool) -> bool {
        let Ok(at) = url.join("/robots.txt") else { return true };
        let rules = match self.kept::<String>("robots", at.as_str()) {
            Some((age, text)) if age < ROBOTS_ARE_FRESH => text,
            _ => {
                let text = match self.get(&at, open, None, BETWEEN_TWO, stop).await {
                    Ok(got) if got.status == 200 && !got.kind.contains("html") => String::from_utf8_lossy(&got.body).into_owned(),
                    // No such file, or no answer: no rules.
                    _ => String::new(),
                };
                self.keep("robots", at.as_str(), &text);
                text
            }
        };
        let path = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_string(),
        };
        robots::Robots::read(&rules).allows(&path)
    }

    // ----- reading a page -------------------------------------------------------------------------------

    /// The text of the page at an address, cut to what was asked `about`. `may_be_local`: the person wrote
    /// this address and turned on reading from this computer and its network.
    pub async fn read(&self, written: &str, about: &str, may_be_local: bool, stop: &AtomicBool) -> Result<Read, String> {
        let (page, kept) = self.page(written, may_be_local, stop).await?;
        Ok(told(&page, about, kept))
    }

    /// The whole readable text of the page at an address, from what is kept or from the site. The second
    /// value says that the site was not asked for the page itself.
    async fn page(&self, written: &str, may_be_local: bool, stop: &AtomicBool) -> Result<(KeptPage, bool), String> {
        let mut url = guard::address(written)?;
        url.set_fragment(None);
        let local = self.is_local(&url);
        if local && !may_be_local {
            return Err(format!("{} {}, and that is not read", url.host_str().unwrap_or_default(), guard::NOT_PUBLIC));
        }
        let key = url.as_str().to_string();
        let before = self.kept::<KeptPage>("page", &key);
        if let Some((age, page)) = &before
            && *age < PAGE_IS_FRESH
        {
            return Ok((page.clone(), true));
        }
        if !local && !self.allowed_by_site(&url, false, stop).await {
            return Err("the site asks programs not to read this page (its robots.txt), so it was not read".into());
        }
        let if_changed = before.as_ref().map(|(_, page)| (&page.etag, &page.changed)).filter(|(etag, changed)| etag.is_some() || changed.is_some());
        let got = self.get(&url, local, if_changed, BETWEEN_TWO, stop).await?;
        if got.status == 304
            && let Some((_, page)) = before
        {
            // Unchanged: what was kept is as good as new.
            self.keep("page", &key, &page);
            return Ok((page, true));
        }
        if !(200..300).contains(&got.status) {
            return Err(match got.status {
                401 | 403 => format!("the site refused ({}): the page is not open to programs, or needs signing in", got.status),
                404 | 410 => format!("there is no page at that address ({})", got.status),
                429 => "the site says it is being asked too often (429)".into(),
                500..=599 => format!("the site has a problem of its own ({})", got.status),
                other => format!("the site answered {other}"),
            });
        }
        let page = self.as_text(got).await?;
        self.keep("page", &key, &page);
        Ok((page, false))
    }

    /// What a site sent, as a page of text. `Err` for what is not text.
    async fn as_text(&self, got: Got) -> Result<KeptPage, String> {
        let kind = got.kind.split(';').next().unwrap_or_default().trim().to_string();
        let looks_like_html = || got.body.iter().take(600).copied().skip_while(u8::is_ascii_whitespace).take(15).collect::<Vec<u8>>().to_ascii_lowercase().starts_with(b"<");
        let is_html = kind.contains("html") || (kind.is_empty() && looks_like_html());
        let is_text = kind.starts_with("text/")
            || kind.is_empty()
            || ["application/json", "application/xml", "application/ld+json", "application/javascript"].contains(&kind.as_str())
            || kind.ends_with("+json")
            || kind.ends_with("+xml");
        if kind == "application/pdf" || got.body.starts_with(b"%PDF-") {
            if got.cut {
                return Err(format!("the PDF is larger than {} MB, the most read", MOST_PDF_BYTES / 1_000_000));
            }
            let text = pdf::read(got.body, &self.pdf).await?;
            let address = got.address.to_string();
            let title = got.address.path_segments().and_then(|mut s| s.next_back().map(str::to_string)).filter(|s| !s.is_empty()).unwrap_or_default();
            let mut text = text;
            if let Some((at, _)) = text.char_indices().nth(MOST_TEXT) {
                text.truncate(at);
            }
            // Said as Demido's, not the file's: told "A PDF: its text only", a real model (Qwen3 4B) passed it
            // on as something the PDF said.
            let notes = vec!["Demido's note: this is a PDF, and only its text was read, without its pictures or its layout.".to_string()];
            return Ok(KeptPage { address, title, text, etag: got.etag, changed: got.changed, notes, product: Vec::new() });
        }
        if !is_html && !is_text {
            return Err(format!("this is not a page of text (it is {kind})"));
        }
        // The page says how it is written: in the header, or for HTML near its top. Otherwise UTF-8.
        let labelled = got.kind.split("charset=").nth(1).map(|c| c.trim_matches(['"', '\'', ' ', ';']).to_string()).or_else(|| {
            let top = String::from_utf8_lossy(&got.body[..got.body.len().min(2048)]).to_ascii_lowercase();
            top.split("charset=").nth(1).map(|c| c.trim_start_matches(['"', '\'']).chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect())
        });
        let encoding = labelled.and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes())).unwrap_or(encoding_rs::UTF_8);
        let (raw, _, _) = encoding.decode(&got.body);
        let (raw, address) = (raw.into_owned(), got.address.to_string());
        let mut notes = Vec::new();
        if got.cut {
            notes.push(format!("The site sent more than {} MB; only the first {} MB were read.", MOST_BYTES / 1_000_000, MOST_BYTES / 1_000_000));
        }
        let mut product = Vec::new();
        let (title, mut text) = if is_html {
            let scripts = raw.matches("<script").count();
            let for_page = address.clone();
            // Taking a page apart is work for a thread of its own.
            let (found, data) = tokio::task::spawn_blocking(move || (page::readable(&raw, &for_page), page::product_data(&raw))).await.map_err(|_| "the page could not be taken apart".to_string())?;
            product = data;
            if found.text.chars().count() < 200 && scripts >= 3 {
                notes.push("This page shows almost nothing until its scripts have run. Demido does not run a page's scripts yet.".into());
            }
            (found.title, found.text)
        } else {
            (String::new(), raw.replace('\r', ""))
        };
        if let Some((at, _)) = text.char_indices().nth(MOST_TEXT) {
            text.truncate(at);
        }
        Ok(KeptPage { address, title, text, etag: got.etag, changed: got.changed, notes, product })
    }
}

/// A kept page as a model is given it: where it is, its title, the parts that matter, and what was left out.
fn told(page: &KeptPage, about: &str, kept: bool) -> Read {
    let given = page::passages(&page.text, about, MOST_OF_A_PAGE);
    let mut gave = String::new();
    if !page.title.is_empty() {
        gave.push_str(&format!("Page: {}\n", page.title));
    }
    gave.push_str(&format!("Address: {}\n\n", page.address));
    // Always given, whatever was asked: the price and the stock the shop itself publishes.
    if !page.product.is_empty() {
        gave.push_str(&format!("The shop's own product data, from the page's description for search engines:\n{}\n\n", page.product.join("\n")));
    }
    gave.push_str(if given.text.is_empty() { "(The page has no text.)" } else { &given.text });
    if given.cut {
        let all = page.text.chars().count();
        gave.push_str(&if given.chosen {
            format!("\n\n(The page is longer: {all} characters. These are the parts that match what was asked.)")
        } else {
            format!("\n\n(The page is longer: {all} characters. This is its start.)")
        });
    }
    for note in &page.notes {
        gave.push_str(&format!("\n\n({note})"));
    }
    Read { address: page.address.clone(), title: page.title.clone(), gave, kept }
}

#[cfg(test)]
pub(crate) mod tests;
