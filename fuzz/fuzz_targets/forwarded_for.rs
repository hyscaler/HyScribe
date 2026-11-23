// SPDX-License-Identifier: AGPL-3.0-or-later

//! The `X-Forwarded-For` entry behind a proxy, which names the client for
//! the login limits and the comment flood check, never panics, and an
//! address it reads reads back the same.

#![no_main]

use hyscribe_http::fuzzing::forwarded_address;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let entry = String::from_utf8_lossy(data);
    if let Some(ip) = forwarded_address(&entry) {
        assert!(entry.is_ascii() && !entry.is_empty());
        assert_eq!(forwarded_address(&ip.to_string()), Some(ip));
    }
});
