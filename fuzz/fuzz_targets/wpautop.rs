// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wpautop()` takes any bytes, with and without line breaks, and gives
//! nothing for what PHP's `trim()` empties.

#![no_main]

use hyscribe_render::autop::wpautop;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for br in [true, false] {
        let out = wpautop(data, br);
        if hyscribe_phpser::php_trim(data).is_empty() {
            assert!(out.is_empty());
        }
    }
});
