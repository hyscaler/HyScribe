// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_kses_post()` accepts any bytes, and what it returns holds no script
//! carrying tag and no event handler attribute.

#![no_main]

use hyscribe_html::TagProcessor;
use hyscribe_render::kses::wp_kses_post;
use libfuzzer_sys::fuzz_target;

// MathML is allowed: WordPress 7 lists it in `$allowedposttags` for the math block.
const FORBIDDEN: [&[u8]; 13] = [
    b"SCRIPT",
    b"STYLE",
    b"IFRAME",
    b"FRAME",
    b"SVG",
    b"EMBED",
    b"BASE",
    b"LINK",
    b"META",
    b"FORM",
    b"TEMPLATE",
    b"NOSCRIPT",
    b"TEXTAREA",
];

fuzz_target!(|data: &[u8]| {
    let clean = wp_kses_post(data, b"example.com");
    let mut p = TagProcessor::new(&clean);
    while p.next_tag(None, None) {
        let tag = p.get_tag().unwrap_or_default();
        assert!(!FORBIDDEN.contains(&tag.as_slice()), "{tag:?} in {clean:?}");
        let handlers = p.get_attribute_names_with_prefix(b"on").unwrap_or_default();
        assert!(handlers.is_empty(), "{handlers:?} in {clean:?}");
    }
});
