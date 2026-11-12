// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP and PCRE building blocks the ports share.

/// The bytes PHP's `trim()` removes by default.
fn is_trimmed(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B)
}

/// PHP's `ltrim()` with its default characters.
pub(crate) fn ltrim(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|&b| !is_trimmed(b))
        .unwrap_or(text.len());
    rest(text, start)
}

/// PHP's `trim()` with its default characters.
pub(crate) fn trim(text: &[u8]) -> &[u8] {
    let text = ltrim(text);
    let end = text
        .iter()
        .rposition(|&b| !is_trimmed(b))
        .map_or(0, |i| i + 1);
    text.get(..end).unwrap_or_default()
}

/// PCRE's `\s` without UTF mode: the C locale's `isspace()`.
pub(crate) fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// PHP's `empty()` for a string.
pub(crate) fn empty(text: &[u8]) -> bool {
    text.is_empty() || text == b"0"
}

/// PHP's `str_contains()`.
pub fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    (0..haystack.len()).any(|i| rest(haystack, i).starts_with(needle))
}

/// Whether `text` starts with `prefix`, ignoring ASCII case.
pub(crate) fn starts_with_ci(text: &[u8], prefix: &[u8]) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// The leading run of `text` that `class` accepts.
pub(crate) fn run(text: &[u8], class: fn(&u8) -> bool) -> &[u8] {
    let len = text.iter().take_while(|b| class(b)).count();
    text.get(..len).unwrap_or_default()
}

/// The value of hexadecimal or decimal digits in `radix`, saturating far
/// above anything the callers accept.
pub(crate) fn value(digits: &[u8], radix: u64) -> u64 {
    digits.iter().fold(0, |value: u64, &digit| {
        let digit = char::from(digit).to_digit(16).map_or(0, u64::from);
        value.saturating_mul(radix).saturating_add(digit)
    })
}

/// `text` from `offset` on; empty past the end.
pub(crate) fn rest(text: &[u8], offset: usize) -> &[u8] {
    text.get(offset..).unwrap_or_default()
}

/// The offsets in `text` where `at` matches, so that later searches are
/// lookups and stay linear however often they repeat.
pub(crate) fn positions(text: &[u8], at: impl Fn(&[u8]) -> bool) -> Vec<usize> {
    (0..text.len()).filter(|&i| at(rest(text, i))).collect()
}

/// The first of the sorted `positions` at or after `start`.
pub(crate) fn first_from(positions: &[usize], start: usize) -> Option<usize> {
    positions
        .get(positions.partition_point(|&p| p < start))
        .copied()
}

/// A match: where it starts and ends, and what it carries.
pub(crate) type Match<T> = (usize, usize, T);

/// Global matching as `preg_match_all()` and `preg_replace()` do it:
/// `at(offset)` is tried at each offset from the left and returns the
/// length of the match there (at least one) and its value; the search
/// resumes after each match. Past the end, `at` must find nothing.
pub(crate) fn scan<T>(
    len: usize,
    mut at: impl FnMut(usize) -> Option<(usize, T)>,
) -> Vec<Match<T>> {
    let mut found = Vec::new();
    let mut offset = 0;
    // Every round moves on by at least one byte.
    for _ in 0..len {
        match at(offset) {
            Some((length, value)) => {
                found.push((offset, offset + length, value));
                offset += length;
            }
            None => offset += 1,
        }
    }
    found
}

/// `subject` with each match replaced by its value.
pub(crate) fn splice(subject: &[u8], found: &[Match<Vec<u8>>]) -> Vec<u8> {
    let mut out = Vec::with_capacity(subject.len());
    let mut copied = 0;
    for (start, end, replacement) in found {
        out.extend_from_slice(subject.get(copied..*start).unwrap_or_default());
        out.extend_from_slice(replacement);
        copied = *end;
    }
    out.extend_from_slice(rest(subject, copied));
    out
}

/// `preg_replace_callback()`: `at(rest)` returns the length and the
/// replacement of a match at the start of `rest`.
pub(crate) fn replace_each(
    subject: &[u8],
    mut at: impl FnMut(&[u8]) -> Option<(usize, Vec<u8>)>,
) -> Vec<u8> {
    splice(subject, &scan(subject.len(), |i| at(rest(subject, i))))
}

/// PHP's `str_replace()` for one string; an empty `needle` finds nothing.
pub(crate) fn str_replace(subject: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return subject.to_vec();
    }
    replace_each(subject, |rest| {
        rest.starts_with(needle)
            .then(|| (needle.len(), with.to_vec()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_php_whitespace_only() {
        assert_eq!(trim(b" \t\n\r\0\x0Ba b\x0B\0\r\n\t "), b"a b");
        assert_eq!(trim(b"\x0Ca\x0C"), b"\x0Ca\x0C");
        assert_eq!(trim(b"  "), b"");
        assert_eq!(trim(b"x"), b"x");
        assert_eq!(trim(b""), b"");
    }

    #[test]
    fn spaces_are_pcre_spaces() {
        let spaces: Vec<u8> = (0..=255).filter(|&b| is_space(b)).collect();
        assert_eq!(spaces, b"\t\n\x0B\x0C\r ");
    }

    #[test]
    fn empty_is_php_empty() {
        assert!(empty(b""));
        assert!(empty(b"0"));
        assert!(!empty(b"00"));
        assert!(!empty(b"a"));
    }

    #[test]
    fn contains_and_prefixes() {
        assert!(contains(b"a/?b", b"/?"));
        assert!(contains(b"/?", b"/?"));
        assert!(!contains(b"/ ?", b"/?"));
        assert!(!contains(b"", b"/?"));
        assert!(starts_with_ci(b"URL(x", b"url("));
        assert!(!starts_with_ci(b"ur", b"url("));
        assert!(!starts_with_ci(b"uri(", b"url("));
    }

    #[test]
    fn ltrims_the_start_only() {
        assert_eq!(ltrim(b" \t\0a \n"), b"a \n");
        assert_eq!(ltrim(b" \x0B"), b"");
    }

    #[test]
    fn replaces_every_occurrence() {
        assert_eq!(str_replace(b"abcabxab", b"ab", b""), b"cx");
        assert_eq!(str_replace(b"aaa", b"aa", b"b"), b"ba");
        assert_eq!(str_replace(b"xyz", b"q", b"r"), b"xyz");
        assert_eq!(str_replace(b"xy", b"", b"z"), b"xy");
    }

    #[test]
    fn values_saturate() {
        assert_eq!(value(b"0129", 10), 129);
        assert_eq!(value(b"aF", 16), 0xAF);
        assert_eq!(value(b"99999999999999999999999", 10), u64::MAX);
        assert_eq!(value(b"18446744073709551616", 10), u64::MAX);
    }

    #[test]
    fn scans_left_to_right() {
        let found = scan(6, |i| (i % 3 == 1 && i < 6).then_some((2, i)));
        assert_eq!(found, vec![(1, 3, 1), (4, 6, 4)]);
    }
}
