// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::encode;

use super::*;
use crate::tests::{arg, blocks, check, golden};

/// One entry of `get_blocks_metadata()`, as PHP builds it.
fn to_value(meta: &BlockMeta) -> Value {
    let pairs = |list: &[(String, String)]| {
        Value::Array(list.iter().map(|(k, v)| (php::key(k), string(v))).collect())
    };
    let mut out = vec![
        (php::key("selector"), string(&meta.selector)),
        (php::key("selectors"), meta.selectors.clone()),
    ];
    let elements: Vec<(String, String)> = meta
        .elements
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect();
    out.push((php::key("elements"), pairs(&elements)));
    if let Some(duotone) = &meta.duotone {
        out.push((php::key("duotone"), string(duotone)));
    }
    if !meta.style_variations.is_empty() {
        out.push((php::key("styleVariations"), pairs(&meta.style_variations)));
    }
    if let Some(states) = &meta.states {
        out.push((php::key("states"), states.clone()));
    }
    Value::Array(out)
}

#[test]
fn default_install_matches_get_blocks_metadata() {
    let all = Value::Array(
        blocks()
            .iter()
            .map(|m| (php::key(&m.name), to_value(m)))
            .collect(),
    );
    assert!(encode(&all) == golden("metadata.ser"));
}

#[test]
fn matches_registered_blocks() {
    check("block_meta", |a| {
        let block = BlockType {
            name: arg(a, 0).to_owned(),
            metadata: a[1].clone(),
        };
        to_value(Blocks::new(&[block], &[]).iter().next().unwrap())
    });
}

#[test]
fn styles_of_unknown_blocks_are_ignored() {
    let mut blocks = Blocks::new(&[], &[("core/x".to_owned(), "y".to_owned())]);
    blocks.register_styles(&[("core/x".to_owned(), "z".to_owned())]);
    assert_eq!(blocks, Blocks::default());
    assert_eq!(element_selector("nope"), None);
}

#[test]
fn styles_register_once() {
    let block = BlockType {
        name: "test/a".to_owned(),
        metadata: hyscribe_phpser::json_decode(br#"{"styles": [{"name": "x"}]}"#).unwrap(),
    };
    let styles = [
        ("test/a".to_owned(), "x".to_owned()),
        ("test/a".to_owned(), "y".to_owned()),
    ];
    let blocks = Blocks::new(&[block], &styles);
    let names: Vec<&str> = blocks
        .get("test/a")
        .unwrap()
        .style_variations
        .iter()
        .map(|(n, _)| n.as_str())
        .collect();
    assert_eq!(names, ["x", "y"]);
}
