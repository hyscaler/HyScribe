// SPDX-License-Identifier: AGPL-3.0-or-later

//! `esc_url()` as `WP_HTML_Tag_Processor::set_attribute()` calls it for URL
//! attributes: display context, the default protocols, no filters. Includes
//! the parts of `wp_kses_normalize_entities()`, `wp_kses_bad_protocol()` and
//! PHP's `parse_url()` it depends on.

use crate::entities::ALLOWED_ENTITY_NAMES;

/// `wp_allowed_protocols()`.
const PROTOCOLS: [&[u8]; 22] = [
    b"http", b"https", b"ftp", b"ftps", b"mailto", b"news", b"irc", b"irc6", b"ircs", b"gopher",
    b"nntp", b"feed", b"telnet", b"mms", b"rtsp", b"sms", b"svn", b"tel", b"fax", b"xmpp",
    b"webcal", b"urn",
];

/// PHP's `trim()` default set.
const TRIM: &[u8] = b" \t\n\r\0\x0b";

/// `esc_url( $url )`.
pub(crate) fn esc_url(url: &[u8]) -> Vec<u8> {
    let start = url
        .iter()
        .position(|b| !TRIM.contains(b))
        .unwrap_or(url.len());
    let mut url: Vec<u8> = url
        .get(start..)
        .unwrap_or_default()
        .iter()
        .flat_map(|b| match b {
            b' ' => b"%20".as_slice(),
            _ if allowed(*b) => std::slice::from_ref(b),
            _ => b"",
        })
        .copied()
        .collect();
    if url.is_empty() {
        return url;
    }
    if !starts_with_ci(&url, b"mailto:") {
        url = deep_replace(url);
    }
    url = replace(&url, b";//", b"://");
    if !url.contains(&b':') && !matches!(url.first(), Some(b'/' | b'#' | b'?')) && !php_file(&url) {
        url = [b"http://", url.as_slice()].concat();
    }
    url = normalize_entities(&url);
    url = replace(&url, b"&amp;", b"&#038;");
    url = replace(&url, b"'", b"&#039;");
    if url.contains(&b'[') || url.contains(&b']') {
        url = escape_brackets(&url);
    }
    if url.first() == Some(&b'/') {
        return url;
    }
    let good = bad_protocol(&url);
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

fn starts_with_ci(text: &[u8], prefix: &[u8]) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

/// `preg_match( '/^[a-z0-9-]+?\.php/i', $url )`.
fn php_file(url: &[u8]) -> bool {
    let name = url
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
        .count();
    name > 0 && starts_with_ci(url.get(name..).unwrap_or_default(), b".php")
}

/// PHP's `str_replace()` for one search string, and how many it replaced.
fn replace_counting(subject: &[u8], search: &[u8], with: &[u8]) -> (Vec<u8>, usize) {
    if search.is_empty() {
        return (subject.to_vec(), 0);
    }
    let mut out = Vec::with_capacity(subject.len());
    let mut count = 0;
    let mut at = 0;
    for _ in 0..=subject.len() {
        let rest = subject.get(at..).unwrap_or_default();
        let Some(found) = rest.windows(search.len()).position(|w| w == search) else {
            out.extend_from_slice(rest);
            break;
        };
        out.extend_from_slice(rest.get(..found).unwrap_or_default());
        out.extend_from_slice(with);
        at += found + search.len();
        count += 1;
    }
    (out, count)
}

fn replace(subject: &[u8], search: &[u8], with: &[u8]) -> Vec<u8> {
    replace_counting(subject, search, with).0
}

/// `_deep_replace( array( '%0d', '%0a', '%0D', '%0A' ), $url )`.
fn deep_replace(mut url: Vec<u8>) -> Vec<u8> {
    for _ in 0..=url.len() {
        let mut count = 0;
        for search in [b"%0d", b"%0a", b"%0D", b"%0A"] {
            let (replaced, n) = replace_counting(&url, search, b"");
            url = replaced;
            count += n;
        }
        if count == 0 {
            break;
        }
    }
    url
}

/// Rewrites every `&amp;` that `rewrite` recognises: it gets the text after
/// `&amp;` and returns the replacement and the bytes it consumed there.
fn each_amp(text: &[u8], rewrite: impl Fn(&[u8]) -> Option<(Vec<u8>, usize)>) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for _ in 0..=text.len() {
        let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) else {
            break;
        };
        let found = rest.strip_prefix(b"&amp;".as_slice()).and_then(&rewrite);
        match found {
            Some((replacement, consumed)) => {
                out.extend_from_slice(&replacement);
                at += 5 + consumed;
            }
            None => {
                out.extend_from_slice(rest.get(..1).unwrap_or_default());
                at += 1;
            }
        }
    }
    out
}

fn run(text: &[u8], accept: impl Fn(&u8) -> bool) -> usize {
    text.iter().take_while(|b| accept(b)).count()
}

/// The digits of `0*[1-9...][0-9...]{0,max}` followed by `;` at the start of
/// `text`, without the semicolon.
fn reference_digits(text: &[u8], hex: bool, max: usize) -> Option<&[u8]> {
    let digits = run(text, |b| {
        if hex {
            b.is_ascii_hexdigit()
        } else {
            b.is_ascii_digit()
        }
    });
    let significant = digits - run(text, |b| *b == b'0');
    let followed = text.get(digits) == Some(&b';');
    (followed && (1..=max + 1).contains(&significant))
        .then(|| text.get(..digits).unwrap_or_default())
}

/// `valid_unicode()`.
fn valid_unicode(i: u32) -> bool {
    matches!(i, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn number(digits: &[u8], radix: u32) -> u32 {
    digits.iter().fold(0, |n, &b| {
        n * radix + char::from(b).to_digit(radix).unwrap_or_default()
    })
}

/// `wp_kses_normalize_entities( $url )`, in the `html` context.
fn normalize_entities(url: &[u8]) -> Vec<u8> {
    let content = replace(url, b"&", b"&amp;");
    let content = each_amp(&content, |after| {
        let digits = match after {
            [b'#', rest @ ..] => reference_digits(rest, false, 6)?,
            _ => return None,
        };
        let trimmed = digits
            .get(run(digits, |b| *b == b'0')..)
            .unwrap_or_default();
        let text = if valid_unicode(number(digits, 10)) {
            let pad = 3usize.saturating_sub(trimmed.len());
            [
                b"&#".as_slice(),
                b"000".get(..pad).unwrap_or_default(),
                trimmed,
                b";",
            ]
            .concat()
        } else {
            [b"&amp;#".as_slice(), digits, b";"].concat()
        };
        Some((text, digits.len() + 2))
    });
    let content = each_amp(&content, |after| {
        let digits = match after {
            [b'#', b'x' | b'X', rest @ ..] => reference_digits(rest, true, 5)?,
            _ => return None,
        };
        let trimmed = digits
            .get(run(digits, |b| *b == b'0')..)
            .unwrap_or_default();
        let text = if valid_unicode(number(digits, 16)) {
            [b"&#x".as_slice(), trimmed, b";"].concat()
        } else {
            [b"&amp;#x".as_slice(), digits, b";"].concat()
        };
        Some((text, digits.len() + 3))
    });
    each_amp(&content, |after| {
        let letters = run(after, u8::is_ascii_alphabetic);
        let digits = run(after.get(letters..).unwrap_or_default(), u8::is_ascii_digit);
        let length = letters + digits;
        let ok = (2..=8).contains(&letters) && digits <= 2 && after.get(length) == Some(&b';');
        let name = after.get(..length).filter(|_| ok)?;
        let text = if ALLOWED_ENTITY_NAMES.binary_search(&name).is_ok() {
            [b"&".as_slice(), name, b";"].concat()
        } else {
            [b"&amp;".as_slice(), name, b";"].concat()
        };
        Some((text, length + 1))
    })
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
    let end_dirty = replace(url, &front, b"");
    let end_clean = replace(&replace(&end_dirty, b"[", b"%5B"), b"]", b"%5D");
    replace(url, &end_dirty, &end_clean)
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
    let count = run(digits, u8::is_ascii_digit);
    let value = number(digits.get(..count).unwrap_or_default(), 10);
    match (count, negative) {
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
                let digits = run(s.get(e + 1..).unwrap_or_default(), u8::is_ascii_digit);
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
                let file = s
                    .get(..e)
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case(b"file"));
                if file && at(e + 3) == Some(b'/') {
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
        let digits = run(s.get(p..).unwrap_or_default(), u8::is_ascii_digit).min(6);
        let pp = p + digits;
        if digits > 0 && digits < 6 && (pp == ue || at(pp) == Some(b'/')) {
            let value = number(s.get(p..pp).unwrap_or_default(), 10);
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
    let e = s
        .get(start..)
        .unwrap_or_default()
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
                    parts.pass = Some(login.get(pp + 1..).unwrap_or_default().to_vec());
                }
                None => parts.user = Some(login.to_vec()),
            }
            authority.get(p + 1..).unwrap_or_default()
        }
        None => authority,
    };
    let ipv6 = authority.first() == Some(&b'[') && authority.last() == Some(&b']');
    let colon = authority.iter().rposition(|&b| b == b':').filter(|_| !ipv6);
    let host_length = match colon {
        Some(p) => {
            if parts.port.unwrap_or(0) == 0 {
                let digits = authority.get(p + 1..).unwrap_or_default();
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

/// `wp_kses_bad_protocol( $url, wp_allowed_protocols() )`. `esc_url()` has
/// already removed the bytes `wp_kses_no_null()` would.
fn bad_protocol(content: &[u8]) -> Vec<u8> {
    if content.starts_with(b"https://") || content.starts_with(b"http://") {
        return content.to_vec();
    }
    let mut content = content.to_vec();
    for _ in 0..6 {
        let next = bad_protocol_once(&content, 1);
        if next == content {
            return content;
        }
        content = next;
    }
    Vec::new()
}

/// The length of `&#0*58` or `&#x0*3a` (case-insensitive) at the start of
/// `text`: a colon written as a character reference, before its semicolon.
fn colon_reference(text: &[u8]) -> Option<(usize, bool)> {
    let (hex, digits_at) = match text {
        [b'&', b'#', b'x' | b'X', ..] => (true, 3),
        [b'&', b'#', ..] => (false, 2),
        _ => return None,
    };
    let rest = text.get(digits_at..).unwrap_or_default();
    let zeros = run(rest, |b| *b == b'0');
    let code = rest.get(zeros..zeros + 2).unwrap_or_default();
    let matched = if hex {
        code.eq_ignore_ascii_case(b"3a")
    } else {
        code == b"58"
    };
    matched.then_some((digits_at + zeros + 2, hex))
}

/// `wp_kses_bad_protocol_once()`.
fn bad_protocol_once(content: &[u8], count: usize) -> Vec<u8> {
    // `&#58` and `&#x3a` not followed by `;` or another digit get a `;`.
    let mut fixed = Vec::with_capacity(content.len() + 1);
    let mut at = 0;
    for _ in 0..=content.len() {
        let Some(rest) = content.get(at..).filter(|rest| !rest.is_empty()) else {
            break;
        };
        let reference = colon_reference(rest).filter(|&(length, hex)| {
            !rest
                .get(length)
                .is_some_and(|&b| b == b';' || b.is_ascii_digit() || (hex && b.is_ascii_hexdigit()))
        });
        let length = reference.map_or(1, |(length, _)| length);
        fixed.extend_from_slice(rest.get(..length).unwrap_or_default());
        if reference.is_some() {
            fixed.push(b';');
        }
        at += length;
    }
    // Split at the first colon, written plainly or as a reference.
    let split = (0..fixed.len()).find_map(|i| {
        let rest = fixed.get(i..).unwrap_or_default();
        let length = if rest.first() == Some(&b':') {
            Some(1)
        } else if starts_with_ci(rest, b"&colon;") {
            Some(7)
        } else {
            colon_reference(rest)
                .filter(|&(length, _)| rest.get(length) == Some(&b';'))
                .map(|(length, _)| length + 1)
        };
        length.map(|length| (i, i + length))
    });
    let Some((scheme_end, rest_at)) = split else {
        return fixed;
    };
    let scheme = fixed.get(..scheme_end).unwrap_or_default();
    if scheme.windows(2).any(|w| w == b"/?") {
        return fixed;
    }
    let rest = fixed.get(rest_at..).unwrap_or_default();
    let rest_start = rest
        .iter()
        .position(|b| !TRIM.contains(b))
        .unwrap_or(rest.len());
    let rest_end = rest
        .iter()
        .rposition(|b| !TRIM.contains(b))
        .map_or(rest_start, |p| p + 1);
    let mut rest = rest.get(rest_start..rest_end).unwrap_or_default().to_vec();
    let protocol = allowed_protocol(scheme);
    if protocol == b"feed:" {
        if count > 2 {
            return Vec::new();
        }
        rest = bad_protocol_once(&rest, count + 1);
        if rest.is_empty() || rest == b"0" {
            return rest;
        }
    }
    [protocol, rest].concat()
}

/// `wp_kses_bad_protocol_once2()`: `"scheme:"` when allowed, else empty.
fn allowed_protocol(scheme: &[u8]) -> Vec<u8> {
    let Some(decoded) = decode_entities(scheme) else {
        // PHP throws a TypeError for a reference beyond PHP_INT_MAX; the
        // scheme is refused instead.
        return Vec::new();
    };
    let spaceless: Vec<u8> = decoded
        .into_iter()
        .filter(|b| !b" \t\n\x0b\x0c\r".contains(b))
        .collect();
    let mut scheme = no_null(&spaceless);
    scheme.make_ascii_lowercase();
    if PROTOCOLS.contains(&scheme.as_slice()) {
        scheme.push(b':');
        scheme
    } else {
        Vec::new()
    }
}

/// `wp_kses_decode_entities()`: `&#N;` and `&#xN;` become the byte `N`
/// modulo 256. `None` for a number beyond PHP_INT_MAX.
fn decode_entities(text: &[u8]) -> Option<Vec<u8>> {
    let decimal = decode_pass(text, false)?;
    decode_pass(&decimal, true)
}

fn decode_pass(text: &[u8], hex: bool) -> Option<Vec<u8>> {
    let radix: u32 = if hex { 16 } else { 10 };
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for _ in 0..=text.len() {
        let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) else {
            break;
        };
        let digits_at = match rest {
            [b'&', b'#', b'x' | b'X', ..] if hex => 3,
            [b'&', b'#', ..] if !hex => 2,
            _ => 0,
        };
        let digits = run(rest.get(digits_at..).unwrap_or_default(), |b| {
            char::from(*b).is_digit(radix)
        });
        if digits_at > 0 && digits > 0 && rest.get(digits_at + digits) == Some(&b';') {
            let value = rest
                .get(digits_at..digits_at + digits)
                .unwrap_or_default()
                .iter()
                .try_fold(0i64, |n, &b| {
                    n.checked_mul(radix.into())?
                        .checked_add(char::from(b).to_digit(radix).unwrap_or_default().into())
                })?;
            out.extend(value.to_le_bytes().first());
            at += digits_at + digits + 1;
        } else {
            out.extend_from_slice(rest.get(..1).unwrap_or_default());
            at += 1;
        }
    }
    Some(out)
}

/// `wp_kses_no_null()`: control bytes go, and so do backslashes followed by
/// zeros.
fn no_null(text: &[u8]) -> Vec<u8> {
    let kept: Vec<u8> = text
        .iter()
        .copied()
        .filter(|b| !matches!(b, 0x00..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f))
        .collect();
    let mut out = Vec::with_capacity(kept.len());
    let mut at = 0;
    for _ in 0..=kept.len() {
        let Some(rest) = kept.get(at..).filter(|rest| !rest.is_empty()) else {
            break;
        };
        let slashes = run(rest, |b| *b == b'\\');
        let zeros = run(rest.get(slashes..).unwrap_or_default(), |b| *b == b'0');
        if slashes > 0 && zeros > 0 {
            at += slashes + zeros;
        } else {
            let length = slashes.max(1);
            out.extend_from_slice(rest.get(..length).unwrap_or_default());
            at += length;
        }
    }
    out
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
            ("http://x/'", "http://x/&#039;"),
            ("HTTP:x", "http:x"),
            ("feed:http://x", "feed:http://x"),
            ("tel:1", "tel:1"),
            ("x:y", ""),
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
    fn normalizes_entities() {
        assert_eq!(
            esc("http://x/&amp;&nbsp;&nosuch;&ab;&abcdefghi;&sup2;&sup23;&a123;"),
            "http://x/&#038;&nbsp;&#038;nosuch;&#038;ab;&#038;abcdefghi;&sup2;&#038;sup23;&#038;a123;"
        );
        assert_eq!(
            esc("http://x/&#9;&#00065;&#1234567;&#12345678;&#55296;&#1;&#0;"),
            "http://x/&#009;&#065;&#038;#1234567;&#038;#12345678;&#038;#55296;&#038;#1;&#038;#0;"
        );
        assert_eq!(
            esc("http://x/&#x41;&#X0041;&#xD800;&#x110000;&#xFFFFFFF;&#x;"),
            "http://x/&#x41;&#x41;&#038;#xD800;&#038;#x110000;&#038;#xFFFFFFF;&#038;#x;"
        );
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

    fn bad(url: &str) -> String {
        String::from_utf8(bad_protocol(url.as_bytes())).unwrap()
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

    #[test]
    fn strips_bad_protocols_as_kses_does() {
        let cases = [
            ("a:b:c:d:e:f:g:h", ""),
            ("x&#58y", "y"),
            ("x&#x3ay", "y"),
            ("x&#x3a;y", "y"),
            ("x&#58;y", "y"),
            ("x&colon;y", "y"),
            ("x&COLON;y", "y"),
            ("x&#585y", "x&#585y"),
            ("x&#58;", ""),
            ("feed:feed:feed:feed:x", ""),
            ("feed:feed:0", "0"),
            ("feed:feed:", ""),
            ("feed:feed: x", "feed:feed:x"),
            ("&#106;avascript:x", "x"),
            ("h&#116;tp:x", "http:x"),
            ("&#x7fffffffffffffff;:x", "x"),
            ("ht&#32;tp:x", "http:x"),
            ("ht&#0;tp:x", "http:x"),
            ("ht\\0tp:x", "http:x"),
            ("ht\\x:y", "y"),
            ("h\x01ttp:x", "http:x"),
            ("mailto: x ", "mailto:x"),
            ("a/?b:c", "a/?b:c"),
            ("HTTP:x", "http:x"),
            ("&#x68;ttp:x", "http:x"),
            ("feed:&#58;", ""),
            ("http:// x", "http:// x"),
            ("http://x ", "http://x "),
            ("https://x ", "https://x "),
            ("ht&#a;tp:x", "x"),
            ("ht&#;tp:x", "x"),
            ("ht&#x;tp:x", "x"),
        ];
        for (url, expected) in cases {
            assert_eq!(bad(url), expected, "{url:?}");
        }
        // PHP throws a TypeError here; the scheme is refused.
        assert_eq!(bad("&#x8000000000000000;:x"), "x");
        assert_eq!(bad("&#99999999999999999999;:x"), "x");
    }

    #[test]
    fn removes_nulls_as_kses_does() {
        assert_eq!(no_null(b"a\x00\x08\x0b\x0c\x0e\x1fb\t\n\r"), b"ab\t\n\r");
        assert_eq!(no_null(b"a0\\y\\\\0x\\0"), b"a0\\yx");
    }

    #[test]
    fn replaces_like_str_replace() {
        assert_eq!(replace(b"abc", b"", b"x"), b"abc");
        assert_eq!(replace(b"aaa", b"aa", b"b"), b"ba");
        assert_eq!(replace(b"", b"a", b"b"), b"");
        assert_eq!(deep_replace(b"%%0a0a%0D%0d".to_vec()), b"");
    }
}
