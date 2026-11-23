// SPDX-License-Identifier: AGPL-3.0-or-later

//! The small text parsers never panic: `sanitize_text_field()`,
//! `sanitize_user()`, `sanitize_email()`, `is_email()`, `sanitize_title()`,
//! a URL's query, permalink structure tags, PHP's numeric
//! strings, `urldecode()` and natural order comparison, which is
//! antisymmetric.

#![no_main]

use hyscribe_kses::url_query;
use hyscribe_phpser::{string_to_float, strnatcasecmp, urldecode};
use hyscribe_render::formatting::{
    is_email, sanitize_email, sanitize_text_field, sanitize_title, sanitize_user,
};
use hyscribe_render::permalinks::tokens;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = sanitize_text_field(data);
    let _ = sanitize_user(data);
    let _ = is_email(&sanitize_email(data));
    let _ = sanitize_title(data);
    assert!(url_query(data).is_none_or(|query| query.len() < data.len()));
    let _ = tokens(&String::from_utf8_lossy(data));
    let _ = string_to_float(data);
    assert!(urldecode(data).len() <= data.len());
    let (a, b) = data.split_at(data.len() / 2);
    assert_eq!(strnatcasecmp(a, b), strnatcasecmp(b, a).reverse());
});
