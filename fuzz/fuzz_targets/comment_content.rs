// SPDX-License-Identifier: AGPL-3.0-or-later

//! A comment's content through kses and `wp_rel_ugc()`, as it is saved,
//! accepts any bytes, and what it returns holds only `$allowedtags` (and
//! `span`) and no event handler attribute.

#![no_main]

use hyscribe_html::TagProcessor;
use hyscribe_render::kses::{wp_kses_comment, wp_rel_ugc};
use libfuzzer_sys::fuzz_target;

const ALLOWED: [&[u8]; 15] = [
    b"A", b"ABBR", b"ACRONYM", b"B", b"BLOCKQUOTE", b"CITE", b"CODE", b"DEL", b"EM", b"I", b"Q",
    b"S", b"STRIKE", b"STRONG", b"SPAN",
];

fuzz_target!(|data: &[u8]| {
    let clean = wp_rel_ugc(&wp_kses_comment(data), b"http://example.com");
    let mut p = TagProcessor::new(&clean);
    while p.next_tag(None, None) {
        let tag = p.get_tag().unwrap_or_default();
        assert!(ALLOWED.contains(&tag.as_slice()), "{tag:?} in {clean:?}");
        let handlers = p.get_attribute_names_with_prefix(b"on").unwrap_or_default();
        assert!(handlers.is_empty(), "{handlers:?} in {clean:?}");
    }
});
