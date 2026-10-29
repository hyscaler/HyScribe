// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Value, json_decode, parse_str};
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::render_content;
use crate::theme::twentytwentyfive;

/// The `pagination` cases of `reference/core-blocks/golden.sh`: page and
/// page count set on the main query in memory.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden
        .lines()
        .filter(|line| line.starts_with("pagination\t"))
    {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let (markup, state, php) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        let state = json_decode(&state).unwrap();
        let site = DefaultInstall {
            home: state.get_str("home").unwrap().to_php_string(),
        };
        let number = |name| state.get_str(name).unwrap().to_int() as u64;
        let single = state.get_str("single") == Some(&Value::Bool(true));
        let base = if single {
            site.request("p=1")
        } else {
            site.request("")
        };
        let request = Request {
            uri: state.get_str("uri").unwrap().to_php_string(),
            paged: number("paged"),
            max_pages: number("max"),
            found_posts: number("found"),
            ..base
        };
        let ours = render_content(&theme, &site, &request, &markup);
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&request.uri),
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
        failures.join("\n")
    );
    assert_eq!(count, 450);
}

/// `found_posts` counts every page's posts, whatever page the query asks for.
#[test]
fn count_posts_counts_every_page() {
    use crate::site::Site;
    let site = DefaultInstall { home: Vec::new() };
    let query = hyscribe_model::PostQuery {
        post_types: vec!["post".into(), "page".into()],
        limit: 1,
        offset: 1,
        ..Default::default()
    };
    assert_eq!(site.count_posts(&query), 2);
}

/// The default install with the posts the golden script inserts, each in
/// the default category.
struct More {
    base: DefaultInstall,
    posts: Vec<hyscribe_model::Post>,
}

impl crate::site::Site for More {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.base.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        let extra = self.posts.iter().find(|p| p.id == id).cloned();
        extra.or_else(|| self.base.post(id))
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        let extra = self.posts.iter().any(|p| p.id == post_id);
        self.base
            .post_terms(if extra { 1 } else { post_id }, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(post_id, key)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        let all = hyscribe_model::PostQuery {
            limit: u32::MAX,
            offset: 0,
            ..query.clone()
        };
        let mut posts = self.base.query(&all);
        posts.extend(
            self.posts
                .iter()
                .filter(|p| query.post_types.contains(&p.post_type))
                .cloned(),
        );
        posts.sort_by(|a, b| b.date.cmp(&a.date));
        posts
            .into_iter()
            .skip(query.offset as usize)
            .take(query.limit as usize)
            .collect()
    }
}

/// The `ownpage` cases of `reference/core-blocks/golden.sh`: queries of
/// their own, paged by `query-<id>-page`.
#[test]
fn own_pages_match_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("ownpage\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let (markup, state, php) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        let state = json_decode(&state).unwrap();
        let text = |row: &Value, name| row.get_str(name).unwrap().to_php_string();
        let posts = state
            .get_str("posts")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|(_, row)| hyscribe_model::Post {
                id: row.get_str("ID").unwrap().to_int() as u64,
                title: text(row, "post_title"),
                name: text(row, "post_name"),
                date: String::from_utf8(text(row, "post_date")).unwrap(),
                date_gmt: String::from_utf8(text(row, "post_date_gmt")).unwrap(),
                post_type: String::from_utf8(text(row, "post_type")).unwrap(),
                status: String::from_utf8(text(row, "post_status")).unwrap(),
                ..Default::default()
            })
            .collect();
        let site = More {
            base: DefaultInstall {
                home: state.get_str("home").unwrap().to_php_string(),
            },
            posts,
        };
        let uri = state.get_str("uri").unwrap().to_php_string();
        let request = Request {
            get: parse_str(uri.splitn(2, |&b| b == b'?').nth(1).unwrap_or_default()),
            uri,
            ..site.base.request("")
        };
        let ours = render_content(&theme, &site, &request, &markup);
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&request.uri),
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
    assert_eq!(count, 312);
}
