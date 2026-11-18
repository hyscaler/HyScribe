// SPDX-License-Identifier: AGPL-3.0-or-later

//! A site without outbound fetches (`HTTP_OUTBOUND=off`, ADR 0019): the
//! link previews answer 501 whatever is asked, and nothing is fetched or
//! cached. `hyscribe-http` replays PHP's answers with fetches on.

use hyscribe_fetch::Args;

use super::*;

#[test]
fn previews_are_not_served_while_fetches_are_off() {
    let oracle = Oracle::over(content::default_install(), false);
    for (who, target) in [
        ("visitor", "/?rest_route=/wp-block-editor/v1/url-details"),
        (
            "administrator",
            "/?rest_route=/wp-block-editor/v1/url-details",
        ),
        (
            "administrator",
            "/?rest_route=/wp-block-editor/v1/url-details&url=http://pages.test/",
        ),
    ] {
        let case = RestCase {
            who: who.into(),
            method: "GET".into(),
            target: target.into(),
            flags: String::new(),
            status: 501,
            headers: Vec::new(),
            body: Vec::new(),
            sent: Vec::new(),
        };
        assert_eq!(
            serve_with(&oracle, &case),
            Served::not_ported(),
            "{who} {target}"
        );
    }
    assert!(!oracle.outbound());
    assert!(
        oracle
            .fetch(b"http://pages.test/", Args::new(String::new()))
            .is_none()
    );
    oracle.set_site_transient("unused", b"", 0);
    oracle.expire_site_transient("unused");
}

/// A cached page is read until its timeout has passed, as
/// `get_site_transient()` compares `$timeout < time()`, then deleted.
#[test]
fn cached_pages_last_until_their_timeout() {
    static CONTENT: std::sync::OnceLock<content::Content> = std::sync::OnceLock::new();
    let content = CONTENT.get_or_init(|| {
        let mut content = content::default_install_with("");
        let key = "g_url_details_response_68fb6b339d66b07e2b438f2f08e7ab24";
        let mut set = |name: String, value: &[u8]| content.options.insert(name, value.to_vec());
        set(format!("_site_transient_timeout_{key}"), b"4102444800");
        set(
            format!("_site_transient_{key}"),
            b"<html><head><title>Cached</title>",
        );
        content
    });
    let oracle = Oracle::over(content, false);
    let key = "g_url_details_response_68fb6b339d66b07e2b438f2f08e7ab24";
    let at = |now: i64| Context {
        site: &oracle,
        user: 0,
        head: false,
        uri: b"",
        parameters: false,
        host: b"",
        now,
        remote_addr: b"",
        user_agent: None,
    };
    let page = crate::url_details::cached(&at(4_102_444_800), key);
    assert!(page.unwrap().starts_with(b"<html><head><title>Cached"));
    assert_eq!(crate::url_details::cached(&at(4_102_444_801), key), None);
}
