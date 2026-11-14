// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::admin_bar::Browser;
use crate::tests::Role;

/// The PHP oracle's dashboard for `role`, under hs-admin's URLs.
fn golden(role: &str) -> String {
    crate::tests::page_golden(role, "index.php").replace("%auto-draft%", "4")
}

/// The part of `page` from `start` to `end`, without `end`.
fn between<'a>(page: &'a str, start: &str, end: &str) -> &'a str {
    let Some(at) = page.find(start) else {
        return "";
    };
    let rest = page.get(at..).unwrap();
    rest.get(..rest.find(end).unwrap()).unwrap()
}

#[test]
fn the_welcome_panel_is_php_s() {
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        let php = golden(role);
        let php = between(
            &php,
            "\t<div id=\"welcome-panel\"",
            "\t<div id=\"dashboard-widgets-wrap\">",
        );
        assert_eq!(welcome_panel(&Role(role)), php, "{role}");
    }
}

#[test]
fn meta_boxes_are_php_s() {
    let php = golden("administrator");
    let php = between(
        &php,
        "<div id=\"dashboard_right_now\"",
        "\t<div class=\"main\">",
    );
    let ours = postbox("dashboard_right_now", "At a Glance", false, false, "");
    assert_eq!(ours, format!("{php}</div>\n</div>\n"));
    let hidden = postbox("x", "<b>A</b> &amp; B", false, true, "y");
    assert!(hidden.starts_with(
        "<div id=\"x\" class=\"postbox  hide-if-js\"  role=\"region\" aria-label=\"A &amp; B\">"
    ));
    assert!(hidden.ends_with("<div class=\"inside\">\ny</div>\n</div>\n"));
}

/// The welcome panel's option: 0 hides it, 2 shows it to the admin email's
/// owner only.
#[test]
fn hidden_welcome_panels() {
    assert!(!welcome_hidden(&Role("administrator")));
    assert!(welcome_hidden(&Role("layout:administrator")));
    assert!(welcome_hidden(&Role("owner:administrator")));
    assert!(!welcome_hidden(&Role("owned:administrator")));
}

#[test]
fn site_health_is_php_s() {
    let php = golden("administrator");
    let php = between(
        &php,
        "\t<div class=\"health-check-widget\">",
        "</div>\n</div>\n<div id=\"dashboard_right_now\"",
    );
    assert_eq!(site_health(&Role("administrator")), php);
}

#[test]
fn at_a_glance_is_php_s() {
    for role in ["administrator", "editor", "author", "contributor"] {
        let php = golden(role);
        let php = between(
            &php,
            "\t<div class=\"main\">",
            "</div>\n</div>\n<div id=\"dashboard_activity\"",
        );
        assert_eq!(right_now(&Role(role)), php, "{role}");
    }
}

#[test]
fn recent_posts_are_php_s() {
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        let php = golden(role);
        let php = between(
            &php,
            "<div id=\"published-posts\"",
            "<div id=\"latest-comments\"",
        );
        let ours = recent_posts(
            &Role(role),
            &Role(role).recent_posts("publish"),
            "Recently Published",
            "published-posts",
        );
        assert_eq!(ours, php, "{role}");
    }
}

/// The default install's comment, with what its row needs.
pub(crate) fn hello_comment(can_edit: bool) -> RecentComment {
    let version = hyscribe_testkit::default_install_versions().pop().unwrap();
    let dump = hyscribe_testkit::default_install(&version).unwrap();
    let row = dump.table("wp_comments").next().unwrap();
    let text = |i: usize| match &row[i] {
        hyscribe_testkit::SqlValue::Text(text) => text.clone(),
        _ => Vec::new(),
    };
    RecentComment {
        comment: hyscribe_model::Comment {
            id: 1,
            post_id: 1,
            author: text(2),
            author_email: text(3),
            author_url: text(4),
            author_ip: text(5),
            date: String::from_utf8(text(6)).unwrap(),
            content: text(8),
            comment_type: text(12),
            ..hyscribe_model::Comment::default()
        },
        status: "approved",
        post: Some(crate::posts::tests::hello()),
        user: None,
        can_edit,
        link: "http://127.0.0.1:8081/?p=1#comment-1".into(),
        permalink: "http://127.0.0.1:8081/?p=1".into(),
    }
}

#[test]
fn comment_rows_are_php_s() {
    for (role, can_edit) in [("administrator", true), ("contributor", false)] {
        let php = golden(role);
        let php = between(&php, "\n\t\t<li id=\"comment-1\"", "</ul>");
        let ours = comment_row(&Role(role), &hello_comment(can_edit), 0, None);
        assert!(
            ours == php,
            "{role}{}",
            crate::tests::first_difference(php, &ours)
        );
    }
}

#[test]
fn activity_is_php_s() {
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        let php = golden(role);
        let php = between(
            &php,
            "<div id=\"activity-widget\">",
            "</div>\n</div>\n</div>",
        );
        let ours = activity(&Role(role), &data(&Role(role)));
        assert!(
            ours == php,
            "{role}{}",
            crate::tests::first_difference(php, &ours)
        );
    }
}

#[test]
fn quick_draft_is_php_s() {
    for role in ["administrator", "editor", "author", "contributor"] {
        let php = golden(role);
        let php = between(
            &php,
            "\n\t<form name=\"post\"",
            "</div>\n</div>\n<div id=\"dashboard_primary\"",
        );
        let ours = quick_press(&Role(role), "/hs-admin/index");
        assert!(
            ours == php,
            "{role}{}",
            crate::tests::first_difference(php, &ours)
        );
    }
}

#[test]
fn the_dashboard_is_php_s() {
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        let php = golden(role);
        let ours = dashboard(
            &Role(role),
            &crate::tests::request(
                &format!("{}/hs-admin/index", crate::tests::SITE),
                "/hs-admin/index",
                &crate::tests::BROWSER,
            ),
        )
        .unwrap();
        assert!(
            ours == php,
            "{role}{}",
            crate::tests::first_difference(&php, &ours)
        );
    }
}

/// Every case of `reference/dashboard/golden.sh`.
#[test]
fn dashboard_variants_are_php_s() {
    let golden = include_str!("../../tests/golden/dashboard.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(format!("{}:{}", f[0], f[1]).into_boxed_str());
        let php =
            hs_admin_page(&String::from_utf8(unhex(f[2])).unwrap()).replace("%auto-draft%", "4");
        let ours = dashboard(
            &Role(role),
            &crate::tests::request(
                &format!("{}/hs-admin/index", crate::tests::SITE),
                "/hs-admin/index",
                &crate::tests::BROWSER,
            ),
        )
        .unwrap();
        if ours != php {
            failures.push(format!(
                "{role}{}",
                crate::tests::first_difference(&php, &ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// What no golden reaches: an empty site, posts of today and tomorrow,
/// a phone, recommendations alone, and avatars turned off.
#[test]
fn other_states() {
    let empty = Role("empty:administrator");
    assert!(!right_now(&empty).contains("<li"));
    assert_eq!(
        activity(&empty, &data(&empty)),
        "<div id=\"activity-widget\"><div class=\"no-activity\"><p>No activity yet!</p></div></div>"
    );
    let phone = Browser {
        user_agent: b"Android",
        mobile_hint: None,
    };
    let page = dashboard(
        &empty,
        &Request {
            canonical: "http://h/",
            origin: "http://h",
            uri: "/",
            browser: &phone,
        },
    )
    .unwrap();
    assert!(page.contains("jquery-touch-punch"));
    assert!(!page.contains("list_args") && !page.contains("tinyMCEPreInit"));
    let dates = recent_posts(
        &Role("dates:administrator"),
        &Role("dates:administrator").recent_posts("publish"),
        "T",
        "x",
    );
    assert!(dates.contains("<span>Today, 8:00 am</span>") && dates.contains("(no title)</a>"));
    assert!(dates.contains("<span>Tomorrow, 8:00 am</span>"));
    assert!(site_health(&Role("some:administrator")).contains("there are still some things"));
    let row = comment_row(
        &Role("noavatars:administrator"),
        &hello_comment(false),
        1,
        None,
    );
    assert!(row.contains("class=\"dashboard-comment-wrap has-row-actions \">"));
}

/// Trackbacks and other types, comments with no post, nameless users, and
/// protected posts.
#[test]
fn comment_kinds() {
    let site = Role("administrator");
    let mut row = hello_comment(false);
    row.comment.comment_type = b"trackback".to_vec();
    assert!(comment_row(&site, &row, 0, None).contains("<strong>Trackback</strong> on <a"));
    row.comment.comment_type = b"web mention".to_vec();
    row.post = None;
    assert!(
        comment_row(&site, &row, 0, None)
            .contains("<strong>Web Mention</strong> <span class=\"approve\">")
    );
    let mut row = hello_comment(false);
    row.comment.comment_type = Vec::new();
    row.comment.author = Vec::new();
    row.comment.user_id = 1;
    row.user = Some(hyscribe_model::User {
        display_name: b"Ann".to_vec(),
        ..hyscribe_model::User::default()
    });
    row.post.as_mut().unwrap().password = b"x".to_vec();
    let global = row.post.clone();
    let html = comment_row(&site, &row, 0, global.as_ref());
    assert!(
        html.contains("class=\"comment byuser comment-author-1 bypostauthor"),
        "{html}"
    );
    assert!(html.contains("rel=\"ugc external nofollow\">Ann</a></cite>"));
    assert!(html.contains("<blockquote><p>Password protected</p></blockquote>"));
    row.user = None;
    assert!(comment_row(&site, &row, 0, None).contains(">Anonymous</a>"));
    assert_eq!(ucwords(b"a b\tc\nd"), b"A B\tC\nD");
}

/// Fewer drafts than the list holds, plural counts, a sorted widget the
/// user may not see, comments by users on others' posts and on untitled
/// posts.
#[test]
fn smaller_cases() {
    let dates = Role("dates:administrator");
    assert!(right_now(&dates).contains(">2 Published posts</a>"));
    let drafts = recent_drafts(&dates);
    assert!(
        drafts.starts_with("<div class=\"drafts\"><h2 class=\"hide-if-no-js\">"),
        "{drafts}"
    );
    let page = dashboard(
        &Role("layout:subscriber"),
        &Request {
            canonical: "http://h/",
            origin: "http://h",
            uri: "/",
            browser: &Browser::default(),
        },
    )
    .unwrap();
    assert!(!page.contains("dashboard_quick_press"));
    let mut row = hello_comment(false);
    row.comment.user_id = 2;
    row.user = Some(hyscribe_model::User::default());
    row.post.as_mut().unwrap().title = Vec::new();
    let html = comment_row(&Role("administrator"), &row, 0, None);
    assert!(html.contains("class=\"comment byuser comment-author-2 even"));
    assert!(html.contains("on <a href=\"http://127.0.0.1:8081/?p=1\">(no title)</a>"));
}

/// Mutation cases: a pending comment alone, the welcome panel of another
/// owner's site, published posts without comments, three drafts, and no
/// reply editor without comments.
#[test]
fn edges() {
    assert!(right_now(&Role("pending:administrator")).contains(">1 Comment in moderation</a>"));
    assert!(welcome_panel(&Role("owner:administrator")).contains("class=\"welcome-panel hidden\""));
    assert!(welcome_panel(&Role("administrator")).contains("class=\"welcome-panel\""));
    let dates = Role("dates:administrator");
    assert!(!activity(&dates, &data(&dates)).contains("no-activity"));
    assert!(!recent_drafts(&dates).contains("view-all"));
    let page = dashboard(
        &Role("empty:administrator"),
        &Request {
            canonical: "http://h/",
            origin: "http://h",
            uri: "/",
            browser: &Browser::default(),
        },
    )
    .unwrap();
    // The reply editor's link dialog, which only its form loads.
    assert!(!page.contains("id=\"wplink-js"));
}

/// What the Activity widget lists for `site`.
fn data(site: &Role) -> Activity {
    Activity {
        future: site.recent_posts("future"),
        published: site.recent_posts("publish"),
        comments: site.recent_comments(None),
    }
}

/// Fixes from the review: the excerpt's `&`, a box sorted twice into one
/// column, protocol-relative author links, and the referrer field.
#[test]
fn review_cases() {
    let site = Role("administrator");
    let mut row = hello_comment(false);
    row.comment.content = b"Tom & Jerry".to_vec();
    row.comment.author_url = b"//127.0.0.1:8081/x".to_vec();
    let html = comment_row(&site, &row, 0, None);
    assert!(html.contains("<p>Tom &#038; Jerry</p>"));
    assert!(html.contains("rel=\"ugc external nofollow\""));
    let twice = Role("twice:administrator");
    let widgets = sorted(
        &twice,
        "meta-box-order_dashboard",
        widgets(&twice, &data(&twice), "/"),
    );
    assert!(!widgets.iter().any(|w| w.id == "dashboard_activity"));
    assert!(
        widgets
            .iter()
            .any(|w| w.id == "dashboard_right_now" && w.context == "side")
    );
    let form = quick_press(&site, "/hs-admin/index?a=1&b=2&_wp_http_referer=x");
    assert!(form.contains("name=\"_wp_http_referer\" value=\"/hs-admin/index?a=1&#038;b=2\""));
}

/// Tomorrow is the next calendar day, on a day of 25 hours too.
#[test]
fn tomorrow_across_a_clock_change() {
    let site = Role("dst:administrator");
    let html = recent_posts(&site, &site.recent_posts("future"), "S", "f");
    assert!(html.contains("<span>Tomorrow, 9:00 am</span>"), "{html}");
}

/// A comment's state and its `comment_approved` map both ways.
#[test]
fn comment_states_map_both_ways() {
    for approved in ["1", "0", "spam", "trash"] {
        assert_eq!(approved_of(comment_status(approved)), approved);
    }
    assert_eq!(comment_status("post-trashed"), "");
}
