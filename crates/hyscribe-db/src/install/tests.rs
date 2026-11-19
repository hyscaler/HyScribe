// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests make and drop tables in `wordpress_saves`, the scratch
//! database, one test at a time.

use super::*;
use hyscribe_config::Prefix;

static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn config(prefix: &str) -> Database {
    Database {
        name: "wordpress_saves".into(),
        prefix: Prefix::new(prefix).unwrap(),
        ..hyscribe_testkit::fixture_database()
    }
}

/// An installer over `wordpress_saves` with the prefix `prefix`, after
/// every table of that database is dropped.
async fn emptied(prefix: &str) -> Installer {
    let mut installer = Installer::open(&config(prefix)).await.unwrap();
    let tables = sqlx::query(
        "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()",
    )
    .fetch_all(&installer.pool)
    .await
    .unwrap();
    for table in tables {
        let name = String::from_utf8(table.get::<Vec<u8>, _>("name")).unwrap();
        installer.created.push(name);
    }
    installer.undo().await.unwrap();
    installer
}

const THINGS: &str = "CREATE TABLE `{prefix}things` (\n  `id` bigint(20) unsigned NOT NULL AUTO_INCREMENT,\n  \
                      `name` varchar(20) DEFAULT NULL,\n  PRIMARY KEY (`id`)\n) ENGINE=InnoDB;";

fn rows(table: &str, columns: &[&str], rows: &[&[Option<&str>]]) -> Rows {
    Rows {
        table: table.into(),
        columns: columns.iter().map(ToString::to_string).collect(),
        rows: rows
            .iter()
            .map(|row| row.iter().map(|value| value.map(String::from)).collect())
            .collect(),
    }
}

/// How many rows `table` has.
async fn count(installer: &Installer, table: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
        .fetch_one(&installer.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn tables_are_made_filled_and_dropped() {
    let _one = ONE_AT_A_TIME.lock().await;
    let mut installer = emptied("ins_").await;
    assert!(installer.existing().await.unwrap().is_empty());
    assert_eq!(installer.create(THINGS).await.unwrap(), "things");
    assert_eq!(installer.existing().await.unwrap(), ["ins_things"]);
    installer
        .insert(&[rows(
            "things",
            &["id", "name"],
            &[&[Some("7"), Some("seven")], &[Some("8"), None]],
        )])
        .await
        .unwrap();
    assert_eq!(count(&installer, "ins_things").await, 2);
    // A prefix is matched whatever its case, so a table of `INS_` counts.
    let upper = Installer::open(&config("INS_")).await.unwrap();
    assert_eq!(upper.existing().await.unwrap(), ["ins_things"]);
    installer.undo().await.unwrap();
    assert!(installer.existing().await.unwrap().is_empty());
}

#[tokio::test]
async fn tables_made_earlier_are_dropped_by_name() {
    let _one = ONE_AT_A_TIME.lock().await;
    let mut installer = emptied("ins_").await;
    installer.create(THINGS).await.unwrap();
    // A fresh installer, as the wizard's later step opens one.
    installer.created.clear();
    assert!(installer.drop_tables(&["bad name".into()]).await.is_err());
    assert_eq!(installer.existing().await.unwrap(), ["ins_things"]);
    installer.drop_tables(&["things".into()]).await.unwrap();
    assert!(installer.existing().await.unwrap().is_empty());
}

/// A drop that another connection's open transaction blocks gives up.
#[tokio::test]
async fn a_drop_that_is_blocked_gives_up() {
    let _one = ONE_AT_A_TIME.lock().await;
    let mut installer = emptied("ins_").await;
    installer.create(THINGS).await.unwrap();
    let db = config("ins_");
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut other = <sqlx::MySqlConnection as sqlx::Connection>::connect_with(&options)
        .await
        .unwrap();
    sqlx::raw_sql("START TRANSACTION; SELECT * FROM ins_things")
        .execute(&mut other)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let err = installer.undo().await.unwrap_err();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "{err}"
    );
    sqlx::raw_sql("ROLLBACK").execute(&mut other).await.unwrap();
    // Nothing was forgotten: the drop works once the lock is gone.
    installer.undo().await.unwrap();
    assert!(installer.existing().await.unwrap().is_empty());
}

#[tokio::test]
async fn bad_statements_and_names_are_refused() {
    let _one = ONE_AT_A_TIME.lock().await;
    let mut installer = emptied("ins_").await;
    for statement in [
        "DROP TABLE `{prefix}things`",
        "CREATE TABLE `{prefix}`",
        "CREATE TABLE `{prefix}a b` (id int)",
    ] {
        assert!(installer.create(statement).await.is_err(), "{statement}");
    }
    // The server's own refusal.
    let broken = "CREATE TABLE `{prefix}things` (id nonsense)";
    assert!(installer.create(broken).await.is_err());
    assert!(installer.existing().await.unwrap().is_empty());
    installer.create(THINGS).await.unwrap();
    assert!(installer.create(THINGS).await.is_err());
    for bad in [
        rows("th ings", &["id"], &[]),
        rows("things", &["id; DROP TABLE x"], &[]),
        rows("things", &[""], &[]),
        rows("missing", &["id"], &[&[Some("1")]]),
    ] {
        assert!(installer.insert(&[bad]).await.is_err());
    }
    installer.undo().await.unwrap();
}

#[tokio::test]
async fn a_failed_insert_leaves_no_rows() {
    let _one = ONE_AT_A_TIME.lock().await;
    let mut installer = emptied("ins_").await;
    installer.create(THINGS).await.unwrap();
    let good = rows("things", &["id", "name"], &[&[Some("1"), Some("one")]]);
    let twice = rows(
        "things",
        &["id", "name"],
        &[&[Some("2"), None], &[Some("2"), None]],
    );
    assert!(installer.insert(&[good, twice]).await.is_err());
    assert_eq!(count(&installer, "ins_things").await, 0);
    installer.undo().await.unwrap();
}

/// Every statement can fail, and each failure is reported.
#[tokio::test]
async fn every_statement_can_fail() {
    let _one = ONE_AT_A_TIME.lock().await;
    let all = async |installer: &mut Installer| -> Result<(), DbError> {
        installer.existing().await?;
        installer.create(THINGS).await?;
        let two = rows("things", &["id"], &[&[Some("1")], &[Some("2")]]);
        installer.insert(&[two]).await?;
        installer.undo().await
    };
    // Counted statements: the listing, the create, the begin, two rows, the
    // commit and the drop.
    for spent in 0..7 {
        let mut installer = emptied("ins_").await;
        installer.fail_after(Some(spent));
        let err = all(&mut installer).await.unwrap_err();
        assert_eq!(err.to_string(), "the test's statement budget is spent");
        installer.fail_after(None);
        installer.undo().await.unwrap();
        assert!(installer.existing().await.unwrap().is_empty());
    }
    let mut installer = emptied("ins_").await;
    installer.fail_after(Some(7));
    all(&mut installer).await.unwrap();
}

#[tokio::test]
async fn an_unreachable_database_is_reported() {
    let config = Database {
        port: 1,
        ..hyscribe_testkit::fixture_database()
    };
    assert!(Installer::open(&config).await.is_err());
}
