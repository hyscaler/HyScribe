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
        &br#"<div data-wp-interactive="t" data-wp-router-region="r"><b data-wp-bind--title="!state.x">x</b></div>"#[..],
        br#"<div data-wp-interactive="t"><template data-wp-each="state.list"><b data-wp-bind--title="!state.x"></b></template></div>"#,
    ] {
        assert_eq!(process_directives(markup), markup);
    }
}

/// A text directive whose tag never closes leaves the markup as rendered.
#[test]
fn text_needs_a_closer() {
    let markup = br#"<div data-wp-interactive="t"><b data-wp-text="state.y">x"#;
    assert_eq!(process_directives(markup), markup);
}

/// Each nested context copies its parent, which PHP shares until it is
/// written; once the copies pass [`MAX_COPIED_BYTES`] the markup is left
/// as rendered, where PHP would run out of memory. A large value and a
/// large key both count.
#[test]
fn copying_past_the_budget_leaves_the_markup_alone() {
    let large = "a".repeat(100_000);
    for context in [
        format!(r#"{{"large":"{large}"}}"#),
        format!(r#"{{"{large}":""}}"#),
    ] {
        let markup = [
            format!(r#"<div data-wp-interactive="t" data-wp-context='{context}'>"#),
            r#"<i data-wp-context='{"n":1}'>"#.repeat(1_000),
            r#"<b data-wp-bind--title="context.n">x</b>"#.into(),
            "</i>".repeat(1_000),
            "</div>".into(),
        ]
        .concat();
        assert_eq!(process_directives(markup.as_bytes()), markup.as_bytes());
        // Below the budget the same markup is processed.
        let few = markup
            .replace(r#"<i data-wp-context='{"n":1}'>"#, "")
            .replace("</i>", "");
        let few = few
            .replace(
                r#"<b data-wp-bind"#,
                r#"<i data-wp-context='{"n":1}'><b data-wp-bind"#,
            )
            .replace("</b>", "</b></i>");
        let processed = String::from_utf8(process_directives(few.as_bytes())).unwrap();
        assert!(processed.contains(r#"title="1""#));
    }
}

/// A child context copies its parent's: `"t"`, `"s"` and the string.
#[test]
fn processing_stops_just_past_the_budget() {
    for (length, processed) in [
        (super::MAX_COPIED_BYTES - 2, true),
        (super::MAX_COPIED_BYTES - 1, false),
    ] {
        let markup = [
            r#"<div data-wp-interactive="t" data-wp-context='{"s":""#,
            &"a".repeat(length),
            r#""}'><i data-wp-context='{}'></i><b data-wp-bind--hidden="!context.x">x</b></div>"#,
        ]
        .concat();
        let out = process_directives(markup.as_bytes());
        assert_eq!(out != markup.as_bytes(), processed, "{length}");
    }
}

/// The value a directive ends on is copied, and counts.
#[test]
fn values_directives_copy_count_against_the_budget() {
    let large = "a".repeat(1 << 20);
    let markup = [
        format!(r#"<div data-wp-interactive="t" data-wp-context='{{"s":"{large}"}}'>"#),
        r#"<b data-wp-bind--title="context.s">x</b>"#.repeat(70),
        "</div>".into(),
    ]
    .concat();
    assert_eq!(process_directives(markup.as_bytes()), markup.as_bytes());
}

/// A directive reads its path in place, copying only the value it ends on,
/// so many directives over a large context stay well within the budget.
#[test]
fn directives_read_a_large_context_in_place() {
    let large = "a".repeat(100_000);
    let markup = [
        format!(r#"<div data-wp-interactive="t" data-wp-context='{{"large":"{large}","n":1}}'>"#),
        r#"<b data-wp-bind--title="context.n">x</b>"#.repeat(1_000),
        "</div>".into(),
    ]
    .concat();
    let html = String::from_utf8(process_directives(markup.as_bytes())).unwrap();
    assert_eq!(html.matches(r#"title="1""#).count(), 1_000);
}

/// Contexts with many keys merge in linear time, each key found through an
/// index rather than a scan.
#[test]
fn contexts_with_many_keys_merge_in_linear_time() {
    let keys = |prefix: &str| {
        (0..50_000)
            .map(|n| format!(r#""{prefix}{n}":{n}"#))
            .collect::<Vec<_>>()
            .join(",")
    };
    let markup = format!(
        r#"<div data-wp-interactive="t" data-wp-context='{{{}}}'><i data-wp-context='{{{}}}'><b data-wp-bind--title="context.a7" data-wp-bind--lang="context.b9">x</b></i></div>"#,
        keys("a"),
        keys("b")
    );
    let html = String::from_utf8(process_directives(markup.as_bytes())).unwrap();
    assert!(html.contains(r#"<b lang="9" title="7" "#));
}
