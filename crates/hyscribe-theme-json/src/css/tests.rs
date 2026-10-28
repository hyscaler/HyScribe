// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::{arg, check, text};

#[test]
fn matches_wp_theme_json() {
    check("is_safe", |a| Value::Bool(is_safe(arg(a, 0), arg(a, 1))));
    check("nest_custom", |a| text(&nest_custom(arg(a, 0), arg(a, 1))));
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

#[test]
fn function_calls_are_stripped() {
    assert_eq!(strip_functions("calc(1px) a var(b(c)) d"), " a  d");
    assert_eq!(
        strip_functions("xcalc(1px) calc(2px"),
        "xcalc(1px) calc(2px"
    );
    assert_eq!(balanced(b"ab(c(d))e", 2), Some(8));
    assert_eq!(balanced(b"a(b", 1), None);
}

#[test]
fn urls_are_refused() {
    // PHP would check the URL's protocol; without that filter, refuse.
    assert!(!is_safe("background", "url(a.png)"));
    assert!(!is_safe("--x", "url(a.png)"));
    assert!(is_safe("--x", "var(--y)"));
    assert!(!is_safe("--x", "x url(a.png)"));
}
