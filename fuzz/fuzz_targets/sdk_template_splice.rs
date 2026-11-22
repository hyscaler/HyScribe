// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The splicer of a theme's rendered page reads hostile output, markers
//! and kept bytes without panicking (ADR 0023).

#![no_main]

use hyscribe_sdk::page::splice;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let mut parts = data.split(|&b| b == 0);
    let out = String::from_utf8_lossy(parts.next().unwrap_or_default());
    let kept: Vec<Vec<u8>> = parts.map(<[u8]>::to_vec).collect();
    let _ = splice(&out, &kept);
});
