// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit_comment()`'s checks and fields on the fixture, each test's writes
//! rolled back.

use hyscribe_db::CommentFields;
use hyscribe_phpser::Key;

use super::*;
use crate::admin::saves::tests::{fields, writer};

const MOVED: &str = "The comment cannot be moved there because it or its replies would exceed the maximum threading depth.";
const INVALID: &str = "Invalid parent comment.";

/// The request posting `query` as user 1.
fn asked(post: &[(Key, Value)]) -> Asked<'_> {
    Asked {
        get: &[],
        post,
        referer: None,
        uri: b"/hs-admin/comment",
        home: "http://127.0.0.1:8081",
        user: 1,
        now: 0,
        ip: b"",
        agent: b"",
    }
}

/// A comment on post 1 under `parent`, approved as `approved`.
fn reply(parent: u64, approved: &str) -> CommentFields {
    CommentFields {
        post_id: 1,
        date: "2026-11-01 00:00:00".into(),
        date_gmt: "2026-11-01 00:00:00".into(),
        approved: approved.into(),
        comment_type: "comment".into(),
        parent,
        ..CommentFields::default()
    }
}

/// A new parent is refused as `edit_comment()` refuses it: threading off,
/// the comment itself, a parent missing, on another post, of another type,
/// spam or trashed, held under an approved comment, among the comment's own
/// replies, or too deep for its subtree.
#[tokio::test(flavor = "multi_thread")]
async fn new_parents_are_checked_as_php_checks_them() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let w = &mut write;
    let b = w.insert_comment(&reply(1, "1")).await.unwrap();
    let other = w
        .insert_comment(&CommentFields {
            post_id: 2,
            ..reply(0, "1")
        })
        .await
        .unwrap();
    let ping = CommentFields {
        comment_type: "pingback".into(),
        ..reply(0, "1")
    };
    let ping = w.insert_comment(&ping).await.unwrap();
    let spam = w.insert_comment(&reply(0, "spam")).await.unwrap();
    let trash = w.insert_comment(&reply(0, "trash")).await.unwrap();
    let held = w.insert_comment(&reply(0, "0")).await.unwrap();
    let mut chain = vec![1];
    for _ in 0..4 {
        let last = *chain.last().unwrap();
        chain.push(w.insert_comment(&reply(last, "1")).await.unwrap());
    }
    let orphan = w.insert_comment(&reply(777_777_777, "1")).await.unwrap();
    let b = i64::try_from(b).unwrap();
    let id = |n: u64| i64::try_from(n).unwrap();
    let check = async |w: &mut Write<'_>, comment: i64, parent: i64, approved: Option<&[u8]>| {
        let found = w.comment(comment.unsigned_abs()).await.unwrap().unwrap();
        parent_error(w, (comment, &found), parent, approved)
            .await
            .unwrap()
    };
    // Unchanged or top level: nothing to check.
    assert_eq!(check(w, b, 1, None).await, None);
    assert_eq!(check(w, b, 0, None).await, None);
    assert_eq!(
        check(w, b, b, None).await,
        Some("A comment cannot be a reply to itself.")
    );
    for parent in [
        999_999_999,
        -5,
        id(other),
        id(ping),
        id(spam),
        id(trash),
        id(held),
    ] {
        assert_eq!(check(w, b, parent, None).await, Some(INVALID), "{parent}");
    }
    assert_eq!(check(w, b, id(held), Some(b"0")).await, None);
    assert_eq!(check(w, b, id(held), Some(b"approve")).await, Some(INVALID));
    let own = "A comment cannot be a reply to one of its own replies.";
    assert_eq!(check(w, 1, b, None).await, Some(own));
    // Comment 1's subtree is five deep, the chain's last at depth 5.
    assert_eq!(check(w, b, id(chain[3]), None).await, None);
    assert_eq!(check(w, b, id(chain[4]), None).await, Some(MOVED));
    assert_eq!(check(w, id(chain[1]), b, None).await, Some(MOVED));
    // A parent whose own parent is gone is at depth 2.
    assert_eq!(check(w, b, id(orphan), None).await, None);
    // Two comments replying to each other: their subtree is measured until
    // it cannot fit, which ends the loop.
    let looped = w.insert_comment(&reply(0, "1")).await.unwrap();
    let back = w.insert_comment(&reply(looped, "1")).await.unwrap();
    let mut first = w.comment(looped).await.unwrap().unwrap();
    first.parent = back;
    w.update_comment(looped, &first).await.unwrap();
    assert_eq!(check(w, id(looped), b, None).await, Some(MOVED));
    w.update_option("thread_comments_depth", b"0")
        .await
        .unwrap();
    assert_eq!(check(w, b, id(chain[4]), None).await, None);
    w.update_option("thread_comments", b"").await.unwrap();
    let off = "The comment parent cannot be changed because threaded comments are disabled.";
    assert_eq!(check(w, b, id(held), None).await, Some(off));
}

/// The edit screen's date, once a field differs from its hidden copy: a
/// whole valid date, else not ported.
#[test]
fn edited_dates_are_whole_and_valid() {
    let date = |query: &str| {
        let post = fields(query);
        edited_date(&asked(&post))
    };
    assert!(matches!(date("aa=2026&hidden_aa="), Ok(None)));
    assert!(matches!(date("aa=2026&hidden_aa=2026"), Ok(None)));
    let whole = "mm=02&jj=03&hh=04&mn=05&ss=06";
    let edited = date(&format!("edit_date=1&aa=2026&{whole}")).unwrap();
    assert_eq!(edited.as_deref(), Some("2026-02-03 04:05:06"));
    for (aa, mm, jj, valid) in [
        ("2024", "02", "29", true),
        ("2000", "02", "29", true),
        ("2026", "02", "28", true),
        ("2026", "02", "29", false),
        ("1900", "02", "29", false),
        ("2026", "04", "30", true),
        ("2026", "04", "31", false),
        ("2026", "06", "31", false),
        ("2026", "09", "31", false),
        ("2026", "11", "31", false),
        ("2026", "12", "31", true),
        ("2026", "13", "01", false),
        ("2026", "00", "01", false),
        ("2026", "01", "00", false),
    ] {
        let query = format!("hidden_aa=1&aa={aa}&mm={mm}&jj={jj}&hh=00&mn=00&ss=00");
        assert_eq!(date(&query).is_ok(), valid, "{aa}-{mm}-{jj}");
    }
    for time in [
        "hh=24&mn=00&ss=00",
        "hh=23&mn=60&ss=00",
        "hh=23&mn=59&ss=60",
    ] {
        let query = format!("edit_date=1&aa=2026&mm=01&jj=01&{time}");
        assert!(date(&query).is_err(), "{time}");
    }
    let last = date("edit_date=1&aa=2026&mm=01&jj=01&hh=23&mn=59&ss=59").unwrap();
    assert_eq!(last.as_deref(), Some("2026-01-01 23:59:59"));
    for odd in ["aa=26", "aa=2O26", "aa[]=2026"] {
        assert!(
            date(&format!("edit_date=1&{odd}&{whole}")).is_err(),
            "{odd}"
        );
    }
}

/// What PHP cannot save as asked is not ported, and the comment the
/// request names must exist.
#[tokio::test(flavor = "multi_thread")]
async fn odd_requests_save_nothing() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    for query in [
        "comment_ID=1&comment_meta[k]=v",
        "comment_ID=1&newcomment_author[]=x",
        "comment_ID=1&comment_post_ID=1a",
        "comment_ID=1&user_id=-1",
        "comment_ID=1&comment_author[]=x",
        "comment_ID=1&comment_karma[]=1",
        "comment_ID=1&comment_status[]=1",
        "comment_ID=1&newcomment_author_email[]=x",
        "comment_ID=1&newcomment_author_url[]=x",
        "comment_ID=1&comment_author_IP[]=x",
        "comment_ID=1&edit_date=1&aa=x",
        "comment_ID=1&content[]=x",
        "comment_ID=1&comment_agent[]=x",
        "comment_ID=1&comment_type[]=x",
        "comment_ID=1&comment_date[]=x",
    ] {
        let post = fields(query);
        let edited = edit_comment(&mut write, &asked(&post), None).await;
        assert!(matches!(edited, Err(Error::Unported(_))), "{query}");
    }
    for query in ["comment_ID=-1", "comment_ID=999999999"] {
        let post = fields(query);
        let edited = edit_comment(&mut write, &asked(&post), None).await;
        assert_eq!(edited.unwrap(), Err("Invalid comment ID."), "{query}");
    }
    // `comment_status` before `comment_approved`, Quick Edit's `status` first.
    let post = fields("comment_ID=1&comment_status=0&comment_approved=spam");
    edit_comment(&mut write, &asked(&post), None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(write.comment(1).await.unwrap().unwrap().approved, "0");
    let status = Some(b"spam".to_vec());
    edit_comment(&mut write, &asked(&post), status)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(write.comment(1).await.unwrap().unwrap().approved, "spam");
}

/// A comment moved under a reply's reply, its own reply moving with it,
/// keeps nothing when a statement fails, each in turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_statement_moves_nothing() {
    let writer = writer().await;
    for budget in 0..100 {
        let mut write = writer.begin().await.unwrap();
        let moved = write.insert_comment(&reply(1, "1")).await.unwrap();
        write.insert_comment(&reply(moved, "1")).await.unwrap();
        let under = write.insert_comment(&reply(1, "1")).await.unwrap();
        let under = write.insert_comment(&reply(under, "1")).await.unwrap();
        write.fail_after(budget);
        let post = fields(&format!(
            "comment_ID={moved}&comment_parent={under}&content=Moved"
        ));
        match edit_comment(&mut write, &asked(&post), None).await {
            Err(Error::Db(_)) => {}
            edited => return assert_eq!(edited.unwrap(), Ok(())),
        }
    }
    panic!("every statement budget failed");
}
