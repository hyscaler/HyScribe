// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// PHP's answers, `reference/hello-elementor/version-golden.php`.
const GOLDEN: &str = include_str!("../../tests/golden/version-compare.tsv");

fn unhex(hex: &str) -> String {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(hex.get(at..at + 2).unwrap_or_default(), 16).unwrap_or(0))
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

#[test]
fn versions_compare_as_php_compares_them() {
    let mut failures = Vec::new();
    for line in GOLDEN.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (a, b) = (unhex(fields[0]), unhex(fields[1]));
        let expected = match fields[2] {
            "-1" => Ordering::Less,
            "0" => Ordering::Equal,
            _ => Ordering::Greater,
        };
        if version_compare(&a, &b) != expected {
            failures.push(format!("{a:?} {b:?}: PHP says {}", fields[2]));
        }
    }
    assert_eq!(GOLDEN.lines().count(), 3481);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn numbers_saturate_as_strtol_does() {
    assert_eq!(number(b"99999999999999999999"), i64::MAX);
    assert_eq!(number(b"12a"), 12);
}
