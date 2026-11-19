// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Value;

use crate::php;
use crate::tests::{check, check_except, resolved};

/// `remove_insecure_properties( $theme_json, 'custom' )` against PHP for
/// the `insecure` cases of `reference/theme-json/cases.txt`.
#[test]
fn insecure_values_go_as_in_php() {
    // A font family with braces is the one value PHP keeps and HyScribe
    // drops; everything else of its answer stands.
    check_except(
        "insecure",
        |args| resolved().remove_insecure_properties(args[0].clone()),
        |args, php_answer| {
            let braces = php::get(&args[0], &["styles", "typography", "fontFamily"])
                .and_then(Value::as_bytes)
                .is_some_and(|family| family.contains(&b'}'));
            if !braces {
                return php_answer;
            }
            let mut answer = hyscribe_phpser::decode(&php_answer).unwrap();
            php::unset(&mut answer, &["styles", "typography", "fontFamily"]);
            hyscribe_phpser::encode(&answer)
        },
    );
}

/// A value with a brace is dropped wherever it stands, as an unsafe one:
/// `red;} body{x:y}` ends no declaration for what follows.
#[test]
fn values_with_braces_are_dropped() {
    let json = |value: &str| {
        let text = format!(
            r#"{{"version": 3, "styles": {{"color": {{"text": "{value}", "background": "blue"}}, "elements": {{"link": {{"color": {{"text": "{value}"}}}}}}, "blocks": {{"core/button": {{"color": {{"text": "{value}"}}}}}}}}}}"#
        );
        hyscribe_phpser::json_decode(text.as_bytes()).unwrap()
    };
    for value in ["red;} body{x:y}", "red;}", "{x}"] {
        let answer = resolved().remove_insecure_properties(json(value));
        assert_eq!(
            php::get(&answer, &["styles", "color", "background"]),
            Some(&php::string("blue"))
        );
        assert_eq!(php::get(&answer, &["styles", "color", "text"]), None);
        assert_eq!(php::get(&answer, &["styles", "elements"]), None);
        assert_eq!(php::get(&answer, &["styles", "blocks"]), None);
    }
    let kept = resolved().remove_insecure_properties(json("red"));
    assert!(php::get(&kept, &["styles", "elements", "link", "color", "text"]).is_some());
}

/// `wp_filter_global_styles_post()` against PHP, whose result is slashed,
/// for the `filter_styles` cases.
#[test]
fn global_styles_posts_are_filtered_as_in_php() {
    check("filter_styles", |args| {
        let content = args[0].as_bytes().unwrap();
        let Some(filtered) = resolved().filter_global_styles_post(content) else {
            return args[0].clone();
        };
        let mut slashed = Vec::new();
        for byte in filtered {
            if matches!(byte, b'\'' | b'"' | b'\\') {
                slashed.push(b'\\');
            }
            slashed.push(byte);
        }
        Value::String(slashed)
    });
}
