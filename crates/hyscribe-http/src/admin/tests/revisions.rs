// SPDX-License-Identifier: AGPL-3.0-or-later

//! The revisions screen and its restore over `reference/revisions/content.sql`,
//! on `wordpress_saves`, as `reference/revisions/golden.sh` recorded PHP's.

use sqlx::{Connection as _, Executor as _};

use super::*;

/// `wordpress_saves` loaded with the fixture, the revisions' content and
/// `sql`.
async fn loaded(sql: &str) {
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
    let content = include_str!("../../../../../reference/revisions/content.sql");
    for statement in [content, sql].into_iter().filter(|sql| !sql.is_empty()) {
        let statement = sqlx::AssertSqlSafe(statement.to_string());
        conn.execute(sqlx::raw_sql(statement)).await.unwrap();
    }
}

/// User 1 as `role`, as `reference/admin-menu/as-role.php` makes them.
fn as_role(role: &str) -> String {
    format!(
        "UPDATE wp_usermeta SET meta_value = 'a:1:{{s:{}:\"{role}\";b:1;}}' WHERE user_id = 1 AND meta_key = 'wp_capabilities'",
        role.len()
    )
}

/// `page` with the time since each revision as the golden masks it.
fn untimed(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(at) = rest.find("\"timeAgo\":\"") {
        let (head, tail) = rest.split_at(at + 11);
        out.push_str(head);
        out.push_str("%time_ago%");
        rest = tail.get(tail.find('"').unwrap()..).unwrap();
    }
    out.push_str(rest);
    out
}

/// What hs-admin does not port: an argument given as an array, a synced
/// pattern's revisions, whose type may turn them off (their restore goes
/// on to a write, here shadow mode's refusal), a revision by a user gone,
/// and a draft with only autosaves, whose current PHP gets wrong.
const UNPORTED: [&str; 4] = [
    "revision[]=1303",
    "revision=1324",
    "revision=1326",
    "revision=1328",
];

/// What hs-admin answers `query` of the screen, as the golden keeps it.
async fn answered(site: &Arc<Site>, cookies: &str, query: &str) -> String {
    let uri = format!("/hs-admin/revision?{query}");
    let response = get(site, uri.trim_end_matches('?'), "127.0.0.1:8081", cookies).await;
    match response.status() {
        StatusCode::FOUND => format!("302\t{}", location(&response)),
        StatusCode::OK => untimed(&page_of(response).await),
        status => format!("{}\t{}", status.as_u16(), page_of(response).await),
    }
}

/// Each role's revisions screens and refusals are PHP's; a restore whose
/// checks pass answers as shadow mode answers a write.
#[tokio::test(flavor = "multi_thread")]
async fn the_revisions_screen_is_php_s() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/revisions.tsv");
    let mut compared = 0;
    for role in [
        "administrator",
        "editor",
        "author",
        "contributor",
        "subscriber",
    ] {
        loaded(&as_role(role)).await;
        let (site, cookies) = logged_in("wordpress_saves", dir.path()).await;
        let rows = golden.lines().filter(|line| !line.contains("_wpnonce="));
        for line in rows.filter(|line| line.split('\t').nth(1) == Some(role)) {
            let f: Vec<&str> = line.split('\t').collect();
            let query = f[0].trim_start_matches("revision.php?");
            let php = String::from_utf8(hyscribe_testkit::unhex(f[2])).unwrap();
            let php = hyscribe_testkit::hs_admin_page(&php).replace("302\tedit.php", "302\tedit");
            let ours = answered(&site, &cookies, query).await;
            compared += 1;
            if UNPORTED.contains(&query) {
                assert!(ours.starts_with("501\t"), "{query}");
                continue;
            }
            if query == "action=restore&revision=1324" {
                assert!(
                    ours.starts_with("403\t") && ours.contains(READ_ONLY),
                    "{query}"
                );
                continue;
            }
            let php = with_values(&php, &ours);
            let at = php
                .bytes()
                .zip(ours.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            let near = |text: &str| {
                text.get(at.saturating_sub(150)..(at + 250).min(text.len()))
                    .unwrap_or_default()
                    .to_string()
            };
            assert!(
                php == ours,
                "{role} {query}\nphp:  {:?}\nours: {:?}",
                near(&php),
                near(&ours)
            );
        }
    }
    assert_eq!(compared, 60);
}

/// A restore with its nonce writes, as PHP's does, then sends the user to
/// the post's editor; without a writer, it is shadow mode's refusal.
#[tokio::test(flavor = "multi_thread")]
async fn a_restore_is_php_s() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/revisions.tsv");
    let row = golden
        .lines()
        .find(|line| line.contains("_wpnonce="))
        .unwrap();
    let php = String::from_utf8(hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap())).unwrap();
    loaded("").await;
    let (site, cookies) = logged_in("wordpress_saves", dir.path()).await;
    let nonce = session_nonce(&site, &cookies, "restore-post_1303").await;
    let query = format!("action=restore&revision=1303&_wpnonce={nonce}");
    let refused = answered(&site, &cookies, &query).await;
    assert!(
        refused.starts_with("403\t") && refused.contains(READ_ONLY),
        "{refused}"
    );
    let config = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    };
    let writer = hyscribe_db::Writer::open(&config, hyscribe_config::Mode::Primary).await;
    let site = Arc::new(
        Arc::try_unwrap(site)
            .ok()
            .unwrap()
            .with_writer(writer.unwrap()),
    );
    let restored = answered(&site, &cookies, &query).await;
    assert_eq!(restored, hyscribe_testkit::hs_admin_page(&php));
}
