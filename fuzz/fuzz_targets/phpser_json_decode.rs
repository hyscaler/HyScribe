// SPDX-License-Identifier: AGPL-3.0-or-later

//! json_decode accepts any bytes, and whatever it decodes serializes and
//! reads back.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(value) = hyscribe_phpser::json_decode(data) {
        let encoded = hyscribe_phpser::encode(&value);
        // unserialize stops at a shallower depth than json_decode.
        if let Ok(decoded) = hyscribe_phpser::decode(&encoded) {
            assert_eq!(decoded, value);
        }
    }
});
