// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Post, PostQuery, Term, User};
use hyscribe_phpser::{json_decode, parse_str};
use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::media::Loading;
use crate::site::{Commenter, Request, Site};
use crate::theme::twentytwentyfive;

/// The fixture database with `golden.php`'s comments and options.
struct WithComments {
    base: DefaultInstall,
    options: Value,
    comments: Vec<Comment>,
    users: Vec<User>,
    password: bool,
    closed: bool,
    siteurl: Vec<u8>,
    /// The held comments.
    held: Vec<Comment>,
    /// PHP's `wp_hash()` of each date.
    hashes: Value,
}

impl Site for WithComments {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        if name == "siteurl" {
            return Some(self.siteurl.clone());
        }
        match self.options.get_str(name) {
            Some(value) => Some(value.to_php_string()),
            None => self.base.option(name),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        let password = if self.password {
            b"pw".to_vec()
        } else {
            Vec::new()
        };
        // PHP counts the approved comments of a post, notes aside.
        let count = self
            .comments
            .iter()
            .filter(|c| c.post_id == id && c.comment_type != b"note")
            .count();
        self.base.post(id).map(|post| Post {
            password,
            comment_count: i64::try_from(count).unwrap(),
            // The golden opens comments on every post, or closes them all.
            comment_status: if self.closed { "closed" } else { "open" }.into(),
            ..post
        })
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.base.post_terms(post_id, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(post_id, key)
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.base.query(query)
    }
    fn comments(&self, post_id: u64) -> Vec<Comment> {
        self.comments
            .iter()
            .filter(|c| c.post_id == post_id && c.comment_type != b"note")
            .cloned()
            .collect()
    }
    fn older_comments(&self, post_id: u64, date_gmt: &str) -> u64 {
        let older = |c: &&Comment| c.post_id == post_id && c.parent == 0 && *c.date_gmt < *date_gmt;
        self.comments.iter().filter(older).count() as u64
    }
    fn comment(&self, id: u64) -> Option<Comment> {
        self.comments.iter().find(|c| c.id == id).cloned()
    }
    fn held_comment(&self, id: u64) -> Option<Comment> {
        self.held.iter().find(|c| c.id == id).cloned()
    }
    fn held_comments(&self, post_id: u64, email: &[u8], only: Option<i64>) -> Vec<Comment> {
        let theirs = |c: &&Comment| {
            c.post_id == post_id
                && c.comment_type != b"note"
                && c.author_email.eq_ignore_ascii_case(email)
                && only.is_none_or(|only| i64::try_from(c.id) == Ok(only))
        };
        self.held.iter().filter(theirs).cloned().collect()
    }
    fn wp_hash(&self, data: &[u8]) -> Option<String> {
        let hash = self
            .hashes
            .get(&hyscribe_phpser::Key::from_php_string(data.to_vec()));
        hash.map(|hash| String::from_utf8_lossy(&hash.to_php_string()).into_owned())
    }
    fn user(&self, id: u64) -> Option<User> {
        self.users.iter().find(|user| user.id == id).cloned()
    }
}

/// The comments of `status` among a golden's rows, notes included, in the
/// order the comment template lists them.
fn of_status(rows: &Value, status: &[u8]) -> Vec<Comment> {
    let text = |row: &Value, name| row.get_str(name).unwrap().to_php_string();
    let number = |row: &Value, name| row.get_str(name).unwrap().to_int() as u64;
    let mut comments: Vec<Comment> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|(_, row)| row)
        .filter(|row| text(row, "comment_approved") == status)
        .map(|row| Comment {
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
        .collect();
    comments.sort_by(|a, b| (&a.date_gmt, a.id).cmp(&(&b.date_gmt, b.id)));
    comments
}

fn users(rows: &Value) -> Vec<User> {
    let text = |row: &Value, name| row.get_str(name).unwrap().to_php_string();
    rows.as_array()
        .unwrap()
        .iter()
        .map(|(_, row)| User {
            id: row.get_str("ID").unwrap().to_int() as u64,
            display_name: text(row, "display_name"),
            nicename: text(row, "user_nicename"),
            login: text(row, "user_login"),
            email: text(row, "user_email"),
            url: Vec::new(),
            first_name: text(row, "first_name"),
            last_name: text(row, "last_name"),
            description: Vec::new(),
        })
        .collect()
}

/// Every case of `reference/comments/golden.sh`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/comments.tsv");
    let failures = differences(golden);
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(3)].join("\n")
    );
    assert_eq!(golden.lines().count(), 2365);
}

/// A held comment shows only to its author, from the link
/// `wp-comments-post.php` sends them to, while it is new, or whatever its
/// age to the commenter cookies' email, and the form and comment blocks
/// show the commenter's cookies (`reference/comments/held-golden.php`).
#[test]
fn held_comments_preview_as_php() {
    let golden = include_str!("../../tests/golden/held-comments.tsv");
    let failures = differences(golden);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 659);
}

/// A site without comment data has no held comments for anyone.
#[test]
fn a_site_without_comments_holds_none() {
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    assert!(site.held_comments(1, b"held@example.test", None).is_empty());
}

/// A site without notes counts its comments' top level ones from before
/// a date, as `get_page_of_comment()` does.
#[test]
fn older_comments_count_the_top_level() {
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    // The fixture's one comment is from 2026-10-27 08:08:15.
    assert_eq!(site.older_comments(1, "2026-10-27 08:08:16"), 1);
    assert_eq!(site.older_comments(1, "2026-10-27 08:08:15"), 0);
}

/// On paged comments, a reply whose thread has no top, as its parent is
/// missing or the parents loop, links to no page, where PHP fails.
#[test]
fn comments_without_a_thread_top_link_to_no_page() {
    let comment = |id, parent| Comment {
        id,
        post_id: 1,
        parent,
        ..Comment::default()
    };
    let comments = vec![comment(10, 11), comment(11, 10), comment(12, 99)];
    let site = linked_site(b"page_comments=1&comments_per_page=1", comments);
    for (id, parent) in [(10, 11), (12, 99)] {
        assert_eq!(
            link_of(&site, &comment(id, parent)),
            format!("http://127.0.0.1:8081/?p=1#comment-{id}")
        );
    }
}

/// With threads one deep, a reply's page is its own, from its date; under
/// pretty permalinks a link without a page is the post's, as in PHP.
#[test]
fn comment_links_on_shallow_threads_and_pretty_links() {
    let comment = |id, parent, date: &str| Comment {
        id,
        post_id: 1,
        parent,
        date_gmt: date.into(),
        ..Comment::default()
    };
    let (a, b, c) = (
        comment(10, 0, "2026-10-27 09:01:00"),
        comment(11, 10, "2026-10-27 09:03:00"),
        comment(12, 0, "2026-10-27 09:02:00"),
    );
    let options = b"page_comments=1&comments_per_page=1&thread_comments_depth=1";
    let site = linked_site(options, vec![a.clone(), b.clone(), c]);
    assert_eq!(
        link_of(&site, &b),
        "http://127.0.0.1:8081/?p=1&cpage=3#comment-11"
    );
    let site = linked_site(b"permalink_structure=/%postname%/", vec![a.clone()]);
    assert_eq!(
        link_of(&site, &a),
        "http://127.0.0.1:8081/hello-world/#comment-10"
    );
}

/// The fixture with `options` and `comments`, all approved.
fn linked_site(options: &[u8], comments: Vec<Comment>) -> WithComments {
    WithComments {
        base: DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        options: parse_str(options),
        comments,
        users: Vec::new(),
        password: false,
        closed: false,
        siteurl: b"http://127.0.0.1:8081".to_vec(),
        held: Vec::new(),
        hashes: Value::Null,
    }
}

/// `get_comment_link( $comment )` on `site`.
fn link_of(site: &WithComments, comment: &Comment) -> String {
    let (theme, request) = (twentytwentyfive(), Request::default());
    let renderer = Renderer::new(&theme, site, &request);
    String::from_utf8(crate::core_blocks::comment_link(&renderer.context, comment)).unwrap()
}

/// Where a golden's cases differ from the renderer's.
fn differences(golden: &str) -> Vec<String> {
    let theme = twentytwentyfive();
    let (shared, cases) = golden.split_once('\n').unwrap();
    let shared = json_decode(&unhex(shared)).unwrap();
    let rows = shared.get_str("comments").unwrap();
    let (comments, held) = (of_status(rows, b"1"), of_status(rows, b"0"));
    let users = shared.get_str("users").map(users).unwrap_or_default();
    let siteurl = shared
        .get_str("siteurl")
        .map_or(b"http://127.0.0.1:8081".to_vec(), Value::to_php_string);
    let mut failures = Vec::new();
    for line in cases.lines() {
        let mut fields = line.split('\t').map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, state, php) = (field(), json_decode(&field()).unwrap(), field());
        let site = WithComments {
            base: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            options: state.get_str("options").unwrap().clone(),
            comments: comments.clone(),
            users: users.clone(),
            password: state.get_str("password") == Some(&Value::Bool(true)),
            closed: state.get_str("closed") == Some(&Value::Bool(true)),
            siteurl: siteurl.clone(),
            held: held.clone(),
            hashes: state.get_str("hashes").cloned().unwrap_or(Value::Null),
        };
        let uri = state
            .get_str("uri")
            .map_or(b"/".to_vec(), Value::to_php_string);
        let request = Request {
            singular: state.get_str("singular") == Some(&Value::Bool(true)),
            now: state.get_str("now").unwrap().to_int(),
            post: state.get_str("global").map(|id| id.to_int() as u64),
            uri: uri.clone(),
            get: parse_str(uri.splitn(2, |&b| b == b'?').nth(1).unwrap_or_default()),
            commenter: state
                .get_str("commenter")
                .map_or_else(Commenter::default, |c| {
                    let text = |name| {
                        c.get_str(name)
                            .map(Value::to_php_string)
                            .unwrap_or_default()
                    };
                    Commenter {
                        author: text("comment_author"),
                        email: text("comment_author_email"),
                        url: text("comment_author_url"),
                    }
                }),
            // `cpage` from a pretty path is recorded on its own.
            vars: state.get_str("vars").cloned().unwrap_or_else(|| {
                parse_str(uri.splitn(2, |&b| b == b'?').nth(1).unwrap_or_default())
            }),
            posts: state.get_str("main").map_or(Vec::new(), |ids| {
                ids.as_array()
                    .unwrap()
                    .iter()
                    .map(|(_, id)| site.post(id.to_int() as u64).unwrap())
                    .collect()
            }),
            ..Request::default()
        };
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.elements_id = state.get_str("elements").unwrap().to_int() as u64;
        if state.get_str("loop") == Some(&Value::Bool(true)) {
            renderer.context.loading = Loading::in_the_loop(true);
        }
        let post = state.get_str("post").unwrap().to_int() as u64;
        renderer.context.post_id = (post != 0).then_some(post);
        let block = &crate::blocks::parse_blocks(&markup)[0];
        let ours = [renderer.block(block), renderer.block(block)].concat();
        if ours != php {
            // Where they part, with some context.
            let at = ours.iter().zip(&php).take_while(|(a, b)| a == b).count();
            let from = at.saturating_sub(80);
            let around = |html: &[u8]| {
                String::from_utf8_lossy(
                    html.get(from..html.len().min(at + 200)).unwrap_or_default(),
                )
                .into_owned()
            };
            failures.push(format!(
                "{} {:?} {:?} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                state.get_str("options"),
                state.get_str("cookies"),
                String::from_utf8_lossy(&uri),
                around(&php),
                around(&ours)
            ));
        }
    }
    failures
}

/// A thread of replies far past `thread_comments_depth` renders on a 2 MiB
/// worker stack, in time linear in its length: replies deeper than the
/// limit go on in the item of the last comment that nests them, as in PHP,
/// without a stack frame each.
#[test]
fn long_reply_chains_render_without_recursing() {
    const REPLIES: u64 = 10_000;
    let comments: Vec<Comment> = (1..=REPLIES)
        .map(|id| Comment {
            id,
            post_id: 1,
            parent: id - 1,
            date_gmt: "2026-11-12 00:00:00".into(),
            ..Comment::default()
        })
        .collect();
    let html = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let site = WithComments {
                base: DefaultInstall {
                    home: b"http://example.test".to_vec(),
                },
                options: json_decode(br#"{"thread_comments":"1","thread_comments_depth":"5"}"#)
                    .unwrap(),
                comments,
                users: Vec::new(),
                password: false,
                closed: false,
                siteurl: b"http://example.test".to_vec(),
                held: Vec::new(),
                hashes: Value::Null,
            };
            let request = Request {
                post: Some(1),
                ..Request::default()
            };
            let theme = twentytwentyfive();
            let mut renderer = Renderer::new(&theme, &site, &request);
            let markup = b"<!-- wp:comment-template --><!-- wp:comment-reply-link /--><!-- /wp:comment-template -->";
            let block = &crate::blocks::parse_blocks(markup)[0];
            String::from_utf8(renderer.block(block)).unwrap()
        })
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(html.matches("<li id=\"comment-").count(), REPLIES as usize);
    assert_eq!(html.matches("</li>").count(), REPLIES as usize);
    // Four levels open below the first, and only those comments can still
    // be replied to.
    assert_eq!(html.matches("<ol>").count(), 4);
    assert_eq!(html.matches("wp-block-comment-reply-link").count(), 4);
    // The replies from the fifth level on close inside its item, then the
    // four levels above it and the template's own list close.
    let end = ["</li>".repeat(REPLIES as usize - 4), "</ol></li>".repeat(4)].concat() + "</ol>";
    assert!(html.ends_with(&end));
}

/// No held comment is previewed for an ID no comment has, a hash that is
/// not a string, or on a site without the salts to sign it.
#[test]
fn a_preview_needs_a_signed_held_comment() {
    let held = Comment {
        id: 3,
        post_id: 1,
        author_email: b"held@example.test".to_vec(),
        date_gmt: "2026-11-15 00:00:00".into(),
        ..Comment::default()
    };
    let site = WithComments {
        base: DefaultInstall {
            home: b"http://example.test".to_vec(),
        },
        options: Value::Null,
        comments: Vec::new(),
        users: Vec::new(),
        password: false,
        closed: false,
        siteurl: b"http://example.test".to_vec(),
        held: vec![held],
        hashes: Value::Null,
    };
    for query in [
        "unapproved=-3&moderation-hash=x",
        "unapproved=3&moderation-hash[]=x",
        "unapproved=3&moderation-hash=x",
    ] {
        let request = Request {
            get: parse_str(query.as_bytes()),
            ..Request::default()
        };
        assert!(unapproved(&site, &request).is_none(), "{query}");
    }
    assert!(site.base.held_comment(3).is_none());
}

/// A signed held comment shows for ten minutes after its date, and only on
/// its own post.
#[test]
fn a_preview_lasts_ten_minutes_on_its_post() {
    let date = "2026-11-15 00:00:00";
    let held = |id, post_id| Comment {
        id,
        post_id,
        author_email: b"held@example.test".to_vec(),
        date_gmt: date.into(),
        ..Comment::default()
    };
    let site = WithComments {
        base: DefaultInstall {
            home: b"http://example.test".to_vec(),
        },
        options: Value::Null,
        comments: Vec::new(),
        users: Vec::new(),
        password: false,
        closed: false,
        siteurl: b"http://example.test".to_vec(),
        held: vec![held(3, 1), held(4, 2)],
        hashes: json_decode(br#"{"2026-11-15 00:00:00":"signed"}"#).unwrap(),
    };
    let expiry = crate::date::stored_unix(date, crate::date::Zone::Utc) + 600;
    let request = |id: u64, now| Request {
        get: parse_str(format!("unapproved={id}&moderation-hash=signed").as_bytes()),
        now,
        ..Request::default()
    };
    let shown = |id, now| post_comments(&site, &request(id, now), 1).0.len();
    assert_eq!(shown(3, expiry - 1), 1);
    assert_eq!(shown(3, expiry), 0);
    assert_eq!(shown(4, expiry - 1), 0);
}

/// An empty `cpage` names no page: with several comment pages the legacy
/// template sets the page, as it does for a request without one.
#[test]
fn an_empty_cpage_names_no_page() {
    let comments = (1..=4)
        .map(|id| Comment {
            id,
            post_id: 1,
            date_gmt: format!("2026-11-12 00:00:0{id}"),
            ..Comment::default()
        })
        .collect();
    let site = linked_site(b"page_comments=1&comments_per_page=2", comments);
    let theme = twentytwentyfive();
    let post = site.post(1).unwrap();
    for vars in ["", "cpage="] {
        let request = Request {
            singular: true,
            vars: parse_str(vars.as_bytes()),
            ..Request::default()
        };
        let renderer = Renderer::new(&theme, &site, &request);
        let page = crate::core_blocks::legacy_cpage(&renderer.context, &post);
        assert_eq!(page, Some(2), "{vars}");
    }
}

/// Only a singular request with no `cpage` of its own gets the one the
/// legacy template sets.
#[test]
fn only_a_singular_request_without_a_cpage_has_one_set() {
    let comments = (1..=4)
        .map(|id| Comment {
            id,
            post_id: 1,
            date_gmt: format!("2026-11-12 00:00:0{id}"),
            ..Comment::default()
        })
        .collect();
    let site = linked_site(b"page_comments=1&comments_per_page=2", comments);
    let theme = twentytwentyfive();
    let post = site.post(1).unwrap();
    for (singular, vars, expected) in [
        (true, "", Some(2)),
        (false, "", None),
        (true, "cpage=1", None),
        (false, "cpage=1", None),
    ] {
        let request = Request {
            singular,
            vars: parse_str(vars.as_bytes()),
            ..Request::default()
        };
        let renderer = Renderer::new(&theme, &site, &request);
        let page = crate::core_blocks::legacy_cpage(&renderer.context, &post);
        assert_eq!(page, expected, "{singular} {vars}");
    }
}
