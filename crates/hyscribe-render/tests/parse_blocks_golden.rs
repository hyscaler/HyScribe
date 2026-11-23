// SPDX-License-Identifier: AGPL-3.0-or-later

//! `parse_blocks` against WordPress's `WP_Block_Parser`: PHP's
//! `serialize()` of its result and of ours are the same bytes for every
//! case. Goldens come from `reference/blocks/golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic)]

use hyscribe_render::blocks::{list, parse_blocks};
use hyscribe_testkit::unhex;

#[test]
fn matches_wp_block_parser() {
    let golden = include_str!("golden/parse-blocks.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        let (input, expected) = (unhex(input), unhex(expected));
        let ours = hyscribe_phpser::encode(&list(&parse_blocks(&input)));
        if ours != expected {
            failures.push(format!(
                "{:?}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&input)
                    .chars()
                    .take(100)
                    .collect::<String>(),
                String::from_utf8_lossy(&expected)
                    .chars()
                    .take(300)
                    .collect::<String>(),
                String::from_utf8_lossy(&ours)
                    .chars()
                    .take(300)
                    .collect::<String>()
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(count, 1662);
}
