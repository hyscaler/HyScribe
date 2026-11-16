// SPDX-License-Identifier: AGPL-3.0-or-later

//! The posts lists' handlers through `act()` against the write oracle
//! (`reference/writes/actions-posts-admin.php`): each setup writes what
//! PHP's does, and the action must write PHP's rows.

use std::sync::Arc;

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Writer};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::users::{UserData, wp_insert_user};

use super::*;
use crate::admin::saves::tests::unread::Unread;
use crate::admin::saves::tests::{SITE, acted, asked, fields, turn, writer};
use crate::admin::saves::{Ajax, ajax_handler, handler};

/// What `screen`'s handler answers `user`'s `query` and `body` on a site
/// with `constants`, as one action on `conn` once every statement budget
/// short of it has failed.
async fn answered(
    writer: &Writer,
    conn: &mut Connection,
    (user, screen, constants): (u64, &str, Constants),
    (query, body): (&str, &str),
) -> Result<Answer, String> {
    let (get, post) = (fields(query), fields(body));
    let uri = format!("/hs-admin/{screen}?{query}");
    let asked = Asked {
        user,
        now: crate::now(),
        ..asked(&uri, &get, &post)
    };
    let action = action(screen, &asked);
    let answer = match screen {
        // Quick Edit's answers as pages: its text, or nothing for its row.
        "admin-ajax" => {
            let handler = ajax_handler(&action).unwrap();
            let saved = acted(writer, conn, (handler, &Unread(constants)), &asked).await;
            saved.map(|saved| match saved {
                Ajax::Text(text) => Answer::Page(text),
                _ => Answer::Page(String::new()),
            })
        }
        _ => {
            let handler = handler(screen, &action).unwrap();
            acted(writer, conn, (handler, &Unread(constants)), &asked).await
        }
    };
    answer.map_err(|err| format!("{err:?}"))
}

/// User 1's context, as the setups write.
fn admin() -> Context {
    Context {
        can_publish: true,
        ..crate::admin::saves::dashboard::context(1, SITE, SITE, Default::default())
    }
}

/// Runs `f` as one committed action on `conn`, as a setup does.
async fn setup<T>(
    writer: &Writer,
    conn: &mut Connection,
    f: impl AsyncFnOnce(&mut Write<'_>, &Context) -> Result<T, Error>,
) -> Result<T, String> {
    let mut write = writer
        .begin_on(conn)
        .await
        .map_err(|err| format!("{err:?}"))?;
    let value = f(&mut write, &admin())
        .await
        .map_err(|err| format!("{err:?}"))?;
    write.commit().await.map_err(|err| format!("{err:?}"))?;
    Ok(value)
}

/// `$posts_admin_lock( $id )`: another user's fresh edit lock.
async fn lock(write: &mut Write<'_>, id: u64) -> Result<(), Error> {
    let some = |text: &str| Some(text.as_bytes().to_vec());
    let user = UserData {
        user_login: some("oracle-editing"),
        user_pass: some("A pass phrase"),
        user_email: some("oracle-editing@example.test"),
        ..UserData::default()
    };
    let user = wp_insert_user(write, &user).await?;
    let lock = Value::String(format!("{}:{user}", crate::now()).into_bytes());
    update_metadata(write, Meta::Post, id, "_edit_lock", &lock, &Value::Null).await?;
    Ok(())
}

/// The post with `import_id`, `title`, `status` and `post_type`.
fn oracle_post(import_id: u64, title: &str, (status, post_type): (&str, &str)) -> PostArgs {
    PostArgs {
        import_id,
        title: title.as_bytes().to_vec(),
        content: b"Its content.".to_vec(),
        status: status.into(),
        post_type: post_type.into(),
        author: Some(1),
        ..PostArgs::default()
    }
}

/// `$posts_admin_quick( $id )`'s fields, `more` over them.
fn quick(id: u64, more: &str) -> String {
    let mut fields = format!(
        "mm=10&jj=27&aa=2026&hh=08&mn=08&ss=15&post_author=1&post_password=&_status=publish\
         &post_view=list&action=inline-save&post_ID={id}&edit_date=true&post_status=\
         &hidden_mm=10&hidden_jj=27&hidden_aa=2026&hidden_hh=08&hidden_mn=08"
    );
    if id == 1 {
        fields.push_str("&comment_status=open&ping_status=open&post_format=0");
    }
    format!("{fields}&{more}")
}

async fn check<T>(
    golden: &str,
    setup: impl AsyncFnOnce(&mut Connection) -> Result<T, String>,
    action: impl AsyncFnOnce(&mut Connection, T) -> Result<(), String>,
) {
    let checked = check_writes(&fixture_database(), golden, setup, action).await;
    checked.unwrap_or_else(|differences| panic!("{differences}"));
}

/// `post.php`'s trash, restore and delete write PHP's rows.
#[tokio::test]
async fn post_php_writes_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    let trashed = async |conn: &mut Connection| {
        setup(writer, conn, async |w, ctx| wp_trash_post(w, ctx, 1).await).await
    };
    let post_php = async |conn: &mut Connection, query: &str| {
        let answer = answered(writer, conn, (1, "post", Constants::default()), (query, "")).await?;
        assert!(matches!(answer, Answer::Redirect(_)), "{query}");
        Ok(())
    };
    check(
        "admin-post-trash",
        async |_| Ok(()),
        async |conn, ()| post_php(conn, "post=1&action=trash").await,
    )
    .await;
    check("admin-post-untrash", trashed, async |conn, _| {
        post_php(conn, "post=1&action=untrash").await
    })
    .await;
    check("admin-post-delete", trashed, async |conn, _| {
        post_php(conn, "post=1&action=delete").await
    })
    .await;
}

/// `edit.php`'s bulk actions and Empty Trash write PHP's rows.
#[tokio::test]
async fn bulk_actions_write_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    let trashed = async |conn: &mut Connection, ids: &[u64]| {
        setup(writer, conn, async |w, ctx| {
            for &id in ids {
                wp_trash_post(w, ctx, id).await?;
            }
            Ok(())
        })
        .await
    };
    let edit = async |conn: &mut Connection, query: &str| {
        let answer = answered(writer, conn, (1, "edit", Constants::default()), (query, "")).await?;
        assert!(matches!(answer, Answer::Redirect(_)), "{query}");
        Ok(())
    };
    check(
        "admin-posts-bulk-trash",
        async |conn| setup(writer, conn, async |w, _| lock(w, 2).await).await,
        async |conn, ()| edit(conn, "post_type=page&action=trash&post[]=2&post[]=3").await,
    )
    .await;
    check(
        "admin-posts-bulk-undo",
        async |conn| trashed(conn, &[1]).await,
        async |conn, ()| edit(conn, "doaction=undo&action=untrash&ids=1").await,
    )
    .await;
    check(
        "admin-posts-bulk-delete",
        async |conn| trashed(conn, &[1]).await,
        async |conn, ()| edit(conn, "action=delete&post[]=1&post[]=2").await,
    )
    .await;
    check(
        "admin-posts-empty-trash",
        async |conn| trashed(conn, &[1, 2]).await,
        async |conn, ()| edit(conn, "action=-1&delete_all=Empty+Trash&post_status=trash").await,
    )
    .await;
    check(
        "admin-posts-bulk-edit",
        async |conn| {
            setup(writer, conn, async |w, ctx| {
                let post = oracle_post(960, "A bulk edited post", ("draft", "post"));
                hyscribe_write::posts::wp_insert_post(w, ctx, post).await
            })
            .await
        },
        async |conn, id| {
            let query = format!(
                "post_type=post&action=edit&bulk_edit=Update&post[]=1&post[]={id}\
                 &post_category[]=0&tax_input[post_tag]=Bulk+tag&post_author=-1\
                 &comment_status=closed&ping_status=&_status=pending&post_format=-1&sticky=sticky"
            );
            edit(conn, &query).await
        },
    )
    .await;
}

/// Quick Edit of a post and of a page writes PHP's rows.
#[tokio::test]
async fn quick_edit_writes_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    let quick_edit = async |conn: &mut Connection, body: &str, constants| {
        let answer = answered(writer, conn, (1, "admin-ajax", constants), ("", body)).await?;
        assert_eq!(answer, Answer::Page(String::new()));
        Ok(())
    };
    check(
        "admin-posts-inline-save",
        async |_| Ok(()),
        async |conn, ()| {
            let more = "post_title=Hello+again&post_name=hello-again&jj=28&screen=edit-post\
                        &post_type=post&post_category[]=0&post_category[]=1\
                        &tax_input[post_tag]=First+tag,+second+,+,&sticky=sticky";
            quick_edit(conn, &quick(1, more), Constants::default()).await
        },
    )
    .await;
    check(
        "admin-pages-inline-save",
        async |conn| {
            setup(writer, conn, async |w, ctx| {
                let page = oracle_post(0, "A parent page", ("publish", "page"));
                hyscribe_write::posts::wp_insert_post(
                    w,
                    ctx,
                    PostArgs {
                        content: Vec::new(),
                        ..page
                    },
                )
                .await
            })
            .await
        },
        async |conn, parent| {
            let more = format!(
                "post_title=Sample+Page&post_name=sample-page&post_type=page&screen=edit-page\
                 &post_parent={parent}&menu_order=3&page_template=default"
            );
            quick_edit(conn, &quick(2, &more), Constants::default()).await
        },
    )
    .await;
    // A title with HTML, filtered, then as an owner's .env lets user 1 post
    // it unfiltered.
    let open = Constants {
        disallow_unfiltered_html: false,
        ..Constants::default()
    };
    for (golden, constants) in [
        ("admin-posts-inline-save-html", Constants::default()),
        ("admin-posts-inline-save-html-unfiltered-html", open),
    ] {
        check(
            golden,
            async |_| Ok(()),
            async |conn, ()| {
                let more = "post_title=%3Cb%3EHi%3C%2Fb%3E+%3Cscript%3Ex()%3C%2Fscript%3E\
                            &post_name=hello-world&screen=edit-post&post_type=post";
                quick_edit(conn, &quick(1, more), constants).await
            },
        )
        .await;
    }
}

/// The database the answers are compared on: `wordpress_saves` loaded with
/// the fixture, `reference/action-checks/content.sql` and `sql`.
async fn copy(sql: &str) -> hyscribe_config::Database {
    use sqlx::{Connection as _, Executor as _};
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let content = include_str!("../../../../../../reference/action-checks/content.sql");
    let content = content.split("-- cleanup").next().unwrap();
    for statement in [content, sql].into_iter().filter(|sql| !sql.is_empty()) {
        let statement = sqlx::AssertSqlSafe(statement.to_string());
        conn.execute(sqlx::raw_sql(statement)).await.unwrap();
    }
    db
}

/// `text` with the ten characters after each `_wpnonce=` of `from` those
/// of `to`'s.
fn nonces_of(text: &str, to: &str) -> String {
    let at = |text: &str| text.find("_wpnonce=").map(|at| at + 9);
    match (at(text), at(to)) {
        (Some(from), Some(ours)) => format!(
            "{}{}{}",
            text.get(..from).unwrap(),
            to.get(ours..ours + 10).unwrap(),
            nonces_of(text.get(from + 10..).unwrap(), to.get(ours + 10..).unwrap())
        ),
        _ => text.to_string(),
    }
}

/// `text` with its nonces masked: PHP's `%nonce:<action>%` and any ten
/// hexadecimal digits standing alone.
fn unnonced(text: &str) -> String {
    let mut placed = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("%nonce:") {
        placed.push_str(rest.get(..at).unwrap());
        placed.push_str("%nonce%");
        let tail = rest.get(at + 1..).unwrap();
        rest = tail.get(tail.find('%').unwrap() + 1..).unwrap();
    }
    placed.push_str(rest);
    let mut out = String::new();
    let mut word = String::new();
    for c in placed.chars().chain(['\0']) {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        let nonce = word.len() == 10 && word.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'));
        out.push_str(if nonce { "%nonce%" } else { &word });
        word.clear();
        out.push(c);
    }
    out.pop();
    out
}

/// `page` with its last modified dates, which are the time of the action.
fn undated(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(at) = rest.find("Last Modified<br />") {
        let (head, tail) = rest.split_at(at + 19);
        out.push_str(head);
        rest = tail.get(tail.find("</td>").unwrap()..).unwrap();
    }
    out.push_str(rest);
    out
}

/// The posts lists' actions of `reference/admin-saves/golden.sh`, sent to
/// hs-admin on a copy of the fixture its writer writes, answer as PHP's,
/// and so do the screens their redirects lead to.
#[tokio::test(flavor = "multi_thread")]
async fn the_lists_actions_answer_as_php() {
    use crate::admin::tests::{location, logged_in, masked_nonces, page_of, send, session_nonce};
    let _saves = crate::admin::tests::SAVES.lock().await;
    use crate::admin::saves::tests::{AFTER, ANSWERS, CASES, case_row, hs};
    let dir = tempfile::tempdir().unwrap();
    let mut compared = 0;
    for line in CASES.lines().filter(|line| line.starts_with("posts-")) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [case, method, target, body, action, field, ..] = fields[..] else {
            panic!("{line}");
        };
        let config = copy(fields.get(7).copied().unwrap_or_default()).await;
        let (site, cookies) = logged_in("wordpress_saves", dir.path()).await;
        let writer = Writer::open(&config, Mode::Primary).await.unwrap();
        let site = Arc::new(Arc::try_unwrap(site).ok().unwrap().with_writer(writer));
        let nonce = session_nonce(&site, &cookies, action).await;
        let uri = format!("/hs-admin/{}", hs(target));
        let response = if method == "POST" {
            let form = format!("{body}&{field}={nonce}");
            let form = ("application/x-www-form-urlencoded", form.as_str());
            send(&site, "POST", &uri, &cookies, Some(form)).await
        } else {
            send(
                &site,
                "GET",
                &format!("{uri}&{field}={nonce}"),
                &cookies,
                None,
            )
            .await
        };
        let [_, status, php_location, php_body] = case_row(ANSWERS, case).unwrap()[..] else {
            panic!("{case}");
        };
        assert_eq!(response.status().as_str(), status, "{case}");
        let ours = response
            .headers()
            .get(axum::http::header::LOCATION)
            .map(|_| location(&response).to_string());
        let php_location = hs(&nonces_of(php_location, &format!("_wpnonce={nonce}")));
        assert_eq!(ours.as_deref().unwrap_or_default(), php_location, "{case}");
        let php = String::from_utf8(hyscribe_testkit::unhex(php_body)).unwrap();
        let page = page_of(response).await;
        // Quick Edit's row has the session's nonces, under hs-admin's names.
        let (page, php) = if php.contains("<tr") {
            (
                unnonced(&page),
                unnonced(&hyscribe_testkit::hs_admin_page(&php)),
            )
        } else {
            (page, php)
        };
        assert_eq!(page, php, "{case}");
        compared += 1;
        let Some(to) = ours else {
            continue;
        };
        let to = to.trim_start_matches(SITE);
        let [_, status, php_body] = case_row(AFTER, case).unwrap()[..] else {
            panic!("{case}");
        };
        // `send` serves `site` as it was loaded; a served site reads it
        // afresh for each request, as `current` does.
        let now = Arc::new(site.reloaded().await.unwrap());
        let response = send(&now, "GET", to, &cookies, None).await;
        assert_eq!(response.status().as_str(), status, "{case} {to}");
        if status == "200" {
            let php = String::from_utf8(hyscribe_testkit::unhex(php_body)).unwrap();
            let php = undated(&unnonced(&hyscribe_testkit::hs_admin_page(&php)));
            let page = undated(&unnonced(&masked_nonces(&page_of(response).await)));
            assert_eq!(page, php, "{case} {to}");
        }
    }
    assert_eq!(compared, 23);
}

/// What `screen`'s handler answers `user`'s `query` and `body` after `sql`,
/// and the values `read` selects after it, all rolled back.
async fn tried(
    user: u64,
    sql: &[String],
    (screen, query, body): (&str, &str, &str),
    read: &str,
) -> (Result<Answer, String>, Vec<String>) {
    use sqlx::{Acquire as _, Executor as _};
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
    for statement in sql {
        let statement = sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()));
        outer.execute(statement).await.unwrap();
    }
    let conn = outer.acquire().await.unwrap();
    let answer = answered(
        &writer().await,
        conn,
        (user, screen, Constants::default()),
        (query, body),
    )
    .await;
    let read = sqlx::query_scalar(sqlx::AssertSqlSafe(read.to_string()));
    (answer, read.fetch_all(&mut *conn).await.unwrap())
}

/// The user `id` with the serialized capabilities `caps`.
fn user(id: u64, caps: &str) -> [String; 2] {
    [
        format!(
            "INSERT INTO wp_users (ID, user_login, user_pass, user_nicename, user_email, \
             user_url, user_registered, user_activation_key, user_status, display_name) VALUES \
             ({id}, 'user{id}', '', 'user{id}', 'user{id}@example.test', '', \
             '2026-11-01 00:00:00', '', 0, 'user{id}')"
        ),
        format!(
            "INSERT INTO wp_usermeta (user_id, meta_key, meta_value) \
             VALUES ({id}, 'wp_capabilities', '{caps}')"
        ),
    ]
}

const AUTHOR: &str = "a:1:{s:6:\"author\";b:1;}";

/// The post `id` of `author`, `status` and `post_type`, titled `title`
/// with `content`, under `parent`.
fn post_row(
    id: u64,
    (author, status, post_type): (u64, &str, &str),
    (title, content): (&str, &str),
    parent: u64,
) -> String {
    format!(
        "INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, \
         post_title, post_excerpt, post_status, comment_status, ping_status, post_password, \
         post_name, to_ping, pinged, post_modified, post_modified_gmt, post_content_filtered, \
         post_parent, guid, menu_order, post_type, post_mime_type, comment_count) VALUES \
         ({id}, {author}, '2026-11-02 00:00:00', '2026-11-02 00:00:00', '{content}', '{title}', \
         '', '{status}', 'open', 'open', '', 'post-{id}', '', '', '2026-11-02 00:00:00', \
         '2026-11-02 00:00:00', '', {parent}, 'http://127.0.0.1:8081/?p={id}', 0, '{post_type}', \
         '', 0)"
    )
}

/// `post`'s meta `key` set to `value`.
fn meta(post: u64, key: &str, value: &str) -> String {
    format!(
        "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES ({post}, '{key}', '{value}')"
    )
}

fn died(message: &'static str) -> Result<Answer, String> {
    Ok(Answer::Died(message))
}

fn unported(answer: &Result<Answer, String>) -> bool {
    matches!(answer, Err(err) if err.starts_with("Unported"))
}

fn redirect(to: &str) -> Result<Answer, String> {
    Ok(Answer::Redirect(format!("{SITE}{to}")))
}

/// What `post.php` cannot go on with dies or is not ported, as in PHP.
#[tokio::test]
async fn post_php_stops_as_php_stops() {
    let _turn = turn().await;
    let trashed = ["UPDATE wp_posts SET post_status = 'trash' WHERE ID = 1".to_string()];
    let (answer, _) = tried(
        1,
        &trashed,
        ("post", "post=1&action=trash", ""),
        "SELECT post_status FROM wp_posts WHERE ID = 2",
    )
    .await;
    assert_eq!(answer, died("Error in moving the item to Trash."));
    let (answer, _) = tried(
        1,
        &[],
        ("post", "post=1&action=untrash", ""),
        "SELECT post_status FROM wp_posts WHERE ID = 2",
    )
    .await;
    assert_eq!(answer, died("Error in restoring the item from Trash."));
    let attachment = [post_row(
        950,
        (1, "inherit", "attachment"),
        ("A file", ""),
        0,
    )];
    let (answer, _) = tried(
        1,
        &attachment,
        ("post", "post=950&action=delete", ""),
        "SELECT post_status FROM wp_posts WHERE ID = 2",
    )
    .await;
    assert!(unported(&answer));
    let (answer, _) = tried(
        1,
        &[],
        ("post", "post=999&action=delete", ""),
        "SELECT post_status FROM wp_posts WHERE ID = 2",
    )
    .await;
    assert!(unported(&answer));
    // From the editor the list is the post type's; from a list, back to it.
    let read = "SELECT post_status FROM wp_posts WHERE ID = 2";
    for (referer, to) in [
        (
            "%2Fhs-admin%2Fpost-new",
            "/hs-admin/edit?post_type=post&trashed=1&ids=1",
        ),
        (
            "%2Fhs-admin%2Fpost%3Fpost%3D1",
            "/hs-admin/edit?post_type=post&trashed=1&ids=1",
        ),
        (
            "%2Fhs-admin%2Fedit%3Fids%3D9",
            "/hs-admin/edit?trashed=1&ids=1",
        ),
    ] {
        let query = format!("post=1&action=trash&_wp_http_referer={referer}");
        let (answer, _) = tried(1, &[], ("post", &query, ""), read).await;
        let to = if to.contains("post_type") {
            format!("{SITE}{to}")
        } else {
            to.into()
        };
        assert_eq!(answer, Ok(Answer::Redirect(to)), "{referer}");
    }
}

/// `edit.php`'s requests that write nothing, or stop part way.
#[tokio::test]
async fn bulk_actions_stop_as_php_stops() {
    let _turn = turn().await;
    let none = "SELECT post_status FROM wp_posts WHERE ID = 1";
    let edit = async |sql: &[String], query: &str| tried(1, sql, ("edit", query, ""), none).await;
    for query in [
        "post_type=attachment&action=trash&post[]=1",
        "action=trash&media[]=1",
        "action=trash&ids[]=1",
        "action=trash&post=1",
        "action=edit&bulk_edit=1&post[]=1&tags_input=a",
    ] {
        let (answer, _) = edit(&[], query).await;
        assert!(unported(&answer), "{query}");
    }
    let (answer, _) = edit(&[], "action=trash&p=5").await;
    assert_eq!(answer, redirect("/hs-admin/edit-comments?p=5"));
    let from_editor = "_wp_http_referer=%2Fhs-admin%2Fpost%3Fpost%3D1";
    let (answer, _) = edit(&[], &format!("action=trash&{from_editor}")).await;
    assert_eq!(answer, redirect("/hs-admin/post-new"));
    let (answer, _) = edit(
        &[],
        &format!("post_type=page&action=trash&post=&{from_editor}"),
    )
    .await;
    assert_eq!(answer, redirect("/hs-admin/post-new?post_type=page"));
    let (answer, _) = edit(&[], "delete_all=1&post_status=g_o-ne!").await;
    assert_eq!(
        answer,
        Ok(Answer::Redirect(
            "/hs-admin/edit?delete_all=1&post_status=g_o-ne%21&paged=1".into()
        ))
    );
    // Empty Trash's status is stripped to the characters a status has.
    let draft = [post_row(950, (1, "auto-draft", "post"), ("Draft", ""), 0)];
    let read = "SELECT post_status FROM wp_posts WHERE ID = 950";
    let query = "delete_all=1&post_status=auto-draft%21";
    let (_, statuses) = tried(1, &draft, ("edit", query, ""), read).await;
    assert_eq!(statuses, ["trash"]);
    let (answer, kept) = edit(&[], "action=edit&post[]=1").await;
    assert_eq!(
        answer,
        Ok(Answer::Redirect("/hs-admin/edit?paged=1".into()))
    );
    assert_eq!(kept, ["publish"]);
    for (query, message) in [
        (
            "action=trash&post[]=999",
            "Sorry, you are not allowed to move this item to the Trash.",
        ),
        (
            "action=untrash&post[]=999",
            "Sorry, you are not allowed to restore this item from the Trash.",
        ),
        (
            "action=delete&ids=1,x",
            "Sorry, you are not allowed to delete this item.",
        ),
        (
            "action=trash&post[]=1&post[]=1",
            "Error in moving the item to Trash.",
        ),
        (
            "action=untrash&post[]=1",
            "Error in restoring the item from Trash.",
        ),
    ] {
        let (answer, kept) = edit(&[], query).await;
        assert_eq!(answer, died(message), "{query}");
        assert_eq!(kept, ["publish"], "{query}");
    }
    // A revision is never deleted from the list.
    let revision = [post_row(
        951,
        (1, "inherit", "revision"),
        ("A revision", ""),
        1,
    )];
    let (answer, _) = edit(&revision, "action=delete&post[]=951").await;
    assert_eq!(
        answer,
        died("Sorry, you are not allowed to delete this item.")
    );
}

/// A post is locked by another user who exists and holds a fresh lock,
/// read from `_edit_last` when the lock does not name the user.
#[tokio::test]
async fn bulk_trash_skips_locked_posts() {
    let _turn = turn().await;
    let now = crate::now();
    let [author, caps] = user(900, AUTHOR);
    let sql = [
        author,
        caps,
        post_row(950, (1, "publish", "post"), ("Locked", "Text."), 0),
        meta(1, "_edit_lock", &now.to_string()),
        meta(1, "_edit_last", "900"),
        meta(2, "_edit_lock", &format!("{}:900", now - 150)),
        meta(3, "_edit_lock", &format!("{now}:1")),
        meta(950, "_edit_lock", &format!("{now}:12345")),
    ];
    let query = "action=trash&post[]=1&post[]=2&post[]=3&post[]=950";
    let read = "SELECT post_status FROM wp_posts WHERE ID IN (1, 2, 3, 950) ORDER BY ID";
    let (answer, statuses) = tried(1, &sql, ("edit", query, ""), read).await;
    let to = "/hs-admin/edit?paged=1&trashed=3&ids=1%2C2%2C3%2C950&locked=1";
    assert_eq!(answer, Ok(Answer::Redirect(to.into())));
    assert_eq!(statuses, ["publish", "trash", "trash", "trash"]);
}

/// Quick Edit of what the golden cases leave out: refused fields, another
/// list, a private post, a draft's slug, another user's limits.
#[tokio::test]
async fn quick_edit_edges() {
    let _turn = turn().await;
    let status = "SELECT CONCAT(post_status, ' ', post_name, ' ', post_password) FROM wp_posts \
                  WHERE ID IN (1, 3, 950) ORDER BY ID";
    let quick_edit = async |user: u64, sql: &[String], body: &str| {
        tried(user, sql, ("admin-ajax", "", body), status).await
    };
    let post = |more: &str| quick(1, &format!("screen=edit-post&post_type=post&{more}"));
    for more in [
        "tags_input=a",
        "screen=edit-attachment",
        "post_ID=999",
        "tax_input[post_format]=aside&tax_input[link_category]=x",
        "tax_input[category]=1",
        "tax_input[post_tag][]=a",
        "post_format=aside",
        "post_title[]=x",
        "aa=x",
        "aa[]=1",
        "_status[]=publish",
    ] {
        let (answer, _) = quick_edit(1, &[], &post(more)).await;
        assert!(unported(&answer), "{more}");
    }
    let (answer, kept) = quick_edit(1, &[], &post("mm=13")).await;
    assert_eq!(answer, Ok(Answer::Page("Invalid date.".into())));
    assert_eq!(kept[0], "publish hello-world ");
    let private = "keep_private=private&sticky=sticky&post_password=x&_status=inherit";
    let (answer, statuses) = quick_edit(1, &[], &post(private)).await;
    assert_eq!(answer, Ok(Answer::Page(String::new())));
    assert_eq!(statuses[0], "private hello-world ");
    let (answer, statuses) =
        quick_edit(1, &[], &post("_status=inherit&tax_input[category][]=1")).await;
    assert_eq!(answer, Ok(Answer::Page(String::new())));
    assert_eq!(statuses[0], "publish hello-world ");
    // A draft's slug is made unique as a published one's.
    let draft = quick(
        3,
        "screen=edit-page&post_type=page&post_name=sample-page&_status=draft",
    );
    let (_, statuses) = quick_edit(1, &[], &draft).await;
    assert_eq!(statuses[1], "draft sample-page-2 ");
    // Revisions from before 3.6, and a post format the post has.
    let revision = [
        post_row(951, (1, "inherit", "revision"), ("A revision", ""), 1),
        "UPDATE wp_posts SET post_name = '1-autosave-v1' WHERE ID = 951".into(),
    ];
    let (answer, _) = quick_edit(1, &revision, &post("")).await;
    assert_eq!(answer, Ok(Answer::Page(String::new())));
    let body = quick(951, "screen=edit-post&post_type=post");
    let (answer, _) = quick_edit(1, &revision, &body).await;
    assert!(unported(&answer));
    let old = ["UPDATE wp_posts SET post_name = '1-revision-v0' WHERE ID = 951".to_string()];
    let (answer, _) = quick_edit(1, &[revision[0].clone(), old[0].clone()], &post("")).await;
    assert!(unported(&answer));
    let formatted = [
        "INSERT INTO wp_terms (term_id, name, slug) VALUES (960, 'Aside', 'post-format-aside')"
            .to_string(),
        "INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description) \
         VALUES (960, 960, 'post_format', '')"
            .into(),
        "INSERT INTO wp_term_relationships (object_id, term_taxonomy_id) VALUES (1, 960)".into(),
    ];
    let (answer, _) = quick_edit(1, &formatted, &post("")).await;
    assert!(unported(&answer));
    // A post left with no title, content or excerpt keeps its title.
    let blank = [post_row(950, (1, "publish", "post"), ("Titled", ""), 0)];
    let body = quick(950, "screen=edit-post&post_type=post&post_title=");
    let (answer, _) = quick_edit(1, &blank, &body).await;
    assert_eq!(answer, Ok(Answer::Page(String::new())));
    // An author may not give a post to another user.
    let [author, caps] = user(900, AUTHOR);
    let (answer, _) = quick_edit(900, &[author.clone(), caps.clone()], &post("")).await;
    assert_eq!(
        answer,
        Ok(Answer::Page(
            "Sorry, you are not allowed to edit posts as this user.".into()
        ))
    );
    let page = quick(2, "screen=edit-page&post_type=page");
    let (answer, _) = quick_edit(900, &[author, caps], &page).await;
    assert_eq!(
        answer,
        Ok(Answer::Page(
            "Sorry, you are not allowed to edit pages as this user.".into()
        ))
    );
    // Who edits others' posts, but publishes none and assigns no terms.
    let [editor, caps] = user(903, "a:1:{s:17:\"edit_others_posts\";b:1;}");
    let draft = post_row(950, (1, "draft", "post"), ("Draft", "Text."), 0);
    let sql = [editor, caps, draft];
    let more = "screen=edit-post&post_type=post&_status=publish&post_password=x\
                &post_category[]=0&tax_input[post_tag]=Unassigned&sticky=sticky";
    let (answer, statuses) = quick_edit(903, &sql, &quick(950, more)).await;
    assert_eq!(answer, Ok(Answer::Page(String::new())));
    assert_eq!(statuses[2], "pending  ");
    let sticky = "SELECT option_value FROM wp_options WHERE option_name = 'sticky_posts'";
    let body = quick(950, more);
    let (_, stickies) = tried(903, &sql, ("admin-ajax", "", &body), sticky).await;
    assert_eq!(stickies, ["a:0:{}"]);
    let more = "screen=edit-post&post_type=post&keep_private=private";
    let (_, statuses) = quick_edit(903, &sql, &quick(950, more)).await;
    assert_eq!(statuses[2], "draft post-950 ");
    // The date's parts as PHP reads them: an hour past 23, a day past 31.
    let dated = "SELECT CAST(post_date AS CHAR) FROM wp_posts WHERE ID = 1";
    let (_, dates) = tried(1, &[], ("admin-ajax", "", &post("hh=25&jj=40")), dated).await;
    assert_eq!(dates, ["2026-10-31 01:08:15"]);
    let undated = quick(1, "screen=edit-post&post_type=post").replace("&aa=2026", "");
    let (_, dates) = tried(1, &[], ("admin-ajax", "", &undated), dated).await;
    let year = format_unix("Y", crate::now(), Zone::Utc);
    assert_eq!(dates, [format!("{year}-10-27 08:08:15")]);
    // A month of 0 is this month's; the last hour, minute and second stay.
    let (_, dates) = tried(
        1,
        &[],
        ("admin-ajax", "", &post("mm=0&hh=23&mn=59&ss=59")),
        dated,
    )
    .await;
    let month = format_unix("m", crate::now(), Zone::Utc);
    assert_eq!(dates, [format!("2026-{month}-27 23:59:59")]);
}

/// Bulk Edit keeps a post's tags by name, as `bulk_edit_posts()` reads
/// them, so none is added named by its ID (found by the switch-back suite).
#[tokio::test]
async fn bulk_edit_keeps_tags() {
    let _turn = turn().await;
    let tagged = [
        "INSERT INTO wp_terms (term_id, name, slug) VALUES (962, 'Kept', 'kept')".to_string(),
        "INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, count) \
         VALUES (962, 962, 'post_tag', '', 1)"
            .into(),
        "INSERT INTO wp_term_relationships (object_id, term_taxonomy_id) VALUES (1, 962)".into(),
    ];
    let names = "SELECT t.name FROM wp_terms AS t \
                 INNER JOIN wp_term_taxonomy AS tt ON tt.term_id = t.term_id \
                 INNER JOIN wp_term_relationships AS tr ON tr.term_taxonomy_id = tt.term_taxonomy_id \
                 WHERE tr.object_id = 1 AND tt.taxonomy = 'post_tag' ORDER BY t.name";
    let query = "action=edit&bulk_edit=Update&post_type=post&post[]=1&tax_input[post_tag]=Added";
    let (_, tags) = tried(1, &tagged, ("edit", query, ""), names).await;
    assert_eq!(tags, ["Added", "Kept"]);
}

/// Bulk Edit of what the golden cases leave out.
#[tokio::test]
async fn bulk_edit_edges() {
    let _turn = turn().await;
    let base = "action=edit&bulk_edit=Update&post_type=post&post_author=-1\
                &comment_status=&ping_status=&sticky=-1&post_format=-1";
    let read = "SELECT CONCAT(ID, ' ', post_status, ' ', post_parent, ' ', comment_status) \
                FROM wp_posts WHERE ID IN (1, 2, 3, 950) ORDER BY ID";
    let bulk_edit = async |user: u64, sql: &[String], more: &str| {
        tried(user, sql, ("edit", &format!("{base}&{more}"), ""), read).await
    };
    let counts = |answer: &Result<Answer, String>, (updated, skipped): (usize, usize)| {
        let counted = format!("&updated={updated}&skipped={skipped}&locked=0");
        matches!(answer, Ok(Answer::Redirect(to)) if to.ends_with(&counted))
    };
    for more in [
        "post[]=1&tax_input[category]=1",
        "post[]=1&tax_input[nav_menu]=x",
        "post[]=1&post_format=aside",
        "post[]=1&aa=x&edit_date=1",
    ] {
        let (answer, _) = bulk_edit(1, &[], more).await;
        assert!(unported(&answer), "{more}");
    }
    let (answer, rows) = bulk_edit(
        1,
        &[],
        "post[]=1&post_category=5&tax_input[post_format]=x&tax_input[post_tag]=",
    )
    .await;
    assert!(counts(&answer, (1, 0)));
    assert_eq!(rows[0], "1 publish 0 open");
    let (_, rows) = bulk_edit(1, &[], "post[]=1&post[]=999&_status=inherit").await;
    assert_eq!(rows[0], "1 publish 0 open");
    let (answer, _) = bulk_edit(1, &[], "post[]=999&_status=draft").await;
    assert!(counts(&answer, (0, 1)));
    // A category added through `tax_input`, and one kept as indeterminate.
    let category = [
        "INSERT INTO wp_terms (term_id, name, slug) VALUES (961, 'Second', 'second')".to_string(),
        "INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description) \
         VALUES (961, 961, 'category', '')"
            .into(),
    ];
    let categories = "SELECT CAST(term_taxonomy_id AS CHAR) FROM wp_term_relationships WHERE object_id = 1 ORDER BY term_taxonomy_id";
    for more in [
        "post[]=1&tax_input[category][]=961",
        "post[]=1&post_category[]=0&post_category[]=961&indeterminate_post_category[]=1",
    ] {
        let query = format!("{base}&{more}");
        let (_, terms) = tried(1, &category, ("edit", &query, ""), categories).await;
        assert_eq!(terms, ["1", "961"], "{more}");
    }
    // A post another user is editing is counted as locked.
    let [editing, caps] = user(900, AUTHOR);
    let lock = meta(1, "_edit_lock", &format!("{}:900", crate::now()));
    let (answer, _) = bulk_edit(1, &[editing, caps, lock], "post[]=1").await;
    assert!(
        matches!(answer, Ok(Answer::Redirect(to)) if to.ends_with("&updated=0&skipped=0&locked=1"))
    );
    // A date part that differs from its hidden copy edits the date.
    let dated = "SELECT CAST(post_date AS CHAR) FROM wp_posts WHERE ID = 1";
    let query = format!("{base}&post[]=1&hidden_jj=27&jj=28&aa=2026&mm=10&hh=08&mn=08&ss=15");
    let (_, dates) = tried(1, &[], ("edit", &query, ""), dated).await;
    assert_eq!(dates, ["2026-10-28 08:08:15"]);
    // Pages are not put under themselves or their children.
    let pages = "action=edit&bulk_edit=Update&post_type=page&post_parent=2&post[]=2&post[]=3";
    let (answer, rows) = tried(1, &[], ("edit", pages, ""), read).await;
    let skipped =
        "/hs-admin/edit?post_type=page&post_parent=2&paged=1&updated=1&skipped=1&locked=0";
    assert_eq!(answer, Ok(Answer::Redirect(skipped.into())));
    assert_eq!(rows[2], "3 draft 2 closed");
    // Parents in a loop end the walk up.
    let looped = [
        post_row(950, (1, "publish", "page"), ("A", ""), 951),
        post_row(951, (1, "publish", "page"), ("B", ""), 950),
    ];
    let pages = "action=edit&bulk_edit=Update&post_type=page&post_parent=950&post[]=951";
    let (answer, _) = tried(1, &looped, ("edit", pages, ""), read).await;
    let skipped =
        "/hs-admin/edit?post_type=page&post_parent=950&paged=1&updated=0&skipped=1&locked=0";
    assert_eq!(answer, Ok(Answer::Redirect(skipped.into())));
    // Only pages are walked up: a post's own parent is not.
    let under = [post_row(950, (1, "publish", "post"), ("Post", "Text."), 2)];
    let pages = "action=edit&bulk_edit=Update&post_type=page&post_parent=950&post[]=2";
    let (answer, _) = tried(1, &under, ("edit", pages, ""), read).await;
    let updated =
        "/hs-admin/edit?post_type=page&post_parent=950&paged=1&updated=1&skipped=0&locked=0";
    assert_eq!(answer, Ok(Answer::Redirect(updated.into())));
    let pages = "action=edit&bulk_edit=Update&post_type=page&post_parent=1&post[]=3";
    let (_, rows) = tried(1, &[], ("edit", pages, ""), read).await;
    assert_eq!(rows[2], "3 draft 1 closed");
    // An author's own post given to another user is skipped; a draft
    // published gets the time of the action.
    let [author, caps] = user(900, AUTHOR);
    let own = post_row(950, (900, "draft", "post"), ("Own", "Text."), 0);
    let sql = [author, caps, own];
    let (answer, _) = bulk_edit(900, &sql, "post[]=950&post_author=1&_status=publish").await;
    assert!(counts(&answer, (0, 1)));
    let dated = "SELECT CAST(post_date AS CHAR) FROM wp_posts WHERE ID = 950";
    let query = format!("{base}&post[]=950&_status=publish");
    let (_, dates) = tried(1, &sql, ("edit", &query, ""), dated).await;
    assert_ne!(dates, ["2026-11-02 00:00:00"]);
    // A post with nothing in it is not saved by `wp_update_post()`.
    let empty = [post_row(950, (1, "publish", "post"), ("", ""), 0)];
    let (answer, _) = bulk_edit(1, &empty, "post[]=950").await;
    assert!(unported(&answer));
    let revision = [post_row(
        951,
        (1, "inherit", "revision"),
        ("A revision", ""),
        1,
    )];
    let (answer, _) = bulk_edit(1, &revision, "post[]=951").await;
    assert!(unported(&answer));
    // Sticky posts: unstuck, and left as they are for an author; an
    // auto-draft becomes a draft.
    let stuck =
        "UPDATE wp_options SET option_value = 'a:1:{i:0;i:1;}' WHERE option_name = 'sticky_posts'";
    let sticky = "SELECT option_value FROM wp_options WHERE option_name = 'sticky_posts'";
    let query = format!("{base}&post[]=1&sticky=unsticky");
    let (_, stickies) = tried(1, &[stuck.into()], ("edit", &query, ""), sticky).await;
    assert_eq!(stickies, ["a:0:{}"]);
    let query = format!("{base}&post[]=1");
    let (_, stickies) = tried(1, &[stuck.into()], ("edit", &query, ""), sticky).await;
    assert_eq!(stickies, ["a:1:{i:0;i:1;}"]);
    let mut sql = sql.to_vec();
    sql.push(stuck.into());
    sql[2] = post_row(950, (900, "auto-draft", "post"), ("Own", "Text."), 0);
    let query = format!("{base}&post[]=950&sticky=unsticky");
    let (_, stickies) = tried(900, &sql, ("edit", &query, ""), sticky).await;
    assert_eq!(stickies, ["a:1:{i:0;i:1;}"]);
    let (_, rows) = tried(900, &sql, ("edit", &query, ""), read).await;
    assert_eq!(rows[3], "950 draft 0 open");
}

/// An administrator posts unfiltered HTML only on a site whose constants
/// allow it; a visitor never does.
#[tokio::test]
async fn unfiltered_html_follows_the_constants() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let open = Constants {
        disallow_unfiltered_html: false,
        ..Constants::default()
    };
    let can = [(1, Constants::default()), (1, open), (0, open)];
    let mut found = Vec::new();
    for (user, constants) in can {
        found.push(unfiltered_html(&mut write, user, constants).await.unwrap());
    }
    assert_eq!(found, [false, true, false]);
}

/// What Quick Edit and Bulk Edit read when a field is left out or odd.
#[tokio::test]
async fn edits_of_odd_fields() {
    let _turn = turn().await;
    let read = "SELECT CONCAT(post_status, ' ', post_author, ' ', post_password) FROM wp_posts \
                WHERE ID = 1";
    let quick_edit = async |body: String| tried(1, &[], ("admin-ajax", "", &body), read).await;
    let post = |more: &str| quick(1, &format!("screen=edit-post&post_type=post&{more}"));
    for (more, status) in [
        ("_status=auto-draft", "draft 1 "),
        ("_status=bogus", "publish 1 "),
        ("post_password=secret", "publish 1 secret"),
        ("post_format=standard", "publish 1 "),
    ] {
        let (answer, statuses) = quick_edit(post(more)).await;
        assert_eq!(answer, Ok(Answer::Page(String::new())), "{more}");
        assert_eq!(statuses, [status], "{more}");
    }
    // No author and only the list's empty status: the user, and a draft,
    // as `wp_insert_post()` makes of an empty status.
    let bare = post("")
        .replace("&post_author=1", "")
        .replace("&_status=publish", "");
    let (answer, statuses) = quick_edit(bare).await;
    assert_eq!(answer, Ok(Answer::Page(String::new())));
    assert_eq!(statuses, ["draft 1 "]);
    // Bulk Edit sent as a form, a category given twice.
    let categories = "SELECT CAST(term_taxonomy_id AS CHAR) FROM wp_term_relationships \
                      WHERE object_id = 1 ORDER BY term_taxonomy_id";
    let body = "post[]=1&post_category[]=1&post_category[]=1";
    let query = "action=edit&bulk_edit=Update&post_type=post";
    let (answer, terms) = tried(1, &[], ("edit", query, body), categories).await;
    assert!(
        matches!(answer, Ok(Answer::Redirect(to)) if to.ends_with("&updated=1&skipped=0&locked=0"))
    );
    assert_eq!(terms, ["1"]);
}

/// The capabilities read in the action: of a page's type, of a user who
/// must exist, and of a trashed post by the status it was trashed from.
#[tokio::test]
async fn capabilities_are_read_in_the_action() {
    let _turn = turn().await;
    // Who edits pages and publishes posts, but not pages.
    let caps = "a:5:{s:10:\"edit_pages\";b:1;s:17:\"edit_others_pages\";b:1;\
                s:20:\"edit_published_pages\";b:1;s:13:\"publish_posts\";b:1;s:10:\"edit_posts\";b:1;}";
    let [editor, meta_caps] = user(905, caps);
    let draft = quick(3, "screen=edit-page&post_type=page&_status=publish");
    let read = "SELECT post_status FROM wp_posts WHERE ID = 3";
    let (_, statuses) = tried(905, &[editor, meta_caps], ("admin-ajax", "", &draft), read).await;
    assert_eq!(statuses, ["pending"]);
    // An administrator's capabilities without the user are none.
    let [_, orphan] = user(906, "a:1:{s:13:\"administrator\";b:1;}");
    let (answer, _) = tried(906, &[orphan], ("edit", "action=trash&post[]=1", ""), read).await;
    assert_eq!(
        answer,
        died("Sorry, you are not allowed to move this item to the Trash.")
    );
    // A contributor's own post trashed once published needs more to delete.
    let [contributor, meta_caps] = user(907, "a:1:{s:11:\"contributor\";b:1;}");
    let trashed = post_row(950, (907, "trash", "post"), ("Gone", "Text."), 0);
    let sql = [
        contributor,
        meta_caps,
        trashed,
        meta(950, "_wp_trash_meta_status", "publish"),
    ];
    let (answer, _) = tried(907, &sql, ("edit", "action=delete&post[]=950", ""), read).await;
    assert_eq!(
        answer,
        died("Sorry, you are not allowed to delete this item.")
    );
}
