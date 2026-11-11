// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/tabs/golden.sh`, in order through one renderer,
/// as PHP's tab panel counters last the request.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let mut renderer = Renderer::new(&theme, &site, &request);
    let golden = include_str!("../../tests/golden/tabs.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        renderer.unique_id = fields[0].parse().unwrap();
        let (markup, php) = (unhex(fields[1]), unhex(fields[2]));
        let ours = renderer.render(&markup);
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 18);
    assert!(
        renderer
            .assets
            .modules
            .contains(&"@wordpress/block-library/tabs/view".to_string())
    );
}
