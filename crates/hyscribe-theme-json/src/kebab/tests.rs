// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::php::php_string;
use crate::tests::{check, text};

#[test]
fn matches_wp_to_kebab_case() {
    check("kebab", |a| text(&kebab_case(&php_string(&a[0]))));
}

#[test]
fn character_classes() {
    assert!(word('é') && word('_') && word('7') && !word('-'));
    assert!(breaks('\u{feff}') && breaks('\u{1680}') && breaks('\u{180e}') && breaks('\u{3000}'));
    assert!(misc('日') && !misc('a') && !misc('A') && !misc('1') && !misc(' '));
    assert_eq!(ordinal_suffix('9', true), "TH");
}
