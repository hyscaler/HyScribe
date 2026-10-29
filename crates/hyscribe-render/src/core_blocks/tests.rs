// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use super::*;
use crate::blocks::parse_blocks;
use crate::theme::twentytwentyfive;

/// The options, home URL and post one golden case rendered with.
#[derive(Default)]
struct Fixture {
    options: HashMap<String, Vec<u8>>,
    home: Vec<u8>,
    post: Option<Post>,
    terms: HashMap<String, Vec<hyscribe_model::Term>>,
}

impl Site for Fixture {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.options.get(name).cloned()
    }

    fn home_url(&self) -> Vec<u8> {
        self.home.clone()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.post.clone().filter(|post| post.id == id)
    }

    fn post_terms(&self, _: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.terms.get(taxonomy).cloned().unwrap_or_default()
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &hyscribe_model::PostQuery) -> Vec<Post> {
        Vec::new()
    }

    /// The fixture database's one user.
    fn user(&self, id: u64) -> Option<hyscribe_model::User> {
        (id == 1).then(|| hyscribe_model::User {
            id,
            display_name: b"admin".to_vec(),
            nicename: b"admin".to_vec(),
            ..Default::default()
        })
    }
}

fn object(json: &[u8]) -> Value {
    json_decode(json).unwrap()
}

fn text(value: &Value, name: &str) -> String {
    String::from_utf8(value.get_str(name).unwrap().to_php_string()).unwrap()
}

/// A post from `get_post( $id, ARRAY_A )`.
fn post(row: &Value) -> Post {
    Post {
        id: row.get_str("ID").unwrap().to_float() as u64,
        author: row.get_str("post_author").unwrap().to_int() as u64,
        comment_count: row.get_str("comment_count").unwrap().to_int(),
        comment_status: text(row, "comment_status"),
        title: row.get_str("post_title").unwrap().to_php_string(),
        status: text(row, "post_status"),
        password: row.get_str("post_password").unwrap().to_php_string(),
        post_type: text(row, "post_type"),
        date: text(row, "post_date"),
        modified: text(row, "post_modified"),
        ..Post::default()
    }
}

fn render_markup(context: &Context<'_>, markup: &[u8]) -> Vec<u8> {
    let block = &parse_blocks(markup)[0];
    render(
        context,
        block.name.as_deref().unwrap(),
        block.attrs.as_deref().unwrap(),
        &block.inner_html,
    )
    .unwrap()
}

/// The `site` and `post` cases: what the block rendered with, and PHP's
/// output.
fn case(kind: &str, state: &Value, data: &Value) -> (Fixture, Request, Option<u64>) {
    let mut site = Fixture::default();
    let mut request = Request::default();
    if kind == "site" {
        for name in ["blogname", "blogdescription"] {
            site.options
                .insert(name.into(), data.get_str(name).unwrap().to_php_string());
        }
        site.options.insert(
            "page_for_posts".into(),
            state.get_str("page_for_posts").unwrap().to_php_string(),
        );
        site.home = data.get_str("home").unwrap().to_php_string();
        let flag = |name| state.get_str(name) == Some(&Value::Bool(true));
        request = Request {
            front_page: flag("front_page"),
            home: flag("home"),
            paged: if flag("paged") { 2 } else { 0 },
            queried_object_id: state.get_str("queried_object_id").unwrap().to_float() as u64,
            ..Request::default()
        };
        return (site, request, None);
    }
    site.home = state.get_str("home").unwrap().to_php_string();
    site.options.insert(
        "date_format".into(),
        state.get_str("date_format").unwrap().to_php_string(),
    );
    site.options.insert("gmt_offset".into(), b"0".to_vec());
    site.post = (*data != Value::Null).then(|| post(data));
    let id = state.get_str("post").unwrap().to_float() as u64;
    (site, request, (id != 0).then_some(id))
}

/// Every case of `reference/core-blocks/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let mut fields = line.split('\t');
        let kind = fields.next().unwrap();
        let mut field = || unhex(fields.next().unwrap());
        if kind == "class"
            || kind == "query"
            || kind == "body"
            || kind == "content"
            || kind == "teaser"
            || kind == "terms"
            || kind == "adjacent"
            || kind == "comments"
            || kind == "pagination"
        {
            continue;
        }
        if kind == "diff" {
            let number = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap().parse::<i64>().unwrap();
            let (from, to, php) = (number(field()), number(field()), field());
            assert_eq!(
                date::human_time_diff(from, to).into_bytes(),
                php,
                "{from} {to}"
            );
            continue;
        }
        let (markup, state, data, php) = (field(), object(&field()), object(&field()), field());
        let (site, request, post_id) = case(kind, &state, &data);
        let context = Context {
            theme: &theme,
            site: &site,
            request: &request,
            post_id,
            query: None,
            comment: None,
            loading: Default::default(),
        };
        let ours = render_markup(&context, &markup);
        if ours != php {
            failures.push(format!(
                "{}\n  {state:?}\n  php:  {}\n  ours: {}",
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
        failures[..failures.len().min(6)].join("\n")
    );
    assert!(golden.lines().count() > 700);
}

/// `human-diff` measures from the request time, which PHP's golden cannot
/// fix, so these use `human_time_diff()` as checked above.
#[test]
fn relative_dates() {
    let theme = twentytwentyfive();
    let site = Fixture {
        post: Some(Post {
            id: 7,
            status: "publish".into(),
            post_type: "post".into(),
            date: "2026-10-27 10:00:00".into(),
            modified: "2026-10-27 10:00:00".into(),
            ..Post::default()
        }),
        ..Fixture::default()
    };
    let requests: Vec<Request> = [7200, -120, 0]
        .map(|delta| Request {
            now: date::stored_unix("2026-10-27 10:00:00", Zone::Utc) + delta,
            ..Request::default()
        })
        .into();
    let at = |now: i64| Context {
        theme: &theme,
        site: &site,
        request: requests.iter().find(|r| r.now == now).unwrap(),
        post_id: Some(7),
        query: None,
        comment: None,
        loading: Default::default(),
    };
    let markup = br#"<!-- wp:post-date {"format":"human-diff"} /-->"#;
    let posted = date::stored_unix("2026-10-27 10:00:00", Zone::Utc);
    let html = |now| String::from_utf8(render_markup(&at(now), markup)).unwrap();
    assert!(html(posted + 7200).contains(">2 hours ago</time>"));
    assert!(html(posted - 120).contains(">2 minutes from now</time>"));
    assert!(html(posted).contains(">1 second ago</time>"));
    let unreadable = br#"<!-- wp:post-date {"datetime":"soon","format":"human-diff"} /-->"#;
    let html = String::from_utf8(render_markup(&at(posted), unreadable)).unwrap();
    assert!(html.contains(">57 years ago</time>"), "{html}");
}

#[test]
fn blocks_without_a_callback_render_their_saved_markup() {
    let theme = twentytwentyfive();
    let site = Fixture::default();
    let context = Context {
        theme: &theme,
        site: &site,
        request: &Request::default(),
        post_id: None,
        query: None,
        comment: None,
        loading: Default::default(),
    };
    assert_eq!(render(&context, "core/paragraph", &[], b""), None);
    assert_eq!(
        render(&context, "core/site-title", &[], b""),
        Some(Vec::new())
    );
}

#[test]
fn bindings_and_the_front_page() {
    let theme = twentytwentyfive();
    let mut site = Fixture {
        home: b"http://example.test".to_vec(),
        post: Some(Post {
            id: 9,
            title: b"Front".to_vec(),
            status: "publish".into(),
            post_type: "page".into(),
            date: "2026-10-27 10:00:00".into(),
            modified: "2026-10-27 10:00:00".into(),
            ..Post::default()
        }),
        ..Fixture::default()
    };
    site.options
        .insert("show_on_front".into(), b"page".to_vec());
    site.options.insert("page_on_front".into(), b"9".to_vec());
    let context = Context {
        theme: &theme,
        site: &site,
        request: &Request::default(),
        post_id: Some(9),
        query: None,
        comment: None,
        loading: Default::default(),
    };
    let html =
        |markup: &str| String::from_utf8(render_markup(&context, markup.as_bytes())).unwrap();
    assert!(
        html(r#"<!-- wp:post-title {"isLink":true} /-->"#)
            .contains(r#"<a href="http://example.test/" target="_self" >Front</a>"#)
    );
    let bound = |source: &str, field: &str| {
        html(&format!(
            r#"<!-- wp:post-date {{"metadata":{{"bindings":{{"datetime":{{"source":"{source}","args":{{"field":"{field}"}}}}}}}}}} /-->"#
        ))
    };
    assert_eq!(bound("core/post-data", "title"), "");
    // A binding without arguments leaves the legacy date in place.
    let sourced = html(
        r#"<!-- wp:post-date {"metadata":{"bindings":{"datetime":{"source":"core/post-data"}}}} /-->"#,
    );
    assert!(
        sourced.contains(r#"<time datetime="2026-10-27T10:00:00+00:00">"#),
        "{sourced}"
    );
    assert_eq!(bound("core/post-meta", "date"), "");
    // PHP binds the link as the date, which strtotime() cannot read.
    assert!(
        bound("core/post-data", "link")
            .contains(r#"<time datetime="http://example.test/"></time>"#)
    );
}

/// The `terms` cases: the post terms block over `golden.php`'s term sets.
#[test]
fn post_terms_match_php() {
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let theme = twentytwentyfive();
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("terms\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, state, php) = (field(), object(&field()), field());
        let mut site = Fixture {
            home: state.get_str("home").unwrap().to_php_string(),
            ..Fixture::default()
        };
        for (taxonomy, terms) in state
            .get_str("terms")
            .unwrap()
            .as_array()
            .unwrap_or_default()
        {
            let terms = terms
                .as_array()
                .unwrap()
                .iter()
                .map(|(_, term)| {
                    let slug = term.get(&Key::Int(1)).unwrap().to_php_string();
                    hyscribe_model::Term {
                        id: term.get(&Key::Int(0)).unwrap().to_int() as u64,
                        name: slug.clone(),
                        slug,
                    }
                })
                .collect();
            let Key::String(taxonomy) = taxonomy else {
                panic!()
            };
            site.terms
                .insert(String::from_utf8(taxonomy.clone()).unwrap(), terms);
        }
        let request = Request::default();
        let context = Context {
            theme: &theme,
            site: &site,
            request: &request,
            post_id: (state.get_str("context") == Some(&Value::Bool(true))).then_some(1001),
            query: None,
            comment: None,
            loading: Default::default(),
        };
        assert_eq!(
            String::from_utf8_lossy(&render_markup(&context, &markup)),
            String::from_utf8_lossy(&php),
            "{} {:?}",
            String::from_utf8_lossy(&markup),
            state
        );
        count += 1;
    }
    assert!(count > 70);
}

/// The fixture database, with one more post from the golden in memory.
struct WithPost {
    base: crate::fixture::DefaultInstall,
    post: Option<Post>,
    date_format: Vec<u8>,
    /// What the adjacent query is rewritten to give, when it is.
    forced: Option<Post>,
}

impl Site for WithPost {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "date_format" => Some(self.date_format.clone()),
            _ => self.base.option(name),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        let own = self
            .post
            .iter()
            .chain(&self.forced)
            .find(|post| post.id == id);
        own.cloned().or_else(|| self.base.post(id))
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        match &self.forced {
            Some(_) => vec![hyscribe_model::Term {
                id: 1,
                ..hyscribe_model::Term::default()
            }],
            None => self.base.post_terms(post_id, taxonomy),
        }
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(post_id, key)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<Post> {
        self.base.query(query)
    }
    fn adjacent_post(
        &self,
        post: &Post,
        previous: bool,
        terms: Option<(&str, &[u64])>,
    ) -> Option<Post> {
        match &self.forced {
            Some(forced) => Some(forced.clone()),
            None => self.base.adjacent_post(post, previous, terms),
        }
    }
}

/// The `adjacent` cases: the post navigation link around the fixture's post.
#[test]
fn post_navigation_links_match_php() {
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let theme = twentytwentyfive();
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("adjacent\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, state, row, php) = (field(), object(&field()), object(&field()), field());
        let full = |row: &Value| Post {
            date: text(row, "post_date"),
            ..post(row)
        };
        let site = WithPost {
            base: crate::fixture::DefaultInstall {
                home: state.get_str("home").unwrap().to_php_string(),
            },
            post: (row != Value::Null).then(|| full(&row)),
            date_format: state.get_str("date_format").unwrap().to_php_string(),
            forced: state
                .get_str("forced")
                .filter(|v| **v != Value::Null)
                .map(full),
        };
        let request = Request {
            singular: state.get_str("singular") == Some(&Value::Bool(true)),
            ..Request::default()
        };
        let context = Context {
            theme: &theme,
            site: &site,
            request: &request,
            post_id: Some(site.post.as_ref().map_or(9999, |post| post.id)),
            query: None,
            comment: None,
            loading: Default::default(),
        };
        assert_eq!(
            String::from_utf8_lossy(&render_markup(&context, &markup)),
            String::from_utf8_lossy(&php),
            "{} {:?}",
            String::from_utf8_lossy(&markup),
            site.post.as_ref().map(|post| post.id)
        );
        count += 1;
    }
    assert!(count > 200);
}

/// The `comments` cases: the comments wrapper and title.
#[test]
fn comments_match_php() {
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let theme = twentytwentyfive();
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("comments\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, row, php) = (field(), object(&field()), field());
        let site = Fixture {
            post: (row != Value::Null).then(|| post(&row)),
            ..Fixture::default()
        };
        let request = Request::default();
        let context = Context {
            theme: &theme,
            site: &site,
            request: &request,
            post_id: site.post.as_ref().map(|post| post.id),
            query: None,
            comment: None,
            loading: Default::default(),
        };
        assert_eq!(
            String::from_utf8_lossy(&render_markup(&context, &markup)),
            String::from_utf8_lossy(&php),
            "{} {:?}",
            String::from_utf8_lossy(&markup),
            site.post.as_ref().map(|post| post.id)
        );
        count += 1;
    }
    assert_eq!(count, 56);
}

/// The legacy comments block runs `comments_template()`, which is not
/// ported; it renders nothing.
#[test]
fn legacy_comments_are_not_ported() {
    let theme = twentytwentyfive();
    let site = Fixture {
        post: Some(Post {
            id: 1,
            comment_status: "open".into(),
            ..Post::default()
        }),
        ..Fixture::default()
    };
    let request = Request::default();
    let context = Context {
        theme: &theme,
        site: &site,
        request: &request,
        post_id: Some(1),
        query: None,
        comment: None,
        loading: Default::default(),
    };
    for markup in [
        &b"<!-- wp:comments {\"legacy\":true} /-->"[..],
        b"<!-- wp:post-comments --><div>saved</div><!-- /wp:post-comments -->",
    ] {
        assert_eq!(render_markup(&context, markup), b"");
    }
}

/// Without an `avatar_default` option `get_option()` falls back to
/// `mystery`, and without `avatar_rating` there is no rating.
#[test]
fn avatar_defaults_without_options() {
    let theme = twentytwentyfive();
    let site = Fixture {
        options: HashMap::from([("show_avatars".to_owned(), b"1".to_vec())]),
        ..Fixture::default()
    };
    let request = Request::default();
    let context = Context {
        theme: &theme,
        site: &site,
        request: &request,
        post_id: None,
        query: None,
        comment: None,
        loading: Default::default(),
    };
    assert_eq!(
        String::from_utf8(render_markup(
            &context,
            br#"<!-- wp:avatar {"userId":1} /-->"#
        ))
        .unwrap(),
        "<div class=\"wp-block-avatar\"><img alt='admin Avatar' \
         src='https://secure.gravatar.com/avatar/?s=96&#038;d=mm' \
         srcset='https://secure.gravatar.com/avatar/?s=192&#038;d=mm 2x' \
         class='avatar avatar-96 photo avatar-default wp-block-avatar__image' height='96' \
         width='96' loading='lazy' decoding='async'/></div>"
    );
}

/// A site without comment data has no comment to reply to, even for a
/// post numbered 0, so the form keeps its plain title and a zero parent.
#[test]
fn comment_form_without_comment_data() {
    let theme = twentytwentyfive();
    let site = Fixture {
        post: Some(Post {
            id: 0,
            status: "publish".into(),
            post_type: "post".into(),
            comment_status: "open".into(),
            ..Post::default()
        }),
        ..Fixture::default()
    };
    let request = Request {
        get: hyscribe_phpser::parse_str(b"replytocom=1"),
        ..Request::default()
    };
    let context = Context {
        theme: &theme,
        site: &site,
        request: &request,
        post_id: Some(0),
        query: None,
        comment: None,
        loading: Default::default(),
    };
    let html =
        String::from_utf8(render_markup(&context, b"<!-- wp:post-comments-form /-->")).unwrap();
    assert!(html.contains(">Leave a Reply</h3>"), "{html}");
    assert!(
        html.contains("name='comment_parent' id='comment_parent' value='0'"),
        "{html}"
    );
}
