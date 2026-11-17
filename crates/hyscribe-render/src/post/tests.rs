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
            paged: if case.get_str("paged") == Some(&Value::Bool(true)) {
                2
            } else {
                0
            },
            ..Request::default()
        };
        let post = Post {
            id: row.get_str("ID").unwrap().to_int() as u64,
            status: text(&row, "post_status"),
            password: row.get_str("post_password").unwrap().to_php_string(),
            post_type: text(&row, "post_type"),
            ..Post::default()
        };
        let ours = post_class(&site, Some(&request), "wp-block-post", &post);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{case:?}"
        );
        count += 1;
    }
    assert!(count >= 64);
}

/// The admin's classes: no bare post type, and `status-sticky`.
#[test]
fn admin_post_classes() {
    let site = Fixture {
        terms: HashMap::from([(
            "category".to_string(),
            vec![Term {
                id: 1,
                slug: b"uncategorized".to_vec(),
                ..Term::default()
            }],
        )]),
        thumbnail: Vec::new(),
        sticky: b"a:1:{i:0;i:7;}".to_vec(),
    };
    let post = |id| Post {
        id,
        status: "publish".into(),
        post_type: "post".into(),
        ..Post::default()
    };
    let class = |id| String::from_utf8(post_class(&site, None, "iedit", &post(id))).unwrap();
    assert_eq!(
        class(7),
        "iedit post-7 type-post status-publish format-standard status-sticky hentry category-uncategorized"
    );
    assert_eq!(
        class(8),
        "iedit post-8 type-post status-publish format-standard hentry category-uncategorized"
    );
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

/// The body classes of term archives in `terms.tsv` from
/// `reference/head/golden.sh`, over the terms of
/// `reference/default-install/terms.sql`.
#[test]
fn term_archive_body_classes_match_php() {
    let golden = include_str!("../../tests/golden/terms.tsv");
    let site = crate::fixture::DefaultInstall { home: Vec::new() };
    let term = |id, slug: &str| Term {
        id,
        slug: slug.into(),
        ..Term::default()
    };
    let cases = [
        (
            "/?tag=foo",
            Archive::Term(Taxonomy::Tag, Some(term(2, "foo"))),
        ),
        (
            "/?m=2026&tag=foo",
            Archive::Term(Taxonomy::Tag, Some(term(2, "foo"))),
        ),
        (
            "/?post_format=aside",
            Archive::Term(Taxonomy::Format, Some(term(3, "post-format-aside"))),
        ),
        (
            "/?post_format=,post-format-aside",
            Archive::Term(Taxonomy::Format, None),
        ),
        (
            "/?s=hello&taxonomy=foo&term=x",
            Archive::Term(Taxonomy::Format, None),
        ),
        (
            "/?post_format=aside&page=2",
            Archive::Term(Taxonomy::Format, Some(term(3, "post-format-aside"))),
        ),
        (
            "/?tag=foo&page=2",
            Archive::Term(Taxonomy::Tag, Some(term(2, "foo"))),
        ),
    ];
    for (path, archive) in cases {
        let line = golden
            .lines()
            .find(|line| {
                line.starts_with(&format!("{}\t", hyscribe_phpser::bin2hex(path.as_bytes())))
            })
            .unwrap();
        let page = String::from_utf8(unhex(line.rsplit('\t').next().unwrap())).unwrap();
        let php = page
            .split("<body class=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let request = Request {
            archive: Some(archive),
            date: path.contains("m=2026"),
            search: path.contains("s=hello").then(|| b"hello".to_vec()),
            page: if path.ends_with("&page=2") { 2 } else { 0 },
            ..Request::default()
        };
        let ours = body_class(&site, &request, "twentytwentyfive");
        assert_eq!(String::from_utf8_lossy(&ours), php, "{path}");
    }
}

/// A page with a published child page is a parent, as on the Theme Test
/// Data's page hierarchy (`reference/pages/golden.sh ttd`).
#[test]
fn pages_with_published_children_are_parents() {
    struct Pages(Vec<Post>);
    impl Site for Pages {
        fn option(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn home_url(&self) -> Vec<u8> {
            Vec::new()
        }
        fn post(&self, _: u64) -> Option<Post> {
            None
        }
        fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
            Vec::new()
        }
        fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn query(&self, query: &PostQuery) -> Vec<Post> {
            crate::fixture::select(self.0.clone(), query, &|_, _| Vec::new())
        }
    }
    let page = |id, parent, status: &str| Post {
        id,
        parent,
        post_type: "page".into(),
        status: status.into(),
        ..Post::default()
    };
    let site = Pages(vec![
        page(1, 0, "publish"),
        page(2, 1, "publish"),
        page(3, 2, "draft"),
    ]);
    let class = |page: &Post| {
        let request = Request {
            posts: vec![page.clone()],
            singular: true,
            ..Request::default()
        };
        String::from_utf8(body_class(&site, &request, "t")).unwrap()
    };
    assert!(class(&site.0[0]).contains(" page-id-1 page-parent "));
    assert!(class(&site.0[1]).contains(" page-id-2 page-child parent-pageid-1 "));
}

/// The `body` cases of `reference/core-blocks/golden.sh`.
#[test]
fn body_classes_match_php() {
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let site = crate::fixture::DefaultInstall { home: Vec::new() };
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("body\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let request = String::from_utf8(fields.next().unwrap()).unwrap();
        let php = fields.next().unwrap();
        let ours = body_class(&site, &site.request(&request), "twentytwentyfive");
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{request}"
        );
        count += 1;
    }
    assert!(count >= 14);
}

/// The admin prefixes neither a protected nor a private post's title.
#[test]
fn admin_titles_have_no_prefix() {
    let post = Post {
        title: b" \"Quoted\" ".to_vec(),
        password: b"secret".to_vec(),
        status: "private".into(),
        ..Post::default()
    };
    assert_eq!(admin_title(&post), "&#8220;Quoted&#8221;".as_bytes());
    assert_eq!(
        the_title(&post),
        "Protected:  &#8220;Quoted&#8221;".as_bytes()
    );
}
