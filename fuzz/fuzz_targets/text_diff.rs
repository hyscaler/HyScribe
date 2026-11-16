// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_text_diff()` takes any two texts, split at the input's first NUL:
//! equal texts give no table, and a table always closes.

#![no_main]

use hyscribe_render::text_diff::wp_text_diff;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let at = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    let (left, right) = (&data[..at], data.get(at + 1..).unwrap_or_default());
    if let Some(table) = wp_text_diff(left, right) {
        assert!(table.is_empty() || table.ends_with("</table>"));
    }
    assert_eq!(wp_text_diff(left, left).as_deref(), Some(""));
});
