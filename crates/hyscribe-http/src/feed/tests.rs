// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn last_modified_without_posts_is_now() {
    assert_eq!(last_modified(None, 0), "Thu, 01 Jan 1970 00:00:00 GMT");
    assert_eq!(
        last_modified(Some("2026-10-27 08:08:15"), 0),
        "Tue, 27 Oct 2026 08:08:15 GMT"
    );
}

#[test]
fn a_feed_that_fails_is_the_server_error_page() {
    let failed = Rendered {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        head: String::new(),
        body: "<body></body>".into(),
    };
    let cache = [
        (header::LAST_MODIFIED, String::new()),
        (header::ETAG, String::new()),
    ];
    let response = response(
        Err(failed),
        "application/rss+xml; charset=UTF-8".into(),
        cache,
    );
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response.headers()[header::CONTENT_TYPE],
        "text/html; charset=UTF-8"
    );
}
