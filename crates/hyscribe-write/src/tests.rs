// SPDX-License-Identifier: AGPL-3.0-or-later

//! The write oracle's harness: each port runs as actions over the writer, on
//! the fixture database, in a transaction that is rolled back.

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Write, Writer};
use hyscribe_phpser::Value;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;

/// One test at a time, so no test waits on another's locked rows.
pub(crate) static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(crate) async fn writer() -> Writer {
    Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap()
}

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
            Err(err) => assert!(matches!(err, crate::Error::Db(_)), "{err:?}"),
        }
    }
}
