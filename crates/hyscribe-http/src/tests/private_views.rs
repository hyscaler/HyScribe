// SPDX-License-Identifier: AGPL-3.0-or-later

//! The front end for a visitor and each role logged in, over the posts of
//! `reference/private-views/content.sql` in every status
//! (`reference/private-views/golden.sh`), the cookies that must leave a
//! request a visitor's, and what a `pagename` finds.

use std::sync::Arc;

use axum::http::{HeaderMap, StatusCode, header};
use sqlx::Connection;
use tokio::runtime::Handle;

use super::{NoFallback, PINNED_NOW, Reply, answer_with, as_wordpress, fixture_config};
use crate::admin::tests::{SAVES, salted, sha256};
use crate::login::LoginReads;
use crate::{Site, router};

/// Each role's cookies.
type Logins = Vec<(String, String)>;

/// The login names of the golden's roles.
const ROLES: [(&str, &str); 5] = [
    ("administrator", "admin"),
    ("editor", "editor"),
    ("author", "author"),
    ("contributor", "contributor"),
    ("subscriber", "subscriber"),
];

/// `wordpress_saves` as the golden script leaves the oracle's database:
/// the default install, `reference/rest/users.sql` and the golden's posts.
async fn database() -> hyscribe_config::Database {
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_config()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    for sql in [
        include_str!("../../../../reference/rest/users.sql"),
        include_str!("../../../../reference/private-views/content.sql"),
    ] {
        execute(&db, sql).await;
    }
    db
}

async fn execute(db: &hyscribe_config::Database, sql: &str) {
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut conn)
        .await
        .unwrap();
}

/// The site over `db` with the oracle's address, salts and sessions in
/// `dir`, under the permalink structure `options` names.
async fn site(db: &hyscribe_config::Database, dir: &std::path::Path, options: &str) -> Arc<Site> {
    let conn = hyscribe_db::Db::connect(db).await.unwrap();
    let oracle = Some("http://127.0.0.1:8081".to_string());
    let mut site = Site::load(conn, oracle.clone(), oracle).await.unwrap();
    if let Some(structure) = options.strip_prefix("permalink_structure=") {
        let structure = structure.split('&').next().unwrap();
        let structure = hyscribe_phpser::urldecode(structure.as_bytes());
        site.options.insert("permalink_structure".into(), structure);
    }
    site.hooks = Arc::new(NoFallback);
    site.build_rewriter();
    let sessions = hyscribe_auth::sessions::Sessions::new(dir);
    // PHP serves author archives.
    let site = salted(site).with_author_archives(true);
    Arc::new(site.with_sessions(sessions))
}

/// The cookies of `login` logged in at `/hs-login`.
async fn logged_in(site: &Arc<Site>, login: &str) -> String {
    let form = format!("log={login}&pwd=reference-only-password");
    let asked = axum::http::Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(axum::body::Body::from(form))
        .unwrap();
    let response = tower::ServiceExt::oneshot(router(Arc::clone(site)), asked)
        .await
        .unwrap();
    let pairs = response.headers().get_all(header::SET_COOKIE).iter();
    let pairs = pairs.map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string());
    let cookies: Vec<String> = pairs.collect();
    assert!(cookies.len() > 1, "{login} logged in");
    cookies.join("; ")
}

/// `wp_create_nonce( "post_preview_$id" )` at `now` for the user `cookie`
/// logs in, a visitor without one.
async fn preview_nonce(site: &Arc<Site>, cookie: &str, id: &str, now: i64) -> String {
    let (site, cookie) = (Arc::clone(site), cookie.to_string());
    let action = format!("post_preview_{id}");
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, Handle::current(), now);
        let jar = crate::login::cookies([cookie.as_bytes()]);
        if let Some((user, token)) = reads.current_user(&jar, false) {
            reads.act_as(user, token);
        }
        hyscribe_admin::login::Site::nonce(&reads, &action)
    })
    .await
    .unwrap()
}

/// The answer to `path` with `cookie` at `now`.
async fn view(site: &Arc<Site>, path: &str, cookie: &str, now: i64) -> Reply {
    PINNED_NOW
        .scope(
            now,
            answer_with(router(Arc::clone(site)), "GET", path, cookie),
        )
        .await
}

fn header_of(headers: &HeaderMap, name: header::HeaderName) -> &str {
    headers.get(name).map_or("", |v| v.to_str().unwrap())
}

/// Every request of `reference/private-views/views.list` answers as PHP
/// does for its user: the status, redirect and page, and a logged in user's cache headers.
#[tokio::test(flavor = "multi_thread")]
async fn private_views_match_php() {
    let _saves = SAVES.lock().await;
    let db = database().await;
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../tests/golden/private-views.tsv");
    let mut sites: Vec<(String, Arc<Site>, Logins)> = Vec::new();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (options, user) = match fields[0].split_once("_user=") {
            Some((options, user)) => (options.trim_end_matches('&'), user),
            None => (fields[0], ""),
        };
        if !sites.iter().any(|(known, ..)| known == options) {
            let site = site(&db, dir.path(), options).await;
            let mut jars = Vec::new();
            for (role, login) in ROLES {
                jars.push((role.to_string(), logged_in(&site, login).await));
            }
            sites.push((options.to_string(), site, jars));
        }
        let (_, site, jars) = sites.iter().find(|(known, ..)| known == options).unwrap();
        let cookie = jars
            .iter()
            .find(|(role, _)| role == user)
            .map_or("", |(_, cookie)| cookie.as_str());
        let now: i64 = fields[6].parse().unwrap();
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        let asked = match path.split_once("%nonce") {
            Some((before, rest)) => {
                let (id, after) = rest.split_once('%').unwrap();
                let nonce = preview_nonce(site, cookie, id, now).await;
                format!("{before}{nonce}{after}")
            }
            None => path.clone(),
        };
        let reply = view(site, &asked, cookie, now).await;
        let ours = [
            reply.status.as_str(),
            header_of(&reply.headers, header::LOCATION),
            header_of(&reply.headers, header::CACHE_CONTROL),
            header_of(&reply.headers, header::EXPIRES),
        ];
        let body = sha256(&as_wordpress(reply.body));
        // A visitor's cache headers are as before: PHP also sends
        // `nocache_headers()` for a 404 and a protected post, not ported.
        let compared = if user.is_empty() { 2 } else { 4 };
        if ours[..compared] != fields[2..2 + compared] || body != fields[7] {
            failures.push(format!("{user} {} {path}: {ours:?}", fields[0]));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(golden.lines().count(), 553);
}

/// A cookie that does not verify leaves the request a visitor's, byte for
/// byte: tampered, expired, or of a user deleted since; a user whose role
/// was lowered sees what their new role sees.
#[tokio::test(flavor = "multi_thread")]
async fn untrusted_cookies_are_visitors() {
    let _saves = SAVES.lock().await;
    let db = database().await;
    let dir = tempfile::tempdir().unwrap();
    let site = site(&db, dir.path(), "").await;
    let now = crate::now();
    // A private post, a draft and its preview, then lists: the home page,
    // a page's query block, the posts feed and a private post's comment feed.
    let paths = [
        "/?p=904",
        "/?p=901",
        "/?p=901&preview=true",
        "/",
        "/?page_id=942",
        "/?feed=rss2",
        "/?p=941&feed=rss2",
    ];
    let mut visitor = Vec::new();
    for path in paths {
        visitor.push(view(&site, path, "", now).await);
    }
    let same = |reply: &Reply, i: usize, why: &str| {
        let theirs: &Reply = &visitor[i];
        assert_eq!(reply.status, theirs.status, "{why} {}", paths[i]);
        assert_eq!(reply.body, theirs.body, "{why} {}", paths[i]);
        assert_eq!(reply.headers, theirs.headers, "{why} {}", paths[i]);
    };
    let admin = logged_in(&site, "admin").await;
    assert_eq!(
        view(&site, paths[0], &admin, now).await.status,
        StatusCode::OK
    );
    // A changed byte in each cookie's signature.
    let tampered = admin.replace("%7C", "%7Cx");
    // Past its expiry, two days on.
    let later = now + 3 * 86_400;
    for (i, path) in paths.iter().enumerate() {
        same(&view(&site, path, &tampered, now).await, i, "tampered");
        same(&view(&site, path, &admin, later).await, i, "expired");
    }
    // Only the auth cookie, which the browser keeps for the admin, counts too.
    let auth = admin
        .split("; ")
        .find(|pair| !pair.contains("logged_in") && !pair.contains("test_cookie"))
        .unwrap();
    assert_eq!(
        view(&site, paths[0], auth, now).await.status,
        StatusCode::OK
    );
    let author = logged_in(&site, "author").await;
    let editor = logged_in(&site, "editor").await;
    let subscriber = logged_in(&site, "subscriber").await;
    assert_eq!(
        view(&site, "/?p=916", &author, now).await.status,
        StatusCode::OK
    );
    execute(&db, "DELETE FROM wp_users WHERE ID = 702").await;
    let gone = view(&site, "/?p=916", &author, now).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
    same(&view(&site, paths[0], &author, now).await, 0, "deleted");
    assert_eq!(
        view(&site, paths[0], &editor, now).await.status,
        StatusCode::OK
    );
    let lowered = "UPDATE wp_usermeta SET meta_value = 'a:1:{s:10:\"subscriber\";b:1;}' \
                   WHERE user_id = 701 AND meta_key = 'wp_capabilities'";
    execute(&db, lowered).await;
    // Not the home page: their own private post stays theirs to read.
    for path in &paths[..3] {
        let as_editor = view(&site, path, &editor, now).await;
        let as_subscriber = view(&site, path, &subscriber, now).await;
        assert_eq!(as_editor.status, as_subscriber.status, "lowered {path}");
        assert_eq!(as_editor.body, as_subscriber.body, "lowered {path}");
    }
}

/// The render reads as the request's user: an administrator may read
/// another's private post, a subscriber may not, a visitor never.
#[tokio::test(flavor = "multi_thread")]
async fn renders_read_as_the_user() {
    use hyscribe_render::site::Site as _;
    let _saves = SAVES.lock().await;
    let db = database().await;
    let dir = tempfile::tempdir().unwrap();
    let site = site(&db, dir.path(), "").await;
    for (login, readable) in [("admin", true), ("subscriber", false), ("", false)] {
        let mut headers = HeaderMap::new();
        if !login.is_empty() {
            let cookie = logged_in(&site, login).await;
            headers.insert(header::COOKIE, cookie.parse().unwrap());
        }
        let viewer = crate::viewer::current(&site, &headers, false).await;
        assert_eq!(viewer.is_some(), !login.is_empty(), "{login}");
        let site = Arc::clone(&site);
        let can = tokio::task::spawn_blocking(move || {
            let mut reads = crate::reads::Reads::new(&site, Handle::current());
            reads.read_as(viewer);
            reads.can_read_post(904)
        });
        assert_eq!(can.await.unwrap(), readable, "{login}");
    }
}

/// A `pagename` finds pages only: an attachment at the path, even one
/// stored as published or private, is no page for a visitor or a user, and
/// the request goes on to the guess as it did before users were read.
#[tokio::test(flavor = "multi_thread")]
async fn a_pagename_finds_no_attachment() {
    let _saves = SAVES.lock().await;
    let db = database().await;
    let dir = tempfile::tempdir().unwrap();
    let stray = "INSERT INTO wp_posts (ID, post_author, post_date, post_date_gmt, post_content, \
                 post_title, post_excerpt, post_status, post_name, to_ping, pinged, \
                 post_content_filtered, post_type, post_mime_type) VALUES \
                 (960, 1, '2026-11-06 10:00:00', '2026-11-06 10:00:00', '', 'Stray file', '', \
                 'publish', 'stray-file', '', '', '', 'attachment', 'image/png'), \
                 (961, 1, '2026-11-06 11:00:00', '2026-11-06 11:00:00', '', 'Kept file', '', \
                 'private', 'kept-file', '', '', '', 'attachment', 'image/png')";
    execute(&db, stray).await;
    let site = site(&db, dir.path(), "").await;
    let admin = logged_in(&site, "admin").await;
    let now = crate::now();
    for cookie in ["", admin.as_str()] {
        let published = view(&site, "/?pagename=stray-file", cookie, now).await;
        assert_eq!(published.status, StatusCode::MOVED_PERMANENTLY, "{cookie}");
        assert_eq!(
            header_of(&published.headers, header::LOCATION),
            "http://127.0.0.1:8081/?attachment_id=960"
        );
        let private = view(&site, "/?pagename=kept-file", cookie, now).await;
        assert_eq!(private.status, StatusCode::NOT_FOUND, "{cookie}");
    }
}
