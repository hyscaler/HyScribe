// SPDX-License-Identifier: AGPL-3.0-or-later

//! The heartbeat of `reference/heartbeat/golden.sh`, sent to hs-admin over
//! `reference/revisions/content.sql` on `wordpress_saves`, with a writer.

use std::sync::Arc;

use axum::response::Response;
use hyscribe_config::Mode;
use hyscribe_db::Writer;
use sqlx::{Connection as _, Executor as _};
use tokio::runtime::Handle;

use crate::Site;
use crate::admin::tests::{SAVES, logged_in, page_of, send, with_values};
use crate::login::{LoginReads, Names};

const GOLDEN: &str = include_str!("../../../../tests/golden/heartbeat.tsv");

/// `wordpress_saves` with the fixture, the revisions' content and `sql`.
async fn loaded(sql: &str) -> hyscribe_config::Database {
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
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
    let content = include_str!("../../../../../../reference/revisions/content.sql");
    for statement in [content, sql].into_iter().filter(|sql| !sql.is_empty()) {
        let statement = sqlx::AssertSqlSafe(statement.to_string());
        conn.execute(sqlx::raw_sql(statement)).await.unwrap();
    }
    db
}

/// The site on `db` with a writer, and its login cookies.
async fn writing(db: &hyscribe_config::Database, dir: &std::path::Path) -> (Arc<Site>, String) {
    let (site, cookies) = logged_in("wordpress_saves", dir).await;
    let writer = Writer::open(db, Mode::Primary).await.unwrap();
    (
        Arc::new(Arc::try_unwrap(site).ok().unwrap().with_writer(writer)),
        cookies,
    )
}

/// The session token of the logged in cookie in `cookies`.
fn token(cookies: &str) -> Vec<u8> {
    let cookie = cookies
        .split("; ")
        .find(|c| c.starts_with("hyscribe_logged_in_"));
    let value = hyscribe_phpser::urldecode(cookie.unwrap().split_once('=').unwrap().1.as_bytes());
    let parsed = hyscribe_auth::cookie::parse_auth_cookie(&value).unwrap();
    parsed.token.to_vec()
}

/// The heartbeat nonce of `cookies`' session at `now`.
async fn nonce(site: &Arc<Site>, cookies: &str, now: i64) -> String {
    let (site, token) = (Arc::clone(site), token(cookies));
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, Handle::current(), now);
        reads.act_as(1, token);
        hyscribe_admin::login::Site::nonce(&reads, "heartbeat-nonce")
    })
    .await
    .unwrap()
}

/// `body` sent to `admin-ajax`.
async fn beat(site: &Arc<Site>, cookies: &str, body: &str) -> Response {
    let form = ("application/x-www-form-urlencoded", body);
    send(site, "POST", "/hs-admin/admin-ajax", cookies, Some(form)).await
}

/// Each role's heartbeats answer as PHP's: the nonces refreshed, the locks
/// checked and renewed, the visitor's; what PHP fails on is not ported.
#[tokio::test(flavor = "multi_thread")]
async fn heartbeats_answer_as_php() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let mut compared = 0;
    for role in ["-", "administrator", "author", "contributor"] {
        let caps = format!(
            "UPDATE wp_usermeta SET meta_value = 'a:1:{{s:{}:\"{role}\";b:1;}}' WHERE user_id = 1 AND meta_key = 'wp_capabilities'",
            role.len()
        );
        let db = loaded(if role == "-" { "" } else { &caps }).await;
        let (site, cookies) = writing(&db, dir.path()).await;
        let now = crate::now();
        let (fresh, old) = (
            nonce(&site, &cookies, now).await,
            nonce(&site, &cookies, now - 43_200).await,
        );
        for line in GOLDEN
            .lines()
            .filter(|line| line.starts_with(&format!("{role}\t")))
        {
            let [_, body, status, php] = line.split('\t').collect::<Vec<_>>()[..] else {
                panic!("{line}");
            };
            let sent = body.replace("%nonce%", &fresh).replace("%old_nonce%", &old);
            let cookies = if role == "-" { "" } else { cookies.as_str() };
            let response = beat(&site, cookies, &sent).await;
            if status == "500" {
                assert_eq!(response.status().as_u16(), 501, "{body}");
                compared += 1;
                continue;
            }
            assert_eq!(response.status().as_str(), status, "{role} {body}");
            let ours = page_of(response).await;
            assert_eq!(with_values(php, &ours), ours, "{role} {body}");
            compared += 1;
        }
    }
    assert_eq!(compared, 23);
}

/// A logged in user whose auth cookie expired within its grace is told
/// to log in again; the classic editor's autosave is not ported; and
/// without a writer, a heartbeat answers as shadow mode answers a write.
#[tokio::test(flavor = "multi_thread")]
async fn heartbeat_edges() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let db = loaded("").await;
    let (site, cookies) = writing(&db, dir.path()).await;
    let now = crate::now();
    let fresh = nonce(&site, &cookies, now).await;
    let body = format!("action=heartbeat&_nonce={fresh}");
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let query = sqlx::query_scalar("SELECT user_pass FROM wp_users WHERE ID = 1");
    let pass: String = query.fetch_one(&mut conn).await.unwrap();
    // The lock renewed is written as PHP writes it.
    let renew = format!("{body}&data[wp-refresh-post-lock][post_id]=1309");
    let answer = page_of(beat(&site, &cookies, &renew).await).await;
    let lock =
        "SELECT meta_value FROM wp_postmeta WHERE post_id = 1309 AND meta_key = '_edit_lock'";
    let lock: String = sqlx::query_scalar(lock).fetch_one(&mut conn).await.unwrap();
    assert!(
        answer.contains(&format!("\"new_lock\":\"{lock}\"")),
        "{answer}"
    );
    assert!(lock.ends_with(":1"));
    let salt = site.salt("auth", |_| None).unwrap();
    let expired = hyscribe_auth::cookie::generate_auth_cookie(
        b"admin",
        pass.as_bytes(),
        now - 60,
        &token(&cookies),
        &salt,
    );
    let expired = String::from_utf8(hyscribe_phpser::urlencode(&expired)).unwrap();
    let auth = format!("{}={expired}", Names::new(&site).auth(false));
    let answer = page_of(beat(&site, &auth, &body).await).await;
    assert!(answer.starts_with("{\"wp-auth-check\":false,"), "{answer}");
    let autosave = format!("{body}&data[wp_autosave][post_id]=1");
    let response = beat(&site, &cookies, &autosave).await;
    assert_eq!(response.status().as_u16(), 501);
    let unwritten = Arc::new(Arc::try_unwrap(site).ok().unwrap().with_writer(None));
    let response = beat(&unwritten, &cookies, &body).await;
    assert_eq!(response.status().as_u16(), 403);
}
