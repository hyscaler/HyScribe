// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up` (CI does).

use super::*;
use hyscribe_config::Secret;

/// The database `reference/default-install/db.sh` starts.
fn fixture_config() -> Database {
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());
    Database {
        host: env("HYSCRIBE_TEST_DB_HOST", "127.0.0.1"),
        port: env("HYSCRIBE_TEST_DB_PORT", "33061").parse().unwrap(),
        name: "wordpress".into(),
        user: "wordpress".into(),
        password: Secret::new("reference-only"),
        prefix: "wp_".into(),
        charset: "utf8mb4".into(),
    }
}

async fn fixture() -> Db {
    Db::connect(&fixture_config())
        .await
        .expect("no fixture database: run reference/default-install/db.sh up")
}

#[test]
fn prefixes_are_validated() {
    assert_eq!(
        Prefix::new("wp_")
            .unwrap()
            .sql("SELECT * FROM {prefix}posts")
            .0,
        "SELECT * FROM wp_posts"
    );
    assert!(Prefix::new("hs2_").is_some());
    for bad in ["wp-", "wp_;", "wp `x`", "é"] {
        assert_eq!(Prefix::new(bad), None, "{bad:?}");
    }
}

#[tokio::test]
async fn reads_the_default_install() {
    let db = fixture().await;
    assert_eq!(db.prefix(), &Prefix("wp_".into()));
    assert_eq!(
        db.option("blogname").await.unwrap(),
        Some(b"HyScribe Default Install".to_vec())
    );
    assert_eq!(db.option("no_such_option").await.unwrap(), None);
    let options = db.autoload_options().await.unwrap();
    assert!(
        options
            .iter()
            .any(|(name, value)| name == "template" && value == b"twentytwentyfive")
    );
    // Autoloaded serialized values come back byte for byte.
    let roles = options
        .iter()
        .find(|(name, _)| name == "wp_user_roles")
        .unwrap();
    assert!(roles.1.starts_with(b"a:5:{s:13:\"administrator\""));

    let hello = db.post(1).await.unwrap().unwrap();
    assert_eq!(hello.title, b"Hello world!");
    assert_eq!(hello.name, b"hello-world");
    assert_eq!(
        (hello.status.as_str(), hello.post_type.as_str()),
        ("publish", "post")
    );
    assert_eq!(
        (
            hello.author,
            hello.parent,
            hello.menu_order,
            hello.comment_count
        ),
        (1, 0, 0, 1)
    );
    assert_eq!(hello.date.len(), "2026-10-27 08:08:04".len());
    assert_eq!(hello.date_gmt.len(), hello.date.len());
    assert!(hello.content.starts_with(b"<!-- wp:paragraph -->"));
    assert_eq!(hello.excerpt, b"");
    assert_eq!(hello.guid, b"http://default-install.test/?p=1");
    assert_eq!(db.post(999).await.unwrap(), None);

    let titles =
        |posts: Vec<Post>| -> Vec<Vec<u8>> { posts.into_iter().map(|p| p.title).collect() };
    assert_eq!(
        titles(db.published("post", 10, 0).await.unwrap()),
        [b"Hello world!".to_vec()]
    );
    // The privacy policy page is a draft.
    assert_eq!(
        titles(db.published("page", 10, 0).await.unwrap()),
        [b"Sample Page".to_vec()]
    );
    assert!(db.published("post", 10, 1).await.unwrap().is_empty());
}

#[tokio::test]
async fn connections_are_read_only() {
    let db = fixture().await;
    let err =
        sqlx::query("UPDATE wp_options SET option_value = 'x' WHERE option_name = 'blogname'")
            .execute(&db.pool)
            .await
            .unwrap_err();
    assert!(err.to_string().contains("READ ONLY"), "{err}");
}

#[tokio::test]
async fn errors_are_reported() {
    // Every query fails against tables that do not exist.
    let mut config = fixture_config();
    config.prefix = "missing_".into();
    let db = Db::connect(&config).await.unwrap();
    for err in [
        db.autoload_options().await.unwrap_err(),
        db.option("blogname").await.unwrap_err(),
        db.post(1).await.unwrap_err(),
        db.published("post", 1, 0).await.unwrap_err(),
    ] {
        assert!(
            err.to_string().contains("missing_options")
                || err.to_string().contains("missing_posts"),
            "{err}"
        );
    }

    config.prefix = "bad-".into();
    let err = Db::connect(&config).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "the table prefix may only hold letters, digits and underscores"
    );

    let mut config = fixture_config();
    config.password = Secret::new("hunter2-wrong");
    let err = Db::connect(&config).await.unwrap_err();
    let message = err.to_string();
    assert!(message.contains("Access denied"), "{message}");
    assert!(!message.contains("hunter2"));
    let err: Box<dyn std::error::Error> = Box::new(err);
    assert!(err.source().is_none());
}

#[tokio::test]
async fn a_row_without_the_columns_fails_to_decode() {
    let db = fixture().await;
    let row = sqlx::query("SELECT 1 AS other")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(post(&row).is_err());
    let mut c = Columns::new(&row);
    let _ = (c.text("option_name"), c.bytes("option_value"));
    assert!(c.finish(()).is_err());
}
