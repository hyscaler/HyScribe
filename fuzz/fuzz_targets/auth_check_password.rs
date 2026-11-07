// SPDX-License-Identifier: AGPL-3.0-or-later

//! Checking a password against any stored hash never panics. Costs that
//! would take minutes are skipped.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&split, rest)) = data.split_first() else {
        return;
    };
    let (password, hash) = rest.split_at(usize::from(split).min(rest.len()));
    let setting = hash.strip_prefix(b"$wp").unwrap_or(hash);
    let slow = match setting {
        [b'$', b'2', _, b'$', tens, ones, ..] => (tens, ones) > (&b'0', &b'5'),
        [b'$', b'P', b'$', count, ..] => count > &b'8',
        _ => false,
    };
    if !slow {
        hyscribe_auth::check_password(password, hash);
    }
});
