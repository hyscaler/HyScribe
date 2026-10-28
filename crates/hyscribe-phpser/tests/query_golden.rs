// SPDX-License-Identifier: AGPL-3.0-or-later

//! `parse_str` against PHP's own `parse_str()`: serializing our result gives
//! PHP's bytes. Goldens come from `reference/phpser/query-golden.php`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
use hyscribe_phpser::{add_query_arg, encode, parse_str, remove_query_arg};
use hyscribe_testkit::unhex;

#[test]
fn matches_php_parse_str() {
    let golden = include_str!("golden/parse-str.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (query, php) = line.split_once('\t').unwrap();
        let (query, php) = (unhex(query), unhex(php));
        let ours = encode(&parse_str(&query));
        if ours != php {
            failures.push(format!(
                "{:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&query),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(5)].join("\n")
    );
    assert!(golden.lines().count() > 3000);
}

/// `add_query_arg()` and `remove_query_arg()` against WordPress's, from
/// `reference/query-arg/golden.sh`.
#[test]
fn matches_wordpress_add_query_arg() {
    let golden = include_str!("golden/add-query-arg.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let uri = &fields[0];
        let ours = [
            remove_query_arg(&[b"replytocom", b"unapproved", b"moderation-hash"], uri),
            add_query_arg(&[(b"x", Some(b"1"))], uri),
            add_query_arg(
                &[
                    (b"a", Some(b"1 2")),
                    (b"b", None),
                    (b"0", Some(b"z")),
                    (b"p", Some(b"9")),
                ],
                uri,
            ),
        ];
        for (ours, php) in ours.iter().zip(&fields[1..]) {
            if ours != php {
                failures.push(format!(
                    "{:?}\n  php:  {:?}\n  ours: {:?}",
                    String::from_utf8_lossy(uri),
                    String::from_utf8_lossy(php),
                    String::from_utf8_lossy(ours)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(5)].join("\n")
    );
    assert!(golden.lines().count() > 2500);
}
