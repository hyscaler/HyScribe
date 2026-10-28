// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Comment, Post, PostQuery, Term, User};
use hyscribe_phpser::{Value, json_decode};
use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::theme::twentytwentyfive;

/// The default install with a case's changes to options and post 1.
struct Changed {
    base: DefaultInstall,
    state: Value,
}

impl Changed {
    fn text(&self, path: &[&str]) -> Option<Vec<u8>> {
        self.state.get_path(path).map(Value::to_php_string)
    }
}

impl Site for Changed {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "siteurl" => Some(b"http://127.0.0.1:8081".to_vec()),
            _ => self
                .text(&["options", name])
                .or_else(|| self.base.option(name)),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        let post = self.base.post(id)?;
        if id != 1 {
            return Some(post);
        }
        let field = |name| self.text(&["post", name]);
        Some(Post {
            comment_status: field("comment_status").map_or(post.comment_status.clone(), |s| {
                String::from_utf8(s).unwrap()
            }),
            comment_count: field("comment_count")
                .map_or(post.comment_count, |n| Value::String(n).to_int()),
            content: field("post_content").unwrap_or(post.content.clone()),
            excerpt: field("post_excerpt").unwrap_or(post.excerpt.clone()),
            title: field("post_title").unwrap_or(post.title.clone()),
            password: field("post_password").unwrap_or(post.password.clone()),
            ..post
        })
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<Term> {
        let terms = self.base.post_terms(id, taxonomy);
        match (
            taxonomy,
            self.state.get_str("tags"),
            self.text(&["category"]),
        ) {
            ("post_tag", Some(Value::Array(tags)), _) => tags
                .iter()
                .map(|(_, name)| Term {
                    name: name.to_php_string(),
                    ..Term::default()
                })
                .collect(),
            ("category", _, Some(name)) => terms
                .into_iter()
                .map(|term| Term {
                    name: name.clone(),
                    ..term
                })
                .collect(),
            _ => terms,
        }
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        match key {
            "enclosure" => self.text(&["enclosure"]),
            _ => self.base.post_meta(id, key),
        }
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.base.query(query)
    }
    fn comments(&self, post_id: u64) -> Vec<Comment> {
        self.base.comments(post_id)
    }
    fn user(&self, id: u64) -> Option<User> {
        self.base.user(id)
    }
}

/// Every case of `reference/feed/golden.sh`: the home feed with options
/// and post 1 changed.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/feed.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (state, php) = line.split_once('\t').unwrap();
        let site = Changed {
            base: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            state: json_decode(&unhex(state)).unwrap(),
        };
        let php = String::from_utf8(unhex(php)).unwrap().replace(
            "https://wordpress.org/?v=7.1.3",
            &format!(
                "{}?v={}",
                env!("CARGO_PKG_REPOSITORY"),
                env!("CARGO_PKG_VERSION")
            ),
        );
        let title = php
            .get(php.find("<title>").unwrap() + 7..php.find("</title>").unwrap())
            .unwrap();
        let request = Request {
            uri: b"/?feed=rss2".to_vec(),
            posts: vec![site.post(1).unwrap()],
            ..site.base.request("")
        };
        let ours = rss2(
            &theme,
            &site,
            &request,
            title.as_bytes(),
            Some("2026-10-27 08:08:15"),
        );
        if ours != php.as_bytes() {
            let ours = String::from_utf8_lossy(&ours);
            let at = ours
                .bytes()
                .zip(php.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            failures.push(format!(
                "{state}\n  php:  {:?}\n  ours: {:?}",
                php.get(at.saturating_sub(60)..php.len().min(at + 120))
                    .unwrap_or_default(),
                ours.get(at.saturating_sub(60)..ours.len().min(at + 120))
                    .unwrap_or_default(),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 14);
}

/// A feed of no posts is built at the site's last change, if any.
#[test]
fn feed_without_posts() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request {
        feed: true,
        ..Request::default()
    };
    let feed = |last| String::from_utf8(rss2(&theme, &site, &request, b"T", last)).unwrap();
    assert!(
        feed(Some("2026-10-27 08:08:15"))
            .contains("<lastBuildDate>Tue, 27 Oct 2026 08:08:15 +0000</lastBuildDate>")
    );
    assert!(feed(None).contains("<lastBuildDate></lastBuildDate>"));
    assert!(feed(None).ends_with("</generator>\n</channel>\n</rss>\n"));
}
