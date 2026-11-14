// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers on the templates' routes and a site's global
//! styles post, `tests/golden/templates.tsv` over the default install and
//! `templates-custom.tsv` over `reference/rest/templates-custom.sql`,
//! replayed for every identity.

use std::sync::OnceLock;

use hyscribe_phpser::json_encode;
use hyscribe_testkit::unhex;

use super::content::hooked;
use super::query::{PART_FILES, hierarchy};
use super::theme;
use crate::response::{list, text};
use crate::tests::content::{Content, default_install, default_install_with};
use crate::tests::{Case, cases_of, kept, serve_over};

/// Each case of `reference/rest/hierarchy-golden.php`.
#[test]
fn hierarchies_are_php_s() {
    let golden = include_str!("../../tests/golden/hierarchy.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (slug, prefix) = (unhex(fields[0]), unhex(fields[2]));
        let ours = hierarchy(&slug, fields[1] == "1", &prefix);
        let ours = list(ours.into_iter().map(text).collect());
        let what = String::from_utf8_lossy(&slug).into_owned();
        assert_eq!(json_encode(&ours).unwrap(), fields[3].as_bytes(), "{what}");
    }
    assert_eq!(golden.lines().count(), 44);
}

/// Each of the theme's parts reads the same wrapped in a template part
/// block, as PHP hooks it, or not.
#[test]
fn theme_parts_need_no_wrapper() {
    for slug in PART_FILES {
        let (content, _) = theme().part(slug.as_bytes()).unwrap();
        let theme = b"twentytwentyfive";
        assert_eq!(
            hooked(content, true, theme),
            hooked(content, false, theme),
            "{slug}"
        );
    }
}

/// The theme footer's credit, which names HyScribe, as JSON.
const CREDIT: [&str; 2] = [
    "Designed with <a href=\\\"https:\\/\\/wordpress.org\\\" rel=\\\"nofollow\\\">WordPress<\\/a>",
    "Designed with <a href=\\\"https:\\/\\/github.com\\/hyscaler\\/HyScribe\\\" rel=\\\"nofollow\\\">HyScribe<\\/a>",
];

/// Templates and global styles whose answers are not ported.
const UNPORTED: &str = concat!(
    "INSERT INTO `wp_terms` VALUES (761,'twentytwentyfive','twentytwentyfive',0);\n",
    "INSERT INTO `wp_term_taxonomy` VALUES (761,761,'wp_theme','',0,4);\n",
    "INSERT INTO `wp_posts` VALUES ",
    "(761,1,'2026-11-10 10:00:00','2026-11-10 10:00:00','x','','','publish','closed','closed','','page-about','','','2026-11-10 10:00:00','2026-11-10 10:00:00','',0,'',0,'wp_template','',0),",
    "(762,1,'2026-11-10 10:00:00','2026-11-10 10:00:00','x','Plugged','','publish','closed','closed','','plugged','','','2026-11-10 10:00:00','2026-11-10 10:00:00','',0,'',0,'wp_template','',0),",
    "(763,1,'2026-11-10 10:00:00','2026-11-10 10:00:00','<!-- wp:pattern {\"slug\":\"core/query-standard-posts\"} /-->','Cored','','publish','closed','closed','','cored','','','2026-11-10 10:00:00','2026-11-10 10:00:00','',0,'',0,'wp_template','',0),",
    "(764,1,'2026-11-10 10:00:00','2026-11-10 10:00:00','{\"version\":3,\"isGlobalStylesUserThemeJSON\":true,\"styles\":{\"background\":{\"backgroundImage\":{\"url\":\"x.png\"}}}}','Pictured','','publish','closed','closed','','wp-global-styles-twentytwentyfive','','','2026-11-10 10:00:00','2026-11-10 10:00:00','',0,'',0,'wp_global_styles','',0);\n",
    "INSERT INTO `wp_term_relationships` VALUES (761,761,0),(762,761,0),(763,761,0),(764,761,0);\n",
    "INSERT INTO `wp_postmeta` VALUES (761,762,'origin','plugin');\n",
);

fn custom() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        default_install_with(include_str!(
            "../../../../reference/rest/templates-custom.sql"
        ))
    })
}

fn replay(golden: &str, content: &'static Content) -> usize {
    let cases = cases_of(golden);
    for case in &cases {
        let served = serve_over(content, case);
        let what = format!(
            "{} {} {} {}",
            case.who, case.method, case.target, case.flags
        );
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(kept(&served.headers), php, "{what}");
        let body = String::from_utf8_lossy(&case.body).replace(CREDIT[0], CREDIT[1]);
        assert_eq!(String::from_utf8_lossy(&served.body), body, "{what}");
    }
    cases.len()
}

#[test]
fn answers_match_php_over_the_default_install() {
    let golden = include_str!("../../tests/golden/templates.tsv");
    assert_eq!(replay(golden, default_install()), 6 * 85);
}

#[test]
fn answers_match_php_over_customised_templates() {
    let golden = include_str!("../../tests/golden/templates-custom.tsv");
    assert_eq!(replay(golden, custom()), 6 * 42);
}

/// A template titled after the page its slug names, one a plugin added,
/// one with a pattern of core's, and the lists that hold them, a theme file
/// found by `realpath()`, an empty lookup's fields, global styles naming a
/// theme file, a route of the templates' not ported, and every write are
/// answered 501.
#[test]
fn what_is_not_ported_answers_501() {
    let content: &'static Content = Box::leak(Box::new(default_install_with(UNPORTED)));
    let targets = [
        (
            "GET",
            "/?rest_route=/wp/v2/templates/twentytwentyfive//page-about",
        ),
        (
            "GET",
            "/?rest_route=/wp/v2/templates/twentytwentyfive//plugged",
        ),
        (
            "GET",
            "/?rest_route=/wp/v2/templates/twentytwentyfive//cored",
        ),
        (
            "GET",
            "/?rest_route=/wp/v2/templates/twentytwentyfive%252F%252Fa%252Eb",
        ),
        (
            "GET",
            "/?rest_route=/wp/v2/templates/lookup&slug=&_fields=id",
        ),
        ("GET", "/?rest_route=/wp/v2/global-styles/764"),
        ("GET", "/?rest_route=/wp/v2/templates"),
        (
            "GET",
            "/?rest_route=/wp/v2/templates/lookup&slug=page-about",
        ),
        (
            "GET",
            "/?rest_route=/wp/v2/templates/twentytwentyfive//index/revisions",
        ),
        ("POST", "/?rest_route=/wp/v2/templates"),
        (
            "DELETE",
            "/?rest_route=/wp/v2/template-parts/twentytwentyfive//header",
        ),
    ];
    for (method, target) in targets {
        let case = Case {
            who: "administrator".into(),
            method: method.into(),
            target: target.into(),
            flags: String::new(),
            status: 501,
            headers: Vec::new(),
            body: Vec::new(),
        };
        let served = serve_over(content, &case);
        assert_eq!(served.status, 501, "{method} {target}");
        assert!(served.headers.is_empty() && served.body.is_empty());
    }
}
