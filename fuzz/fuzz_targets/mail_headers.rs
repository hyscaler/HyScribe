// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_mail()`'s headers string: never a panic, a stray CR or a NUL
//! refuses it, and nothing it keeps holds a CR, LF or NUL, so no value can
//! start a header of its own.

#![no_main]

use hyscribe_mail::{breaks_line, parse_headers};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(headers) = parse_headers(data) else {
        let unix = data.split(|&b| b == b'\n');
        assert!(unix.flat_map(|line| line.strip_suffix(b"\r").unwrap_or(line)).any(|&b| b == b'\r' || b == 0));
        return;
    };
    let kept = [&headers.from_name, &headers.from_email, &headers.content_type, &headers.charset];
    for value in kept.into_iter().flatten().chain(&headers.reply_to).chain(&headers.dropped) {
        assert!(!breaks_line(value));
    }
});
