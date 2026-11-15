// SPDX-License-Identifier: AGPL-3.0-or-later

//! The handlers through [`act`], against the write oracle: each runs on
//! the fixture database inside a transaction that is rolled back.

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Writer};
use hyscribe_phpser::{Key, Value, parse_str};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::users::{UserData, wp_insert_user};

use super::*;

/// One test at a time writes users: their count is one row, which
/// concurrent actions would deadlock over.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The turn of a test that writes users.
pub(super) async fn turn() -> tokio::sync::MutexGuard<'static, ()> {
    SERIAL.lock().await
}

pub(super) async fn writer() -> Writer {
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    writer.unwrap().unwrap()
}

pub(super) fn fields(query: &str) -> Vec<(Key, Value)> {
    parse_str(query.as_bytes()).as_array().unwrap().to_vec()
}

/// A request to a handler: its screen and action, `$_GET` and `$_POST` as
/// queries, and the current user.
pub(super) struct Request<'q> {
    pub screen: &'q str,
    pub action: &'q [u8],
    pub get: &'q str,
    pub post: &'q str,
    pub user: u64,
}

/// The handler `request` names, asked by it as one action on `conn` whose
/// statements after `budget` fail.
pub(super) async fn acted(
    writer: &Writer,
    conn: &mut Connection,
    request: &Request<'_>,
    budget: Option<usize>,
) -> Result<Answer, hyscribe_write::Error> {
    let (get, post) = (fields(request.get), fields(request.post));
    let asked = Asked {
        get: &get,
        post: &post,
        referer: None,
        uri: b"/hs-admin/",
        home: "http://127.0.0.1:8081",
        user: request.user,
    };
    let mut write = writer.begin_on(conn).await;
    if let (Ok(write), Some(budget)) = (&mut write, budget) {
        write.fail_after(budget);
    }
    let handler = handler(request.screen, request.action).unwrap();
    act(write, handler, &asked).await
}

/// The answer to `request` once every statement budget short of it has
/// failed, each failure rolled back.
pub(super) async fn answered(
    writer: &Writer,
    conn: &mut Connection,
    request: &Request<'_>,
) -> Result<Answer, hyscribe_write::Error> {
    let mut budget = 0;
    loop {
        match acted(writer, conn, request, Some(budget)).await {
            // Any other error ends the test's transaction too.
            Err(hyscribe_write::Error::Db(err)) if err.to_string().contains("budget is spent") => {
                budget += 1;
            }
            answer => return answer,
        }
    }
}

/// The redirect of `request`, once every budget short of it has failed.
pub(super) async fn redirected(
    writer: &Writer,
    conn: &mut Connection,
    request: &Request<'_>,
) -> String {
    match answered(writer, conn, request).await {
        Ok(Answer::Redirect(to)) => to,
        _ => panic!("{} did not redirect", request.post),
    }
}

/// `text` under hs-admin's names.
pub(super) fn hs_names(text: &str) -> String {
    let text = text
        .replace("wp-admin/", "hs-admin/")
        .replace("wp-admin%2F", "hs-admin%2F");
    ["users", "user-new", "user-edit", "profile"]
        .iter()
        .fold(text, |text, screen| {
            text.replace(&format!("{screen}.php"), screen)
        })
}

/// The golden's status, `Location` under hs-admin's names and body of
/// PHP's `case`.
pub(super) fn php_answer(case: &str) -> (String, String, Vec<u8>) {
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/admin-saves.tsv");
    let row = golden
        .lines()
        .find(|row| row.starts_with(&format!("{case}\t")));
    let fields: Vec<&str> = row.unwrap().split('\t').collect();
    let body = hyscribe_testkit::unhex(fields[3]);
    (fields[1].into(), hs_names(fields[2]), body)
}

/// The golden's `Location` of PHP's `case`, under hs-admin's names.
pub(super) fn php_location(case: &str) -> String {
    php_answer(case).1
}

/// Whether the user `id`'s stored hash is of `password`, which the write
/// oracle masks.
pub(super) async fn password_is(
    writer: &Writer,
    conn: &mut Connection,
    id: u64,
    password: &[u8],
) -> bool {
    let mut write = writer.begin_on(conn).await.unwrap();
    let (_, user) = write.user(id).await.unwrap().unwrap();
    hyscribe_auth::check_password(password, user.pass.as_bytes())
}

/// A user made as the oracle's cases make them, with `role`.
pub(super) async fn new_user(
    writer: &Writer,
    conn: &mut Connection,
    login: &str,
    role: &str,
) -> u64 {
    let mut write = writer.begin_on(conn).await.unwrap();
    let some = |text: String| Some(text.into_bytes());
    let user = UserData {
        user_login: some(login.into()),
        user_pass: some("A pass phrase".into()),
        user_email: some(format!("{login}@example.test")),
        role: (!role.is_empty()).then(|| role.as_bytes().to_vec()),
        ..UserData::default()
    };
    let id = wp_insert_user(&mut write, &user).await.unwrap();
    write.commit().await.unwrap();
    id
}

/// `users.php`'s role change writes PHP's rows and answers as PHP: a user
/// made an editor, user 1 kept as an administrator, and nothing kept of a
/// request that dies part way or meets a failed statement.
#[tokio::test]
async fn the_role_change_is_php_s() {
    let _turn = turn().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let promoted = new_user(&writer, conn, "oracle-promoted", "").await;
        let kept = new_user(&writer, conn, "oracle-kept", "").await;
        Ok::<_, String>((promoted, kept))
    };
    let action = async |conn: &mut Connection, (promoted, kept): (u64, u64)| {
        fn promote(get: &str) -> Request<'_> {
            Request {
                screen: "users",
                action: b"promote",
                get,
                post: "",
                user: 1,
            }
        }
        let none = format!("new_role=none&users[]={kept}&users[]=1");
        let died = acted(&writer, conn, &promote(&none), None).await;
        let own = "Sorry, you cannot remove your own role.";
        assert!(matches!(died, Ok(Answer::Denied(message)) if message == own));
        let alone = format!("new_role=editor&users[]={promoted}");
        let to = redirected(&writer, conn, &promote(&alone)).await;
        assert_eq!(to, php_location("users-promote"));
        let to = redirected(&writer, conn, &promote(&format!("{alone}&users[]=1"))).await;
        assert_eq!(to, php_location("users-promote-self-editor"));
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-users-promote", setup, action).await;
    report.unwrap();
}
