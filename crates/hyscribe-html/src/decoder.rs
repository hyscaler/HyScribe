// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_HTML_Decoder`: character references in an attribute value, with the
//! HTML5 rules for a missing semicolon there, and in text.

use crate::entities::{ENTITIES, LONGEST};

/// What `&#128;` to `&#159;` mean: Windows-1252, as HTML5 specifies.
const WINDOWS_1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039,
    0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014,
    0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// U+FFFD REPLACEMENT CHARACTER.
pub(crate) const REPLACEMENT: &[u8] = "\u{FFFD}".as_bytes();

/// `WP_HTML_Decoder::decode_attribute()`: decodes the character references
/// in an attribute value.
pub fn decode_attribute(text: &[u8]) -> Vec<u8> {
    decode(text, true)
}

/// `WP_HTML_Decoder::decode_text_node()`: decodes the character references
/// in text, where a reference without its semicolon is never ambiguous.
pub fn decode_text_node(text: &[u8]) -> Vec<u8> {
    decode(text, false)
}

/// `WP_HTML_Decoder::decode()` in the attribute context or the data one.
fn decode(text: &[u8], attribute: bool) -> Vec<u8> {
    let mut decoded = Vec::with_capacity(text.len());
    let mut was_at = 0;
    let mut at = 0;
    for _ in 0..=text.len() {
        let Some(found) = text
            .get(at..)
            .and_then(|rest| rest.iter().position(|&b| b == b'&'))
        else {
            break;
        };
        let reference_at = at + found;
        match read_character_reference(text, reference_at, attribute) {
            Some((replacement, length)) => {
                decoded.extend_from_slice(text.get(was_at..reference_at).unwrap_or_default());
                decoded.extend_from_slice(&replacement);
                at = reference_at + length;
                was_at = at;
            }
            None => at = reference_at + 1,
        }
    }
    decoded.extend_from_slice(text.get(was_at..).unwrap_or_default());
    decoded
}

/// The character reference at `at` (an `&`), as its UTF-8 bytes and the
/// length it spans.
pub(crate) fn read_character_reference(
    text: &[u8],
    at: usize,
    attribute: bool,
) -> Option<(Vec<u8>, usize)> {
    let byte = |i: usize| text.get(i).copied();
    match byte(at + 1)? {
        b'#' => numeric_reference(text, at),
        _ => {
            let name_at = at + 1;
            if name_at + 2 > text.len() {
                return None;
            }
            let (replacement, name_length) =
                named_reference(text.get(name_at..).unwrap_or_default())?;
            let after_name = name_at + name_length;
            // Without its semicolon, a reference followed by `=` or an
            // alphanumeric is literal text inside an attribute.
            let ambiguous = attribute
                && byte(after_name - 1) != Some(b';')
                && byte(after_name).is_some_and(|b| b == b'=' || b.is_ascii_alphanumeric());
            (!ambiguous).then(|| (replacement.to_vec(), after_name - at))
        }
    }
}

/// `&#...;` and `&#x...;` at `at`.
fn numeric_reference(text: &[u8], at: usize) -> Option<(Vec<u8>, usize)> {
    let mut digits_at = at + 2;
    let (hex, max_digits) = match text.get(digits_at)? {
        b'x' | b'X' => (true, 6),
        _ => (false, 7),
    };
    digits_at += usize::from(hex);
    let rest = text.get(digits_at..).unwrap_or_default();
    let zero_count = rest.iter().take_while(|&&b| b == b'0').count();
    let digits = rest
        .get(zero_count..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| {
            if hex {
                b.is_ascii_hexdigit()
            } else {
                b.is_ascii_digit()
            }
        })
        .count();
    let after_digits = digits_at + zero_count + digits;
    let end_of_span = after_digits + usize::from(text.get(after_digits) == Some(&b';'));
    if digits == 0 && zero_count == 0 {
        return None;
    }
    let length = end_of_span - at;
    if digits == 0 || digits > max_digits {
        return Some((REPLACEMENT.to_vec(), length));
    }
    let radix = if hex { 16 } else { 10 };
    let code_point = text
        .get(digits_at + zero_count..after_digits)
        .unwrap_or_default()
        .iter()
        .fold(0, |n, &b| {
            n * radix + char::from(b).to_digit(radix).unwrap_or_default()
        });
    let code_point = match code_point.checked_sub(0x80) {
        Some(offset @ 0..=0x1F) => WINDOWS_1252
            .get(offset as usize)
            .copied()
            .unwrap_or_default(),
        _ => code_point,
    };
    Some((utf8(code_point), length))
}

/// PHP's `mb_chr()`, with U+FFFD for what it refuses.
fn utf8(code_point: u32) -> Vec<u8> {
    char::from_u32(code_point).map_or_else(|| REPLACEMENT.to_vec(), |c| c.to_string().into_bytes())
}

/// The longest named character reference `text` starts with, and its
/// length. Entity names are at least two bytes long.
fn named_reference(text: &[u8]) -> Option<(&'static [u8], usize)> {
    (2..=LONGEST.min(text.len())).rev().find_map(|length| {
        let name = text.get(..length).unwrap_or_default();
        let index = ENTITIES
            .binary_search_by(|(entry, _)| entry.cmp(&name))
            .ok()?;
        ENTITIES
            .get(index)
            .map(|(_, replacement)| (*replacement, length))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(text: &str) -> String {
        String::from_utf8(decode_attribute(text.as_bytes())).unwrap()
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(decode(""), "");
        assert_eq!(decode("plain"), "plain");
        assert_eq!(decode("a & b"), "a & b");
        assert_eq!(decode("&"), "&");
        assert_eq!(decode("x&"), "x&");
        assert_eq!(decode("&x&amp;&;&lt"), "&x&&;<");
    }

    #[test]
    fn named_references() {
        assert_eq!(decode("&amp;"), "&");
        assert_eq!(decode("a&lt;b&gt;c"), "a<b>c");
        assert_eq!(decode("&amp"), "&");
        assert_eq!(decode("&amp x"), "& x");
        assert_eq!(decode("&notin;"), "\u{2209}");
        assert_eq!(decode("&notit;"), "&notit;");
        assert_eq!(decode("&not it"), "\u{AC} it");
        assert_eq!(decode("&gt"), ">");
        assert_eq!(decode("&g"), "&g");
        assert_eq!(decode("&zz;"), "&zz;");
        assert_eq!(decode("&CounterClockwiseContourIntegral;"), "\u{2233}");
    }

    #[test]
    fn ambiguous_ampersands_stay_literal() {
        assert_eq!(decode("&ampx"), "&ampx");
        assert_eq!(decode("&amp1"), "&amp1");
        assert_eq!(decode("&ampX"), "&ampX");
        assert_eq!(decode("&amp="), "&amp=");
        assert_eq!(decode("&amp;x"), "&x");
        assert_eq!(decode("&amp-"), "&-");
        assert_eq!(decode("&ampé"), "&é");
    }

    #[test]
    fn numeric_references() {
        assert_eq!(decode("&#65;"), "A");
        assert_eq!(decode("&#65"), "A");
        assert_eq!(decode("&#x41;"), "A");
        assert_eq!(decode("&#X41"), "A");
        assert_eq!(decode("&#xaB;"), "\u{AB}");
        assert_eq!(decode("&#0065;"), "A");
        assert_eq!(decode("&#0;"), "\u{FFFD}");
        assert_eq!(decode("&#000"), "\u{FFFD}");
        assert_eq!(decode("&#x0;"), "\u{FFFD}");
        assert_eq!(decode("&#"), "&#");
        assert_eq!(decode("&#;"), "&#;");
        assert_eq!(decode("&#x;"), "&#x;");
        assert_eq!(decode("&#x"), "&#x");
        assert_eq!(decode("&#1114111;"), "\u{10FFFF}");
        assert_eq!(decode("&#1114112;"), "\u{FFFD}");
        assert_eq!(decode("&#9999999;"), "\u{FFFD}");
        assert_eq!(decode("&#10000000;"), "\u{FFFD}");
        assert_eq!(decode("&#x10FFFF;"), "\u{10FFFF}");
        assert_eq!(decode("&#x1000000;"), "\u{FFFD}");
        assert_eq!(decode("&#xD800;"), "\u{FFFD}");
        assert_eq!(decode("&#55296;"), "\u{FFFD}");
    }

    #[test]
    fn windows_1252_range() {
        assert_eq!(decode("&#127;"), "\u{7F}");
        assert_eq!(decode("&#128;"), "\u{20AC}");
        assert_eq!(decode("&#x81;"), "\u{81}");
        assert_eq!(decode("&#159;"), "\u{178}");
        assert_eq!(decode("&#160;"), "\u{A0}");
    }

    #[test]
    fn leading_zeros_are_not_digits() {
        assert_eq!(decode_attribute(b"&#x0000001;"), b"\x01");
    }
}
