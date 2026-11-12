// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/redirect/golden.sh`, on the oracle's host.
#[test]
fn redirects_are_checked_as_php_checks_them() {
    let golden = include_str!("../../tests/golden/redirect.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let (location, sanitized, validated) = (&fields[0], &fields[1], &fields[2]);
        let ours = (
            wp_sanitize_redirect(location),
            wp_validate_redirect(location, b"127.0.0.1", b"FALLBACK"),
        );
        if ours != (sanitized.clone(), validated.clone()) {
            failures.push(format!(
                "{:?}: php {:?} {:?}, ours {:?} {:?}",
                String::from_utf8_lossy(location),
                String::from_utf8_lossy(sanitized),
                String::from_utf8_lossy(validated),
                String::from_utf8_lossy(&ours.0),
                String::from_utf8_lossy(&ours.1)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 1575);
}

/// A home URL's host is compared as given and lowercased.
#[test]
fn the_home_host_matches_in_either_case() {
    for location in ["http://Example.TEST/x", "http://example.test/x"] {
        assert_eq!(
            wp_validate_redirect(location.as_bytes(), b"Example.TEST", b"F"),
            location.as_bytes()
        );
    }
    assert_eq!(
        wp_validate_redirect(b"http://EXAMPLE.test/x", b"Example.TEST", b"F"),
        b"F"
    );
}
