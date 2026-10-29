// SPDX-License-Identifier: AGPL-3.0-or-later

//! Query var parsing accepts any request input without panicking.

#![no_main]

use hyscribe_http::query;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = query::absint(&text);
    let _ = query::date_parts(&text);
    let _ = hyscribe_render::search_terms(&text);
    let _ = query::other_args(Some(&text), &["p", "page"]);
});
