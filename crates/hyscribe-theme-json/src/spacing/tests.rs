// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::check;

fn list(items: Vec<Value>) -> Value {
    Value::Array(
        items
            .into_iter()
            .enumerate()
            .map(|(i, v)| (Key::Int(i as i64), v))
            .collect(),
    )
}

#[test]
fn matches_wp_theme_json() {
    check("spacing", |a| list(compute_sizes(&a[0])));
    check("merge_spacing", |a| {
        merge_sizes(
            php::entries(&a[0]).iter().map(|(_, v)| v.clone()).collect(),
            &a[1],
        )
    });
}

#[test]
fn steps_are_bounded() {
    let scale = hyscribe_phpser::json_decode(
        br#"{"operator": "+", "increment": 1, "steps": 1e300, "mediumStep": 1, "unit": "px"}"#,
    )
    .unwrap();
    assert_eq!(compute_sizes(&scale).len(), MAX_STEPS + 1);
}
