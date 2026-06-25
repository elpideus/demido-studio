use regex::Regex;
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
fn extract_text(html: &str) -> String {
    let script_re = Regex::new(r"(?si)<(script|style)[^>]*>.*?</\1>").unwrap();
    let without = script_re.replace_all(html, " ");
    strip_html(&without)
}

pub async fn web_search_impl(query: &str, page: u64) -> String {
    let client = match reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error building client: {}", e),
    };

    let offset = page * 20;
    let mut params: Vec<(&str, String)> = vec![("q", query.to_string())];
    if offset > 0 {
        params.push(("s", offset.to_string()));
        params.push(("dc", (offset + 1).to_string()));
    }

    let resp = match client
        .post("https://html.duckduckgo.com/html/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&params)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return format!("Search request error: {}", e),
    };

    let html = match resp.text().await {
        Ok(b) => b,
        Err(e) => return format!("Failed to read response: {}", e),
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
        return format!("No results found for \"{}\".", query);
    }

    let mut lines = Vec::new();
    for (i, (url, title)) in urls.iter().zip(titles.iter()).enumerate() {
        let snippet = snippets.get(i).map(|s| s.as_str()).unwrap_or("");
        lines.push(format!("{}. {}\n   {}\n   {}", i + 1, title, url, snippet));
    }
    lines.join("\n\n")
}

pub async fn web_fetch_impl(url: &str) -> String {
    if let Err(e) = check_ssrf(url) {
        return format!("SSRF blocked: {}", e);
    }

    // Disable auto-redirect so we can re-validate each hop against SSRF guard.
    let client = match reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; Demido/1.0)")
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error: {}", e),
    };

    let mut current_url = url.to_string();
    const MAX_REDIRECTS: usize = 10;

    for _ in 0..MAX_REDIRECTS {
        let resp = match client.get(&current_url).send().await {
            Ok(r) => r,
            Err(e) => return format!("Fetch error: {}", e),
        };

        let status = resp.status();

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

        if !status.is_success() {
            return format!("HTTP {}", status.as_u16());
        }

        const MAX: usize = 20_000;
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => return format!("Read error: {}", e),
        };

        let text = if content_type.contains("html") {
            extract_text(&body)
        } else {
            body
        };

        return if text.len() > MAX {
            format!("{}\n[truncated at 20k chars]", &text[..MAX])
        } else {
            text
        };
    }

    "Too many redirects".into()
}
