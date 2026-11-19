// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Chromium version `wp_get_chromium_major_version()` reads from a
//! hostile `User-Agent` never panics, and follows a `Chrome/` and its digits.

#![no_main]

use hyscribe_admin::page::chromium_version;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(version) = chromium_version(data) {
        assert!(version >= 0);
        assert!(data.windows(7).any(|window| window == b"Chrome/"));
    }
});
