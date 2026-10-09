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

fn decode_chunk(chunk: &str) -> Option<String> {
    // The standard alphabet with ',' in place of '/'.
    let alphabet = Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,").ok()?;
    let engine = GeneralPurpose::new(
        &alphabet,
        GeneralPurposeConfig::new()
            .with_decode_allow_trailing_bits(true)
            .with_decode_padding_mode(DecodePaddingMode::RequireNone),
    );
    let bytes = engine.decode(chunk).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let units = bytes.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]]));
    char::decode_utf16(units).collect::<Result<String, _>>().ok()
}

#[cfg(test)]
mod tests {
    use super::decode;

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
    fn keeps_what_is_not_utf7() {
        assert_eq!(decode("a&b"), "a&b");
        assert_eq!(decode("x&!!!-y"), "x&!!!-y");
    }
}
