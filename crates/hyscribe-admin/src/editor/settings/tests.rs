// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the editor's goldens do not reach: their site has no unfiltered
//! HTML, the bundled theme's duotone presets are all six-digit hex colors,
//! and its posts keep their default template.

use hyscribe_phpser::{Key, Value, json_decode};

use super::*;

fn keys(value: &Value) -> Vec<String> {
    let entries = value.as_array().unwrap_or_default();
    entries
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(&k.to_php_string()).into_owned())
        .collect()
}

/// `get_allowed_mime_types()` keeps `htm|html` after `css` and `js` after
/// `rtf`, where `wp_get_mime_types()` lists them.
#[test]
fn unfiltered_html_allows_html_and_scripts() {
    let filtered = keys(&allowed_mime_types(false));
    let all = keys(&allowed_mime_types(true));
    assert_eq!(all.len(), filtered.len() + 2);
    let after = |key: &str| {
        all.get(all.iter().position(|k| k == key).unwrap() + 1)
            .cloned()
    };
    assert_eq!(after("css").as_deref(), Some("htm|html"));
    assert_eq!(after("rtf").as_deref(), Some("js"));
    assert!(!filtered.contains(&"js".to_string()));
}

/// `colord_parse_hex()` and PHP's printing of the shares, as `php -r`
/// prints `136/255` (0.53333333333333) and `round( 136/255, 2 )`.
#[test]
fn hex_colors_of_every_length() {
    let strings = |c: [&str; 4]| c.map(String::from);
    assert_eq!(channels(b"#fff"), Some(strings(["1", "1", "1", "1"])));
    assert_eq!(channels(b"#0008"), Some(strings(["0", "0", "0", "0.53"])));
    assert_eq!(
        channels(b"#88000088"),
        Some(strings(["0.53333333333333", "0", "0", "0.53"]))
    );
    for invalid in [&b"#12345"[..], b"red", b"#ggg", b"#1234567"] {
        assert_eq!(channels(invalid), None);
    }
}

/// A later origin's preset of a slug replaces an earlier one's in place,
/// and colors that are not hex are left out of the filter.
#[test]
fn duotone_presets_by_filter() {
    let settings = json_decode(
        br##"{"color":{"duotone":{"default":[{"slug":"a","colors":["#000","#fff"]},{"slug":"b","colors":["#000","#fff"]}],"theme":[{"slug":"a","colors":["#fff","hsl(0,0%,0%)"]}]}}}"##,
    )
    .unwrap();
    let presets = duotone_presets(&settings);
    let ids: Vec<&str> = presets.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["wp-duotone-a", "wp-duotone-b"]);
    assert_eq!(presets[0].1.len(), 2);
    let styles = duotone_styles(&settings);
    let svgs = styles[0].get_str("assets").unwrap().to_php_string();
    assert!(String::from_utf8_lossy(&svgs).contains("tableValues=\"1\""));
    assert!(duotone_styles(&Value::Null).is_empty());
}

/// A feature path through what is not an array takes nothing.
#[test]
fn features_missing_are_left() {
    let mut features = json_decode(br#"{"color":{"custom":true},"spacing":1}"#).unwrap();
    assert_eq!(take(&mut features, &["spacing", "units"]), None);
    assert_eq!(take(&mut features, &["spacing"]), Some(Value::Int(1)));
    assert_eq!(take(&mut Value::Null, &["color"]), None);
    assert_eq!(take(&mut features, &[]), None);
    assert_eq!(
        take(&mut features, &["color", "custom"]),
        Some(Value::Bool(true))
    );
    let by = json_decode(br#"{"p":{"default":[1]}}"#).unwrap();
    assert_eq!(
        by_origin(&by, &["p"]),
        Some(Value::Array(vec![(Key::Int(0), Value::Int(1))]))
    );
    assert_eq!(
        by_origin(&json_decode(br#"{"p":{}}"#).unwrap(), &["p"]),
        Some(Value::Null)
    );
}

#[test]
fn templates_by_slug() {
    assert_eq!(template_slug(b"", "page"), "page");
    assert_eq!(template_slug(b"default", "post"), "single");
    assert_eq!(template_slug(b"page-no-title", "page"), "page-no-title");
}
