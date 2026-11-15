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

/// User 1's hs-admin on the fixture, as the handlers read it.
fn admin<'a>(site: &'a crate::Site, reads: &'a LoginReads<'a>) -> Admin<'a> {
    read(|| {
        let user = reads.reads.wait(site.db.user(1)).unwrap();
        Admin::new(reads, site, user)
    })
}

async fn writer() -> Writer {
    let writer = Writer::open(&fixture_database(), Mode::Primary).await;
    writer.unwrap().unwrap()
}

fn fields(query: &str) -> Vec<(Key, Value)> {
    parse_str(query.as_bytes()).as_array().unwrap().to_vec()
}

/// The users list's role change asked with `query` by user 1, as one
/// action on `conn` whose statements after `budget` fail.
async fn promote(
    writer: &Writer,
    admin: &Admin<'_>,
    conn: &mut Connection,
    query: &str,
    budget: Option<usize>,
) -> Result<Answer, hyscribe_write::Error> {
    let get = fields(query);
    let asked = Asked {
        get: &get,
        post: &[],
        referer: None,
        uri: b"/hs-admin/users",
        home: "http://127.0.0.1:8081",
        user: 1,
        now: 0,
    };
    let mut write = writer.begin_on(conn).await;
    if let (Ok(write), Some(budget)) = (&mut write, budget) {
        write.fail_after(budget);
    }
    act(write, handler("users", b"promote").unwrap(), &asked, admin).await
}

/// The redirect of `query` once every statement budget short of it has
/// failed, each failure rolled back.
async fn promoted(
    writer: &Writer,
    admin: &Admin<'_>,
    conn: &mut Connection,
    query: &str,
) -> String {
    let mut budget = 0;
    loop {
        match promote(writer, admin, conn, query, Some(budget)).await {
            Err(hyscribe_write::Error::Db(_)) => budget += 1,
            saved => {
                let Ok(Answer::Redirect(to)) = saved else {
                    panic!("{query} did not redirect");
                };
                return to;
            }
        }
    }
}

/// The golden's `Location` of PHP's `case`, under hs-admin's names.
fn php_location(case: &str) -> String {
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/admin-saves.tsv");
    let row = golden
        .lines()
        .find(|row| row.starts_with(&format!("{case}\t")));
    row.unwrap()
        .split('\t')
        .nth(2)
        .unwrap()
        .replace("users.php", "users")
}

/// `users.php`'s role change writes PHP's rows and answers as PHP: a user
/// made an editor, user 1 kept as an administrator, and nothing kept of a
/// request that dies part way or meets a failed statement.
#[tokio::test(flavor = "multi_thread")]
async fn the_role_change_is_php_s() {
    let writer = writer().await;
    let site = oracle_site().await;
    let reads = LoginReads::new(&site, Handle::current(), 0);
    let admin = admin(&site, &reads);
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let mut ids = Vec::new();
        for login in ["oracle-promoted", "oracle-kept"] {
            let some = |text: String| Some(text.into_bytes());
            let user = UserData {
                user_login: some(login.into()),
                user_pass: some("A pass phrase".into()),
                user_email: some(format!("{login}@example.test")),
                ..UserData::default()
            };
            ids.push(wp_insert_user(&mut write, &user).await.unwrap());
        }
        write.commit().await.unwrap();
        Ok::<_, String>(ids)
    };
    let action = async |conn: &mut Connection, ids: Vec<u64>| {
        let (promoted_id, kept) = (ids[0], ids[1]);
        let query = format!("new_role=none&users[]={kept}&users[]=1");
        let died = promote(&writer, &admin, conn, &query, None).await;
        let own = "Sorry, you cannot remove your own role.";
        assert!(matches!(died, Ok(Answer::Denied(message)) if message == own));
        let alone = format!("new_role=editor&users[]={promoted_id}");
        let to = promoted(&writer, &admin, conn, &alone).await;
        assert_eq!(to, php_location("users-promote"));
        let to = promoted(&writer, &admin, conn, &format!("{alone}&users[]=1")).await;
        assert_eq!(to, php_location("users-promote-self-editor"));
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-users-promote", setup, action).await;
    report.unwrap();
}
