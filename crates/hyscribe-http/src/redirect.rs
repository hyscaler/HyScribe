// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_safe_redirect()`'s checks: `wp_sanitize_redirect()` and
//! `wp_validate_redirect()`, which keep a redirect on the site's host.

use hyscribe_kses::{deep_replace, parse_url, wp_kses_no_null};

/// The length of the UTF-8 sequence of `first` and the bytes `after` it
/// that `wp_sanitize_redirect()`'s pattern matches, if any.
fn utf8_sequence(first: u8, after: &[u8]) -> Option<usize> {
    let tail = |b: Option<&u8>| b.is_some_and(|b| (0x80..=0xbf).contains(b));
    let second = after.first();
    let (length, second_ok) = match first {
        0xc2..=0xdf => (2, tail(second)),
        0xe0 => (3, second.is_some_and(|b| (0xa0..=0xbf).contains(b))),
        0xe1..=0xec | 0xee | 0xef => (3, tail(second)),
        0xed => (3, second.is_some_and(|b| (0x80..=0x9f).contains(b))),
        0xf0 => (4, second.is_some_and(|b| (0x90..=0xbf).contains(b))),
        0xf1..=0xf3 => (4, tail(second)),
        0xf4 => (4, second.is_some_and(|b| (0x80..=0x8f).contains(b))),
        _ => return None,
    };
    let rest_ok = (1..length - 1).all(|i| tail(after.get(i)));
    (second_ok && rest_ok).then_some(length)
}

/// `wp_sanitize_redirect()`: spaces and UTF-8 percent-encoded, every other
/// character a URL does not use dropped, and encoded line breaks removed.
pub fn wp_sanitize_redirect(location: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(location.len());
    let mut at = 0;
    while let Some(&b) = location.get(at) {
        let length = utf8_sequence(b, location.get(at + 1..).unwrap_or_default());
        let allowed = b.is_ascii_alphanumeric() || b"-~+_.?#=&;,/:%!*[]()@".contains(&b);
        match length {
            Some(length) => {
                for byte in location.get(at..at + length).unwrap_or_default() {
                    out.extend(format!("%{byte:02X}").bytes());
                }
                at += length;
                continue;
            }
            None if b == b' ' => out.extend(b"%20"),
            None if allowed => out.push(b),
            None => {}
        }
        at += 1;
    }
    deep_replace(wp_kses_no_null(&out))
}

/// `wp_validate_redirect( $location, $fallback )` for a request to the
/// login screen at the site's root, on a site whose home URL has `host`.
pub fn wp_validate_redirect(location: &[u8], host: &[u8], fallback: &[u8]) -> Vec<u8> {
    let trimmed = location
        .iter()
        .position(|b| !b" \t\n\r\0\x08\x0B".contains(b))
        .map_or(&b""[..], |start| {
            let end = location
                .iter()
                .rposition(|b| !b" \t\n\r\0\x08\x0B".contains(b))
                .map_or(start, |end| end + 1);
            location.get(start..end).unwrap_or_default()
        });
    let mut location = wp_sanitize_redirect(trimmed);
    if location.starts_with(b"//") {
        location = [&b"http:"[..], &location].concat();
    }
    // PHP does not cut at a `?` first, but neither form has a host or scheme.
    let test = location.split(|&b| b == b'?').next().unwrap_or_default();
    let Some(parts) = parse_url(test) else {
        return fallback.to_vec();
    };
    if parts
        .scheme
        .as_deref()
        .is_some_and(|scheme| scheme != b"http" && scheme != b"https")
    {
        return fallback.to_vec();
    }
    let relative = parts
        .path
        .as_deref()
        .is_some_and(|path| !hyscribe_phpser::is_empty_string(path) && path.first() != Some(&b'/'));
    if parts.host.is_none() && relative {
        // `dirname()` of the login screen's path is the root.
        location = [&b"/"[..], &location].concat();
    }
    // `parse_url()` gives credentials and a port only with a host.
    if parts.host.is_none() && parts.scheme.is_some() {
        return fallback.to_vec();
    }
    let unsafe_part = [&parts.user, &parts.pass, &parts.host]
        .into_iter()
        .flatten()
        .any(|part| part.iter().any(|b| b":/?#@".contains(b)));
    if unsafe_part {
        return fallback.to_vec();
    }
    match parts.host {
        Some(other) if other != host && other != host.to_ascii_lowercase() => fallback.to_vec(),
        _ => location,
    }
}

/// `wp_safe_redirect()`'s location: `location` kept on the site's host,
/// else `fallback`, sanitized.
pub(crate) fn wp_safe_redirect(site: &crate::Site, location: &[u8], fallback: &str) -> String {
    let host = hyscribe_kses::url_host(site.home.as_bytes()).unwrap_or_default();
    let location = wp_validate_redirect(location, &host, fallback.as_bytes());
    String::from_utf8_lossy(&wp_sanitize_redirect(&location)).into_owned()
}

#[cfg(test)]
mod tests;
