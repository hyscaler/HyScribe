// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Comment, Post, PostQuery, Term};
use hyscribe_testkit::unhex;

use crate::blocks::parse_blocks;
use crate::core_blocks::render;
use crate::fixture::DefaultInstall;
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/latest-comments/golden.sh`, through the
/// render callback, as the oracle leaves out the elements support.
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
    let golden = include_str!("../../../tests/golden/latest-comments.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let (markup, php) = (unhex(markup), unhex(php));
        let context = Renderer::new(&theme, &site, &request).context;
        let block = &parse_blocks(&markup)[0];
        let attrs = block.attrs.as_deref().unwrap_or_default();
        let ours = render(&context, block.name.as_deref().unwrap(), attrs, b"").unwrap();
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
    assert_eq!(golden.lines().count(), 12);
}

/// Comments the default install has none like, on posts 1 (protected) and
/// 2 (untitled).
#[derive(Default)]
struct Odd {
    comments: Vec<Comment>,
}

impl Site for Odd {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }

    fn post(&self, id: u64) -> Option<Post> {
        Some(Post {
            id,
            title: if id == 1 {
                b"Locked".to_vec()
            } else {
                Vec::new()
            },
            password: if id == 1 { b"pw".to_vec() } else { Vec::new() },
            post_type: "post".into(),
            status: "publish".into(),
            ..Post::default()
        })
    }

    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }

    fn recent_comments(&self, _: u64) -> Vec<Comment> {
        self.comments.clone()
    }
}

/// Anonymous and user comments without a site, on a protected post and an
/// untitled one; and a site with no comments.
#[test]
fn odd_comments() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let html = |site: &dyn Site, markup: &[u8]| {
        let context = Renderer::new(&theme, site, &request).context;
        let block = &parse_blocks(markup)[0];
        let attrs = block.attrs.as_deref().unwrap_or_default();
        String::from_utf8(render(&context, "core/latest-comments", attrs, b"").unwrap()).unwrap()
    };
    let comment = |id, post_id, user_id| Comment {
        id,
        post_id,
        user_id,
        content: b"Some <b>text</b>".to_vec(),
        date: "2026-01-01 00:00:00".into(),
        ..Comment::default()
    };
    let site = Odd {
        comments: vec![comment(1, 1, 0), comment(2, 2, 3)],
    };
    let excerpts = html(&site, b"<!-- wp:latest-comments /-->");
    assert!(
        excerpts
            .contains("<span class=\"wp-block-latest-comments__comment-author\">Anonymous</span>")
    );
    assert!(
        excerpts.contains("href=\"/?author=3\">Anonymous</a>"),
        "{excerpts}"
    );
    assert!(excerpts.contains(">Protected: Locked</a>") && excerpts.contains(">(no title)</a>"));
    assert!(
        excerpts.contains("<p>Password protected</p>") && excerpts.contains("<p>Some text</p>")
    );
    let full = html(
        &site,
        br#"<!-- wp:latest-comments {"displayContent":"full"} /-->"#,
    );
    assert!(full.contains("<p>Password protected</p>") && full.contains("<p>Some <b>text</b></p>"));
    assert_eq!(
        html(&Odd::default(), b"<!-- wp:latest-comments /-->"),
        "<div class=\"has-avatars has-dates has-excerpts no-comments wp-block-latest-comments\">No comments to show.</div>"
    );
}
