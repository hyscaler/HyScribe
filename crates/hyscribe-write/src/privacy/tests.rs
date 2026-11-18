// SPDX-License-Identifier: AGPL-3.0-or-later

//! A confirmation link's writes against the write oracle's golden
//! (`reference/writes/actions-privacy.php`).

use std::result::Result as StdResult;
use std::sync::Mutex;

use hyscribe_db::Connection;

use super::*;
use crate::posts::tests::{act, check, shown};
use crate::posts::{PostArgs, wp_insert_post};
use crate::tests::{ONE_AT_A_TIME, admin, fails_at_each_step, writer};

fn ctx() -> Context {
    Context { user: 0, ..admin() }
}

/// `$privacy_requests()`: seven requests written raw and the notice the
/// confirmed erasure was told of; their IDs.
async fn requests(conn: &mut Connection) -> StdResult<Vec<u64>, String> {
    let rows = [
        ("export_personal_data", "request-pending"),
        ("remove_personal_data", "request-pending"),
        ("export_personal_data", "request-failed"),
        ("export_personal_data", "request-confirmed"),
        ("remove_personal_data", "request-confirmed"),
        ("export_personal_data", "request-completed"),
        ("export_personal_data", "request-pending"),
    ];
    let mut ids = Vec::new();
    for (name, status) in rows {
        let sql = "INSERT INTO wp_posts (post_author, post_date, post_date_gmt, \
                   post_content, post_title, post_excerpt, post_status, post_password, \
                   post_name, to_ping, pinged, post_modified, post_modified_gmt, \
                   post_content_filtered, post_type, comment_status, ping_status) VALUES \
                   (0, '2020-01-01 00:00:00', '2020-01-01 00:00:00', '[]', \
                   'reader@example.test', '', ?, 'a-request-key', ?, '', '', \
                   '2020-01-01 00:00:00', '2020-01-01 00:00:00', '', \
                   'user_request', 'closed', 'closed')";
        let done = sqlx::query(sql).bind(status).bind(name);
        ids.push(
            done.execute(&mut *conn)
                .await
                .map_err(shown)?
                .last_insert_id(),
        );
    }
    let meta = "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) \
                VALUES (?, '_wp_admin_notified', '1')";
    let told = sqlx::query(meta).bind(ids[4]);
    told.execute(&mut *conn).await.map_err(shown)?;
    Ok(ids)
}

/// Each request confirmed as PHP confirms it: pending and failed ones
/// become confirmed at the time of the link, and the admin is to be told of
/// every confirmed one they were not told of; the rest, and a post that is
/// no request, are left alone.
#[tokio::test]
async fn confirms_requests_as_php_does() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let (told, ctx) = (Mutex::new(Vec::new()), ctx());
    check("login-confirm-request", requests, async |conn, ids| {
        let negative = -ids[6].cast_signed();
        let asked = ids
            .into_iter()
            .take(6)
            .map(u64::cast_signed)
            .chain([1, negative]);
        for id in asked {
            let asked = act(writer, conn, &ctx, async |w, ctx| {
                let asked = user_request_action_confirmed(w, ctx, id).await?;
                if asked.is_some() {
                    admin_notified(w, id.unsigned_abs()).await?;
                }
                Ok(asked)
            });
            told.lock().unwrap().push(asked.await?);
        }
        Ok(())
    })
    .await;
    let export = || Confirmed {
        email: b"reader@example.test".to_vec(),
        action_name: b"export_personal_data".to_vec(),
    };
    let erase = Confirmed {
        action_name: b"remove_personal_data".to_vec(),
        ..export()
    };
    let told = told.into_inner().unwrap();
    assert_eq!(
        told,
        [
            Some(export()),
            Some(erase),
            Some(export()),
            Some(export()),
            None,
            None,
            None,
            None
        ]
    );
}

/// Every failed statement of a confirmation fails it, for a request, for a
/// post that is none and for a post that is no request.
#[tokio::test]
async fn confirmations_return_each_database_error() {
    let request = PostArgs {
        title: b"reader@example.test".to_vec(),
        content: b"[]".to_vec(),
        status: "request-pending".into(),
        post_type: "user_request".into(),
        ..PostArgs::default()
    };
    let setup = async |write: &mut Write<'_>| {
        wp_insert_post(write, &ctx(), request.clone())
            .await
            .unwrap()
    };
    fails_at_each_step(setup, async |write, id| {
        let id = id.cast_signed();
        let asked = user_request_action_confirmed(write, &ctx(), id).await?;
        assert!(asked.is_some());
        admin_notified(write, id.cast_unsigned()).await?;
        for other in [1, 999_999_999, -1] {
            let asked = user_request_action_confirmed(write, &ctx(), other).await?;
            assert!(asked.is_none());
        }
        let again = user_request_action_confirmed(write, &ctx(), id).await?;
        assert!(again.is_none());
        Ok(())
    })
    .await;
}
