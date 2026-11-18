// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media library's handlers through `act()` against the write oracle
//! (`reference/writes/actions-media.php`) over
//! `reference/attachment-edit/content.sql`: each must write PHP's rows.

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_config::Constants;
use hyscribe_db::{Connection, Writer};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use sqlx::Executor as _;

use crate::admin::saves::tests::unread::Unread;
use crate::admin::saves::tests::{acted, asked, fields, turn, writer};
use crate::admin::saves::{Ajax, ajax_handler, handler};

/// What the handler of `screen` and `action` answers `query` and `body` on
/// a site with `MEDIA_TRASH` as `trash`.
async fn answered(
    writer: &Writer,
    conn: &mut Connection,
    (screen, action, trash): (&str, &[u8], bool),
    (query, body): (&str, &str),
) -> Result<String, String> {
    let (get, post) = (fields(query), fields(body));
    let uri = format!("/hs-admin/{screen}?{query}");
    let asked = Asked {
        now: crate::now(),
        ..asked(&uri, &get, &post)
    };
    let constants = Constants {
        media_trash: trash,
        ..Constants::default()
    };
    let site = Unread(constants);
    let answer = match screen {
        "admin-ajax" => {
            let handler = ajax_handler(action).unwrap();
            match acted(writer, conn, (handler, &site), &asked).await {
                Ok(Ajax::Text(text) | Ajax::Json(text)) => Ok(text),
                Ok(Ajax::Attachment(id)) => Ok(id.to_string()),
                other => Err(format!("{other:?}")),
            }
        }
        _ => {
            let handler = handler(screen, action).unwrap();
            match acted(writer, conn, (handler, &site), &asked).await {
                Ok(Answer::Redirect(to)) => Ok(to),
                other => Err(format!("{other:?}")),
            }
        }
    };
    answer.map_err(|err| format!("{screen} {query} {body}: {err}"))
}

/// The content the oracle's actions start from.
async fn content(conn: &mut Connection) -> Result<(), String> {
    let sql = include_str!("../../../../../../reference/attachment-edit/content.sql");
    let sql = sqlx::AssertSqlSafe(sql.to_string());
    let done = conn.execute(sqlx::raw_sql(sql)).await;
    done.map(|_| ()).map_err(|err| err.to_string())
}

/// The golden `name` of the request, with the answer it must give.
async fn check(writer: &Writer, name: &str, asked: (&str, &[u8], &str, &str), expected: &str) {
    let (screen, action, query, body) = asked;
    let trash = name.ends_with("-media-trash");
    let act = async |conn: &mut Connection, ()| {
        let answer = answered(writer, conn, (screen, action, trash), (query, body)).await?;
        assert_eq!(answer, expected, "{name}");
        Ok(())
    };
    let checked = check_writes(&fixture_database(), name, content, act).await;
    checked.unwrap_or_else(|differences| panic!("{differences}"));
}

/// The grid's and the modal's saves write PHP's rows.
#[tokio::test]
async fn saves_write_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    let ajax = |action: &'static str| ("admin-ajax", action.as_bytes(), "");
    let saved = "{\"success\":true}";
    for (name, body) in [
        (
            "media-save-attachment",
            "id=801&changes[title]=New+%22title%22+%26amp%3B+more&changes[caption]=A+%3Cem%3Ecaption%3C%2Fem%3E&changes[description]=Some+%3Cb%3Edescription%3C%2Fb%3E+with+a+%5C+backslash&changes[alt]=%3Cb%3EAlt%3C%2Fb%3E+text",
        ),
        (
            "media-save-attachment-parent",
            "id=802&changes[parent]=2&changes[alt]=Same&changes[menuOrder]=3",
        ),
        (
            "media-save-attachment-same-alt",
            "id=801&changes[alt]=Alt+%22text%22+%26amp%3B+more",
        ),
        (
            "media-save-attachment-audio",
            "id=817&changes[artist]=New+%3Cb%3Eartist%3C%2Fb%3E&changes[album]=Second&changes[title]=Song",
        ),
        (
            "media-save-attachment-audio-unread",
            "id=820&changes[artist]=Someone",
        ),
        (
            "media-save-attachment-not-audio",
            "id=802&changes[artist]=Nobody",
        ),
        (
            "media-save-attachment-status",
            "id=801&changes[status]=trash&changes[title]=Kept",
        ),
        (
            "media-save-attachment-trash-media-trash",
            "id=801&changes[status]=trash&changes[title]=Dropped&changes[alt]=Changed",
        ),
        (
            "media-save-attachment-inherit-media-trash",
            "id=810&changes[status]=inherit",
        ),
        (
            "media-save-attachment-private-media-trash",
            "id=802&changes[status]=private",
        ),
    ] {
        let (screen, action, query) = ajax("save-attachment");
        check(writer, name, (screen, action, query, body), saved).await;
    }
    for (name, id) in [
        ("media-save-attachment-compat", "801"),
        ("media-save-attachment-compat-untitled", "805"),
    ] {
        let (screen, action, query) = ajax("save-attachment-compat");
        let body = format!("id={id}&attachments[{id}][a]=b");
        check(writer, name, (screen, action, query, &body), id).await;
    }
}

/// The grid's deletes, the list's bulk actions and `post.php`'s write
/// PHP's rows.
#[tokio::test]
async fn deletes_write_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    for (name, id) in [
        ("media-delete-post", "801"),
        ("media-delete-post-song", "817"),
        ("media-delete-post-media-trash", "801"),
        ("media-delete-post-trashed-media-trash", "810"),
    ] {
        let body = format!("id={id}");
        let asked = ("admin-ajax", &b"delete-post"[..], "", body.as_str());
        check(writer, name, asked, "1").await;
    }
    let upload = "http://127.0.0.1:8081/hs-admin/upload";
    for (name, query, location) in [
        (
            "media-bulk-delete",
            "action=delete&media[]=801&media[]=802",
            "upload.php?deleted=2",
        ),
        (
            "media-bulk-delete-media-trash",
            "action=delete&media[]=801&media[]=810",
            "upload.php?deleted=2",
        ),
        (
            "media-bulk-trash-media-trash",
            "action=trash&media[]=801&media[]=802",
            "upload.php?trashed=2&ids=801,802",
        ),
        (
            "media-bulk-untrash-media-trash",
            "action=untrash&media[]=810",
            "upload.php?untrashed=1",
        ),
    ] {
        let action = query.split(['=', '&']).nth(1).unwrap();
        let asked = ("upload", action.as_bytes(), query, "");
        check(writer, name, asked, location).await;
    }
    for (name, id, action, notice) in [
        ("media-post-delete", "801", "delete", "deleted=1"),
        (
            "media-post-delete-media-trash",
            "810",
            "delete",
            "deleted=1",
        ),
        (
            "media-post-trash-media-trash",
            "801",
            "trash",
            "trashed=1&ids=801",
        ),
        (
            "media-post-untrash-media-trash",
            "810",
            "untrash",
            "untrashed=1&ids=810",
        ),
    ] {
        let query = format!("post={id}&action={action}");
        let asked = ("post", action.as_bytes(), query.as_str(), "");
        check(writer, name, asked, &format!("{upload}?{notice}")).await;
    }
}
