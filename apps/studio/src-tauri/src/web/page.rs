//! A web page as the model reads it: its readable part, in Markdown.
//!
//! Most pages are an article inside menus, sidebars and footers. Mozilla's Readability (the
//! algorithm behind Firefox's reader view, here `dom_smoothie`) finds the article; a page it
//! does not suit (a listing, a search page, documentation with its own navigation) is read whole
//! instead, minus what never holds text for a reader. The same reading serves pages fetched over
//! HTTP and pages the built-in browser rendered, which hands over its live HTML.
//!
//! It also tells two kinds of page that hold nothing to read yet: a bot check (Cloudflare's
//! "Just a moment...", a captcha) and an empty shell that JavaScript fills in a browser.

use std::sync::LazyLock;

use dom_query::Document;
use dom_smoothie::{Config, Readability, TextMode};
use regex::Regex;

/// The most HTML read from one page: beyond it a page is mostly scripts and inline images.
pub const MAX_HTML_BYTES: usize = 5 * 1024 * 1024;

/// Below this many characters of visible text, a page that runs scripts is taken for a shell
/// that JavaScript fills in a browser.
const SHELL_CHARS: usize = 400;

/// Elements that never hold text for a reader.
const NOISE: &str = "script, style, noscript, template, svg, canvas, iframe, object, embed, \
                     link, meta, button, select, input, textarea, [hidden], [aria-hidden=true]";
/// Page furniture, dropped when a page is read whole.
const FURNITURE: &str = "nav, aside, footer, [role=navigation], [role=banner], [role=contentinfo], \
                         [role=search], [role=dialog], [aria-modal=true]";

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub url: String,
    pub title: String,
    /// The readable text, in Markdown with absolute links.
    pub text: String,
    pub site: Option<String>,
    pub byline: Option<String>,
    pub published: Option<String>,
    /// So little text beside its scripts that the page is built in a browser.
    pub shell: bool,
}

/// Reads `html`, served at `url`.
pub fn readable(url: &str, html: &str) -> Page {
    let html = cap(html, MAX_HTML_BYTES);
    let whole = whole_page(url, html);
    let scripted = html.contains("<script");
    let config = Config {
        text_mode: TextMode::Markdown,
        ..Default::default()
    };
    let article = Readability::new(html, Some(url), Some(config))
        .ok()
        .and_then(|mut r| r.parse().ok())
        .filter(|a| !a.text_content.trim().is_empty());
    let visible = visible_chars(&whole.text);
    let shell = scripted && visible < SHELL_CHARS;
    match article {
        // The article, unless it is a small part of a page with much more to read: a listing
        // Readability took one item of.
        Some(a) if visible_chars(&a.text_content) * 10 >= visible * 3 || visible > 400_000 => {
            let text = tidy(&a.text_content);
            Page {
                url: url.to_string(),
                title: first_nonempty(&[&a.title, &whole.title]),
                text,
                site: a.site_name.filter(|s| !s.trim().is_empty()),
                byline: a.byline.filter(|s| !s.trim().is_empty()),
                published: a.published_time.filter(|s| !s.trim().is_empty()),
                shell,
            }
        }
        _ => Page { shell, ..whole },
    }
}

/// The whole page minus noise and furniture.
fn whole_page(url: &str, html: &str) -> Page {
    let doc = Document::from(html);
    let title = clean_line(&doc.select("head title").first().text());
    doc.select(NOISE).remove();
    absolutize(&doc, url);
    let body = doc.select("body");
    let furnished = body.nodes().first().map(|n| n.md(None).to_string()).unwrap_or_default();
    doc.select(FURNITURE).remove();
    let bare = body.nodes().first().map(|n| n.md(None).to_string()).unwrap_or_default();
    // A page that is all furniture (a site map, a menu page) keeps it.
    let text = if visible_chars(&bare) >= SHELL_CHARS || visible_chars(&furnished) < SHELL_CHARS {
        bare
    } else {
        furnished
    };
    Page {
        url: url.to_string(),
        title,
        text: tidy(&text),
        site: None,
        byline: None,
        published: None,
        shell: false,
    }
}

/// Makes links and images absolute, so the model can follow them, and drops inline images.
fn absolutize(doc: &Document, url: &str) {
    let Ok(base) = url::Url::parse(url) else {
        return;
    };
    let base = doc
        .select("head base[href]")
        .attr("href")
        .and_then(|b| base.join(&b).ok())
        .unwrap_or(base);
    for (sel, attr) in [("a[href]", "href"), ("img[src]", "src")] {
        for el in doc.select(sel).iter() {
            let Some(value) = el.attr(attr) else { continue };
            let value = value.trim();
            if value.starts_with("data:") {
                el.remove();
            } else if value.starts_with("javascript:") || value.starts_with('#') {
                el.remove_attr(attr);
            } else if let Ok(abs) = base.join(value) {
                el.set_attr(attr, abs.as_str());
            }
        }
    }
}

/// Characters of text a reader sees, without Markdown's link targets.
fn visible_chars(md: &str) -> usize {
    static TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]\([^)]*\)").unwrap());
    TARGET
        .replace_all(md, "]")
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '#' | '*' | '_' | '|' | '-' | '[' | ']' | '!' | '>'))
        .count()
}

/// Tidies Markdown for reading: no trailing spaces, no runs of blank lines, no inline images,
/// no table rows with nothing in them.
pub(crate) fn tidy(md: &str) -> String {
    static INLINE_IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[[^\]]*\]\(data:[^)]*\)").unwrap());
    let md = INLINE_IMAGE.replace_all(md, "");
    let mut out = String::with_capacity(md.len());
    let mut blank = 0;
    for line in md.lines() {
        let line = line.trim_end();
        if line.starts_with('|') && !line.contains('-') && line.chars().all(|c| matches!(c, '|' | ' ')) {
            continue;
        }
        if line.is_empty() {
            blank += 1;
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank > 0 { "\n\n" } else { "\n" });
        }
        blank = 0;
        out.push_str(line);
    }
    out
}

/// What keeps a page from being read until a person (or a real browser) passes a check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Check {
    /// Cloudflare's "Just a moment...": a browser usually passes it alone in a few seconds.
    Cloudflare,
    /// Another browser check that usually passes alone (Anubis, DDoS-Guard, Vercel).
    Wait,
    /// A captcha or "press and hold" check, which needs the person.
    Captcha,
}

impl Check {
    pub fn label(self) -> &'static str {
        match self {
            Check::Cloudflare => "Cloudflare's browser check",
            Check::Wait => "a browser check",
            Check::Captcha => "a captcha",
        }
    }
}

/// The parts of a response that tell a bot check.
pub struct Response<'a> {
    pub status: u16,
    pub server: &'a str,
    /// Cloudflare's `cf-mitigated` header.
    pub mitigated: bool,
    pub html: &'a str,
}

/// The check `r` shows instead of the page, if any. Phrases about captchas count only on a page
/// with little else in it, so an article about captchas is not taken for one.
pub fn check(r: &Response) -> Option<Check> {
    let lower = cap(r.html, 200_000).to_ascii_lowercase();
    let title = title_of(&lower);
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    let titled = |needles: &[&str]| needles.iter().any(|n| title.contains(n));
    let refused = matches!(r.status, 403 | 429 | 503);
    if r.mitigated
        || titled(&[
            "just a moment",
            "attention required! | cloudflare",
            "please wait... | cloudflare",
        ])
        || (refused && r.server.eq_ignore_ascii_case("cloudflare") && has(&["challenge-platform", "cf-chl", "_cf_chl"]))
    {
        return Some(Check::Cloudflare);
    }
    if r.html.len() > 60_000 {
        return None;
    }
    let text = text_chars(&lower);
    if has(&[
        "captcha-delivery.com",
        "px-captcha",
        "_pxcaptcha",
        "press & hold",
        "press and hold",
    ]) || titled(&[
        "captcha",
        "robot check",
        "are you a robot",
        "verify you are human",
        "human verification",
        "security check",
    ]) || has(&["enable js and disable any ad blocker"])
        || (has(&[
            "g-recaptcha",
            "h-captcha",
            "hcaptcha.com/1/api",
            "cf-turnstile",
            "recaptcha/api.js",
        ]) && (refused || text < 1_500))
    {
        return Some(Check::Captcha);
    }
    if titled(&[
        "verifying your request",
        "checking your browser",
        "ddos-guard",
        "vercel security checkpoint",
        "making sure you're not a bot",
    ]) || has(&["anubis_challenge", "id=\"anubis", "within.website/x/cmd/anubis"])
    {
        return Some(Check::Wait);
    }
    None
}

/// Characters of text in HTML, roughly: without scripts, styles and tags.
fn text_chars(html: &str) -> usize {
    static MARKUP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<script.*?</script>|<style.*?</style>|<[^>]*>").unwrap());
    MARKUP
        .replace_all(html, " ")
        .chars()
        .filter(|c| !c.is_whitespace())
        .count()
}

/// The `<title>` of lower-cased HTML.
fn title_of(lower: &str) -> String {
    let Some(start) = lower.find("<title") else {
        return String::new();
    };
    let rest = &lower[start..];
    let Some(open) = rest.find('>') else {
        return String::new();
    };
    let rest = &rest[open + 1..];
    rest[..rest.find("</title").unwrap_or(rest.len().min(300))]
        .trim()
        .to_string()
}

/// At most `max` bytes of `s`, cut at a character boundary.
pub(crate) fn cap(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// `s` on one line, its whitespace collapsed.
pub(crate) fn clean_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first_nonempty(options: &[&str]) -> String {
    options
        .iter()
        .map(|s| clean_line(s))
        .find(|s| !s.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLE: &str = r#"<!doctype html><html><head><title>Why otters hold hands | Wild Notes</title>
        <meta property="og:site_name" content="Wild Notes"></head><body>
        <nav><a href="/">Home</a> <a href="/about">About</a> <a href="/shop">Shop</a></nav>
        <article><h1>Why otters hold hands</h1>
        <p>Sea otters sleep floating on their backs, and a sleeping otter drifts. Pairs and groups
        hold paws so the current does not carry them apart while they rest, a habit that biologists
        call rafting. Some rafts hold more than a hundred animals.</p>
        <p>Mothers keep pups on their chests and, when they dive for food, wrap them in kelp so they
        stay put. The kelp works as an anchor for adults too: an otter alone rolls itself in a frond
        before it falls asleep. See <a href="/kelp">our kelp guide</a> and
        <a href="https://example.org/otters">the survey</a>.</p>
        <p>Holding hands also keeps the group warm and together when storms come in, which matters
        for an animal that has no blubber and relies on dense fur and constant grooming instead.</p>
        <img src="data:image/png;base64,AAAA" alt="">
        </article>
        <footer>© Wild Notes · <a href="/privacy">Privacy</a></footer></body></html>"#;

    #[test]
    fn an_article_is_read_without_its_page() {
        let page = readable("https://wild.example/otters/hands", ARTICLE);
        assert!(page.title.contains("otters hold hands"), "{}", page.title);
        assert!(page.text.contains("rafting"));
        assert!(page.text.contains("(https://wild.example/kelp)"), "{}", page.text);
        assert!(!page.text.contains("Privacy"));
        assert!(!page.text.contains("data:"));
        assert!(!page.text.contains("\n\n\n"));
        assert!(!page.shell);
    }

    #[test]
    fn a_listing_is_read_whole() {
        let items: String = (1..=30)
            .map(|i| format!("<li><a href=\"/item/{i}\">Story number {i} about something</a> <span>{i} points by someone</span></li>"))
            .collect();
        let html = format!("<html><head><title>Front page</title></head><body><ol>{items}</ol></body></html>");
        let page = readable("https://news.example/", &html);
        assert!(page.text.contains("Story number 1 "), "{}", page.text);
        assert!(page.text.contains("Story number 30"));
        assert!(page.text.contains("https://news.example/item/30"));
    }

    #[test]
    fn a_page_built_by_scripts_is_a_shell() {
        let html = r#"<html><head><title>reddit</title><script src="/app.js"></script></head>
            <body><div id="root"></div><noscript>Please enable JavaScript to continue.</noscript>
            <script>window.__INITIAL__ = {"a": 1};</script></body></html>"#;
        assert!(readable("https://www.reddit.com/r/rust/", html).shell);
        // A short page without scripts is just short.
        let plain = "<html><head><title>Example Domain</title></head><body><h1>Example Domain</h1><p>This domain is for use in illustrative examples.</p></body></html>";
        let page = readable("https://example.com/", plain);
        assert!(!page.shell);
        assert!(page.text.contains("illustrative examples"));
    }

    fn checked(status: u16, server: &str, mitigated: bool, html: &str) -> Option<Check> {
        check(&Response {
            status,
            server,
            mitigated,
            html,
        })
    }

    #[test]
    fn bot_checks_are_told_from_pages() {
        let cf = r#"<!DOCTYPE html><html lang="en-US"><head><title>Just a moment...</title></head><body><noscript>Enable JavaScript and cookies to continue</noscript><script>window._cf_chl_opt={cvId: '3'};</script></body></html>"#;
        assert_eq!(checked(403, "cloudflare", true, cf), Some(Check::Cloudflare));
        assert_eq!(checked(403, "cloudflare", false, cf), Some(Check::Cloudflare));
        let anubis = r#"<html><head><title>Verifying your request</title></head><body><div id="anubis_challenge"></div></body></html>"#;
        assert_eq!(checked(200, "", false, anubis), Some(Check::Wait));
        let datadome = r#"<html><head><title>g2.com</title></head><body><p>Please enable JS and disable any ad blocker</p><script src="https://ct.captcha-delivery.com/c.js"></script></body></html>"#;
        assert_eq!(checked(403, "", false, datadome), Some(Check::Captcha));
        let amazon =
            "<html><head><title>Robot Check</title></head><body><form>Type the characters you see</form></body></html>";
        assert_eq!(checked(200, "", false, amazon), Some(Check::Captcha));
    }

    #[test]
    fn pages_about_captchas_are_pages() {
        assert_eq!(checked(200, "nginx", false, ARTICLE), None);
        let contact = format!(
            "<html><head><title>Contact us</title></head><body>{}<form><div class=\"g-recaptcha\"></div></form></body></html>",
            "<p>We answer every message within two working days, so please tell us what you need.</p>".repeat(30)
        );
        assert_eq!(checked(200, "nginx", false, &contact), None);
        let essay = format!(
            "<html><head><title>A history of the CAPTCHA</title></head><body>{}</body></html>",
            "<p>The captcha was invented to tell computers and humans apart, and press and hold checks came later.</p>"
                .repeat(700)
        );
        assert_eq!(checked(200, "nginx", false, &essay), None);
    }

    #[test]
    fn tidy_leaves_readable_markdown() {
        let md = "# Title  \n\n\n\nText ![x](data:image/png;base64,AAA) here\n| | |\n|---|---|\n| a | b |\n\n\n";
        assert_eq!(tidy(md), "# Title\n\nText  here\n|---|---|\n| a | b |");
    }

    #[test]
    fn cap_cuts_at_a_character() {
        assert_eq!(cap("aé", 2), "a");
        assert_eq!(cap("abc", 10), "abc");
    }
}
