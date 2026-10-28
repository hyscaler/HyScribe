// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Key, Value};

use super::*;
use crate::tests::{arg, check, text};

#[test]
fn matches_wp_theme_json() {
    check("append", |a| text(&append(arg(a, 0), arg(a, 1))));
    check("prepend", |a| text(&prepend(arg(a, 0), arg(a, 1))));
    check("split", |a| {
        Value::Array(
            split(arg(a, 0))
                .iter()
                .enumerate()
                .map(|(i, s)| (Key::Int(i as i64), text(s)))
                .collect(),
        )
    });
    check("scope", |a| text(&scope(arg(a, 0), arg(a, 1))));
    check("variation", |a| text(&variation(arg(a, 0), arg(a, 1))));
}
