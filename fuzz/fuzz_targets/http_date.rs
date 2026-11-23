// SPDX-License-Identifier: AGPL-3.0-or-later

//! An `If-Modified-Since` header of any text parses without panicking, and
//! never to a time before 1970.

#![no_main]

use hyscribe_http::fuzzing::http_date;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    assert!(http_date(&text).is_none_or(|time| time >= 0));
});
