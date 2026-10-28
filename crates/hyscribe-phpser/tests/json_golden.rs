// SPDX-License-Identifier: AGPL-3.0-or-later

//! `json_decode` against PHP's own `json_decode($json, true)`: for every
//! case, serializing our result gives PHP's bytes, and errors match.
//! Goldens come from `reference/blocks/golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
use hyscribe_testkit::unhex;

#[test]
fn matches_php_json_decode() {
    let golden = include_str!("golden/json-decode.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        let (input, expected) = (unhex(input), unhex(expected));
        let ours = match hyscribe_phpser::json_decode(&input) {
            Some(value) => hyscribe_phpser::encode(&value),
            // PHP returns null on errors.
            None => b"N;".to_vec(),
        };
        if ours != expected {
            failures.push(format!(
                "{:?}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&input)
                    .chars()
                    .take(80)
                    .collect::<String>(),
                String::from_utf8_lossy(&expected)
                    .chars()
                    .take(120)
                    .collect::<String>(),
                String::from_utf8_lossy(&ours)
                    .chars()
                    .take(120)
                    .collect::<String>()
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(count, 713);
}
