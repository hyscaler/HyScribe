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

use super::super::tests::{all, hs_login, hurried, send};
use super::*;
use crate::Site;
use hyscribe_admin::login::Session;

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

/// `site`, a site anyone may register on, or not.
fn registering(site: Arc<Site>, open: bool) -> Arc<Site> {
    let mut site = Arc::try_unwrap(site).unwrap();
    let on = if open { b"1".to_vec() } else { b"0".to_vec() };
    site.options.insert("users_can_register".into(), on);
    Arc::new(site)
}

/// Every registration form of `reference/register/golden.sh`, in order,
/// answers as PHP's: the form with its errors, or the redirect, the users
/// made being the write oracle's `login-register`.
#[tokio::test(flavor = "multi_thread")]
async fn registrations_are_php_s() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded("").await;
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/register.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let site = registering(site(&db, dir.path(), true).await, f[1] == "register");
        let uri = format!("/hs-login?{}", f[0]);
        let reply = send(&site, "POST", &uri, "", f[2]).await;
        assert_eq!(reply.status.as_str(), f[3], "{line:.100}");
        let location = reply.headers.get(header::LOCATION);
        let location = location.map_or("", |to| to.to_str().unwrap());
        assert_eq!(location, hs_login(f[4]), "{line:.100}");
        let php = hs_login(&String::from_utf8(unhex(f[6])).unwrap());
        assert!(reply.body == php, "{line:.100}");
        // The test cookie is all a form sets.
        assert_eq!(f[5], "", "{line:.100}");
    }
    assert_eq!(golden.lines().count(), 23);
}

/// A registration's mails go to the admin and to the new user, with the
/// key whose hash the user keeps, and the user is a subscriber with the
/// password nag; a form as an array in `redirect_to` is not committed.
#[tokio::test(flavor = "multi_thread")]
async fn a_registration_mails_the_admin_and_the_user() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded("").await;
    let dir = tempfile::tempdir().unwrap();
    let site = registering(site(&db, dir.path(), true).await, true);
    let (handle, shared) = (Handle::current(), Arc::clone(&site));
    let (answers, mails) = tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&shared, handle, crate::now());
        reads.client = Ok(Some("192.0.2.1".parse().unwrap()));
        let query = parse_str(b"");
        let post = |form: &[u8]| {
            let form = parse_str(form);
            register(&reads, &posted(&query, &form), &mut Vec::new())
        };
        let fatal = post(b"user_login=reg-fatal&user_email=fatal@example.test&redirect_to[]=x");
        let ((status, _), mails) = post(b"user_login=reg-mail&user_email=reg-mail@example.test");
        ((fatal, status), mails)
    })
    .await
    .unwrap();
    let ((fatal, status), mails) = (answers, mails);
    assert_eq!(
        fatal,
        ((StatusCode::NOT_IMPLEMENTED, Vec::new()), Vec::new())
    );
    assert_eq!(status, StatusCode::FOUND);
    let texts = [
        "HyScribe Default Install".to_string(),
        URL.into(),
        URL.into(),
        format!("{URL}/hs-admin/"),
        "admin@default-install.test".into(),
    ];
    let user = User {
        login: b"reg-mail",
        email: b"reg-mail@example.test",
    };
    let to_admin = hyscribe_mail::new_user_to_admin(&mail_site(&texts), &user);
    assert_eq!(mails[0], to_admin);
    let message = String::from_utf8(mails[1].message.clone()).unwrap();
    let key = message
        .split("&key=")
        .nth(1)
        .unwrap()
        .split('&')
        .next()
        .unwrap();
    let to_user = hyscribe_mail::new_user_to_user(&mail_site(&texts), &user, key.as_bytes());
    assert_eq!((mails.len(), &mails[1]), (2, &to_user));
    let writer = Writer::open(&db, Mode::Primary).await.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    let id = write.user_id_by_login(b"reg-mail").await.unwrap().unwrap();
    let (_, row) = write.user(id).await.unwrap().unwrap();
    assert!(wp_verify_fast_hash(
        key.as_bytes(),
        row.activation_key.split_once(':').unwrap().1.as_bytes()
    ));
    assert!(
        write
            .user_id_by_login(b"reg-fatal")
            .await
            .unwrap()
            .is_none()
    );
}

/// Past its turn or its place under the site wide cap, or with no client
/// named, a registration is refused as a failure to register, with nothing
/// written; shadow mode and a failing database answer as for every write.
#[tokio::test(flavor = "multi_thread")]
async fn registrations_are_held_to_the_login_limits() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded("").await;
    let dir = tempfile::tempdir().unwrap();
    let mut open = Arc::try_unwrap(site(&db, dir.path(), true).await).unwrap();
    let limiter = Arc::get_mut(&mut open.limiter).unwrap();
    limiter.checks = crate::login::limits::checks::Checks::new(1, hurried);
    let limited = registering(Arc::new(open), true);
    let client: std::net::IpAddr = "192.0.2.7".parse().unwrap();
    let (handle, shared) = (Handle::current(), Arc::clone(&limited));
    let answers = tokio::task::spawn_blocking(move || {
        let form = parse_str(b"user_login=reg-held&user_email=reg-held@example.test");
        let query = parse_str(b"");
        let ask = |client| {
            let mut reads = LoginReads::new(&shared, handle.clone(), crate::now());
            reads.client = client;
            register(&reads, &posted(&query, &form), &mut Vec::new())
        };
        let turn = shared.limiter.turn(Some(client));
        let busy = ask(Ok(Some(client)));
        drop(turn);
        let unnamed = ask(Err(crate::comment_post::Unnamed("unknown".into())));
        let held = shared.limiter.checks.start();
        let capped = ask(Ok(Some(client)));
        drop(held);
        [busy, unnamed, capped]
    })
    .await
    .unwrap();
    for ((status, page), mails) in answers {
        let page = String::from_utf8(page).unwrap();
        assert_eq!((status, mails.len()), (StatusCode::OK, 0));
        assert!(page.contains("Could not register you"), "{page}");
        assert!(page.contains("value=\"reg-held\""), "{page}");
    }
    let writer = Writer::open(&db, Mode::Primary).await.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    assert!(write.user_id_by_login(b"reg-held").await.unwrap().is_none());
    // Its open transaction would hold the tables the next fixtures drop.
    drop(write);
    // Shadow mode, then a table the write needs gone.
    for (primary, sql) in [
        (false, "DROP TABLE wp_usermeta"),
        (true, "DROP TABLE wp_usermeta"),
        (
            true,
            "UPDATE wp_options SET option_value = '1' WHERE option_name = 'db_version'",
        ),
    ] {
        let db = loaded(sql).await;
        let site = registering(site(&db, dir.path(), primary).await, true);
        let handle = Handle::current();
        let (status, page) = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, crate::now());
            let form = parse_str(b"user_login=reg-late&user_email=reg-late@example.test");
            let query = parse_str(b"");
            register(&reads, &posted(&query, &form), &mut Vec::new()).0
        })
        .await
        .unwrap();
        let page = String::from_utf8(page).unwrap();
        let (code, text) = match primary {
            true => (StatusCode::INTERNAL_SERVER_ERROR, "Error establishing"),
            false => (StatusCode::FORBIDDEN, READ_ONLY),
        };
        assert_eq!(status, code, "{primary}");
        assert!(page.contains(text), "{primary}");
    }
}

const REQUESTS: &str = include_str!("../../../../../reference/confirm-action/content.sql");

/// The status and `_wp_admin_notified` meta of the request `id`.
async fn request_state(db: &Database, id: u64) -> (String, bool) {
    let writer = Writer::open(db, Mode::Primary).await.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    let post = write.post(id).await.unwrap().unwrap();
    let key = "_wp_admin_notified";
    let meta = hyscribe_write::options::get_metadata(&mut write, hyscribe_db::Meta::Post, id, key);
    (post.status, !meta.await.unwrap().is_empty())
}

/// Every link of `reference/confirm-action/golden.sh`, in order, answers
/// as PHP's, and confirms what PHP confirms: a pending or failed request
/// becomes confirmed, and the admin's mail is noted as sent.
#[tokio::test(flavor = "multi_thread")]
async fn confirmation_links_are_php_s() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded(REQUESTS).await;
    let dir = tempfile::tempdir().unwrap();
    let site = site(&db, dir.path(), true).await;
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/confirm-action.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let reply = send(&site, "GET", &format!("/hs-login?{}", f[0]), "", "").await;
        assert_eq!(reply.status.as_str(), f[1], "{line:.100}");
        let php = hs_login(&String::from_utf8(unhex(f[2])).unwrap());
        assert!(reply.body == php, "{line:.100}");
    }
    assert_eq!(golden.lines().count(), 21);
    for (id, state) in [
        (951, ("request-confirmed", true)),
        (952, ("request-confirmed", true)),
        (953, ("request-confirmed", true)),
        (954, ("request-pending", false)),
        (955, ("request-confirmed", false)),
        (956, ("request-completed", false)),
        (959, ("request-confirmed", true)),
        (960, ("request-pending", false)),
    ] {
        assert_eq!(
            request_state(&db, id).await,
            (state.0.into(), state.1),
            "{id}"
        );
    }
}

/// The admin is mailed once, in PHP's words for the request's action, and
/// a request of another action is not confirmed (its mail is not ported);
/// without a transport the mail is not sent and the admin is not noted as
/// told; shadow mode and a failing database answer as for every write.
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmation_mails_the_admin_once() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded(REQUESTS).await;
    let dir = tempfile::tempdir().unwrap();
    let live = site(&db, dir.path(), true).await;
    let link = |id: u64| {
        format!("/hs-login?action=confirmaction&request_id={id}&confirm_key=freshkey0123456789AB")
    };
    let other = send(&live, "GET", &link(960), "", "").await;
    assert_eq!(other.status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(request_state(&db, 960).await.0, "request-pending");
    let (handle, shared) = (Handle::current(), Arc::clone(&live));
    let mails = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&shared, handle, crate::now());
        let query = parse_str(b"");
        let found = |id, name: &[u8]| UserRequest {
            id,
            email: b"eraser@example.test".to_vec(),
            action_name: name.to_vec(),
            status: "request-pending".into(),
            confirm_key: Vec::new(),
            modified: 0,
        };
        let mut mails = Vec::new();
        for (id, name) in [
            (952, &b"remove_personal_data"[..]),
            (951, b"export_personal_data"),
        ] {
            let form = parse_str(b"");
            let request = posted(&query, &form);
            let ((status, page), sent) =
                confirm(&reads, &request, (&found(id, name), id.cast_signed()));
            assert_eq!(status, StatusCode::OK);
            assert!(
                String::from_utf8(page)
                    .unwrap()
                    .contains("Thanks for confirming your")
            );
            mails.extend(sent);
        }
        // The admin was told of both: confirming again mails nobody.
        let form = parse_str(b"");
        let request = posted(&query, &form);
        let again = confirm(
            &reads,
            &request,
            (&found(952, b"remove_personal_data"), 952),
        );
        assert!(again.1.is_empty());
        mails
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
    let site_texts = mail_site(&texts);
    let erase =
        hyscribe_mail::request_confirmed(&site_texts, b"eraser@example.test", PrivacyAction::Erase);
    let export = hyscribe_mail::request_confirmed(
        &site_texts,
        b"reader@example.test",
        PrivacyAction::Export,
    );
    assert_eq!(mails, [erase, export]);
    // No transport: the mail is not sent, so the admin is not told.
    let db = loaded(REQUESTS).await;
    let connected = hyscribe_db::Db::connect(&db).await.unwrap();
    let silent = Site::load(connected, Some(URL.into()), Some(URL.into()));
    let writer = Writer::open(&db, Mode::Primary).await.unwrap();
    let silent = Arc::new(silent.await.unwrap().with_writer(writer));
    let reply = send(&silent, "GET", &link(951), "", "").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(
        request_state(&db, 951).await,
        ("request-confirmed".into(), false)
    );
    // A database that refuses only the note that the admin was told: the
    // request stays confirmed and the page is the error page.
    let trigger = "CREATE TRIGGER no_told BEFORE INSERT ON wp_postmeta FOR EACH ROW \
                   SET NEW.post_id = IF(NEW.meta_key = '_wp_admin_notified', NULL, NEW.post_id)";
    let db = loaded(&format!("{REQUESTS}\n{trigger}")).await;
    let refusing = site(&db, dir.path(), true).await;
    let reply = send(&refusing, "GET", &link(951), "", "").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(request_state(&db, 951).await.0, "request-confirmed");
    // A database that stops taking writes once the request is confirmed: the
    // admin's note cannot begin.
    let trigger = "CREATE TRIGGER stale AFTER UPDATE ON wp_posts FOR EACH ROW \
                   UPDATE wp_options SET option_value = '1' WHERE option_name = 'db_version'";
    let db = loaded(&format!("{REQUESTS}\n{trigger}")).await;
    let stopping = site(&db, dir.path(), true).await;
    let reply = send(&stopping, "GET", &link(951), "", "").await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    // Shadow mode, then a table the write needs gone.
    for (primary, sql) in [
        (false, "DROP TABLE wp_postmeta"),
        (true, "DROP TABLE wp_postmeta"),
        (
            true,
            "UPDATE wp_options SET option_value = '1' WHERE option_name = 'db_version'",
        ),
    ] {
        let db = loaded(&format!("{REQUESTS}\n{sql}")).await;
        let site = site(&db, dir.path(), primary).await;
        let reply = send(&site, "GET", &link(951), "", "").await;
        let (code, text) = match primary {
            true => (StatusCode::INTERNAL_SERVER_ERROR, "Error establishing"),
            false => (StatusCode::FORBIDDEN, READ_ONLY),
        };
        assert_eq!(reply.status, code, "{primary}");
        assert!(reply.body.contains(text), "{primary}");
    }
}

/// Registrations are capped per client address and hour: past the
/// cap, PHP's registration error and nothing written; only accounts really
/// created count, and `-1` lifts the cap.
#[tokio::test(flavor = "multi_thread")]
async fn registrations_are_capped_per_address() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = loaded("").await;
    let dir = tempfile::tempdir().unwrap();
    let limits = |registrations| hyscribe_config::LoginLimits {
        registrations,
        ..Default::default()
    };
    let capped = [limits(Some(2)), limits(None)];
    let mut sites = Vec::new();
    for limits in capped {
        let open = Arc::try_unwrap(site(&db, dir.path(), true).await).unwrap();
        sites.push(registering(Arc::new(open.with_login_limits(limits)), true));
    }
    let handle = Handle::current();
    let answers = tokio::task::spawn_blocking(move || {
        let client: std::net::IpAddr = "192.0.2.9".parse().unwrap();
        let query = parse_str(b"");
        let mut statuses = Vec::new();
        for (site, forms) in sites.iter().zip([
            &[
                // Refused by PHP's own checks: not created, not counted.
                "user_login=&user_email=",
                // Made, but never committed: not counted.
                "user_login=cap-fatal&user_email=cap-fatal@example.test&redirect_to[]=x",
                "user_login=cap-a&user_email=cap-a@example.test",
                "user_login=cap-b&user_email=cap-b@example.test",
                "user_login=cap-c&user_email=cap-c@example.test",
            ][..],
            &[
                "user_login=cap-d&user_email=cap-d@example.test",
                "user_login=cap-e&user_email=cap-e@example.test",
                "user_login=cap-f&user_email=cap-f@example.test",
                "user_login=cap-g&user_email=cap-g@example.test",
            ][..],
        ]) {
            for form in forms {
                let mut reads = LoginReads::new(site, handle.clone(), crate::now());
                reads.client = Ok(Some(client));
                let form = parse_str(form.as_bytes());
                let ((status, page), _) = register(&reads, &posted(&query, &form), &mut Vec::new());
                let page = String::from_utf8(page).unwrap();
                statuses.push((status, page.contains("Could not register you")));
            }
        }
        statuses
    })
    .await
    .unwrap();
    let ok = (StatusCode::FOUND, false);
    let page = (StatusCode::OK, true);
    assert_eq!(
        answers,
        [
            (StatusCode::OK, false),
            (StatusCode::NOT_IMPLEMENTED, false),
            ok,
            ok,
            page,
            ok,
            ok,
            ok,
            ok,
        ]
    );
    let writer = Writer::open(&db, Mode::Primary).await.unwrap().unwrap();
    let mut write = writer.begin().await.unwrap();
    for (login, made) in [
        (&b"cap-a"[..], true),
        (b"cap-b", true),
        (b"cap-c", false),
        (b"cap-fatal", false),
        (b"cap-g", true),
    ] {
        let found = write.user_id_by_login(login).await.unwrap();
        assert_eq!(found.is_some(), made, "{}", String::from_utf8_lossy(login));
    }
}
