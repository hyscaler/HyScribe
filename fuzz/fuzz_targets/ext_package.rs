// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a `.hyx` package (ADR 0022) never panics, and what it accepts
//! stays within the format's bounds: relative names, no more than its
//! limits.

#![no_main]

use hyscribe_ext::hyx::{Hyx, MAX_TOTAL};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(hyx) = Hyx::read(bytes) {
        let total: usize = hyx.files.iter().map(|(_, data)| data.len()).sum();
        assert!(total as u64 <= MAX_TOTAL);
        for (name, _) in &hyx.files {
            assert!(!name.starts_with('/') && !name.split('/').any(|step| step == ".."));
        }
    }
});
