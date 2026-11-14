// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin's action checks read hostile queries, form bodies and
//! referers, as `parse_str()` gives them, without panicking.

#![no_main]

use hyscribe_admin::actions::{ActionSite, Asked, admin, admin_ajax};
use hyscribe_auth::caps::{CapPost, CapabilityData};
use hyscribe_phpser::{Value, parse_str};
use libfuzzer_sys::fuzz_target;

/// A site whose user may do anything, and whose nonces starting with `1`
/// verify.
struct Open;

impl CapabilityData for Open {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn user_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn user_exists(&self, _: u64) -> bool {
        true
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        Some(CapPost {
            id,
            author: 1,
            status: "trash".into(),
            post_type: "post".into(),
            parent: 0,
        })
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        Some(1)
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        Some("post_tag".into())
    }
    fn constant(&self, _: &str) -> bool {
        false
    }
}

impl ActionSite for Open {
    fn current_user_id(&self) -> u64 {
        1
    }
    fn current_user_can(&self, _: &str, _: &[Value]) -> bool {
        true
    }
    fn verify_nonce(&self, nonce: &[u8], _: &str) -> bool {
        nonce.first() == Some(&b'1')
    }
    fn validate_redirect(&self, location: &[u8]) -> Vec<u8> {
        location.to_vec()
    }
    fn comment_approved(&self, _: u64) -> Option<Vec<u8>> {
        Some(b"1".to_vec())
    }
    fn user_roles(&self) -> Option<Vec<u8>> {
        None
    }
}

const SCREENS: [&str; 11] = [
    "edit",
    "post",
    "edit-tags",
    "edit-comments",
    "users",
    "user-new",
    "profile",
    "user-edit",
    "options",
    "options-permalink",
    "themes",
];

fuzz_target!(|data: &[u8]| {
    let mut parts = data.split(|&b| b == 0);
    let mut next = || parts.next().unwrap_or_default();
    let (screen, query, body, referer) = (next(), next(), next(), next());
    let fields = |text: &[u8]| match parse_str(text) {
        Value::Array(fields) => fields,
        _ => Vec::new(),
    };
    let (get, post) = (fields(query), fields(body));
    let asked = Asked {
        get: &get,
        post: &post,
        referer: Some(referer),
        uri: query,
        home: "http://example.test",
    };
    let screen = SCREENS[usize::from(screen.first().copied().unwrap_or(0)) % SCREENS.len()];
    let _ = admin(&Open, screen, &asked, !body.is_empty());
    let _ = admin_ajax(Some(&Open), &asked);
    let _ = admin_ajax(None, &asked);
});
