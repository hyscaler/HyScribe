// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers on the routes of synced patterns, navigation
//! menus and classic menus, `tests/golden/patterns.tsv` over
//! `reference/rest/patterns.sql`, replayed for every identity.

use std::sync::OnceLock;

use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use crate::tests::content::{Content, default_install_with};
use crate::tests::serve_over;

fn patterns() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        default_install_with(include_str!("../../../../reference/rest/patterns.sql"))
    })
}

/// Requests PHP answers on routes or with arguments not ported: the
/// revisions and autosaves of patterns, navigation menus and menu items,
/// and the term limits whose schemas use `oneOf`.
fn unported(case: &RestCase) -> bool {
    let target = &case.target;
    target.contains("/revisions")
        || target.contains("/autosaves")
        || target.contains("wp_pattern_category=")
        || target.contains("menus=")
}

#[test]
fn answers_match_php() {
    let cases = rest_cases(include_str!("../../tests/golden/patterns.tsv"));
    let mut not_served = 0;
    for case in &cases {
        let served = serve_over(patterns(), case);
        let what = format!(
            "{} {} {} {}",
            case.who, case.method, case.target, case.flags
        );
        if unported(case) {
            assert_eq!((served.status, served.body.len()), (501, 0), "{what}");
            not_served += 1;
            continue;
        }
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        let body = String::from_utf8_lossy(&case.body);
        assert_eq!(String::from_utf8_lossy(&served.body), body, "{what}");
    }
    assert_eq!((cases.len(), not_served), (6 * 100, 6 * 9));
}

/// `content` serves `target` to the administrator.
fn get(content: &'static Content, target: &str) -> crate::Served {
    let case = RestCase {
        who: "administrator".into(),
        method: "GET".into(),
        target: target.into(),
        flags: String::new(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
    };
    serve_over(content, &case)
}

/// A menu item of a post type archive, or of a type or taxonomy WordPress
/// registers but does not show in REST, is not ported; without
/// `nav_menu_options`, as on the default install, a menu adds no new
/// pages, and a location the theme's mods give a category is no menu's
/// and links from no category.
#[test]
fn menu_items_not_ported_and_plain_menus() {
    let state = concat!(
        "INSERT INTO `wp_options` VALUES (9801,'theme_mods_twentytwentyfive',",
        "'a:1:{s:18:\"nav_menu_locations\";a:1:{s:7:\"primary\";i:1;}}','auto');\n",
        "INSERT INTO `wp_terms` VALUES (870,'Plain','plain',0);\n",
        "INSERT INTO `wp_term_taxonomy` VALUES (870,870,'nav_menu','',0,0);\n",
        "INSERT INTO `wp_posts` VALUES ",
        "(871,1,'2026-11-07 10:00:00','2026-11-07 10:00:00','','','','publish','closed','closed','','871','','','2026-11-07 10:00:00','2026-11-07 10:00:00','',0,'',1,'nav_menu_item','',0),",
        "(872,1,'2026-11-07 10:00:00','2026-11-07 10:00:00','','','','publish','closed','closed','','872','','','2026-11-07 10:00:00','2026-11-07 10:00:00','',0,'',2,'nav_menu_item','',0),",
        "(873,1,'2026-11-07 10:00:00','2026-11-07 10:00:00','','','','publish','closed','closed','','873','','','2026-11-07 10:00:00','2026-11-07 10:00:00','',0,'',3,'nav_menu_item','',0);\n",
        "INSERT INTO `wp_postmeta` VALUES (871,871,'_menu_item_type','post_type_archive'),",
        "(872,872,'_menu_item_type','post_type'),(873,872,'_menu_item_object','revision'),",
        "(874,873,'_menu_item_type','taxonomy'),(875,873,'_menu_item_object','post_format');\n",
    );
    let content: &'static Content = Box::leak(Box::new(default_install_with(state)));
    for id in 871..=873 {
        let served = get(content, &format!("/?rest_route=/wp/v2/menu-items/{id}"));
        assert_eq!(served.status, 501, "{id}");
    }
    let served = get(
        content,
        "/?rest_route=/wp/v2/menus/870&_fields=locations,auto_add",
    );
    assert_eq!(served.body, br#"{"locations":[],"auto_add":false}"#);
    let category = get(content, "/?rest_route=/wp/v2/categories/1").body;
    assert!(!String::from_utf8_lossy(&category).contains("menu-location"));
}
