// SPDX-License-Identifier: AGPL-3.0-or-later

//! `safecss_filter_attr()` never keeps a `url()` with a disallowed
//! protocol where WordPress checks them: the properties that take URLs,
//! and custom properties whose value starts with `url(`.
//!
//! WordPress checks lowercase `url(` only, and not inside allowed
//! functions of other properties (`width:calc(url(...))`) or later in a
//! custom property (`--x:var(--y,url(...))`); the port keeps those as
//! WordPress does, so they are outside this property.

#![no_main]

use hyscribe_kses::{ALLOWED_PROTOCOLS, safecss_filter_attr, wp_kses_decode_entities};
use libfuzzer_sys::fuzz_target;

const URL_PROPERTIES: [&[u8]; 14] = [
    b"background",
    b"background-image",
    b"cursor",
    b"filter",
    b"list-style",
    b"list-style-image",
    b"clip-path",
    b"fill",
    b"marker",
    b"marker-end",
    b"marker-mid",
    b"marker-start",
    b"mask",
    b"stroke",
];

fn trim(text: &[u8]) -> &[u8] {
    text.trim_ascii()
}

/// The scheme a browser would see in a `url()` body, if any.
fn scheme(body: &[u8]) -> Option<Vec<u8>> {
    let cleaned: Vec<u8> = body
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace() && !matches!(b, b'"' | b'\'' | 0x0B))
        .collect();
    let decoded = wp_kses_decode_entities(&cleaned)?.to_ascii_lowercase();
    let colon = decoded.iter().position(|&b| b == b':')?;
    let scheme = decoded.get(..colon)?;
    let valid = scheme.first().is_some_and(u8::is_ascii_alphabetic)
        && scheme
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'));
    valid.then(|| scheme.to_vec())
}

fuzz_target!(|data: &[u8]| {
    let css = safecss_filter_attr(data);
    assert!(css.len() <= data.len());
    for declaration in css.split(|&b| b == b';') {
        let Some(colon) = declaration.iter().position(|&b| b == b':') else {
            continue;
        };
        let (name, value) = (trim(&declaration[..colon]), &declaration[colon + 1..]);
        let checked = URL_PROPERTIES.contains(&name)
            || (name.starts_with(b"--") && trim(value).starts_with(b"url("));
        if !checked {
            continue;
        }
        // A `url(` token runs to the first `)`, as for browsers, where
        // a `(` inside an unquoted URL makes it invalid.
        let mut rest = value;
        while let Some(start) = rest.windows(4).position(|w| w == b"url(") {
            rest = &rest[start + 4..];
            let Some(end) = rest.iter().position(|&b| b == b')') else {
                break;
            };
            let body = &rest[..end];
            rest = &rest[end..];
            if let Some(scheme) = scheme(body) {
                assert!(
                    ALLOWED_PROTOCOLS.iter().any(|p| p.as_bytes() == scheme),
                    "{:?} kept a url() with {:?}",
                    String::from_utf8_lossy(&css),
                    String::from_utf8_lossy(&scheme)
                );
            }
        }
    }
});
