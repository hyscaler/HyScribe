// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/formatting/golden.sh balance`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/force-balance-tags.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (input, php) = line.split_once('\t').unwrap();
        let (input, php) = (unhex(input), unhex(php));
        let ours = force_balance_tags(&input);
        if ours != php {
            failures.push(format!(
                "{:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&input),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(6)].join("\n")
    );
    assert!(golden.lines().count() > 3000);
}
