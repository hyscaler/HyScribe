// SPDX-License-Identifier: AGPL-3.0-or-later

//! `add_query_arg()` and `remove_query_arg()` take any URI without
//! panicking or hanging.

#![no_main]

use hyscribe_phpser::{add_query_arg, remove_query_arg};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = remove_query_arg(&[b"replytocom", b"unapproved"], data);
    let _ = add_query_arg(&[(b"a", Some(b"1")), (b"0", None)], data);
});
