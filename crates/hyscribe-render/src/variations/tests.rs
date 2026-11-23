// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Key, Value, json_decode, json_encode};
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive_with;

/// Every case of `reference/theme-json/variations-render.php`, with the
/// synthetic variations of `variations.json` as user global styles.
#[test]
fn matches_php() {
    let synthetic = json_decode(include_bytes!(
        "../../../../reference/theme-json/variations.json"
    ))
    .unwrap();
    let text = |value: &Value| String::from_utf8(value.to_php_string()).unwrap();
    let styles: Vec<(String, String)> = synthetic
        .get_str("styles")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|(_, pair)| {
            let item = |i| text(pair.get(&Key::Int(i)).unwrap());
            (item(0), item(1))
        })
        .collect();
    let mut user = synthetic.get_str("theme").unwrap().clone();
    if let Value::Array(entries) = &mut user {
        entries.push((
            Key::String(b"isGlobalStylesUserThemeJSON".to_vec()),
            Value::Bool(true),
        ));
    }
    let user = json_encode(&user).unwrap();
    let theme = twentytwentyfive_with(&user, &styles);
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request::default();
    let golden = include_str!("../../tests/golden/style-variations.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let markup = unhex(fields[0]);
        let (html, css) = (unhex(fields[2]), unhex(fields[3]));
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = fields[1].parse().unwrap();
        let ours = renderer.render(&markup);
        let ours_css = renderer.assets.variations.join("\n");
        if ours != html || ours_css.as_bytes() != css {
            failures.push(format!(
                "{}\n  php:  {}\n        {}\n  ours: {}\n        {ours_css}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&html),
                String::from_utf8_lossy(&css),
                String::from_utf8_lossy(&ours),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(golden.lines().count() == 13);
}
