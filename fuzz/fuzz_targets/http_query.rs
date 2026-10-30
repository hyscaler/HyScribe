// SPDX-License-Identifier: AGPL-3.0-or-later

//! Query var parsing accepts any request input without panicking.

#![no_main]

use hyscribe_http::query;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = query::absint(&text);
    let _ = query::date_query(Some(&text), Some(&text), Some(&text), Some(&text));
    let _ = query::clean_query(&text);
    let vars = ["cat", "category_name", "tag", "post_format", "taxonomy", "term"]
        .map(|name| (name.to_string(), text.to_string()));
    let tax = query::tax_query(&vars.into_iter().collect());
    let _ = tax.lookups();
    let _ = tax.resolve(&[], data);
    let _ = hyscribe_render::search_terms(&text);
    let _ = query::other_args(Some(&text), &["p", "page"]);
});
