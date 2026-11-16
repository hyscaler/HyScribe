// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_specialchars_decode()` never grows its input, keeps a text without
//! `&` as it is, and without quotes decodes no quote.

#![no_main]

use hyscribe_mail::wp_specialchars_decode;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let all = wp_specialchars_decode(data, true);
    let none = wp_specialchars_decode(data, false);
    assert!(all.len() <= none.len() && none.len() <= data.len());
    if !data.contains(&b'&') {
        assert_eq!(all, data);
    }
    let quotes = |text: &[u8]| text.iter().filter(|&&b| b == b'"' || b == b'\'').count();
    assert_eq!(quotes(&none), quotes(data));
});
