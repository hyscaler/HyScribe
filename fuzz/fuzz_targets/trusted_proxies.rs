// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `TRUSTED_PROXIES` list never panics, stays within its bounds, and
//! every address it names is matched by the range made from it.

#![no_main]

use hyscribe_config::proxies::{Cidr, MAX_PROXIES, parse_list};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    if let Some(list) = parse_list(text) {
        assert!(list.len() <= MAX_PROXIES);
    }
    for entry in text.split(',').map(str::trim) {
        if let Some(range) = Cidr::parse(entry) {
            if let Ok(ip) = entry.split('/').next().unwrap_or_default().parse() {
                assert!(range.contains(ip));
            }
        }
    }
});
