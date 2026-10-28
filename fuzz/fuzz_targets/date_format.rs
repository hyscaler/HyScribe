// SPDX-License-Identifier: AGPL-3.0-or-later

//! Date formatting and `strtotime()` accept any input without panicking.

#![no_main]

use hyscribe_render::date::{self, Zone};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let middle = (0..=text.len() / 2)
        .rev()
        .find(|&i| text.is_char_boundary(i))
        .unwrap_or(0);
    let (format, stored) = text.split_at(middle);
    let _ = date::format(format, stored, Zone::Offset(-34_200));
    let _ = date::site_zone(format.as_bytes(), stored.as_bytes());
    let _ = date::strtotime(&text);
});
