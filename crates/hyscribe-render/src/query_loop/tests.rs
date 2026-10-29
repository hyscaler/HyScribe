// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::{Renderer, render_content};
use crate::theme::twentytwentyfive;

/// The `query` cases of `reference/core-blocks/golden.sh`, on the fixture's
/// posts.
#[test]
fn query_loops_match_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("query\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, request, home, php) = (field(), field(), field(), field());
        let counter = String::from_utf8(field()).unwrap();
        let site = DefaultInstall { home };
        let request = String::from_utf8(request).unwrap();
        let request = site.request(&request);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.elements_id = counter.parse().unwrap();
        let ours = renderer.render(&markup);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{}",
            String::from_utf8_lossy(&markup)
        );
        count += 1;
    }
    assert!(count >= 50);
}

/// The default install with one sticky post, which its own data lacks.
struct Sticky(DefaultInstall);

impl crate::site::Site for Sticky {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "sticky_posts" => Some(b"a:1:{i:0;i:1;}".to_vec()),
            _ => self.0.option(name),
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.0.home_url()
    }

    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.0.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.0.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.0.post_meta(post_id, key)
    }

    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.0.query(query)
    }
}

#[test]
fn sticky_posts_are_selected_or_left_out() {
    let theme = twentytwentyfive();
    let site = Sticky(DefaultInstall {
        home: b"http://example.test".to_vec(),
    });
    let request = Request::default();
    let html = |sticky: &str| {
        let markup = format!(
            r#"<!-- wp:query {{"query":{{"perPage":3,"sticky":"{sticky}","inherit":false}}}} --><div><!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template --></div><!-- /wp:query -->"#
        );
        String::from_utf8(render_content(&theme, &site, &request, markup.as_bytes())).unwrap()
    };
    assert!(html("only").contains("Hello world!"));
    assert_eq!(
        html("exclude"),
        r#"<div class="is-layout-flow wp-block-query-is-layout-flow"></div>"#
    );
}

/// The default install with the posts and pages `reference/core-blocks/
/// golden.sh` inserts for its `filter` cases, with their terms.
struct Filtered {
    base: DefaultInstall,
    posts: Vec<(hyscribe_model::Post, Vec<(String, hyscribe_model::Term)>)>,
}

impl crate::site::Site for Filtered {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.base.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        let extra = self.posts.iter().find(|(p, _)| p.id == id);
        extra.map(|(p, _)| p.clone()).or_else(|| self.base.post(id))
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        match self.posts.iter().find(|(post, _)| post.id == post_id) {
            Some((_, terms)) => terms
                .iter()
                .filter(|(t, _)| t == taxonomy)
                .map(|(_, term)| term.clone())
                .collect(),
            None => self.base.post_terms(post_id, taxonomy),
        }
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(post_id, key)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        let posts = self
            .base
            .query(&hyscribe_model::PostQuery {
                limit: u32::MAX,
                offset: 0,
                terms: vec![],
                ..query.clone()
            })
            .into_iter()
            .chain(self.posts.iter().map(|(post, _)| post.clone()));
        let has_term = |post: &hyscribe_model::Post, filter: &hyscribe_model::TermFilter| {
            self.post_terms(post.id, &filter.taxonomy)
                .iter()
                .any(|term| filter.terms.contains(&term.id))
        };
        crate::fixture::select(posts, query, has_term)
    }
}

/// The `filter` cases of `reference/core-blocks/golden.sh`: taxonomy,
/// author, search and parent filters, and orders.
#[test]
fn query_filters_match_php() {
    use hyscribe_phpser::{Value, json_decode};
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("filter\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let (markup, state, php) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        let state = json_decode(&state).unwrap();
        let text = |row: &Value, name| row.get_str(name).unwrap().to_php_string();
        let string = |row: &Value, name| String::from_utf8(text(row, name)).unwrap();
        let number = |row: &Value, name| row.get_str(name).unwrap().to_int().unsigned_abs();
        let posts = state
            .get_str("posts")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|(_, row)| {
                let post = hyscribe_model::Post {
                    id: number(row, "ID"),
                    author: number(row, "post_author"),
                    title: text(row, "post_title"),
                    content: text(row, "post_content"),
                    name: text(row, "post_name"),
                    date: string(row, "post_date"),
                    date_gmt: string(row, "post_date_gmt"),
                    modified: string(row, "post_modified"),
                    post_type: string(row, "post_type"),
                    status: string(row, "post_status"),
                    parent: number(row, "post_parent"),
                    menu_order: row.get_str("menu_order").unwrap().to_int(),
                    ..Default::default()
                };
                let terms = row
                    .get_str("terms")
                    .and_then(Value::as_array)
                    .unwrap_or_default()
                    .iter()
                    .map(|(_, term)| {
                        (
                            string(term, "taxonomy"),
                            hyscribe_model::Term {
                                id: number(term, "term_id"),
                                name: text(term, "name"),
                                slug: text(term, "slug"),
                                description: Vec::new(),
                            },
                        )
                    })
                    .collect();
                (post, terms)
            })
            .collect();
        let site = Filtered {
            base: DefaultInstall {
                home: state.get_str("home").unwrap().to_php_string(),
            },
            posts,
        };
        let request = site.base.request("");
        let ours = render_content(&theme, &site, &request, &markup);
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(4)].join("\n")
    );
    assert_eq!(count, 66);
}

#[test]
fn search_terms_split_like_wordpress() {
    use crate::query_loop::search_terms;
    assert_eq!(search_terms("a b,c+d\te"), ["a", "b", "c", "d", "e"]);
    assert!(search_terms("  ,+ ").is_empty());
}

/// `WP_Query::parse_orderby()`'s post columns, under both of their names.
#[test]
fn order_keys_name_post_columns() {
    use crate::query_loop::order_column;
    use hyscribe_model::OrderBy;
    for (keys, column) in [
        (&[&b"post_name"[..], b"name"][..], OrderBy::Name),
        (&[b"post_type", b"type"], OrderBy::Type),
        (&[b"comment_count"], OrderBy::CommentCount),
        (&[b"post_date", b"date"], OrderBy::Date),
        (&[b"post_modified", b"modified"], OrderBy::Modified),
    ] {
        for key in keys {
            assert_eq!(order_column(key), Some(column));
        }
    }
    assert_eq!(order_column(b"id"), None);
}
