// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::{arg, check};

/// The origin PHP's constructor would use for an origin name.
pub(crate) fn origin(name: &str) -> Origin {
    match name {
        "default" => Origin::Default,
        "blocks" => Origin::Blocks,
        "custom" => Origin::Custom,
        _ => Origin::Theme,
    }
}

#[test]
fn matches_wp_theme_json_schema() {
    check("migrate", |a| migrate(a[0].clone(), origin(arg(a, 1))));
}
