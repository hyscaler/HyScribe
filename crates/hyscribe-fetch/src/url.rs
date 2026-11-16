// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_http_validate_url()` without its DNS lookup, which the client makes
//! itself on every connection, and `WP_Http::make_absolute_url()`.

use std::net::Ipv4Addr;

use hyscribe_kses::{parse_url, wp_kses_bad_protocol, wp_parse_url};

use crate::address::is_public;

/// The ports `http_allowed_safe_ports` allows by default.
const SAFE_PORTS: [u16; 3] = [80, 443, 8080];

/// Where a valid URL is fetched from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// `https` rather than `http`.
    pub(crate) https: bool,
    /// The host, as the URL gives it.
    pub(crate) host: String,
    /// The port connected to.
    pub(crate) port: u16,
    /// The `Host` header: the host, and the port the URL gives.
    pub(crate) authority: String,
    /// The path and query, percent-encoded where HTTP needs it.
    pub(crate) path: String,
}

/// `wp_http_validate_url( $url )` with the site's `home`, where the target
/// passes: HTTP or HTTPS, a host without user or password, and a safe port
/// or the home's own. The host must be a plain DNS name, whose addresses
/// are the client's to check, or a public IPv4 address, the home's too.
pub fn validate_url(url: &[u8], home: &[u8]) -> Option<Target> {
    let checked = wp_kses_bad_protocol(url, &["http", "https"]);
    if checked.is_empty() || !checked.eq_ignore_ascii_case(url) {
        return None;
    }
    let parts = parse_url(&checked)?;
    let host = parts.host.filter(|host| !host.is_empty())?;
    let plain = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_');
    if parts.user.is_some() || parts.pass.is_some() || !host.iter().all(plain) {
        return None;
    }
    // wp_kses_bad_protocol() left HTTP or HTTPS.
    let https = parts.scheme?.eq_ignore_ascii_case(b"https");
    let host = String::from_utf8_lossy(&host).into_owned();
    let literal = host.trim_matches('.').parse::<Ipv4Addr>();
    if literal.is_ok_and(|ip| !is_public(ip.into())) {
        return None;
    }
    let home = parse_url(home).unwrap_or_default();
    let same_host = home
        .host
        .is_some_and(|home| home.eq_ignore_ascii_case(host.as_bytes()));
    let given = parts.port.filter(|port| *port != 0);
    if let Some(port) = given {
        let home_port = same_host && home.port == Some(port);
        if !SAFE_PORTS.contains(&port) && !home_port {
            return None;
        }
    }
    let authority = match given {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    let mut path = encoded(parts.path.as_deref().unwrap_or(b"/"));
    if let Some(query) = parts.query {
        path.push('?');
        path.push_str(&encoded(&query));
    }
    Some(Target {
        https,
        port: given.unwrap_or(if https { 443 } else { 80 }),
        host,
        authority,
        path,
    })
}

/// `bytes` with what a request target cannot carry percent-encoded.
fn encoded(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if b.is_ascii_graphic() && !b"\"<>\\^`{|}#".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// PHP's `empty()` of a string.
fn empty(text: Option<&[u8]>) -> bool {
    matches!(text, None | Some(b"" | b"0"))
}

/// `WP_Http::make_absolute_url( $maybe_relative_path, $url )`.
pub fn make_absolute_url(relative: &[u8], url: &[u8]) -> Vec<u8> {
    let (Some(base), Some(rel)) = (
        Some(url)
            .filter(|url| !empty(Some(url)))
            .and_then(wp_parse_url),
        wp_parse_url(relative),
    ) else {
        return relative.to_vec();
    };
    if !empty(rel.scheme.as_deref()) {
        return relative.to_vec();
    }
    let mut absolute = base.scheme.unwrap_or_default();
    absolute.extend_from_slice(b"://");
    let (host, port) = match rel.host {
        Some(host) => (host, rel.port),
        None => (base.host.unwrap_or_default(), base.port),
    };
    absolute.extend(host);
    if let Some(port) = port {
        absolute.extend(format!(":{port}").into_bytes());
    }
    let base_path = base.path.filter(|path| !empty(Some(path)));
    let mut path = base_path.unwrap_or(b"/".to_vec());
    match rel.path {
        Some(rel) if rel.first() == Some(&b'/') => path = rel,
        Some(rel) if !empty(Some(&rel)) => {
            // `strrpos()` false is 0, plus 1.
            let keep = path.iter().rposition(|&b| b == b'/').unwrap_or(0) + 1;
            path.truncate(keep);
            path.extend(rel);
            path = dot_segments(path);
        }
        _ => {}
    }
    if let Some(query) = rel.query.filter(|query| !empty(Some(query))) {
        path.push(b'?');
        path.extend(query);
    }
    if let Some(fragment) = rel.fragment.filter(|fragment| !empty(Some(fragment))) {
        path.push(b'#');
        path.extend(fragment);
    }
    absolute.push(b'/');
    let start = path.iter().position(|&b| b != b'/').unwrap_or(path.len());
    absolute.extend(path.get(start..).unwrap_or_default());
    absolute
}

/// The relative path's `../` stripped as `make_absolute_url()` strips
/// them: `[^/]+/../` removed while `../` is found past the start, then a
/// leading `/../` run. A pass removes `../` runs as well as the segments
/// they follow, so nesting of any depth takes few passes.
fn dot_segments(mut path: Vec<u8>) -> Vec<u8> {
    while find(&path, b"../").is_some_and(|at| at > 1) {
        let mut out = Vec::new();
        let (mut at, mut replaced) = (0, false);
        while let Some(rest) = path.get(at..).filter(|rest| !rest.is_empty()) {
            let run = rest.iter().take_while(|&&c| c != b'/').count();
            let tail = rest.get(run..).unwrap_or_default();
            if run > 0 && tail.starts_with(b"/../") {
                at += run + 4;
                replaced = true;
            } else {
                // A run that fails fails from each of its bytes.
                let kept = run.max(1);
                out.extend(rest.get(..kept).unwrap_or_default());
                at += kept;
            }
        }
        path = out;
        if !replaced {
            break;
        }
    }
    if path.starts_with(b"/../") {
        let mut at = 1;
        while path.get(at..).is_some_and(|rest| rest.starts_with(b"../")) {
            at += 3;
        }
        path.drain(..at);
    }
    path
}

/// `strpos()`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests;
