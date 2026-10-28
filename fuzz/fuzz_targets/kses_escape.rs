// SPDX-License-Identifier: AGPL-3.0-or-later

//! `esc_attr()` output never holds a quote or an angle bracket and is
//! stable under escaping again; `wp_kses_no_null()` and `sanitize_key()`
//! leave none of what they remove.

#![no_main]

use hyscribe_kses::{
    esc_attr, esc_html, sanitize_key, wp_kses_no_null, wp_kses_normalize_entities,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let escaped = esc_attr(data);
    assert!(!escaped.contains(['"', '\'', '<', '>']));
    assert_eq!(esc_attr(escaped.as_bytes()), escaped);
    assert_eq!(esc_html(data), escaped);
    assert_eq!(
        escaped.is_empty(),
        std::str::from_utf8(data).is_err() || data.is_empty()
    );

    let normalized = wp_kses_normalize_entities(data);
    assert_eq!(wp_kses_normalize_entities(&normalized), normalized);

    let clean = wp_kses_no_null(data);
    assert!(
        !clean
            .iter()
            .any(|&b| matches!(b, 0..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F))
    );
    assert!(!clean.windows(2).any(|pair| pair == b"\\0"));

    let key = sanitize_key(data);
    assert!(
        key.bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
    );
});
