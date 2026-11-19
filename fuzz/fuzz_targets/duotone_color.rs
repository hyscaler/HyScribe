// SPDX-License-Identifier: AGPL-3.0-or-later

//! A duotone color string and a duotone attribute accept any bytes: a color
//! that parses has channels in range, and a slug has no white space.

#![no_main]

use hyscribe_render::duotone::parse_color;
use hyscribe_theme_json::duotone_slug;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some([r, g, b, a]) = parse_color(data) {
        for channel in [r, g, b] {
            assert!((0.0..=255.0).contains(&channel), "{channel} in {data:?}");
        }
        assert!((0.0..=1.0).contains(&a), "{a} in {data:?}");
    }
    let slug = duotone_slug(data);
    let space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r');
    assert!(!slug.bytes().any(space), "{slug:?} in {data:?}");
});
