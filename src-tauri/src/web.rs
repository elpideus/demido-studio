use crate::local::searxng;
use regex::Regex;
use serde_json::{json, Value};
use std::net::{IpAddr, ToSocketAddrs};

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (o[0] == 169 && o[1] == 254) // link-local
                || (o[0] == 100 && (64..=127).contains(&o[1])) // CGNAT
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10
        }
    }
}

fn check_ssrf(url: &str) -> Result<(), String> {
    let parsed = url
        .parse::<reqwest::Url>()
        .map_err(|e| format!("Invalid URL: {}", e))?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("Scheme '{}' not allowed", scheme));
    }
    let host = parsed
        .host_str()
        .ok_or("No host in URL")?
        .to_string();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let addrs: Vec<IpAddr> = format!("{}:{}", host, port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed: {}", e))?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() {
        return Err("DNS resolution returned no addresses".into());
    }
    for ip in &addrs {
        if is_private_ip(ip) {
            return Err(format!("Private/loopback IP blocked: {}", ip));
        }
    }
    Ok(())
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn strip_html(s: &str) -> String {
    let tag_re = Regex::new(r"<[^>]+>").unwrap();
    let ws_re = Regex::new(r"\s+").unwrap();
    let stripped = tag_re.replace_all(s, " ");
    let decoded = stripped
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    ws_re.replace_all(decoded.trim(), " ").into_owned()
}

/// Extract readable text from HTML: strip scripts/styles, then tags.
///
/// One regex per tag, not `<(script|style)...</\1>`: Rust's `regex` has no backreferences, so
/// that pattern's `unwrap()` panicked on every HTML fetch.
fn extract_text(html: &str) -> String {
    let script_re = Regex::new(r"(?si)<script[^>]*>.*?</script>").unwrap();
    let style_re = Regex::new(r"(?si)<style[^>]*>.*?</style>").unwrap();
    let without = script_re.replace_all(html, " ");
    let without = style_re.replace_all(&without, " ");
    strip_html(&without)
}

const DESKTOP_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

const EXA_URL: &str = "https://mcp.exa.ai/mcp";
const PARALLEL_URL: &str = "https://search.parallel.ai/mcp";

/// Parse an MCP `tools/call` HTTP response body, which may be plain JSON or an SSE stream
/// (`data: {...}` lines). Returns the first non-empty `result.content[].text`.
fn parse_mcp_response(body: &str) -> Option<String> {
    let extract = |json_str: &str| -> Option<String> {
        let v: Value = serde_json::from_str(json_str).ok()?;
        v["result"]["content"]
            .as_array()?
            .iter()
            .find_map(|c| c["text"].as_str().filter(|t| !t.is_empty()).map(|t| t.to_string()))
    };

    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        if let Some(text) = extract(trimmed) {
            return Some(text);
        }
    }
    for line in body.lines() {
        if let Some(payload) = line.strip_prefix("data: ") {
            if let Some(text) = extract(payload) {
                return Some(text);
            }
        }
    }
    None
}

async fn call_mcp_tool(
    client: &reqwest::Client,
    url: &str,
    tool: &str,
    arguments: Value,
    extra_headers: &[(&str, String)],
) -> Result<String, String> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments }
    });

    let mut req = client
        .post(url)
        .header("Accept", "application/json, text/event-stream")
        .json(&body);
    for (k, v) in extra_headers {
        req = req.header(*k, v);
    }

    let resp = req
        .timeout(std::time::Duration::from_secs(25))
        .send()
        .await
        .map_err(|e| format!("{} request error: {}", tool, e))?;

    if !resp.status().is_success() {
        return Err(format!("{} HTTP {}", tool, resp.status().as_u16()));
    }

    let text = resp
        .text()
        .await
        .map_err(|e| format!("{} read error: {}", tool, e))?;

    parse_mcp_response(&text).ok_or_else(|| format!("{}: no content in response", tool))
}

async fn exa_search(client: &reqwest::Client, query: &str, api_key: Option<&str>) -> Result<String, String> {
    let url = match api_key {
        Some(k) if !k.is_empty() => format!("{}?exaApiKey={}", EXA_URL, urlencode(k)),
        _ => EXA_URL.to_string(),
    };
    call_mcp_tool(
        client,
        &url,
        "web_search_exa",
        json!({ "query": query, "type": "auto", "numResults": 8, "livecrawl": "fallback" }),
        &[],
    )
    .await
}

async fn parallel_search(client: &reqwest::Client, query: &str, api_key: Option<&str>) -> Result<String, String> {
    let mut headers = vec![];
    if let Some(k) = api_key.filter(|k| !k.is_empty()) {
        headers.push(("Authorization", format!("Bearer {}", k)));
    }
    call_mcp_tool(
        client,
        PARALLEL_URL,
        "web_search",
        json!({ "objective": query, "search_queries": [query] }),
        &headers,
    )
    .await
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// A search backend the user can enable and position in Tools > Web Browsing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SearchProvider {
    Exa,
    Parallel,
    Searxng,
    Ddg,
}

impl SearchProvider {
    fn from_id(s: &str) -> Option<Self> {
        match s.trim() {
            "exa" => Some(Self::Exa),
            "parallel" => Some(Self::Parallel),
            "searxng" => Some(Self::Searxng),
            "ddg" => Some(Self::Ddg),
            _ => None,
        }
    }

    /// The `settings` key holding this provider's on/off state, and its default.
    pub fn toggle_key(self) -> &'static str {
        match self {
            Self::Exa => "websearch_exa_enabled",
            Self::Parallel => "websearch_parallel_enabled",
            Self::Searxng => "websearch_searxng_enabled",
            Self::Ddg => "websearch_ddg_enabled",
        }
    }

    /// SearXNG is opt-in — first use triggers a heavy install. The rest default on.
    pub fn default_enabled(self) -> bool {
        !matches!(self, Self::Searxng)
    }
}

/// Order used when the user hasn't reordered anything.
pub const DEFAULT_ORDER: [SearchProvider; 4] = [
    SearchProvider::Exa,
    SearchProvider::Parallel,
    SearchProvider::Searxng,
    SearchProvider::Ddg,
];

/// Parse the `websearch_order` setting (a comma-separated id list). Unknown and duplicate ids
/// are dropped, and any provider the stored order doesn't mention is appended in default
/// order — so a setting written by an older or newer build still yields every provider exactly
/// once.
pub fn parse_order(stored: Option<&str>) -> Vec<SearchProvider> {
    let mut order: Vec<SearchProvider> = Vec::with_capacity(DEFAULT_ORDER.len());
    for id in stored.unwrap_or("").split(',') {
        if let Some(p) = SearchProvider::from_id(id) {
            if !order.contains(&p) {
                order.push(p);
            }
        }
    }
    for p in DEFAULT_ORDER {
        if !order.contains(&p) {
            order.push(p);
        }
    }
    order
}

/// Web search over the user's chosen provider order (Tools > Web Browsing). Walks `order` —
/// which the caller has already filtered down to enabled providers — and returns the first
/// non-empty result, moving on whenever a provider errors or finds nothing. Exa and Parallel
/// take optional user API keys for higher quota; SearXNG is skipped unless its worker is up.
pub async fn web_search_impl(
    query: &str,
    page: u64,
    exa_key: Option<&str>,
    parallel_key: Option<&str>,
    searxng_engine: Option<&searxng::SearxngEngine>,
    order: &[SearchProvider],
) -> String {
    if order.is_empty() {
        return "All web search providers are disabled. Enable one in Tools > Web Browsing.".into();
    }

    let client = match reqwest::Client::builder()
        .user_agent(format!("Demido/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error building client: {}", e),
    };

    for provider in order {
        let result = match provider {
            SearchProvider::Exa => exa_search(&client, query, exa_key).await,
            SearchProvider::Parallel => parallel_search(&client, query, parallel_key).await,
            SearchProvider::Ddg => ddg_search(&client, query, page).await,
            SearchProvider::Searxng => match searxng_engine.filter(|e| e.is_running()) {
                Some(engine) => searxng::search(engine, query).await,
                None => continue,
            },
        };
        if let Ok(text) = result {
            if !text.trim().is_empty() {
                return text;
            }
        }
    }

    format!("No results found for \"{}\".", query)
}

/// `Ok("")` means "ran fine, found nothing" — the caller treats that the same as a failure and
/// moves to the next provider, since DuckDuckGo is no longer guaranteed to be last in the order.
async fn ddg_search(client: &reqwest::Client, query: &str, page: u64) -> Result<String, String> {
    let offset = page * 20;
    let mut params: Vec<(&str, String)> = vec![("q", query.to_string())];
    if offset > 0 {
        params.push(("s", offset.to_string()));
        params.push(("dc", (offset + 1).to_string()));
    }

    let resp = match client
        .post("https://html.duckduckgo.com/html/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("User-Agent", DESKTOP_UA)
        .timeout(std::time::Duration::from_secs(15))
        .form(&params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(format!("Search request error: {}", e)),
    };

    let html = match resp.text().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Failed to read response: {}", e)),
    };

    // DuckDuckGo HTML search result links: href="//duckduckgo.com/l/?uddg=<url>&..."
    // or direct http(s) links
    let link_re = Regex::new(
        r#"(?i)class="result__a"[^>]*href="(?:[^"]*uddg=([^&"]+)|([^"]+))"[^>]*>([\s\S]*?)</a>"#,
    )
    .unwrap();
    let snippet_re =
        Regex::new(r#"(?i)class="result__snippet"[^>]*>([\s\S]*?)</(?:a|span|div)"#).unwrap();

    let mut urls: Vec<String> = Vec::new();
    let mut titles: Vec<String> = Vec::new();

    for cap in link_re.captures_iter(&html) {
        let url = if let Some(uddg) = cap.get(1) {
            percent_decode(uddg.as_str())
        } else if let Some(direct) = cap.get(2) {
            direct.as_str().to_string()
        } else {
            continue;
        };
        let title = strip_html(cap.get(3).map(|m| m.as_str()).unwrap_or(""));
        if url.starts_with("http") && !title.is_empty() {
            urls.push(url);
            titles.push(title);
        }
        if urls.len() >= 15 {
            break;
        }
    }

    let snippets: Vec<String> = snippet_re
        .captures_iter(&html)
        .map(|c| strip_html(c.get(1).map(|m| m.as_str()).unwrap_or("")))
        .collect();

    if urls.is_empty() {
        return Ok(String::new());
    }

    let mut lines = Vec::new();
    for (i, (url, title)) in urls.iter().zip(titles.iter()).enumerate() {
        let snippet = snippets.get(i).map(|s| s.as_str()).unwrap_or("");
        lines.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, snippet));
    }
    Ok(lines.join("\n\n"))
}

const MAX_FETCH_BYTES: u64 = 5 * 1024 * 1024; // 5MB
const MAX_FETCH_CHARS: usize = 20_000;

/// Fetch a URL and return its content as markdown (HTML pages), plaintext (other content), or a
/// short notice for images. `format` controls HTML handling: "markdown" (default), "text", "html".
pub async fn web_fetch_impl(url: &str, format: &str) -> String {
    if let Err(e) = check_ssrf(url) {
        return format!("SSRF blocked: {}", e);
    }

    // Disable auto-redirect so we can re-validate each hop against SSRF guard.
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: {}", e),
    };

    let mut current_url = url.to_string();
    let mut user_agent = DESKTOP_UA.to_string();
    const MAX_REDIRECTS: usize = 10;

    for _ in 0..MAX_REDIRECTS {
        let resp = match client
            .get(&current_url)
            .header("User-Agent", &user_agent)
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return format!("Fetch error: {}", e),
        };

        let status = resp.status();

        // Cloudflare bot-detection false positive on our spoofed UA: retry once honestly.
        if status.as_u16() == 403
            && user_agent == DESKTOP_UA
            && resp
                .headers()
                .get("cf-mitigated")
                .and_then(|v| v.to_str().ok())
                == Some("challenge")
        {
            user_agent = format!("Demido/{}", env!("CARGO_PKG_VERSION"));
            continue;
        }

        // Follow 3xx manually, re-checking SSRF on each Location.
        if status.is_redirection() {
            let location = match resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                Some(loc) => loc.to_string(),
                None => return "Redirect with no Location header".into(),
            };
            // Resolve relative redirects
            let next = match reqwest::Url::parse(&current_url)
                .ok()
                .and_then(|base| base.join(&location).ok())
            {
                Some(u) => u.to_string(),
                None => location,
            };
            if let Err(e) = check_ssrf(&next) {
                return format!("SSRF blocked on redirect: {}", e);
            }
            current_url = next;
            continue;
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_lowercase();
        let mime = content_type.split(';').next().unwrap_or("").trim().to_string();

        if !status.is_success() {
            return format!("HTTP {}", status.as_u16());
        }

        if let Some(len) = resp.content_length() {
            if len > MAX_FETCH_BYTES {
                return "Response too large (exceeds 5MB limit)".into();
            }
        }

        if mime.starts_with("image/") {
            return format!("URL points to an image ({}). Not fetched as text.", mime);
        }

        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return format!("Read error: {}", e),
        };
        if bytes.len() as u64 > MAX_FETCH_BYTES {
            return "Response too large (exceeds 5MB limit)".into();
        }
        let body = String::from_utf8_lossy(&bytes).into_owned();
        let is_html = mime.contains("html");

        let text = match (format, is_html) {
            ("html", _) => body,
            ("text", true) => extract_text(&body),
            ("text", false) => body,
            (_, true) => htmd::convert(&body).unwrap_or_else(|_| extract_text(&body)), // markdown (default)
            (_, false) => body,
        };

        return if text.chars().count() > MAX_FETCH_CHARS {
            let cut = text
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= MAX_FETCH_CHARS)
                .last()
                .unwrap_or(0);
            format!("{}\n[truncated at 20k chars]", &text[..cut])
        } else {
            text
        };
    }

    "Too many redirects".into()
}

/// What the Sources details panel shows for one cited link. Every field past `url` is best-effort:
/// a page that serves no Open Graph tags, or is simply down, still gets a row — `error` explains
/// the gap rather than the row vanishing, since a missing source reads as a citation we made up.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub url: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub site_name: Option<String>,
    pub error: Option<String>,
}

impl LinkPreview {
    fn failed(url: &str, error: String) -> Self {
        Self {
            url: url.to_string(),
            title: None,
            description: None,
            image: None,
            site_name: None,
            error: Some(error),
        }
    }
}

/// Only the `<head>` carries the metadata, and pages routinely run to megabytes of body after it.
/// Preview fetches stop here rather than pulling the whole document for four tags.
const MAX_PREVIEW_BYTES: usize = 128 * 1024;
const MAX_DESC_CHARS: usize = 400;

/// Pull `<meta>` name/property → content pairs out of an HTML head.
///
/// Attribute order is not fixed in the wild (`content` before `property` is common), so each tag's
/// attributes are parsed individually rather than matched by one positional pattern.
fn parse_meta_tags(html: &str) -> Vec<(String, String)> {
    let meta_re = Regex::new(r"(?is)<meta\s+([^>]*?)/?>").unwrap();
    let attr_re =
        Regex::new(r#"(?is)([\w:.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#).unwrap();

    meta_re
        .captures_iter(html)
        .filter_map(|tag| {
            let mut key: Option<String> = None;
            let mut content: Option<String> = None;
            for attr in attr_re.captures_iter(tag.get(1)?.as_str()) {
                let name = attr.get(1)?.as_str().to_lowercase();
                let value = attr
                    .get(2)
                    .or_else(|| attr.get(3))
                    .or_else(|| attr.get(4))
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                match name.as_str() {
                    "property" | "name" => key = Some(value.to_lowercase()),
                    "content" => content = Some(value),
                    _ => {}
                }
            }
            Some((key?, content?))
        })
        .collect()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut.trim_end())
}

/// Extract preview metadata from an HTML document. `base_url` resolves relative `og:image` paths —
/// plenty of sites ship `og:image` as `/static/card.png`, which is useless to an `<img>` tag.
fn preview_from_html(url: &str, html: &str) -> LinkPreview {
    let metas = parse_meta_tags(html);
    let pick = |keys: &[&str]| -> Option<String> {
        keys.iter().find_map(|k| {
            metas
                .iter()
                .find(|(name, content)| name == k && !content.trim().is_empty())
                .map(|(_, content)| strip_html(content))
        })
    };

    let title = pick(&["og:title", "twitter:title"]).or_else(|| {
        Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
            .unwrap()
            .captures(html)
            .and_then(|c| c.get(1))
            .map(|m| strip_html(m.as_str()))
            .filter(|t| !t.trim().is_empty())
    });

    let description = pick(&["og:description", "twitter:description", "description"])
        .map(|d| truncate_chars(&d, MAX_DESC_CHARS));

    let image = pick(&["og:image", "og:image:url", "twitter:image"]).and_then(|img| {
        reqwest::Url::parse(url)
            .ok()
            .and_then(|base| base.join(&img).ok())
            .map(|u| u.to_string())
            .or(Some(img))
            .filter(|u| u.starts_with("http"))
    });

    LinkPreview {
        url: url.to_string(),
        title,
        description,
        image,
        site_name: pick(&["og:site_name"]),
        error: None,
    }
}

/// Fetch one cited URL and read its Open Graph / meta tags for the details panel.
///
/// Runs the same SSRF guard as `web_fetch`, including on redirects: these URLs come from model
/// output, so they are attacker-influenceable in exactly the way that guard exists for.
pub async fn link_preview_impl(client: &reqwest::Client, url: &str) -> LinkPreview {
    if let Err(e) = check_ssrf(url) {
        return LinkPreview::failed(url, e);
    }

    let mut current = url.to_string();
    const MAX_REDIRECTS: usize = 5;

    for _ in 0..MAX_REDIRECTS {
        let resp = match client
            .get(&current)
            .header("User-Agent", DESKTOP_UA)
            .header("Accept-Language", "en-US,en;q=0.9")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return LinkPreview::failed(url, format!("Fetch error: {}", e)),
        };

        let status = resp.status();
        if status.is_redirection() {
            let location = match resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                Some(loc) => loc.to_string(),
                None => return LinkPreview::failed(url, "Redirect with no Location header".into()),
            };
            let next = reqwest::Url::parse(&current)
                .ok()
                .and_then(|base| base.join(&location).ok())
                .map(|u| u.to_string())
                .unwrap_or(location);
            if let Err(e) = check_ssrf(&next) {
                return LinkPreview::failed(url, format!("SSRF blocked on redirect: {}", e));
            }
            current = next;
            continue;
        }

        if !status.is_success() {
            return LinkPreview::failed(url, format!("HTTP {}", status.as_u16()));
        }

        let mime = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if !mime.contains("html") && !mime.is_empty() {
            let mut preview = LinkPreview::failed(url, format!("Not an HTML page ({})", mime));
            preview.title = reqwest::Url::parse(url)
                .ok()
                .and_then(|u| u.path_segments().and_then(|mut s| s.next_back().map(String::from)))
                .filter(|s| !s.is_empty());
            return preview;
        }

        // Read the head and stop — chunk by chunk, so a huge page costs a few KB, not its size.
        let mut body = Vec::new();
        let mut resp = resp;
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    body.extend_from_slice(&chunk);
                    let seen = String::from_utf8_lossy(&body);
                    if body.len() >= MAX_PREVIEW_BYTES || seen.contains("</head>") {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    if body.is_empty() {
                        return LinkPreview::failed(url, format!("Read error: {}", e));
                    }
                    break;
                }
            }
        }

        // `url`, not `current`: the panel links where the model cited, not where it redirected to.
        return preview_from_html(url, &String::from_utf8_lossy(&body));
    }

    LinkPreview::failed(url, "Too many redirects".into())
}

/// Fetch previews for every cited URL at once. One dead link must not hold up the panel, so each
/// result carries its own error and the set always comes back in the order it was asked for.
pub async fn link_previews_impl(urls: Vec<String>) -> Vec<LinkPreview> {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return urls
                .iter()
                .map(|u| LinkPreview::failed(u, format!("Error building client: {}", e)))
                .collect()
        }
    };

    futures_util::future::join_all(urls.iter().map(|u| link_preview_impl(&client, u))).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_open_graph_tags() {
        let html = r#"<html><head>
            <title>Fallback</title>
            <meta property="og:title" content="Real Title">
            <meta property="og:description" content="A description.">
            <meta property="og:image" content="https://cdn.example.com/a.png">
            <meta property="og:site_name" content="Example">
        </head></html>"#;
        let p = preview_from_html("https://example.com/post", html);
        assert_eq!(p.title.as_deref(), Some("Real Title"));
        assert_eq!(p.description.as_deref(), Some("A description."));
        assert_eq!(p.image.as_deref(), Some("https://cdn.example.com/a.png"));
        assert_eq!(p.site_name.as_deref(), Some("Example"));
        assert!(p.error.is_none());
    }

    #[test]
    fn falls_back_to_title_tag_and_meta_description() {
        let html = r#"<html><head><title>  Just a Title </title>
            <meta name="description" content="Plain meta."></head></html>"#;
        let p = preview_from_html("https://example.com/", html);
        assert_eq!(p.title.as_deref(), Some("Just a Title"));
        assert_eq!(p.description.as_deref(), Some("Plain meta."));
    }

    /// `content` before `property`, single quotes, unquoted values — all seen in the wild. A
    /// positional pattern silently misses these and the panel shows a blank card.
    #[test]
    fn survives_real_world_attribute_order_and_quoting() {
        let html = r#"<head>
            <meta content="Backwards" property="og:title" />
            <meta property='og:description' content='Single quoted.'>
            <meta name=twitter:image content=https://x.test/i.png>
        </head>"#;
        let p = preview_from_html("https://x.test/", html);
        assert_eq!(p.title.as_deref(), Some("Backwards"));
        assert_eq!(p.description.as_deref(), Some("Single quoted."));
        assert_eq!(p.image.as_deref(), Some("https://x.test/i.png"));
    }

    #[test]
    fn resolves_relative_image_against_page_url() {
        let html = r#"<head><meta property="og:image" content="/static/card.png"></head>"#;
        let p = preview_from_html("https://example.com/blog/post", html);
        assert_eq!(p.image.as_deref(), Some("https://example.com/static/card.png"));
    }

    #[test]
    fn page_without_metadata_yields_empty_fields_not_an_error() {
        let p = preview_from_html("https://example.com/", "<html><body>hi</body></html>");
        assert!(p.title.is_none());
        assert!(p.description.is_none());
        assert!(p.image.is_none());
        assert!(p.error.is_none());
        assert_eq!(p.url, "https://example.com/");
    }

    #[test]
    fn long_descriptions_are_truncated() {
        let long = "x".repeat(600);
        let html = format!(r#"<head><meta property="og:description" content="{}"></head>"#, long);
        let p = preview_from_html("https://example.com/", &html);
        let desc = p.description.unwrap();
        assert!(desc.chars().count() <= MAX_DESC_CHARS + 1, "got {} chars", desc.chars().count());
        assert!(desc.ends_with('…'));
    }

    #[test]
    fn decodes_entities_in_metadata() {
        let html = r#"<head><meta property="og:title" content="Tom &amp; Jerry&#39;s"></head>"#;
        let p = preview_from_html("https://example.com/", html);
        assert_eq!(p.title.as_deref(), Some("Tom & Jerry's"));
    }

    /// The pre-existing `<(script|style)...</\1>` pattern was a backreference, which Rust's regex
    /// rejects at compile time — so this panicked on every HTML fetch.
    #[test]
    fn extract_text_does_not_panic_on_scripts_and_styles() {
        let html = "<html><head><style>a{color:red}</style><script>var x = 1 < 2;</script>\
            </head><body><p>Hello</p></body></html>";
        assert_eq!(extract_text(html), "Hello");
    }

    #[test]
    fn parse_order_honours_stored_sequence() {
        assert_eq!(
            parse_order(Some("ddg,searxng,parallel,exa")),
            vec![
                SearchProvider::Ddg,
                SearchProvider::Searxng,
                SearchProvider::Parallel,
                SearchProvider::Exa
            ]
        );
    }

    #[test]
    fn parse_order_fills_defaults_and_drops_junk() {
        // Unset, unknown ids, and duplicates must all still yield every provider exactly once.
        assert_eq!(parse_order(None), DEFAULT_ORDER.to_vec());
        assert_eq!(parse_order(Some("")), DEFAULT_ORDER.to_vec());
        assert_eq!(parse_order(Some("bogus")), DEFAULT_ORDER.to_vec());
        assert_eq!(
            parse_order(Some("ddg,ddg,nope,exa")),
            vec![
                SearchProvider::Ddg,
                SearchProvider::Exa,
                SearchProvider::Parallel,
                SearchProvider::Searxng
            ]
        );
    }
}
