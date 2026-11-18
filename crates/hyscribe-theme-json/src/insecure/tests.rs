// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Value;

use crate::tests::{check, resolved};

/// `remove_insecure_properties( $theme_json, 'custom' )` against PHP for
/// the `insecure` cases of `reference/theme-json/cases.txt`.
#[test]
fn insecure_values_go_as_in_php() {
    check("insecure", |args| {
        resolved().remove_insecure_properties(args[0].clone())
    });
}

/// `wp_filter_global_styles_post()` against PHP, whose result is slashed,
/// for the `filter_styles` cases.
#[test]
fn global_styles_posts_are_filtered_as_in_php() {
    check("filter_styles", |args| {
        let content = args[0].as_bytes().unwrap();
        let Some(filtered) = resolved().filter_global_styles_post(content) else {
            return args[0].clone();
        };
        let mut slashed = Vec::new();
        for byte in filtered {
            if matches!(byte, b'\'' | b'"' | b'\\') {
                slashed.push(b'\\');
            }
            slashed.push(byte);
        }
        Value::String(slashed)
    });
}
