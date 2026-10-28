// SPDX-License-Identifier: AGPL-3.0-or-later

//! `parse_str()` takes any bytes without panicking or hanging.

#![no_main]

use hyscribe_phpser::parse_str;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_str(data);
});
