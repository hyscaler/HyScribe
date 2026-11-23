// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading an extension's manifest (`hyscribe.ext.toml`, ADR 0022) never
//! panics, and what it accepts passes the grants' own checks.

#![no_main]

use hyscribe_ext::Manifest;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    if let Ok(manifest) = Manifest::parse(bytes) {
        assert!(manifest.grants.check().is_ok());
        assert!(!manifest.id.is_empty() && manifest.id.len() <= 100);
    }
});
