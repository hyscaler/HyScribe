// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers on the navigation fallback route, replayed for
//! every identity: `tests/golden/navigation-fallback-existing.tsv` over a
//! site with menus, and `navigation-fallback.tsv` over one without, where
//! what PHP writes is a plan (`hyscribe-http` makes the write).

use std::sync::OnceLock;

use hyscribe_render::{Fallback, navigation_fallback};
use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use crate::tests::content::{Content, default_install_with};
use crate::tests::serve_over;
use crate::tests::site::DECLINE_FALLBACK;

const ROUTE: &str = "/?rest_route=/wp-block-editor/v1/navigation-fallback";

fn existing() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        default_install_with(include_str!(
            "../../../../../reference/rest/navigation-fallback-existing.sql"
        ))
    })
}

fn empty() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| default_install_with(""))
}

/// `content` serves `case` as PHP does: its status, headers and body.
fn answers_as_php(content: &'static Content, case: &RestCase) {
    let served = serve_over(content, case);
    let what = format!("{} {} {}", case.who, case.method, case.target);
    assert_eq!(served.status, case.status, "{what}");
    let mut php = case.headers.clone();
    php.sort();
    assert_eq!(rest_headers(&served.headers), php, "{what}");
    let body = String::from_utf8_lossy(&case.body);
    assert_eq!(String::from_utf8_lossy(&served.body), body, "{what}");
}

/// With a menu the route finds the most recently published one, and
/// writes nothing.
#[test]
fn a_site_with_menus_answers_as_php() {
    let cases = rest_cases(include_str!(
        "../../../tests/golden/navigation-fallback-existing.tsv"
    ));
    for case in &cases {
        answers_as_php(existing(), case);
    }
    assert_eq!(cases.len(), 6 * 15);
}

/// Without a menu, what PHP refuses is refused as PHP refuses it, and what
/// PHP answers 200 to a read that writes is a plan, the answer 501 here.
#[test]
fn a_site_without_menus_answers_as_php_where_it_writes_nothing() {
    let cases = rest_cases(include_str!(
        "../../../tests/golden/navigation-fallback.tsv"
    ));
    let mut planned = 0;
    for case in &cases {
        if case.status == 200 && case.method != "OPTIONS" {
            assert_eq!(serve_over(empty(), case).status, 501, "{}", case.target);
            planned += 1;
        } else {
            answers_as_php(empty(), case);
        }
    }
    assert_eq!((cases.len(), planned), (6 * 15, 14));
}

/// `get_fallback()` gives no post where the filter declines one.
#[test]
fn a_declined_fallback_is_not_found() {
    DECLINE_FALLBACK.set(true);
    let case = RestCase {
        who: "administrator".into(),
        method: "GET".into(),
        target: ROUTE.into(),
        flags: String::new(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
    };
    let served = serve_over(empty(), &case);
    DECLINE_FALLBACK.set(false);
    assert_eq!(served.status, 404);
    let error =
        r#"{"code":"no_fallback_menu","message":"No fallback menu found.","data":{"status":404}}"#;
    assert_eq!(String::from_utf8_lossy(&served.body), error);
}

/// A classic menu with a link not ported makes no post, so there is no
/// answer to give.
#[test]
fn a_classic_menu_not_ported_is_not_answered() {
    let state = concat!(
        "INSERT INTO `wp_terms` VALUES (9101,'Unported','unported',0);\n",
        "INSERT INTO `wp_term_taxonomy` VALUES (9101,9101,'nav_menu','',0,1);\n",
        "INSERT INTO `wp_posts` VALUES ",
        "(9102,1,'2026-11-07 10:00:00','2026-11-07 10:00:00','','Link','','publish','closed','closed','','9102','','','2026-11-07 10:00:00','2026-11-07 10:00:00','',0,'',1,'nav_menu_item','',0);\n",
        "INSERT INTO `wp_term_relationships` VALUES (9102,9101,0);\n",
        "INSERT INTO `wp_postmeta` VALUES (9103,9102,'_menu_item_type','taxonomy'),",
        "(9104,9102,'_menu_item_object','nav_menu'),(9105,9102,'_menu_item_object_id','9101');\n",
    );
    let content: &'static Content = Box::leak(Box::new(default_install_with(state)));
    let case = RestCase {
        who: "administrator".into(),
        method: "GET".into(),
        target: ROUTE.into(),
        flags: String::new(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
    };
    assert_eq!(serve_over(content, &case).status, 501);
    // What no write makes the answer 501, not a plan.
    let site = crate::tests::render_site(content);
    let found = navigation_fallback(&*site, &hyscribe_render::site::Request::default());
    assert!(matches!(found, Fallback::NotPorted));
}
