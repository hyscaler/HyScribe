// SPDX-License-Identifier: AGPL-3.0-or-later

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
        let variant = match f[0].split_once('?').map_or(f[0], |(variant, _)| variant) {
            "content" => "comments",
            "paged" => "cpaged",
            "empty" => "cempty",
            "hidecols" => "chidden",
            other => other,
        };
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        let path = "/hs-admin/edit-comments";
        let ours = edit_comments(
            &Role(role),
            None,
            &format!("{SITE}{path}"),
            SITE,
            path,
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
            Answer::Redirect(to) => (recorded, to),
        };
        if ours != php {
            failures.push(format!("{role}: {}", first_difference(&php, &ours)));
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
