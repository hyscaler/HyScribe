// SPDX-License-Identifier: AGPL-3.0-or-later

//! Each port against the rows PHP WordPress wrote for the same action
//! (`reference/writes/actions-comments.php`), as user 1 unless noted.

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Writer};
use hyscribe_testkit::{fixture_database, writes::check_writes};

use super::*;

/// The oracle's `home_url()`.
const HOME: &[u8] = b"http://127.0.0.1:8081";

/// The oracle's user 1, an administrator without `unfiltered_html`.
const ADMIN: Commenter = Commenter {
    moderate_comments: true,
    manage_options: true,
    unfiltered_html: false,
};

/// One check at a time: each holds row locks until its rollback.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn context() -> Context<'static> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    Context {
        now: i64::try_from(now.unwrap().as_secs()).unwrap(),
        home: HOME,
    }
}

/// Runs `action` as one action on the oracle's connection.
async fn on<T>(conn: &mut Connection, action: impl AsyncFnOnce(&mut Write<'_>) -> T) -> T {
    let writer = Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let mut write = writer.begin_on(conn).await.unwrap();
    let out = action(&mut write).await;
    write.commit().await.unwrap();
    out
}

/// Checks the port against the golden `name`: `setup` and `action` each
/// run as one action.
async fn check<T>(
    name: &str,
    setup: impl AsyncFnOnce(&mut Write<'_>) -> T,
    action: impl AsyncFnOnce(&mut Write<'_>, T),
) {
    let _one = ONE_AT_A_TIME.lock().await;
    let setup = async |conn: &mut Connection| Ok(on(conn, setup).await);
    let action = async |conn: &mut Connection, state| {
        on(conn, async |write: &mut Write<'_>| {
            action(write, state).await
        })
        .await;
        Ok(())
    };
    check_writes(&fixture_database(), name, setup, action)
        .await
        .unwrap();
}

async fn nothing(_: &mut Write<'_>) {}

/// The actions' `$comment`: a comment on post 1 by a reader.
fn reader(approved: &str) -> NewComment {
    NewComment {
        post_id: 1,
        author: b"A reader".to_vec(),
        author_email: b"reader@example.test".to_vec(),
        author_url: b"https://reader.example.test".to_vec(),
        author_ip: b"192.0.2.1".to_vec(),
        content: b"A comment.".to_vec(),
        approved: Some(approved.into()),
        agent: b"Oracle".to_vec(),
        ..NewComment::default()
    }
}

/// The actions' `$submit` data.
fn submitted() -> NewComment {
    NewComment {
        content: b"A new comment.".to_vec(),
        approved: None,
        ..reader("")
    }
}

/// The fixture's approved commenter.
fn known(content: &[u8]) -> NewComment {
    NewComment {
        author: b"A WordPress Commenter".to_vec(),
        author_email: b"wapuu@wordpress.example".to_vec(),
        content: content.to_vec(),
        ..submitted()
    }
}

async fn submit(
    write: &mut Write<'_>,
    comment: NewComment,
    commenter: Commenter,
) -> Result<u64, Refused> {
    new_comment(write, context(), comment, commenter)
        .await
        .unwrap()
}

async fn insert(write: &mut Write<'_>, comment: NewComment) -> u64 {
    insert_comment(write, context(), &comment).await.unwrap()
}

/// A setup that sets the option `name`.
fn option(name: &'static str, value: &'static [u8]) -> impl AsyncFnOnce(&mut Write<'_>) {
    async move |write: &mut Write<'_>| {
        write.update_option(name, value).await.unwrap();
    }
}

#[tokio::test]
async fn inserts_and_approves() {
    // Approved when `comment_approved` is not given; PHP gave `1`.
    check("insert-comment", nothing, async |write, ()| {
        let comment = NewComment {
            approved: None,
            ..reader("")
        };
        insert(write, comment).await;
    })
    .await;
    check(
        "approve-comment",
        async |write| insert(write, reader("0")).await,
        async |write, id| assert!(set_comment_status(write, id, "approve").await.unwrap()),
    )
    .await;
}

/// `wp_new_comment()`'s data with HTML in every field.
fn html() -> NewComment {
    NewComment {
        user_id: 1,
        author: b" <b>Admin</b> & co's\t%41name ".to_vec(),
        author_email: b" ADMIN<i>@</i>Example.test ".to_vec(),
        author_url: b" https://example.test/a b<b>c</b> ".to_vec(),
        author_ip: b"192.0.2.1<script>, ::1".to_vec(),
        agent: b"Agent ".repeat(50),
        content: b"<a href=\"https://example.test\" class=\"x\" rel=\"me\">out</a> \
            <A HREF='http://127.0.0.1:8081/about'>in</A> <a\nhref=\"https://split.test\">split</a> \
            <span class=\"wp-note-mention user-2 user-0 evil\">@me</span> \
            <script>alert(1)</script> &#147;quoted&#148; it's \"fine\" \\ <b>bold"
            .to_vec(),
        ..submitted()
    }
}

#[tokio::test]
async fn new_comments_are_filtered() {
    check("new-comment-held", nothing, async |write, ()| {
        submit(write, submitted(), ADMIN).await.unwrap();
    })
    .await;
    check("new-comment-by-admin", nothing, async |write, ()| {
        submit(write, html(), ADMIN).await.unwrap();
    })
    .await;
    let unfiltered = Commenter {
        unfiltered_html: true,
        ..ADMIN
    };
    check("new-comment-unfiltered", nothing, async |write, ()| {
        submit(write, html(), unfiltered).await.unwrap();
    })
    .await;
    let balanced = known(b"<b>Bold <i>both</b> it's");
    check(
        "new-comment-balanced",
        option("use_balanceTags", b"1"),
        async |write, ()| {
            submit(write, balanced, ADMIN).await.unwrap();
        },
    )
    .await;
}

#[tokio::test]
async fn new_comments_follow_the_discussion_options() {
    check("new-comment-links", nothing, async |write, ()| {
        let two = known(b"See https://a.test and www.b.test.");
        submit(write, two, ADMIN).await.unwrap();
        submit(write, known(b"Only https://c.test here."), ADMIN)
            .await
            .unwrap();
    })
    .await;
    check(
        "new-comment-moderation",
        option("comment_moderation", b"1"),
        async |write, ()| {
            submit(write, known(b"A new comment."), ADMIN)
                .await
                .unwrap();
        },
    )
    .await;
    check(
        "new-comment-moderation-key",
        option("moderation_keys", "\n  0\nSPAM#Wörd\n".as_bytes()),
        async |write, ()| {
            let content = known("This has spam#wÖrd in it.".as_bytes());
            submit(write, content, ADMIN).await.unwrap();
            let agent = NewComment {
                agent: "a SPAM#WÖRD agent".as_bytes().to_vec(),
                ..known(b"A new comment.")
            };
            submit(write, agent, ADMIN).await.unwrap();
            submit(write, known(b"This is clean."), ADMIN)
                .await
                .unwrap();
        },
    )
    .await;
    check(
        "new-comment-disallowed",
        option("disallowed_keys", b"\nbadword\n"),
        async |write, ()| {
            submit(write, known(b"A bad<b>word</b> here."), ADMIN)
                .await
                .unwrap();
            let own = NewComment {
                user_id: 1,
                content: b"My own badword.".to_vec(),
                ..submitted()
            };
            submit(write, own, ADMIN).await.unwrap();
        },
    )
    .await;
}

#[tokio::test]
async fn new_comments_by_others() {
    let unfiltered = Commenter {
        unfiltered_html: true,
        ..ADMIN
    };
    check("new-comment-anonymous", nothing, async |write, ()| {
        let anonymous = NewComment {
            author_email: Vec::new(),
            content:
                b"&#100; &#129; &#130; &#1300; <a download rel=\"me  me\" href=\"/about\">x</a> \
                <a href=\"mailto:x@example.test\">m</a>"
                    .to_vec(),
            ..submitted()
        };
        submit(write, anonymous, unfiltered).await.unwrap();
    })
    .await;
    check("new-comment-pingback", nothing, async |write, ()| {
        let pingback = NewComment {
            comment_type: "pingback".into(),
            date: Some("2026-01-02 03:04:05".into()),
            date_gmt: Some("0".into()),
            author_url: b"https://elsewhere.test/post".to_vec(),
            ..known(b"A new comment.")
        };
        submit(write, pingback, ADMIN).await.unwrap();
    })
    .await;
    let author = Commenter {
        moderate_comments: false,
        ..ADMIN
    };
    check("new-comment-by-author", nothing, async |write, ()| {
        let own = NewComment {
            user_id: 1,
            content: b"By the author.".to_vec(),
            ..submitted()
        };
        submit(write, own, author).await.unwrap();
        write.reassign_posts(1, 2).await.unwrap();
        let other = NewComment {
            user_id: 1,
            author: b"admin".to_vec(),
            author_email: b"admin@default-install.test".to_vec(),
            content: b"Not the author.".to_vec(),
            ..submitted()
        };
        submit(write, other, author).await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn new_comments_reply_and_are_refused() {
    check(
        "new-comment-replies",
        async |write| {
            (
                insert(write, reader("spam")).await,
                insert(write, reader("0")).await,
            )
        },
        async |write, (spam, held)| {
            for (n, parent) in [1, spam, held, 999].into_iter().enumerate() {
                let reply = NewComment {
                    user_id: 1,
                    parent,
                    content: format!("Reply {n}.").into_bytes(),
                    ..submitted()
                };
                submit(write, reply, ADMIN).await.unwrap();
            }
        },
    )
    .await;
    check("new-comment-duplicate", nothing, async |write, ()| {
        let fixture = write.comment(1).await.unwrap().unwrap();
        let dupe = known(&fixture.content);
        let refused = submit(write, dupe.clone(), ADMIN).await;
        assert_eq!(refused, Err(Refused::Duplicate));
        let reply = NewComment { parent: 1, ..dupe };
        submit(write, reply, ADMIN).await.unwrap();
    })
    .await;
    check(
        "new-comment-flood",
        async |write| {
            let earlier = NewComment {
                author_ip: b"192.0.2.7".to_vec(),
                ..reader("1")
            };
            insert(write, earlier).await;
        },
        async |write, ()| {
            let visitor = Commenter::default();
            let other = NewComment {
                author_ip: b"192.0.2.8".to_vec(),
                author_email: b"other@example.test".to_vec(),
                ..submitted()
            };
            submit(write, other, visitor).await.unwrap();
            let again = NewComment {
                author_ip: b"192.0.2.7".to_vec(),
                ..submitted()
            };
            assert_eq!(submit(write, again, visitor).await, Err(Refused::Flood));
        },
    )
    .await;
}

#[tokio::test]
async fn updates() {
    check("update-comment", nothing, async |write, ()| {
        let content = b"Edited <script>x</script> <a href=\"https://example.test\">it's</a>";
        let edit = CommentEdit {
            id: 1,
            author: Some(b"Edited <i>name</i>".to_vec()),
            content: Some(content.to_vec()),
            date: Some("2026-11-01 10:00:00".into()),
            approved: Some("hold".into()),
            ..CommentEdit::default()
        };
        let changed = update_comment(write, context(), &edit, true).await;
        assert_eq!(changed.unwrap(), Some(true));
    })
    .await;
    check(
        "update-comment-moved",
        async |write| insert(write, reader("0")).await,
        async |write, id| {
            let update = async |write: &mut Write<'_>, edit: CommentEdit| {
                update_comment(write, context(), &edit, true).await.unwrap()
            };
            let same = CommentEdit {
                id,
                ..CommentEdit::default()
            };
            assert_eq!(update(write, same.clone()).await, Some(false));
            let missing = CommentEdit {
                id: 999,
                ..CommentEdit::default()
            };
            assert_eq!(update(write, missing).await, None);
            let nowhere = CommentEdit {
                post_id: Some(999),
                ..same.clone()
            };
            assert_eq!(update(write, nowhere).await, None);
            let moved = CommentEdit {
                post_id: Some(2),
                author_email: Some(b"new@example.test".to_vec()),
                author_url: Some(b"https://new.example.test".to_vec()),
                author_ip: Some(b"192.0.2.9".to_vec()),
                karma: Some(3),
                approved: Some("approve".into()),
                agent: Some(b"New agent".to_vec()),
                comment_type: Some("pingback".into()),
                parent: Some(1),
                user_id: Some(1),
                ..same
            };
            assert_eq!(update(write, moved).await, Some(true));
            let spam = CommentEdit {
                id: 1,
                post_id: Some(0),
                approved: Some("spam".into()),
                ..CommentEdit::default()
            };
            assert_eq!(update(write, spam).await, Some(true));
        },
    )
    .await;
}

#[tokio::test]
async fn missing_comments_and_statuses() {
    check("comment-missing", nothing, async |write, ()| {
        assert!(!trash_comment(write, context(), 999).await.unwrap());
        assert!(!spam_comment(write, context(), 999).await.unwrap());
        assert!(!restore_comment(write, 999).await.unwrap());
        assert!(!set_comment_status(write, 1, "bogus").await.unwrap());
        assert!(!delete_comment(write, context(), 999, false).await.unwrap());
        assert!(set_comment_status(write, 1, "0").await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn statuses_change() {
    check("hold-comment", nothing, async |write, ()| {
        assert!(set_comment_status(write, 1, "hold").await.unwrap());
    })
    .await;
    check(
        "status-unchanged",
        async |write| {
            write.set_comment_count(1, 5).await.unwrap();
        },
        async |write, ()| {
            assert!(!set_comment_status(write, 1, "approve").await.unwrap());
            insert(write, reader("0")).await;
        },
    )
    .await;
    check("spam-comment", nothing, async |write, ()| {
        assert!(spam_comment(write, context(), 1).await.unwrap());
    })
    .await;
    check(
        "unspam-comment",
        async |write| assert!(spam_comment(write, context(), 1).await.unwrap()),
        async |write, ()| assert!(restore_comment(write, 1).await.unwrap()),
    )
    .await;
}

#[tokio::test]
async fn trash_and_delete() {
    check("trash-comment", nothing, async |write, ()| {
        assert!(trash_comment(write, context(), 1).await.unwrap());
    })
    .await;
    check(
        "untrash-comment",
        async |write| {
            let held = insert(write, reader("0")).await;
            assert!(trash_comment(write, context(), held).await.unwrap());
            (held, insert(write, reader("trash")).await)
        },
        async |write, (held, trash)| {
            assert!(restore_comment(write, held).await.unwrap());
            assert!(restore_comment(write, trash).await.unwrap());
        },
    )
    .await;
    check(
        "delete-comment",
        async |write| {
            let reply = insert(
                write,
                NewComment {
                    parent: 1,
                    ..reader("1")
                },
            )
            .await;
            insert(
                write,
                NewComment {
                    parent: reply,
                    ..reader("0")
                },
            )
            .await;
            for value in [b"a", b"b"] {
                let meta = write.add_meta(Meta::Comment, reply, "oracle", value).await;
                meta.unwrap();
            }
            reply
        },
        async |write, reply| assert!(delete_comment(write, context(), reply, true).await.unwrap()),
    )
    .await;
    check("delete-comment-to-trash", nothing, async |write, ()| {
        assert!(delete_comment(write, context(), 1, false).await.unwrap());
    })
    .await;
    check(
        "delete-trashed-comment",
        async |write| assert!(trash_comment(write, context(), 1).await.unwrap()),
        async |write, ()| assert!(delete_comment(write, context(), 1, false).await.unwrap()),
    )
    .await;
    check(
        "trash-note",
        async |write| {
            let note = NewComment {
                comment_type: "note".into(),
                ..reader("1")
            };
            let note = insert(write, note).await;
            let children = [("1", "01"), ("0", "03"), ("spam", "02"), ("1", "04")];
            for (n, (approved, second)) in children.into_iter().enumerate() {
                let date = format!("2026-01-01 00:00:{second}");
                let child = NewComment {
                    comment_type: if n == 3 { "comment" } else { "note" }.into(),
                    parent: note,
                    date: Some(date.clone()),
                    date_gmt: Some(date),
                    ..reader(approved)
                };
                insert(write, child).await;
            }
            note
        },
        async |write, note| assert!(trash_comment(write, context(), note).await.unwrap()),
    )
    .await;
}
