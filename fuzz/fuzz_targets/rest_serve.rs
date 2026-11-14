// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API answers any route, query, method and headers without
//! panicking: route matching, `_fields`, `_jsonp`, `_envelope`, the
//! arguments' schemas and the types route all take input from the request.

#![no_main]

use hyscribe_phpser::{Value, parse_str};
use hyscribe_rest::{Request, Site, serve};
use libfuzzer_sys::fuzz_target;

struct Fuzzed;

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
    fn can(&self, user: u64, cap: &str) -> bool {
        user % 2 == 1 && cap.len() % 2 == 0
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
