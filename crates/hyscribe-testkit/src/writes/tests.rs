// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

async fn run(conn: &mut MySqlConnection, sql: &'static str) -> Result<(), String> {
    sqlx::raw_sql(sql)
        .execute(conn)
        .await
        .map(drop)
        .map_err(sql_error)
}

async fn run_with(conn: &mut MySqlConnection, sql: &'static str, id: u64) -> Result<(), String> {
    let done = sqlx::query(sql).bind(id).execute(conn).await;
    done.map(drop).map_err(sql_error)
}

/// A setup running `sql`. Its type, and [`then`]'s, is the same for every
/// `sql`, so one instance of the generic helpers sees every path.
fn steps(
    sql: &'static [&'static str],
) -> impl AsyncFnOnce(&mut MySqlConnection) -> Result<(), String> {
    async move |conn: &mut MySqlConnection| {
        for sql in sql {
            run(conn, sql).await?;
        }
        Ok(())
    }
}

/// An action running `sql`.
fn then(
    sql: &'static [&'static str],
) -> impl AsyncFnOnce(&mut MySqlConnection, ()) -> Result<(), String> {
    async move |conn: &mut MySqlConnection, ()| steps(sql)(conn).await
}

const RENAME: &[&str] =
    &["UPDATE wp_options SET option_value = 'A renamed site' WHERE option_name = 'blogname'"];

const POST: &str = "INSERT INTO wp_posts (post_author, post_date, post_date_gmt, post_content, \
    post_title, post_excerpt, post_status, comment_status, ping_status, post_password, post_name, \
    to_ping, pinged, post_modified, post_modified_gmt, post_content_filtered, post_parent, guid, \
    menu_order, post_type, post_mime_type, comment_count) VALUES (1, UTC_TIMESTAMP(), \
    UTC_TIMESTAMP(), 'Its content.', 'A write oracle post', '', ?, ?, ?, '', ?, '', '', \
    UTC_TIMESTAMP(), UTC_TIMESTAMP(), '', ?, '', 0, ?, '', 0)";

/// PHP's `wp_insert_post()` of a post (or of a revision of `parent`) in raw SQL.
async fn insert_post(conn: &mut MySqlConnection, parent: u64) -> Result<u64, String> {
    let (status, open, name, kind) = match parent {
        0 => ("publish", "open", "a-write-oracle-post".to_string(), "post"),
        _ => (
            "inherit",
            "closed",
            format!("{parent}-revision-v1"),
            "revision",
        ),
    };
    let done = sqlx::query(POST)
        .bind(status)
        .bind(open)
        .bind(open)
        .bind(name)
        .bind(parent)
        .bind(kind)
        .execute(&mut *conn)
        .await
        .map_err(sql_error)?;
    let id = done.last_insert_id();
    run_with(
        conn,
        "UPDATE wp_posts SET guid = CONCAT('http://127.0.0.1:8081/?p=', ID) WHERE ID = ?",
        id,
    )
    .await?;
    Ok(id)
}

#[tokio::test]
async fn raw_writes_match_php_goldens() {
    check_writes(
        &crate::fixture_database(),
        "update-option",
        steps(&[]),
        then(RENAME),
    )
    .await
    .unwrap();
    check_writes(
        &crate::fixture_database(),
        "insert-category",
        steps(&[]),
        then(&[
            "INSERT INTO wp_terms (name, slug, term_group) VALUES ('News', 'news', 0)",
            "INSERT INTO wp_term_taxonomy (term_id, taxonomy, description, parent, count) \
             VALUES (LAST_INSERT_ID(), 'category', 'What is new.', 0, 0)",
            "INSERT INTO wp_options (option_name, option_value, autoload) \
             VALUES ('category_children', 'a:0:{}', 'auto')",
        ]),
    )
    .await
    .unwrap();
    // The setup's rows are new too: their IDs come first.
    let setup = async |conn: &mut MySqlConnection| {
        let id = insert_post(conn, 0).await?;
        for sql in [
            "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (?, '_pingme', '1')",
            "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) VALUES (?, '_encloseme', '1')",
            "INSERT INTO wp_term_relationships (object_id, term_taxonomy_id) VALUES (?, 1)",
        ] {
            run_with(conn, sql, id).await?;
        }
        run(
            conn,
            "UPDATE wp_term_taxonomy SET count = 2 WHERE term_taxonomy_id = 1",
        )
        .await?;
        Ok(id)
    };
    check_writes(
        &crate::fixture_database(),
        "trash-post",
        setup,
        async |conn, id| {
            for sql in [
                "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) \
             VALUES (?, '_wp_trash_meta_status', 'publish')",
                "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) \
             VALUES (?, '_wp_trash_meta_time', UNIX_TIMESTAMP())",
                "INSERT INTO wp_postmeta (post_id, meta_key, meta_value) \
             VALUES (?, '_wp_desired_post_slug', 'a-write-oracle-post')",
                "UPDATE wp_posts SET post_status = 'trash', \
             post_name = 'a-write-oracle-post__trashed' WHERE ID = ?",
            ] {
                run_with(conn, sql, id).await?;
            }
            insert_post(conn, id).await?;
            run(
                conn,
                "UPDATE wp_term_taxonomy SET count = 1 WHERE term_taxonomy_id = 1",
            )
            .await
        },
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn reports_differences_by_table_and_column() {
    let wrong = then(&[
        "UPDATE wp_options SET option_value = 'Wrong\\tname' WHERE option_name = 'blogname'",
        "DELETE FROM wp_term_relationships WHERE object_id = 1",
    ]);
    let report = check_writes(
        &crate::fixture_database(),
        "update-option",
        steps(&[]),
        wrong,
    )
    .await
    .unwrap_err();
    assert_eq!(
        report,
        "update-option:\n\
         wp_options option_value (~ blogname): PHP \"HyScribe Default Install -> A renamed site\", \
         HyScribe \"HyScribe Default Install -> Wrong\\\\tname\"\n\
         wp_term_relationships object_id (- 1,1): PHP nothing, HyScribe \"1\"\n\
         wp_term_relationships term_order (- 1,1): PHP nothing, HyScribe \"0\"\n\
         wp_term_relationships term_taxonomy_id (- 1,1): PHP nothing, HyScribe \"1\""
    );
    // Nothing was kept: the same action still matches.
    check_writes(
        &crate::fixture_database(),
        "update-option",
        steps(&[]),
        then(RENAME),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn reports_failures() {
    let missing = check_writes(
        &crate::fixture_database(),
        "no-such-action",
        steps(&[]),
        then(RENAME),
    )
    .await
    .unwrap_err();
    assert!(missing.contains("no-such-action.tsv"), "{missing}");
    let failed = check_writes(
        &crate::fixture_database(),
        "update-option",
        steps(&[]),
        then(&["UPDATE wp_no_such_table SET x = 1"]),
    )
    .await
    .unwrap_err();
    assert!(failed.contains("wp_no_such_table"), "{failed}");
    let setup = check_writes(
        &crate::fixture_database(),
        "update-option",
        steps(&["SELECT * FROM wp_no_such_setup"]),
        then(RENAME),
    )
    .await;
    let setup = setup.unwrap_err();
    assert!(setup.contains("wp_no_such_setup"), "{setup}");
    let mut closed = crate::fixture_database();
    closed.port = 1;
    let refused = check_writes(&closed, "update-option", steps(&[]), then(RENAME));
    let refused = refused.await.unwrap_err();
    assert!(refused.starts_with(crate::NO_FIXTURE_DATABASE), "{refused}");
    let mut conn = MySqlConnection::connect_with(&options(&crate::fixture_database()))
        .await
        .unwrap();
    // Killing its own connection fails the statement too.
    run(&mut conn, "KILL CONNECTION_ID()").await.unwrap_err();
    let gone = changed_rows(&mut conn, steps(&[]), then(RENAME)).await;
    assert!(gone.is_err(), "{gone:?}");
}

#[tokio::test]
async fn reports_database_errors_after_the_start() {
    // The setup succeeds with the connection gone, so the snapshot after it fails.
    let kill = async |conn: &mut MySqlConnection| {
        drop(run(conn, "KILL CONNECTION_ID()").await);
        Ok(())
    };
    let mut conn = MySqlConnection::connect_with(&options(&crate::fixture_database()))
        .await
        .unwrap();
    let report = changed_rows(&mut conn, kill, async |_, ()| Ok(())).await;
    assert!(report.is_err(), "{report:?}");
}

#[test]
fn masks_times_and_new_ids() {
    let new = NewIds::from([(("wp_posts", b"12".to_vec()), b"%wp_posts:1%".to_vec())]);
    let masks = Masks {
        start: (1_794_000_000, b"2026-11-14 10:00:00".to_vec()),
        end: (1_794_000_002, b"2026-11-14 10:00:02".to_vec()),
        new,
    };
    let posts = &TABLES[0];
    let mask = |column: &str, value: &str| {
        String::from_utf8(masks.mask(posts, column, value.as_bytes())).unwrap()
    };
    assert_eq!(mask("post_date", "2026-11-14 10:00:00"), "%now%");
    assert_eq!(mask("post_date", "2026-11-14 10:00:02"), "%now%");
    assert_eq!(
        mask("post_date", "2026-11-14 10:00:03"),
        "2026-11-14 10:00:03"
    );
    assert_eq!(
        mask("post_date", "2026-11-14 09:59:59"),
        "2026-11-14 09:59:59"
    );
    assert_eq!(
        mask("post_date", "2026-11-14 10:00:011"),
        "2026-11-14 10:00:011"
    );
    assert_eq!(
        mask("post_date", "2026-11-14T10:00:01"),
        "2026-11-14T10:00:01"
    );
    assert_eq!(
        mask("post_date", "2026/11/14 10:00:01"),
        "2026/11/14 10:00:01"
    );
    assert_eq!(
        mask("post_date", "2026-11-14 10-00:01"),
        "2026-11-14 10-00:01"
    );
    assert_eq!(
        mask("post_date", "2026-11-14 1a:00:01"),
        "2026-11-14 1a:00:01"
    );
    assert_eq!(
        mask(
            "meta",
            "1794000000:1 1794000002 1794000003 1793999999 01794000001"
        ),
        "%unix%:1 %unix% 1794000003 1793999999 01794000001"
    );
    assert_eq!(mask("ID", "12"), "%wp_posts:1%");
    assert_eq!(mask("ID", "120"), "120");
    assert_eq!(mask("post_parent", "3"), "3");
    assert_eq!(mask("post_author", "12"), "12");
    assert_eq!(mask("post_content", "12"), "12");
    assert_eq!(
        mask("guid", "http://h:8081/?p=12"),
        "http://h:8081/?p=%wp_posts:1%"
    );
    assert_eq!(
        mask("post_name", "12-revision-v1"),
        "%wp_posts:1%-revision-v1"
    );
    assert_eq!(mask("post_name", "a-12b"), "a-%wp_posts:1%b");
}

#[test]
fn escapes_as_php_does() {
    assert_eq!(escape(None), b"\\N");
    assert_eq!(escape(Some(b"a\\b\tc\nd\re")), b"a\\\\b\\tc\\nd\\re");
    assert_eq!(escape(Some(b"\\N")), b"\\\\N");
}

#[test]
fn compares_lines_with_golden() {
    let golden = b"# comment\n\nt\tk\t+\tc\tv\nt\tk\t~\td\ta\tb\n";
    let ours = [b"t\tk\t+\tc\tv".to_vec(), b"t\tk\t~\td\ta\tb".to_vec()];
    assert!(differences(golden, &ours).is_empty());
    assert_eq!(
        differences(golden, &ours[..1]),
        ["t d (~ k): PHP \"a -> b\", HyScribe nothing"]
    );
    assert_eq!(
        differences(b"t\tk\t+\tc\tw", &ours[..1]),
        ["t c (+ k): PHP \"w\", HyScribe \"v\""]
    );
    assert_eq!(
        differences(b"short", &[]),
        ["short  ( ): PHP \"\", HyScribe nothing"]
    );
}
