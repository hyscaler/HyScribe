// SPDX-License-Identifier: AGPL-3.0-or-later

//! `make_clickable()` takes any bytes without panicking or hanging.

#![no_main]

use hyscribe_render::clickable::make_clickable;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = make_clickable(data, b"http://example.com", true);
});
