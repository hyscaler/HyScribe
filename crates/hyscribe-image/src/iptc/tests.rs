// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::bin2hex;
use hyscribe_testkit::unhex;

use super::*;
use crate::header::getimagesize;
use crate::tests::hostile;

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

#[test]
fn reads_distinct_keys_in_linear_time() {
    // Every key there is, once, from record 1 on.
    let iim: Vec<u8> = (1..=255u8)
        .chain([0])
        .flat_map(|r| (0..=255u8).flat_map(move |d| [0x1C, r, d, 0, 0]))
        .collect();
    let start = std::time::Instant::now();
    let sets = iptcparse(&iim).unwrap();
    assert_eq!(sets.len(), 65_536);
    assert_eq!(
        (sets[0].0.as_str(), sets[65_535].0.as_str()),
        ("1#000", "0#255")
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    // The most a JPEG's APP13 holds.
    let jpeg = hostile("iptc-keys.jpg");
    let app13 = getimagesize(&jpeg).unwrap().app13.unwrap();
    assert_eq!(iptcparse(app13).unwrap().len(), 13_106);
}
