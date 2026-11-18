// SPDX-License-Identifier: AGPL-3.0-or-later

//! The lost password and reset password flows over `wordpress_saves` and
//! `reference/lost-password/content.sql`, as `reference/lost-password/golden.sh`
//! recorded PHP's, and the mails and sessions their writes leave.

use std::sync::Arc;

use hyscribe_auth::sessions::{Origin, Sessions};
use hyscribe_auth::{check_password, wp_verify_fast_hash};
use hyscribe_config::{Database, Mode};
use hyscribe_db::Writer;
use hyscribe_phpser::{md5, parse_str};
use hyscribe_testkit::unhex;
use sqlx::{Connection as _, Executor as _};
use tokio::runtime::Handle;

use super::super::tests::{all, hs_login, send};
use super::*;
use crate::Site;
use hyscribe_admin::login::{Session, Site as _};

const URL: &str = "http://127.0.0.1:8081";

/// lost-reset's stored key in `reference/lost-password/content.sql`.
const RESET_KEY: &[u8] = b"4102444800:$generic$2OXFKFqn5zqNAyOyU3N8wD1O8ES-1FwtFHRdHfnk";

/// `wordpress_saves` loaded with the fixture, the users of
/// `reference/lost-password/content.sql` and `sql`.
async fn loaded(sql: &str) -> Database {
    let db = Database {
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
    let content = include_str!("../../../../../reference/lost-password/content.sql");
    for statement in [content, sql].into_iter().filter(|sql| !sql.is_empty()) {
        let statement = sqlx::AssertSqlSafe(statement.to_string());
        conn.execute(sqlx::raw_sql(statement)).await.unwrap();
    }
    db
}

/// The site over `db`, with salts and sessions in `dir`, in primary mode
/// or in shadow mode.
async fn site(db: &Database, dir: &std::path::Path, primary: bool) -> Arc<Site> {
    let connected = hyscribe_db::Db::connect(db).await.unwrap();
    let site = Site::load(connected, Some(URL.into()), Some(URL.into()));
    let site = crate::admin::tests::salted(site.await.unwrap()).with_sessions(Sessions::new(dir));
    let writer = match primary {
        true => Writer::open(db, Mode::Primary).await.unwrap(),
        false => None,
    };
    // A transport that sends, so the flows answer as PHP's with mail.
    let recorder = Arc::new(crate::mail::tests::Recorder::default());
    let (site, _) = site
        .with_writer(writer)
        .with_mail(&Default::default(), recorder);
    Arc::new(site)
}

/// The user `id`'s stored password hash and reset key.
async fn stored(db: &Database, id: u64) -> (String, String) {
    let writer = Writer::open(db, Mode::Primary).await.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    let (_, user) = write.user(id).await.unwrap().unwrap();
    (user.pass, user.activation_key)
}

/// `page` with what the golden masks masked: the suggested password, the
/// time of utils' settings and user-profile's nonce.
fn masked(page: &str) -> String {
    let mut page = page.to_string();
    for (start, mask) in [
        ("data-pw=\"", "%pw%"),
        ("\"time\":\"", "%time%"),
        ("\"nonce\":\"", "%nonce:reset-password-for-user%"),
    ] {
        if let Some(at) = page.find(start) {
            let from = at + start.len();
            let to = from + page.get(from..).unwrap().find('"').unwrap();
            page.replace_range(from..to, mask);
        }
    }
    page
}

/// The reset password cookies HyScribe set as PHP's golden shows them.
fn reset_cookies(cookies: &[String]) -> String {
    let ours = cookies
        .iter()
        .filter(|c| c.starts_with("hyscribe-resetpass-"));
    let ours = ours.map(|cookie| {
        let cookie = cookie
            .replace("hyscribe-resetpass-", "wp-resetpass-")
            .replace("path=/hs-login", "path=/wp-login.php")
            .replace("; SameSite=Lax", "");
        let (head, rest) = cookie.split_once("; expires=").unwrap();
        let rest = rest.split_once("; path=").unwrap().1;
        format!("{head}; expires=%date%; Max-Age=%age%; path={rest}")
    });
    ours.collect::<Vec<_>>().join(" ")
}

/// Every request of `reference/lost-password/golden.sh`, in order, answers
/// as PHP's under HyScribe's URLs and cookie names: pages, redirects and
/// the reset password cookie; the keys and passwords written are PHP's.
#[tokio::test(flavor = "multi_thread")]
async fn the_lost_and_reset_password_flows_are_php_s() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded("").await;
    let dir = tempfile::tempdir().unwrap();
    let hash = md5(URL.as_bytes());
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/lost-password.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let uri = format!("/hs-login?{}", f[1]);
        let cookie = match f[2] {
            "" => String::new(),
            value => format!("hyscribe-resetpass-{hash}={value}"),
        };
        // A transport of its own, as the golden asks for more reset mails to
        // one account than an hour's limit.
        let site = site(&db, dir.path(), true).await;
        let reply = send(&site, f[0], &uri, &cookie, f[3]).await;
        assert_eq!(reply.status.as_str(), f[4], "{line:.100}");
        let location = reply.headers.get(header::LOCATION);
        let location = location.map_or("", |to| to.to_str().unwrap());
        assert_eq!(location, hs_login(f[5]), "{line:.100}");
        let cookies = all(&reply.headers, header::SET_COOKIE);
        assert_eq!(reset_cookies(&cookies), f[6], "{line:.100}");
        let php = hs_login(&String::from_utf8(unhex(f[7])).unwrap());
        assert!(masked(&reply.body) == php, "{line:.100}");
    }
    assert_eq!(golden.lines().count(), 42);
    let (_, key) = stored(&db, 1).await;
    let (time, hash) = key.split_once(':').unwrap();
    assert!(time.parse::<i64>().unwrap() > 1_794_000_000 && hash.starts_with("$generic$"));
    for (id, password) in [(701, &b"one"[..]), (706, b"It\\'s new")] {
        let (pass, key) = stored(&db, id).await;
        assert!(check_password(password, pass.as_bytes()), "{id}");
        assert_eq!(key, "", "{id}");
    }
}

/// The request a form posts to `query`.
fn posted<'a>(query: &'a Value, form: &'a Value) -> Request<'a> {
    Request {
        query,
        form: Some(form),
        test_cookie: false,
        logged_in_cookie: false,
        session: Session::None,
        reset_cookie: None,
        read_only: false,
    }
}

/// `retrieve_password()`'s mail carries the key whose hash it keeps, the
/// visitor's address and the user's locale, and no address for a logged in
/// user; a reset's notice goes to the admin, but for the admin's own reset,
/// and ends the user's sessions. An array as `redirect_to` keeps nothing.
#[tokio::test(flavor = "multi_thread")]
async fn the_mails_carry_the_key_and_a_reset_ends_the_sessions() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let locale =
        "UPDATE wp_usermeta SET meta_value = 'de_DE' WHERE user_id = 1 AND meta_key = 'locale'";
    let db = loaded(locale).await;
    let dir = tempfile::tempdir().unwrap();
    let site = site(&db, dir.path(), true).await;
    let origin = Origin {
        ip: b"",
        ua: b"",
        login: 0,
    };
    let sessions = site.sessions.as_ref().unwrap();
    let token = sessions.create(706, 4_102_444_800, &origin).unwrap();
    assert!(sessions.verify(706, token.as_bytes(), 0));
    let (query, form) = (parse_str(b""), parse_str(b"user_login=admin"));
    let fatal = parse_str(b"user_login=admin&redirect_to[]=x");
    let (handle, shared) = (Handle::current(), Arc::clone(&site));
    let (fatal, mails) = tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&shared, handle, crate::now());
        reads.client = Ok(Some("192.0.2.1".parse().unwrap()));
        let lost = |reads: &LoginReads, form| {
            lost_password(
                reads,
                &posted(&query, form),
                "lostpassword",
                &mut Vec::new(),
            )
        };
        let fatal = lost(&reads, &fatal);
        let kept = reads.user_by_login(b"admin").unwrap().activation_key;
        let mut mails = lost(&reads, &form).1;
        reads.act_as(1, Vec::new());
        mails.extend(lost(&reads, &form).1);
        ((fatal, kept), mails)
    })
    .await
    .unwrap();
    let unported = ((StatusCode::NOT_IMPLEMENTED, Vec::new()), Vec::new());
    assert_eq!(fatal, (unported, Vec::new()));
    let message = String::from_utf8(mails.last().unwrap().message.clone()).unwrap();
    let key = message
        .split("&key=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let (_, kept) = stored(&db, 1).await;
    assert!(wp_verify_fast_hash(
        key.as_bytes(),
        kept.split_once(':').unwrap().1.as_bytes()
    ));
    let (handle, shared) = (Handle::current(), Arc::clone(&site));
    let notices = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&shared, handle, crate::now());
        let (query, form) = (parse_str(b""), parse_str(b""));
        let reset = |id| reset(&reads, &posted(&query, &form), "resetpass", (id, b"new"));
        [706, 1].map(|id| {
            let (answer, mails) = reset(id);
            assert_eq!(answer.0, StatusCode::OK);
            mails
        })
    })
    .await
    .unwrap();
    let texts = [
        "HyScribe Default Install".to_string(),
        URL.into(),
        URL.into(),
        format!("{URL}/hs-admin/"),
        "admin@default-install.test".into(),
    ];
    let admin = User {
        login: b"admin",
        email: b"admin@default-install.test",
    };
    // Each mail as PHP builds it with the key it carries.
    for (mail, address) in mails.iter().zip([Some(&b"192.0.2.1"[..]), None]) {
        let message = String::from_utf8(mail.message.clone()).unwrap();
        let key = message
            .split("&key=")
            .nth(1)
            .unwrap()
            .split('&')
            .next()
            .unwrap();
        let site = mail_site(&texts);
        let php =
            hyscribe_mail::retrieve_password(&site, &admin, key.as_bytes(), b"de_DE", address);
        assert_eq!(*mail, php);
        assert_eq!(key.len(), 20);
    }
    assert_eq!(mails.len(), 2);
    // The admin's own reset cleared the key the last mail carried.
    let (_, key) = stored(&db, 1).await;
    assert_eq!(key, "");
    let reset_user = User {
        login: b"lost-reset",
        email: b"reset@example.test",
    };
    let notice = hyscribe_mail::password_reset_to_admin(&mail_site(&texts), &reset_user);
    assert_eq!(notices, [notice.into_iter().collect(), Vec::new()]);
    assert!(!sessions.verify(706, token.as_bytes(), 0));
    let (pass, _) = stored(&db, 706).await;
    assert!(check_password(b"new", pass.as_bytes()));
}

/// Shadow mode answers both forms as it answers every write, and a
/// write the database fails shows its error page.
#[tokio::test(flavor = "multi_thread")]
async fn shadow_mode_and_failed_writes_write_nothing() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    // A table the writes need gone, then a database HyScribe writes nothing to.
    let dropped = "DROP TABLE wp_usermeta";
    let version = "UPDATE wp_options SET option_value = '1' WHERE option_name = 'db_version'";
    for (primary, sql) in [(false, dropped), (true, dropped), (true, version)] {
        let db = loaded(sql).await;
        let site = site(&db, dir.path(), primary).await;
        let handle = Handle::current();
        let answers = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, crate::now());
            let (query, form) = (parse_str(b""), parse_str(b"user_login=admin"));
            let request = posted(&query, &form);
            let lost = lost_password(&reads, &request, "lostpassword", &mut Vec::new());
            let reset = reset(&reads, &request, "resetpass", (706, b"new"));
            let key = |login: &[u8]| reads.user_by_login(login).unwrap().activation_key;
            ((lost, reset), (key(b"admin"), key(b"lost-reset")))
        })
        .await
        .unwrap();
        let ((lost, reset), keys) = answers;
        let status = match primary {
            true => StatusCode::INTERNAL_SERVER_ERROR,
            false => StatusCode::FORBIDDEN,
        };
        for ((answer, mails), what) in [(lost, "lost"), (reset, "reset")] {
            assert_eq!((answer.0, mails.len()), (status, 0), "{what} {primary}");
            let page = String::from_utf8(answer.1).unwrap();
            let text = if primary {
                "Error establishing"
            } else {
                READ_ONLY
            };
            assert!(page.contains(text), "{what} {primary}");
        }
        assert_eq!(keys, (Vec::new(), RESET_KEY.to_vec()), "{sql}");
    }
}

/// A login hashes a password stored in an old format again and clears a
/// pending reset key, as `wp_signon()` does, and its cookies are signed for
/// the new hash; a write the database refuses logs nobody in.
#[tokio::test(flavor = "multi_thread")]
async fn a_login_rehashes_old_passwords_and_clears_reset_keys() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let login = async |db: &Database, form: &str| {
        let site = site(db, dir.path(), true).await;
        let reply = send(&site, "POST", "/hs-login", "hyscribe_test_cookie=1", form).await;
        (site, reply)
    };
    let db = loaded("UPDATE wp_users SET user_pass = MD5('old-format') WHERE ID = 706").await;
    let (site, reply) = login(&db, "log=lost-reset&pwd=old-format").await;
    assert_eq!(reply.status, StatusCode::FOUND);
    let (pass, key) = stored(&db, 706).await;
    assert!(pass.starts_with("$wp$2y$12$") && check_password(b"old-format", pass.as_bytes()));
    assert_eq!(key, "");
    let cookies = all(&reply.headers, axum::http::header::SET_COOKIE);
    let auth = cookies[1].split(';').next().unwrap();
    let again = send(&site, "GET", "/hs-login?redirect_to=%2Fx", auth, "").await;
    assert_eq!(again.status, StatusCode::FOUND);
    // A current hash: only the key is cleared.
    let admin = "log=admin&pwd=reference-only-password";
    let keyed = "UPDATE wp_users SET user_activation_key = 'k' WHERE ID = 1";
    let db = loaded(keyed).await;
    let current = stored(&db, 1).await.0;
    assert_eq!(login(&db, admin).await.1.status, StatusCode::FOUND);
    assert_eq!(stored(&db, 1).await, (current, String::new()));
    // Writes refused at an unsupported database version, and a write the
    // database fails.
    let version = "UPDATE wp_options SET option_value = '1' WHERE option_name = 'db_version'";
    for broken in [version, "DROP TABLE wp_usermeta"] {
        let db = loaded(&format!("{keyed}; {broken}")).await;
        let (site, refused) = login(&db, admin).await;
        assert_eq!(
            refused.status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{broken}"
        );
        let handle = Handle::current();
        let key = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, crate::now());
            reads.user_by_login(b"admin").unwrap().activation_key
        });
        assert_eq!(key.await.unwrap(), b"k", "{broken}");
    }
}

/// Without a transport the reset mail is not sent: the key is kept, and
/// the form answers PHP's `retrieve_password_email_failure`.
#[tokio::test(flavor = "multi_thread")]
async fn a_reset_mail_not_sent_is_php_s_error() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded("").await;
    let connected = hyscribe_db::Db::connect(&db).await.unwrap();
    let site = Site::load(connected, Some(URL.into()), Some(URL.into()));
    let writer = Writer::open(&db, Mode::Primary).await.unwrap();
    let site = Arc::new(site.await.unwrap().with_writer(writer));
    let (query, form) = (parse_str(b""), parse_str(b"user_login=admin"));
    let handle = Handle::current();
    let ((status, page), mails) = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, crate::now());
        let request = posted(&query, &form);
        lost_password(&reads, &request, "lostpassword", &mut Vec::new())
    })
    .await
    .unwrap();
    let page = String::from_utf8(page).unwrap();
    assert_eq!((status, mails.len()), (StatusCode::OK, 1));
    assert!(page.contains("The email could not be sent."), "{page}");
    let (_, key) = stored(&db, 1).await;
    assert!(!key.is_empty());
}
