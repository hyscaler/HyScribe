// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{PostQuery, Term};
use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use super::*;
use crate::site::{Request, Site};
use crate::template::render_content;
use crate::theme::twentytwentyfive;

/// One in-memory post of the golden script, seen from `home`.
struct Fixture {
    post: Post,
    home: Vec<u8>,
    siteurl: Vec<u8>,
    smilies: bool,
    migration: Option<Vec<u8>>,
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
            "https_migration_required" => self.migration.clone(),
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
        excerpt: row
            .get_str("post_excerpt")
            .map_or_else(Vec::new, Value::to_php_string),
        password: text("post_password"),
        status: "publish".into(),
        post_type: "post".into(),
        ..Post::default()
    };
    // The script sets `$more` in a singular request, as for the post asked for.
    let singular = state.get_str("singular") == Some(&Value::Bool(true));
    let request = Request {
        singular,
        queried_object_id: if singular { post.id } else { 0 },
        page: state.get_str("page").map_or(0, |p| p.to_int() as u64),
        post: Some(post.id),
        ..Request::default()
    };
    let site = Fixture {
        post,
        home: state.get_str("home").unwrap().to_php_string(),
        siteurl: state.get_str("siteurl").unwrap().to_php_string(),
        smilies: true,
        migration: None,
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
            "excerpt" => {
                let (markup, state, row, php) = (field(), field(), field(), field());
                let (site, mut request) = case(&state, &row);
                let state = json_decode(&state).unwrap();
                // get_the_excerpt() sets up its post itself: the request's is post 1.
                if request.singular {
                    request.queried_object_id = 1;
                }
                if state.get_str("post") != Some(&Value::Bool(true)) {
                    request.post = None;
                }
                let mut renderer = Renderer::new(&theme, &site, &request);
                renderer.elements_id = state.get_str("elements").unwrap().to_int() as u64;
                (renderer.render(&markup), php, markup)
            }
            "teaser" => {
                let (state, row, php) = (field(), field(), field());
                let (site, request) = case(&state, &row);
                let renderer = Renderer::new(&theme, &site, &request);
                (get_the_content(&renderer, &site.post, true), php, row)
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
    assert!(count >= 146);
}

/// `reference/insecure-home-url/golden.sh`: its content, then each home,
/// siteurl and `https_migration_required` state with the result.
#[test]
fn insecure_home_urls_match_php() {
    let mut lines = include_str!("../../tests/golden/insecure-home-url.tsv").lines();
    let content = unhex(lines.next().unwrap());
    let mut count = 0;
    for line in lines {
        let [home, siteurl, stored, set, expected] =
            <[&str; 5]>::try_from(line.split('\t').collect::<Vec<_>>()).unwrap();
        let site = Fixture {
            post: Post::default(),
            home: unhex(home),
            siteurl: unhex(siteurl),
            smilies: false,
            migration: (set == "1").then(|| unhex(stored)),
        };
        assert_eq!(
            String::from_utf8_lossy(&replace_insecure_home_url(&site, &content)),
            String::from_utf8_lossy(&if expected == "=" {
                content.clone()
            } else {
                unhex(expected)
            }),
            "home {:?} siteurl {:?} option {:?}",
            String::from_utf8_lossy(&site.home),
            String::from_utf8_lossy(&site.siteurl),
            site.migration.as_deref().map(String::from_utf8_lossy),
        );
        count += 1;
    }
    assert_eq!(count, 384);
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
        r#"<section class="entry-content wp-block-post-content is-layout-flow wp-block-post-content-is-layout-flow">"#.to_string()
            + "<p>\u{1f642}</p>\n</section>"
    );
    site.smilies = false;
    assert!(html(&site, &request, "<!-- wp:post-content /-->").contains("<p>:)</p>"));
    site.smilies = true;
    // Unterminated more tags and delimiters are left as they are; without
    // blocks, wpautop() gives the tag a paragraph of its own.
    site.post.content = b"<p>a</p><!--more".to_vec();
    assert!(
        html(&site, &request, "<!-- wp:post-content /-->").contains("<p>a</p>\n<p><!--more\n</p>")
    );
    site.post.content = b"<p>a</p><!--more--><!-- wp:more".to_vec();
    assert!(
        html(&site, &request, "<!-- wp:post-content /-->")
            .contains(r#"<span id="more-5"></span><!-- wp:more"#)
    );
}

/// A protected post's password form, in a block theme, enqueues the button
/// block's stylesheet, as PHP's does on the Theme Test Data's pages.
#[test]
fn password_forms_enqueue_the_button_style() {
    let theme = twentytwentyfive();
    for (password, enqueued) in [("enter", true), ("", false)] {
        let row = format!(
            r#"{{"ID":5,"post_title":"T","post_content":"<p>x</p>","post_password":"{password}"}}"#
        );
        let (site, request) = case(
            br#"{"singular":true,"home":"","siteurl":""}"#,
            row.as_bytes(),
        );
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.render(b"<!-- wp:post-content /-->");
        let styles = &renderer.assets.styles;
        assert_eq!(styles.iter().any(|s| s == "wp-block-button"), enqueued);
    }
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
