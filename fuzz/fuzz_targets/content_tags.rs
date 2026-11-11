// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_filter_content_tags()` takes any content without panicking, with an
//! attachment whose stored metadata comes from the input too.

#![no_main]

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_render::content_tags::filter;
use hyscribe_render::site::Site;
use libfuzzer_sys::fuzz_target;

/// Every `wp-image-N` names an attachment with the input's metadata.
struct Fuzzed<'a> {
    meta: &'a [u8],
}

impl Site for Fuzzed<'_> {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }
    fn post(&self, id: u64) -> Option<Post> {
        Some(Post {
            id,
            post_type: "attachment".into(),
            mime_type: "image/jpeg".into(),
            ..Post::default()
        })
    }
    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        Some(self.meta.to_vec())
    }
    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }
}

fuzz_target!(|data: &[u8]| {
    let middle = data.len() / 2;
    let (meta, content) = data.split_at(middle);
    let site = Fuzzed { meta };
    let _ = filter(&site, content, middle % 2 == 0);
});
