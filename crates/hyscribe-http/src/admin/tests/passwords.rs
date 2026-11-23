// SPDX-License-Identifier: AGPL-3.0-or-later

//! Profile and Edit User over HTTPS with application passwords stored, on
//! `wordpress_saves`: the list is the one `reference/profile/golden.sh`
//! recorded; passwords PHP would repair are not ported.

use sqlx::{Connection as _, Executor as _};

use super::*;

/// An HTTPS site over `wordpress_saves` loaded with the fixture and `sql`.
async fn loaded(sql: &str) -> Arc<Site> {
    let db = hyscribe_config::Database {
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
    conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string())))
        .await
        .unwrap();
    let url = "https://127.0.0.1:8081";
    let db = hyscribe_db::Db::connect(&db).await.unwrap();
    Arc::new(
        Site::load(db, Some(url.into()), Some(url.into()))
            .await
            .unwrap(),
    )
}

/// The administrator's `path` over HTTPS: its status and page, `None` for
/// what is not ported.
async fn served(site: &Arc<Site>, path: &'static str) -> Option<(StatusCode, String)> {
    let (site, handle) = (Arc::clone(site), tokio::runtime::Handle::current());
    let response = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let user = hyscribe_model::User {
            id: 1,
            ..hyscribe_model::User::default()
        };
        let admin = Admin::new(&reads, &site, user);
        let file = path.split_once('?').map_or(path, |(file, _)| file);
        let url = format!("https://127.0.0.1:8081{path}");
        screen(
            &admin,
            file,
            path,
            &url,
            "https://127.0.0.1:8081",
            &Browser::default(),
        )
    })
    .await
    .unwrap()?;
    let status = response.status();
    Some((status, page_of(response).await))
}

/// The meta of user 1 holding `passwords`, serialized.
fn meta(passwords: &str) -> String {
    format!(
        "INSERT INTO wp_usermeta (user_id, meta_key, meta_value) VALUES (1, '_application_passwords', '{passwords}')"
    )
}

/// Passwords are listed over HTTPS on both screens; those PHP would write
/// back are not ported.
#[tokio::test(flavor = "multi_thread")]
async fn passwords_are_listed_over_https() {
    let _saves = SAVES.lock().await;
    let item = "a:1:{i:0;a:7:{s:4:\"uuid\";s:36:\"77777777-7777-4777-8777-777777777777\";s:6:\"app_id\";s:0:\"\";s:4:\"name\";s:9:\"Admin app\";s:8:\"password\";s:1:\"z\";s:7:\"created\";i:1700000000;s:9:\"last_used\";N;s:7:\"last_ip\";N;}}";
    let site = loaded(&meta(item)).await;
    for path in ["/hs-admin/profile", "/hs-admin/user-edit?user_id=1"] {
        let (status, page) = served(&site, path).await.unwrap();
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(
            page.contains("<tr data-uuid=\"77777777-7777-4777-8777-777777777777\"><th class='name column-name has-row-actions column-primary' data-colname=\"Name\" scope=\"row\">Admin app<"),
            "{path}"
        );
        assert!(page.contains("November 14, 2023"), "{path}");
        assert!(
            page.contains("id=\"tmpl-application-password-row\""),
            "{path}"
        );
    }
    let nameless = "a:1:{i:0;a:1:{s:4:\"name\";s:1:\"n\";}}";
    let site = loaded(&meta(nameless)).await;
    for path in ["/hs-admin/profile", "/hs-admin/user-edit?user_id=1"] {
        assert!(served(&site, path).await.is_none(), "{path}");
    }
}
