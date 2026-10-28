// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::schema::tests::origin;
use crate::tests::{arg, blocks, check};

#[test]
fn matches_wp_theme_json() {
    check("new", |a| new(a[0].clone(), origin(arg(a, 1)), blocks()));
    check("merge", |a| {
        let mut data = new(a[0].clone(), origin(arg(a, 1)), blocks());
        merge(&mut data, &new(a[2].clone(), origin(arg(a, 3)), blocks()));
        data
    });
}

#[test]
fn origins() {
    let names: Vec<&str> = [
        Origin::Default,
        Origin::Blocks,
        Origin::Theme,
        Origin::Custom,
    ]
    .iter()
    .map(|o| o.name())
    .collect();
    assert_eq!(names, ORIGINS);
}
