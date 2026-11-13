// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{addslashes, remove_query_arg, urldecode};
use hyscribe_testkit::{hs_admin_page, settled_page, unhex};

use super::*;
use crate::tests::{Role, SITE, first_difference};

/// Every page of `reference/comments-list/golden.sh`; a refusal is `403`.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/comments-list.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (variant, query) = f[0].split_once('?').unwrap_or((f[0], ""));
        let arg = |name: &str| {
            query
                .split('&')
                .find_map(|pair| Some(pair.strip_prefix(name)?.strip_prefix('=')?.to_string()))
        };
        let decoded = |name: &str| {
            arg(name).map(|value| String::from_utf8(urldecode(value.as_bytes())).unwrap())
        };
        let args = CommentArgs {
            status: decoded("comment_status"),
            user_id: decoded("user_id"),
            paged: decoded("paged"),
            search: decoded("s"),
            kind: decoded("comment_type"),
            orderby: decoded("orderby"),
            order: decoded("order"),
            mode: decoded("mode"),
            clauses: comment_order(
                &decoded("orderby").unwrap_or_default(),
                &decoded("order").unwrap_or_default(),
            )
            .unwrap(),
        };
        let variant = match variant {
            "content" => "comments",
            "paged" => "cpaged",
            "empty" => "cempty",
            "hidecols" => "chidden",
            other => other,
        };
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        // The request's path and query, slashed as `wp_magic_quotes()` does.
        let path = if query.is_empty() {
            "/hs-admin/edit-comments".to_string()
        } else {
            format!("/hs-admin/edit-comments?{query}")
        };
        let path = String::from_utf8(addslashes(path.as_bytes())).unwrap();
        let ours = edit_comments(
            &Role(role),
            &args,
            None,
            &String::from_utf8(remove_query_arg(
                &[b"message"],
                format!("{SITE}{path}").as_bytes(),
            ))
            .unwrap(),
            SITE,
            &path,
            &crate::admin_bar::Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        );
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let (php, ours) = match ours {
            Answer::Page(page) => (settled_page(&hs_admin_page(&recorded)), settled_page(&page)),
            Answer::Denied(message) => {
                let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
                (
                    recorded,
                    format!("403\t{}", String::from_utf8(page).unwrap()),
                )
            }
            Answer::Redirect(to) => (hs_admin_page(&recorded), format!("302\t{to}")),
        };
        if ours != php {
            failures.push(format!("{role}?{query}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// In the extended view the row actions always show.
#[test]
fn the_extended_view_shows_row_actions() {
    let path = "/hs-admin/edit-comments";
    let browser = crate::admin_bar::Browser::default();
    let settings = Some("posts_list_mode=excerpt");
    let Answer::Page(page) = edit_comments(
        &Role("administrator"),
        &CommentArgs::default(),
        settings,
        &format!("{SITE}{path}"),
        SITE,
        path,
        &browser,
    ) else {
        panic!("no page");
    };
    assert!(
        page.contains("<div class=\"row-actions visible\">") && page.contains("table-view-excerpt")
    );
}

/// A page asks for up to eight comments more, for the rows acted on.
#[test]
fn a_page_asks_for_spare_comments() {
    let path = "/hs-admin/edit-comments";
    let browser = crate::admin_bar::Browser::default();
    edit_comments(
        &Role("administrator"),
        &CommentArgs::default(),
        None,
        &format!("{SITE}{path}"),
        SITE,
        path,
        &browser,
    );
    assert_eq!(crate::tests::COMMENTS_ASKED.get(), 28);
}

/// The author URL drops only a scheme and the `www.` after one, and is
/// cut past 50 characters.
#[test]
fn author_urls_are_shortened() {
    assert_eq!(strip_scheme(b"www.example.test/"), b"www.example.test");
    assert_eq!(strip_scheme(b"HTTPS://www.example.test//"), b"example.test");
    let role = Role("comments:administrator");
    let columns = columns();
    let listing = Listing {
        site: &role,
        columns: &columns,
        hidden: &[],
        excerpt: false,
        view: "all",
    };
    let mut row = crate::dashboard::tests::hello_comment(true);
    let fifty = "a".repeat(50);
    row.comment.author_url = format!("http://{fifty}").into_bytes();
    assert!(
        listing
            .column_author(&row)
            .contains(&format!(">{fifty}</a>"))
    );
}

/// All is the approved and pending comments.
#[test]
fn all_counts_approved_and_pending() {
    let counts = crate::dashboard::CommentCounts {
        approved: 3,
        moderated: 2,
        ..Default::default()
    };
    assert_eq!(counts.all(), 5);
}

/// In the spam view an author's IP links to their other spam.
#[test]
fn the_spam_view_links_ips_to_spam() {
    let role = Role("comments:administrator");
    let columns = columns();
    let listing = Listing {
        site: &role,
        columns: &columns,
        hidden: &[],
        excerpt: false,
        view: "spam",
    };
    let mut row = crate::dashboard::tests::hello_comment(true);
    row.comment.author_ip = b"192.0.2.9".to_vec();
    let author = listing.column_author(&row);
    assert!(
        author.contains("&#038;mode=detail&#038;comment_status=spam\">"),
        "{author}"
    );
}

/// `orderby` as `WP_Comment_Query` reads it.
#[test]
fn orders_are_parsed_as_php_parses_them() {
    assert_eq!(comment_order("none", "asc"), Some(Vec::new()));
    assert_eq!(
        comment_order("", "ASC"),
        Some(vec![("comment_date_gmt", false)])
    );
    assert_eq!(comment_order("comment__in", ""), None);
    // An ID anywhere leaves out the ID clause added for a stable order.
    assert_eq!(
        comment_order("comment_ID,,0,comment_author", "asc"),
        Some(vec![("comment_ID", false), ("comment_author", false)])
    );
    assert_eq!(
        comment_order("comment_karma\tcomment_date", "asc"),
        Some(vec![
            ("comment_karma", false),
            ("comment_date", false),
            ("comment_ID", false)
        ])
    );
}

/// A type's view links are built as `add_query_arg()` builds them, as
/// PHP printed them for `a&b`.
#[test]
fn view_links_keep_the_type_as_php_does() {
    let views = comment_views(&Role("administrator"), "all", "a&b");
    assert!(
        views[0]
            .url
            .ends_with("edit-comments?comment_type=a&b&comment_status=all")
    );
    assert!(
        views[1]
            .url
            .ends_with("?comment_type=a&b&comment_status=mine&user_id=1")
    );
}
