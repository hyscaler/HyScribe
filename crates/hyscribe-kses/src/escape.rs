// SPDX-License-Identifier: AGPL-3.0-or-later

//! `esc_html()`, `esc_attr()`, `esc_textarea()` and `sanitize_key()`.

use crate::entities::wp_kses_normalize_entities;
use crate::php::rest;

/// `esc_textarea()`: `htmlspecialchars()` with `ENT_QUOTES`, which encodes
/// entities again, and gives an empty string for invalid UTF-8.
pub fn esc_textarea(text: &[u8]) -> Vec<u8> {
    if std::str::from_utf8(text).is_err() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(text.len());
    for &byte in text {
        match byte {
            b'&' => out.extend_from_slice(b"&amp;"),
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&#039;"),
            _ => out.push(byte),
        }
    }
    out
}

/// `esc_html()`: `wp_check_invalid_utf8()`, so invalid UTF-8 gives an
/// empty string, then `_wp_specialchars()` with `ENT_QUOTES` and no
/// double encoding.
pub fn esc_html(text: &[u8]) -> String {
    if std::str::from_utf8(text).is_err() {
        return String::new();
    }
    let text = wp_kses_normalize_entities(text);
    let mut out = Vec::with_capacity(text.len());
    for (i, &byte) in text.iter().enumerate() {
        match byte {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&#039;"),
            // After normalizing, every `&` starts `&amp;`, a valid numeric
            // reference or an allowed name. `htmlspecialchars()` keeps all
            // of those but `&apos;`, which is not in HTML 4.01.
            b'&' if rest(&text, i).starts_with(b"&apos;") => out.extend_from_slice(b"&amp;"),
            _ => out.push(byte),
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

/// `esc_attr()`: the same escaping as [`esc_html`].
pub fn esc_attr(text: &[u8]) -> String {
    esc_html(text)
}

/// `sanitize_key()`: lowercase ASCII letters, digits, `_` and `-` only.
pub fn sanitize_key(key: &[u8]) -> String {
    key.iter()
        .map(u8::to_ascii_lowercase)
        .filter(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from PHP's `esc_textarea()`.
    #[test]
    fn esc_textarea_encodes_entities_again() {
        assert_eq!(
            esc_textarea(b"<a href=\"x\">'&amp;</a>"),
            b"&lt;a href=&quot;x&quot;&gt;&#039;&amp;amp;&lt;/a&gt;"
        );
        assert_eq!(
            esc_textarea("&#038; \u{e9}".as_bytes()),
            "&amp;#038; \u{e9}".as_bytes()
        );
        assert_eq!(esc_textarea(b"\xff"), b"");
    }

    #[test]
    fn escapes_markup_and_quotes() {
        assert_eq!(
            esc_html(br#"<a href="x">'&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&#039;&amp;&#039;&lt;/a&gt;"
        );
        assert_eq!(
            esc_attr(b"\" onmouseover=\"x"),
            "&quot; onmouseover=&quot;x"
        );
        assert_eq!(esc_html("caf\u{e9}".as_bytes()), "caf\u{e9}");
    }

    #[test]
    fn does_not_double_encode() {
        assert_eq!(
            esc_html(b"&amp; &lt; &#039; &#x41; &hellip;"),
            "&amp; &lt; &#039; &#x41; &hellip;"
        );
        assert_eq!(
            esc_attr(b"&apos; &AMP; &#0;"),
            "&amp;apos; &amp;AMP; &amp;#0;"
        );
    }

    #[test]
    fn rejects_invalid_utf8() {
        for bytes in [
            &b"\xff"[..],
            b"a\xc3",
            b"\xc0\x80",
            b"\xed\xa0\x80",
            b"\xf4\x90\x80\x80",
        ] {
            assert_eq!(esc_html(bytes), "");
            assert_eq!(esc_attr(bytes), "");
        }
    }

    #[test]
    fn keys_keep_lowercase_word_characters() {
        assert_eq!(sanitize_key("My Key_-1.\u{e9}/Z".as_bytes()), "mykey_-1z");
        assert_eq!(sanitize_key(b""), "");
    }
}
