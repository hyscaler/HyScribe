// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{PostQuery, Term};
use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use super::*;
use crate::core_blocks::Context;
use crate::site::{Request, Site};
use crate::template::render_content;
use crate::theme::twentytwentyfive;

/// One in-memory post of the golden script, seen from `home`.
struct Fixture {
    post: Post,
    home: Vec<u8>,
    siteurl: Vec<u8>,
    smilies: bool,
}

impl Site for Fixture {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "use_smilies" => Some(if self.smilies {
                b"1".to_vec()
            } else {
                b"0".to_vec()
            }),
            "siteurl" => Some(self.siteurl.clone()),
            _ => None,
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.home.clone()
    }

    fn post(&self, id: u64) -> Option<Post> {
        (id == self.post.id).then(|| self.post.clone())
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
}

/// The site and request of a `content` or `teaser` case.
fn case(state: &[u8], row: &[u8]) -> (Fixture, Request) {
    let state = json_decode(state).unwrap();
    let row = json_decode(row).unwrap();
    let text = |name| row.get_str(name).unwrap().to_php_string();
    let post = Post {
        id: row.get_str("ID").unwrap().to_int() as u64,
        title: text("post_title"),
        content: text("post_content"),
        password: text("post_password"),
        status: "publish".into(),
        post_type: "post".into(),
        ..Post::default()
    };
    let request = Request {
        singular: state.get_str("singular") == Some(&Value::Bool(true)),
        post: Some(post.id),
        ..Request::default()
    };
    let site = Fixture {
        post,
        home: state.get_str("home").unwrap().to_php_string(),
        siteurl: state.get_str("siteurl").unwrap().to_php_string(),
        smilies: true,
    };
    (site, request)
}

/// The `content` and `teaser` cases of `reference/core-blocks/golden.sh`.
#[test]
fn post_content_matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut count = 0;
    for line in golden.lines() {
        let mut fields = line.split('\t');
        let kind = fields.next().unwrap();
        let mut field = || unhex(fields.next().unwrap());
        let (ours, php, what) = match kind {
            "content" => {
                let (markup, state, row, php) = (field(), field(), field(), field());
                let (site, request) = case(&state, &row);
                (
                    render_content(&theme, &site, &request, &markup),
                    php,
                    markup,
                )
            }
            "teaser" => {
                let (state, row, php) = (field(), field(), field());
                let (site, request) = case(&state, &row);
                let renderer = Renderer {
                    context: Context {
                        theme: &theme,
                        site: &site,
                        request: &request,
                        post_id: request.post,
                        query: None,
                    },
                    open_parts: Vec::new(),
                    open_patterns: Vec::new(),
                    open_posts: Vec::new(),
                };
                (get_the_content(&renderer, &site.post), php, row)
            }
            _ => continue,
        };
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{}",
            String::from_utf8_lossy(&what)
        );
        count += 1;
    }
    assert!(count >= 46);
}

#[test]
fn insecure_home_urls_become_https() {
    let content = br#"<a href="http://example.test/x">x</a> "http:\/\/example.test\/y""#;
    assert_eq!(
        String::from_utf8(replace_insecure_home_url(b"https://example.test", content)).unwrap(),
        r#"<a href="https://example.test/x">x</a> "https:\/\/example.test\/y""#
    );
    assert_eq!(
        replace_insecure_home_url(b"http://example.test", content),
        content
    );
}

#[test]
fn post_content_needs_a_post() {
    let theme = twentytwentyfive();
    let (mut site, request) = case(
        br#"{"singular":true,"home":"","siteurl":""}"#,
        br#"{"ID":5,"post_title":"T","post_content":"<p>:)</p>","post_password":""}"#,
    );
    let html = |site: &Fixture, request: &Request, markup: &str| {
        String::from_utf8(render_content(&theme, site, request, markup.as_bytes())).unwrap()
    };
    let outside = Request {
        post: None,
        ..request.clone()
    };
    assert_eq!(html(&site, &outside, "<!-- wp:post-content /-->"), "");
    let missing = Request {
        post: Some(6),
        ..request.clone()
    };
    assert_eq!(html(&site, &missing, "<!-- wp:post-content /-->"), "");
    assert_eq!(
        html(
            &site,
            &request,
            r#"<!-- wp:post-content {"tagName":"section"} /-->"#
        ),
        r#"<section class="entry-content wp-block-post-content">"#.to_string()
            + "<p>\u{1f642}</p></section>"
    );
    site.smilies = false;
    assert!(html(&site, &request, "<!-- wp:post-content /-->").contains("<p>:)</p>"));
    site.smilies = true;
    // Unterminated more tags and delimiters are left as they are.
    site.post.content = b"<p>a</p><!--more".to_vec();
    assert!(html(&site, &request, "<!-- wp:post-content /-->").contains("<p>a</p><!--more"));
    site.post.content = b"<p>a</p><!--more--><!-- wp:more".to_vec();
    assert!(
        html(&site, &request, "<!-- wp:post-content /-->")
            .contains(r#"<span id="more-5"></span><!-- wp:more"#)
    );
}

#[test]
fn empty_paragraphs_stay_empty() {
    let theme = twentytwentyfive();
    let (site, request) = case(
        br#"{"singular":true,"home":"","siteurl":""}"#,
        br#"{"ID":5,"post_title":"T","post_content":"","post_password":""}"#,
    );
    assert_eq!(
        render_content(&theme, &site, &request, b"<!-- wp:paragraph /-->"),
        b""
    );
}

#[test]
fn later_pages_of_a_post() {
    let theme = twentytwentyfive();
    let (site, request) = case(br#"{"singular":true,"home":"","siteurl":""}"#, br#"{"ID":5,"post_title":"T","post_content":"<p>One</p><!--nextpage--><p>Two</p><!--nextpage--><p>Three</p>","post_password":""}"#);
    let page = |n| {
        let request = Request {
            page: n,
            ..request.clone()
        };
        String::from_utf8(render_content(
            &theme,
            &site,
            &request,
            b"<!-- wp:post-content /-->",
        ))
        .unwrap()
    };
    assert!(page(2).contains("<p>Two</p>") && !page(2).contains("Three"));
    // A page past the last shows the last, as WordPress does.
    assert!(page(9).contains("<p>Three</p>"));
}
