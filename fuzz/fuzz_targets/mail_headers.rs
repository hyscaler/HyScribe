// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_mail()`'s headers string: never a panic, a stray CR or a NUL
//! refuses it, and nothing it keeps holds a CR, LF or NUL, so no value can
//! start a header of its own.

#![no_main]

use hyscribe_mail::{breaks_line, parse_headers};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(headers) = parse_headers(data) else {
        // A CR ends a line only before an LF, so the last line keeps its own.
        let lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
        let stray = lines.iter().enumerate().any(|(at, line)| {
            let line = match at + 1 < lines.len() {
                true => line.strip_suffix(b"\r").unwrap_or(line),
                false => line,
            };
            line.iter().any(|&b| b == b'\r' || b == 0)
        });
        assert!(stray);
        return;
    };
    let kept = [&headers.from_name, &headers.from_email, &headers.content_type, &headers.charset];
    for value in kept.into_iter().flatten().chain(&headers.reply_to).chain(&headers.dropped) {
        assert!(!breaks_line(value));
    }
});
