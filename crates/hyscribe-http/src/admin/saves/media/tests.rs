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
            "upload?deleted=2",
        ),
        (
            "media-bulk-delete-media-trash",
            "action=delete&media[]=801&media[]=810",
            "upload?deleted=2",
        ),
        (
            "media-bulk-trash-media-trash",
            "action=trash&media[]=801&media[]=802",
            "upload?trashed=2&ids=801,802",
        ),
        (
            "media-bulk-untrash-media-trash",
            "action=untrash&media[]=810",
            "upload?untrashed=1",
        ),
        (
            "media-bulk-attach",
            "action=attach&found_post_id=2&media[]=802&media[]=803&media[]=99999",
            "upload?attached=2",
        ),
        (
            "media-bulk-attach-same-parent",
            "action=attach&found_post_id=1&media[]=801&media[]=803",
            "upload?attached=1",
        ),
        (
            "media-bulk-detach",
            "action=detach&parent_post_id=1&media[]=801&media[]=802&media[]=807",
            "upload?detach=2",
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

/// The attachment edit screen's saves write PHP's rows and send the user
/// back to the screen with the message `redirect_post()` picks.
#[tokio::test]
async fn edits_write_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    let image = "post_author=2&post_author_override=1&post_title=New+%22title%22+%5C+%26+more\
        &excerpt=A+%3Cem%3Ecaption%3C%2Fem%3E\
        &content=Some+%3Cb%3Edescription%3C%2Fb%3E+with+a+%5C+backslash\
        &_wp_attachment_image_alt=%3Cb%3EAlt%3C%2Fb%3E+text&comment_status=open\
        &post_name=new-slug&advanced_view=1&save=Update";
    let same_alt = "post_author=2&post_title=Sunset+%22dusk%22+%26amp%3B+sea\
        &_wp_attachment_image_alt=Alt+%22text%22+%26amp%3B+more\
        &post_name=sunset-dusk-sea&save=Update";
    let pdf = "post_author=1&post_title=Report+2026&comment_status=open&ping_status=open\
        &post_name=report&save=Update";
    let song = "post_author=2&post_title=Song&id3_artist=New+%3Cb%3Eartist%3C%2Fb%3E\
        &id3_album=Second&id3_genre=Ignored&_thumbnail_id=-1&comment_status=open&save=Update";
    let by_admin = "post_author=1&save=Update";
    let video = "post_author=1&id3_artist=Director&_thumbnail_id=-1&save=Update";
    let untouched = "post_author=2&_thumbnail_id=801&save=Update";
    let notes = "post_author=3&post_title=Notes&save=Update";
    for (name, id, body, message) in [
        ("media-edit-image", "801", image, "1"),
        ("media-edit-same-alt", "801", same_alt, "1"),
        ("media-edit-pdf", "802", pdf, "1"),
        (
            "media-edit-gone-author",
            "805",
            "post_author=99&save=Update",
            "10",
        ),
        ("media-edit-private-parent", "803", notes, "1"),
        ("media-edit-pending-parent", "808", by_admin, "8"),
        ("media-edit-trashed-parent", "813", by_admin, "10"),
        ("media-edit-private-attachment", "811", by_admin, "1"),
        ("media-edit-trashed", "810", by_admin, "1"),
        ("media-edit-song", "817", song, "1"),
        ("media-edit-song-untouched", "817", untouched, "1"),
        ("media-edit-video", "818", video, "1"),
        ("media-edit-no-save-button", "802", "post_author=1", "4"),
    ] {
        let body = format!("post_ID={id}&post_type=attachment&action=editpost&{body}");
        let asked = ("post", &b"editpost"[..], "", body.as_str());
        let edit = format!("http://127.0.0.1:8081/hs-admin/post?post={id}&action=edit");
        check(writer, name, asked, &format!("{edit}&message={message}")).await;
    }
}

/// What `handler( screen, action )` answers `user`'s `query` and `body` sent
/// from `referer`, after `sql` on the oracle's content, all rolled back.
async fn tried(
    user: u64,
    sql: &[&str],
    (screen, action, query, body): (&str, &[u8], &str, &str),
    referer: Option<&str>,
) -> Result<Answer, String> {
    use sqlx::Acquire as _;
    let fixture = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&fixture.host)
        .port(fixture.port)
        .database(&fixture.name)
        .username(&fixture.user)
        .password(fixture.password.expose());
    let mut conn = <Connection as sqlx::Connection>::connect_with(&options)
        .await
        .unwrap();
    let mut outer = conn.begin().await.unwrap();
    let conn = outer.acquire().await.unwrap();
    content(conn).await.unwrap();
    for statement in sql {
        let statement = sqlx::raw_sql(sqlx::AssertSqlSafe(statement.to_string()));
        conn.execute(statement).await.unwrap();
    }
    let (get, post) = (fields(query), fields(body));
    let uri = format!("/hs-admin/{screen}?{query}");
    let asked = Asked {
        user,
        referer: referer.map(str::as_bytes),
        ..asked(&uri, &get, &post)
    };
    let handler = handler(screen, action).unwrap();
    let site = Unread::default();
    let answer = acted(&writer().await, conn, (handler, &site), &asked).await;
    answer.map_err(|err| format!("{err:?}"))
}

fn unported(answer: &Result<Answer, String>) -> bool {
    matches!(answer, Err(err) if err.contains("Unported"))
}

/// The edit screen's saves PHP dies on or this port cannot take.
#[tokio::test]
async fn edits_stop_as_php_stops() {
    let _turn = turn().await;
    let edit = async |user: u64, sql: &[&str], body: &str| {
        let body = format!("post_type=attachment&action=editpost&{body}");
        tried(user, sql, ("post", b"editpost", "", &body), None).await
    };
    for body in [
        "post_ID=801&meta[1][key]=a",
        "post_ID=801&post_title[]=a",
        "post_ID=801&post_status=draft",
        "post_ID=99999",
        "post_ID=",
        "post_ID=1",
        "post_ID=801&_thumbnail_id=-1",
        "post_ID=817&_thumbnail_id=805",
        "post_ID=817&id3_artist[]=a",
    ] {
        let answer = edit(1, &[], body).await;
        assert!(unported(&answer), "{body}: {answer:?}");
    }
    let thumbnail = |id: &str| {
        format!(
            "UPDATE wp_postmeta SET meta_value = '{id}' \
             WHERE post_id = 817 AND meta_key = '_thumbnail_id'"
        )
    };
    for stored in ["802", "99999"] {
        let body = format!("post_ID=817&_thumbnail_id={stored}");
        let answer = edit(1, &[&thumbnail(stored)], &body).await;
        assert!(unported(&answer), "{stored}: {answer:?}");
    }
    let no_file = "DELETE FROM wp_postmeta WHERE post_id = 817 AND meta_key = '_wp_attached_file'";
    let answer = edit(1, &[no_file], "post_ID=817&_thumbnail_id=-1").await;
    assert!(unported(&answer), "{answer:?}");
    // Tags are the files' of songs and videos only.
    let answer = edit(1, &[], "post_ID=801&id3_artist=x").await;
    assert!(answer.is_ok(), "{answer:?}");
    // Who may not edit it is refused, and who may not edit as another user.
    let message = "Sorry, you are not allowed to edit this post.";
    assert_eq!(edit(3, &[], "post_ID=801").await, Ok(Answer::Died(message)));
    let author =
        "UPDATE wp_usermeta SET meta_value = 'a:1:{s:6:\"author\";b:1;}' WHERE umeta_id = 805";
    let message = "Sorry, you are not allowed to edit posts as this user.";
    let answer = edit(3, &[author], "post_ID=803&post_author_override=1").await;
    assert_eq!(answer, Ok(Answer::Died(message)));
    let answer = edit(3, &[author], "post_ID=803&post_author=3").await;
    assert!(answer.is_ok(), "{answer:?}");
    // Links to attachment pages are fixed under pretty permalinks, by status.
    let links =
        "UPDATE wp_posts SET post_content = '<a href=\"?attachment_id=5\">x</a>' WHERE ID = 801";
    let private = "UPDATE wp_posts SET post_status = 'private' WHERE ID = 801";
    let answer = edit(1, &[links], "post_ID=801").await;
    assert!(answer.is_ok(), "{answer:?}");
    let answer = edit(1, &[links, private], "post_ID=801").await;
    assert!(unported(&answer), "{answer:?}");
    let answer = edit(1, &[private], "post_ID=801").await;
    assert!(answer.is_ok(), "{answer:?}");
}

/// The message the edit screen is sent back with is by the status the
/// attachment has: its parent's, its own or published.
#[tokio::test]
async fn edits_are_answered_by_status() {
    let _turn = turn().await;
    let saved = async |id: u64, sql: &[&str]| {
        let body = format!("post_ID={id}&post_type=attachment&action=editpost&save=Update");
        tried(1, sql, ("post", b"editpost", "", &body), None).await
    };
    let to = |id: u64, message: &str| {
        let edit = format!("http://127.0.0.1:8081/hs-admin/post?post={id}&action=edit");
        Ok(Answer::Redirect(format!("{edit}&message={message}")))
    };
    let parent = |id: u64| format!("UPDATE wp_posts SET post_parent = {id} WHERE ID = 801");
    let future = "UPDATE wp_posts SET post_status = 'future' WHERE ID = 1";
    let draft = "UPDATE wp_posts SET post_status = 'draft' WHERE ID = 1";
    assert_eq!(saved(801, &[future]).await, to(801, "9"));
    assert_eq!(saved(801, &[draft]).await, to(801, "10"));
    for id in [0, 801, 99999] {
        assert_eq!(saved(801, &[&parent(id)]).await, to(801, "1"), "{id}");
    }
    assert!(unported(&saved(801, &[&parent(802)]).await));
    // A trashed parent without its status kept is taken as published.
    assert_eq!(saved(815, &[]).await, to(815, "1"));
}

/// Attaching and detaching, which PHP does as the user may edit the post and the media.
#[tokio::test]
async fn attaching_stops_as_php_stops() {
    let _turn = turn().await;
    let list = |query: &str, referer: Option<&str>, user: u64| {
        let action = query.split(['=', '&']).nth(1).unwrap().as_bytes().to_vec();
        let query = query.to_string();
        let referer = referer.map(str::to_string);
        async move {
            let asked = ("upload", &action[..], query.as_str(), "");
            tried(user, &[], asked, referer.as_deref()).await
        }
    };
    let to = |location: &str| Ok(Answer::Redirect(location.into()));
    for query in [
        "action=attach&found_post_id=&media[]=801",
        "action=attach&found_post_id=0&media[]=801",
        "action=detach&parent_post_id=&media[]=801",
        "action=attach&found_post_id=2&media[]=99999",
        "action=detach&parent_post_id=2&media[]=99999&media[]=-5",
    ] {
        assert_eq!(list(query, None, 1).await, to("upload"), "{query}");
    }
    let message = "Sorry, you are not allowed to edit this post.";
    for (query, user) in [
        ("action=attach&found_post_id=2&media[]=801", 3),
        ("action=attach&found_post_id=99999&media[]=801", 1),
        ("action=attach&found_post_id=-5&media[]=801", 1),
        ("action=attach&found_post_id=abc&media[]=801", 1),
    ] {
        let answer = list(query, None, user).await;
        assert_eq!(answer, Ok(Answer::Died(message)), "{query}");
    }
    let answer = list("action=attach&found_post_id[]=2&media[]=801", None, 1).await;
    assert!(unported(&answer), "{answer:?}");
    // A single file, and the list the user came from.
    let query = "action=attach&found_post_id=2&media=802";
    assert_eq!(list(query, None, 1).await, to("upload?attached=1"));
    let from = |path: &str| Some(format!("http://127.0.0.1:8081/hs-admin/{path}"));
    let referer = from("upload?mode=list&attached=3&detach=4");
    let answer = list(query, referer.as_deref(), 1).await;
    assert_eq!(
        answer,
        to("http://127.0.0.1:8081/hs-admin/upload?mode=list&attached=1")
    );
    let answer = list(query, from("edit").as_deref(), 1).await;
    assert_eq!(answer, to("upload?attached=1"));
    let query = "action=detach&parent_post_id=1&media[]=801";
    let answer = list(query, referer.as_deref(), 1).await;
    assert_eq!(
        answer,
        to("http://127.0.0.1:8081/hs-admin/upload?mode=list&detach=1")
    );
}
