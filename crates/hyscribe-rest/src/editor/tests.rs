// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers on the editors' routes, `tests/golden/editor.tsv`
//! from `reference/rest/editor.list`, and on the block editor's preloads,
//! `tests/golden/preloads.tsv` from `reference/rest/preloads.list` over
//! `preloads.sql`, replayed for every identity.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value};
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use crate::templates::tests::CREDIT;
use crate::tests::content::{Content, default_install_with};
use crate::tests::{changed, get, serve_case, serve_over, sha256_hex};

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
    assert_eq!(cases.len(), 314);
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

/// A site whose active theme is `slug`, with the global styles post PHP
/// creates for it (`reference/rest/theme-<slug>.sql`).
fn themed(slug: &str) -> &'static Content {
    static CONTENT: OnceLock<Vec<(&str, Content)>> = OnceLock::new();
    let themes = CONTENT.get_or_init(|| {
        let state = |slug: &str, sql: &str| {
            let options = format!(
                "INSERT INTO `wp_options` VALUES (9901,'template','{slug}','yes'),(9902,'stylesheet','{slug}','yes');\n"
            );
            default_install_with(&format!("{options}{sql}"))
        };
        vec![
            (
                "twentytwentyfour",
                state(
                    "twentytwentyfour",
                    include_str!("../../../../reference/rest/theme-twentytwentyfour.sql"),
                ),
            ),
            (
                "twentytwentythree",
                state(
                    "twentytwentythree",
                    include_str!("../../../../reference/rest/theme-twentytwentythree.sql"),
                ),
            ),
        ]
    });
    &themes.iter().find(|(known, _)| *known == slug).unwrap().1
}

/// Every answer of `reference/rest/theme.list` over a site with `slug`
/// active is PHP's, as each identity: the larger bodies by hash, the
/// footer's credit naming HyScribe.
fn themed_answers(slug: &str, golden: &str, count: usize) {
    let cases = rest_cases(golden);
    assert_eq!(cases.len(), count);
    let content = themed(slug);
    for case in &cases {
        let served = serve_over(content, case);
        let what = format!("{} {} {}", case.who, case.method, case.target);
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        let body = if case.flag("hash") {
            sha256_hex(&served.body)
        } else {
            served.body
        };
        let php = String::from_utf8_lossy(&case.body).replace(CREDIT[0], CREDIT[1]);
        assert_eq!(String::from_utf8_lossy(&body), php, "{what}");
    }
}

#[test]
fn twenty_twenty_four_answers_match_php() {
    let golden = include_str!("../../tests/golden/theme-twentytwentyfour.tsv");
    themed_answers("twentytwentyfour", golden, 342);
}

#[test]
fn twenty_twenty_three_answers_match_php() {
    let golden = include_str!("../../tests/golden/theme-twentytwentythree.tsv");
    themed_answers("twentytwentythree", golden, 330);
}

fn preloads() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        default_install_with(include_str!("../../../../reference/rest/preloads.sql"))
    })
}

/// Descriptions of routes not ported, which PHP gives.
const UNDESCRIBED: [&str; 2] = [
    "/?rest_route=/wp/v2/plugins",
    "/?rest_route=/wp/v2/global-styles/901/revisions",
];

/// Every ported route's description, the active theme and the site
/// editor's preloads are PHP's, the larger bodies by hash; other themes are
/// not ported.
#[test]
fn preloads_match_php() {
    let cases = rest_cases(include_str!("../../tests/golden/preloads.tsv"));
    assert_eq!(cases.len(), 785);
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
        let php = String::from_utf8_lossy(&case.body).replace(CREDIT[0], CREDIT[1]);
        assert_eq!(String::from_utf8_lossy(&body), php, "{what}");
    }
    assert_eq!(unported, 6 * UNDESCRIBED.len());
}

/// The registered patterns, as PHP gives them with the pattern directory out
/// of reach (the theme footer's credit names HyScribe), as each identity.
#[test]
fn block_patterns_match_php() {
    let cases = rest_cases(include_str!("../../tests/golden/block-patterns.tsv"));
    assert_eq!(cases.len(), 144);
    for case in &cases {
        let served = serve_case(case);
        let what = format!("{} {} {}", case.who, case.method, case.target);
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        let body = String::from_utf8_lossy(&case.body).replace(CREDIT[0], CREDIT[1]);
        assert_eq!(String::from_utf8_lossy(&served.body), body, "{what}");
    }
}

/// The old core categories PHP renames, which no registered pattern has
/// but `query`.
#[test]
fn old_categories_are_migrated() {
    let name =
        |at: usize, name: &str| (Key::Int(at as i64), Value::String(name.as_bytes().to_vec()));
    let old = Value::Array(vec![
        name(0, "buttons"),
        name(1, "columns"),
        name(2, "query"),
        name(3, "text"),
    ]);
    let new = Value::Array(vec![
        name(0, "call-to-action"),
        name(1, "text"),
        name(2, "posts"),
        name(3, "text"),
    ]);
    assert_eq!(super::patterns::migrated(&old), new);
    assert_eq!(
        super::patterns::migrated(&Value::Bool(true)),
        Value::Bool(true)
    );
}

/// Theme file URLs in pattern content, nested patterns included, are the
/// site's own, whatever address the theme export ran under.
#[test]
fn pattern_urls_follow_the_site() {
    let site = changed(&[("siteurl", b"https://wp.test")]);
    let target = "/?rest_route=/wp/v2/block-patterns/patterns&_fields=content";
    let served = get(site, "administrator", target);
    let body = String::from_utf8_lossy(&served.body);
    let own = r"https:\/\/wp.test\/wp-content\/themes\/twentytwentyfive\/assets\/images\/";
    assert!(body.contains(own));
    assert!(!body.contains("127.0.0.1"));
}

/// A pattern naming a pattern that is not the theme's (core's or the pattern
/// directory's) is not ported.
#[test]
fn a_foreign_pattern_block_is_not_ported() {
    let content = "<!-- wp:pattern {\"slug\":\"core/nope\"} /-->";
    let pattern = Value::Array(vec![(
        Key::String(b"content".to_vec()),
        Value::String(content.as_bytes().to_vec()),
    )]);
    let wanted = vec![b"content".to_vec()];
    let theme = hyscribe_render::theme::twentytwentyfive();
    let served = super::patterns::prepared(&theme, &[&pattern], &wanted, b"http://example.test");
    assert!(served.is_err());
}

/// A theme the site names that HyScribe does not have is not ported.
#[test]
fn a_missing_theme_is_not_ported() {
    let state = "INSERT INTO `wp_options` VALUES (9901,'stylesheet','gone','yes');\n";
    let content: &'static Content = Box::leak(Box::new(default_install_with(state)));
    for target in [
        "/?rest_route=/wp/v2/themes&status=active&_fields=stylesheet",
        "/?rest_route=/wp/v2/themes/twentytwentyfive&_fields=stylesheet",
        "/?rest_route=/wp/v2/block-patterns/patterns",
        "/?rest_route=/wp/v2/block-patterns/patterns&_fields=name",
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
