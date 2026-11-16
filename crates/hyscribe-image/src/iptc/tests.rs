// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::bin2hex;
use hyscribe_testkit::unhex;

use super::*;

#[test]
fn matches_iptcparse() {
    let golden = include_str!("../../tests/golden/iptc.tsv");
    let mut wrong = Vec::new();
    for line in golden.lines() {
        let (hex, php) = line.split_once('\t').unwrap();
        let ours = iptcparse(&unhex(hex)).map_or("false".to_string(), |sets| {
            let sets = sets.iter().map(|(key, values)| {
                let values: Vec<String> = values.iter().map(|v| bin2hex(v)).collect();
                format!("{key}={}", values.join(","))
            });
            sets.collect::<Vec<_>>().join(";")
        });
        if ours != php {
            wrong.push(format!("{hex}: {ours} != {php}"));
        }
    }
    assert!(golden.lines().count() > 500);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
