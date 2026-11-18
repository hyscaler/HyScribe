// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// The scratch database loads, and a database that cannot take the
/// fixture, or cannot be reached, is an error.
#[tokio::test]
async fn the_default_install_loads_afresh() {
    let saves = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..crate::fixture_database()
    };
    load_default_install(&saves).await.unwrap();
    let read_only = hyscribe_config::Database {
        user: "wordpress_read".into(),
        name: "wordpress".into(),
        ..saves.clone()
    };
    assert!(load_default_install(&read_only).await.is_err());
    let closed = hyscribe_config::Database { port: 1, ..saves };
    assert!(load_default_install(&closed).await.is_err());
}

/// The same for the Theme Test Data, with the terms of one name told apart.
#[tokio::test]
async fn the_theme_test_data_loads_afresh() {
    let saves = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..crate::fixture_database()
    };
    let dump = std::fs::read_to_string(THEME_TEST_DATA).unwrap();
    load_theme_test_data(&saves, &dump).await.unwrap();
    let read_only = hyscribe_config::Database {
        user: "wordpress_read".into(),
        name: "wordpress".into(),
        ..saves.clone()
    };
    assert!(load_theme_test_data(&read_only, &dump).await.is_err());
    let closed = hyscribe_config::Database { port: 1, ..saves };
    assert!(load_theme_test_data(&closed, &dump).await.is_err());
}

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

/// A time from five seconds before the run, such as one a test read
/// before connecting, is the run's.
#[tokio::test]
async fn times_from_five_seconds_before_the_run_are_masked() {
    let mut conn = MySqlConnection::connect_with(&options(&crate::fixture_database()))
        .await
        .unwrap();
    let earlier = then(&[
        "UPDATE wp_posts SET post_modified = UTC_TIMESTAMP() - INTERVAL 5 SECOND, \
         comment_count = UNIX_TIMESTAMP() - 5 WHERE ID = 1",
    ]);
    let lines = changed_rows(&mut conn, steps(&[]), earlier).await.unwrap();
    let values: Vec<&[u8]> = lines
        .iter()
        .map(|line| line.rsplit(|&b| b == b'\t').next().unwrap())
        .collect();
    assert_eq!(values, [&b"%now%"[..], b"%unix%"]);
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
fn masks_password_hashes_and_transient_expiries() {
    let masks = Masks {
        start: (0, Vec::new()),
        end: (0, Vec::new()),
        new: NewIds::new(),
    };
    let users = &TABLES[9];
    let hash = "$wp$2y$12$AcR1MJvv/EqV00wDAwU9hOfvyPBz2LDs7JdSwTxTWj1XcAYchMKii";
    assert_eq!(
        masks.mask(users, "user_pass", hash.as_bytes()),
        b"%password%"
    );
    for kept in [
        &hash.replace("ii", "i"),
        &hash.replace('A', "!"),
        &hash.replace("$12$", "$10$"),
    ] {
        assert_eq!(
            masks.mask(users, "user_pass", kept.as_bytes()),
            kept.as_bytes()
        );
    }
    let options = &TABLES[8];
    let row = |name: &str| {
        let row: Row = vec![
            ("option_name".to_string(), name.as_bytes().to_vec()),
            ("option_value".to_string(), b"1794686138".to_vec()),
        ];
        BTreeMap::from([(b"k".to_vec(), row)])
    };
    let value = |name: &str, table: &Table| {
        let rows = masked(&masks, table, &row(name));
        rows.into_values().next().unwrap()[1].1.clone()
    };
    assert_eq!(value("_transient_timeout_a", options), b"%expiry%");
    assert_eq!(value("_site_transient_timeout_a", options), b"%expiry%");
    assert_eq!(value("_transient_a", options), b"1794686138");
    assert_eq!(value("_transient_timeout_a", users), b"1794686138");
}

#[test]
fn masks_times_and_new_ids() {
    let new = NewIds::from([(("wp_posts", b"12".to_vec()), b"%wp_posts:1%".to_vec())]);
    let masks = Masks {
        start: (1_793_999_995, b"2026-11-14 09:59:55".to_vec()),
        end: (1_794_000_002, b"2026-11-14 10:00:02".to_vec()),
        new,
    };
    let posts = &TABLES[0];
    let mask = |column: &str, value: &str| {
        String::from_utf8(masks.mask(posts, column, value.as_bytes())).unwrap()
    };
    assert_eq!(mask("post_date", "2026-11-14 09:59:55"), "%now%");
    assert_eq!(mask("post_date", "2026-11-14 10:00:02"), "%now%");
    assert_eq!(
        mask("post_date", "2026-11-14 10:00:03"),
        "2026-11-14 10:00:03"
    );
    assert_eq!(
        mask("post_date", "2026-11-14 09:59:54"),
        "2026-11-14 09:59:54"
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
            "1794000000:1 1794000002 1794000003 1793999995 1793999994 01794000001"
        ),
        "%unix%:1 %unix% 1794000003 %unix% 1793999994 01794000001"
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
fn masks_term_ids_in_term_maps() {
    let new = NewIds::from([
        (("wp_terms", b"2".to_vec()), b"%wp_terms:1%".to_vec()),
        (("wp_terms", b"3".to_vec()), b"%wp_terms:2%".to_vec()),
    ]);
    let masks = Masks {
        start: (0, Vec::new()),
        end: (0, Vec::new()),
        new,
    };
    let (terms, options) = (&TABLES[4], &TABLES[8]);
    let mask = |table, column: &str, value: &str| {
        String::from_utf8(masks.mask(table, column, value.as_bytes())).unwrap()
    };
    assert_eq!(mask(terms, "slug", "2"), "%wp_terms:1%");
    assert_eq!(mask(terms, "slug", "kid-2"), "kid-2");
    let map = mask(
        options,
        "option_value",
        "a:2:{i:3;a:1:{i:0;i:2;}i:2;a:0:{}}",
    );
    assert_eq!(
        map,
        "a:2:{i:%wp_terms:2%;a:1:{i:0;i:%wp_terms:1%;}i:%wp_terms:1%;a:0:{}}"
    );
    assert_eq!(
        mask(options, "option_value", "a:1:{i:4;a:1:{i:2;i:5;}}"),
        "a:1:{i:4;a:1:{i:2;i:5;}}"
    );
    // Other values keep their digits.
    for value in [
        "a:1:{i:0;i:2;}",
        "a:1:{s:1:\"x\";a:0:{}}",
        "a:1:{i:2;a:1:{i:0;s:1:\"2\";}}",
        "a:1:{i:2;a:1:{s:1:\"k\";i:2;}}",
        "i:2;",
        "2",
    ] {
        assert_eq!(mask(options, "option_value", value), value);
    }
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

/// A reset key's fast hash is `$generic$` and the 40 characters after it,
/// as `reference/writes/run.php` masks it.
#[test]
fn fast_hashes_are_masked() {
    let key = format!("1:$generic${}tail", "a".repeat(40));
    assert_eq!(fast_hashes(key.as_bytes()), b"1:%fasthash%tail");
    assert_eq!(fast_hashes(b"$generic$short"), b"%fasthash%");
}

#[test]
fn masks_admin_email_keys_and_a_new_policy_page() {
    let new = NewIds::from([(("wp_posts", b"12".to_vec()), b"%wp_posts:1%".to_vec())]);
    let masks = Masks {
        start: (0, Vec::new()),
        end: (0, Vec::new()),
        new,
    };
    let value = |table: &Table, name: &str, value: &str| {
        let row: Row = vec![
            ("option_name".to_string(), name.as_bytes().to_vec()),
            ("option_value".to_string(), value.as_bytes().to_vec()),
        ];
        let rows = masked(&masks, table, &BTreeMap::from([(b"k".to_vec(), row)]));
        String::from_utf8(rows.into_values().next().unwrap()[1].1.clone()).unwrap()
    };
    let (options, users) = (&TABLES[8], &TABLES[9]);
    let key = "a:2:{s:4:\"hash\";s:32:\"0123456789abcdef0123456789abcdef\";s:1:\"x\";s:32:\"0123456789abcdef0123456789abcdeF\";}";
    assert_eq!(
        value(options, "adminhash", key),
        "a:2:{s:4:\"hash\";s:32:\"%hash%\";s:1:\"x\";s:32:\"0123456789abcdef0123456789abcdeF\";}"
    );
    let short = "s:32:\"0123456789abcdef0123456789abcdef";
    assert_eq!(value(options, "adminhash", short), short);
    assert_eq!(value(options, "other", key), key);
    assert_eq!(value(users, "adminhash", key), key);
    assert_eq!(
        value(options, "wp_page_for_privacy_policy", "12"),
        "%wp_posts:1%"
    );
    assert_eq!(value(options, "wp_page_for_privacy_policy", "3"), "3");
    assert_eq!(value(options, "page_on_front", "12"), "12");
}

#[test]
fn masks_the_times_events_are_rescheduled_to() {
    let masks = Masks {
        start: (1_793_999_995, Vec::new()),
        end: (1_794_000_002, Vec::new()),
        new: NewIds::new(),
    };
    let value = |name: &str, value: &str| {
        let row: Row = vec![
            ("option_name".to_string(), name.as_bytes().to_vec()),
            ("option_value".to_string(), value.as_bytes().to_vec()),
        ];
        let rows = masked(&masks, &TABLES[8], &BTreeMap::from([(b"k".to_vec(), row)]));
        String::from_utf8(rows.into_values().next().unwrap()[1].1.clone()).unwrap()
    };
    let times = "i:1794003600;i:1794043202;i:1794086395;i:1794604800;\
                 i:1794003603;i:1794003594;i:3600;i:17940036000;";
    assert_eq!(
        value("cron", times),
        "i:%unix+3600%;i:%unix+43200%;i:%unix+86400%;i:%unix+604800%;\
         i:1794003603;i:1794003594;i:3600;i:17940036000;"
    );
    assert_eq!(value("other", "i:1794003600;"), "i:1794003600;");
}

/// An upload's folder of the month and its sizes in bytes are masked in
/// posts' GUIDs and meta values, and nowhere else.
#[test]
fn masks_upload_folders_and_sizes() {
    let masks = Masks {
        start: (0, b"2026-11-17 01:02:03".to_vec()),
        end: (0, b"2026-11-17 01:02:04".to_vec()),
        new: NewIds::new(),
    };
    let (posts, meta, options) = (&TABLES[0], &TABLES[1], &TABLES[8]);
    let guid = b"http://example.test/wp-content/uploads/2026/11/a.jpg";
    assert_eq!(
        masks.mask(posts, "guid", guid),
        b"http://example.test/wp-content/uploads/%ym%/a.jpg"
    );
    let stored =
        b"a:2:{s:4:\"file\";s:13:\"2026/11/a.jpg\";s:8:\"filesize\";i:4096;}s:8:\"filesize\";i:";
    assert_eq!(
        masks.mask(meta, "meta_value", stored),
        b"a:2:{s:4:\"file\";s:13:\"%ym%/a.jpg\";s:8:\"filesize\";i:%filesize%;}s:8:\"filesize\";i:%filesize%"
    );
    assert_eq!(masks.mask(posts, "post_name", b"2026/11/"), b"2026/11/");
    assert_eq!(
        masks.mask(options, "option_value", b"2026/11/"),
        b"2026/11/"
    );
}

/// A new ID is masked as a whole number only: the same digits in a dotted
/// number, such as the site's address, are left.
#[test]
fn ids_are_masked_as_whole_numbers() {
    let new = NewIds::from([
        (("wp_posts", b"127".to_vec()), b"%wp_posts:1%".to_vec()),
        (("wp_posts", b"1".to_vec()), b"%wp_posts:2%".to_vec()),
    ]);
    let masks = Masks {
        start: (0, Vec::new()),
        end: (0, Vec::new()),
        new,
    };
    let posts = &TABLES[0];
    let guid = |id: &str| format!("http://127.0.0.1:8081/?p={id}");
    let mask = |value: &str| masks.mask(posts, "guid", value.as_bytes());
    assert_eq!(mask(&guid("127")), guid("%wp_posts:1%").into_bytes());
    assert_eq!(mask(&guid("1")), guid("%wp_posts:2%").into_bytes());
    assert_eq!(mask("1.127 127. .1"), b"1.127 %wp_posts:1%. .%wp_posts:2%");
}
