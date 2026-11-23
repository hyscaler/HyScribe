// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use super::*;
use crate::registry::{block_type, load};
use crate::theme::twentytwentyfive;

/// Every case of `reference/layout/golden.sh`.
#[test]
fn matches_php() {
    let global = twentytwentyfive().global;
    let types = load(include_bytes!("../../tests/golden/layout-types.json"));
    let golden = include_str!("../../tests/golden/layout.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("style\t") {
            let (arguments, php) = rest.split_once('\t').unwrap();
            let arguments = json_decode(&unhex(arguments)).unwrap();
            let argument = |i| arguments.get(&Key::Int(i)).unwrap().clone();
            let layout = Value::Array(vec![(Key::String(b"type".to_vec()), argument(0))]);
            let (gap_value, fallback) = (argument(1), argument(2));
            let flag = |i| argument(i) == Value::Bool(true);
            let gap = Gap {
                supported: flag(3),
                value: &gap_value,
                skip: flag(4),
                fallback: &fallback,
                spacing: None,
            };
            assert_eq!(
                stylesheet(&layout_style(".c", &layout, &gap)).as_bytes(),
                unhex(php),
                "{arguments:?}"
            );
            continue;
        }
        let mut fields = line.split('\t').map(unhex);
        let mut field = || fields.next().unwrap();
        let (name, attrs, parent, first, content, php, php_css) = (
            field(),
            field(),
            field(),
            field(),
            field(),
            field(),
            field(),
        );
        let name = String::from_utf8(name).unwrap();
        let attributes = json_decode(&attrs).unwrap();
        let parent = json_decode(&parent).filter(|p| *p != Value::Null);
        let block = LayoutBlock {
            name: &name,
            block_type: types
                .get(name.as_bytes())
                .or_else(|| block_type(&name))
                .unwrap(),
            attributes: &attributes,
            parent_layout: parent.as_ref(),
            first_chunk: (!first.is_empty()).then_some(first.as_slice()),
        };
        let (html, rules) = render_layout(&block, &global, content.clone());
        let css = stylesheet(&rules);
        if html != php || css.as_bytes() != php_css {
            failures.push(format!(
                "{name} {}\n  parent {:?}\n  php:  {}\n  ours: {}\n  php css:  {}\n  ours css: {css}",
                String::from_utf8_lossy(&attrs),
                parent,
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&html),
                String::from_utf8_lossy(&php_css)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(5)].join("\n")
    );
    assert!(golden.lines().count() > 1000);
}

#[test]
fn rules_without_safe_declarations_print_nothing() {
    let unsafe_rule = rule(".x".into(), &[("color", string("expression(alert(1))"))]);
    assert_eq!(stylesheet(&[unsafe_rule]), "");
}
