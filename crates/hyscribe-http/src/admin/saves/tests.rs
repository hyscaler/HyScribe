// SPDX-License-Identifier: AGPL-3.0-or-later

//! The handlers through [`act`], against the write oracle: each runs on
//! the fixture database inside a transaction that is rolled back.

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Writer};
use hyscribe_phpser::{Key, Value, parse_str};
use hyscribe_testkit::writes::check_writes;
use hyscribe_testkit::{fixture_database, hs_admin_page, unhex};
use hyscribe_write::users::{UserData, wp_insert_user};
use tokio::runtime::Handle;

use super::super::Admin;
use super::*;
use crate::login::LoginReads;
use crate::tests::oracle_site;

mod comments;
pub(super) mod unfiltered;
pub(super) mod unread;

/// User 1's hs-admin on the fixture, as the handlers read it.
pub(super) fn admin<'a>(site: &'a crate::Site, reads: &'a LoginReads<'a>) -> Admin<'a> {
    read(|| {
        let user = reads.reads.wait(site.db.user(1)).unwrap();
        Admin::new(reads, site, user)
    })
}

/// One test at a time writes users: their count is one row, which
/// concurrent actions would deadlock over.
static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The turn of a test that writes users.
pub(super) async fn turn() -> tokio::sync::MutexGuard<'static, ()> {
    SERIAL.lock().await
}

pub(in crate::admin) async fn writer() -> Writer {
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    writer.unwrap().unwrap()
}

pub(in crate::admin) fn fields(query: &str) -> Vec<(Key, Value)> {
    parse_str(query.as_bytes()).as_array().unwrap().to_vec()
}

/// The write oracle's site address, which `reference/admin-saves/` keeps.
pub(in crate::admin) const SITE: &str = "http://127.0.0.1:8081";

/// A request to `uri` past its checks, by user 1 at time 0, with `get`
/// and `post` as `$_GET` and `$_POST`.
pub(in crate::admin) fn asked<'a>(
    uri: &'a str,
    get: &'a [(Key, Value)],
    post: &'a [(Key, Value)],
) -> Asked<'a> {
    Asked {
        get,
        post,
        referer: None,
        uri: uri.as_bytes(),
        home: SITE,
        user: 1,
        now: 0,
        ip: b"",
        agent: b"",
    }
}

/// What `attempt` answers once each statement budget short of it, which
/// it is given, has failed.
pub(in crate::admin) async fn settled<T>(
    mut attempt: impl AsyncFnMut(usize) -> Result<T, hyscribe_write::Error>,
) -> Result<T, hyscribe_write::Error> {
    let mut budget = 0;
    loop {
        match attempt(budget).await {
            // Any other error ends the test's transaction too.
            Err(hyscribe_write::Error::Db(err)) if err.to_string().contains("budget is spent") => {
                budget += 1;
            }
            answer => return answer,
        }
    }
}

/// `handler` asked `asked` by `site` as one action on `conn`, once every
/// statement budget short of it has failed, each failure rolled back.
pub(super) async fn acted<T>(
    writer: &Writer,
    conn: &mut Connection,
    (handler, site): (Handler<T>, &dyn Acting),
    asked: &Asked<'_>,
) -> Result<T, hyscribe_write::Error> {
    settled(async |budget| {
        let mut write = writer.begin_on(conn).await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        act(write, handler, asked, site).await
    })
    .await
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

/// The answer of the handler `request` names, as [`acted`].
pub(super) async fn answered(
    writer: &Writer,
    conn: &mut Connection,
    request: &Request<'_>,
) -> Result<Answer, hyscribe_write::Error> {
    let (get, post) = (fields(request.get), fields(request.post));
    let asked = Asked {
        user: request.user,
        ..asked("/hs-admin/", &get, &post)
    };
    let handler = handler(request.screen, request.action).unwrap();
    acted(writer, conn, (handler, &unread::Unread::default()), &asked).await
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

/// `reference/admin-saves/golden.sh`'s answers: a case, its status, its
/// `Location` and its body in hexadecimal.
pub(in crate::admin) const ANSWERS: &str =
    include_str!("../../../../hyscribe-admin/tests/golden/admin-saves.tsv");

/// The requests of `reference/admin-saves/golden.sh`.
pub(in crate::admin) const CASES: &str =
    include_str!("../../../../../reference/admin-saves/cases.list");

/// The pages PHP answers after them, a case, its status and its page in
/// hexadecimal.
pub(in crate::admin) const AFTER: &str =
    include_str!("../../../../hyscribe-admin/tests/golden/admin-saves-after.tsv");

/// The fields of the row of `case` in `rows`, a golden or [`CASES`].
pub(in crate::admin) fn case_row(rows: &'static str, case: &str) -> Option<Vec<&'static str>> {
    let row = rows
        .lines()
        .find(|row| row.starts_with(&format!("{case}\t")));
    row.map(|row| row.split('\t').collect())
}

/// `text` of `reference/admin-saves/` under hs-admin's names.
pub(in crate::admin) fn hs(text: &str) -> String {
    text.replace("wp-admin/", "hs-admin/")
        .replace("wp-admin%2F", "hs-admin%2F")
        .replace("wp-admin%252F", "hs-admin%252F")
        .replace(".php", "")
}

/// The golden's status, `Location` under hs-admin's names and body of
/// PHP's `case`.
pub(super) fn php_answer(case: &str) -> (String, String, String) {
    let fields = case_row(ANSWERS, case).unwrap();
    let body = String::from_utf8(unhex(fields[3])).unwrap();
    (fields[1].into(), hs(fields[2]), body)
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
#[tokio::test(flavor = "multi_thread")]
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
        let died = answered(&writer, conn, &promote(&none)).await;
        let own = "Sorry, you cannot remove your own role.";
        assert!(matches!(died, Ok(Answer::Denied(message)) if message == own));
        let alone = format!("new_role=editor&users[]={promoted}");
        let to = redirected(&writer, conn, &promote(&alone)).await;
        assert_eq!(to, php_answer("users-promote").1);
        let to = redirected(&writer, conn, &promote(&format!("{alone}&users[]=1"))).await;
        assert_eq!(to, php_answer("users-promote-self-editor").1);
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-users-promote", setup, action).await;
    report.unwrap();
}
