// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/formatting/golden.sh autop`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/wpautop.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let (input, br, php) = (unhex(fields[0]), fields[1] == "1", unhex(fields[2]));
        let ours = wpautop(&input, br);
        if ours != php {
            failures.push(format!(
                "{:?} br={br}\n  php:  {:?}\n  ours: {:?}",
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
    assert!(golden.lines().count() > 6000);
}
