// SPDX-License-Identifier: AGPL-3.0-or-later

//! `json_decode` against PHP's own `json_decode($json, true)`: for every
//! case, serializing our result gives PHP's bytes, and errors match
//! `json_last_error()` and `json_last_error_msg()`. Goldens come from
//! `reference/blocks/golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
use hyscribe_testkit::unhex;

#[test]
fn matches_php_json_decode() {
    let golden = include_str!("golden/json-decode.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [input, expected, code, message] = fields[..] else {
            panic!("{line}");
        };
        let (input, expected) = (unhex(input), unhex(expected));
        let decoded = hyscribe_phpser::json_decode_checked(&input);
        let (ours, error) = match &decoded {
            Ok(value) => (hyscribe_phpser::encode(value), (0, "No error")),
            // PHP returns null on errors.
            Err(error) => (b"N;".to_vec(), (error.0, error.message())),
        };
        assert_eq!(hyscribe_phpser::json_decode(&input), decoded.ok());
        if ours != expected || error != (code.parse().unwrap(), message) {
            failures.push(format!(
                "{:?}\n  php:  {} {code} {message}\n  ours: {} {error:?}",
                String::from_utf8_lossy(&input)
                    .chars()
                    .take(80)
                    .collect::<String>(),
                String::from_utf8_lossy(&expected)
                    .chars()
                    .take(120)
                    .collect::<String>(),
                String::from_utf8_lossy(&ours)
                    .chars()
                    .take(120)
                    .collect::<String>()
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(count, 718);
}
