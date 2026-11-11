// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::Term;
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::{Archive, Request, Taxonomy};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/term-blocks/golden.sh`, in order: on the category
/// archive the term carries the given name and count; elsewhere there is
/// no term.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../../tests/golden/term-blocks.tsv");
    let mut failures = Vec::new();
    // The `wp-elements-` classes PHP has given so far.
    let mut elements = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let term = Term {
            id: 1,
            name: unhex(fields[1]),
            slug: b"uncategorized".to_vec(),
            count: fields[2].parse().unwrap(),
            ..Term::default()
        };
        let request = Request {
            archive: (unhex(fields[0]) == b"cat=1")
                .then_some(Archive::Term(Taxonomy::Category, Some(term))),
            ..Request::default()
        };
        let markup = unhex(fields[3]);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.elements_id = elements;
        let ours = renderer.render(&markup);
        let php = unhex(fields[4]);
        elements += u64::from(crate::formatting::contains(&php, b"wp-elements-"));
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&unhex(fields[0])),
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
    assert_eq!(golden.lines().count(), 117);
}
