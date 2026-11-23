// SPDX-License-Identifier: AGPL-3.0-or-later

//! `strnatcasecmp()` against PHP over every pair of
//! `reference/phpser/natcmp-golden.php`'s strings.

// Tests fail by panicking.
#![allow(clippy::panic, clippy::indexing_slicing)]

use std::cmp::Ordering;

use hyscribe_phpser::strnatcasecmp;
use hyscribe_testkit::unhex;

#[test]
fn matches_php() {
    let golden = include_str!("golden/natcmp.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (a, b) = (unhex(fields[0]), unhex(fields[1]));
        let php = match fields[2] {
            "-1" => Ordering::Less,
            "0" => Ordering::Equal,
            _ => Ordering::Greater,
        };
        if strnatcasecmp(&a, &b) != php {
            failures.push(format!("{a:?} {b:?}: php {php:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{}: {:?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
    assert_eq!(golden.lines().count(), 82 * 82);
}
