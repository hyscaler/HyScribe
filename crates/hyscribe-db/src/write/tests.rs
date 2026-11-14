// SPDX-License-Identifier: AGPL-3.0-or-later

//! The writer against the default install fixture: every action is rolled
//! back, so the read tests keep seeing the fixture as built.

use super::*;
use crate::Db;
use crate::dialect::WORDPRESS_SQL_MODE;
use crate::tests::fixture_config;
use hyscribe_config::Secret;

/// Write tests run one at a time, so no action waits on another's rows
/// and the engine test's table is never written meanwhile.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn writer() -> Writer {
    Writer::open(&fixture_config(), Mode::Primary)
        .await
        .unwrap()
        .unwrap()
}

/// The JSON array `sql` selects for `id`, read inside the action.
async fn json(write: &mut Write<'_>, sql: &'static str, id: u64) -> String {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(&mut *write.tx)
        .await
        .unwrap()
}

fn sorted(mut ids: Vec<u64>) -> Vec<u64> {
    ids.sort_unstable();
    ids
}

const POST: &str = "SELECT JSON_ARRAY(post_author, post_date, post_date_gmt, post_content, \
     post_content_filtered, post_title, post_excerpt, post_status, post_type, comment_status, \
     ping_status, post_password, post_name, to_ping, pinged, post_modified, post_modified_gmt, \
     post_parent, menu_order, post_mime_type, guid) FROM wp_posts WHERE ID = ?";

const POST_STATE: &str =
    "SELECT JSON_ARRAY(post_name, guid, post_status, comment_count) FROM wp_posts WHERE ID = ?";

/// The fixture's first post, as PHP wrote it.
fn hello() -> PostFields {
    PostFields {
        author: 1,
        date: "2026-10-27 08:08:15".into(),
        date_gmt: "2026-10-27 08:08:15".into(),
        content: b"<!-- wp:paragraph -->\n<p>Welcome to WordPress. This is your first post. \
            Edit or delete it, then start writing!</p>\n<!-- /wp:paragraph -->"
            .to_vec(),
        content_filtered: Vec::new(),
        title: b"Hello world!".to_vec(),
        excerpt: Vec::new(),
        status: "publish".into(),
        post_type: "post".into(),
        comment_status: "open".into(),
        ping_status: "open".into(),
        password: Vec::new(),
        name: b"hello-world".to_vec(),
        to_ping: Vec::new(),
        pinged: Vec::new(),
        modified: "2026-10-27 08:08:15".into(),
        modified_gmt: "2026-10-27 08:08:15".into(),
        parent: 0,
        menu_order: 0,
        mime_type: String::new(),
        guid: b"http://default-install.test/?p=1".to_vec(),
    }
}

/// A post with a different value in each column.
fn draft() -> PostFields {
    PostFields {
        author: 2,
        date: "2026-11-01 10:00:01".into(),
        date_gmt: "0000-00-00 00:00:00".into(),
        content: "Café ✓".as_bytes().to_vec(),
        content_filtered: b"filtered".to_vec(),
        title: b"Title".to_vec(),
        excerpt: b"Excerpt".to_vec(),
        status: "draft".into(),
        post_type: "page".into(),
        comment_status: "closed".into(),
        ping_status: "closed-ping".into(),
        password: b"secret".to_vec(),
        name: b"name".to_vec(),
        to_ping: b"http://a.test".to_vec(),
        pinged: b"http://b.test".to_vec(),
        modified: "2026-11-02 10:00:02".into(),
        modified_gmt: "2026-11-02 05:00:03".into(),
        parent: 3,
        menu_order: -4,
        mime_type: "text/plain".into(),
        guid: b"http://default-install.test/?page_id=9".to_vec(),
    }
}

#[tokio::test]
async fn shadow_mode_has_no_writer() {
    // Not even a connection: an unreachable database does not matter.
    let config = Database {
        port: 1,
        ..fixture_config()
    };
    assert!(Writer::open(&config, Mode::Shadow).await.unwrap().is_none());
    let err = Writer::open(&config, Mode::Primary).await.err().unwrap();
    assert!(!err.to_string().contains("reference-only"), "{err}");
}

#[tokio::test]
async fn reads_go_through_the_read_user() {
    let read_only = Database {
        read_user: Some("wordpress_read".into()),
        read_password: Secret::new("reference-only"),
        ..fixture_config()
    };
    let current = "SELECT CURRENT_USER()";
    let db = Db::connect(&read_only).await.unwrap();
    assert_eq!(
        db.option("blogname").await.unwrap().unwrap(),
        b"HyScribe Default Install"
    );
    let user: String = sqlx::query_scalar(current)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(user, "wordpress_read@%");
    // Out of the read only session too, the read user cannot write.
    let mut conn = db.pool.acquire().await.unwrap().detach();
    sqlx::query("SET SESSION TRANSACTION READ WRITE")
        .execute(&mut conn)
        .await
        .unwrap();
    let delete = "DELETE FROM wp_options WHERE option_name = 'hyscribe_none'";
    let err = sqlx::query(delete).execute(&mut conn).await.unwrap_err();
    assert!(err.to_string().contains("command denied"), "{err}");

    // Without a read user, reads are the main user's, read only.
    let db = Db::connect(&fixture_config()).await.unwrap();
    let user: String = sqlx::query_scalar(current)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(user, "wordpress@%");
    let err = sqlx::query(delete).execute(&db.pool).await.unwrap_err();
    assert!(err.to_string().contains("READ ONLY"), "{err}");

    // The writer is the main user, whatever the read user.
    let writer = Writer::open(&read_only, Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let user: String = sqlx::query_scalar(current)
        .fetch_one(&writer.pool)
        .await
        .unwrap();
    assert_eq!(user, "wordpress@%");
}

/// `wpdb::set_sql_mode()`'s filter, for the expected values.
fn without_incompatible(modes: &str) -> String {
    const INCOMPATIBLE: [&str; 6] = [
        "NO_ZERO_DATE",
        "ONLY_FULL_GROUP_BY",
        "STRICT_TRANS_TABLES",
        "STRICT_ALL_TABLES",
        "TRADITIONAL",
        "ANSI",
    ];
    modes
        .split(',')
        .filter(|mode| !mode.is_empty() && !INCOMPATIBLE.contains(mode))
        .collect::<Vec<_>>()
        .join(",")
}

#[tokio::test]
async fn the_writer_takes_wordpress_sql_modes() {
    let modes = "SELECT @@SESSION.sql_mode";
    let db = Db::connect(&fixture_config()).await.unwrap();
    let server: String = sqlx::query_scalar(modes).fetch_one(&db.pool).await.unwrap();
    assert!(server.contains("STRICT_TRANS_TABLES"), "{server}");
    let writer = writer().await;
    let mut conn = writer.pool.acquire().await.unwrap().detach();
    let session: String = sqlx::query_scalar(modes)
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(session, without_incompatible(&server));

    for set in [
        "SET SESSION sql_mode = 'ANSI,TRADITIONAL,ONLY_FULL_GROUP_BY,NO_ZERO_IN_DATE'",
        "SET SESSION sql_mode = 'NO_ZERO_DATE,STRICT_ALL_TABLES'",
        "SET SESSION sql_mode = ''",
    ] {
        sqlx::query(set).execute(&mut conn).await.unwrap();
        let before: String = sqlx::query_scalar(modes)
            .fetch_one(&mut conn)
            .await
            .unwrap();
        sqlx::query(WORDPRESS_SQL_MODE)
            .execute(&mut conn)
            .await
            .unwrap();
        let after: String = sqlx::query_scalar(modes)
            .fetch_one(&mut conn)
            .await
            .unwrap();
        assert_eq!(after, without_incompatible(&before), "{before}");
    }
}

#[tokio::test]
async fn an_action_commits_or_rolls_back_as_a_whole() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let key = "hyscribe_commit_test";
    let mut write = writer.begin().await.unwrap();
    write.add_meta(Meta::Term, 1, key, b"a").await.unwrap();
    write.add_meta(Meta::Term, 1, key, b"b").await.unwrap();
    drop(write);
    let mut write = writer.begin().await.unwrap();
    assert!(
        write
            .meta_ids_of_key(Meta::Term, key)
            .await
            .unwrap()
            .is_empty()
    );

    let first = write.add_meta(Meta::Term, 1, key, b"a").await.unwrap();
    let second = write.add_meta(Meta::Term, 1, key, b"b").await.unwrap();
    write.commit().await.unwrap();
    let mut write = writer.begin().await.unwrap();
    let kept = sorted(write.meta_ids_of_key(Meta::Term, key).await.unwrap());
    assert_eq!(kept, [first, second]);
    // Leave the fixture as built.
    assert_eq!(write.delete_meta(Meta::Term, &kept).await.unwrap(), 2);
    write.commit().await.unwrap();
}

/// The write oracle's way: actions on a connection in a transaction the
/// caller never commits.
#[tokio::test]
async fn an_action_in_the_callers_transaction_is_rolled_back_with_it() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let key = "hyscribe_nested_test";
    let mut conn = writer.pool.acquire().await.unwrap();
    let mut outer = sqlx::Connection::begin(&mut *conn).await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    let kept = write.add_meta(Meta::Term, 1, key, b"kept").await.unwrap();
    write.commit().await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    write
        .add_meta(Meta::Term, 1, key, b"dropped")
        .await
        .unwrap();
    drop(write);
    // A committed action is seen in the caller's transaction, a dropped one
    // is not.
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    assert_eq!(
        write.meta_ids_of_key(Meta::Term, key).await.unwrap(),
        [kept]
    );
    drop(write);
    drop(outer);
    drop(conn);
    let mut write = writer.begin().await.unwrap();
    assert!(
        write
            .meta_ids_of_key(Meta::Term, key)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn writes_are_refused_unless_db_version_is_supported() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    assert!(writer.begin().await.is_ok());
    let set = "UPDATE wp_options SET option_value = ? WHERE option_name = 'db_version'";
    for (value, refused) in [
        ("61834", true),
        ("61832", true),
        ("70000", true),
        ("new", true),
        ("61833", false),
    ] {
        let mut tx = writer.pool.begin().await.unwrap();
        sqlx::query(set)
            .bind(value)
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = Write::start(&writer, Ok(tx)).await;
        assert_eq!(result.is_err(), refused, "{value}");
        if let Err(err) = result {
            assert_eq!(
                err.to_string(),
                format!(
                    "the database's db_version ({value}) is not one this release writes to \
                     (61833 to 61833), so HyScribe writes nothing to it"
                )
            );
        }
    }
    let mut tx = writer.pool.begin().await.unwrap();
    sqlx::query("DELETE FROM wp_options WHERE option_name = 'db_version'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let err = Write::start(&writer, Ok(tx)).await.err().unwrap();
    assert!(err.to_string().contains("(missing)"), "{err}");
    let err = Write::start(&writer, Err(sqlx::Error::PoolClosed))
        .await
        .err()
        .unwrap();
    assert!(err.to_string().contains("closed"), "{err}");
}

#[tokio::test]
async fn writes_to_a_myisam_table_are_refused() {
    let _one = ONE_AT_A_TIME.lock().await;
    let admin = writer().await;
    let engine = |engine: &'static str| sqlx::query(engine).execute(&admin.pool);
    engine("ALTER TABLE wp_links ENGINE=MyISAM").await.unwrap();
    let tried = async {
        let writer = writer().await;
        let mut write = writer.begin().await.unwrap();
        let links = write.reassign_links(1, 2).await.map(|_| ());
        let posts = write.set_post_status(1, "publish").await;
        (
            links.map_err(|err| err.to_string()),
            posts.map_err(|err| err.to_string()),
        )
    };
    let (links, posts) = tokio::spawn(tried)
        .await
        .unwrap_or_else(|_| (Err("the attempt panicked".into()), Err(String::new())));
    // Restore the fixture before anything is asserted.
    engine("ALTER TABLE wp_links ENGINE=InnoDB").await.unwrap();
    assert_eq!(
        links,
        Err(
            "wp_links uses the MyISAM engine, which cannot roll back, so HyScribe does not \
             write to it; convert it with ALTER TABLE wp_links ENGINE=InnoDB"
                .into()
        )
    );
    // The other tables still take writes.
    assert_eq!(posts, Ok(1));
}

#[tokio::test]
async fn writes_posts() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let id = write.insert_post(&hello()).await.unwrap();
    assert!(id > 3);
    // The row PHP wrote for the same post.
    assert_eq!(
        json(&mut write, POST, id).await,
        json(&mut write, POST, 1).await
    );

    assert_eq!(write.update_post(id, &draft()).await.unwrap(), 1);
    assert_eq!(
        json(&mut write, POST, id).await,
        r#"[2, "2026-11-01 10:00:01", "0000-00-00 00:00:00", "Café ✓", "filtered", "Title", "Excerpt", "draft", "page", "closed", "closed-ping", "secret", "name", "http://a.test", "http://b.test", "2026-11-02 10:00:02", "2026-11-02 05:00:03", 3, -4, "text/plain", "http://default-install.test/?page_id=9"]"#
    );
    assert_eq!(write.set_post_name(id, b"slug").await.unwrap(), 1);
    assert_eq!(write.set_post_guid(id, b"http://g.test/").await.unwrap(), 1);
    assert_eq!(write.set_post_status(id, "publish").await.unwrap(), 1);
    assert_eq!(write.set_comment_count(id, 5).await.unwrap(), 1);
    assert_eq!(
        json(&mut write, POST_STATE, id).await,
        r#"["slug", "http://g.test/", "publish", 5]"#
    );
    assert_eq!(write.update_post(999_999, &draft()).await.unwrap(), 0);

    let child = PostFields {
        parent: id,
        post_type: "revision".into(),
        ..hello()
    };
    let first = write.insert_post(&child).await.unwrap();
    let second = write.insert_post(&child).await.unwrap();
    assert_eq!(second, first + 1);
    let children = write.child_ids(id, "revision").await.unwrap();
    assert_eq!(sorted(children), [first, second]);
    assert!(write.child_ids(id, "page").await.unwrap().is_empty());
    assert_eq!(write.reparent_posts(id, "page", 7).await.unwrap(), 0);
    assert_eq!(write.reparent_posts(id, "revision", 7).await.unwrap(), 2);
    assert_eq!(
        sorted(write.child_ids(7, "revision").await.unwrap()),
        [first, second]
    );

    let mine = sorted(write.author_post_ids(1).await.unwrap());
    assert_eq!(mine, [1, 2, 3, first, second]);
    assert_eq!(write.reassign_posts(1, 6).await.unwrap(), 5);
    assert_eq!(sorted(write.author_post_ids(6).await.unwrap()), mine);
    assert!(write.author_post_ids(1).await.unwrap().is_empty());

    assert_eq!(write.delete_post(first).await.unwrap(), 1);
    assert_eq!(write.delete_post(first).await.unwrap(), 0);
    assert_eq!(write.child_ids(7, "revision").await.unwrap(), [second]);
}

const COMMENT: &str = "SELECT JSON_ARRAY(comment_post_ID, comment_author, \
     comment_author_email, comment_author_url, comment_author_IP, comment_date, comment_date_gmt, \
     comment_content, comment_karma, comment_approved, comment_agent, comment_type, \
     comment_parent, user_id) FROM wp_comments WHERE comment_ID = ?";

const APPROVED: &str = "SELECT JSON_ARRAY(comment_approved) FROM wp_comments WHERE comment_ID = ?";

/// The fixture's comment, as PHP wrote it.
fn commenter() -> CommentFields {
    CommentFields {
        post_id: 1,
        author: b"A WordPress Commenter".to_vec(),
        author_email: b"wapuu@wordpress.example".to_vec(),
        author_url: b"https://wordpress.org/".to_vec(),
        author_ip: Vec::new(),
        date: "2026-10-27 08:08:15".into(),
        date_gmt: "2026-10-27 08:08:15".into(),
        content: b"Hi, this is a comment.\nTo get started with moderating, editing, and deleting \
            comments, please visit the Comments screen in the dashboard.\nCommenter avatars come \
            from <a href=\"https://gravatar.com/\">Gravatar</a>."
            .to_vec(),
        karma: 0,
        approved: "1".into(),
        agent: Vec::new(),
        comment_type: "comment".into(),
        parent: 0,
        user_id: 0,
    }
}

#[tokio::test]
async fn writes_comments() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    assert_eq!(write.approved_comment_count(1).await.unwrap(), 1);
    let id = write.insert_comment(&commenter()).await.unwrap();
    assert!(id > 1);
    assert_eq!(
        json(&mut write, COMMENT, id).await,
        json(&mut write, COMMENT, 1).await
    );
    assert_eq!(write.approved_comment_count(1).await.unwrap(), 2);
    // Notes are not counted.
    let note = CommentFields {
        comment_type: "note".into(),
        ..commenter()
    };
    let note = write.insert_comment(&note).await.unwrap();
    assert_eq!(write.approved_comment_count(1).await.unwrap(), 2);
    assert_eq!(write.post_comment_ids(1).await.unwrap(), [note, id, 1]);

    let changed = CommentFields {
        post_id: 2,
        author: b"Ann".to_vec(),
        author_email: b"ann@a.test".to_vec(),
        author_url: b"http://a.test".to_vec(),
        author_ip: b"192.0.2.1".to_vec(),
        date: "2026-11-01 10:00:01".into(),
        date_gmt: "2026-11-01 05:00:02".into(),
        content: "Merci ✓".as_bytes().to_vec(),
        karma: -3,
        approved: "0".into(),
        agent: b"Agent".to_vec(),
        comment_type: "pingback".into(),
        parent: 1,
        user_id: 4,
    };
    assert_eq!(write.update_comment(id, &changed).await.unwrap(), 1);
    assert_eq!(
        json(&mut write, COMMENT, id).await,
        r#"[2, "Ann", "ann@a.test", "http://a.test", "192.0.2.1", "2026-11-01 10:00:01", "2026-11-01 05:00:02", "Merci ✓", -3, "0", "Agent", "pingback", 1, 4]"#
    );
    assert_eq!(write.update_comment(999_999, &changed).await.unwrap(), 0);

    assert_eq!(write.set_comment_status(id, "spam").await.unwrap(), 1);
    assert_eq!(json(&mut write, APPROVED, id).await, r#"["spam"]"#);
    assert_eq!(write.trash_post_comments(1).await.unwrap(), 2);
    assert_eq!(
        json(&mut write, APPROVED, note).await,
        r#"["post-trashed"]"#
    );
    assert_eq!(write.set_comments_status(&[1, note], "1").await.unwrap(), 2);
    assert_eq!(write.set_comments_status(&[], "1").await.unwrap(), 0);
    assert_eq!(json(&mut write, APPROVED, 1).await, r#"["1"]"#);
    assert_eq!(json(&mut write, APPROVED, note).await, r#"["1"]"#);

    assert_eq!(write.reparent_comments(1, 9).await.unwrap(), 1);
    assert!(json(&mut write, COMMENT, id).await.ends_with(", 9, 4]"));
    assert_eq!(write.delete_comment(note).await.unwrap(), 1);
    assert_eq!(write.delete_comment(note).await.unwrap(), 0);
    assert_eq!(write.post_comment_ids(1).await.unwrap(), [1]);
}

const USER: &str = "SELECT JSON_ARRAY(user_login, user_pass, user_nicename, user_email, \
     user_url, user_registered, user_activation_key, user_status, display_name) FROM wp_users \
     WHERE ID = ?";

/// The fixture's user, as PHP wrote it.
fn admin() -> UserFields {
    UserFields {
        pass: "$wp$2y$12$H0NCQv.BOqgC8HC3RvJ7u.sQwrqr14ruxSEt.fWE6FVneSAfNtBYy".into(),
        nicename: b"admin".to_vec(),
        email: b"admin@default-install.test".to_vec(),
        url: b"http://default-install.test".to_vec(),
        registered: "2026-10-27 08:08:15".into(),
        activation_key: String::new(),
        display_name: b"admin".to_vec(),
    }
}

#[tokio::test]
async fn writes_users() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let id = write.insert_user(b"admin", &admin()).await.unwrap();
    assert!(id > 1);
    assert_eq!(
        json(&mut write, USER, id).await,
        json(&mut write, USER, 1).await
    );

    let changed = UserFields {
        pass: "$wp$hash".into(),
        nicename: b"ann".to_vec(),
        email: b"ann@a.test".to_vec(),
        url: b"http://a.test".to_vec(),
        registered: "2026-11-01 10:00:01".into(),
        activation_key: "1700000000:key".into(),
        display_name: "Ann ✓".as_bytes().to_vec(),
    };
    assert_eq!(write.update_user(id, &changed).await.unwrap(), 1);
    assert_eq!(
        json(&mut write, USER, id).await,
        r#"["admin", "$wp$hash", "ann", "ann@a.test", "http://a.test", "2026-11-01 10:00:01", "1700000000:key", 0, "Ann ✓"]"#
    );
    assert_eq!(write.update_user(999_999, &changed).await.unwrap(), 0);
    assert_eq!(write.clear_activation_key(id).await.unwrap(), 1);
    assert!(
        json(&mut write, USER, id)
            .await
            .contains(r#""2026-11-01 10:00:01", "", 0"#)
    );
    assert_eq!(write.update_user(id, &changed).await.unwrap(), 1);
    assert_eq!(write.set_user_password(id, "$wp$new").await.unwrap(), 1);
    assert_eq!(
        json(&mut write, USER, id).await,
        r#"["admin", "$wp$new", "ann", "ann@a.test", "http://a.test", "2026-11-01 10:00:01", "", 0, "Ann ✓"]"#
    );

    sqlx::query("INSERT INTO wp_links (link_owner) VALUES (?), (?)")
        .bind(id)
        .bind(id)
        .execute(&mut *write.tx)
        .await
        .unwrap();
    assert_eq!(write.reassign_links(id, 1).await.unwrap(), 2);
    assert_eq!(write.reassign_links(id, 1).await.unwrap(), 0);
    assert_eq!(write.delete_user(id).await.unwrap(), 1);
    assert_eq!(write.delete_user(id).await.unwrap(), 0);
}

const OPTION: &str =
    "SELECT JSON_ARRAY(option_name, option_value, autoload) FROM wp_options WHERE option_id = ?";

#[tokio::test]
async fn writes_options() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let name = "hyscribe_test";
    assert_eq!(
        write
            .add_option(name, "a ✓".as_bytes(), "off")
            .await
            .unwrap(),
        1
    );
    let id: u64 = sqlx::query_scalar("SELECT option_id FROM wp_options WHERE option_name = ?")
        .bind(name)
        .fetch_one(&mut *write.tx)
        .await
        .unwrap();
    assert_eq!(
        json(&mut write, OPTION, id).await,
        r#"["hyscribe_test", "a ✓", "off"]"#
    );
    // Adding it again replaces it, in the same row.
    assert_eq!(write.add_option(name, b"b", "on").await.unwrap(), 2);
    assert_eq!(
        json(&mut write, OPTION, id).await,
        r#"["hyscribe_test", "b", "on"]"#
    );
    assert_eq!(write.update_option(name, b"c").await.unwrap(), 1);
    assert_eq!(
        json(&mut write, OPTION, id).await,
        r#"["hyscribe_test", "c", "on"]"#
    );
    assert_eq!(
        write
            .update_option_autoload(name, b"d", "auto-off")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        json(&mut write, OPTION, id).await,
        r#"["hyscribe_test", "d", "auto-off"]"#
    );
    assert_eq!(
        write.option_autoload(name).await.unwrap().as_deref(),
        Some("auto-off")
    );
    assert_eq!(write.option_autoload("hyscribe_none").await.unwrap(), None);
    assert_eq!(write.update_option("hyscribe_none", b"x").await.unwrap(), 0);
    assert_eq!(
        write
            .update_option_autoload("hyscribe_none", b"x", "on")
            .await
            .unwrap(),
        0
    );
    assert_eq!(write.delete_option(name).await.unwrap(), 1);
    assert_eq!(write.delete_option(name).await.unwrap(), 0);
}

const TERM: &str = "SELECT JSON_ARRAY(name, slug, term_group) FROM wp_terms WHERE term_id = ?";

const TAXONOMY: &str = "SELECT JSON_ARRAY(term_id, taxonomy, description, parent, count) \
     FROM wp_term_taxonomy WHERE term_taxonomy_id = ?";

const ORDERS: &str = "SELECT JSON_ARRAYAGG(JSON_ARRAY(term_taxonomy_id, term_order) \
     ORDER BY term_taxonomy_id) FROM wp_term_relationships WHERE object_id = ?";

#[tokio::test]
async fn writes_terms() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let term = write
        .insert_term(b"Uncategorized", b"uncategorized", 0)
        .await
        .unwrap();
    assert!(term > 1);
    assert_eq!(
        json(&mut write, TERM, term).await,
        json(&mut write, TERM, 1).await
    );
    let tt = write
        .insert_term_taxonomy((term, "category"), b"", 0)
        .await
        .unwrap();
    assert!(tt > 1);
    assert_eq!(
        json(&mut write, TAXONOMY, tt).await,
        format!(r#"[{term}, "category", "", 0, 0]"#)
    );

    assert_eq!(
        write
            .update_term(term, "Café ✓".as_bytes(), b"cafe", 2)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        json(&mut write, TERM, term).await,
        r#"["Café ✓", "cafe", 2]"#
    );
    assert_eq!(write.update_term(999_999, b"x", b"x", 0).await.unwrap(), 0);
    assert_eq!(write.set_term_slug(term, b"cafe-2").await.unwrap(), 1);
    assert_eq!(write.set_term_group(term, 5).await.unwrap(), 1);
    assert_eq!(
        json(&mut write, TERM, term).await,
        r#"["Café ✓", "cafe-2", 5]"#
    );
    let update = write.update_term_taxonomy(tt, (term, "post_tag"), b"About", 1);
    assert_eq!(update.await.unwrap(), 1);
    assert_eq!(
        json(&mut write, TAXONOMY, tt).await,
        format!(r#"[{term}, "post_tag", "About", 1, 0]"#)
    );
    assert_eq!(write.reparent_terms("category", 1, 3).await.unwrap(), 0);
    assert_eq!(write.reparent_terms("post_tag", 1, 3).await.unwrap(), 1);
    assert_eq!(write.set_term_count(tt, 4).await.unwrap(), 1);
    assert_eq!(
        json(&mut write, TAXONOMY, tt).await,
        format!(r#"[{term}, "post_tag", "About", 3, 4]"#)
    );
    assert_eq!(write.term_taxonomy_count(term).await.unwrap(), 1);
    let second = write
        .insert_term_taxonomy((term, "category"), b"", 0)
        .await
        .unwrap();
    assert_eq!(write.term_taxonomy_count(term).await.unwrap(), 2);

    // The counts, as PHP recomputes them.
    assert_eq!(write.term_object_count(1).await.unwrap(), 1);
    assert_eq!(
        write
            .term_post_count(1, &["publish"], &["post"])
            .await
            .unwrap(),
        1
    );
    assert_eq!(write.add_relationship(2, 1).await.unwrap(), 1);
    assert_eq!(write.term_object_count(1).await.unwrap(), 2);
    assert_eq!(
        write
            .term_post_count(1, &["publish"], &["post"])
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        write
            .term_post_count(1, &["publish"], &["post", "page"])
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        write
            .term_post_count(1, &["draft"], &["post", "page"])
            .await
            .unwrap(),
        0
    );
    assert_eq!(write.term_post_count(1, &[], &["post"]).await.unwrap(), 0);
    assert_eq!(
        write.term_post_count(1, &["publish"], &[]).await.unwrap(),
        0
    );
    let attachment = PostFields {
        status: "inherit".into(),
        post_type: "attachment".into(),
        parent: 1,
        ..hello()
    };
    let attachment = write.insert_post(&attachment).await.unwrap();
    let orphan = PostFields {
        status: "inherit".into(),
        post_type: "attachment".into(),
        ..hello()
    };
    let orphan = write.insert_post(&orphan).await.unwrap();
    let published = PostFields {
        post_type: "attachment".into(),
        ..hello()
    };
    let published = write.insert_post(&published).await.unwrap();
    for post in [attachment, orphan, published] {
        write.add_relationship(post, 1).await.unwrap();
    }
    assert_eq!(
        write.term_attachment_count(1, &["publish"]).await.unwrap(),
        2
    );
    assert_eq!(
        write.term_attachment_count(1, &["private"]).await.unwrap(),
        0
    );
    assert_eq!(write.term_attachment_count(1, &[]).await.unwrap(), 0);

    // A sorted taxonomy's orders: the existing relationship is updated.
    assert_eq!(
        write.set_term_order(1, &[(1, 2), (tt, 1)]).await.unwrap(),
        3
    );
    assert_eq!(
        json(&mut write, ORDERS, 1).await,
        format!("[[1, 2],[{tt}, 1]]")
    );
    assert_eq!(write.set_term_order(1, &[(second, 3)]).await.unwrap(), 1);
    assert!(write.set_term_order(1, &[]).await.is_err());
    assert_eq!(
        write.remove_relationships(1, &[tt, second]).await.unwrap(),
        2
    );
    assert_eq!(write.remove_relationships(1, &[]).await.unwrap(), 0);
    assert_eq!(json(&mut write, ORDERS, 1).await, "[[1, 2]]");

    assert_eq!(write.delete_term_taxonomy(tt).await.unwrap(), 1);
    assert_eq!(write.delete_term_taxonomy(tt).await.unwrap(), 0);
    assert_eq!(write.term_taxonomy_count(term).await.unwrap(), 1);
    assert_eq!(write.delete_term(term).await.unwrap(), 1);
    assert_eq!(write.delete_term(term).await.unwrap(), 0);
}

#[tokio::test]
async fn writes_meta() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    // The rows PHP wrote for the same meta.
    let template = write
        .add_meta(Meta::Post, 2, "_wp_page_template", b"default")
        .await
        .unwrap();
    let post_row =
        "SELECT JSON_ARRAY(post_id, meta_key, meta_value) FROM wp_postmeta WHERE meta_id = ?";
    assert_eq!(
        json(&mut write, post_row, template).await,
        json(&mut write, post_row, 1).await
    );
    let nickname = write
        .add_meta(Meta::User, 1, "nickname", b"admin")
        .await
        .unwrap();
    let user_row =
        "SELECT JSON_ARRAY(user_id, meta_key, meta_value) FROM wp_usermeta WHERE umeta_id = ?";
    assert_eq!(
        json(&mut write, user_row, nickname).await,
        json(&mut write, user_row, 1).await
    );

    let kinds = [
        (Meta::Post, post_row),
        (
            Meta::Comment,
            "SELECT JSON_ARRAY(comment_id, meta_key, meta_value) FROM wp_commentmeta WHERE meta_id = ?",
        ),
        (
            Meta::Term,
            "SELECT JSON_ARRAY(term_id, meta_key, meta_value) FROM wp_termmeta WHERE meta_id = ?",
        ),
        (Meta::User, user_row),
    ];
    let key = "hyscribe_key";
    for (meta, row) in kinds {
        let one = write.add_meta(meta, 1, key, b"one").await.unwrap();
        let two = write
            .add_meta(meta, 1, key, "two ✓".as_bytes())
            .await
            .unwrap();
        let other = write.add_meta(meta, 2, key, b"one").await.unwrap();
        assert_eq!(
            json(&mut write, row, two).await,
            r#"[1, "hyscribe_key", "two ✓"]"#
        );
        assert_eq!(
            sorted(write.meta_ids(meta, 1, key).await.unwrap()),
            [one, two]
        );
        assert!(write.meta_ids(meta, 3, key).await.unwrap().is_empty());
        let valued = write.meta_ids_valued(meta, (1, key), b"one").await.unwrap();
        assert_eq!(valued, [one]);
        assert_eq!(
            sorted(write.meta_ids_of_key(meta, key).await.unwrap()),
            [one, two, other]
        );
        let valued = write
            .meta_ids_of_key_valued(meta, key, b"one")
            .await
            .unwrap();
        assert_eq!(sorted(valued), [one, other]);
        let all = write.object_meta_ids(meta, 1).await.unwrap();
        assert!(all.contains(&one) && all.contains(&two) && !all.contains(&other));

        let from = write
            .update_meta_from(meta, (1, key), b"one", b"uno")
            .await
            .unwrap();
        assert_eq!(from, 1);
        assert_eq!(
            json(&mut write, row, one).await,
            r#"[1, "hyscribe_key", "uno"]"#
        );
        assert_eq!(
            json(&mut write, row, other).await,
            r#"[2, "hyscribe_key", "one"]"#
        );
        assert_eq!(write.update_meta(meta, 1, key, b"both").await.unwrap(), 2);
        assert_eq!(
            json(&mut write, row, two).await,
            r#"[1, "hyscribe_key", "both"]"#
        );
        assert_eq!(
            write
                .update_meta_by_id(meta, one, "renamed", b"x")
                .await
                .unwrap(),
            1
        );
        assert_eq!(json(&mut write, row, one).await, r#"[1, "renamed", "x"]"#);
        assert_eq!(write.delete_meta(meta, &[one, two]).await.unwrap(), 2);
        assert_eq!(write.delete_meta(meta, &[]).await.unwrap(), 0);
        assert_eq!(write.meta_ids_of_key(meta, key).await.unwrap(), [other]);
    }
}

/// Every write method on a site whose tables are missing, so each fails:
/// the error paths, and for the portability guard every write statement.
pub(crate) async fn every_write() -> Vec<DbError> {
    let mut config = fixture_config();
    config.prefix = Prefix::new("missing_").unwrap();
    let writer = Writer::open(&config, Mode::Primary).await.unwrap().unwrap();
    let mut errors = vec![writer.begin().await.err().unwrap()];
    let mut w = Write {
        writer: &writer,
        tx: writer.pool.begin().await.unwrap(),
        budget: None,
    };
    let post = hello();
    let comment = commenter();
    let user = admin();
    let meta = Meta::Post;
    let ids = [
        w.child_ids(1, "page").await.err(),
        w.author_post_ids(1).await.err(),
        w.post_comment_ids(1).await.err(),
        w.meta_ids(meta, 1, "k").await.err(),
        w.meta_ids_valued(meta, (1, "k"), b"v").await.err(),
        w.meta_ids_of_key(meta, "k").await.err(),
        w.meta_ids_of_key_valued(meta, "k", b"v").await.err(),
        w.object_meta_ids(meta, 1).await.err(),
        w.object_meta(meta, 1).await.err(),
        w.user(1).await.err(),
        w.user_id_by_login(b"u").await.err(),
        w.user_id_by_email(b"e").await.err(),
        w.nicename_owner(b"n", b"u").await.err(),
        w.author_post_ids_of_types(1, &["post"]).await.err(),
        w.owner_link_ids(1).await.err(),
        w.option_value("o").await.err(),
    ];
    let counts = [
        w.approved_comment_count(1).await.err(),
        w.term_taxonomy_count(1).await.err(),
        w.term_post_count(1, &["publish"], &["post"]).await.err(),
        w.term_attachment_count(1, &["publish"]).await.err(),
        w.term_object_count(1).await.err(),
        w.user_count().await.err(),
    ];
    let writes = [
        w.insert_post(&post).await.err(),
        w.update_post(1, &post).await.err(),
        w.set_post_name(1, b"n").await.err(),
        w.set_post_guid(1, b"g").await.err(),
        w.set_post_status(1, "publish").await.err(),
        w.set_comment_count(1, 1).await.err(),
        w.reparent_posts(1, "page", 0).await.err(),
        w.reassign_posts(1, 2).await.err(),
        w.delete_post(1).await.err(),
        w.insert_comment(&comment).await.err(),
        w.update_comment(1, &comment).await.err(),
        w.set_comment_status(1, "1").await.err(),
        w.set_comments_status(&[1], "1").await.err(),
        w.trash_post_comments(1).await.err(),
        w.reparent_comments(1, 0).await.err(),
        w.delete_comment(1).await.err(),
        w.insert_user(b"u", &user).await.err(),
        w.update_user(1, &user).await.err(),
        w.set_user_password(1, "h").await.err(),
        w.clear_activation_key(1).await.err(),
        w.reassign_links(1, 2).await.err(),
        w.delete_user(1).await.err(),
        w.add_option("o", b"v", "on").await.err(),
        w.update_option("o", b"v").await.err(),
        w.update_option_autoload("o", b"v", "on").await.err(),
        w.delete_option("o").await.err(),
        w.option_autoload("o").await.err(),
        w.add_meta(meta, 1, "k", b"v").await.err(),
        w.update_meta(meta, 1, "k", b"v").await.err(),
        w.update_meta_from(meta, (1, "k"), b"p", b"v").await.err(),
        w.update_meta_by_id(meta, 1, "k", b"v").await.err(),
        w.delete_meta(meta, &[1]).await.err(),
        w.insert_term(b"n", b"s", 0).await.err(),
        w.update_term(1, b"n", b"s", 0).await.err(),
        w.set_term_slug(1, b"s").await.err(),
        w.set_term_group(1, 1).await.err(),
        w.delete_term(1).await.err(),
        w.insert_term_taxonomy((1, "category"), b"", 0).await.err(),
        w.update_term_taxonomy(1, (1, "category"), b"", 0)
            .await
            .err(),
        w.reparent_terms("category", 1, 0).await.err(),
        w.set_term_count(1, 1).await.err(),
        w.delete_term_taxonomy(1).await.err(),
        w.add_relationship(1, 1).await.err(),
        w.set_term_order(1, &[(1, 1)]).await.err(),
        w.remove_relationships(1, &[1]).await.err(),
    ];
    for error in ids.into_iter().chain(counts).chain(writes) {
        errors.push(error.expect("a missing table fails"));
    }
    errors
}

#[tokio::test]
async fn every_write_fails_without_its_table() {
    for err in every_write().await {
        assert!(err.to_string().contains("missing_"), "{err}");
    }
}

/// The reads the write functions make inside an action.
#[tokio::test]
async fn reads_inside_an_action() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    assert_eq!(write.prefix(), "wp_");
    let (login, admin) = write.user(1).await.unwrap().unwrap();
    assert_eq!(login, b"admin");
    assert_eq!(admin.email, b"admin@default-install.test");
    assert_eq!(admin.registered, "2026-10-27 08:08:15");
    assert_eq!(admin.display_name, b"admin");
    assert!(admin.pass.starts_with("$wp$"));
    assert!(write.user(999_999).await.unwrap().is_none());
    assert_eq!(write.user_id_by_login(b"admin").await.unwrap(), Some(1));
    assert_eq!(write.user_id_by_login(b"nobody").await.unwrap(), None);
    let email = b"admin@default-install.test";
    assert_eq!(write.user_id_by_email(email).await.unwrap(), Some(1));
    assert_eq!(write.user_id_by_email(b"no@a.test").await.unwrap(), None);
    assert_eq!(
        write.nicename_owner(b"admin", b"other").await.unwrap(),
        Some(1)
    );
    assert_eq!(
        write.nicename_owner(b"admin", b"admin").await.unwrap(),
        None
    );
    assert_eq!(write.user_count().await.unwrap(), 1);
    let types = write
        .author_post_ids_of_types(1, &["page", "attachment"])
        .await;
    assert_eq!(sorted(types.unwrap()), [2, 3]);
    assert!(
        write
            .author_post_ids_of_types(2, &["post"])
            .await
            .unwrap()
            .is_empty()
    );
    sqlx::query("INSERT INTO wp_links (link_owner) VALUES (5), (5), (6)")
        .execute(&mut *write.tx)
        .await
        .unwrap();
    assert_eq!(write.owner_link_ids(5).await.unwrap().len(), 2);
    let blogname = write.option_value("blogname").await.unwrap();
    assert_eq!(blogname.unwrap(), b"HyScribe Default Install");
    assert!(
        write
            .option_value("no_such_option")
            .await
            .unwrap()
            .is_none()
    );
    let meta = write.object_meta(Meta::User, 1).await.unwrap();
    assert_eq!(meta[0], (b"nickname".to_vec(), Some(b"admin".to_vec())));
    assert_eq!(meta.len(), 16);
    write
        .add_meta(Meta::Post, 1, "hyscribe_test", b"v")
        .await
        .unwrap();
    let meta = write.object_meta(Meta::Post, 1).await.unwrap();
    assert_eq!(
        meta.last().unwrap(),
        &(b"hyscribe_test".to_vec(), Some(b"v".to_vec()))
    );
}

/// Each statement fails once the budget a test sets is spent.
#[tokio::test]
async fn a_spent_budget_fails_every_statement() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut w = writer.begin().await.unwrap();
    w.fail_after(1);
    assert!(w.option_value("blogname").await.unwrap().is_some());
    let spent = [
        w.option_value("o").await.err(),
        w.option_autoload("o").await.err(),
        w.user(1).await.err(),
        w.object_meta(Meta::User, 1).await.err(),
        w.user_count().await.err(),
        w.owner_link_ids(1).await.err(),
        w.delete_option("o").await.err(),
    ];
    for err in spent {
        assert_eq!(
            err.unwrap().to_string(),
            "the test's statement budget is spent"
        );
    }
}
