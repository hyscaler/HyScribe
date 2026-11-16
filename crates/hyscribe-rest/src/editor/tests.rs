// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers on the editors' routes, `tests/golden/editor.tsv`
//! from `reference/rest/editor.list`, and on the block editor's preloads,
//! `tests/golden/preloads.tsv` from `reference/rest/preloads.list` over
//! `preloads.sql`, replayed for every identity.

use std::sync::OnceLock;

use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use crate::tests::content::{Content, default_install_with};
use crate::tests::{serve_case, serve_over, sha256_hex};

/// The identities PHP lets read the editors' registries.
const EDITORS: [&str; 4] = ["administrator", "editor", "author", "contributor"];

/// `case` as `who`.
fn as_who(case: &RestCase, who: &str) -> RestCase {
    RestCase {
        who: who.into(),
        method: case.method.clone(),
        target: case.target.clone(),
        flags: case.flags.clone(),
        status: case.status,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
    }
}

fn not_ported(case: &RestCase) {
    let served = serve_case(case);
    let what = format!("{} {} {}", case.who, case.method, case.target);
    assert_eq!(served.status, 501, "{what}");
    assert!(
        served.headers.is_empty() && served.body.is_empty(),
        "{what}"
    );
}

/// Every recorded answer is PHP's; a request PHP answers by writing, or
/// was not sent as an identity because it would, is answered 501, as is
/// an inactive theme, which HyScribe does not have.
#[test]
fn answers_match_php() {
    let cases = rest_cases(include_str!("../../tests/golden/editor.tsv"));
    assert_eq!(cases.len(), 317);
    for case in &cases {
        if case.status == 0 {
            EDITORS
                .iter()
                .for_each(|who| not_ported(&as_who(case, who)));
            continue;
        }
        if let Some((_, skipped)) = case.flags.split_once("skip=") {
            skipped
                .split('+')
                .for_each(|who| not_ported(&as_who(case, who)));
        }
        if case.who == "administrator" && case.target == "/?rest_route=/wp/v2/themes/nope" {
            not_ported(case);
            continue;
        }
        let served = serve_case(case);
        let what = format!(
            "{} {} {} {}",
            case.who, case.method, case.target, case.flags
        );
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        assert_eq!(
            String::from_utf8_lossy(&served.body),
            String::from_utf8_lossy(&case.body),
            "{what}"
        );
    }
}

fn preloads() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        default_install_with(include_str!("../../../../reference/rest/preloads.sql"))
    })
}

/// Descriptions of routes not ported, which PHP gives.
const UNDESCRIBED: [&str; 4] = [
    "/?rest_route=/",
    "/?rest_route=/wp/v2",
    "/?rest_route=/wp/v2/plugins",
    "/?rest_route=/wp/v2/global-styles/901/revisions",
];

/// Every ported route's description and the active theme are PHP's, the
/// larger bodies by hash; other themes are not ported.
#[test]
fn preloads_match_php() {
    let cases = rest_cases(include_str!("../../tests/golden/preloads.tsv"));
    assert_eq!(cases.len(), 689);
    let mut unported = 0;
    for case in &cases {
        if let Some((_, skipped)) = case.flags.split_once("skip=") {
            for who in skipped.split('+') {
                let served = serve_over(preloads(), &as_who(case, who));
                assert_eq!(served.status, 501, "{who} {}", case.target);
            }
        }
        let served = serve_over(preloads(), case);
        let what = format!(
            "{} {} {} {}",
            case.who, case.method, case.target, case.flags
        );
        if served.status == 501 {
            assert!(UNDESCRIBED.contains(&case.target.as_str()), "{what}");
            assert!(served.headers.is_empty() && served.body.is_empty());
            unported += 1;
            continue;
        }
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        let body = if case.flag("hash") {
            sha256_hex(&served.body)
        } else {
            served.body
        };
        assert_eq!(
            String::from_utf8_lossy(&body),
            String::from_utf8_lossy(&case.body),
            "{what}"
        );
    }
    assert_eq!(unported, 6 * UNDESCRIBED.len());
}

/// Another theme active, which HyScribe does not have, is not ported.
#[test]
fn another_theme_is_not_ported() {
    let state = "INSERT INTO `wp_options` VALUES (9901,'stylesheet','twentytwentyfour','yes');\n";
    let content: &'static Content = Box::leak(Box::new(default_install_with(state)));
    for target in [
        "/?rest_route=/wp/v2/themes&status=active&_fields=stylesheet",
        "/?rest_route=/wp/v2/themes/twentytwentyfive&_fields=stylesheet",
    ] {
        let case = RestCase {
            who: "administrator".into(),
            method: "GET".into(),
            target: target.into(),
            flags: String::new(),
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            sent: Vec::new(),
        };
        assert_eq!(serve_over(content, &case).status, 501, "{target}");
    }
}
