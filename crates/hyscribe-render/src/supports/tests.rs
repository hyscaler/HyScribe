// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::OnceLock;

use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;
use hyscribe_theme_json::Sources;

use super::*;
use crate::registry::load;

/// The default install's global settings: core and Twenty Twenty-Five
/// (the supports read none of the block level settings).
fn global() -> &'static GlobalStyles {
    static GLOBAL: OnceLock<GlobalStyles> = OnceLock::new();
    GLOBAL.get_or_init(|| {
        GlobalStyles::resolve(&Sources {
            core: include_bytes!("../../../../wordpress/wp-includes/theme.json"),
            blocks: &[],
            block_styles: &[],
            theme: Some(include_bytes!(
                "../../../../themes/twentytwentyfive/theme.json"
            )),
            theme_styles: &[],
            parent_theme: None,
            user: None,
        })
    })
}

fn entries(json: &[u8]) -> Vec<(Key, Value)> {
    match json_decode(json) {
        Some(Value::Array(entries)) => entries,
        other => panic!("not an object: {other:?}"),
    }
}

/// The PHP oracle's cases (`reference/supports/golden.sh`), on core blocks
/// and on the synthetic types it registers.
#[test]
fn every_golden_case() {
    let types = load(include_bytes!("../../tests/golden/wrapper-types.json"));
    let golden = include_str!("../../tests/golden/wrapper-attributes.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let name = String::from_utf8(unhex(fields[0])).unwrap();
        let attributes = entries(&unhex(fields[1]));
        let extra: Vec<(String, Value)> = entries(&unhex(fields[3]))
            .into_iter()
            .map(|(key, value)| match key {
                Key::String(key) => (String::from_utf8(key).unwrap(), value),
                Key::Int(n) => (n.to_string(), value),
            })
            .collect();
        let extra: Vec<(&str, Value)> =
            extra.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let block_type = types.get(name.as_bytes()).or_else(|| block_type(&name));
        let ours = wrapper_attributes(&name, block_type, &attributes, &extra, global());
        let php = String::from_utf8(unhex(fields[2])).unwrap();
        if ours != php {
            failures.push(format!(
                "{name} {} {}:\n  php:  {php}\n  ours: {ours}",
                String::from_utf8_lossy(&unhex(fields[1])),
                String::from_utf8_lossy(&unhex(fields[3])),
            ));
        }
    }
    assert!(golden.lines().count() > 1700);
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
fn registered_blocks_are_looked_up() {
    let attributes = entries(br#"{"align":"wide","className":"x"}"#);
    let extra = [("data-x", string("1"))];
    assert_eq!(
        get_block_wrapper_attributes("core/group", &attributes, &extra, global()),
        r#"class="alignwide x wp-block-group" data-x="1""#
    );
    assert_eq!(
        get_block_wrapper_attributes("core/nope", &attributes, &extra, global()),
        r#"data-x="1""#
    );
}
