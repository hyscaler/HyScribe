// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use crate::blocks::parse_blocks;
use crate::core_blocks::render;
use crate::fixture::DefaultInstall;
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install with the terms a case gives `get_terms()`.
struct Tagged {
    site: DefaultInstall,
    terms: Vec<(Term, u64)>,
}

impl Site for Tagged {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.site.option(name)
    }

    fn home_url(&self) -> Vec<u8> {
        self.site.home_url()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.site.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.site.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.site.post_meta(post_id, key)
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.site.query(query)
    }

    fn cloud_terms(&self, _: &str, limit: u64) -> Vec<(Term, u64)> {
        let limit = usize::try_from(limit).unwrap();
        self.terms.iter().take(limit).cloned().collect()
    }

    /// The same terms for a category cloud: `terms_pre_query` skips the
    /// tree's handling, and the sets are already most used first.
    fn taxonomy_terms(&self, _: &str) -> Vec<(Term, u64)> {
        self.terms.clone()
    }
}

/// Every case of `reference/tag-cloud/golden.sh`, through the render
/// callback, as the oracle leaves out the elements support.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let golden = include_str!("../../../tests/golden/tag-cloud.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (markup, terms, php) = (
            unhex(fields[0]),
            json_decode(&unhex(fields[1])).unwrap(),
            unhex(fields[2]),
        );
        let terms = terms
            .as_array()
            .unwrap()
            .iter()
            .map(|(_, t)| {
                let field = |i: i64| t.get(&hyscribe_phpser::Key::Int(i)).unwrap();
                let term = Term {
                    id: field(0).to_int() as u64,
                    name: field(1).to_php_string(),
                    slug: field(2).to_php_string(),
                    ..Term::default()
                };
                (term, field(3).to_int() as u64)
            })
            .collect();
        let site = Tagged {
            site: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            terms,
        };
        let context = Renderer::new(&theme, &site, &request).context;
        let block = &parse_blocks(&markup)[0];
        let attrs = block.attrs.as_deref().unwrap_or_default();
        let ours = render(&context, "core/tag-cloud", attrs, b"").unwrap();
        if ours != php {
            failures.push(format!(
                "{} {:?}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&unhex(fields[1])),
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
    assert_eq!(golden.lines().count(), 96);
}
