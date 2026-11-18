// SPDX-License-Identifier: AGPL-3.0-or-later

//! The shortcode engine takes any bytes: every match lies inside the text
//! and replacing each by itself changes nothing, the attributes of any text
//! come back as an array, and stripping and unwrapping end.

#![no_main]

use hyscribe_render::formatting::CORE_SHORTCODE_TAGS;
use hyscribe_render::shortcode::{
    replace_shortcodes, shortcode_parse_atts, shortcode_unautop, strip_shortcodes, tags_in,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = shortcode_parse_atts(data);
    let tags = tags_in(&CORE_SHORTCODE_TAGS, data);
    let same = replace_shortcodes(data, &tags, &mut |shortcode| {
        assert!(shortcode.start < shortcode.end && shortcode.end <= data.len());
        assert_eq!(Some(shortcode.whole), data.get(shortcode.start..shortcode.end));
        shortcode.whole.to_vec()
    });
    assert_eq!(same, data);
    let _ = strip_shortcodes(data, &CORE_SHORTCODE_TAGS);
    let _ = shortcode_unautop(data, &CORE_SHORTCODE_TAGS);
});
