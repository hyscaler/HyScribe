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
        // The oracle's changes are to post 1; its counts are of every post.
        let field = |name| self.text(&["post", name]).filter(|_| id == 1);
        Some(Post {
            comment_status: field("comment_status").map_or(post.comment_status.clone(), |s| {
                String::from_utf8(s).unwrap()
            }),
            comment_count: field("comment_count")
                .or_else(|| self.text(&["counts", &id.to_string()]))
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
    fn comment(&self, id: u64) -> Option<Comment> {
        feed_comments(&self.state)
            .into_iter()
            .find(|comment| comment.id == id)
            .or_else(|| self.base.comment(id))
    }
    fn user(&self, id: u64) -> Option<User> {
        self.base.user(id).map(|user| User {
            url: self.text(&["user_url"]).unwrap_or(user.url.clone()),
            ..user
        })
    }
}

/// The PHP feed with HyScribe's generator, as ours prints it.
fn with_hyscribe(feed: &[u8]) -> String {
    let (repository, version) = (env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_VERSION"));
    String::from_utf8(feed.to_vec())
        .unwrap()
        .replace(
            "https://wordpress.org/?v=7.1.3",
            &format!("{repository}?v={version}"),
        )
        .replace(
            "<generator uri=\"https://wordpress.org/\" version=\"7.1.3\">WordPress</generator>",
            &format!("<generator uri=\"{repository}\" version=\"{version}\">HyScribe</generator>"),
        )
        .replace(
            "<!-- generator=\"WordPress/7.1.3\" -->",
            &format!("<!-- generator=\"HyScribe/{version}\" -->"),
        )
}

/// Where `ours` parts from `php`, with some context.
fn difference(php: &str, ours: &[u8]) -> String {
    let ours = String::from_utf8_lossy(ours);
    let at = ours
        .bytes()
        .zip(php.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    format!(
        "\n  php:  {:?}\n  ours: {:?}",
        php.get(at.saturating_sub(60)..php.len().min(at + 160))
            .unwrap_or_default(),
        ours.get(at.saturating_sub(60)..ours.len().min(at + 160))
            .unwrap_or_default(),
    )
}

/// Every case of `reference/feed/golden.sh`: the home feed with options
/// and post 1 changed, in RSS2 and Atom.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/feed.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let site = Changed {
            base: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            state: json_decode(&fields[0]).unwrap(),
        };
        let (php, php_atom) = (with_hyscribe(&fields[1]), with_hyscribe(&fields[2]));
        let (php_rdf, php_rss) = (with_hyscribe(&fields[3]), with_hyscribe(&fields[4]));
        let title = php
            .get(php.find("<title>").unwrap() + 7..php.find("</title>").unwrap())
            .unwrap();
        let request = Request {
            uri: b"/?feed=rss2".to_vec(),
            posts: vec![site.post(1).unwrap()],
            ..site.base.request("")
        };
        let last = Some("2026-10-27 08:08:15");
        let ours = rss2(&theme, &site, &request, title.as_bytes(), last);
        if ours != php.as_bytes() {
            failures.push(format!("rss2 {}{}", line.len(), difference(&php, &ours)));
        }
        let request = Request {
            uri: b"/?feed=atom".to_vec(),
            ..request
        };
        let ours = atom(&theme, &site, &request, title.as_bytes(), last);
        if ours != php_atom.as_bytes() {
            failures.push(format!(
                "atom {:?}{}",
                site.state,
                difference(&php_atom, &ours)
            ));
        }
        let request = Request {
            uri: b"/?feed=rdf".to_vec(),
            ..request
        };
        let ours = rdf(&theme, &site, &request, title.as_bytes(), last);
        if ours != php_rdf.as_bytes() {
            failures.push(format!(
                "rdf {:?}{}",
                site.state,
                difference(&php_rdf, &ours)
            ));
        }
        let request = Request {
            uri: b"/?feed=comments-rss".to_vec(),
            ..request
        };
        let ours = rss092(&theme, &site, &request, title.as_bytes(), last);
        if ours != php_rss.as_bytes() {
            failures.push(format!(
                "rss {:?}{}",
                site.state,
                difference(&php_rss, &ours)
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

/// The comments of a case, as the oracle's feed query found them.
fn feed_comments(state: &Value) -> Vec<Comment> {
    let text = |row: &Value, name| row.get_str(name).unwrap().to_php_string();
    let number = |row: &Value, name| row.get_str(name).unwrap().to_int() as u64;
    state
        .get_str("comments")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(|(_, row)| Comment {
            id: number(row, "comment_ID"),
            post_id: number(row, "comment_post_ID"),
            author: text(row, "comment_author"),
            author_email: text(row, "comment_author_email"),
            author_url: text(row, "comment_author_url"),
            author_ip: text(row, "comment_author_IP"),
            date: String::from_utf8(text(row, "comment_date")).unwrap(),
            date_gmt: String::from_utf8(text(row, "comment_date_gmt")).unwrap(),
            content: text(row, "comment_content"),
            comment_type: text(row, "comment_type"),
            parent: number(row, "comment_parent"),
            user_id: number(row, "user_id"),
        })
        .collect()
}

/// Every comment feed case of `reference/feed/golden.sh`.
#[test]
fn comment_feeds_match_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/comment-feeds.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let state = String::from_utf8_lossy(&fields[0]).into_owned();
        let site = Changed {
            base: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            state: json_decode(&fields[0]).unwrap(),
        };
        let (php, php_atom) = (with_hyscribe(&fields[1]), with_hyscribe(&fields[2]));
        let query = site.state.get_str("query").unwrap();
        let id = |name| query.get_str(name).map(|id| id.to_int() as u64);
        let (single, page) = (id("p"), id("page_id"));
        let search = query.get_str("s").map(Value::to_php_string);
        let post = single.or(page).unwrap_or(1);
        let uri = format!(
            "/?{}",
            query
                .as_array()
                .unwrap()
                .iter()
                .map(|(k, v)| format!(
                    "{}={}",
                    String::from_utf8(k.to_php_string()).unwrap(),
                    String::from_utf8(hyscribe_phpser::urlencode(&v.to_php_string())).unwrap()
                ))
                .collect::<Vec<_>>()
                .join("&")
        );
        let request = Request {
            singular: single.or(page).is_some(),
            search,
            uri: uri.into_bytes(),
            posts: vec![site.post(post).unwrap()],
            ..site.base.request("")
        };
        let title = php
            .get(php.find("Comments for ").map_or(0, |at| at + 13)..php.find("\t</title>").unwrap())
            .unwrap();
        let ours = rss2_comments(
            &theme,
            &site,
            &request,
            title.as_bytes(),
            single.is_some(),
            &feed_comments(&site.state),
            Some("2026-10-27 08:08:15"),
        );
        if ours != php.as_bytes() {
            failures.push(format!("rss2 {state}{}", difference(&php, &ours)));
        }
        let atom_uri = String::from_utf8(request.uri.clone())
            .unwrap()
            .replace("rss2", "atom");
        let request = Request {
            uri: atom_uri.into_bytes(),
            ..request
        };
        let ours = atom_comments(
            &theme,
            &site,
            &request,
            title.as_bytes(),
            &feed_comments(&site.state),
            Some("2026-10-27 08:08:15"),
        );
        if ours != php_atom.as_bytes() {
            failures.push(format!("atom {state}{}", difference(&php_atom, &ours)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 12);
}
