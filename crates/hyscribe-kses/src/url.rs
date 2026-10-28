// SPDX-License-Identifier: AGPL-3.0-or-later

//! `esc_url()` and the part of PHP's `parse_url()` it needs.

use crate::entities::wp_kses_normalize_entities;
use crate::php::{ltrim, rest, run, starts_with_ci, str_replace, value};
use crate::protocol::{ALLOWED_PROTOCOLS, wp_kses_bad_protocol};

/// `esc_url( $url )`: the display context with the default protocols, as
/// `WP_HTML_Tag_Processor::set_attribute()` calls it. Empty for a URL
/// whose scheme is not allowed.
pub fn esc_url(url: &[u8]) -> Vec<u8> {
    let url: Vec<u8> = str_replace(ltrim(url), b" ", b"%20")
        .into_iter()
        .filter(|&b| allowed(b))
        .collect();
    if url.is_empty() {
        return url;
    }
    let mut url = if starts_with_ci(&url, b"mailto:") {
        url
    } else {
        deep_replace(url)
    };
    url = str_replace(&url, b";//", b"://");
    if !url.contains(&b':') && !matches!(url.first(), Some(b'/' | b'#' | b'?')) && !php_file(&url) {
        url = [b"http://", url.as_slice()].concat();
    }
    url = wp_kses_normalize_entities(&url);
    url = str_replace(&url, b"&amp;", b"&#038;");
    url = str_replace(&url, b"'", b"&#039;");
    if url.contains(&b'[') || url.contains(&b']') {
        url = escape_brackets(&url);
    }
    if url.first() == Some(&b'/') {
        return url;
    }
    let good = wp_kses_bad_protocol(&url, &ALLOWED_PROTOCOLS);
    if good.eq_ignore_ascii_case(&url) {
        good
    } else {
        Vec::new()
    }
}

/// What `esc_url()`'s first regex keeps.
fn allowed(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b >= 0x80 || b"-~+_.?#=!&;,/:%@$|*'()[]".contains(&b)
}

/// `preg_match( '/^[a-z0-9-]+?\.php/i', $url )`.
fn php_file(url: &[u8]) -> bool {
    let name = run(url, |b| b.is_ascii_alphanumeric() || *b == b'-').len();
    name > 0 && starts_with_ci(rest(url, name), b".php")
}

/// `_deep_replace( array( '%0d', '%0a', '%0D', '%0A' ), $url )`. Every
/// round that changes anything shortens the URL.
fn deep_replace(mut url: Vec<u8>) -> Vec<u8> {
    for _ in 0..=url.len() {
        let before = url.len();
        for search in [b"%0d", b"%0a", b"%0D", b"%0A"] {
            url = str_replace(&url, search, b"");
        }
        if url.len() == before {
            break;
        }
    }
    url
}

/// What `esc_url()` keeps of `parse_url()`.
#[derive(Debug, Default, PartialEq)]
struct Parts {
    scheme: Option<Vec<u8>>,
    user: Option<Vec<u8>>,
    pass: Option<Vec<u8>>,
    host: Option<Vec<u8>>,
    port: Option<u16>,
}

/// Percent-encodes the brackets outside the scheme, credentials, host and
/// port, as `esc_url()` does with `wp_parse_url()`.
fn escape_brackets(url: &[u8]) -> Vec<u8> {
    let parsed = wp_parse_url(url).unwrap_or_default();
    let mut front = Vec::new();
    if let Some(scheme) = &parsed.scheme {
        front.extend_from_slice(scheme);
        front.extend_from_slice(b"://");
    } else if url.first() == Some(&b'/') {
        front.extend_from_slice(b"//");
    }
    if let Some(user) = &parsed.user {
        front.extend_from_slice(user);
    }
    if let Some(pass) = &parsed.pass {
        front.push(b':');
        front.extend_from_slice(pass);
    }
    if parsed.user.is_some() || parsed.pass.is_some() {
        front.push(b'@');
    }
    if let Some(host) = &parsed.host {
        front.extend_from_slice(host);
    }
    if let Some(port) = parsed.port {
        front.push(b':');
        front.extend_from_slice(port.to_string().as_bytes());
    }
    let end_dirty = str_replace(url, &front, b"");
    let end_clean = str_replace(&str_replace(&end_dirty, b"[", b"%5B"), b"]", b"%5D");
    str_replace(url, &end_dirty, &end_clean)
}

/// `wp_parse_url( $url )`; `None` where it returns `false`.
fn wp_parse_url(url: &[u8]) -> Option<Parts> {
    if url.starts_with(b"//") {
        parse_url(&[b"placeholder:", url].concat()).map(|parts| Parts {
            scheme: None,
            ..parts
        })
    } else if url.starts_with(b"/") {
        parse_url(&[b"placeholder://placeholder", url].concat()).map(|parts| Parts {
            scheme: None,
            host: None,
            ..parts
        })
    } else {
        parse_url(url)
    }
}

/// C's `strtol()` on the text after a host's colon, as `parse_url()`
/// validates a port: an optional sign, then at least one digit; anything
/// after the digits is ignored.
fn port(text: &[u8]) -> Option<u16> {
    let (negative, digits) = match text {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, text),
    };
    let digits = run(digits, u8::is_ascii_digit);
    let value = value(digits, 10);
    match (digits.len(), negative) {
        (0, _) => None,
        (_, true) => (value == 0).then_some(0),
        _ => u16::try_from(value).ok(),
    }
}

/// The scheme, credentials, host and port of PHP's `parse_url()`
/// (`php_url_parse_ex2()`); `None` where it fails. The path, query and
/// fragment are not needed.
fn parse_url(s: &[u8]) -> Option<Parts> {
    let ue = s.len();
    let at = |i: usize| s.get(i).copied();
    let slice = |from: usize, to: usize| s.get(from..to).unwrap_or_default().to_vec();
    let relative = |from: usize| at(from) == Some(b'/') && at(from + 1) == Some(b'/');
    let mut parts = Parts::default();
    let colon = s.iter().position(|&b| b == b':');
    let mut start = 0;
    // Where parsing goes on: the port after the colon at `e`, or the host.
    let port_at = match colon {
        Some(e) if e > 0 => {
            let scheme = s.get(..e).unwrap_or_default();
            let valid = scheme
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'.' | b'-'));
            // The colon comes before any query or fragment.
            let before_query = !scheme.iter().any(|b| matches!(b, b'?' | b'#'));
            if !valid {
                if e + 1 < ue && before_query {
                    Some(e)
                } else if relative(0) {
                    start = 2;
                    None
                } else {
                    return Some(parts);
                }
            } else if e + 1 == ue {
                parts.scheme = Some(slice(0, e));
                return Some(parts);
            } else if at(e + 1) != Some(b'/') {
                let digits = run(rest(s, e + 1), u8::is_ascii_digit).len();
                let p = e + 1 + digits;
                if (p == ue || at(p) == Some(b'/')) && digits < 6 {
                    Some(e)
                } else {
                    parts.scheme = Some(slice(0, e));
                    return Some(parts);
                }
            } else {
                parts.scheme = Some(slice(0, e));
                if at(e + 2) != Some(b'/') {
                    return Some(parts);
                }
                start = e + 3;
                if scheme.eq_ignore_ascii_case(b"file") && at(e + 3) == Some(b'/') {
                    return Some(parts);
                }
                None
            }
        }
        Some(e) => Some(e),
        None if relative(0) => {
            start = 2;
            None
        }
        None => return Some(parts),
    };
    if let Some(e) = port_at {
        let p = e + 1;
        let digits = run(rest(s, p), u8::is_ascii_digit).len().min(6);
        let pp = p + digits;
        if digits > 0 && digits < 6 && (pp == ue || at(pp) == Some(b'/')) {
            let value = value(s.get(p..pp).unwrap_or_default(), 10);
            parts.port = Some(u16::try_from(value).ok()?);
            if relative(start) {
                start += 2;
            }
        } else if digits == 0 && pp == ue {
            return None;
        } else if relative(start) {
            start += 2;
        } else {
            return Some(parts);
        }
    }
    // The host, after any credentials.
    let e = rest(s, start)
        .iter()
        .position(|b| matches!(b, b'/' | b'?' | b'#'))
        .map_or(ue, |p| start + p);
    let authority = s.get(start..e).unwrap_or_default();
    let authority = match authority.iter().rposition(|&b| b == b'@') {
        Some(p) => {
            let login = authority.get(..p).unwrap_or_default();
            match login.iter().position(|&b| b == b':') {
                Some(pp) => {
                    parts.user = Some(login.get(..pp).unwrap_or_default().to_vec());
                    parts.pass = Some(rest(login, pp + 1).to_vec());
                }
                None => parts.user = Some(login.to_vec()),
            }
            rest(authority, p + 1)
        }
        None => authority,
    };
    let ipv6 = authority.first() == Some(&b'[') && authority.last() == Some(&b']');
    let colon = authority.iter().rposition(|&b| b == b':').filter(|_| !ipv6);
    let host_length = match colon {
        Some(p) => {
            if parts.port.unwrap_or(0) == 0 {
                let digits = rest(authority, p + 1);
                if digits.len() > 5 {
                    return None;
                }
                if !digits.is_empty() {
                    parts.port = Some(port(digits)?);
                }
            }
            p
        }
        None => authority.len(),
    };
    if host_length == 0 {
        return None;
    }
    parts.host = Some(authority.get(..host_length).unwrap_or_default().to_vec());
    Some(parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc(url: &str) -> String {
        String::from_utf8(esc_url(url.as_bytes())).unwrap()
    }

    #[test]
    fn cleans_urls_as_esc_url_does() {
        let cases = [
            ("", ""),
            (" ", ""),
            (" \t\0http://a b", "http://a%20b"),
            ("http://x/<\"q\">", "http://x/q"),
            ("index.php", "index.php"),
            ("a-b.PHP?x", "a-b.PHP?x"),
            (".php", "http://.php"),
            ("a.phpx", "a.phpx"),
            ("a_b.php", "http://a_b.php"),
            ("example.com", "http://example.com"),
            ("/rel", "/rel"),
            ("#f", "#f"),
            ("?q", "?q"),
            ("javascript:x", ""),
            ("mailto:a%0a@b", "mailto:a%0a@b"),
            ("MAILTO:%0D", "mailto:%0D"),
            ("http://x/%%0a0a", "http://x/"),
            ("%0a", "http://"),
            ("x;//y", ""),
            ("http://x/'&amp;", "http://x/&#039;&#038;"),
            ("HTTP:x", "http:x"),
            ("feed:http://x", "feed:http://x"),
            ("tel:1", "tel:1"),
            ("x:y", ""),
            ("&#99999999999999999999;:x", ""),
        ];
        for (url, expected) in cases {
            assert_eq!(esc(url), expected, "{url:?}");
        }
        assert_eq!(esc_url(b"http://x/\x80\xff\x7f"), b"http://x/\x80\xff");
    }

    #[test]
    fn encodes_brackets_after_the_host() {
        let cases = [
            ("http://x/?a[1]=2", "http://x/?a%5B1%5D=2"),
            ("http://[::1]/a[b]", "http://[::1]/a%5Bb%5D"),
            ("http://u:p@h:80/[x]", "http://u:p@h:80/%5Bx%5D"),
            ("http://@h/[x]", "http://@h/%5Bx%5D"),
            ("//h:80/[x]", "//h:80/%5Bx%5D"),
            ("/[x]", "/%5Bx%5D"),
            ("[x]", "http://[x]"),
            ("x:/[y]", ""),
            ("http://h:99999/[x]", "http://h:99999/%5Bx%5D"),
            ("http://h:-1/[x]", "http://h:-1/%5Bx%5D"),
            ("a[1]:8/", ""),
        ];
        for (url, expected) in cases {
            assert_eq!(esc(url), expected, "{url:?}");
        }
    }

    #[test]
    fn deep_replaces_line_breaks() {
        assert_eq!(deep_replace(b"%%0a0a%0D%0d".to_vec()), b"");
        assert_eq!(deep_replace(b"%%%0a0D0A".to_vec()), b"");
    }

    fn parsed(url: &str) -> Option<String> {
        let show = |part: &Option<Vec<u8>>| {
            part.as_deref().map_or_else(
                || "-".to_string(),
                |p| String::from_utf8_lossy(p).into_owned(),
            )
        };
        parse_url(url.as_bytes()).map(|p| {
            format!(
                "{} {} {} {} {}",
                show(&p.scheme),
                show(&p.user),
                show(&p.pass),
                show(&p.host),
                p.port
                    .map_or_else(|| "-".to_string(), |port| port.to_string())
            )
        })
    }

    #[test]
    fn parses_urls_as_php_does() {
        let cases = [
            ("//x/a", Some("- - - x -")),
            ("//x:8/a", Some("- - - x 8")),
            ("//", None),
            (":", None),
            (":8", None),
            (":8/x", None),
            ("a:", Some("a - - - -")),
            ("a_b:", Some("- - - - -")),
            ("a_b:c", Some("- - - - -")),
            ("a_b?:c", Some("- - - - -")),
            ("//u@h:80", Some("- u - h 80")),
            ("x://h:8", Some("x - - h 8")),
            ("x:8", Some("- - - x 8")),
            ("x:8/", Some("- - - x 8")),
            ("x:123456", Some("x - - - -")),
            ("x:/a", Some("x - - - -")),
            ("x:a", Some("x - - - -")),
            ("file:///c", Some("file - - - -")),
            ("file://h", Some("file - - h -")),
            ("x://h:", Some("x - - h -")),
            ("x://h:+5", Some("x - - h 5")),
            ("x://h:-0", Some("x - - h 0")),
            ("x://h:-1", None),
            ("x://h:8x", Some("x - - h 8")),
            ("x://h:x", None),
            ("x://h:123456", None),
            ("x://:8", None),
            ("x://[::1]", Some("x - - [::1] -")),
            ("x://[::1]:8", Some("x - - [::1] 8")),
            ("x://u:p@h", Some("x u p h -")),
            ("x://@h", Some("x  - h -")),
            ("a_b://h", Some("- - - - -")),
            ("a?b:8/", Some("- - - - -")),
            ("x:0/", Some("- - - x 0")),
            ("x://h:0:8", Some("x - - h:0 8")),
            ("//a:0/", Some("- - - a 0")),
            ("//x:", Some("- - - x -")),
            ("//x:abc/", None),
            ("//x?:8", Some("- - - x -")),
            ("abc", Some("- - - - -")),
            ("x:99999", None),
            ("//x:8", Some("- - - x 8")),
            ("a_b:8/x", Some("- - - a_b 8")),
            (":000080", Some("- - - - -")),
            ("//x:8a", Some("- - - x 8")),
            ("x://h:65535", Some("x - - h 65535")),
            ("a/b", Some("- - - - -")),
            ("/x", Some("- - - - -")),
            (":8a", Some("- - - - -")),
            ("a_b:000080", Some("- - - - -")),
        ];
        for (url, expected) in cases {
            assert_eq!(parsed(url).as_deref(), expected, "{url:?}");
        }
    }

    #[test]
    fn parses_urls_as_wordpress_does() {
        assert_eq!(wp_parse_url(b"//"), None);
        assert_eq!(wp_parse_url(b"/"), Some(Parts::default()));
        assert_eq!(
            wp_parse_url(b"//h:8/"),
            Some(Parts {
                host: Some(b"h".to_vec()),
                port: Some(8),
                ..Parts::default()
            })
        );
    }
}
