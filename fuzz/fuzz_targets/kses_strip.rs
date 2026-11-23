// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_strip_all_tags()` never grows its input, and every `<` it leaves
//! is followed by whitespace or ends the text, so no tag survives.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for remove_breaks in [false, true] {
        let text = hyscribe_kses::wp_strip_all_tags(data, remove_breaks);
        assert!(text.len() <= data.len());
        for (i, _) in text.iter().enumerate().filter(|&(_, &b)| b == b'<') {
            assert!(
                text.get(i + 1)
                    .is_none_or(|&b| b.is_ascii_whitespace() || b == 0x0B),
                "{:?}",
                String::from_utf8_lossy(&text)
            );
        }
    }
});
