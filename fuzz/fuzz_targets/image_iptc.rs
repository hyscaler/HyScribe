// SPDX-License-Identifier: AGPL-3.0-or-later

//! The IPTC reader takes any `APP13` segment: every value it gives lies
//! within the input, under a `record#dataset` key.

#![no_main]

use hyscribe_image::iptc::iptcparse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(sets) = iptcparse(data) {
        assert!(!sets.is_empty());
        let total: usize = sets.iter().flat_map(|(_, values)| values.iter().map(|v| v.len() + 5)).sum();
        assert!(total <= data.len());
        assert!(sets.iter().all(|(key, values)| key.contains('#') && !values.is_empty()));
    }
});
