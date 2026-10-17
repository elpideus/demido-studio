//! IMAP's modified UTF-7 (RFC 3501 §5.1.3), the encoding servers use for folder names outside
//! ASCII: `Posta in arrivo` stays as it is, `Entwürfe` arrives as `Entw&APw-rfe`.

use base64::Engine;
use base64::alphabet::Alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// A name as people read it. Anything that is not valid modified UTF-7 is kept as it came.
pub fn decode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('-') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let chunk = &after[..end];
        if chunk.is_empty() {
            out.push('&');
        } else if let Some(text) = decode_chunk(chunk) {
            out.push_str(&text);
        } else {
            out.push_str(&rest[start..start + end + 2]);
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// A name as the server wants it: printable ASCII as it is (`&` as `&-`), every run of other
/// characters as UTF-16 in base64 between `&` and `-`.
pub fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut run: Vec<u16> = Vec::new();
    let flush = |run: &mut Vec<u16>, out: &mut String| {
        if run.is_empty() {
            return;
        }
        let bytes: Vec<u8> = run.iter().flat_map(|u| u.to_be_bytes()).collect();
        out.push('&');
        out.push_str(&engine().encode(bytes));
        out.push('-');
        run.clear();
    };
    for c in name.chars() {
        if (' '..='~').contains(&c) {
            flush(&mut run, &mut out);
            if c == '&' {
                out.push_str("&-");
            } else {
                out.push(c);
            }
        } else {
            run.extend(c.encode_utf16(&mut [0; 2]).iter());
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Base64 with ',' in place of '/' and no padding.
fn engine() -> GeneralPurpose {
    let alphabet =
        Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,").expect("a valid alphabet");
    GeneralPurpose::new(
        &alphabet,
        GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone),
    )
}

fn decode_chunk(chunk: &str) -> Option<String> {
    let bytes = engine().decode(chunk).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let (pairs, _) = bytes.as_chunks::<2>();
    let units = pairs.iter().map(|&pair| u16::from_be_bytes(pair));
    char::decode_utf16(units).collect::<Result<String, _>>().ok()
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn decodes_folder_names() {
        assert_eq!(decode("INBOX"), "INBOX");
        assert_eq!(decode("Entw&APw-rfe"), "Entwürfe");
        assert_eq!(decode("[Gmail]/Posta inviata"), "[Gmail]/Posta inviata");
        assert_eq!(decode("Tom &- Jerry"), "Tom & Jerry");
        assert_eq!(decode("&ZeVnLIqe-"), "日本語");
        assert_eq!(decode("&2D3eAA-"), "😀");
    }

    #[test]
    fn encodes_folder_names() {
        for (plain, coded) in [
            ("INBOX", "INBOX"),
            ("Entwürfe", "Entw&APw-rfe"),
            ("Tom & Jerry", "Tom &- Jerry"),
            ("日本語", "&ZeVnLIqe-"),
            ("😀", "&2D3eAA-"),
            ("Già fatto/Più", "Gi&AOA- fatto/Pi&APk-"),
        ] {
            assert_eq!(encode(plain), coded);
            assert_eq!(decode(coded), plain);
        }
    }

    #[test]
    fn keeps_what_is_not_utf7() {
        assert_eq!(decode("a&b"), "a&b");
        assert_eq!(decode("x&!!!-y"), "x&!!!-y");
    }
}
