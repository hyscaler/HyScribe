// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/interactivity/golden.sh`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/interactivity.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let (markup, php) = (unhex(markup), unhex(php));
        let ours = process_directives(&markup);
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(6)].join("\n")
    );
    assert!(golden.lines().count() > 100);
}

/// Markup using a directive that is not ported is left as rendered.
#[test]
fn unported_directives_leave_the_markup_alone() {
    for markup in [
        &br#"<div data-wp-interactive="t"><b data-wp-bind--title="!state.x" data-wp-text="state.y">x</b></div>"#[..],
        br#"<div data-wp-interactive="t" data-wp-router-region="r"><b data-wp-bind--title="!state.x">x</b></div>"#,
        br#"<div data-wp-interactive="t"><template data-wp-each="state.list"><b></b></template></div>"#,
    ] {
        assert_eq!(process_directives(markup), markup);
    }
}
