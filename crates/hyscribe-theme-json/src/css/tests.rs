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
