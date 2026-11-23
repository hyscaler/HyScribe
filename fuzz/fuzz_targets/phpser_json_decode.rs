// SPDX-License-Identifier: AGPL-3.0-or-later

//! json_decode accepts any bytes, whatever it decodes serializes and reads
//! back, and its JSON encoding is stable.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(value) = hyscribe_phpser::json_decode(data) {
        let encoded = hyscribe_phpser::encode(&value);
        // unserialize stops at a shallower depth than json_decode.
        if let Ok(decoded) = hyscribe_phpser::decode(&encoded) {
            assert_eq!(decoded, value);
        }
        // What decodes encodes to JSON that decodes, and the encoding
        // settles after one more round. Not at once: as in PHP, `-0.0`
        // encodes as `-0`, which decodes as the int 0 and encodes as `0`.
        let round = |json: &[u8]| {
            let decoded = hyscribe_phpser::json_decode(json).expect("encoded JSON decodes");
            hyscribe_phpser::json_encode(&decoded)
        };
        if let Some(once) = hyscribe_phpser::json_encode(&value).and_then(|json| round(&json)) {
            assert_eq!(round(&once), Some(once));
        }
    }
});
