// SPDX-License-Identifier: AGPL-3.0-or-later

//! The parser against `reference/url-details/parse-golden.php`, PHP's own
//! methods on hostile snippets, and its time on hostile pages.

use std::time::{Duration, Instant};

use hyscribe_testkit::unhex;

use crate::url_details::details;

#[test]
fn pages_are_read_as_php_reads_them() {
    let golden = include_str!("../../../tests/golden/url-details-parse.tsv");
    let mut count = 0;
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let [page, url, expected @ ..] = fields.as_slice() else {
            panic!("{line}");
        };
        let read = details(page, url);
        let show = |parts: &[Vec<u8>]| {
            parts
                .iter()
                .map(|part| String::from_utf8_lossy(part).into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            show(&read),
            show(expected),
            "{}",
            String::from_utf8_lossy(page)
        );
        count += 1;
    }
    assert_eq!(count, 83);
}

#[test]
fn hostile_pages_take_linear_time() {
    let pages = [
        "<meta ".repeat(40_000) + ">",
        "<link ".repeat(40_000) + ">",
        "<meta content=\"".repeat(20_000) + ">",
        "<link rel=".repeat(20_000) + ">",
        "<title>".to_string() + &"< /".repeat(50_000),
        "<meta content='x name=description ".repeat(10_000) + "'>",
    ];
    for page in pages {
        let started = Instant::now();
        details(page.as_bytes(), b"http://pages.test/");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{}",
            page.get(..20).unwrap()
        );
    }
}
