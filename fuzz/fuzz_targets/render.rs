// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rendering any post content with Twenty Twenty-Five never panics or
//! overflows the stack. The site's options, post meta (footnotes, image
//! metadata, bindings) and comments come from the input too, and the post
//! is its own synced pattern and query result, so self-reference reaches
//! the render depth limit.

#![no_main]

use std::sync::LazyLock;

use hyscribe_model::{Comment, Post, PostQuery, Term};
use hyscribe_render::site::{Request, Site};
use hyscribe_render::template::render_content;
use hyscribe_render::theme::{Theme, twentytwentyfive};
use libfuzzer_sys::fuzz_target;

static THEME: LazyLock<Theme> = LazyLock::new(twentytwentyfive);

/// A site of one post, whose stored values are all parts of the input.
struct Fuzzed<'a> {
    content: &'a [u8],
    meta: &'a [u8],
    option: &'a [u8],
}

impl Fuzzed<'_> {
    fn the_post(&self) -> Post {
        Post {
            id: 1,
            content: self.content.to_vec(),
            title: self.option.to_vec(),
            excerpt: self.meta.to_vec(),
            status: "publish".into(),
            post_type: "post".into(),
            comment_status: "open".into(),
            ..Post::default()
        }
    }
}

impl Site for Fuzzed<'_> {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        Some(self.option.to_vec())
    }
    fn home_url(&self) -> Vec<u8> {
        b"http://example.org".to_vec()
    }
    fn post(&self, id: u64) -> Option<Post> {
        (id == 1).then(|| self.the_post())
    }
    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        Some(self.meta.to_vec())
    }
    fn query(&self, _: &PostQuery) -> Vec<Post> {
        vec![self.the_post()]
    }
    fn comments(&self, _: u64) -> Vec<Comment> {
        vec![Comment {
            id: 1,
            post_id: 1,
            content: self.meta.to_vec(),
            author: self.option.to_vec(),
            ..Comment::default()
        }]
    }
}

fuzz_target!(|data: &[u8]| {
    let mut parts = data.splitn(3, |b| *b == 0xFF);
    let site = Fuzzed {
        content: parts.next().unwrap_or_default(),
        meta: parts.next().unwrap_or_default(),
        option: parts.next().unwrap_or_default(),
    };
    let request = Request {
        singular: true,
        post: Some(1),
        queried_object_id: 1,
        posts: vec![site.the_post()],
        ..Request::default()
    };
    let _ = render_content(&THEME, &site, &request, site.content);
});
