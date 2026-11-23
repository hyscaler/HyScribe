// SPDX-License-Identifier: AGPL-3.0-or-later

//! `ent2ncr()` and `html_entity_decode()` accept any bytes, leave text
//! without `&` or `|` as it is, and decoding never makes text longer.

#![no_main]

use hyscribe_render::formatting::{ent2ncr, html_entity_decode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let numeric = ent2ncr(data);
    let decoded = html_entity_decode(data);
    if !data.iter().any(|c| *c == b'&' || *c == b'|') {
        assert_eq!(numeric, data);
        assert_eq!(decoded, data);
    }
    assert!(decoded.len() <= data.len());
});
