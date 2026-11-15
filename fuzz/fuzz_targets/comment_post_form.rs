// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp-comments-post.php`'s body parser never panics, under any content
//! type, and what it reads is a form.

#![no_main]

use hyscribe_http::fuzzing::read_form;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = usize::from(data.first().copied().unwrap_or(0)).min(data.len());
    let (kind, body) = data.split_at(split);
    let _ = read_form(kind, body);
    let form = read_form(b"application/x-www-form-urlencoded", data);
    assert!(form.is_none_or(|form| form.as_array().is_some()));
});
