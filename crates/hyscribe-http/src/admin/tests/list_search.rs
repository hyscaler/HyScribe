// SPDX-License-Identifier: AGPL-3.0-or-later

//! The list screens' search forms as a browser submits them, every field
//! the form renders and no action chosen, on `wordpress_saves`, as
//! `reference/list-search/golden.sh` recorded PHP's answers.

use sqlx::{Connection as _, Executor as _};

use super::*;

/// `wordpress_saves` loaded with the fixture, user 1 as `role` as
/// `reference/admin-menu/as-role.php` makes them.
async fn loaded(role: &str) {
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
    let sql = format!(
        "UPDATE wp_usermeta SET meta_value = 'a:1:{{s:{}:\"{role}\";b:1;}}' WHERE user_id = 1 AND meta_key = 'wp_capabilities'",
        role.len()
    );
    conn.execute(sqlx::raw_sql(sqlx::AssertSqlSafe(sql)))
        .await
        .unwrap();
}

/// `page` without its footer, whose text is the look's.
fn unfooted(page: &str) -> String {
    let Some((head, tail)) = page.split_once("<div id=\"wpfooter\"") else {
        return page.to_string();
    };
    let (_, tail) = tail.split_once("<div class=\"clear\"></div>").unwrap();
    format!("{head}{tail}")
}

/// A recorded request under hs-admin's URLs, as the browser would send it.
fn hs_uri(recorded: &str) -> String {
    let uri = hyscribe_testkit::hs_admin_page(&format!("/wp-admin/{recorded}"));
    uri.replace("%2Fwp-admin%2F", "%2Fhs-admin%2F")
        .replace(".php%3F", "%3F")
        .replace(".php&", "&")
}

/// Each role's search forms are PHP's: the redirects that drop the nonce and
/// the referer, the lists they lead to and the refusals.
#[tokio::test(flavor = "multi_thread")]
async fn list_searches_are_php_s() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../../tests/golden/list-search.tsv");
    let mut compared = 0;
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        loaded(role).await;
        let (site, cookies) = logged_in("wordpress_saves", dir.path()).await;
        for line in golden.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f[1] != role {
                continue;
            }
            let uri = hs_uri(f[0]);
            let response = get(&site, &uri, "127.0.0.1:8081", &cookies).await;
            let (status, php) = (f[2], f[3]);
            compared += 1;
            let ours = response.status().as_u16().to_string();
            assert_eq!(ours, status, "{role} {uri}");
            if status == "302" {
                let expected = hyscribe_testkit::hs_admin_page(php);
                assert_eq!(location(&response), expected, "{role} {uri}");
                continue;
            }
            let page = page_of(response).await;
            if php == "-" {
                continue;
            }
            let php = String::from_utf8(hyscribe_testkit::unhex(php)).unwrap();
            let php = hyscribe_testkit::settled_page(&hyscribe_testkit::hs_admin_page(&php));
            let page = hyscribe_testkit::settled_page(&page);
            let (php, page) = (unfooted(&php), unfooted(&page));
            let php = with_values(&php, &page);
            let at = php
                .bytes()
                .zip(page.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let near = |text: &str| {
                text.get(at.saturating_sub(150)..(at + 250).min(text.len()))
                    .unwrap_or_default()
                    .to_string()
            };
            assert!(
                php == page,
                "{role} {uri}
php:  {:?}
ours: {:?}",
                near(&php),
                near(&page)
            );
        }
    }
    assert_eq!(compared, golden.lines().count());
}
