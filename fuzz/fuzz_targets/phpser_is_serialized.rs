// SPDX-License-Identifier: AGPL-3.0-or-later

//! The WordPress helpers never panic, and strict implies loose.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if hyscribe_phpser::is_serialized(data, true) {
        assert!(hyscribe_phpser::is_serialized(data, false));
    }
    let _ = hyscribe_phpser::maybe_unserialize(data);
});
