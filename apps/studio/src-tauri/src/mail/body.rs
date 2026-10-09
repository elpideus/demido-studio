//! What a message is made of and how its pieces become text.
//!
//! The list needs little: who, what, when, and a short preview, read from a few header fields and
//! the first 2 KB of the body. Opening a message needs its readable body. Small messages are
//! fetched whole; for a large one, the server's BODYSTRUCTURE (summed up as a [`Plan`]) says which
//! parts hold the text and the inline images, and only those are fetched, never the attachments.
//! Decoding (transfer encodings, charsets, encoded words) is mail-parser's.

use std::borrow::Cow;

use async_imap::imap_proto::{BodyContentCommon, BodyStructure, ContentEncoding};
use base64::Engine;
use mail_parser::{MessageParser, MimeHeaders, PartType};
use serde::{Deserialize, Serialize};

/// The parts of a message worth fetching, by IMAP section ("1", "1.2"…).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// The root is multipart, so its parts can be fetched one by one.
    #[serde(default)]
    pub multipart: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Images the HTML shows through `cid:` links.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline: Vec<InlinePart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InlinePart {
    pub section: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub section: String,
    pub name: String,
    pub mime: String,
    /// Decoded size, estimated from the encoded one.
    pub size: u64,
}

impl Plan {
    pub fn from_structure(bs: &BodyStructure<'_>) -> Self {
        let mut plan = Plan {
            multipart: matches!(bs, BodyStructure::Multipart { .. }),
            ..Plan::default()
        };
        walk(bs, "", &mut plan);
        plan
    }
}

fn walk(bs: &BodyStructure<'_>, section: &str, plan: &mut Plan) {
    // A message that is not multipart has one part, numbered 1.
    let here = if section.is_empty() { "1" } else { section };
    match bs {
        BodyStructure::Multipart { bodies, .. } => {
            for (i, body) in bodies.iter().enumerate() {
                let child = if section.is_empty() {
                    (i + 1).to_string()
                } else {
                    format!("{section}.{}", i + 1)
                };
                walk(body, &child, plan);
            }
        }
        BodyStructure::Text { common, other, .. } => {
            let subtype = common.ty.subtype.to_ascii_lowercase();
            let attached = is_attachment(common) || file_name(common).is_some();
            if !attached && subtype == "html" && plan.html.is_none() {
                plan.html = Some(here.to_string());
            } else if !attached && subtype == "plain" && plan.text.is_none() {
                plan.text = Some(here.to_string());
            } else if attached || (subtype != "html" && subtype != "plain") {
                plan.attachments
                    .push(attachment(common, here, other.octets, &other.transfer_encoding));
            }
        }
        BodyStructure::Basic { common, other, .. } => {
            let image = common.ty.ty.eq_ignore_ascii_case("image");
            if image && other.id.is_some() && !is_attachment(common) {
                plan.inline.push(InlinePart {
                    section: here.to_string(),
                    size: decoded_size(other.octets, &other.transfer_encoding),
                });
            } else {
                plan.attachments
                    .push(attachment(common, here, other.octets, &other.transfer_encoding));
            }
        }
        BodyStructure::Message {
            common,
            other,
            envelope,
            ..
        } => {
            let mut a = attachment(common, here, other.octets, &other.transfer_encoding);
            if file_name(common).is_none()
                && let Some(subject) = envelope.subject.as_deref()
            {
                let subject = decode_words(&String::from_utf8_lossy(subject));
                if !subject.trim().is_empty() {
                    a.name = format!("{}.eml", sanitize_name(subject.trim()));
                }
            }
            plan.attachments.push(a);
        }
    }
}

fn is_attachment(common: &BodyContentCommon<'_>) -> bool {
    common
        .disposition
        .as_ref()
        .is_some_and(|d| d.ty.eq_ignore_ascii_case("attachment"))
}

fn attachment(
    common: &BodyContentCommon<'_>,
    section: &str,
    octets: u32,
    encoding: &ContentEncoding<'_>,
) -> Attachment {
    let mime = format!("{}/{}", common.ty.ty, common.ty.subtype).to_ascii_lowercase();
    let name = file_name(common).unwrap_or_else(|| default_name(&mime));
    Attachment {
        section: section.to_string(),
        name,
        mime,
        size: decoded_size(octets, encoding),
    }
}

fn decoded_size(octets: u32, encoding: &ContentEncoding<'_>) -> u64 {
    match encoding {
        ContentEncoding::Base64 => octets as u64 * 3 / 4,
        _ => octets as u64,
    }
}

fn default_name(mime: &str) -> String {
    match mime {
        "message/rfc822" => "message.eml".into(),
        "text/calendar" => "invite.ics".into(),
        _ => {
            let ext = mime.rsplit('/').next().unwrap_or("bin");
            let ext: String = ext.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
            format!("attachment.{}", if ext.is_empty() { "bin" } else { &ext })
        }
    }
}

/// The part's file name, decoded: servers hand the parameters over as they were written, which
/// may be RFC 2231 (`filename*=utf-8''…`, possibly split) or RFC 2047 (`=?UTF-8?B?…?=`). Rebuilding
/// the two headers and letting mail-parser read them covers every form.
fn file_name(common: &BodyContentCommon<'_>) -> Option<String> {
    let has_name = |params: &Option<Vec<(Cow<'_, str>, Cow<'_, str>)>>| {
        params.as_ref().is_some_and(|p| {
            p.iter().any(|(k, _)| {
                let k = k.to_ascii_lowercase();
                k.starts_with("filename") || k.starts_with("name")
            })
        })
    };
    let disposition = common.disposition.as_ref();
    if !has_name(&common.ty.params) && !disposition.is_some_and(|d| has_name(&d.params)) {
        return None;
    }
    let mut raw = String::new();
    if let Some(d) = disposition {
        raw.push_str(&format!(
            "Content-Disposition: {}{}\r\n",
            d.ty,
            header_params(&d.params)
        ));
    }
    raw.push_str(&format!(
        "Content-Type: {}/{}{}\r\n\r\n",
        common.ty.ty,
        common.ty.subtype,
        header_params(&common.ty.params)
    ));
    let message = MessageParser::default().parse_headers(raw.as_bytes())?;
    let name = message.root_part().attachment_name()?.trim();
    (!name.is_empty()).then(|| sanitize_name(name))
}

fn header_params(params: &Option<Vec<(Cow<'_, str>, Cow<'_, str>)>>) -> String {
    let mut out = String::new();
    for (key, value) in params.iter().flatten() {
        let extended = key.ends_with('*') && !value.contains([' ', '"', ';']);
        if extended {
            out.push_str(&format!("; {key}={value}"));
        } else {
            let value = value.replace('\\', "\\\\").replace('"', "\\\"");
            out.push_str(&format!("; {key}=\"{value}\""));
        }
    }
    out
}

/// A name safe to save under: no folders, no characters Windows refuses.
pub fn sanitize_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim();
    let cleaned: String = cleaned.chars().take(150).collect();
    if cleaned.is_empty() {
        "attachment".into()
    } else {
        cleaned
    }
}

/// Decodes RFC 2047 encoded words in a header value.
fn decode_words(raw: &str) -> String {
    let header = format!("Subject: {}\r\n\r\n", raw.replace(['\r', '\n'], " "));
    MessageParser::default()
        .parse_headers(header.as_bytes())
        .and_then(|m| m.subject().map(str::to_string))
        .unwrap_or_else(|| raw.to_string())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Addr {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub email: String,
}

impl Addr {
    /// `Name <email>`, or the email alone.
    pub fn display(&self) -> String {
        if self.name.is_empty() {
            self.email.clone()
        } else if self.email.is_empty() {
            self.name.clone()
        } else {
            format!("{} <{}>", self.name, self.email)
        }
    }
}

/// What the message list shows, read from a few header fields and the start of the body.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    /// Milliseconds since the epoch, from the Date header.
    pub date: Option<i64>,
    pub from: Option<Addr>,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub message_id: Option<String>,
    pub snippet: String,
}

pub const SNIPPET_CHARS: usize = 200;

/// Reads `header` (header fields, including Content-Type) and, when given, the first bytes of the
/// body that follows it, for a preview.
pub fn parse_header(header: &[u8], text_start: Option<&[u8]>) -> Header {
    let parser = MessageParser::default();
    let joined;
    let message = match text_start {
        Some(text) => {
            joined = [header, text].concat();
            parser.parse(&joined)
        }
        None => parser.parse_headers(header),
    };
    let Some(message) = message else {
        return Header::default();
    };
    let addrs = |a: Option<&mail_parser::Address<'_>>| -> Vec<Addr> {
        a.map(|a| {
            a.iter()
                .map(|x| Addr {
                    name: x.name().map(clean_line).unwrap_or_default(),
                    email: x.address().unwrap_or_default().trim().to_string(),
                })
                .filter(|x| !x.email.is_empty() || !x.name.is_empty())
                .collect()
        })
        .unwrap_or_default()
    };
    let snippet = if text_start.is_some() {
        let preview = message.body_preview(SNIPPET_CHARS * 2).unwrap_or_default();
        // The 2 KB cut leaves the part it ends in unterminated. That is harmless for text, but
        // base64 cut short may not decode and would show as raw letters: better no preview.
        let source = message
            .text_body
            .first()
            .or(message.html_body.first())
            .and_then(|i| message.parts.get(*i as usize));
        let raw = source.is_some_and(|p| {
            p.is_encoding_problem
                && p.content_transfer_encoding()
                    .is_some_and(|e| e.eq_ignore_ascii_case("base64"))
        });
        if raw { String::new() } else { snippet(&preview) }
    } else {
        String::new()
    };
    Header {
        date: message.date().map(|d| d.to_timestamp() * 1000),
        from: addrs(message.from()).into_iter().next(),
        to: addrs(message.to()),
        cc: addrs(message.cc()),
        subject: message.subject().map(clean_line).unwrap_or_default(),
        message_id: message.message_id().map(str::to_string),
        snippet,
    }
}

fn clean_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A one-line preview: whitespace collapsed, the invisible padding newsletters put after their
/// preheader text removed.
pub fn snippet(text: &str) -> String {
    let visible: String = text
        .chars()
        .map(|c| match c {
            '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{206F}' | '\u{FEFF}' | '\u{034F}' | '\u{00AD}' | '\u{180E}' => {
                ' '
            }
            c => c,
        })
        .collect();
    let line = clean_line(&visible);
    let mut out: String = line.chars().take(SNIPPET_CHARS).collect();
    if line.chars().count() > SNIPPET_CHARS {
        out.push('…');
    }
    out
}

/// A message's readable body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// The HTML, with inline (`cid:`) images embedded as data URLs. None for plain-text mail.
    pub html: Option<String>,
    /// The plain text: the text part, or the HTML's text when there is none.
    pub text: String,
}

/// Largest inline image embedded in the HTML.
const MAX_INLINE_IMAGE: usize = 5 * 1024 * 1024;

/// Reads a whole message, or a synthetic one made of fetched parts (see [`synthetic`]).
pub fn parse_body(raw: &[u8]) -> Body {
    let Some(message) = MessageParser::default().parse(raw) else {
        return Body::default();
    };
    let part = |id: &u32| message.parts.get(*id as usize);
    let has_html = message
        .html_body
        .iter()
        .filter_map(part)
        .any(|p| matches!(p.body, PartType::Html(_)));
    let mut html = None;
    if has_html {
        let mut out = String::new();
        for p in message.html_body.iter().filter_map(part) {
            match &p.body {
                PartType::Html(h) => out.push_str(h),
                PartType::Text(t) => out.push_str(&mail_parser::decoders::html::text_to_html(t)),
                PartType::Binary(b) | PartType::InlineBinary(b)
                    if p.content_id().is_none() && is_image(p) && b.len() <= MAX_INLINE_IMAGE =>
                {
                    out.push_str(&format!("<img src=\"{}\">", data_url(p, b)));
                }
                _ => {}
            }
        }
        html = Some(embed_cid_images(&message, out));
    }
    let mut text: Vec<String> = message
        .text_body
        .iter()
        .filter_map(part)
        .filter_map(|p| match &p.body {
            PartType::Text(t) => Some(t.to_string()),
            _ => None,
        })
        .collect();
    if text.is_empty()
        && let Some(h) = &html
    {
        text.push(mail_parser::decoders::html::html_to_text(h));
    }
    Body {
        html,
        text: tidy_text(&text.join("\n\n")),
    }
}

fn is_image(p: &mail_parser::MessagePart<'_>) -> bool {
    p.content_type()
        .is_some_and(|t| t.ctype().eq_ignore_ascii_case("image"))
}

fn data_url(p: &mail_parser::MessagePart<'_>, bytes: &[u8]) -> String {
    let mime = p
        .content_type()
        .map(|t| format!("{}/{}", t.ctype(), t.subtype().unwrap_or("png")))
        .unwrap_or_else(|| "image/png".into())
        .to_ascii_lowercase();
    // Only image types become data URLs: nothing else may run or render as a document.
    let mime = if mime.starts_with("image/") && !mime.contains("svg") {
        mime
    } else {
        "image/png".into()
    };
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

fn embed_cid_images(message: &mail_parser::Message<'_>, mut html: String) -> String {
    if !html.contains("cid:") && !html.contains("CID:") {
        return html;
    }
    for p in &message.parts {
        let (Some(cid), PartType::Binary(bytes) | PartType::InlineBinary(bytes)) = (p.content_id(), &p.body) else {
            continue;
        };
        if !is_image(p) || bytes.len() > MAX_INLINE_IMAGE {
            continue;
        }
        let cid = cid.trim().trim_start_matches('<').trim_end_matches('>');
        if cid.is_empty() {
            continue;
        }
        let url = data_url(p, bytes);
        for prefix in ["cid:", "CID:", "Cid:"] {
            html = html.replace(&format!("{prefix}{cid}"), &url);
        }
    }
    html
}

/// Trailing spaces removed and runs of blank lines shortened to one.
fn tidy_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

const BOUNDARY: &str = "demido-mail-part-0b6e1c";

/// A multipart message built from fetched parts, each its MIME header and its body, so a large
/// message's text can be read without fetching its attachments.
pub fn synthetic(parts: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut out =
        format!("MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"{BOUNDARY}\"\r\n\r\n").into_bytes();
    for (header, body) in parts {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        out.extend_from_slice(&header_block(header));
        out.extend_from_slice(body);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    out
}

/// A header block ending in its blank line, whichever way the server sent it.
fn header_block(header: &[u8]) -> Vec<u8> {
    let mut h = header.to_vec();
    while h.last().is_some_and(|b| *b == b'\r' || *b == b'\n') {
        h.pop();
    }
    h.extend_from_slice(b"\r\n\r\n");
    h
}

/// One part's decoded content, from its MIME header and its body as fetched.
pub fn part_contents(mime_header: &[u8], body: &[u8]) -> Vec<u8> {
    let raw = [header_block(mime_header).as_slice(), body].concat();
    match MessageParser::default().parse(&raw) {
        Some(message) => message.root_part().contents().to_vec(),
        None => body.to_vec(),
    }
}

/// The numbers of a section such as "1.2".
pub fn section_path(section: &str) -> Option<Vec<u32>> {
    section.split('.').map(|n| n.parse().ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn structure(raw: &'static str) -> BodyStructure<'static> {
        let line = format!("* 1 FETCH (BODYSTRUCTURE {raw})\r\n");
        let leaked: &'static [u8] = Box::leak(line.into_bytes().into_boxed_slice());
        match async_imap::imap_proto::Response::parse(leaked).unwrap().1 {
            async_imap::imap_proto::Response::Fetch(_, attrs) => attrs
                .into_iter()
                .find_map(|a| match a {
                    async_imap::imap_proto::AttributeValue::BodyStructure(b) => Some(b),
                    _ => None,
                })
                .unwrap(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn plans_a_message_with_alternatives_an_inline_image_and_attachments() {
        let bs = structure(concat!(
            r#"((("TEXT" "PLAIN" ("CHARSET" "UTF-8") NIL NIL "QUOTED-PRINTABLE" 120 4 NIL NIL NIL)"#,
            r#"("TEXT" "HTML" ("CHARSET" "UTF-8") NIL NIL "QUOTED-PRINTABLE" 900 20 NIL NIL NIL) "ALTERNATIVE" ("BOUNDARY" "b2") NIL NIL)"#,
            r#"("IMAGE" "PNG" ("NAME" "logo.png") "<logo@x>" NIL "BASE64" 4000 NIL ("INLINE" ("FILENAME" "logo.png")) NIL)"#,
            r#"("APPLICATION" "PDF" ("NAME" "=?UTF-8?B?RmF0dHVyYSDigqwucGRm?=") NIL NIL "BASE64" 40000 NIL ("ATTACHMENT" ("FILENAME" "=?UTF-8?B?RmF0dHVyYSDigqwucGRm?=")) NIL)"#,
            r#"("APPLICATION" "OCTET-STREAM" NIL NIL NIL "BASE64" 400 NIL ("ATTACHMENT" ("FILENAME*" "utf-8''na%C3%AFve%20notes.txt")) NIL)"#,
            r#" "MIXED" ("BOUNDARY" "b1") NIL NIL)"#,
        ));
        let plan = Plan::from_structure(&bs);
        assert!(plan.multipart);
        assert_eq!(plan.text.as_deref(), Some("1.1"));
        assert_eq!(plan.html.as_deref(), Some("1.2"));
        assert_eq!(
            plan.inline,
            vec![InlinePart {
                section: "2".into(),
                size: 3000
            }]
        );
        let names: Vec<&str> = plan.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Fattura €.pdf", "naïve notes.txt"]);
        assert_eq!(plan.attachments[0].section, "3");
        assert_eq!(plan.attachments[0].mime, "application/pdf");
        assert_eq!(plan.attachments[0].size, 30000);
    }

    #[test]
    fn a_single_part_message_is_section_one() {
        let bs = structure(r#"("TEXT" "HTML" ("CHARSET" "UTF-8") NIL NIL "7BIT" 300 10 NIL NIL NIL)"#);
        let plan = Plan::from_structure(&bs);
        assert!(!plan.multipart);
        assert_eq!(plan.html.as_deref(), Some("1"));
        assert!(plan.attachments.is_empty());
    }

    #[test]
    fn reads_the_list_fields_and_a_preview() {
        let header = b"Date: Tue, 06 Oct 2026 09:30:00 +0200\r\nFrom: =?UTF-8?Q?Jos=C3=A9?= <jose@example.com>\r\nTo: a@example.com, \"B\" <b@example.com>\r\nSubject: =?UTF-8?B?Q2lhbyDwn5GL?=\r\nMessage-ID: <abc@example.com>\r\nContent-Type: multipart/alternative; boundary=\"zz\"\r\n\r\n";
        let text = b"--zz\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nHello =E2=80=94 the report\r\nis ready.=E2=80=8C=E2=80=8C\r\n--zz\r\nContent-Type: text/html\r\n\r\n<p>Hel";
        let h = parse_header(header, Some(text));
        assert_eq!(h.subject, "Ciao 👋");
        assert_eq!(h.from.as_ref().unwrap().name, "José");
        assert_eq!(h.from.as_ref().unwrap().email, "jose@example.com");
        assert_eq!(h.to.len(), 2);
        assert_eq!(h.to[1].display(), "B <b@example.com>");
        assert_eq!(h.message_id.as_deref(), Some("abc@example.com"));
        assert_eq!(h.date, Some(1_791_271_800_000));
        assert_eq!(h.snippet, "Hello — the report is ready.");
    }

    #[test]
    fn a_preview_cut_inside_base64_is_dropped_not_shown_raw() {
        let header =
            b"Subject: x\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n";
        let h = parse_header(header, Some(b"SGVsbG8gdGhlcmUsIGhvdyBhcmUgeW91IHRvZGF5Pw=="));
        assert_eq!(h.snippet, "Hello there, how are you today?");
        let cut = parse_header(header, Some(b"SGVsbG8gdGhlcmUsIGhvdyBhcmUgeW91IHRvZG"));
        assert!(!cut.snippet.contains("SGVsbG8"), "{}", cut.snippet);
    }

    #[test]
    fn embeds_cid_images_and_derives_text_from_html() {
        let raw = concat!(
            "Content-Type: multipart/related; boundary=\"r\"\r\n\r\n",
            "--r\r\nContent-Type: text/html; charset=utf-8\r\n\r\n",
            "<p>Hi <b>there</b></p><img src=\"cid:pic@x\">\r\n",
            "--r\r\nContent-Type: image/png\r\nContent-ID: <pic@x>\r\nContent-Transfer-Encoding: base64\r\n\r\niVBORw0KGgo=\r\n",
            "--r--\r\n"
        );
        let body = parse_body(raw.as_bytes());
        let html = body.html.unwrap();
        assert!(html.contains("src=\"data:image/png;base64,iVBORw0KGgo=\""), "{html}");
        assert!(!html.contains("cid:"));
        assert_eq!(body.text, "Hi there");
    }

    #[test]
    fn plain_text_mail_has_no_html() {
        let body = parse_body(b"Subject: x\r\nContent-Type: text/plain\r\n\r\nLine one\r\n\r\n\r\n\r\nLine two   \r\n");
        assert_eq!(body.html, None);
        assert_eq!(body.text, "Line one\n\nLine two");
    }

    #[test]
    fn reads_fetched_parts_without_the_rest() {
        let html_mime =
            b"Content-Type: text/html; charset=iso-8859-1\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\n";
        let raw = synthetic(&[(html_mime, b"<p>Caf=E9</p>")]);
        let body = parse_body(&raw);
        assert_eq!(body.html.as_deref(), Some("<p>Café</p>"));
        assert_eq!(body.text, "Café");
    }

    #[test]
    fn decodes_one_part() {
        let mime = b"Content-Type: application/pdf\r\nContent-Transfer-Encoding: base64\r\n";
        assert_eq!(part_contents(mime, b"JVBERi0x\r\nLjQK"), b"%PDF-1.4\n");
    }

    #[test]
    fn snippets_drop_invisible_padding() {
        assert_eq!(
            snippet("Sale  ends\u{200c} \u{034f} \u{200c}today\n\n"),
            "Sale ends today"
        );
        assert_eq!(snippet(&"a".repeat(300)).chars().count(), SNIPPET_CHARS + 1);
    }

    #[test]
    fn names_are_safe_to_save() {
        assert_eq!(sanitize_name("../a/b:c?.pdf"), "_a_b_c_.pdf");
        assert_eq!(sanitize_name("  "), "attachment");
        assert_eq!(section_path("1.2.3"), Some(vec![1, 2, 3]));
        assert_eq!(section_path("1.x"), None);
    }
}
