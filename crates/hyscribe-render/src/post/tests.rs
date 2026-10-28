// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use hyscribe_model::Term;
use hyscribe_phpser::{Key, json_decode};
use hyscribe_testkit::unhex;

use super::*;

/// Terms, thumbnail and sticky posts of one golden case.
struct Fixture {
    terms: HashMap<String, Vec<Term>>,
    thumbnail: Vec<u8>,
    sticky: Vec<u8>,
}

impl Site for Fixture {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        (name == "sticky_posts").then(|| self.sticky.clone())
    }

    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }

    fn post(&self, _: u64) -> Option<Post> {
        None
    }

    fn post_terms(&self, _: u64, taxonomy: &str) -> Vec<Term> {
        self.terms.get(taxonomy).cloned().unwrap_or_default()
    }

    fn post_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        (key == "_thumbnail_id").then(|| self.thumbnail.clone())
    }

    fn query(&self, _: &hyscribe_model::PostQuery) -> Vec<Post> {
        Vec::new()
    }
}

fn text(value: &Value, name: &str) -> String {
    String::from_utf8(value.get_str(name).unwrap().to_php_string()).unwrap()
}

/// The `class` cases of `reference/core-blocks/golden.sh`.
#[test]
fn post_classes_match_php() {
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("class\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let mut field = || fields.next().unwrap();
        let (case, row, php) = (
            json_decode(&field()).unwrap(),
            json_decode(&field()).unwrap(),
            field(),
        );
        let terms = case
            .get_str("terms")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|(taxonomy, list)| {
                let Key::String(taxonomy) = taxonomy else {
                    panic!("{taxonomy:?}")
                };
                let list = list
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|(_, term)| Term {
                        id: term.get(&Key::Int(0)).unwrap().to_int() as u64,
                        slug: term.get(&Key::Int(1)).unwrap().to_php_string(),
                        ..Term::default()
                    })
                    .collect();
                (String::from_utf8(taxonomy.clone()).unwrap(), list)
            })
            .collect();
        let site = Fixture {
            terms,
            thumbnail: case.get_str("thumbnail").unwrap().to_php_string(),
            sticky: case.get_str("sticky").unwrap().to_php_string(),
        };
        let request = Request {
            home: case.get_str("home") == Some(&Value::Bool(true)),
            paged: case.get_str("paged") == Some(&Value::Bool(true)),
            ..Request::default()
        };
        let post = Post {
            id: row.get_str("ID").unwrap().to_int() as u64,
            status: text(&row, "post_status"),
            password: row.get_str("post_password").unwrap().to_php_string(),
            post_type: text(&row, "post_type"),
            ..Post::default()
        };
        let ours = post_class(&site, &request, "wp-block-post", &post);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{case:?}"
        );
        count += 1;
    }
    assert!(count >= 64);
}

#[test]
fn sanitize_html_class_like_php() {
    // sanitize_html_class() in PHP WordPress 7.1.3.
    for (class, fallback, php) in [
        ("a%zzb", "", "azzb"),
        ("%2gx", "", "2gx"),
        ("!!", "x y", "xy"),
        ("%41b", "", "b"),
        ("", "", ""),
    ] {
        assert_eq!(
            sanitize_html_class(class.as_bytes(), fallback.as_bytes()),
            php.as_bytes(),
            "{class:?}"
        );
    }
}
