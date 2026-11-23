// SPDX-License-Identifier: AGPL-3.0-or-later

//! `force_balance_tags()` takes any bytes without panicking or hanging.

#![no_main]

use hyscribe_render::balance::force_balance_tags;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = force_balance_tags(data);
});
