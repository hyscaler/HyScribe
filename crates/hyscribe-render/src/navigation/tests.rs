// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::Term;
use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::theme::twentytwentyfive;

/// Every case of `reference/navigation/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/navigation.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let (markup, counter, queried, php) =
            (unhex(fields[0]), fields[1], fields[2], unhex(fields[3]));
        let mut request = Request::default();
        if let Some((kind, id)) = queried.split_once(':') {
            request.queried_object_id = id.parse().unwrap();
            if kind == "term" {
                request.archive = Some(Archive::Category(Term {
                    id: request.queried_object_id,
                    name: "Uncategorized".into(),
                    slug: "uncategorized".into(),
                }));
            }
        }
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = counter.parse().unwrap();
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
        failures[..failures.len().min(4)].join("\n")
    );
    assert!(golden.lines().count() > 50);
}
