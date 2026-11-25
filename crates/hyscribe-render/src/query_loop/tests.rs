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
    sticky: Vec<u8>,
    posts: Vec<(hyscribe_model::Post, Vec<(String, hyscribe_model::Term)>)>,
}

impl crate::site::Site for Filtered {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "sticky_posts" => Some(self.sticky.clone()),
            _ => self.base.option(name),
        }
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
                post_types: vec!["post".into(), "page".into()],
                limit: u32::MAX,
                ..Default::default()
            })
            .into_iter()
            .chain(self.posts.iter().map(|(post, _)| post.clone()))
            .collect();
        crate::fixture::select(posts, query, &|id, taxonomy| self.post_terms(id, taxonomy))
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
                                ..hyscribe_model::Term::default()
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
            sticky: state.get_str("sticky").unwrap().to_php_string(),
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
    assert_eq!(count, 123);
}

/// Every case of `reference/search-terms/golden.sh`, through the query
/// block's `stripslashes()`.
#[test]
fn search_terms_match_php() {
    use crate::query_loop::{search_count, search_terms};
    use hyscribe_phpser::stripslashes;
    use hyscribe_phpser::{Key, Value, json_encode};
    let golden = include_str!("../../tests/golden/search-terms.tsv");
    for line in golden.lines() {
        let mut fields = line.split('\t').map(unhex);
        let (search, php) = (fields.next().unwrap(), fields.next().unwrap());
        let search = String::from_utf8(stripslashes(&search)).unwrap();
        let terms = search_terms(&search)
            .into_iter()
            .enumerate()
            .map(|(i, term)| (Key::Int(i as i64), Value::String(term.into_bytes())))
            .collect();
        assert_eq!(
            json_encode(&Value::Array(terms)).unwrap(),
            php,
            "{search:?}"
        );
    }
    assert_eq!(golden.lines().count(), 54);
    assert!(search_terms("").is_empty());
    // The words before any are dropped, one at the least.
    assert_eq!(
        [
            search_count("the red, fox"),
            search_count(""),
            search_count("a\r\nb")
        ],
        [3, 1, 1]
    );
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
        (&[b"rand"], OrderBy::Random),
        (&[b"post__in"], OrderBy::Included),
        (&[b"post_parent__in"], OrderBy::Parents),
        (&[b"post_name__in"], OrderBy::Names),
    ] {
        for key in keys {
            assert_eq!(order_column(key), Some(column));
        }
    }
    assert_eq!(order_column(b"id"), None);
}

/// `RAND(n)` as `parse_orderby()` reads it: the first match anywhere, its
/// digits cast as `(int)` casts them.
#[test]
fn rand_seeds_read_as_php_does() {
    use crate::query_loop::order_column;
    use hyscribe_model::OrderBy;
    for (key, seed) in [
        (&b"RAND(42)"[..], Some(42)),
        (b"rand(x)Rand(007)", Some(7)),
        (b"RAND(99999999999999999999)", Some(i64::MAX as u64)),
        (b"RAND(9223372036854775808)", Some(i64::MAX as u64)),
        (b"RAND(12", None),
        (b"RAND()", None),
    ] {
        assert_eq!(
            order_column(key).filter(|c| matches!(c, OrderBy::Seeded(_))),
            seed.map(OrderBy::Seeded),
            "{}",
            String::from_utf8_lossy(key)
        );
    }
}

/// `render_block_core_query()` leaves the block's style handle out unless
/// the query is interactive, so a plain query never queues `wp-block-query`
/// (and the theme styles of the block are not on the page); an interactive
/// one queues it after what its inner blocks queued.
#[test]
fn the_query_style_is_queued_by_an_interactive_query_only() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://example.test".to_vec(),
    };
    let request = Request::default();
    for (attributes, queued) in [
        (r#"{"queryId":1,"query":{"inherit":false}}"#, false),
        (
            r#"{"queryId":1,"enhancedPagination":true,"query":{"inherit":false}}"#,
            true,
        ),
    ] {
        let markup = format!(
            "<!-- wp:query {attributes} --><div><!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template --></div><!-- /wp:query -->"
        );
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.render(markup.as_bytes());
        let styles = &renderer.assets.styles;
        assert_eq!(styles.iter().any(|style| style == "wp-block-query"), queued);
        assert_eq!(
            styles.last().map(String::as_str),
            Some(if queued {
                "wp-block-query"
            } else {
                "wp-block-post-template"
            })
        );
    }
}

/// The default install for a logged in user, keeping the queries asked.
struct LoggedIn {
    base: DefaultInstall,
    user: u64,
    asked: std::cell::RefCell<Vec<hyscribe_model::PostQuery>>,
}

impl crate::site::Site for LoggedIn {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.base.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.base.post(id)
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.base.post_terms(post_id, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(post_id, key)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.asked.borrow_mut().push(query.clone());
        self.base.query(query)
    }
    fn viewer(&self) -> hyscribe_model::Viewer {
        hyscribe_model::Viewer {
            user: self.user,
            read_private: vec!["post".into()],
        }
    }
}

/// A query block's own query lists a logged in user's readable private
/// posts and searches password protected posts too; a visitor's neither.
#[test]
fn own_queries_follow_the_user() {
    let theme = twentytwentyfive();
    let markup = br#"<!-- wp:query {"query":{"perPage":3,"search":"hello","inherit":false}} --><div><!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template --></div><!-- /wp:query -->"#;
    for user in [0, 3] {
        let site = LoggedIn {
            base: DefaultInstall {
                home: b"http://example.test".to_vec(),
            },
            user,
            asked: Default::default(),
        };
        render_content(&theme, &site, &Request::default(), markup);
        let asked = site.asked.borrow();
        let query = asked.first().unwrap();
        assert_eq!(query.logged_in, user != 0);
        let readable = crate::site::Site::viewer(&site).statuses(&["post".to_string()]);
        assert_eq!(query.statuses, readable);
    }
}
