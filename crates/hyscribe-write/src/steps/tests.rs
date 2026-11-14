// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_config::Mode;
use hyscribe_db::Writer;
use hyscribe_testkit::fixture_database;
use sqlx::Connection as _;

use super::*;

#[tokio::test]
async fn the_first_error_stops_the_action() {
    let db = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let id: u64 = sqlx::query_scalar("SELECT CONNECTION_ID()")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    let writer = Writer::open(&db, Mode::Primary).await.unwrap().unwrap();
    let mut write = writer.begin_on(&mut conn).await.unwrap();
    let mut steps = Steps::new(&mut write);
    // One closure type, so one instance of `run` takes every path.
    let blogname = || async |w: &mut Write<'_>| w.option("blogname").await;
    let name = steps.run(blogname()).await;
    assert_eq!(name.unwrap(), b"HyScribe Default Install");

    let mut killer = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    sqlx::query("KILL ?")
        .bind(id)
        .execute(&mut killer)
        .await
        .unwrap();
    assert_eq!(steps.run(blogname()).await, None);
    // Skipped, and the first error kept.
    assert_eq!(steps.run(blogname()).await, None);
    assert!(steps.finish(()).is_err());
}
