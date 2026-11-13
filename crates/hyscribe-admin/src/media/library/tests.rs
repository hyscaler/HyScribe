// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::admin_bar::Browser;
use crate::media::NewArgs;
use crate::tests::{MEDIA_QUERIES, Role, SITE};

/// `upload.php` for `query` as `role` sees it.
fn answer(role: &'static str, query: &str) -> Option<Answer> {
    let uri = format!("/hs-admin/upload?{query}");
    let canonical = format!("{SITE}{uri}");
    let browser = Browser {
        user_agent: b"curl",
        mobile_hint: None,
    };
    let request = Request {
        canonical: &canonical,
        origin: SITE,
        uri: &uri,
        browser: &browser,
    };
    upload(&Role(role), &LibraryArgs::parse(query)?, &request)
}

/// Actions, arguments not ported, and arrays or bytes that are not text
/// in the query are not answered.
#[test]
fn what_is_not_ported() {
    for query in [
        "mode=list&action=delete&media=1",
        "mode=list&action2=attach",
        "mode=list&p=1",
        "mode=list&cat=3",
        "mode=list&x[]=1",
        "mode=list&s=%FF",
        "%FF=1",
    ] {
        assert!(answer("administrator", query).is_none(), "{query}");
    }
    assert!(answer("administrator", "mode=list&action=-1").is_some());
    // An empty mode is the list's.
    for query in ["mode=", "mode=0"] {
        let Some(Answer::Page(page)) = answer("m4list:administrator", query) else {
            unreachable!()
        };
        assert!(page.contains("table-view-list"), "{query}");
    }
    // Cleanups and types not in the list are left out of the query.
    MEDIA_QUERIES.with_borrow_mut(Vec::clear);
    answer("m4ttdcron:administrator", "mode=list&post_mime_type=bogus").unwrap();
    let query = MEDIA_QUERIES.take().remove(0);
    assert_eq!(
        (query.exclude, query.mime_types),
        (vec![611], Vec::<String>::new())
    );
    assert!(NewArgs::parse("post_id[]=1", None).is_none());
    // Hidden columns span no empty list.
    let Some(Answer::Page(page)) = answer("m4ttdhidden:administrator", "mode=list&s=zzz") else {
        unreachable!()
    };
    assert!(page.contains("colspan=\"4\">No media files found."));
    assert!(answer("m4ttdtemplate:administrator", "mode=list").is_none());
    let args = LibraryArgs::parse("1=x").unwrap();
    assert_eq!(args.args, [("1".to_string(), "x".to_string())]);
}

/// The notices of `upload.php`'s messages, and none for another.
#[test]
fn messages() {
    let site = Role("administrator");
    let notice = |query: &str| notice(&site, &LibraryArgs::parse(query).unwrap()).0;
    for (message, text) in [
        ("1", "Media file updated."),
        ("2", "Media file permanently deleted."),
        ("3", "Error saving media file."),
        ("5", "Media file restored from the Trash."),
    ] {
        assert!(notice(&format!("message={message}")).contains(&format!("<p>{text}</p>")));
    }
    assert!(notice("message=6").is_empty());
    assert!(notice("attached=abc").is_empty());
}

/// `wp_post_mime_type_where()`'s patterns.
#[test]
fn mime_type_patterns() {
    assert_eq!(
        mime_patterns("image/jpeg, audio/"),
        ["image/jpeg", "audio/%"]
    );
    assert_eq!(mime_patterns("im**ge, v<i>deo"), ["im%ge", "video/%"]);
    assert_eq!(mime_patterns("text/c+s v"), ["text/c+sv"]);
    assert!(mime_patterns("image,%").is_empty());
}

/// The attachments of interrupted updates' cleanups are left out.
#[test]
fn cleanups_are_left_out() {
    let cron = b"a:3:{i:1;a:1:{s:26:\"upgrader_scheduled_cleanup\";a:1:{s:3:\"key\";a:1:{s:4:\"args\";a:1:{i:0;i:42;}}}}i:2;a:1:{s:4:\"hook\";a:0:{}}s:7:\"version\";i:2;}";
    assert_eq!(scheduled_cleanups(cron), [42]);
    let odd = b"a:3:{i:1;a:1:{s:26:\"upgrader_scheduled_cleanup\";s:1:\"x\";}i:2;a:1:{s:26:\"upgrader_scheduled_cleanup\";a:1:{i:0;a:0:{}}}i:3;a:1:{s:26:\"upgrader_scheduled_cleanup\";a:0:{}}}";
    assert!(scheduled_cleanups(odd).is_empty());
    assert!(scheduled_cleanups(b"x").is_empty());
}

/// The cells PHP fills for posts the fixtures do not hold: no author, no
/// date, and parents of other types.
#[test]
fn cells_of_other_posts() {
    let site = Role("m4ttd:administrator");
    let post = |id, post_type: &str, author| Post {
        id,
        author,
        post_type: post_type.into(),
        title: b"T".to_vec(),
        date: "0000-00-00 00:00:00".into(),
        status: "publish".into(),
        ..Post::default()
    };
    let file = post(1, "attachment", 99);
    assert!(column_author(&site, &file).contains("(no author)"));
    assert_eq!(column_date(&site, &file), "Unpublished");
    // Only a time within the last day, now not included, is ago.
    let now = crate::posts::PostsSite::clock(&site).1;
    for (ago, shown) in [
        (0, "2026/11/12"),
        (86_399, "24 hours ago"),
        (86_400, "2026/11/11"),
    ] {
        let date = hyscribe_render::date::format_unix(
            "Y-m-d H:i:s",
            now - ago,
            hyscribe_render::date::Zone::Utc,
        );
        let dated = Post {
            date,
            ..file.clone()
        };
        assert_eq!(column_date(&site, &dated), shown, "{ago}");
    }
    let cell = |parent: &Post| column_parent(&site, &file, Some(parent));
    assert!(cell(&post(2, "custom_css", 1)).starts_with("<strong>T</strong>"));
    assert!(cell(&post(2, "plugin_type", 1)).starts_with("(Private post)"));
    let author = Role("m4ttd:author");
    let private = Post {
        status: "private".into(),
        ..post(2, "post", 2)
    };
    assert_eq!(
        column_parent(&author, &file, Some(&private)),
        "(Private post)"
    );
}

/// A search asked to be ordered by relevance is; one over 1600 bytes is
/// left out.
#[test]
fn searches() {
    MEDIA_QUERIES.with_borrow_mut(Vec::clear);
    answer(
        "m4ttd:administrator",
        "mode=list&s=canola&orderby=relevance",
    )
    .unwrap();
    let long = "x".repeat(1601);
    answer("m4ttd:administrator", &format!("mode=list&s={long}")).unwrap();
    let queries = MEDIA_QUERIES.take();
    assert_eq!(queries[0].order[0], (OrderBy::Relevance, false));
    assert!(queries[1].search.is_empty());
    let shown = |length: usize| {
        let query = format!("mode=list&s={}", "x".repeat(length));
        let Some(Answer::Page(page)) = answer("administrator", &query) else {
            unreachable!()
        };
        page.contains("<strong></strong>")
    };
    assert!(shown(1601) && !shown(1600));
}

/// A title that is all markup labels no row.
#[test]
fn markup_titles_label_no_row() {
    let site = Role("m4ttd:administrator");
    let mut file = crate::posts::PostsSite::post(&site, 1692).unwrap();
    file.title = b"<br>".to_vec();
    let listing = Listing {
        posts: Vec::new(),
        total: 0,
        pages: 0,
        page: 1,
        per_page: 20,
        trash: false,
        detached: false,
        filter: String::new(),
        columns: columns(&site, false),
        hidden: Vec::new(),
    };
    let html = row(&site, &file, &listing);
    assert!(html.contains("scope=\"row\">\t\t<strong"), "{html}");
    // A file without a URL has nothing to copy or download.
    file.id = 999_999;
    file.guid.clear();
    let html = row(&site, &file, &listing);
    assert!(
        !html.contains("Copy URL") && !html.contains("Download file"),
        "{html}"
    );
}
