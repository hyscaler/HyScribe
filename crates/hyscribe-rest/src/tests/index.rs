// SPDX-License-Identifier: AGPL-3.0-or-later

//! The index's goldens, `index.tsv` over the default install and
//! `index-site.tsv` and `index-logo.tsv` over their lists' rows, replayed
//! for a visitor and each role, and what the oracle, served over HTTP at
//! `127.0.0.1`, cannot show: HTTPS, and the hosts PHP takes as `localhost`.

use std::sync::OnceLock;

use super::content::default_install_with;
use super::*;

/// Every answer of `golden` over `content` is PHP's, but those flagged
/// `refused`, which are answered 501.
fn replay(content: &'static Content, golden: &str, count: usize) {
    let refused = |case: &RestCase| case.flag("refused");
    let (failures, not_served) = mismatches(&[(content, golden, false)], true, &refused);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    let cases = rest_cases(golden);
    assert_eq!(cases.len(), count);
    assert_eq!(
        not_served,
        cases.iter().filter(|case| refused(case)).count()
    );
}

#[test]
fn index_matches_php() {
    let golden = include_str!("../../tests/golden/index.tsv");
    replay(content::default_install(), golden, 6 * 41);
}

/// The namespace indexes (`namespaces.tsv`), but for a namespace that is
/// not a string, an offset PHP fails on, which is answered 501.
#[test]
fn namespace_indexes_match_php() {
    let golden = include_str!("../../tests/golden/namespaces.tsv");
    let unported = |case: &RestCase| case.target.contains("namespace[]");
    let goldens = [(content::default_install(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &unported);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!((rest_cases(golden).len(), not_served), (6 * 34, 6));
}

/// A site icon, a theme's logo and a named time zone.
#[test]
fn icon_logo_and_zone_match_php() {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    let content = CONTENT.get_or_init(|| {
        default_install_with(include_str!("../../../../reference/rest/index-site.sql"))
    });
    replay(
        content,
        include_str!("../../tests/golden/index-site.tsv"),
        6 * 7,
    );
}

/// A draft's unset GMT dates in a named time zone, which overrides the
/// stored `gmt_offset` as it does the index's (`zone.list`).
#[test]
fn zoned_draft_matches_php() {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    let content = CONTENT
        .get_or_init(|| default_install_with(include_str!("../../../../reference/rest/zone.sql")));
    replay(content, include_str!("../../tests/golden/zone.tsv"), 6);
}

/// The Site Logo block's option before the theme's logo, and a post and a
/// page as the logo and icon.
#[test]
fn logo_option_and_posts_match_php() {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    let content = CONTENT.get_or_init(|| {
        default_install_with(include_str!("../../../../reference/rest/index-logo.sql"))
    });
    replay(
        content,
        include_str!("../../tests/golden/index-logo.tsv"),
        6 * 3,
    );
}

/// Over HTTPS PHP adds application passwords, which are not ported, to
/// `authentication`, and gives users who may upload the image settings.
#[test]
fn https_sites() {
    let https = changed(&[("siteurl", b"https://127.0.0.1:8081")]);
    let authentication = "/?rest_route=/&_fields=authentication";
    assert_eq!(get(https, "visitor", authentication).status, 501);
    let http = get(content::default_install(), "visitor", authentication);
    assert_eq!(http.body, br#"{"authentication":[]}"#);
    let strip = "/?rest_route=/&_fields=image_strip_meta";
    assert_eq!(
        get(https, "author", strip).body,
        br#"{"image_strip_meta":true}"#
    );
    assert_eq!(get(https, "subscriber", strip).body, b"[]");
}

/// `strtok( $host, ':' )`: the host without the separators before it or the
/// port after, in any case; `localhost` and its subdomains are secure.
#[test]
fn local_hosts_are_secure_contexts() {
    let oracle = Oracle::over(content::default_install(), false);
    let hosts: [(&[u8], bool); 6] = [
        (b"LocalHost", true),
        (b"::localhost:80", true),
        (b"a.localhost", true),
        (b"localhost.test", false),
        (b"alocalhost", false),
        (b"", false),
    ];
    for (host, secure) in hosts {
        let context = Context {
            site: &oracle,
            user: 0,
            head: false,
            uri: b"/",
            parameters: false,
            remote_addr: b"",
            user_agent: None,
            host,
            now: 0,
        };
        let found = crate::index::client_side_media(&context);
        assert_eq!(found, secure, "{}", String::from_utf8_lossy(host));
    }
}
