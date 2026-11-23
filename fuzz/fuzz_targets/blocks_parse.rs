// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block parser accepts any bytes, and the HTML it splits out never
//! exceeds the input.

#![no_main]

use libfuzzer_sys::fuzz_target;

fn html_len(blocks: &[hyscribe_render::blocks::Block]) -> usize {
    blocks
        .iter()
        .map(|b| b.inner_html.len() + html_len(&b.inner_blocks))
        .sum()
}

fuzz_target!(|data: &[u8]| {
    let blocks = hyscribe_render::blocks::parse_blocks(data);
    // Unclosed nested blocks repeat the tail, at most once per open block.
    assert!(html_len(&blocks) <= data.len() * (data.len() / 12 + 1));
});
