// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wptexturize()` and `convert_smilies()` accept any bytes, and leave text
//! without anything to replace as it is.

#![no_main]

use hyscribe_render::formatting::{convert_smilies, wptexturize};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let shortcodes: [&[u8]; 3] = [b"caption", b"code", b"gallery"];
    let texturized = wptexturize(data, &shortcodes);
    let smilies = convert_smilies(data, b"/wp-includes/images/smilies/");
    if !data.iter().any(|c| b"'\"`.-x&( ".contains(c)) {
        assert_eq!(texturized, data);
    }
    if !data.iter().any(|c| b":;8".contains(c)) {
        assert_eq!(smilies, data);
    }
});
