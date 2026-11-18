// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query of `wp_ajax_query_attachments()` for what the golden of
//! `reference/media-ajax/golden.sh` does not send; the HTTP route's tests
//! replay the golden itself.

use hyscribe_model::{OrderBy, PostQuery};
use hyscribe_phpser::{Key, Value};

use super::attachments_query;
use crate::tests::Role;

fn query(entries: &[(&str, Value)]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value.clone()))
            .collect(),
    )
}

fn text(value: &str) -> Value {
    Value::String(value.as_bytes().to_vec())
}

fn list(values: &[Value]) -> Value {
    Value::Array(
        (0..)
            .zip(values)
            .map(|(n, v)| (Key::Int(n), v.clone()))
            .collect(),
    )
}

fn asked(role: &'static str, entries: &[(&str, Value)]) -> Option<(PostQuery, i64)> {
    attachments_query(&Role(role), &query(entries), false)
}

/// The site's posts per page when none is asked, the oracle's 10; a
/// number below -1 as its absolute value, 0 as 1, and what is not a whole
/// number not ported.
#[test]
fn posts_per_page_as_wp_query_reads_it() {
    let per_page = |value: Value| asked("administrator", &[("posts_per_page", value)]);
    let (query, ten) = asked("administrator", &[]).unwrap();
    assert_eq!((query.limit, ten), (10, 10));
    assert_eq!(per_page(text("0")).unwrap().1, 10);
    assert_eq!(per_page(text("-5")).unwrap().1, 5);
    let (all, minus) = per_page(text("-1")).unwrap();
    assert_eq!((all.limit, all.offset, minus), (u32::MAX, 0, -1));
    assert_eq!(per_page(text("5x")), None);
    assert_eq!(per_page(list(&[text("5")])), None);
    let paged = asked(
        "administrator",
        &[("posts_per_page", text("3")), ("paged", text("4"))],
    );
    assert_eq!(paged.unwrap().0.offset, 9);
    let listed = asked("administrator", &[("paged", list(&[text("4")]))]);
    assert_eq!(listed.unwrap().0.offset, 0);
}

/// Private attachments for who may read them, the Trash only with
/// `MEDIA_TRASH` and asked for.
#[test]
fn statuses_follow_the_user_and_media_trash() {
    let statuses = |query: PostQuery| -> Vec<String> {
        query.statuses.into_iter().map(|s| s.status).collect()
    };
    let (author, _) = asked("author", &[]).unwrap();
    assert_eq!(statuses(author), ["inherit"]);
    let trash = query(&[("post_status", text("trash"))]);
    let (on, _) = attachments_query(&Role("administrator"), &trash, true).unwrap();
    assert_eq!(statuses(on), ["trash", "private"]);
    let (off, _) = attachments_query(&Role("administrator"), &trash, false).unwrap();
    assert_eq!(statuses(off), ["inherit", "private"]);
}

/// Types, parents, authors and lists as `WP_Query` reads them, and the
/// values PHP fails on not ported.
#[test]
fn filters_as_wp_query_reads_them() {
    let one = |key: &str, value: Value| asked("administrator", &[(key, value)]);
    let mime = one("post_mime_type", list(&[text("image"), text("video/mp4")]));
    assert_eq!(mime.unwrap().0.mime_types, ["image/%", "video/mp4"]);
    assert_eq!(one("post_mime_type", list(&[text("a,b")])), None);
    assert_eq!(one("post_mime_type", list(&[list(&[])])), None);
    let any = one("post_mime_type", text(""));
    assert!(any.unwrap().0.mime_types.is_empty());
    assert_eq!(one("post_parent", text("3")).unwrap().0.parents, [3]);
    assert!(one("post_parent", text("x")).unwrap().0.parents.is_empty());
    assert_eq!(one("post_parent", text("-2")), None);
    assert_eq!(one("author", list(&[text("1")])), None);
    let authors = one("author", text("2,-3")).unwrap().0;
    assert_eq!(
        (authors.authors, authors.excluded_authors),
        (vec![], vec![3])
    );
    assert_eq!(one("post__in", text("1,2")), None);
    assert_eq!(one("post__not_in", text("1")), None);
    let both = asked(
        "administrator",
        &[
            ("post__in", list(&[text("5"), text("-4")])),
            ("post__not_in", list(&[text("5")])),
        ],
    );
    let both = both.unwrap().0;
    assert_eq!((both.include, both.exclude), (Some(vec![5, 4]), vec![]));
    assert_eq!(one("monthnum", text("13")), None);
    let dated = asked(
        "administrator",
        &[("year", text("2026")), ("monthnum", list(&[]))],
    );
    let dated = dated.unwrap().0;
    assert_eq!((dated.year, dated.month), (Some(2026), None));
}

/// A search finds file names too, by relevance unless another order is
/// asked; one too long, or not a string, is dropped.
#[test]
fn searches_as_wp_query_reads_them() {
    let (found, _) = asked("administrator", &[("s", text("sunset\r\nsea"))]).unwrap();
    assert_eq!(found.sentence, "sunsetsea");
    assert!(found.filenames && found.logged_in);
    assert_eq!(found.order[0], (OrderBy::Relevance, false));
    let ordered = asked(
        "administrator",
        &[
            ("s", text("sea")),
            ("orderby", text("title")),
            ("order", text("asc")),
        ],
    );
    assert_eq!(ordered.unwrap().0.order, [(OrderBy::Title, true)]);
    let excluded = asked("administrator", &[("s", text("-sea"))]).unwrap().0;
    assert_eq!(excluded.order, [(OrderBy::Date, false)]);
    let long = asked("administrator", &[("s", text(&"a".repeat(1601)))]);
    assert!(long.unwrap().0.search.is_empty());
    let listed = asked("administrator", &[("s", list(&[text("sea")]))]);
    assert!(listed.unwrap().0.search.is_empty());
}
