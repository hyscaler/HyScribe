// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::{check, resolved};

#[test]
fn matches_wp_get_typography_font_size_value() {
    // PHP falls back to the default install's global settings.
    check("font_size", |a| {
        font_size_value(&a[0], &a[1], resolved().settings()).unwrap_or(Value::Null)
    });
}

#[test]
fn values_and_units() {
    assert_eq!(value_and_unit(&Value::Bool(true), ""), None);
    assert_eq!(value_and_unit(&string("0"), ""), None);
    assert_eq!(
        value_and_unit(&string("2em"), "px"),
        Some((32.0, "px".to_owned()))
    );
    assert_eq!(
        value_and_unit(&string("16px"), "rem"),
        Some((1.0, "rem".to_owned()))
    );
    assert_eq!(
        value_and_unit(&string("2em"), "rem"),
        Some((2.0, "rem".to_owned()))
    );
    assert_eq!(value_and_unit(&string("2%"), ""), None);
}
