// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fs;

use hyscribe_phpser::{Key, json_decode, json_encode};
use hyscribe_testkit::unhex;

use super::*;
use crate::php::php_string;
use crate::tests::{fixture, golden, root};

/// The default install with the synthetic variations of
/// `reference/theme-json/variations.json`.
fn styles() -> GlobalStyles {
    let synthetic =
        json_decode(&fs::read(root().join("reference/theme-json/variations.json")).unwrap())
            .unwrap();
    let mut fixture = fixture();
    for (_, pair) in php::entries(php::get(&synthetic, &["styles"]).unwrap()) {
        let item = |i| php_string(pair.get(&Key::Int(i)).unwrap());
        fixture.styles.push((item(0), item(1)));
    }
    let mut theme = json_decode(&fixture.theme).unwrap();
    php::replace_recursive(&mut theme, php::get(&synthetic, &["theme"]).unwrap());
    fixture.theme = json_encode(&theme).unwrap();
    GlobalStyles::resolve(&fixture.sources())
}

#[test]
fn style_variations_match_php() {
    let styles = styles();
    let golden = String::from_utf8(golden("style-variations.tsv")).unwrap();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        let input = json_decode(&unhex(input)).unwrap();
        let expected = json_decode(&unhex(expected)).unwrap();
        let block = php_string(php::get(&input, &["block"]).unwrap());
        let class = php::get(&input, &["className"]).and_then(|c| c.as_bytes());
        let counter = php::number(php::get(&input, &["counter"]).unwrap()).unwrap();
        let mut class_name = class.map(|c| String::from_utf8_lossy(c).into_owned());
        let mut css = String::new();
        if let Some(variation) = class.and_then(|c| styles.style_variation(&block, c)) {
            let instance = format!("{variation}--{}", counter + 1.0);
            css = styles.style_variation_css(&block, &variation, &instance);
            if !css.is_empty() {
                class_name = class_name.map(|c| format!("{c} is-style-{instance}"));
            }
        }
        let want_class = php::get(&expected, &["className"])
            .and_then(|c| c.as_bytes())
            .map(|c| String::from_utf8_lossy(c).into_owned());
        let want_css = php_string(php::get(&expected, &["css"]).unwrap());
        let want_instance = php::get(&expected, &["instance"]).and_then(|c| c.as_bytes());
        let instance = class_name
            .as_deref()
            .and_then(|c| instance_class(c.as_bytes()));
        if class_name != want_class || css != want_css || instance != want_instance {
            failures.push(format!(
                "{block} {class_name:?}\n  want {want_class:?} {want_css}\n  got  {css}"
            ));
        }
    }
    assert!(golden.lines().count() == 81, "case count");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
