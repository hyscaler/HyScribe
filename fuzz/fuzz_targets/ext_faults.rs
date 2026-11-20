// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a fault record (`faults.txt`, ADR 0022) never panics, and what
//! it reads it writes back to a record that reads the same.

#![no_main]

use hyscribe_ext::faults::Record;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Some(record) = Record::parse(bytes) {
        let text = record.text();
        assert!(text.len() <= 4 << 10);
        assert!(text.lines().count() <= 4);
        assert_eq!(Record::parse(text.as_bytes()), Some(record));
    }
});
