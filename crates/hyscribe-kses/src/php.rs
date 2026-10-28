// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP and PCRE building blocks the ports share.

/// The bytes PHP's `trim()` removes by default.
fn is_trimmed(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0B)
}

/// PHP's `trim()` with its default characters.
pub(crate) fn trim(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|&b| !is_trimmed(b))
        .unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|&b| !is_trimmed(b))
        .map_or(0, |i| i + 1);
    text.get(start..end).unwrap_or_default()
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
pub(crate) fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    (0..haystack.len()).any(|i| rest(haystack, i).starts_with(needle))
}

/// Whether `text` starts with `prefix`, ignoring ASCII case.
pub(crate) fn starts_with_ci(text: &[u8], prefix: &[u8]) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
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

/// PHP's `str_replace()` with an empty replacement.
pub(crate) fn remove_all(subject: &[u8], needle: &[u8]) -> Vec<u8> {
    replace_each(subject, |rest| {
        rest.starts_with(needle)
            .then_some((needle.len(), Vec::new()))
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
    fn removes_every_occurrence() {
        assert_eq!(remove_all(b"abcabxab", b"ab"), b"cx");
        assert_eq!(remove_all(b"aaa", b"aa"), b"a");
        assert_eq!(remove_all(b"xyz", b"q"), b"xyz");
        assert_eq!(remove_all(b"xy", b""), b"xy");
    }

    #[test]
    fn scans_left_to_right() {
        let found = scan(6, |i| (i % 3 == 1 && i < 6).then_some((2, i)));
        assert_eq!(found, vec![(1, 3, 1), (4, 6, 4)]);
    }
}
