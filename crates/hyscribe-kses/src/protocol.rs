// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_kses_no_null()` and `wp_kses_bad_protocol()`.

use crate::entities::wp_kses_decode_entities;
use crate::php::{contains, empty, is_space, replace_each, rest, starts_with_ci, trim};

/// `wp_allowed_protocols()` with the default filters.
pub const ALLOWED_PROTOCOLS: [&str; 22] = [
    "http", "https", "ftp", "ftps", "mailto", "news", "irc", "irc6", "ircs", "gopher", "nntp",
    "feed", "telnet", "mms", "rtsp", "sms", "svn", "tel", "fax", "xmpp", "webcal", "urn",
];

/// `wp_kses_no_null()`: removes control characters other than tab, line
/// feed and carriage return, and every run of backslashes followed by
/// zeros (`\\+0+`).
pub fn wp_kses_no_null(content: &[u8]) -> Vec<u8> {
    let content: Vec<u8> = content
        .iter()
        .copied()
        .filter(|&b| !matches!(b, 0..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F))
        .collect();
    replace_each(&content, |text| {
        let slashes = text.iter().take_while(|&&b| b == b'\\').count();
        if slashes == 0 {
            return None;
        }
        let zeros = rest(text, slashes)
            .iter()
            .take_while(|&&b| b == b'0')
            .count();
        // A run of backslashes without zeros fails at each of them, so it
        // is skipped as a whole.
        match zeros {
            0 => Some((slashes, text.get(..slashes).unwrap_or_default().to_vec())),
            _ => Some((slashes + zeros, Vec::new())),
        }
    })
}

/// `&#0*58` or `&#x0*3a` (case-insensitively) at the start of `text`: its
/// length, and whether it is hexadecimal.
fn numeric_colon(text: &[u8]) -> Option<(usize, bool)> {
    let tail = text.strip_prefix(b"&#")?;
    let hex = matches!(tail.first(), Some(b'x' | b'X'));
    let digits = rest(tail, usize::from(hex));
    let zeros = digits.iter().take_while(|&&b| b == b'0').count();
    let code = rest(digits, zeros);
    let colon = if hex {
        starts_with_ci(code, b"3a")
    } else {
        code.starts_with(b"58")
    };
    colon.then_some((4 + usize::from(hex) + zeros, hex))
}

/// The length of a colon at the start of `text`: `:`, `&#0*58;`,
/// `&#x0*3a;` or `&colon;`, case-insensitively.
fn colon(text: &[u8]) -> Option<usize> {
    if text.first() == Some(&b':') {
        return Some(1);
    }
    if starts_with_ci(text, b"&colon;") {
        return Some(7);
    }
    let (len, _) = numeric_colon(text)?;
    (rest(text, len).first() == Some(&b';')).then_some(len + 1)
}

/// `wp_kses_bad_protocol_once2()`: the scheme followed by `:` when it is
/// allowed, else nothing. `None` where PHP throws.
fn checked_scheme(scheme: &[u8], allowed: &[&str]) -> Option<Vec<u8>> {
    let decoded: Vec<u8> = wp_kses_decode_entities(scheme)?
        .into_iter()
        .filter(|&b| !is_space(b))
        .collect();
    let mut scheme = wp_kses_no_null(&decoded);
    scheme.make_ascii_lowercase();
    if !allowed
        .iter()
        .any(|p| p.as_bytes().eq_ignore_ascii_case(&scheme))
    {
        return Some(Vec::new());
    }
    scheme.push(b':');
    Some(scheme)
}

/// `wp_kses_bad_protocol_once()`. `None` where PHP throws.
fn once(content: &[u8], allowed: &[&str], count: u8) -> Option<Vec<u8>> {
    // `&#58` and `&#x3a` without their `;` get one.
    let content = replace_each(content, |text| {
        let (len, hex) = numeric_colon(text)?;
        let next = rest(text, len).first().copied().unwrap_or_default();
        let digit = if hex {
            next.is_ascii_hexdigit()
        } else {
            next.is_ascii_digit()
        };
        (next != b';' && !digit)
            .then(|| (len, [text.get(..len).unwrap_or_default(), b";"].concat()))
    });
    let Some((start, end)) =
        (0..content.len()).find_map(|i| colon(rest(&content, i)).map(|len| (i, i + len)))
    else {
        return Some(content);
    };
    let scheme = content.get(..start).unwrap_or_default();
    if contains(scheme, b"/?") {
        return Some(content);
    }
    let mut after = trim(rest(&content, end)).to_vec();
    let protocol = checked_scheme(scheme, allowed)?;
    if protocol == b"feed:" {
        if count > 2 {
            return Some(Vec::new());
        }
        after = once(&after, allowed, count + 1)?;
        if empty(&after) {
            return Some(after);
        }
    }
    Some([protocol, after].concat())
}

/// `wp_kses_bad_protocol()`: strips every scheme not in `allowed` (see
/// [`ALLOWED_PROTOCOLS`]) from the start of `content`, through entities
/// and whitespace, giving up with an empty string after six rounds.
///
/// Where PHP throws a `TypeError` (a numeric reference beyond
/// `PHP_INT_MAX` in a scheme) the result is empty too.
pub fn wp_kses_bad_protocol(content: &[u8], allowed: &[&str]) -> Vec<u8> {
    let mut content = wp_kses_no_null(content);
    if (content.starts_with(b"https://") && allowed.contains(&"https"))
        || (content.starts_with(b"http://") && allowed.contains(&"http"))
    {
        return content;
    }
    for _ in 0..6 {
        let Some(next) = once(&content, allowed, 1) else {
            return Vec::new();
        };
        if next == content {
            return content;
        }
        content = next;
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bad(text: &str) -> String {
        String::from_utf8(wp_kses_bad_protocol(text.as_bytes(), &ALLOWED_PROTOCOLS)).unwrap()
    }

    #[test]
    fn removes_control_characters() {
        assert_eq!(
            wp_kses_no_null(b"\x00a\x01\x08\x09\x0a\x0b\x0c\x0d\x0e\x1f\x20\x7f"),
            b"a\t\n\r \x7f"
        );
    }

    #[test]
    fn removes_backslash_zeros() {
        assert_eq!(wp_kses_no_null(b"a\\0b\\\\00c\\0\\0d"), b"abcd");
        assert_eq!(wp_kses_no_null(b"\\a\\\\b\\"), b"\\a\\\\b\\");
        assert_eq!(wp_kses_no_null(b"0\\\x000"), b"0");
    }

    #[test]
    fn strips_disallowed_schemes() {
        assert_eq!(bad("javascript:alert(1)"), "alert(1)");
        assert_eq!(bad(" JavaScript : alert(1)"), "alert(1)");
        assert_eq!(bad("java\tscr\x0bipt\\0:x"), "x");
        assert_eq!(bad("java&#115;cript:x"), "x");
        assert_eq!(bad("&#106;ava&#x73;cript&#58;x"), "x");
        assert_eq!(bad("data:text/html,x"), "text/html,x");
    }

    #[test]
    fn keeps_allowed_schemes_lowercased() {
        assert_eq!(bad("http://x"), "http://x");
        assert_eq!(bad("http://x "), "http://x ");
        assert_eq!(bad("HTTP://x"), "http://x");
        assert_eq!(bad("&#104;ttp: //x"), "http://x");
        assert_eq!(bad("mailto:a@b"), "mailto:a@b");
        assert_eq!(bad("//x"), "//x");
    }

    #[test]
    fn short_circuits_only_for_allowed_http() {
        assert_eq!(wp_kses_bad_protocol(b"https://x", &["http"]), b"//x");
        assert_eq!(wp_kses_bad_protocol(b"http://x", &["https"]), b"//x");
        assert_eq!(wp_kses_bad_protocol(b"https://x", &["https"]), b"https://x");
        assert_eq!(wp_kses_bad_protocol(b"Https://x", &["HTTPS"]), b"https://x");
    }

    #[test]
    fn finds_colons_in_every_spelling() {
        assert_eq!(bad("javascript&#58x"), "x");
        assert_eq!(bad("javascript&#0058;x"), "x");
        assert_eq!(bad("javascript&#x3a;x"), "x");
        assert_eq!(bad("javascript&#X03A;x"), "x");
        assert_eq!(bad("javascript&#x3aG"), "G");
        assert_eq!(bad("javascript&colon;x"), "x");
        assert_eq!(bad("javascript&COLON;x"), "x");
        assert_eq!(bad("javascript&#585;x"), "javascript&#585;x");
        assert_eq!(bad("javascript&#x3ab;x"), "javascript&#x3ab;x");
        assert_eq!(bad("javascript&#5;8x"), "javascript&#5;8x");
        assert_eq!(bad("javascript&#x;x"), "javascript&#x;x");
    }

    #[test]
    fn leaves_paths_with_queries() {
        assert_eq!(bad("/?a:b"), "/?a:b");
        assert_eq!(bad("a/?b:c"), "a/?b:c");
        assert_eq!(bad("a/b?:c"), "c");
    }

    #[test]
    fn recurses_into_feeds_twice() {
        assert_eq!(bad("feed:http://x"), "feed:http://x");
        assert_eq!(bad("feed:javascript:x"), "feed:x");
        assert_eq!(bad("feed:feed:http://x"), "feed:feed:http://x");
        assert_eq!(bad("feed:feed:feed:http://x"), "");
        assert_eq!(bad("feed:0"), "0");
        assert_eq!(bad("feed:"), "");
    }

    #[test]
    fn gives_up_after_six_rounds() {
        assert_eq!(bad(&format!("{}x", "javascript:".repeat(5))), "x");
        assert_eq!(bad(&format!("{}x", "javascript:".repeat(6))), "");
    }

    #[test]
    fn empties_where_php_throws() {
        assert_eq!(bad("&#99999999999999999999;:x"), "");
        assert_eq!(bad("x:&#99999999999999999999;:y"), "");
        assert_eq!(bad("feed:&#99999999999999999999;:x"), "");
        assert_eq!(bad("&#9223372036854775807;:x"), "x");
    }
}
