// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use hyscribe_testkit::unhex;

/// `reference/text-diff/golden.sh`'s pairs: PHP's tables, or its fatal error.
#[test]
fn diffs_are_php_s() {
    let golden = include_str!("../../tests/golden/text-diff.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (left, right, php) = (unhex(f[0]), unhex(f[1]), unhex(f[2]));
        let ours = wp_text_diff(&left, &right).map_or(b"FATAL".to_vec(), String::into_bytes);
        if ours != php {
            failures.push(format!(
                "{:?} -> {:?}\nphp:  {}\nours: {}",
                String::from_utf8_lossy(&left),
                String::from_utf8_lossy(&right),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert_eq!(golden.lines().count(), 639);
    assert!(
        failures.is_empty(),
        "{} of 639:\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
