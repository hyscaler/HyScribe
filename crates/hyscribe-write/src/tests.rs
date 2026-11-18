// SPDX-License-Identifier: AGPL-3.0-or-later

//! The write oracle's harness: each port runs as actions over the writer, on
//! the fixture database, in a transaction that is rolled back.

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Write, Writer};
use hyscribe_phpser::Value;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;

/// An ID no row has: rolled back inserts still move the auto increment
/// counters, so a small one is reached after enough tests (PHP's `HYSCRIBE_MISSING_ID`).
pub(crate) const MISSING: u64 = 1 << 40;

/// One test at a time, so no test waits on another's locked rows.
pub(crate) static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn writer() -> Writer {
    Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap()
}

/// A connection of the test's own to the fixture database, to write around
/// an action in an outer transaction that is rolled back.
pub(crate) async fn connection() -> Connection {
    let db = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose())
        .charset(&db.charset);
    <Connection as sqlx::Connection>::connect_with(&options)
        .await
        .unwrap()
}

/// The oracle's request: user 1 on PHP's address, now.
pub(crate) fn admin() -> crate::posts::Context {
    crate::posts::Context {
        user: 1,
        can_publish: true,
        home: b"http://127.0.0.1:8081".to_vec(),
        upload_host: b"127.0.0.1:8081".to_vec(),
        unfiltered_html: false,
        constants: hyscribe_config::Constants::default(),
        now: crate::now(),
    }
}

/// A setup that writes nothing.
pub(crate) async fn nothing(_: &mut Write<'_>) {}

/// `text` as a PHP string.
pub(crate) fn text(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// Runs `setup`, then `action`, each as one action, and compares the rows
/// `action` wrote with the golden `name`.
pub(crate) async fn check<T>(
    name: &str,
    setup: impl AsyncFnOnce(&mut Write<'_>) -> T,
    action: impl AsyncFnOnce(&mut Write<'_>, T),
) {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let state = setup(&mut write).await;
        write.commit().await.unwrap();
        Ok::<_, String>(state)
    };
    let action = async |conn: &mut Connection, state| {
        let mut write = writer.begin_on(conn).await.unwrap();
        action(&mut write, state).await;
        write.commit().await.unwrap();
        Ok(())
    };
    let report = check_writes(&fixture_database(), name, setup, action).await;
    report.unwrap();
}

/// Runs `test` as one action in a transaction that is rolled back.
pub(crate) async fn rolled_back(test: impl AsyncFnOnce(&mut Write<'_>)) {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    test(&mut write).await;
}

/// Runs `action` after `setup` with each statement budget from none up,
/// each time in a transaction that is rolled back, until it succeeds:
/// every `?` on its way returns the database's error once.
pub(crate) async fn fails_at_each_step<T>(
    setup: impl AsyncFn(&mut Write<'_>) -> T,
    action: impl AsyncFn(&mut Write<'_>, T) -> Result<(), crate::Error>,
) {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    for budget in 0.. {
        let mut write = writer.begin().await.unwrap();
        let state = setup(&mut write).await;
        write.fail_after(budget);
        match action(&mut write, state).await {
            Ok(()) => break,
            // Only the budget may stop it, so a fault that fails every try ends the loop.
            Err(err) => assert!(
                matches!(&err, crate::Error::Db(db) if db.to_string().contains("budget is spent")),
                "{err:?}"
            ),
        }
    }
}
