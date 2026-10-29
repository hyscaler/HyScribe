// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::layout::stylesheet;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/supports/elements-golden.php`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request::default();
    let golden = include_str!("../../tests/golden/elements.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let markup = unhex(fields[0]);
        let (html, css) = (unhex(fields[2]), unhex(fields[3]));
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.elements_id = fields[1].parse().unwrap();
        let ours = renderer.render(&markup);
        let ours_css = stylesheet(&renderer.assets.rules);
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
    assert_eq!(golden.lines().count(), 19);
}
