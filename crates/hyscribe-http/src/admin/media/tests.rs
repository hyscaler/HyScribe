// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media library's Ajax answers of `reference/media-ajax/golden.sh`,
//! sent to hs-admin over `reference/attachment-edit/content.sql` on
//! `wordpress_saves`.

use std::sync::Arc;

use axum::response::Response;
use hyscribe_config::Mode;
use hyscribe_db::Writer;
use hyscribe_phpser::{Key, Value, json_encode};
use sqlx::{Connection as _, Executor as _};

use crate::Site;
use crate::admin::tests::{SAVES, logged_in, page_of, send, session_nonce, with_values};

pub(crate) const GOLDEN: &str = include_str!("../../../tests/golden/media-ajax.tsv");

/// A row of the golden: who sent what, and PHP's answer.
pub(crate) struct Case<'a> {
    pub(crate) role: &'a str,
    pub(crate) flags: &'a str,
    pub(crate) body: &'a str,
    pub(crate) status: u16,
    pub(crate) headers: &'a str,
    pub(crate) answer: String,
}

/// The golden's rows, their answers under hs-admin's URLs.
pub(crate) fn cases() -> Vec<Case<'static>> {
    GOLDEN
        .lines()
        .map(|line| {
            let [role, flags, body, status, headers, answer] =
                line.splitn(6, '\t').collect::<Vec<_>>()[..]
            else {
                panic!("{line}");
            };
            Case {
                role,
                flags,
                body,
                status: status.parse().unwrap(),
                headers,
                answer: hyscribe_testkit::hs_admin_page(answer),
            }
        })
        .collect()
}

/// A connection to `db`.
pub(crate) async fn connect(db: &hyscribe_config::Database) -> sqlx::MySqlConnection {
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    sqlx::MySqlConnection::connect_with(&options).await.unwrap()
}

/// `wordpress_saves` afresh with the fixture and the media's content.
pub(crate) async fn loaded() -> hyscribe_config::Database {
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let content = include_str!("../../../../../reference/attachment-edit/content.sql");
    let mut conn = connect(&db).await;
    conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(content.to_string())))
        .await
        .unwrap();
    db
}

/// User 1 in `role`, as the oracle's must-use plugin makes them.
pub(crate) async fn as_role(db: &hyscribe_config::Database, role: &str) {
    let caps = format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len());
    let mut conn = connect(db).await;
    sqlx::query(
        "UPDATE wp_usermeta SET meta_value = ? WHERE user_id = 1 AND meta_key = 'wp_capabilities'",
    )
    .bind(caps)
    .execute(&mut conn)
    .await
    .unwrap();
}

/// The site on `db`, writing, with `MEDIA_TRASH` as `trash`, and its login
/// cookies.
pub(crate) async fn site(
    db: &hyscribe_config::Database,
    dir: &std::path::Path,
    trash: bool,
) -> (Arc<Site>, String) {
    let (site, cookies) = logged_in("wordpress_saves", dir).await;
    let writer = Writer::open(db, Mode::Primary).await.unwrap();
    let constants = hyscribe_config::Constants {
        media_trash: trash,
        ..hyscribe_config::Constants::default()
    };
    let uploads = crate::uploads::Uploads::new(
        dir.join("uploads"),
        1 << 20,
        hyscribe_config::ImageLimits::default(),
    );
    let site = Arc::try_unwrap(site).ok().unwrap().with_writer(writer);
    let site = site.with_constants(constants).with_uploads(uploads);
    (Arc::new(site), cookies)
}

/// `body` sent to `admin-ajax`, its nonces the session's.
pub(crate) async fn sent(site: &Arc<Site>, cookies: &str, body: &str) -> Response {
    let mut body = body.to_string();
    while let Some((head, tail)) = body.split_once("%nonce:") {
        let (action, rest) = tail.split_once('%').unwrap();
        let nonce = session_nonce(site, cookies, action).await;
        body = format!("{head}{nonce}{rest}");
    }
    let form = ("application/x-www-form-urlencoded", body.as_str());
    send(site, "POST", "/hs-admin/admin-ajax", cookies, Some(form)).await
}

/// The answer key of a query's order, which MariaDB leaves ties of in no
/// set order.
fn order_key(what: &str) -> &'static str {
    let orders = [
        ("orderby]=title", "title"),
        ("orderby]=name", "name"),
        ("orderby]=modified", "modified"),
        ("orderby]=parent", "uploadedTo"),
        ("orderby]=author", "author"),
        ("orderby]=menu_order", "menuOrder"),
        ("orderby]=ID", "id"),
        ("orderby]=post__in", "id"),
    ];
    let found = orders.iter().find(|(asked, _)| what.contains(asked));
    found.map_or("date", |(_, key)| key)
}

/// `ours` answers as `php` does: a list in the same order of its sort key,
/// each attachment the same, its nonces the session's.
fn settled(php: &str, ours: &str, what: &str) {
    let decoded = |json: &str| hyscribe_phpser::json_decode(json.as_bytes());
    let list = |json: Option<Value>| {
        let data = json?.get_str("data")?.clone();
        data.as_array().map(<[_]>::to_vec)
    };
    let (Some(theirs), Some(mine)) = (list(decoded(php)), list(decoded(ours))) else {
        assert_eq!(with_values(php, ours), ours, "{what}");
        return;
    };
    let key = order_key(what);
    let keys = |list: &[(Key, Value)]| -> Vec<Vec<u8>> {
        let key = |entry: &Value| entry.get_str(key).map(Value::to_php_string);
        list.iter()
            .map(|(_, entry)| key(entry).unwrap_or_default())
            .collect()
    };
    assert_eq!(keys(&mine), keys(&theirs), "{what}");
    let by_id = |list: &[(Key, Value)]| {
        let mut list: Vec<_> = list.iter().map(|(_, entry)| entry.clone()).collect();
        list.sort_by_key(|entry| entry.get_str("id").map_or(0, Value::to_int));
        list
    };
    for (theirs, mine) in by_id(&theirs).iter().zip(by_id(&mine)) {
        let encode = |entry: &Value| String::from_utf8(json_encode(entry).unwrap()).unwrap();
        let (theirs, mine) = (encode(theirs), encode(&mine));
        assert_eq!(with_values(&theirs, &mine), mine, "{what}");
    }
}

/// `"modified":<ms>` of an answer written now, as the golden masks it.
fn now_masked(answer: &str) -> String {
    match answer.split_once("\"modified\":") {
        Some((head, tail)) => {
            let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
            let rest = tail.get(digits..).unwrap_or_default();
            format!("{head}\"modified\":%now%{rest}")
        }
        None => answer.to_string(),
    }
}

/// Each role's saves and deletes, refused or written, answer as PHP's,
/// each write over the content afresh; a deleted image's files go with
/// it, and only they.
#[tokio::test(flavor = "multi_thread")]
async fn media_writes_match_php() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("uploads/2026/05");
    std::fs::create_dir_all(&folder).unwrap();
    let names = ["big-scaled.jpg", "big-150x150.jpg", "big.jpg", "binned.png"];
    for name in names {
        std::fs::write(folder.join(name), b"x").unwrap();
    }
    let writes = cases();
    let writes = writes.iter().filter(|case| {
        !["action=query-attachments", "action=get-attachment"]
            .iter()
            .any(|action| case.body.starts_with(action))
    });
    let mut compared = 0;
    let mut db = loaded().await;
    for case in writes {
        if case.flags.contains("restore") {
            db = loaded().await;
        }
        as_role(&db, case.role).await;
        let (site, cookies) = site(&db, dir.path(), case.flags.contains("trash")).await;
        let response = sent(&site, &cookies, case.body).await;
        let what = format!("{} {}", case.role, case.body);
        assert_eq!(response.status().as_u16(), case.status, "{what}");
        let ours = now_masked(&page_of(response).await);
        settled(&case.answer, &ours, &what);
        compared += 1;
    }
    assert_eq!(compared, 41);
    let kept: Vec<_> = names
        .iter()
        .filter(|name| folder.join(name).exists())
        .collect();
    assert_eq!(kept, [&"binned.png"]);
}

/// The site icon's and logo's states, a size read from the file where the
/// metadata has none, and what is not ported: a file by its absolute path,
/// a parent edited on a screen other than `post.php`.
#[tokio::test(flavor = "multi_thread")]
async fn states_and_file_sizes() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = loaded().await;
    let mut conn = connect(&db).await;
    for sql in [
        "UPDATE wp_options SET option_value = '801' WHERE option_name = 'site_icon'",
        "INSERT INTO wp_options (option_name, option_value) VALUES \
         ('theme_mods_twentytwentyfive', 'a:1:{s:11:\"custom_logo\";i:807;}') \
         ON DUPLICATE KEY UPDATE option_value = VALUES(option_value)",
        r#"UPDATE wp_postmeta SET meta_value = 'a:1:{s:8:"filesize";i:0;}' WHERE meta_id = 805"#,
        "UPDATE wp_postmeta SET meta_value = 'a:0:{}' WHERE meta_id = 807",
        "UPDATE wp_postmeta SET meta_value = '/srv/notes' WHERE meta_id = 806",
        "UPDATE wp_posts SET post_type = 'wp_template' WHERE ID = 806",
        // The logo has no file, so no size; a song's featured image is gone.
        "UPDATE wp_postmeta SET meta_value = '' WHERE meta_id = 810",
        "UPDATE wp_postmeta SET meta_value = 'a:0:{}' WHERE meta_id = 811",
        "UPDATE wp_postmeta SET meta_value = '-5' WHERE post_id = 817 AND meta_key = '_thumbnail_id'",
        // Full sizes: whole, without a height, a width, or a file.
        r#"UPDATE wp_postmeta SET meta_value = 'a:2:{s:9:"mime_type";s:9:"video/mp4";s:5:"sizes";a:1:{s:4:"full";a:3:{s:4:"file";s:5:"f.png";s:5:"width";i:10;s:6:"height";i:20;}}}' WHERE post_id = 818 AND meta_key = '_wp_attachment_metadata'"#,
        r#"UPDATE wp_postmeta SET meta_value = 'a:1:{s:5:"sizes";a:1:{s:4:"full";a:2:{s:4:"file";s:5:"g.png";s:5:"width";i:5;}}}' WHERE post_id = 819 AND meta_key = '_wp_attachment_metadata'"#,
        r#"UPDATE wp_postmeta SET meta_value = 'a:1:{s:5:"sizes";a:1:{s:4:"full";a:1:{s:4:"file";s:5:"h.png";}}}' WHERE post_id = 817 AND meta_key = '_wp_attachment_metadata'"#,
        r#"UPDATE wp_postmeta SET meta_value = 'a:1:{s:5:"sizes";a:1:{s:4:"full";a:3:{s:4:"file";s:0:"";s:5:"width";i:1;s:6:"height";i:2;}}}' WHERE post_id = 820 AND meta_key = '_wp_attachment_metadata'"#,
    ] {
        conn.execute(sql).await.unwrap();
    }
    let folder = dir.path().join("uploads/2025/12");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("report.pdf"), b"pdf").unwrap();
    let (site, cookies) = site(&db, dir.path(), false).await;
    let answer = async |id: u64| {
        let response = sent(&site, &cookies, &format!("action=get-attachment&id={id}")).await;
        (response.status().as_u16(), page_of(response).await)
    };
    let (_, icon) = answer(801).await;
    assert!(icon.ends_with("\"mediaStates\":\"Site Icon\"}}"), "{icon}");
    let (_, logo) = answer(807).await;
    assert!(logo.ends_with("\"mediaStates\":\"Logo\"}}"), "{logo}");
    let (_, report) = answer(802).await;
    assert!(report.contains("\"filesizeInBytes\":3,"), "{report}");
    assert_eq!(answer(803).await.0, 501);
    assert_eq!(answer(805).await.0, 501);
    assert!(!logo.contains("filesizeInBytes"), "{logo}");
    let (_, song) = answer(817).await;
    assert!(!song.contains("\"thumb\""), "{song}");
    let (_, video) = answer(818).await;
    let full = "\\/2026\\/06\\/f.png\",\"height\":20,\"width\":10,\"orientation\":\"portrait\"}}";
    assert!(video.contains(full), "{video}");
    for id in [817, 819, 820] {
        let (_, bare) = answer(id).await;
        assert!(bare.contains("\"sizes\":[]"), "{bare}");
    }
    // What cannot be shown is not ported, in a query and a save too, as is
    // a query PHP's own arguments make of it.
    let query = "action=query-attachments&query[post__in][]=803&query[posts_per_page]=5";
    assert_eq!(sent(&site, &cookies, query).await.status().as_u16(), 501);
    let odd = "action=query-attachments&query[author][]=1";
    assert_eq!(sent(&site, &cookies, odd).await.status().as_u16(), 501);
    let nonce = "nonce=%nonce:update-post_803%";
    let save = format!("action=save-attachment-compat&id=803&{nonce}&attachments[803][a]=b");
    assert_eq!(sent(&site, &cookies, &save).await.status().as_u16(), 501);
    let scalar = format!("action=save-attachment-compat&id=803&{nonce}&attachments=x");
    assert_eq!(sent(&site, &cookies, &scalar).await.status().as_u16(), 501);
    // Without an uploads folder a file's size is not known.
    let (bare, bare_cookies) = logged_in("wordpress_saves", dir.path()).await;
    let read = sent(&bare, &bare_cookies, "action=get-attachment&id=802").await;
    assert_eq!(read.status().as_u16(), 501);
}

/// `upload.php`'s bulk actions in list mode: checked as PHP checks them,
/// refused before the first write, written, or not ported.
#[tokio::test(flavor = "multi_thread")]
async fn bulk_actions_are_checked() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = loaded().await;
    let mut conn = connect(&db).await;
    let mode = "INSERT INTO wp_usermeta (user_id, meta_key, meta_value) \
                VALUES (1, 'wp_media_library_mode', 'list')";
    conn.execute(mode).await.unwrap();
    let (first, cookies) = site(&db, dir.path(), false).await;
    let nonce = session_nonce(&first, &cookies, "bulk-media").await;
    let asked = async |query: &str| {
        let uri = format!("/hs-admin/upload?{query}&_wpnonce={nonce}");
        send(&first, "GET", &uri, &cookies, None).await
    };
    let status = async |query: &str| asked(query).await.status().as_u16();
    assert_eq!(status("action=trash&media[]=810").await, 500);
    assert_eq!(status("action=untrash&media[]=801").await, 500);
    assert_eq!(status("action=delete&media[]=1").await, 500);
    assert_eq!(status("action=delete&ids=1,2").await, 500);
    // Attaching to the parent a file has, and detaching what is not there, change nothing.
    let attach = "action=attach&found_post_id=1&media[]=801";
    assert_eq!(status(attach).await, 302);
    let detach = "action=detach&parent_post_id=1&media[]=99999";
    assert_eq!(status(detach).await, 302);
    assert_eq!(status("mode=list&action=delete&media[]=801").await, 501);
    assert_eq!(status("action=bogus&media[]=801").await, 302);
    assert_eq!(status("action=delete&media=").await, 500);
    let deleted = asked("action=delete&media[]=802").await;
    assert_eq!(deleted.headers()["location"], "upload?deleted=1");
    let bad = "/hs-admin/upload?action=delete&media[]=801&_wpnonce=bad";
    let refused = send(&first, "GET", bad, &cookies, None).await;
    assert_eq!(refused.status().as_u16(), 403);
    let emptied = asked("action=-1&delete_all=Empty").await;
    assert_eq!(emptied.headers()["location"], "upload?deleted=1");
    // In the grid the same arguments are the screen's, not an action.
    let mode = |mode: &str| {
        format!(
            "UPDATE wp_usermeta SET meta_value = '{mode}' WHERE meta_key = 'wp_media_library_mode'"
        )
    };
    conn.execute(sqlx::AssertSqlSafe(mode("grid")))
        .await
        .unwrap();
    assert_ne!(status("action=delete&media[]=803").await, 302);
    conn.execute(sqlx::AssertSqlSafe(mode("list")))
        .await
        .unwrap();
    as_role(&db, "author").await;
    let (as_user, cookies) = site(&db, dir.path(), false).await;
    let nonce = session_nonce(&as_user, &cookies, "bulk-media").await;
    let uri = format!("/hs-admin/upload?action=delete&media[]=803&_wpnonce={nonce}");
    let denied = send(&as_user, "GET", &uri, &cookies, None).await;
    assert_eq!(denied.status().as_u16(), 500);
    as_role(&db, "subscriber").await;
    let (as_user, cookies) = site(&db, dir.path(), false).await;
    let uri = "/hs-admin/upload?action=delete&media[]=803";
    let denied = send(&as_user, "GET", uri, &cookies, None).await;
    assert_eq!(denied.status().as_u16(), 403);
}

/// Each role's attachment queries and single attachments answer as PHP's,
/// with their counts, with `MEDIA_TRASH` off and on.
#[tokio::test(flavor = "multi_thread")]
async fn media_reads_match_php() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = loaded().await;
    let reads = cases();
    let reads = reads.iter().filter(|case| {
        ["action=query-attachments", "action=get-attachment"]
            .iter()
            .any(|action| case.body.starts_with(action))
    });
    let mut compared = 0;
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        as_role(&db, role).await;
        let sites = [
            site(&db, dir.path(), false).await,
            site(&db, dir.path(), true).await,
        ];
        for case in reads.clone().filter(|case| case.role == role) {
            let (site, cookies) = &sites[usize::from(case.flags.contains("trash"))];
            let response = sent(site, cookies, case.body).await;
            let what = format!("{role} {}", case.body);
            assert_eq!(response.status().as_u16(), case.status, "{what}");
            let header = |name| {
                let value = response.headers().get(name);
                value
                    .map_or("", |value| value.to_str().unwrap())
                    .to_string()
            };
            let headers = format!("{},{}", header("x-wp-total"), header("x-wp-totalpages"));
            assert_eq!(headers, case.headers, "{what}");
            let ours = page_of(response).await;
            settled(&case.answer, &ours, &what);
            compared += 1;
        }
    }
    assert_eq!(compared, 66);
    // The library's screens are not ported with `MEDIA_TRASH`.
    let (trash, cookies) = site(&db, dir.path(), true).await;
    let response = send(&trash, "GET", "/hs-admin/upload", &cookies, None).await;
    assert_eq!(response.status().as_u16(), 501);
}
