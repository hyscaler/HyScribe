// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::check;

#[test]
fn matches_wp_theme_json() {
    check("media_queries", |a| {
        Value::Array(
            media_queries(Some(&a[0]))
                .iter()
                .map(|(k, v)| (php::key(k), string(v)))
                .collect(),
        )
    });
    check("viewport", |a| sanitize(&a[0]));
    assert_eq!(media_queries(None), media_queries(Some(&Value::Null)));
}
