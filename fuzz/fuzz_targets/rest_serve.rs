// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API answers any route, query, method, headers and body without
//! panicking: route matching, the JSON and form bodies, `_fields`,
//! `_jsonp`, `_envelope`, the arguments' schemas, the routes and the
//! writes' plans and answers all take input from the request, and
//! the site's one template, template part and global styles post, its one
//! comment and its one user have the input's content.

#![no_main]

use std::sync::OnceLock;

use hyscribe_model::{Comment, CommentQuery, CommentRow, Post, PostQuery, Term, TermQuery, User, UserQuery};
use hyscribe_phpser::{Key, Value, parse_str};
use hyscribe_render::theme::{Theme, twentytwentyfive};
use hyscribe_rest::{Answer, Outcome, Request, Site, answer, written};
use libfuzzer_sys::fuzz_target;

/// A site whose posts have the fuzzed content.
struct Fuzzed<'a> {
    content: &'a [u8],
}

impl Site for Fuzzed<'_> {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        Some(b"UTF-8".to_vec())
    }
    fn rest_url(&self, path: &str) -> String {
        format!("http://example.test/index.php?rest_route={path}")
    }
    fn verify_nonce(&self, nonce: &[u8]) -> bool {
        nonce == b"n"
    }
    fn create_nonce(&self) -> String {
        "n".into()
    }
    fn nonce(&self, action: &str) -> String {
        action.into()
    }
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        user % 2 == 1 && (cap.len() + args.len()) % 2 == 0
    }
    fn terms(&self, _: &TermQuery) -> Vec<Term> {
        Vec::new()
    }
    fn count_terms(&self, _: &TermQuery) -> u64 {
        0
    }
    fn term_link(&self, taxonomy: &str, _: &Term) -> String {
        format!("http://example.test/?taxonomy={taxonomy}")
    }
    fn post(&self, id: u64) -> Option<Post> {
        let post_type = ["wp_template", "wp_template_part", "wp_global_styles"];
        let post_type = post_type.get(usize::try_from(id).ok()?.checked_sub(1)?)?;
        Some(Post {
            id,
            author: id % 2,
            content: self.content.to_vec(),
            title: self.content.to_vec(),
            name: self.content.to_vec(),
            status: "publish".into(),
            date: "2026-11-14 10:00:00".into(),
            modified: "2026-11-14 10:00:00".into(),
            post_type: (*post_type).into(),
            ..Post::default()
        })
    }
    fn posts_of_type(&self, post_type: &str) -> Vec<Post> {
        (1..4)
            .filter_map(|id| self.post(id))
            .filter(|post| post.post_type == post_type)
            .collect()
    }
    fn post_terms(&self, post: u64, _: &str) -> Vec<Term> {
        let name = if post == 2 { self.content } else { b"UTF-8" };
        vec![Term {
            name: name.to_vec(),
            ..Term::default()
        }]
    }
    fn post_meta(&self, post: u64, _: &str) -> Option<Vec<u8>> {
        (post == 1).then(|| self.content.to_vec())
    }
    fn revisions(&self, post: u64) -> Vec<u64> {
        (0..post).collect()
    }
    fn user(&self, id: u64) -> Option<User> {
        (id == 1).then(|| User {
            id,
            display_name: self.content.to_vec(),
            nicename: self.content.to_vec(),
            ..User::default()
        })
    }
    fn table_prefix(&self) -> String {
        "wp_".into()
    }
    fn comments(&self, _: &CommentQuery) -> Vec<CommentRow> {
        vec![CommentRow {
            comment: Comment {
                id: 1,
                post_id: 1,
                author: self.content.to_vec(),
                content: self.content.to_vec(),
                date: "2026-11-14 10:00:00".into(),
                date_gmt: "2026-11-14 10:00:00".into(),
                ..Comment::default()
            },
            approved: "1".into(),
            ..CommentRow::default()
        }]
    }
    fn count_comments(&self, _: &CommentQuery) -> u64 {
        1
    }
    fn comment_link(&self, comment: &Comment) -> String {
        format!("http://example.test/#comment-{}", comment.id)
    }
    fn users(&self, _: &UserQuery) -> Vec<User> {
        self.user(1).into_iter().collect()
    }
    fn count_users(&self, _: &UserQuery) -> u64 {
        1
    }
    fn user_meta(&self, id: u64, _: &str) -> Option<Vec<u8>> {
        (id == 1).then(|| self.content.to_vec())
    }
    fn user_registered(&self, _: u64) -> String {
        "2026-11-14 10:00:00".into()
    }
    fn allcaps(&self, id: u64) -> Vec<(Key, Value)> {
        vec![(
            Key::from_php_string(b"read".to_vec()),
            Value::Bool(id % 2 == 1),
        )]
    }
    fn author_link(&self, user: &User) -> String {
        format!("http://example.test/?author={}", user.id)
    }
    fn has_posts(&self, author: u64, _: &[(String, bool)]) -> bool {
        author == 1
    }
    fn public_users(&self) -> bool {
        self.content.len() % 2 == 0
    }
    fn reads(&self) -> &dyn hyscribe_render::site::Site {
        self
    }
    fn theme(&self) -> &Theme {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(twentytwentyfive)
    }
}

/// The post routes' reads: one published post, 4, with the fuzzed content.
impl hyscribe_render::site::Site for Fuzzed<'_> {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn home_url(&self) -> Vec<u8> {
        b"http://example.test".to_vec()
    }
    fn post(&self, id: u64) -> Option<Post> {
        (id == 4).then(|| Post {
            id,
            author: 1,
            content: self.content.to_vec(),
            status: "publish".into(),
            post_type: "post".into(),
            ..Post::default()
        })
    }
    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        let found = query.post_types.iter().any(|t| t == "post") && query.offset == 0;
        found.then(|| hyscribe_render::site::Site::post(self, 4)).flatten().into_iter().collect()
    }
}

fuzz_target!(|data: &[u8]| {
    let mut parts = data.split(|&b| b == 0);
    let mut next = || parts.next().unwrap_or_default();
    let (verb, route, query, header, content) = (next(), next(), next(), next(), next());
    let body = next();
    let methods = ["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
    let method = methods[usize::from(verb.first().copied().unwrap_or(0)) % methods.len()];
    let types: [Option<&[u8]>; 5] = [
        None,
        Some(b"application/json"),
        Some(b"application/x-www-form-urlencoded"),
        Some(b"multipart/form-data; boundary=x"),
        Some(b"text/plain"),
    ];
    let content_type = types[usize::from(verb.get(1).copied().unwrap_or(0)) % types.len()];
    let post = match parse_str(body) {
        Value::Array(post) if method == "POST" && content_type == types[2] => post,
        _ => Vec::new(),
    };
    let get = match parse_str(query) {
        Value::Array(get) => get,
        _ => Vec::new(),
    };
    let request = Request {
        method,
        route,
        uri: query,
        get: &get,
        post: &post,
        cookie_user: u64::from(header.first().copied().unwrap_or(0) % 3),
        origin: header.get(1..).filter(|origin| !origin.is_empty()),
        nonce: header.get(..1),
        remote_addr: header.get(1..).unwrap_or_default(),
        user_agent: Some(header),
        method_override: None,
        content_type,
        body,
    };
    let site = Fuzzed { content };
    let served = match answer(&site, &request) {
        Answer::Served(served) => served,
        // The write made, the item written is the fuzzed one, or refused.
        Answer::Write(pending) => {
            let outcomes = [
                Outcome::Written(4),
                Outcome::Refused("rest_cannot_delete"),
                Outcome::Exists(1),
                Outcome::Duplicate(content.to_vec()),
                Outcome::Cached(vec![("blogname".into(), Value::String(content.to_vec()))]),
            ];
            let at = usize::from(verb.get(2).copied().unwrap_or(0)) % outcomes.len();
            written(&site, &request, *pending, outcomes[at].clone())
        }
    };
    assert!((200..600).contains(&served.status));
});
