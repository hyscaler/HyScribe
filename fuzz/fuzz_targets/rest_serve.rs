// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API answers any route, query, method and headers without
//! panicking: route matching, `_fields`, `_jsonp`, `_envelope`, `_embed`,
//! the arguments' schemas and the routes all take input from the request,
//! the post routes over one published post.

#![no_main]

use std::sync::OnceLock;

use hyscribe_model::{Post, PostQuery, Term, TermQuery};
use hyscribe_phpser::{Value, parse_str};
use hyscribe_render::theme::{Theme, twentytwentyfive};
use hyscribe_rest::{Request, Site, serve};
use libfuzzer_sys::fuzz_target;

struct Fuzzed;

fn post() -> Post {
    Post {
        id: 1,
        author: 1,
        status: "publish".into(),
        post_type: "post".into(),
        content: b"<!-- wp:paragraph --><p>A</p><!-- /wp:paragraph -->".to_vec(),
        ..Post::default()
    }
}

impl hyscribe_render::site::Site for Fuzzed {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn home_url(&self) -> Vec<u8> {
        b"http://example.test".to_vec()
    }
    fn post(&self, id: u64) -> Option<Post> {
        (id == 1).then(post)
    }
    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        let found = query.post_types.iter().any(|t| t == "post") && query.offset == 0;
        if found { vec![post()] } else { Vec::new() }
    }
}

impl Site for Fuzzed {
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
    fn can(&self, user: u64, cap: &str, _: &[Value]) -> bool {
        user % 2 == 1 && cap.len() % 2 == 0
    }
    fn terms(&self, _: &TermQuery) -> Vec<Term> {
        Vec::new()
    }
    fn count_terms(&self, _: &TermQuery) -> u64 {
        0
    }
    fn term_link(&self, _: &str, _: &Term) -> String {
        String::new()
    }
    fn post(&self, id: u64) -> Option<Post> {
        (id == 1).then(post)
    }
    fn reads(&self) -> &dyn hyscribe_render::site::Site {
        self
    }
    fn theme(&self) -> &Theme {
        static THEME: OnceLock<Theme> = OnceLock::new();
        THEME.get_or_init(twentytwentyfive)
    }
}

fuzz_target!(|data: &[u8]| {
    let mut parts = data.split(|&b| b == 0);
    let mut next = || parts.next().unwrap_or_default();
    let (method, route, query, header) = (next(), next(), next(), next());
    let methods = ["GET", "HEAD", "POST", "PUT", "DELETE", "OPTIONS"];
    let method = methods[usize::from(method.first().copied().unwrap_or(0)) % methods.len()];
    let get = match parse_str(query) {
        Value::Array(get) => get,
        _ => Vec::new(),
    };
    let request = Request {
        method,
        route,
        uri: query,
        get: &get,
        post: &[],
        cookie_user: u64::from(header.first().copied().unwrap_or(0) % 3),
        origin: header.get(1..).filter(|origin| !origin.is_empty()),
        nonce: header.get(..1),
        method_override: None,
    };
    let served = serve(&Fuzzed, &request);
    assert!((200..600).contains(&served.status));
});
