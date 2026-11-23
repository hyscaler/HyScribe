// SPDX-License-Identifier: AGPL-3.0-or-later

//! Hello Elementor's `version_compare()` reads any stored version and
//! ends without panicking (MVP 10.5).

#![no_main]

use hyscribe_theme_hello_elementor::version::version_compare;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let (a, b) = text.split_once('\0').unwrap_or((&text, "3.5.2"));
    let _ = (version_compare(a, b), version_compare(b, a));
});
