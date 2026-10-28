// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every function against PHP WordPress: the same output bytes for every
//! recorded input. Goldens come from `reference/kses/golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic)]

use hyscribe_kses::{
    ALLOWED_PROTOCOLS, esc_attr, esc_html, safecss_filter_attr, sanitize_key, wp_kses_bad_protocol,
    wp_kses_decode_entities, wp_kses_no_null, wp_kses_normalize_entities, wp_strip_all_tags,
};
use hyscribe_testkit::unhex;

/// Compares `ours` with every line of a golden; "throws" stands for an
/// empty result, as the port returns where PHP throws.
fn check(golden: &str, expected_count: usize, ours: impl Fn(&[u8]) -> Vec<u8>) {
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        let input = unhex(input);
        let expected = if expected == "throws" {
            Vec::new()
        } else {
            unhex(expected)
        };
        let got = ours(&input);
        if got != expected {
            failures.push(format!(
                "{:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&input),
                String::from_utf8_lossy(&expected),
                String::from_utf8_lossy(&got)
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(count, expected_count);
}

#[test]
fn esc_html_matches() {
    check(include_str!("golden/esc-html.tsv"), 1815, |t| {
        esc_html(t).into_bytes()
    });
}

#[test]
fn esc_attr_matches() {
    check(include_str!("golden/esc-attr.tsv"), 1815, |t| {
        esc_attr(t).into_bytes()
    });
}

#[test]
fn normalize_entities_matches() {
    check(
        include_str!("golden/normalize-entities.tsv"),
        1496,
        wp_kses_normalize_entities,
    );
}

#[test]
fn decode_entities_matches() {
    check(include_str!("golden/decode-entities.tsv"), 1311, |t| {
        wp_kses_decode_entities(t).unwrap_or_default()
    });
    // Where PHP throws, the port says so.
    assert_eq!(wp_kses_decode_entities(b"&#99999999999999999999;"), None);
}

#[test]
fn no_null_matches() {
    check(include_str!("golden/no-null.tsv"), 1326, wp_kses_no_null);
}

#[test]
fn bad_protocol_matches() {
    check(include_str!("golden/bad-protocol.tsv"), 1614, |t| {
        wp_kses_bad_protocol(t, &ALLOWED_PROTOCOLS)
    });
}

#[test]
fn strip_all_tags_matches() {
    check(include_str!("golden/strip-all-tags.tsv"), 1602, |t| {
        wp_strip_all_tags(t, false)
    });
    check(
        include_str!("golden/strip-all-tags-breaks.tsv"),
        1494,
        |t| wp_strip_all_tags(t, true),
    );
}

#[test]
fn sanitize_key_matches() {
    check(include_str!("golden/sanitize-key.tsv"), 1399, |t| {
        sanitize_key(t).into_bytes()
    });
}

#[test]
fn safecss_filter_attr_matches() {
    check(
        include_str!("golden/safecss-filter-attr.tsv"),
        1601,
        safecss_filter_attr,
    );
}

/// Inputs at the edges of PCRE's JIT stack and backtrack limits.
#[test]
fn limits_match() {
    let mut count = 0;
    for line in include_str!("golden/limits.tsv").lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [function, prefix, unit, repeat, suffix, expected] = fields[..] else {
            panic!("bad line {line}");
        };
        let input = [
            unhex(prefix),
            unhex(unit).repeat(repeat.parse().unwrap()),
            unhex(suffix),
        ]
        .concat();
        let got = match function {
            "wp_strip_all_tags" => wp_strip_all_tags(&input, false),
            "safecss_filter_attr" => safecss_filter_attr(&input),
            _ => panic!("unknown function {function}"),
        };
        match expected.strip_prefix("len:") {
            _ if expected == "=" => assert!(got == input, "{function} {repeat}: changed"),
            Some(len) => assert_eq!(
                got.len(),
                len.parse::<usize>().unwrap(),
                "{function} {repeat}"
            ),
            None => assert_eq!(got, unhex(expected), "{function} {repeat}"),
        }
        count += 1;
    }
    assert_eq!(count, 22);
}
