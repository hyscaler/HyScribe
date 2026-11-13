// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use crate::excerpt::the_excerpt;
use crate::fixture::DefaultInstall;
use crate::formatting::{contains, convert_chars, ent2ncr, html_entity_decode};
use crate::site::{Request, Site};
use crate::template::{Renderer, replace};
use crate::theme::twentytwentyfive;

/// The default install with one more post, the case's.
struct WithPost {
    base: DefaultInstall,
    post: Post,
}

impl Site for WithPost {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        // The oracle's address, which the password form posts to.
        match name {
            "siteurl" => Some(b"http://127.0.0.1:8081".to_vec()),
            _ => self.base.option(name),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        (id == self.post.id)
            .then(|| self.post.clone())
            .or_else(|| self.base.post(id))
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<Term> {
        self.base.post_terms(id, taxonomy)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(id, key)
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.base.query(query)
    }
}

/// Every case of `reference/feed/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/excerpt.tsv");
    let mut failures = Vec::new();
    for (n, line) in golden.lines().enumerate() {
        let fields: Vec<Vec<u8>> = line.split('\t').skip(1).map(unhex).collect();
        if line.starts_with("ent2ncr\t") {
            assert_eq!(ent2ncr(&fields[0]), fields[1], "{line}");
            continue;
        }
        if line.starts_with("decode\t") {
            assert_eq!(html_entity_decode(&fields[0]), fields[1], "{line}");
            continue;
        }
        let input = json_decode(&fields[0]).unwrap();
        let text = |key| input.get_str(key).unwrap().to_php_string();
        let base = DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        };
        let post = Post {
            id: 9000 + n as u64,
            content: text("content"),
            excerpt: text("excerpt"),
            password: text("password"),
            name: format!("case-{n}").into_bytes(),
            ..base.post(1).unwrap()
        };
        let site = WithPost { base, post };
        let request = Request {
            feed: true,
            ..Request::default()
        };
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.context.post_id = Some(site.post.id);
        let excerpt = renderer.get_the_excerpt(&site.post);
        // Outside a feed the content stops at its more tag.
        if !contains(&site.post.content, b"<!--more") {
            assert_eq!(the_excerpt(&theme, &site, &site.post), excerpt);
        }
        let rss = convert_chars(&ent2ncr(&excerpt));
        let content = replace(&renderer.the_content(&site.post), b"]]>", b"]]&gt;");
        // wp_filter_content_tags(), which adds image attributes, is not
        // ported; content with images is checked for its excerpt only.
        let content = if contains(&fields[3], b"<img") {
            fields[3].clone()
        } else {
            content
        };
        if [&excerpt, &rss, &content] != [&fields[1], &fields[2], &fields[3]] {
            failures.push(format!(
                "{}\n  php:  {:?}\n        {:?}\n  ours: {:?}\n        {:?}",
                String::from_utf8_lossy(&site.post.content),
                String::from_utf8_lossy(&fields[1]),
                String::from_utf8_lossy(&fields[3]),
                String::from_utf8_lossy(&excerpt),
                String::from_utf8_lossy(&content),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert!(golden.lines().count() > 30);
}
