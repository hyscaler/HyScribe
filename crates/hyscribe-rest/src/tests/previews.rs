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
