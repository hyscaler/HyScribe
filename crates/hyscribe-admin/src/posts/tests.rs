// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::TermFilter;
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::menu::MenuUser;
use crate::tests::{Role, first_difference};

/// The PHP oracle's posts list for `role`, under hs-admin's URLs.
fn golden(role: &str) -> String {
    crate::tests::page_golden(role, "edit.php")
}

/// The fixture's post 1.
pub(crate) fn hello() -> Post {
    Post {
        id: 1,
        author: 1,
        date: "2026-10-27 08:08:15".into(),
        date_gmt: "2026-10-27 08:08:15".into(),
        title: b"Hello world!".to_vec(),
        status: "publish".into(),
        name: b"hello-world".to_vec(),
        post_type: "post".into(),
        comment_count: 1,
        comment_status: "open".into(),
        ping_status: "open".into(),
        content: b"<!-- wp:paragraph -->\n<p>Welcome to WordPress. This is your first post. Edit or delete it, then start writing!</p>\n<!-- /wp:paragraph -->".to_vec(),
        ..Post::default()
    }
}

#[test]
fn rows_are_php_s() {
    for role in ["administrator", "editor", "author", "contributor"] {
        let page = golden(role);
        let open = "<tbody id=\"the-list\"\n\t\t\t\t>\n\t\t";
        let start = page.find(open).unwrap() + open.len();
        let rest = page.get(start..).unwrap();
        let php = rest.get(..rest.find("\t</tbody>").unwrap()).unwrap();
        let ours = row(&Role(role), &hello(), &columns(""), &[], "", false);
        assert!(ours == php, "{role}: {}", first_difference(php, &ours));
    }
}

#[test]
fn inline_editors_are_php_s() {
    for role in ["administrator", "editor", "author", "contributor"] {
        let page = golden(role);
        let start = page.find("\t\t<form method=\"get\">").unwrap();
        let rest = page.get(start..).unwrap();
        let php = rest
            .get(..rest.find("\t\t</form>\n").unwrap() + 10)
            .unwrap();
        let ours = inline_edit(&Role(role), &hello(), 7, "", false);
        assert!(ours == php, "{role}: {}", first_difference(php, &ours));
    }
}

#[test]
fn the_list_is_php_s() {
    for role in ["administrator", "editor", "author", "contributor"] {
        let page = golden(role);
        let start = page.find("<div class=\"wrap\">").unwrap();
        let rest = page.get(start..).unwrap();
        let php = rest
            .get(
                ..rest
                    .find("\n<div class=\"clear\"></div></div><!-- wpbody-content -->")
                    .unwrap(),
            )
            .unwrap();
        let listing = Listing {
            posts: vec![hello()],
            total: 1,
            counts: vec![("publish".into(), 1)],
            own: 1,
            sticky: 0,
            own_only: !Role(role).can("edit_others_posts"),
            user: 1,
            filter: String::new(),
            base: true,
            page: 1,
            pages: 1,
            orderby: None,
            order: None,
            mode: "list".into(),
            ..Listing::default()
        };
        let url = format!("{}/hs-admin/edit", crate::tests::SITE);
        let ours = list_content(&Role(role), &listing, &[], &url, "/hs-admin/edit");
        assert!(ours == php, "{role}: {}", first_difference(php, &ours));
    }
}

#[test]
fn the_page_is_php_s() {
    for role in ["administrator", "editor", "author", "contributor"] {
        let php = golden(role);
        let ours = page(edit(
            &Role(role),
            &ListArgs {
                base: true,
                ..ListArgs::default()
            },
            &format!("{}/hs-admin/edit", crate::tests::SITE),
            crate::tests::SITE,
            "/hs-admin/edit",
            &crate::admin_bar::Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        ));
        assert!(ours == php, "{role}: {}", first_difference(&php, &ours));
    }
}

/// Every page of `reference/posts-list/golden.sh`.
#[test]
fn variants_are_php_s() {
    let golden = include_str!("../../tests/golden/posts-list.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (variant, query) = f[0].split_once('?').unwrap_or((f[0], ""));
        let variant = if variant == "content" {
            "posts"
        } else {
            variant
        };
        let arg = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
                .map(String::from)
        };
        // The filters `main_query()` makes of the request.
        let category = |terms: Vec<u64>| TermFilter {
            taxonomy: "category".into(),
            terms,
            exclude: false,
        };
        let (terms, cat) = match query {
            "cat=301" => (vec![category(vec![301, 302])], "301"),
            "cat=0" | "cat=00" => (Vec::new(), "0"),
            "category_name=local" => (vec![category(vec![302])], "302"),
            "tag=php" | "post_format=aside" => {
                let (taxonomy, id) = if query == "tag=php" {
                    ("post_tag", 304)
                } else {
                    ("post_format", 305)
                };
                let filter = TermFilter {
                    taxonomy: taxonomy.into(),
                    terms: vec![id],
                    exclude: false,
                };
                (vec![filter], "")
            }
            _ => (Vec::new(), ""),
        };
        let m = arg("m").unwrap_or_default();
        // The search as `main_query()` reads it.
        let search =
            arg("s").map(|s| String::from_utf8(hyscribe_phpser::urldecode(s.as_bytes())).unwrap());
        let sentence = search.clone().unwrap_or_default();
        let filters = PostQuery {
            search: match &search {
                Some(s) if arg("sentence").is_some() => vec![s.clone()],
                Some(s) => hyscribe_render::search_terms(s),
                None => Vec::new(),
            },
            exact: arg("exact").is_some(),
            search_count: match &search {
                Some(_) if arg("sentence").is_some() => 1,
                Some(s) => hyscribe_render::search_count(s),
                None => 1,
            },
            sentence,
            terms,
            year: m.get(..4).and_then(|y| y.parse().ok()),
            month: m.get(4..6).and_then(|m| m.parse().ok()),
            ..PostQuery::default()
        };
        let flag = |name| arg(name).is_some_and(|v| v == "1");
        let args = ListArgs {
            status: arg("post_status").unwrap_or_default(),
            orderby: arg("orderby"),
            order: arg("order"),
            paged: arg("paged"),
            search,
            mode: arg("mode"),
            settings: (variant == "excerpt").then(|| "posts_list_mode=excerpt".to_string()),
            author: arg("author"),
            all_posts: arg("all_posts"),
            show_sticky: flag("show_sticky"),
            m,
            post_format: arg("post_format").unwrap_or_default(),
            cat: cat.into(),
            filters,
            base: query.is_empty() || query.starts_with("paged=") || query.starts_with("mode="),
        };
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        // The request's path and query, slashed as `wp_magic_quotes()` does.
        let path = if query.is_empty() {
            "/hs-admin/edit".to_string()
        } else {
            let path = format!("/hs-admin/edit?{query}");
            String::from_utf8(hyscribe_phpser::addslashes(path.as_bytes())).unwrap()
        };
        // The canonical URL, rebuilt as `remove_query_arg()` rebuilds it.
        let url = format!("{}{path}", crate::tests::SITE);
        let canonical = hyscribe_phpser::remove_query_arg(&[b"message"], url.as_bytes());
        let ours = edit(
            &Role(role),
            &args,
            &String::from_utf8(canonical).unwrap(),
            crate::tests::SITE,
            &path,
            &crate::admin_bar::Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        );
        let (php, ours) = match recorded.strip_prefix("302\t") {
            Some(to) => (hs_admin_page(to), redirect(ours)),
            None => (hs_admin_page(&recorded), page(ours)),
        };
        if ours != php {
            failures.push(format!("{role}?{query}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The page of an answer, empty for any other.
fn page(answer: Option<Answer>) -> String {
    match answer {
        Some(Answer::Page(page)) => page,
        _ => String::new(),
    }
}

/// Where an answer redirects, empty for any other.
fn redirect(answer: Option<Answer>) -> String {
    match answer {
        Some(Answer::Redirect(to)) => to,
        _ => String::new(),
    }
}

/// A user who may not edit posts is refused.
#[test]
fn subscribers_may_not_list_posts() {
    let page = edit(
        &Role("subscriber"),
        &ListArgs::default(),
        &format!("{}/hs-admin/edit", crate::tests::SITE),
        crate::tests::SITE,
        "/hs-admin/edit",
        &crate::admin_bar::Browser {
            user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
            mobile_hint: None,
        },
    );
    assert!(page.is_none());
}

/// `touch_time()` of a draft never dated shows the time now.
#[test]
fn undated_drafts_take_the_time_now() {
    let draft = Post {
        status: "draft".into(),
        date: "2026-01-02 03:04:05".into(),
        date_gmt: "0000-00-00 00:00:00".into(),
        ..Post::default()
    };
    let fields = touch_time(&draft, Zone::Utc, 1_794_500_000);
    for field in [
        "value=\"11\" data-text=\"Nov\"  selected='selected'",
        "name=\"jj\" value=\"12\"",
        "name=\"aa\" value=\"2026\"",
        "name=\"hh\" value=\"16\"",
        "name=\"mn\" value=\"13\"",
        "name=\"ss\" value=\"20\"",
    ] {
        assert!(fields.contains(field), "{field}: {fields}");
    }
    let empty = Post {
        date_gmt: String::new(),
        ..draft
    };
    assert_eq!(touch_time(&empty, Zone::Utc, 1_794_500_000), fields);
}

/// Moving to the Trash is offered only to those who may delete posts.
#[test]
fn the_trash_needs_delete_posts() {
    let listing = Listing {
        posts: vec![hello()],
        total: 1,
        counts: Vec::new(),
        own: 0,
        sticky: 0,
        own_only: false,
        user: 1,
        filter: String::new(),
        base: true,
        page: 1,
        pages: 1,
        orderby: None,
        order: None,
        mode: "list".into(),
        ..Listing::default()
    };
    let nav = |role| tablenav(&Role(role), &listing, "bottom", &[], String::new());
    assert!(nav("contributor").contains("value=\"trash\""));
    assert!(!nav("nobody").contains("value=\"trash\""));
}

/// A post scheduled for this very second is not missed yet.
#[test]
fn a_post_due_now_is_scheduled() {
    let post = Post {
        status: "future".into(),
        date: "2026-11-12 16:13:20".into(),
        ..Post::default()
    };
    assert_eq!(
        column_date(&Role("administrator"), &post),
        "Scheduled<br />2026/11/12 at 4:13 pm"
    );
}

/// A dated draft keeps its date in Quick Edit.
#[test]
fn dated_drafts_keep_their_date() {
    let draft = Post {
        status: "draft".into(),
        date: "2026-01-02 03:04:05".into(),
        date_gmt: "2026-01-02 03:04:05".into(),
        ..Post::default()
    };
    assert!(touch_time(&draft, Zone::Utc, 1_794_500_000).contains("name=\"jj\" value=\"02\""));
}

/// A user without posts gets no Mine link.
#[test]
fn no_posts_no_mine_link() {
    let listing = Listing {
        posts: Vec::new(),
        total: 1,
        counts: vec![("publish".into(), 1)],
        own: 0,
        sticky: 0,
        own_only: false,
        user: 1,
        filter: String::new(),
        base: true,
        page: 1,
        pages: 1,
        orderby: None,
        order: None,
        mode: "list".into(),
        ..Listing::default()
    };
    assert!(
        status_views(&listing)
            .iter()
            .all(|view| view.class != "mine")
    );
}

/// Only private posts are limited to the user's own.
#[test]
fn only_private_posts_are_the_user_s_own() {
    let statuses = all_statuses(&Role("author"));
    let limited: Vec<&str> = statuses
        .iter()
        .filter(|filter| filter.author == Some(1))
        .map(|filter| filter.status.as_str())
        .collect();
    assert_eq!(limited, ["private"]);
}

/// An untitled post's excerpt stays hidden behind its password.
#[test]
fn protected_posts_show_no_excerpt() {
    let protected = Post {
        password: b"secret".to_vec(),
        excerpt: b"Hidden words".to_vec(),
        ..hello()
    };
    let untitled = Post {
        title: Vec::new(),
        ..protected.clone()
    };
    assert_eq!(
        no_title_excerpt(&Role("administrator"), &untitled, false),
        ""
    );
    let open = Post {
        password: Vec::new(),
        ..untitled
    };
    assert_eq!(
        no_title_excerpt(&Role("administrator"), &open, false),
        "Hidden words"
    );
    // The extended view shows the excerpt under the title instead.
    assert_eq!(no_title_excerpt(&Role("administrator"), &open, true), "");
}

/// `WP_Query`'s `author`: cleaned, sorted, and leaving out wins.
#[test]
fn authors_are_read_as_wp_query_reads_them() {
    assert_eq!(author_ids("abc"), None);
    assert_eq!(author_ids(""), None);
    assert_eq!(author_ids("00"), None);
    assert_eq!(author_ids("3, 1x,3"), Some(vec![1, 3]));
    assert_eq!(author_lists(&[1, 3]), (vec![1, 3], Vec::new()));
    assert_eq!(author_lists(&[0, 1]), (Vec::new(), vec![0]));
    assert_eq!(author_lists(&[-2, 1]), (Vec::new(), vec![2]));
}

/// The All view is current only for a request with no filter.
#[test]
fn the_status_is_read_as_php_reads_it() {
    let listing = Listing {
        posts: Vec::new(),
        total: 0,
        counts: Vec::new(),
        own: 0,
        sticky: 0,
        own_only: false,
        user: 1,
        filter: String::new(),
        base: false,
        page: 1,
        pages: 1,
        orderby: None,
        order: None,
        mode: "list".into(),
        ..Listing::default()
    };
    assert!(!status_views(&listing)[0].current);
}

/// The test site's relevance follows `parse_search_order()`'s ranks.
#[test]
fn the_test_site_ranks_searches() {
    use super::content::rank;
    let post = |title: &str, excerpt: &str, content: &str| Post {
        title: title.as_bytes().to_vec(),
        excerpt: excerpt.as_bytes().to_vec(),
        content: content.as_bytes().to_vec(),
        ..Post::default()
    };
    let query = |search: &[&str], sentence: &str| PostQuery {
        search: search.iter().map(|term| (*term).to_string()).collect(),
        sentence: sentence.into(),
        search_count: search.len(),
        ..PostQuery::default()
    };
    let two = query(&["red", "fox"], "red fox");
    let cases = [
        (post("A red fox", "", ""), 1),
        (post("Fox, red", "", ""), 2),
        (post("Red", "", ""), 3),
        (post("", "A red fox", ""), 4),
        (post("", "", "A red fox"), 5),
        (post("", "", ""), 6),
    ];
    for (post, rank_of) in cases {
        assert_eq!(rank(&post, &two), rank_of);
    }
    let left_out = query(&["red", "-fox"], "red -fox");
    assert_eq!(rank(&post("", "red -fox", ""), &left_out), 6);
    let one = query(&["red"], "red");
    assert_eq!(
        (
            rank(&post("Red", "", ""), &one),
            rank(&post("", "", ""), &one)
        ),
        (0, 1)
    );
}

/// A search is ranked first unless another order is asked; `relevance`
/// and `0` ask for none.
#[test]
fn searches_rank_unless_ordered_otherwise() {
    let query = PostQuery {
        search: vec!["red".into()],
        ..PostQuery::default()
    };
    let first = |orderby: Option<&str>| {
        let args = ListArgs {
            orderby: orderby.map(String::from),
            ..ListArgs::default()
        };
        sort(&args, &query).first().map(|(column, _)| *column)
    };
    for orderby in [None, Some("0"), Some("relevance")] {
        assert_eq!(first(orderby), Some(OrderBy::Relevance), "{orderby:?}");
    }
    assert_eq!(first(Some("title")), Some(OrderBy::Title));
}

/// `get_search_query()` of a search over 1600 bytes is empty.
#[test]
fn long_searches_show_empty() {
    let listing = Listing {
        posts: vec![hello()],
        total: 1,
        pages: 1,
        page: 1,
        search: Some("x".repeat(1601)),
        mode: "list".into(),
        ..Listing::default()
    };
    let url = format!("{}/hs-admin/edit", crate::tests::SITE);
    let page = list_content(
        &Role("administrator"),
        &listing,
        &[],
        &url,
        "/hs-admin/edit",
    );
    assert!(page.contains("Search results for: <strong></strong>"));
}

/// `get_user_setting()`: the cookie when sent, cleaned and needing an `=`
/// after its first character; else the stored option.
#[test]
fn user_settings_are_read_as_php_reads_them() {
    let stored = Role("settings:administrator");
    let mode = |cookie| user_setting(&stored, cookie, "posts_list_mode");
    assert_eq!(mode(None).as_deref(), Some("excerpt"));
    assert_eq!(mode(Some("posts_list_mode=list")).as_deref(), Some("list"));
    assert_eq!(mode(Some("=x&posts_list_mode=list")), None);
    assert_eq!(mode(Some("posts_list_mode")), None);
    assert_eq!(
        mode(Some("posts_list_mode=ex cerpt!")).as_deref(),
        Some("excerpt")
    );
    assert_eq!(
        user_setting(&Role("administrator"), None, "posts_list_mode"),
        None
    );
}
