// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tag processor accepts any bytes: walking it changes nothing, and
//! edits chosen by the first bytes never panic or hang.

#![no_main]

use hyscribe_html::TagProcessor;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (ops, html) = data.split_at(data.len().min(16));

    let mut p = TagProcessor::new(html);
    let mut tags = 0;
    while p.next_tag(None, None) {
        assert!(p.get_tag().is_some());
        tags += 1;
    }
    assert!(tags <= html.len() / 2);
    assert_eq!(p.get_updated_html(), html);

    let mut p = TagProcessor::new(html);
    for (i, &op) in ops.iter().enumerate() {
        let word = html.get(i..i + usize::from(op >> 4)).unwrap_or_default();
        match op % 10 {
            0 | 1 => {
                p.next_tag(None, None);
            }
            2 => {
                p.next_tag(Some(word), None);
            }
            3 => {
                p.set_attribute(word, html);
            }
            4 => {
                p.set_attribute(b"href", word);
            }
            5 => {
                p.remove_attribute(word);
            }
            6 => {
                p.add_class(word);
            }
            7 => {
                p.remove_class(word);
            }
            8 => {
                p.get_attribute(word);
                p.class_list();
            }
            _ => {
                p.get_updated_html();
            }
        }
    }
    p.get_updated_html();
});
