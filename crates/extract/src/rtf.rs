//! Rich Text Format: the text of the document, without its control words, font and colour
//! tables, pictures or other hidden destinations.

/// Destinations whose content is not part of the text a person reads.
const HIDDEN: &[&str] = &[
    "fonttbl",
    "colortbl",
    "stylesheet",
    "info",
    "pict",
    "object",
    "themedata",
    "colorschememapping",
    "datastore",
    "latentstyles",
    "listtable",
    "listoverridetable",
    "rsidtbl",
    "generator",
    "xmlnstbl",
    "mmathPr",
    "header",
    "footer",
    "headerl",
    "headerr",
    "footerl",
    "footerr",
    "footnote",
    "fldinst",
    "bkmkstart",
    "bkmkend",
    "filetbl",
    "revtbl",
    "pgdsctbl",
    "wgrffmtfilter",
];

struct Group {
    hidden: bool,
    /// Characters to skip after a `\u` escape (`\ucN`), 1 by default.
    skip_after_unicode: usize,
}

/// The text of an RTF document. Code page bytes (`\'e8`) are read as Windows-1252.
pub(crate) fn to_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut stack = vec![Group {
        hidden: false,
        skip_after_unicode: 1,
    }];
    let mut skip = 0usize;
    let mut i = 0usize;
    let mut pending: Vec<u8> = Vec::new();
    let flush = |pending: &mut Vec<u8>, out: &mut String| {
        if !pending.is_empty() {
            out.push_str(&encoding_rs::WINDOWS_1252.decode_without_bom_handling(pending).0);
            pending.clear();
        }
    };
    while i < bytes.len() {
        let hidden = stack.last().is_some_and(|g| g.hidden);
        let b = bytes[i];
        match b {
            b'{' => {
                flush(&mut pending, &mut out);
                let parent = stack.last().map_or((false, 1), |g| (g.hidden, g.skip_after_unicode));
                // `{\*\dest ...}` marks a destination a reader may ignore.
                let starred = bytes.get(i + 1..i + 3) == Some(b"\\*");
                stack.push(Group {
                    hidden: parent.0 || starred,
                    skip_after_unicode: parent.1,
                });
                i += 1;
            }
            b'}' => {
                flush(&mut pending, &mut out);
                if stack.len() > 1 {
                    stack.pop();
                }
                i += 1;
            }
            b'\\' => {
                let Some(&next) = bytes.get(i + 1) else { break };
                if next == b'\'' {
                    // A code page byte: \'hh.
                    let hex = bytes.get(i + 2..i + 4).and_then(|h| std::str::from_utf8(h).ok());
                    if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                        if skip > 0 {
                            skip -= 1;
                        } else if !hidden {
                            pending.push(v);
                        }
                    }
                    i += 4;
                    continue;
                }
                if !next.is_ascii_alphabetic() {
                    // A control symbol: \\ \{ \} \~ \- \_ and others.
                    if !hidden {
                        flush(&mut pending, &mut out);
                        match next {
                            b'\\' | b'{' | b'}' => out.push(next as char),
                            b'~' => out.push('\u{a0}'),
                            b'_' => out.push('-'),
                            _ => {}
                        }
                    }
                    i += 2;
                    continue;
                }
                // A control word: letters, an optional signed number, an optional space.
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
                    j += 1;
                }
                let word = std::str::from_utf8(&bytes[start..j]).unwrap_or_default();
                let num_start = j;
                if j < bytes.len() && bytes[j] == b'-' {
                    j += 1;
                }
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let num: Option<i32> = std::str::from_utf8(&bytes[num_start..j])
                    .ok()
                    .and_then(|n| n.parse().ok());
                if j < bytes.len() && bytes[j] == b' ' {
                    j += 1;
                }
                i = j;
                if HIDDEN.contains(&word) {
                    if let Some(g) = stack.last_mut() {
                        g.hidden = true;
                    }
                    continue;
                }
                if hidden {
                    continue;
                }
                flush(&mut pending, &mut out);
                match word {
                    "par" | "line" | "sect" | "page" | "row" => out.push('\n'),
                    "tab" | "cell" => out.push('\t'),
                    "uc" => {
                        if let Some(g) = stack.last_mut() {
                            g.skip_after_unicode = num.unwrap_or(1).max(0) as usize;
                        }
                    }
                    "u" => {
                        if let Some(n) = num {
                            let code = if n < 0 { n + 65536 } else { n } as u32;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                            skip = stack.last().map_or(1, |g| g.skip_after_unicode);
                        }
                    }
                    "emdash" => out.push('—'),
                    "endash" => out.push('–'),
                    "bullet" => out.push('•'),
                    "lquote" => out.push('‘'),
                    "rquote" => out.push('’'),
                    "ldblquote" => out.push('“'),
                    "rdblquote" => out.push('”'),
                    _ => {}
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                if skip > 0 {
                    skip -= 1;
                } else if !hidden {
                    pending.push(b);
                }
                i += 1;
            }
        }
    }
    flush(&mut pending, &mut out);
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut text = lines.join("\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_without_the_markup() {
        let rtf = br"{\rtf1\ansi\deff0{\fonttbl{\f0 Times New Roman;}}{\colortbl;\red0\green0\blue0;}
{\*\generator Riched20;}\f0\fs24 Hello \b world\b0 !\par
Second line with caff\'e8 and \u8364?uro.\par
{\*\bkmkstart x}Tab\tab end \{braces\}}";
        assert_eq!(
            to_text(rtf),
            "Hello world!\nSecond line with caffè and €uro.\nTab\tend {braces}"
        );
    }

    #[test]
    fn broken_rtf_is_not_a_crash() {
        assert_eq!(to_text(br"{\rtf1 unterminated \'z"), "unterminated");
        assert_eq!(to_text(b"}}}{{{\\"), "");
    }
}
