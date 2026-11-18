// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::layout::stylesheet;
use crate::site::Request;
use crate::theme::{twentytwentyfive, twentytwentyfive_with};

/// Every case of `reference/duotone/golden.sh`, in the order PHP ran them,
/// the ID counter running on.
#[test]
fn matches_php() {
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request::default();
    let mut theme = twentytwentyfive();
    let (mut counter, mut cases) = (0, 0);
    let mut failures = Vec::new();
    for line in include_str!("../../tests/golden/duotone.tsv").lines() {
        if let Some(user) = line.strip_prefix("user\t") {
            theme = twentytwentyfive_with(&unhex(user), &[]);
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let markup = unhex(fields[1]);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = counter;
        let html = renderer.render(&markup);
        let rules = renderer.duotone.rules();
        renderer.assets.rules.extend(rules);
        let ours = [
            html,
            stylesheet(&renderer.assets.rules).into_bytes(),
            renderer.duotone.footer(),
            renderer.duotone.global_styles().into_bytes(),
        ];
        let php = [2, 3, 4, 5].map(|at| unhex(fields[at]));
        counter = renderer.unique_id;
        cases += 1;
        if ours != php {
            let names = ["html", "css", "footer", "presets"];
            let parts: Vec<String> = (0..4)
                .filter(|&at| ours[at] != php[at])
                .map(|at| {
                    format!(
                        "{}\n    php:  {}\n    ours: {}",
                        names[at],
                        String::from_utf8_lossy(&php[at]),
                        String::from_utf8_lossy(&ours[at])
                    )
                })
                .collect();
            failures.push(format!(
                "phase {} {}\n  {}",
                fields[0],
                String::from_utf8_lossy(&markup),
                parts.join("\n  ")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(10)].join("\n")
    );
    assert_eq!(cases, 86);
}
