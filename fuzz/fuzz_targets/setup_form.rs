// SPDX-License-Identifier: AGPL-3.0-or-later

//! Setup mode's form reader never panics, under any content type, and what
//! it reads holds each field once, at most 32 of them.

#![no_main]

use hyscribe::fuzzing::read_form;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = usize::from(data.first().copied().unwrap_or(0)).min(data.len());
    let (kind, body) = data.split_at(split);
    let _ = read_form(kind, body);
    if let Some(form) = read_form(b"application/x-www-form-urlencoded", data) {
        assert!(form.len() <= 32);
        for (at, (name, _)) in form.iter().enumerate() {
            assert!(form.iter().skip(at + 1).all(|(other, _)| other != name));
        }
    }
});
