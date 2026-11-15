// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comments' moderation against the write oracle, and against the
//! answers `reference/admin-saves/golden.sh` recorded from PHP writing.

use hyscribe_admin::actions::action;
use hyscribe_write::comments::{Context, NewComment, insert_comment};

use super::super::comments::count_of;
use super::*;

/// An ID no comment has (PHP's `HYSCRIBE_MISSING_ID`).
const MISSING: u64 = 1 << 40;

/// A request to the hs-admin screen `screen`, by user 1.
struct Request<'a> {
    screen: &'a str,
    get: String,
    post: String,
    now: i64,
}

impl Request<'_> {
    /// Its answer, as one action on `conn` whose statements after `budget`
    /// fail.
    async fn send(
        &self,
        writer: &Writer,
        admin: &Admin<'_>,
        conn: &mut Connection,
        budget: Option<usize>,
    ) -> Result<Answer, hyscribe_write::Error> {
        let (get, post) = (fields(&self.get), fields(&self.post));
        let uri = format!("/hs-admin/{}?{}", self.screen, self.get);
        let asked = Asked {
            get: &get,
            post: &post,
            referer: None,
            uri: uri.as_bytes(),
            home: "http://127.0.0.1:8081",
            user: 1,
            now: self.now,
        };
        let handler = handler(self.screen, &action(self.screen, &asked)).unwrap();
        let mut write = writer.begin_on(conn).await;
        if let (Ok(write), Some(budget)) = (&mut write, budget) {
            write.fail_after(budget);
        }
        act(write, handler, &asked, admin).await
    }

    /// Its answer once every statement budget short of it has failed, each
    /// failure rolled back.
    async fn saved(&self, writer: &Writer, admin: &Admin<'_>, conn: &mut Connection) -> Answer {
        let mut budget = 0;
        loop {
            match self.send(writer, admin, conn, Some(budget)).await {
                Err(hyscribe_write::Error::Db(_)) => budget += 1,
                saved => return saved.unwrap(),
            }
        }
    }
}

fn request<'a>(screen: &'a str, get: &str, post: &str) -> Request<'a> {
    Request {
        screen,
        get: get.into(),
        post: post.into(),
        now: crate::now(),
    }
}

/// Each moderation's rows are PHP's: Spam and Empty Trash from the list,
/// Unapprove from `comment.php`, and the list's Ajax Unapprove and Trash.
#[tokio::test(flavor = "multi_thread")]
async fn moderations_write_php_s_rows() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let spam = format!(
        "action=spam&delete_comments[]=1&delete_comments[]={MISSING}&_wp_http_referer=%2Fhs-admin%2Fedit-comments"
    );
    let empty = "action=-1&delete_all=Empty+Trash&comment_status=trash&pagegen_timestamp=2099-01-01+00%3A00%3A00";
    let cases = [
        ("admin-comments-spam", vec![("edit-comments", &*spam, "")]),
        (
            "admin-comments-empty-trash",
            vec![
                ("edit-comments", "action=trash&ids=1", ""),
                ("edit-comments", empty, ""),
            ],
        ),
        (
            "admin-comment-unapprove",
            vec![("comment", "action=unapprovecomment&c=1", "")],
        ),
        (
            "admin-comment-dim",
            vec![("admin-ajax", "", "action=dim-comment&id=1&new=unapproved")],
        ),
        (
            "admin-comment-ajax-trash",
            vec![("admin-ajax", "", "action=delete-comment&id=1&trash=1")],
        ),
    ];
    for (golden, requests) in cases {
        let action = async |conn: &mut Connection, ()| {
            for (screen, get, post) in &requests {
                let request = request(screen, get, post);
                request.saved(&writer, &admin, conn).await;
            }
            Ok(())
        };
        let report = check_writes(&fixture_database(), golden, async |_| Ok(()), action).await;
        report.unwrap();
    }
}

/// The golden's `case`: its request, with PHP's time and nonce, and what
/// PHP answered under hs-admin's names, `302<TAB>Location` for a redirect.
fn golden_case(row: &str) -> (Request<'static>, String) {
    let cases = include_str!("../../../../../../reference/admin-saves/cases.list");
    let [case, status, location, body] = row.split('\t').collect::<Vec<_>>()[..] else {
        panic!("{row}");
    };
    let line = cases.lines().find(|l| l.starts_with(&format!("{case}\t")));
    let [_, _, target, data, _, field] = line.unwrap().split('\t').collect::<Vec<_>>()[..] else {
        panic!("{case}");
    };
    let body = String::from_utf8(unhex(body)).unwrap();
    let time = body.split("<time><![CDATA[").nth(1).unwrap_or(&body);
    let now = time.split(']').next().unwrap().parse().unwrap_or(0);
    // The nonce is in the Location PHP sends back to the request's URI.
    let nonce = location
        .split("_wpnonce=")
        .nth(1)
        .map_or("0", |n| n.get(..10).unwrap());
    let target = hs_admin_page(&format!("/wp-admin/{target}"));
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let screen = match path.trim_start_matches("/hs-admin/") {
        "admin-ajax" => "admin-ajax",
        "comment" => "comment",
        _ => "edit-comments",
    };
    let data = if data == "-" {
        String::new()
    } else {
        hs_admin_page(data)
    };
    let (get, post) = match data.as_str() {
        "" => (format!("{query}&{field}={nonce}"), String::new()),
        _ => (query.to_string(), format!("{data}&{field}={nonce}")),
    };
    let answer = match status {
        "302" => format!("302\t{}", hs_admin_page(location)),
        _ => body,
    };
    (
        Request {
            screen,
            get,
            post,
            now,
        },
        answer,
    )
}

/// Comment 1 moderated in turn, as `reference/admin-saves/golden.sh` had
/// PHP moderate it, each answered as PHP answered, the rows at the end
/// those Empty Trash leaves.
#[tokio::test(flavor = "multi_thread")]
async fn moderations_answer_as_php() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let golden = include_str!("../../../../../hyscribe-admin/tests/golden/admin-saves.tsv");
    let action = async |conn: &mut Connection, ()| {
        let rows = golden.lines().filter(|row| row.starts_with("moderate-"));
        for row in rows {
            let (request, php) = golden_case(row);
            let ours = match request.saved(&writer, &admin, conn).await {
                Answer::Redirect(to) => format!("302\t{to}"),
                Answer::Ajax(text) | Answer::Xml(text) => text,
                _ => panic!("{row}"),
            };
            assert_eq!(ours, php, "{row}");
        }
        Ok(())
    };
    let empty = "admin-comments-empty-trash";
    let report = check_writes(&fixture_database(), empty, async |_| Ok(()), action).await;
    report.unwrap();
}

/// A comment of a trashed post goes to the Trash through the list and
/// stays there: `post-trashed` is no status to restore, and
/// `wp_ajax_delete_comment()` answers `0`.
#[tokio::test(flavor = "multi_thread")]
async fn a_post_trashed_comment_stays_in_the_trash() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let comment = NewComment {
            post_id: 1,
            author: b"A reader".to_vec(),
            content: b"A comment.".to_vec(),
            approved: Some("post-trashed".into()),
            ..NewComment::default()
        };
        let context = Context {
            now: crate::now(),
            home: b"http://127.0.0.1:8081",
        };
        let id = insert_comment(&mut write, context, &comment).await.unwrap();
        write.commit().await.unwrap();
        Ok::<_, String>(id)
    };
    let action = async |conn: &mut Connection, id: u64| {
        let trash = format!("action=delete-comment&id={id}&trash=1");
        request("admin-ajax", "", &trash)
            .saved(&writer, &admin, conn)
            .await;
        let untrash = format!("action=delete-comment&id={id}&untrash=1");
        let kept = request("admin-ajax", "", &untrash)
            .saved(&writer, &admin, conn)
            .await;
        assert!(matches!(kept, Answer::Ajax(text) if text == "0"));
        Ok(())
    };
    let golden = "admin-comment-ajax-untrash-kept";
    let report = check_writes(&fixture_database(), golden, setup, action).await;
    report.unwrap();
}

/// What PHP cannot go on with once it has written keeps nothing: an
/// array as the list's URL, or a page size `mt_rand()` refuses.
#[tokio::test(flavor = "multi_thread")]
async fn a_fatal_error_keeps_nothing() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let action = async |conn: &mut Connection, ()| {
        let trash = "action=delete-comment&id=1&trash=1";
        for more in ["_url[]=x", "_total=4&_per_page=-2&_page=1&_url=x"] {
            let fatal = request("admin-ajax", "", &format!("{trash}&{more}"));
            let answer = fatal.send(&writer, &admin, conn, None).await;
            assert!(matches!(answer, Err(hyscribe_write::Error::Unported(_))));
        }
        request("admin-ajax", "", trash)
            .saved(&writer, &admin, conn)
            .await;
        Ok(())
    };
    let golden = "admin-comment-ajax-trash";
    let report = check_writes(&fixture_database(), golden, async |_| Ok(()), action).await;
    report.unwrap();
}

/// `wp_count_comments()`'s counts by name, `None` for a view it lacks.
#[test]
fn the_list_s_views_are_counted() {
    let counts = [1, 2, 4, 8, 16];
    for (view, count) in [
        ("approved", Some(1)),
        ("moderated", Some(2)),
        ("spam", Some(4)),
        ("trash", Some(8)),
        ("post-trashed", Some(16)),
        ("all", Some(3)),
        ("total_comments", Some(7)),
        ("mine", None),
    ] {
        assert_eq!(count_of(&counts, view.as_bytes()), count, "{view}");
    }
}
