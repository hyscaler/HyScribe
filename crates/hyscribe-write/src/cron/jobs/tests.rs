// SPDX-License-Identifier: AGPL-3.0-or-later

//! The jobs against the write oracle's goldens
//! (`reference/writes/actions-cron.php`): each setup writes what PHP's
//! does, and the job, run with no user, must write PHP's rows.

use super::*;
use hyscribe_db::{Connection, Writer};
use std::result::Result as StdResult;

use crate::options::{Autoload, add_metadata, add_option, set_transient, update_metadata};
use crate::posts::tests::{act, check, future_post, insert_post, oracle_post, shown};
use crate::posts::{PostArgs, wp_trash_post};
use crate::tests::{MISSING, ONE_AT_A_TIME, admin, fails_at_each_step, rolled_back, text, writer};

/// PHP's `HYSCRIBE_CRON_OLD`: a time long past.
pub(crate) const OLD: i64 = 1_000_000_000;

/// Cron's request: no user, now.
pub(crate) fn cron() -> Context {
    Context { user: 0, ..admin() }
}

/// Runs `sql` with `binds` on the oracle's connection.
async fn raw(conn: &mut Connection, sql: &str, binds: &[&str]) -> StdResult<(), String> {
    let query = binds
        .iter()
        .fold(sqlx::query(sqlx::AssertSqlSafe(sql)), |q, b| q.bind(*b));
    query.execute(&mut *conn).await.map(drop).map_err(shown)
}

/// `$cron_missed()`: a post scheduled for 2030 moved to 2026-01-01, with
/// its event.
pub(crate) async fn missed(writer: &Writer, conn: &mut Connection) -> StdResult<u64, String> {
    let id = insert_post(writer, conn, future_post()).await?;
    let date = "2026-01-01 10:00:00";
    let sql = "UPDATE wp_posts SET post_date = ?, post_date_gmt = ? WHERE ID = ?";
    raw(conn, sql, &[date, date, &id.to_string()]).await?;
    act(writer, conn, &admin(), async |w, ctx| {
        clear(w, "publish_future_post", id).await?;
        schedule(w, (ctx.now, 1_767_261_600), "publish_future_post", Some(id)).await
    })
    .await?;
    Ok(id)
}

/// `$cron_comment( $id, $approved, $trashed )`.
async fn comment(
    conn: &mut Connection,
    id: u64,
    approved: &str,
    trashed: i64,
) -> StdResult<(), String> {
    let sql = "INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_author, \
               comment_date, comment_date_gmt, comment_content, comment_approved) VALUES \
               (?, 1, 'A reader', '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'A comment.', ?)";
    raw(conn, sql, &[&id.to_string(), approved]).await?;
    let meta = "INSERT INTO wp_commentmeta (comment_id, meta_key, meta_value) VALUES (?, ?, ?)";
    let id = id.to_string();
    raw(conn, meta, &[&id, "_wp_trash_meta_status", "1"]).await?;
    raw(
        conn,
        meta,
        &[&id, "_wp_trash_meta_time", &trashed.to_string()],
    )
    .await
}

/// `$cron_trash()`.
pub(crate) async fn trash(writer: &Writer, conn: &mut Connection) -> StdResult<(), String> {
    let ctx = admin();
    let old = insert_post(writer, conn, oracle_post("publish")).await?;
    act(writer, conn, &ctx, async |w, ctx| {
        wp_trash_post(w, ctx, old).await?;
        let (time, any) = (Value::Int(OLD), Value::String(Vec::new()));
        update_metadata(w, Meta::Post, old, "_wp_trash_meta_time", &time, &any).await?;
        Ok(())
    })
    .await?;
    let recent = insert_post(writer, conn, oracle_post("publish")).await?;
    act(writer, conn, &ctx, async |w, ctx| {
        wp_trash_post(w, ctx, recent).await
    })
    .await?;
    let restored = insert_post(writer, conn, oracle_post("publish")).await?;
    act(writer, conn, &ctx, async |w, _| {
        let time = Value::Int(OLD);
        add_metadata(
            w,
            Meta::Post,
            restored,
            "_wp_trash_meta_status",
            &text("publish"),
            false,
        )
        .await?;
        add_metadata(w, Meta::Post, restored, "_wp_trash_meta_time", &time, false).await?;
        add_metadata(w, Meta::Post, MISSING, "_wp_trash_meta_time", &time, false).await?;
        Ok(())
    })
    .await?;
    comment(conn, 901, "trash", OLD).await?;
    comment(conn, 902, "1", OLD).await?;
    comment(conn, 903, "trash", crate::now()).await
}

/// `$cron_transients()`.
pub(crate) async fn transients(writer: &Writer, conn: &mut Connection) -> StdResult<(), String> {
    act(writer, conn, &admin(), async |w, _| {
        set_transient(w, "oracle_old", &text("old"), 3_600).await?;
        set_transient(w, "oracle_fresh", &text("fresh"), 3_600).await?;
        // set_site_transient() on a single site: the timeout, then the value.
        let off = Some(Autoload::Off);
        let timeout = Value::Int(crate::now() + 3_600);
        add_option(w, "_site_transient_timeout_oracle_site_old", &timeout, off).await?;
        add_option(w, "_site_transient_oracle_site_old", &text("old"), off).await?;
        add_option(w, "_transient_oracle_lone", &text("lone"), None).await?;
        Ok(())
    })
    .await?;
    let sql = "UPDATE wp_options SET option_value = '1000000000' WHERE option_name IN \
               ('_transient_timeout_oracle_old', '_site_transient_timeout_oracle_site_old')";
    raw(conn, sql, &[]).await
}

#[tokio::test]
async fn posts_are_published_as_php_publishes_them() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    // These goldens were recorded as user 1.
    let publish = async |conn: &mut Connection, id: u64| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_publish_post(w, ctx, id).await
        })
        .await
    };
    let future = async |conn: &mut Connection| insert_post(writer, conn, future_post()).await;
    check("publish-future-post", future, publish).await;
    let uncategorized = async |conn: &mut Connection| {
        let id = insert_post(writer, conn, oracle_post("draft")).await?;
        act(writer, conn, &admin(), async |w, _| {
            Ok(w.remove_relationships(id, &[1]).await?)
        })
        .await?;
        Ok(id)
    };
    check("publish-uncategorized-draft", uncategorized, publish).await;
    let page = PostArgs {
        post_type: "page".into(),
        ..oracle_post("draft")
    };
    let page = async |conn: &mut Connection| insert_post(writer, conn, page).await;
    check("publish-draft-page", page, publish).await;
}

#[tokio::test]
async fn publishing_what_is_missing_published_or_unported_writes_nothing() {
    rolled_back(async |w| {
        let ctx = cron();
        wp_publish_post(w, &ctx, MISSING).await.unwrap();
        wp_publish_post(w, &ctx, 1).await.unwrap();
        // No default category: a published draft stays without one.
        let zero = text("0");
        crate::options::update_option(w, "default_category", &zero, None)
            .await
            .unwrap();
        let admin = admin();
        let id = crate::posts::wp_insert_post(w, &admin, oracle_post("draft"));
        let id = id.await.unwrap();
        w.remove_relationships(id, &[1]).await.unwrap();
        wp_publish_post(w, &ctx, id).await.unwrap();
        assert!(w.object_terms(id, "category").await.unwrap().is_empty());
        // An attachment is not a ported type.
        let post = w.post(1).await.unwrap().unwrap();
        let attachment = hyscribe_db::PostFields {
            post_type: "attachment".into(),
            status: "inherit".into(),
            ..post
        };
        let id = w.insert_post(&attachment).await.unwrap();
        let refused = wp_publish_post(w, &ctx, id).await;
        assert!(matches!(refused, Err(crate::Error::Unported(_))));
    })
    .await;
}

#[tokio::test]
async fn missed_posts_are_published_and_early_events_moved_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let publish = async |conn: &mut Connection, id: u64| {
        act(writer, conn, &cron(), async |w, ctx| {
            check_and_publish_future_post(w, ctx, id).await
        })
        .await
    };
    check(
        "cron-publish-missed",
        async |conn| missed(writer, conn).await,
        publish,
    )
    .await;
    let early = async |conn: &mut Connection| {
        let id = insert_post(writer, conn, future_post()).await?;
        act(writer, conn, &admin(), async |w, ctx| {
            clear(w, "publish_future_post", id).await?;
            schedule(w, (ctx.now, OLD), "publish_future_post", Some(id)).await
        })
        .await?;
        Ok(id)
    };
    check("cron-publish-early", early, publish).await;
}

#[tokio::test]
async fn posts_not_scheduled_are_left_alone() {
    rolled_back(async |w| {
        let ctx = cron();
        check_and_publish_future_post(w, &ctx, MISSING)
            .await
            .unwrap();
        // The sample page is published.
        check_and_publish_future_post(w, &ctx, 2).await.unwrap();
        assert_eq!(w.post(2).await.unwrap().unwrap().status, "publish");
    })
    .await;
}

#[tokio::test]
async fn the_trash_is_emptied_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    check(
        "cron-scheduled-delete",
        async |conn| trash(writer, conn).await,
        async |conn, ()| {
            act(writer, conn, &cron(), async |w, ctx| {
                wp_scheduled_delete(w, ctx).await
            })
            .await
        },
    )
    .await;
}

#[tokio::test]
async fn expired_transients_are_deleted_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    check(
        "cron-expired-transients",
        async |conn| transients(writer, conn).await,
        async |conn, ()| {
            act(writer, conn, &cron(), async |w, ctx| {
                delete_expired_transients(w, ctx.now).await
            })
            .await
        },
    )
    .await;
}

#[tokio::test]
async fn old_auto_drafts_are_deleted_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let setup = async |conn: &mut Connection| {
        let old = insert_post(writer, conn, oracle_post("auto-draft")).await?;
        insert_post(writer, conn, oracle_post("auto-draft")).await?;
        let sql = "UPDATE wp_posts SET post_date = '2020-01-01 00:00:00' WHERE ID = ?";
        raw(conn, sql, &[&old.to_string()]).await
    };
    check("cron-auto-drafts", setup, async |conn, ()| {
        act(writer, conn, &cron(), async |w, ctx| {
            wp_delete_auto_drafts(w, ctx).await
        })
        .await
    })
    .await;
}

#[tokio::test]
async fn stale_privacy_requests_fail_as_php_s_do() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let setup = async |conn: &mut Connection| {
        let requests = [
            (
                "export_personal_data",
                "request-pending",
                "2020-01-01 00:00:00",
            ),
            (
                "remove_personal_data",
                "request-pending",
                "2999-01-01 00:00:00",
            ),
            (
                "remove_personal_data-2",
                "request-confirmed",
                "2020-01-01 00:00:00",
            ),
        ];
        for (name, status, modified) in requests {
            let sql = "INSERT INTO wp_posts (post_author, post_date, post_date_gmt, \
                       post_content, post_title, post_excerpt, post_status, post_password, \
                       post_name, to_ping, pinged, post_modified, post_modified_gmt, \
                       post_content_filtered, post_type, comment_status, ping_status) VALUES \
                       (0, '2020-01-01 00:00:00', '2020-01-01 00:00:00', '[]', \
                       'reader@example.test', '', ?, 'a-request-key', ?, '', '', ?, ?, '', \
                       'user_request', 'closed', 'closed')";
            raw(conn, sql, &[status, name, modified, modified]).await?;
        }
        Ok(())
    };
    check("cron-privacy-requests", setup, async |conn, ()| {
        act(writer, conn, &cron(), async |w, ctx| {
            wp_privacy_personal_data_cleanup_requests(w, ctx).await
        })
        .await
    })
    .await;
}

#[tokio::test]
async fn each_job_stops_at_a_database_error() {
    let any = Value::String(Vec::new());
    let ctx = cron();
    let auto_draft = PostArgs {
        date: "2020-01-01 00:00:00".into(),
        ..oracle_post("auto-draft")
    };
    let request = PostArgs {
        title: b"reader@example.test".to_vec(),
        content: b"[]".to_vec(),
        status: "request-pending".into(),
        post_type: "user_request".into(),
        date: "2020-01-01 00:00:00".into(),
        ..PostArgs::default()
    };
    fails_at_each_step(
        async |w| {
            let ctx = admin();
            let old = crate::posts::wp_insert_post(w, &ctx, oracle_post("publish")).await;
            let old = old.unwrap();
            wp_trash_post(w, &ctx, old).await.unwrap();
            let time = Value::Int(OLD);
            let key = "_wp_trash_meta_time";
            update_metadata(w, Meta::Post, old, key, &time, &any)
                .await
                .unwrap();
            add_metadata(w, Meta::Post, MISSING, key, &time, false)
                .await
                .unwrap();
            let comments = comments::Context {
                now: ctx.now,
                home: &ctx.home,
            };
            comments::trash_comment(w, comments, 1).await.unwrap();
            update_metadata(w, Meta::Comment, 1, key, &time, &any)
                .await
                .unwrap();
            add_metadata(w, Meta::Comment, MISSING, key, &time, false)
                .await
                .unwrap();
            for args in [auto_draft.clone(), request.clone()] {
                crate::posts::wp_insert_post(w, &ctx, args).await.unwrap();
            }
        },
        async |w, ()| {
            wp_scheduled_delete(w, &ctx).await?;
            delete_expired_transients(w, ctx.now).await?;
            wp_delete_auto_drafts(w, &ctx).await?;
            wp_privacy_personal_data_cleanup_requests(w, &ctx).await
        },
    )
    .await;
    fails_at_each_step(
        async |w| {
            let id = crate::posts::wp_insert_post(w, &admin(), future_post()).await;
            id.unwrap()
        },
        async |w, id| check_and_publish_future_post(w, &ctx, id).await,
    )
    .await;
}

/// A new folder of `name` in the system's temporary folder.
fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("hyscribe-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// A file at `path` last changed `age` seconds before `now`.
fn file(path: &Path, now: i64, age: i64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let time = UNIX_EPOCH + std::time::Duration::from_secs((now - age) as u64);
    file.set_modified(time).unwrap();
}

#[test]
fn export_files_older_than_three_days_are_deleted() {
    let uploads = folder("exports");
    let exports = uploads.join("wp-personal-data-exports");
    let now = crate::now();
    let old = 3 * DAY + 1;
    for name in [
        "old.zip",
        "index.php",
        ".hidden",
        "nested/old.html",
        "nested/index.php",
        "nested/.hidden/old.zip",
    ] {
        file(&exports.join(name), now, old);
    }
    file(&exports.join("new.zip"), now, 3 * DAY);
    file(&uploads.join("outside.zip"), now, old);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&uploads, exports.join("link")).unwrap();
    wp_privacy_delete_old_export_files(&uploads, now);
    let left = |name: &str| exports.join(name).exists();
    assert!(!left("old.zip") && !left("nested/old.html") && !left("nested/index.php"));
    assert!(left("index.php") && left(".hidden") && left("nested/.hidden/old.zip"));
    assert!(left("new.zip") && uploads.join("outside.zip").exists());
    // A missing folder deletes nothing.
    wp_privacy_delete_old_export_files(&uploads.join("missing"), now);
    std::fs::remove_dir_all(&uploads).unwrap();
}

#[test]
fn files_are_listed_a_hundred_folders_deep() {
    let top = folder("deep");
    let mut deepest = top.clone();
    for _ in 0..100 {
        deepest = deepest.join("d");
    }
    file(&top.join("a/b.zip"), 0, 0);
    file(&deepest.join("lost.zip"), 0, 0);
    assert_eq!(list_files(&top, 100, &[]), [top.join("a/b.zip")]);
    assert_eq!(list_files(&top, 0, &[]), Vec::<PathBuf>::new());
    std::fs::remove_dir_all(&top).unwrap();
}

#[tokio::test]
async fn the_jobs_keep_to_php_s_ages() {
    rolled_back(async |w| {
        let ctx = cron();
        // A post is published at its very time.
        let admin = admin();
        let id = crate::posts::wp_insert_post(w, &admin, future_post());
        let id = id.await.unwrap();
        let at = Context {
            now: 1_893_492_000,
            ..cron()
        };
        check_and_publish_future_post(w, &at, id).await.unwrap();
        assert_eq!(w.post(id).await.unwrap().unwrap().status, "publish");
        // A comment two days in the trash stays; one past thirty days goes.
        let comments = comments::Context {
            now: ctx.now,
            home: &ctx.home,
        };
        comments::trash_comment(w, comments, 1).await.unwrap();
        let key = "_wp_trash_meta_time";
        let any = Value::String(Vec::new());
        for (age, kept) in [(2 * DAY, true), (30 * DAY + 1, false)] {
            let time = Value::Int(ctx.now - age);
            update_metadata(w, Meta::Comment, 1, key, &time, &any)
                .await
                .unwrap();
            wp_scheduled_delete(w, &ctx).await.unwrap();
            assert_eq!(w.comment(1).await.unwrap().is_some(), kept);
        }
        // A request changed two hours ago stays pending; a day and more fails.
        let zone = wp_timezone(w).await.unwrap();
        for (age, status) in [(7_200, "request-pending"), (DAY + 10, "request-failed")] {
            let request = PostArgs {
                title: b"reader@example.test".to_vec(),
                status: "request-pending".into(),
                post_type: "user_request".into(),
                date: format_unix(MYSQL, ctx.now - age, zone),
                ..PostArgs::default()
            };
            let id = crate::posts::wp_insert_post(w, &admin, request)
                .await
                .unwrap();
            wp_privacy_personal_data_cleanup_requests(w, &ctx)
                .await
                .unwrap();
            assert_eq!(w.post(id).await.unwrap().unwrap().status, status);
            // wp_delete_post() trashes only posts and pages.
            assert!(wp_delete_post(w, &ctx, id, false).await.unwrap());
            assert!(w.post(id).await.unwrap().is_none());
        }
    })
    .await;
}
