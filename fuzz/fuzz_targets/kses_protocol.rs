// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_kses_bad_protocol()` is stable: its output comes back unchanged,
//! and a scheme it leaves in front (before the first `:`, without `/?`
//! or an entity) is an allowed one, spelled as in the list.

#![no_main]

use hyscribe_kses::{ALLOWED_PROTOCOLS, wp_kses_bad_protocol, wp_kses_decode_entities};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let url = wp_kses_bad_protocol(data, &ALLOWED_PROTOCOLS);
    assert_eq!(wp_kses_bad_protocol(&url, &ALLOWED_PROTOCOLS), url);
    if let Some(colon) = url.iter().position(|&b| b == b':') {
        let scheme = &url[..colon];
        if !scheme.windows(2).any(|w| w == b"/?") && !scheme.contains(&b'&') {
            assert!(
                ALLOWED_PROTOCOLS.iter().any(|p| p.as_bytes() == scheme),
                "{:?} kept {:?}",
                String::from_utf8_lossy(data),
                String::from_utf8_lossy(scheme)
            );
        }
    }
    let _ = wp_kses_decode_entities(data);
});
