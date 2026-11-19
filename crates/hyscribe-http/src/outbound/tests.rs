// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_config::Mode;
use hyscribe_db::Writer;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;

use super::*;

/// The cell of `column` of the new post in the golden of a write oracle
/// `case`, unescaped as the oracle escapes its values.
fn golden(case: &str, column: &str) -> Vec<u8> {
    let path = format!(
        "{}/../hyscribe-testkit/tests/golden/writes/{case}.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden = std::fs::read_to_string(path).unwrap();
    let wanted = format!("\t+\t{column}\t");
    let line = golden
        .lines()
        .find(|line| line.starts_with("wp_posts\t") && line.contains(&wanted))
        .unwrap();
    let mut unescaped = String::new();
    let mut chars = line.split_once(&wanted).unwrap().1.chars();
    while let Some(c) = chars.next() {
        unescaped.push(match c {
            '\\' => match chars.next() {
                Some('t') => '\t',
                Some('n') => '\n',
                Some(other) => other,
                None => c,
            },
            _ => c,
        });
    }
    unescaped.into_bytes()
}

/// The `wp_navigation` post a visitor's page view makes of a classic menu,
/// or of the page list when there is none, is written row for row as PHP
/// writes it (`navigation-fallback-*` of the write oracle), by no user.
#[tokio::test(flavor = "multi_thread")]
async fn the_navigation_fallback_is_written_as_php_writes_it() {
    let db = fixture_database();
    let writer = Writer::open(&db, Mode::Primary).await.unwrap().unwrap();
    let site = crate::tests::oracle_site().await;
    let ctx = visitor(&site);
    let cases = [
        "navigation-fallback-classic",
        "navigation-fallback-default",
        "navigation-fallback-objects",
        "navigation-fallback-escapes",
        "navigation-fallback-primary-location",
        "navigation-fallback-primary-slug",
        "navigation-fallback-slug",
    ];
    for case in cases {
        let (title, name, content) = (
            golden(case, "post_title"),
            golden(case, "post_name"),
            golden(case, "post_content"),
        );
        // The oracle's setup made the menu and its items, as posts of their own.
        let setup = async |conn: &mut hyscribe_db::Connection| {
            let row = "INSERT INTO wp_posts (post_author, post_content, post_title, post_excerpt, \
                post_status, post_name, post_type, to_ping, pinged, post_content_filtered) \
                VALUES (1, '', '', '', 'publish', '', 'nav_menu_item', '', '', '')";
            for _ in 1..golden_new_posts(case) {
                let inserted = sqlx::query(row).execute(&mut *conn).await;
                inserted.map_err(|err| err.to_string())?;
            }
            Ok(())
        };
        let action = async |conn: &mut hyscribe_db::Connection, ()| {
            let mut write = writer
                .begin_on(conn)
                .await
                .map_err(|err| format!("{err:?}"))?;
            let post = [&title[..], &name[..], &content[..]];
            insert_navigation(&mut write, &ctx, post)
                .await
                .map_err(|err| format!("{err:?}"))?;
            write.commit().await.map_err(|err| format!("{err:?}"))
        };
        check_writes(&db, case, setup, action).await.unwrap();
    }
}

/// How many posts the oracle's action's post follows: the number its
/// header names, as `%wp_posts:n%`.
fn golden_new_posts(case: &str) -> usize {
    let path = format!(
        "{}/../hyscribe-testkit/tests/golden/writes/{case}.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden = std::fs::read_to_string(path).unwrap();
    golden.matches("# %wp_posts:").count()
}
