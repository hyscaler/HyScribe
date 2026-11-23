// SPDX-License-Identifier: AGPL-3.0-or-later

//! The codec against values serialized by real PHP
//! (`reference/phpser/golden.php`).

// Tests fail by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use hyscribe_phpser::{Float, Value, decode, encode};
use hyscribe_testkit::unhex;

fn golden() -> Vec<Vec<String>> {
    include_str!("golden/php-serialize.tsv")
        .lines()
        .map(|line| line.split('\t').map(String::from).collect())
        .collect()
}

#[test]
fn every_php_value_round_trips() {
    let mut seen = 0;
    for fields in golden().iter().filter(|f| f[0] == "value") {
        let bytes = unhex(&fields[2]);
        let value = decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", fields[1]));
        assert_eq!(encode(&value), bytes, "{}", fields[1]);
        seen += 1;
    }
    assert!(seen >= 20);
}

#[test]
fn floats_format_and_round_trip_like_php() {
    let mut seen = 0;
    for fields in golden().iter().filter(|f| f[0] == "float") {
        let bits: [u8; 8] = unhex(&fields[1]).try_into().unwrap();
        // `pack('E')` in the generator is big-endian.
        let number = f64::from_be_bytes(bits);
        let bytes = unhex(&fields[2]);
        let expected = Value::Float(Float::from_f64(number));
        assert_eq!(encode(&expected), bytes, "{number:e}");
        assert_eq!(decode(&bytes).unwrap(), expected);
        let Value::Float(decoded) = decode(&bytes).unwrap() else {
            unreachable!()
        };
        assert_eq!(decoded.to_f64().to_bits(), number.to_bits(), "{number:e}");
        seen += 1;
    }
    assert!(seen > 4000);
}
