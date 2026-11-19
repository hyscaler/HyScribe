// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::{arg, check, check_except, text};

#[test]
fn matches_wp_theme_json() {
    // Only a value with a brace differs from PHP: it is unsafe.
    check_except(
        "is_safe",
        |a| Value::Bool(is_safe(arg(a, 0), arg(a, 1))),
        |a, php| match arg(a, 1).contains(['{', '}']) {
            true => b"b:0;".to_vec(),
            false => php,
        },
    );
    check("nest_custom", |a| text(&nest_custom(arg(a, 0), arg(a, 1))));
}

/// A value with a brace is dropped as an unsafe one, however
/// `safecss_filter_attr()` would split it; any other value is judged as PHP
/// judges it.
#[test]
fn values_with_braces_are_unsafe() {
    for value in ["red;} body{x:y}", "red;}", "{", "}", "a{b", "calc(1px) }"] {
        assert!(!is_safe("color", value), "{value}");
    }
    assert!(is_safe("color", "red"));
    assert!(is_safe("color", "red;x"));
}

#[test]
fn rulesets() {
    assert_eq!(ruleset("a", &[]), "");
    let declarations = [
        Declaration::text("color", "red"),
        Declaration::new("z-index", Value::Int(2)),
        Declaration::new("skipped", Value::Bool(true)),
    ];
    assert_eq!(ruleset("a", &declarations), "a{color: red;z-index: 2;}");
    assert_eq!(ruleset("a", &declarations[2..]), "a{}");
}
