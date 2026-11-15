// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comments' moderation against the write oracle, and against the
//! answers `reference/admin-saves/golden.sh` recorded from PHP writing.

use hyscribe_admin::actions::action;
use hyscribe_write::comments::{Context, NewComment, insert_comment};

use super::super::comments::count_of;
use super::super::{Ajax, ajax_handler};
use super::*;
use hyscribe_db::Write;

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
    ) -> Result<String, hyscribe_write::Error> {
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
            ip: b"",
            agent: b"",
        };
        let action = action(self.screen, &asked);
        let begun = |write: &mut Result<Write<'_>, _>| {
            if let (Ok(write), Some(budget)) = (write, budget) {
                write.fail_after(budget);
            }
        };
        let mut write = writer.begin_on(conn).await;
        begun(&mut write);
        if self.screen == "admin-ajax" {
            let handler = ajax_handler(&action).unwrap();
            return Ok(match act(write, handler, &asked, admin).await? {
                Ajax::Text(text) | Ajax::Xml(text) => text,
                // Its row is read once committed, outside the action.
                Ajax::CommentRow(_) => "row".into(),
                answer => panic!("{answer:?}"),
            });
        }
        let handler = handler(self.screen, &action).unwrap();
        match act(write, handler, &asked, admin).await? {
            Answer::Redirect(to) => Ok(format!("302\t{to}")),
            _ => panic!("{}", self.get),
        }
    }

    /// Its answer, `302<TAB>Location` for a redirect, once every statement
    /// budget short of it has failed, each failure rolled back.
    async fn saved(&self, writer: &Writer, admin: &Admin<'_>, conn: &mut Connection) -> String {
        for budget in 0..100 {
            match self.send(writer, admin, conn, Some(budget)).await {
                Err(hyscribe_write::Error::Db(_)) => {}
                saved => return saved.unwrap(),
            }
        }
        panic!("every statement budget failed: {}", self.post)
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
        (
            "admin-comment-quick-edit",
            vec![(
                "admin-ajax",
                "",
                "action=edit-comment&comment_ID=1&comment_post_ID=1&newcomment_author=Ann&newcomment_author_email=ann%40example.test&newcomment_author_url=https%3A%2F%2Fexample.test&status=0&content=Edited+%3Cb%3Ereply%3C%2Fb%3E.&checkbox=1",
            )],
        ),
        (
            "admin-comment-edited",
            vec![(
                "comment",
                "",
                "action=editedcomment&comment_ID=1&comment_post_ID=1&newcomment_author=Ann+B&comment_status=spam&content=On+the+screen.&aa=2026&mm=02&jj=03&hh=04&mn=05&ss=06&hidden_aa=2025",
            )],
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
            let ours = request.saved(&writer, &admin, conn).await;
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
        assert_eq!(kept, "0");
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

/// A reply past what its checks read answers as `wp_ajax_replyto_comment()`
/// goes on: `-1` for a post gone or a held parent the user may not
/// approve, `1` for a post without a status, and a user gone is not logged
/// in. Nothing is kept.
#[tokio::test(flavor = "multi_thread")]
async fn replies_past_what_the_checks_read() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let mut write = writer.begin().await.unwrap();
    let held = hyscribe_db::CommentFields {
        post_id: 1,
        date: "2026-11-01 00:00:00".into(),
        date_gmt: "2026-11-01 00:00:00".into(),
        approved: "0".into(),
        comment_type: "comment".into(),
        ..hyscribe_db::CommentFields::default()
    };
    // Unseen by the site, which the user may not edit as far as it knows.
    let held = write.insert_comment(&held).await.unwrap();
    write.set_post_status(2, "").await.unwrap();
    for (query, user, answer) in [
        ("comment_post_ID=999999999&content=x".to_string(), 1, "-1"),
        (
            "comment_post_ID=1&content=x".to_string(),
            99,
            "Sorry, you must be logged in to reply to a comment.",
        ),
        ("comment_post_ID=2&content=x".to_string(), 1, "1"),
        (
            format!("comment_post_ID=1&comment_ID={held}&approve_parent=1&content=x"),
            1,
            "-1",
        ),
    ] {
        let post = fields(&query);
        let asked = Asked {
            get: &[],
            post: &post,
            referer: None,
            uri: b"/hs-admin/admin-ajax",
            home: "http://127.0.0.1:8081",
            user,
            now: 0,
            ip: b"",
            agent: b"",
        };
        let replied = super::super::comments::reply(&mut write, &asked, &admin).await;
        assert!(
            matches!(replied, Ok(Ajax::Text(text)) if text == answer),
            "{query}"
        );
    }
    // A reply keeps the request's user agent and the type given.
    let post = fields("comment_post_ID=1&comment_ID=1&content=Typed&comment_type=trackback");
    let asked = Asked {
        get: &[],
        post: &post,
        referer: None,
        uri: b"/hs-admin/admin-ajax",
        home: "http://127.0.0.1:8081",
        user: 1,
        now: 0,
        ip: b"192.0.2.1",
        agent: b"An agent",
    };
    let replied = super::super::comments::reply(&mut write, &asked, &admin).await;
    assert!(matches!(replied, Ok(Ajax::CommentRow(_))));
    let id = write.post_comment_ids(1).await.unwrap()[0];
    let saved = write.comment(id).await.unwrap().unwrap();
    let kept = (saved.agent, saved.comment_type, saved.author_ip);
    assert_eq!(
        kept,
        (
            b"An agent".to_vec(),
            "trackback".into(),
            b"192.0.2.1".to_vec()
        )
    );
}

/// A reply that approves its held parent writes PHP's rows, each statement
/// failing in turn first.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_approves_its_held_parent() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        hyscribe_write::comments::set_comment_status(&mut write, 1, "hold")
            .await
            .unwrap();
        write.commit().await.unwrap();
        Ok::<_, String>(())
    };
    let action = async |conn: &mut Connection, ()| {
        let query = "action=replyto-comment&comment_ID=1&comment_post_ID=1&approve_parent=1&content=+A+%3Cb%3Ereply%3C%2Fb%3E+%3Cscript%3Ex%3C%2Fscript%3E.+";
        request("admin-ajax", "", query)
            .saved(&writer, &admin, conn)
            .await;
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-comment-reply", setup, action).await;
    report.unwrap();
}
