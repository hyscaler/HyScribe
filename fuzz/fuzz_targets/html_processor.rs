// SPDX-License-Identifier: AGPL-3.0-or-later

//! The HTML processor walks any bytes to an end or an error without
//! panicking, opening at most one element per token.

#![no_main]

use hyscribe_html::HtmlProcessor;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|html: &[u8]| {
    let mut p = HtmlProcessor::create_fragment(html);
    let mut depth = p.get_current_depth();
    while p.next_token() {
        assert!(p.get_current_depth() <= depth + 1);
        p.get_breadcrumbs();
        p.get_token_type();
        p.get_token_name();
        p.is_tag_closer();
        p.get_namespace();
        p.get_modifiable_text();
        depth = p.get_current_depth();
    }
    assert!(!p.next_token());
    p.get_last_error();
});
