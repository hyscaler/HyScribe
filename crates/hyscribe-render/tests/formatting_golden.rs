// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wptexturize()` and `convert_smilies()` against WordPress's: the same
//! bytes for every case. Goldens come from `reference/formatting/golden.sh`,
//! run against the default install fixture.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic)]

use hyscribe_render::formatting::{CORE_SHORTCODE_TAGS, convert_smilies, wptexturize};
use hyscribe_testkit::unhex;

/// Core's shortcodes, plus the `code` one the PHP generator adds.
fn shortcode_tags() -> Vec<&'static [u8]> {
    CORE_SHORTCODE_TAGS
        .into_iter()
        .chain([&b"code"[..]])
        .collect()
}

/// `includes_url( 'images/smilies/' )` of the fixture's PHP container.
const SMILIES_URL: &[u8] = b"http://127.0.0.1:8081/wp-includes/images/smilies/";

fn check(golden: &str, expected_count: usize, ours: impl Fn(&[u8]) -> Vec<u8>) {
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        let (input, expected) = (unhex(input), unhex(expected));
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
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(count, expected_count);
}

#[test]
fn matches_wptexturize() {
    check(include_str!("golden/wptexturize.tsv"), 859, |text| {
        wptexturize(text, &shortcode_tags())
    });
}

#[test]
fn matches_convert_smilies() {
    check(include_str!("golden/convert-smilies.tsv"), 555, |text| {
        convert_smilies(text, SMILIES_URL)
    });
}
