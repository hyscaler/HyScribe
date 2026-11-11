// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use hyscribe_phpser::encode;
use hyscribe_testkit::unhex;

/// The PHP oracle's cases (`reference/supports/registry-golden.sh`).
#[test]
fn every_golden_case() {
    let mut failures = Vec::new();
    for line in include_str!("../../tests/golden/registry.tsv").lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "v" {
            let schema = json_decode(&unhex(fields[1])).unwrap();
            let value = json_decode(&unhex(fields[2])).unwrap();
            if is_valid(&value, &schema) != (fields[3] == "1") {
                failures.push(format!(
                    "{} with {}: php {}",
                    String::from_utf8_lossy(&unhex(fields[1])),
                    String::from_utf8_lossy(&unhex(fields[2])),
                    fields[3]
                ));
            }
        } else {
            let name = String::from_utf8(unhex(fields[1])).unwrap();
            let Some(Value::Array(attrs)) = json_decode(&unhex(fields[2])) else {
                panic!("attrs");
            };
            let ours = match block_type(&name) {
                Some(block) => encode(&Value::Array(block.prepare(&attrs))),
                None => encode(&Value::Array(attrs)),
            };
            if ours != unhex(fields[3]) {
                failures.push(format!(
                    "prepare {name} {}:\n  php:  {}\n  ours: {}",
                    String::from_utf8_lossy(&unhex(fields[2])),
                    String::from_utf8_lossy(&unhex(fields[3])),
                    String::from_utf8_lossy(&ours)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn malformed_registry_data_is_skipped() {
    assert!(load(b"not json").is_empty());
    let types = load(br#"{"7":{},"core/x":{"supports":{"align":true}},"core/y":{"attributes":3}}"#);
    assert_eq!(types.len(), 2);
    assert!(types[&b"core/x".to_vec()].attributes.is_empty());
    assert_eq!(types[&b"core/y".to_vec()].supports, Value::Null);
    assert!(block_type("core/paragraph").is_some());
    // `core/missing` is a real block: the stand-in for unsupported ones.
    assert!(block_type("core/missing").is_some());
    assert!(block_type("core/nope").is_none());
}

#[test]
fn a_null_default_counts_as_none() {
    let Some(Value::Array(attributes)) = json_decode(
        br#"{"a":{"type":"string","default":null},"b":{"type":"string","default":"x"}}"#,
    ) else {
        panic!("json");
    };
    let block = BlockType {
        attributes,
        supports: Value::Null,
        view_script_modules: Vec::new(),
    };
    assert_eq!(
        block.prepare(&[]),
        [(Key::String(b"b".to_vec()), Value::String(b"x".to_vec()))]
    );
}

#[test]
fn floats_compare_by_value_not_text() {
    let float = |text: &str| Value::Float(Float::parse(text.as_bytes()).unwrap());
    // Editors may save `2.0` where PHP would print `2`.
    assert!(equal(&float("2.0"), &float("2")));
    assert!(!equal(&float("2.5"), &float("2")));
}
