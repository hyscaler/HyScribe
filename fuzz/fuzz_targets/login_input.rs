// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the login screen parses from a request never panics: the Cookie
//! header, a URL's parts, and a redirect, which keeps only URL characters.

#![no_main]

use hyscribe_http::fuzzing::{cookies, wp_sanitize_redirect, wp_validate_redirect};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cookies([data]);
    let _ = hyscribe_kses::parse_url(data);
    let url_chars = |b: &u8| b.is_ascii_alphanumeric() || b"-~+_.?#=&;,/:%!*[]()@".contains(b);
    assert!(wp_sanitize_redirect(data).iter().all(url_chars));
    let checked = wp_validate_redirect(data, b"example.test", b"FALLBACK");
    assert!(checked.iter().all(url_chars));
});
