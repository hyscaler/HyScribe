// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Value, encode};
use hyscribe_testkit::unhex;

use super::*;
use crate::formatting::CORE_SHORTCODE_TAGS;

const GOLDEN: &str = include_str!("../../tests/golden/shortcodes.tsv");

/// The rows of one kind of `reference/shortcodes/golden.sh`: the input and
/// PHP's answer.
fn rows(kind: &str) -> Vec<(Vec<u8>, Vec<u8>)> {
    GOLDEN
        .lines()
        .map(|line| line.split('\t').collect::<Vec<&str>>())
        .filter(|fields| fields[0] == kind)
        .map(|fields| (unhex(fields[1]), unhex(fields[2])))
        .collect()
}

/// Every answer of `function` over the rows of `kind` is PHP's.
fn matches_php(kind: &str, function: impl Fn(&[u8]) -> Vec<u8>) -> usize {
    let rows = rows(kind);
    let failures: Vec<String> = rows
        .iter()
        .filter(|(input, php)| function(input) != *php)
        .map(|(input, php)| {
            format!(
                "{:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(input),
                String::from_utf8_lossy(php),
                String::from_utf8_lossy(&function(input))
            )
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    rows.len()
}

#[test]
fn parsed_attributes_match_php() {
    let cases = matches_php("atts", |text| {
        encode(&Value::Array(shortcode_parse_atts(text)))
    });
    assert_eq!(cases, 39);
}

#[test]
fn stripped_shortcodes_match_php() {
    let cases = matches_php("strip", |text| strip_shortcodes(text, &CORE_SHORTCODE_TAGS));
    assert_eq!(cases, 18);
}

#[test]
fn unwrapped_shortcodes_match_php() {
    let cases = matches_php("unautop", |text| {
        shortcode_unautop(text, &CORE_SHORTCODE_TAGS)
    });
    assert_eq!(cases, 27);
}

#[test]
fn tags_are_those_after_a_bracket_in_registration_order() {
    let registered: [&[u8]; 3] = [b"b", b"a", b"c"];
    assert!(tags_in(&registered, b"no brackets a b").is_empty());
    assert_eq!(
        tags_in(&registered, b"[a] [b x] [c] [d]"),
        [b"b", b"a", b"c"]
    );
    // Names stop at the characters a tag cannot hold, and are whole.
    assert_eq!(
        tags_in(&registered, b"[a=1] [b/] [c<] [a&] [[b]"),
        [b"b", b"a", b"c"]
    );
    assert!(tags_in(&registered, b"[ a] [] [\x01a] [ax] [bb]").is_empty());
}

#[test]
fn a_shortcode_is_found_with_its_parts() {
    let tags: [&[u8]; 2] = [b"one", b"two"];
    let text = b"a [[one x]] b [two y=1]body[/two]] c [one/] [one] [two";
    let mut seen = Vec::new();
    let out = replace_shortcodes(text, &tags, &mut |m| {
        seen.push((
            m.start,
            m.end,
            m.open,
            m.tag.to_vec(),
            m.atts.to_vec(),
            m.content.map(<[u8]>::to_vec),
            m.close,
        ));
        format!("<{}>", String::from_utf8_lossy(m.tag)).into_bytes()
    });
    assert_eq!(
        String::from_utf8(out).unwrap(),
        "a <one> b <two> c <one> <one> [two"
    );
    assert_eq!(
        seen[0],
        (2, 11, true, b"one".to_vec(), b" x".to_vec(), None, true)
    );
    assert_eq!(
        seen[1],
        (
            14,
            34,
            false,
            b"two".to_vec(),
            b" y=1".to_vec(),
            Some(b"body".to_vec()),
            true
        )
    );
}

#[test]
fn brackets_in_elements_are_hidden_until_unescaped() {
    let text = b"[a] <b title=\"[a]\">[a]</b> &#91;a&#93; <!-- [a] --> <i";
    let hidden = escape_brackets_in_elements(text);
    assert_eq!(
        String::from_utf8(hidden.clone()).unwrap(),
        "[a] <b title=\"&#91;a&#93;\">[a]</b> &#091;a&#093; <!-- &#91;a&#93; --> <i"
    );
    assert_eq!(
        String::from_utf8(unescape_invalid_shortcodes(&hidden)).unwrap(),
        "[a] <b title=\"[a]\">[a]</b> &#091;a&#093; <!-- [a] --> <i"
    );
}

#[test]
fn no_tags_leave_the_text_as_it_is() {
    assert_eq!(
        replace_shortcodes(b"[a]", &[], &mut |_| unreachable!()),
        b"[a]"
    );
    assert_eq!(shortcode_unautop(b"<p>[a]</p>", &[]), b"<p>[a]</p>");
}

#[test]
fn defaults_fill_what_the_attributes_do_not_name() {
    let atts = shortcode_parse_atts(b"b=2 c=\"x\" 5=6");
    let pairs = [
        ("a", Value::Int(1)),
        ("b", Value::Int(0)),
        ("5", Value::Null),
    ];
    let out = shortcode_atts(&pairs, &atts);
    assert_eq!(out[0], ("a".to_string(), Value::Int(1)));
    assert_eq!(out[1], ("b".to_string(), Value::String(b"2".to_vec())));
    assert_eq!(out[2].1, Value::String(b"6".to_vec()));
}
