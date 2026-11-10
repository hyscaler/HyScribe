// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/latest-posts/golden.sh`, on the home page,
/// whose global post is the default install's one post.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request {
        home: true,
        post: Some(1),
        ..Request::default()
    };
    let golden = include_str!("../../tests/golden/latest-posts.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let (markup, php) = (unhex(markup), unhex(php));
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
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
        failures[..failures.len().min(4)].join("\n")
    );
    assert_eq!(golden.lines().count(), 40);
}

/// Posts the default install has none like, and the queries asked for.
struct Odd {
    posts: Vec<hyscribe_model::Post>,
    queries: std::cell::RefCell<Vec<hyscribe_model::PostQuery>>,
}

impl crate::site::Site for Odd {
    /// `posts_per_page` 0, which `WP_Query` takes as 1.
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        (name == "posts_per_page").then(|| b"0".to_vec())
    }

    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }

    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.posts.iter().find(|p| p.id == id).cloned()
    }

    fn post_terms(&self, _: u64, _: &str) -> Vec<hyscribe_model::Term> {
        Vec::new()
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.queries.borrow_mut().push(query.clone());
        self.posts.clone()
    }
}

/// An untitled post, a protected one, and one whose content lists the
/// posts in full, itself among them, which the block's guard leaves empty.
#[test]
fn untitled_protected_and_nested_posts() {
    const FULL: &[u8] =
        br#"<!-- wp:latest-posts {"displayPostContent":true,"displayPostContentRadio":"full_post","postsToShow":0} /-->"#;
    let post = |id, title: &[u8], password: &[u8], content: &[u8]| hyscribe_model::Post {
        id,
        title: title.to_vec(),
        password: password.to_vec(),
        content: content.to_vec(),
        post_type: "post".into(),
        status: "publish".into(),
        date: "2026-01-01 00:00:00".into(),
        ..hyscribe_model::Post::default()
    };
    let site = Odd {
        posts: vec![post(5, b"", b"", FULL), post(6, b"Locked", b"pw", b"")],
        queries: std::cell::RefCell::default(),
    };
    let theme = twentytwentyfive();
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &site, &request);
    let html = String::from_utf8(renderer.render(FULL)).unwrap();
    assert!(html.contains(">(no title)</a>"), "{html}");
    assert!(html.contains("full-content\">This content is password protected.</div>"));
    // The outer list renders post 5, whose list leaves post 5 empty.
    assert_eq!(html.matches("full-content\"></div>").count(), 1, "{html}");
    let excerpts = String::from_utf8(
        renderer.render(br#"<!-- wp:latest-posts {"displayPostContent":true} /-->"#),
    )
    .unwrap();
    assert!(excerpts.contains("excerpt\">This content is password protected.</div>"));
    // No user, so no byline.
    let authors = renderer.render(br#"<!-- wp:latest-posts {"displayAuthor":true} /-->"#);
    assert!(!String::from_utf8(authors).unwrap().contains("post-author"));
    renderer.render(br#"<!-- wp:latest-posts {"selectedAuthor":0} /-->"#);
    renderer.render(br#"<!-- wp:latest-posts {"order":"asc","orderBy":"title"} /-->"#);
    let queries = site.queries.borrow();
    // Author 0 is no author, neither in nor out.
    let none = &queries[queries.len() - 2];
    assert!(none.authors.is_empty() && none.excluded_authors.is_empty());
    assert_eq!(queries[0].limit, 1);
    let title = (hyscribe_model::OrderBy::Title, true);
    assert_eq!(queries.last().unwrap().order, [title]);
}
