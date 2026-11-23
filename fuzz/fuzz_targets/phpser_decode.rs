// SPDX-License-Identifier: AGPL-3.0-or-later

//! Any input either fails to decode or round trips exactly.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(value) = hyscribe_phpser::decode(data) {
        let encoded = hyscribe_phpser::encode(&value);
        assert_eq!(hyscribe_phpser::decode(&encoded), Ok(value));
    }
});
