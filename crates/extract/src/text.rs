//! Turning the bytes of a text file into a string.

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const UTF16LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16BE_BOM: &[u8] = &[0xFE, 0xFF];

/// The file starts with a UTF-8 or UTF-16 byte order mark.
pub(crate) fn has_bom(bytes: &[u8]) -> bool {
    bytes.starts_with(UTF8_BOM) || bytes.starts_with(UTF16LE_BOM) || bytes.starts_with(UTF16BE_BOM)
}

/// Decodes a text file: UTF-8 (a byte order mark is dropped), UTF-16 with a byte order mark, and
/// anything else as UTF-8 with bad bytes replaced.
///
/// A file with no UTF-8 at all beyond ASCII, and bytes that are not UTF-8, is an old Windows
/// ("ANSI") file: it is read as Windows-1252, so its accented letters survive.
pub(crate) fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(UTF8_BOM) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(UTF16LE_BOM) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(UTF16BE_BOM) {
        return utf16(rest, u16::from_be_bytes);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    let has_utf8 = bytes.utf8_chunks().any(|chunk| !chunk.valid().is_ascii());
    if has_utf8 {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        encoding_rs::WINDOWS_1252
            .decode_without_bom_handling(bytes)
            .0
            .into_owned()
    }
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
    let (pairs, _) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|&pair| word(pair)).collect();
    String::from_utf16_lossy(&units)
}

/// Whether a file of unknown type is text: a byte order mark, or no NUL bytes and almost no
/// other control characters (binary formats are full of both).
pub(crate) fn looks_like_text(bytes: &[u8]) -> bool {
    if has_bom(bytes) {
        return true;
    }
    if bytes.contains(&0) {
        return false;
    }
    let controls = bytes
        .iter()
        .filter(|&&b| (b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C | 0x1B)) || b == 0x7F)
        .count();
    controls * 100 <= bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_order_marks() {
        assert_eq!(decode(b"\xEF\xBB\xBFhello"), "hello");
        assert_eq!(decode(&[0xFF, 0xFE, b'h', 0, 0xE9, 0]), "hé");
        assert_eq!(decode(&[0xFE, 0xFF, 0, b'h', 0, 0xE9]), "hé");
    }

    #[test]
    fn windows_1252_and_broken_utf8() {
        // "caffè" saved by an old Windows editor.
        assert_eq!(decode(b"caff\xE8 \x80"), "caffè €");
        // Real UTF-8 with one bad byte keeps its UTF-8.
        assert_eq!(decode("perché \u{00E8}".as_bytes()), "perché è");
        assert_eq!(decode(b"perch\xC3\xA9 \xFF"), "perché \u{FFFD}");
    }

    #[test]
    fn text_or_binary() {
        assert!(looks_like_text(b"plain words\r\n\ttabbed"));
        assert!(looks_like_text(b""));
        assert!(!looks_like_text(b"ab\0cd"));
        assert!(!looks_like_text(&[1, 2, 3, 4, 5, b'a']));
    }
}
