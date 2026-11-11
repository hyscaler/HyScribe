// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's `is_serialized()` and `maybe_unserialize()`.

use crate::{Error, Value, decode};

/// The result of [`maybe_unserialize`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Maybe<'a> {
    /// WordPress would not treat the data as serialized; use it as is.
    Plain(&'a [u8]),
    /// Serialized data, decoded.
    Value(Value),
    /// Looks serialized to WordPress but does not decode. WordPress returns
    /// `false` here; callers should keep the raw bytes and never rewrite them.
    Invalid(Error),
}

/// WordPress's `is_serialized( $data, $strict )`, byte for byte.
pub fn is_serialized(data: &[u8], strict: bool) -> bool {
    let data = php_trim(data);
    if data == b"N;" {
        return true;
    }
    let [token, b':', ..] = data else {
        return false;
    };
    if data.len() < 4 {
        return false;
    }
    if strict {
        if !matches!(data.last(), Some(b';' | b'}')) {
            return false;
        }
    } else {
        // WordPress also rejects a `;` before offset 3 or a `}` before
        // offset 4. Either one sits where the token checks below need a
        // digit, so those checks reject the same inputs.
        if !data.iter().any(|&b| b == b';' || b == b'}') {
            return false;
        }
    }
    let after_token = data.get(2..).unwrap_or_default();
    match token {
        b's' => {
            let quote_ok = if strict {
                data.len() >= 2 && data.get(data.len() - 2) == Some(&b'"')
            } else {
                data.contains(&b'"')
            };
            quote_ok && digits_then_colon(after_token)
        }
        b'a' | b'O' | b'E' => digits_then_colon(after_token),
        b'b' | b'i' | b'd' => {
            // `/^{$token}:[0-9.E+-]+;$end/`
            let len = after_token
                .iter()
                .take_while(|&&b| b.is_ascii_digit() || matches!(b, b'.' | b'E' | b'+' | b'-'))
                .count();
            match after_token.get(len..) {
                Some([b';', rest @ ..]) if len > 0 => !strict || rest.is_empty(),
                _ => false,
            }
        }
        _ => false,
    }
}

/// WordPress's `maybe_unserialize()`: decodes when `is_serialized()` is true.
pub fn maybe_unserialize(data: &[u8]) -> Maybe<'_> {
    if is_serialized(data, true) {
        match decode(php_trim(data)) {
            Ok(value) => Maybe::Value(value),
            Err(err) => Maybe::Invalid(err),
        }
    } else {
        Maybe::Plain(data)
    }
}

/// `/^[0-9]+:/`
fn digits_then_colon(bytes: &[u8]) -> bool {
    let len = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    len > 0 && bytes.get(len) == Some(&b':')
}

/// PHP `trim()` with its default character set.
pub fn php_trim(data: &[u8]) -> &[u8] {
    let is_space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | b'\0' | b'\x0B');
    let start = data.iter().position(|b| !is_space(b)).unwrap_or(data.len());
    let end = data
        .iter()
        .rposition(|b| !is_space(b))
        .map_or(start, |i| i + 1);
    data.get(start..end).unwrap_or_default()
}

/// `wp_basename()`: the last part of `path`, `/` or `\\` before it.
pub fn wp_basename(path: &[u8]) -> Vec<u8> {
    let end = path
        .iter()
        .rposition(|b| !b"/\\".contains(b))
        .map_or(0, |i| i + 1);
    let path = path.get(..end).unwrap_or_default();
    let start = path
        .iter()
        .rposition(|b| b"/\\".contains(b))
        .map_or(0, |i| i + 1);
    path.get(start..).unwrap_or_default().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorKind;

    #[test]
    fn strict_matches_wordpress() {
        // Expected values from WordPress 7.1 `is_serialized()`.
        let cases: &[(&str, bool)] = &[
            ("N;", true),
            ("  N;\n", true),
            ("", false),
            ("N", false),
            ("i:", false),
            ("x;y;", false),
            ("i:1", false),
            ("i:1;", true),
            ("i:-1;", true),
            ("d:1.5E+3;", true),
            ("b:1;", true),
            ("b:;", false),
            ("i:1;x;", false),
            ("i:a;", false),
            ("s:1:\"a\";", true),
            ("s:1:a;", false),
            ("s:x:\"a\";", false),
            ("a:0:{}", true),
            ("a::{}", false),
            ("O:8:\"stdClass\":0:{}", true),
            ("E:7:\"Suit:Up\";", true),
            ("C:1:\"a\":0:{}", false),
            ("a:0:{}x", false),
        ];
        for &(input, expected) in cases {
            assert_eq!(is_serialized(input.as_bytes(), true), expected, "{input:?}");
        }
    }

    #[test]
    fn loose_matches_wordpress() {
        let cases: &[(&str, bool)] = &[
            ("i:1;x", true),
            ("i:1", false),
            ("a:{;", false),
            ("a:0}xx", false),
            ("a:0:{}x", true),
            ("s:1:\"a\";x", true),
            ("s:1:a;x", false),
            ("b:1;;", true),
            ("i:12", false),
            ("a:;xx", false),
            ("a:}1:", false),
            ("i:1}x", false),
            ("s:}\"x", false),
            ("a:1}:", false),
            ("i:;xx", false),
            ("a:1:xx", false),
            ("a:;1:x", false),
            ("b:;x}", false),
            ("s:1:}\"x", true),
        ];
        for &(input, expected) in cases {
            assert_eq!(
                is_serialized(input.as_bytes(), false),
                expected,
                "{input:?}"
            );
        }
    }

    #[test]
    fn maybe_unserialize_covers_three_outcomes() {
        assert_eq!(maybe_unserialize(b"hello"), Maybe::Plain(b"hello"));
        assert_eq!(maybe_unserialize(b" i:5; "), Maybe::Value(Value::Int(5)));
        assert_eq!(
            maybe_unserialize(b"s:9:\"short\";"),
            Maybe::Invalid(Error {
                offset: 5,
                kind: ErrorKind::UnexpectedEnd,
            })
        );
    }

    #[test]
    fn trims_like_php() {
        assert_eq!(php_trim(b"\0\x0B a \t\r\n"), b"a");
        assert_eq!(php_trim(b"   "), b"");
        assert_eq!(php_trim(b""), b"");
    }

    #[test]
    fn wp_basename_takes_the_last_part() {
        for (path, name) in [
            (&b"a/b\\c//"[..], &b"c"[..]),
            (b"a\\b.jpg", b"b.jpg"),
            (b"a/b//", b"b"),
            (b"plain", b"plain"),
            (b"//", b""),
        ] {
            assert_eq!(wp_basename(path), name);
        }
    }
}
