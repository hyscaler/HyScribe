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
    write
        .add_meta(Meta::Term, 1, key, Some(b"a"))
        .await
        .unwrap();
    write
        .add_meta(Meta::Term, 1, key, Some(b"b"))
        .await
        .unwrap();
    drop(write);
    let mut write = writer.begin().await.unwrap();
    assert!(
        write
            .meta_ids_of_key(Meta::Term, key)
            .await
            .unwrap()
            .is_empty()
    );

    let first = write
        .add_meta(Meta::Term, 1, key, Some(b"a"))
        .await
        .unwrap();
    let second = write
        .add_meta(Meta::Term, 1, key, Some(b"b"))
        .await
        .unwrap();
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
    let kept = write
        .add_meta(Meta::Term, 1, key, Some(b"kept"))
        .await
        .unwrap();
    write.commit().await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    write
        .add_meta(Meta::Term, 1, key, Some(b"dropped"))
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

/// A revision of the fixture's first post saved at `date`.
fn revision(date: &str) -> PostFields {
    PostFields {
        parent: 1,
        post_type: "revision".into(),
        status: "inherit".into(),
        name: b"1-revision-v1".to_vec(),
        date: date.into(),
        ..hello()
    }
}

#[tokio::test]
async fn reads_posts_inside_an_action() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let shown = |post: Option<PostFields>| format!("{post:?}");
    assert_eq!(shown(write.post(1).await.unwrap()), shown(Some(hello())));
    assert!(write.post(999_999).await.unwrap().is_none());
    assert_eq!(write.insert_post_with_id(990, &draft()).await.unwrap(), 990);
    assert_eq!(shown(write.post(990).await.unwrap()), shown(Some(draft())));

    assert!(
        write
            .post_name_used(b"hello-world", "post", 0)
            .await
            .unwrap()
    );
    assert!(
        !write
            .post_name_used(b"hello-world", "post", 1)
            .await
            .unwrap()
    );
    assert!(
        !write
            .post_name_used(b"hello-world", "page", 0)
            .await
            .unwrap()
    );
    let used = async |write: &mut Write<'_>, parent, id| {
        write
            .sibling_name_used(b"sample-page", ("page", parent), id)
            .await
            .unwrap()
    };
    assert!(used(&mut write, 0, 0).await);
    assert!(!used(&mut write, 1, 0).await);
    assert!(!used(&mut write, 0, 2).await);
    assert!(
        !write
            .sibling_name_used(b"sample-page", ("post", 0), 0)
            .await
            .unwrap()
    );

    assert!(write.published_post_exists().await.unwrap());
    write.set_post_status(1, "trash").await.unwrap();
    assert!(!write.published_post_exists().await.unwrap());
    assert_eq!(
        write
            .trashed_post_ids_named(b"hello-world", 0)
            .await
            .unwrap(),
        [1]
    );
    assert!(
        write
            .trashed_post_ids_named(b"hello-world", 1)
            .await
            .unwrap()
            .is_empty()
    );

    let old = write
        .insert_post(&revision("2026-01-01 00:00:00"))
        .await
        .unwrap();
    let new = write
        .insert_post(&revision("2026-02-01 00:00:00"))
        .await
        .unwrap();
    let tie = write
        .insert_post(&revision("2026-02-01 00:00:00"))
        .await
        .unwrap();
    let other = PostFields {
        status: "draft".into(),
        ..revision("2026-03-01 00:00:00")
    };
    write.insert_post(&other).await.unwrap();
    assert_eq!(write.revision_ids(1).await.unwrap(), [tie, new, old]);

    assert_eq!(
        write.post_comments(1).await.unwrap(),
        [(1, "1".to_string())]
    );
    assert!(write.post_comments(2).await.unwrap().is_empty());
    // A spent statement budget fails the reads too.
    write.fail_after(0);
    assert!(write.post(1).await.is_err());
    assert!(write.post_comments(1).await.is_err());
    assert!(write.object_terms(1, "category").await.is_err());
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

#[tokio::test]
async fn reads_for_comment_writes() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let fixture = commenter();
    assert_eq!(write.comment(1).await.unwrap().unwrap(), fixture);
    assert_eq!(write.comment(999_999).await.unwrap(), None);
    let negative = CommentFields {
        karma: -3,
        parent: 1,
        user_id: 1,
        date: "0000-00-00 00:00:00".into(),
        ..commenter()
    };
    let id = write.insert_comment(&negative).await.unwrap();
    assert_eq!(write.comment(id).await.unwrap().unwrap(), negative);

    let (author, email) = (&fixture.author[..], &fixture.author_email[..]);
    let duplicate = async |write: &mut Write<'_>, parent, email, content: &[u8]| {
        let found = write.duplicate_comment((1, parent), author, email, content);
        found.await.unwrap()
    };
    assert_eq!(
        duplicate(&mut write, 0, Some(email), &fixture.content).await,
        Some(1)
    );
    assert_eq!(
        duplicate(&mut write, 0, None, &fixture.content).await,
        Some(1)
    );
    assert_eq!(
        duplicate(&mut write, 0, Some(b"x"), &fixture.content).await,
        None
    );
    assert_eq!(
        duplicate(&mut write, 1, None, &fixture.content).await,
        Some(id)
    );
    assert_eq!(duplicate(&mut write, 0, None, b"other").await, None);
    write.set_comment_status(1, "trash").await.unwrap();
    assert_eq!(duplicate(&mut write, 0, None, &fixture.content).await, None);
    write.set_comment_status(1, "1").await.unwrap();

    assert!(
        write
            .author_has_approved_comment(author, email)
            .await
            .unwrap()
    );
    assert!(
        !write
            .author_has_approved_comment(author, b"x")
            .await
            .unwrap()
    );
    assert!(write.user_has_approved_comment(1).await.unwrap());
    assert!(!write.user_has_approved_comment(2).await.unwrap());
    write.set_comment_status(id, "0").await.unwrap();
    assert!(!write.user_has_approved_comment(1).await.unwrap());

    let since = "2026-10-27 08:08:15";
    let found = Some("2026-10-27 08:08:15".to_string());
    let later = "2026-10-27 08:08:16";
    for (since, user, email, last) in [
        (since, (0, &b""[..]), &b"x"[..], &found),
        (since, (0, b"x"), email, &found),
        (since, (1, b"x"), b"x", &found),
        (since, (2, b""), b"x", &None),
        (later, (0, b""), email, &None),
    ] {
        let date = write.last_comment_date(since, user, email).await.unwrap();
        assert_eq!(&date, last, "{since} {user:?}");
    }

    let note = |approved: &str, date: &str| CommentFields {
        comment_type: "note".into(),
        approved: approved.into(),
        parent: id,
        date_gmt: date.into(),
        ..commenter()
    };
    let older = write
        .insert_comment(&note("1", "2026-01-01 00:00:01"))
        .await
        .unwrap();
    let newer = write
        .insert_comment(&note("0", "2026-01-01 00:00:02"))
        .await
        .unwrap();
    write
        .insert_comment(&note("spam", "2026-01-01 00:00:03"))
        .await
        .unwrap();
    let reply = CommentFields {
        comment_type: "comment".into(),
        ..note("1", "2026-01-01 00:00:04")
    };
    write.insert_comment(&reply).await.unwrap();
    assert_eq!(write.child_note_ids(id).await.unwrap(), [newer, older]);

    assert_eq!(write.post_author(1).await.unwrap(), Some(1));
    assert_eq!(write.post_author(999_999).await.unwrap(), None);
    let admin = b"admin@default-install.test";
    assert_eq!(write.user_id_by_email(admin).await.unwrap(), Some(1));
    assert_eq!(write.user_id_by_email(b"x").await.unwrap(), None);

    let meta = Meta::Comment;
    assert_eq!(write.meta_value(meta, id, "k").await.unwrap(), None);
    write.add_meta(meta, id, "k", Some(b"first")).await.unwrap();
    write
        .add_meta(meta, id, "k", Some(b"second"))
        .await
        .unwrap();
    write.add_meta(meta, id, "other", Some(b"x")).await.unwrap();
    let first = write.meta_value(meta, id, "k").await.unwrap();
    assert_eq!(first.as_deref(), Some(&b"first"[..]));
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
async fn reads_terms_inside_the_action() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let uncategorized = TermRow {
        term_id: 1,
        name: b"Uncategorized".to_vec(),
        slug: b"uncategorized".to_vec(),
        term_group: 0,
        term_taxonomy_id: 1,
        description: Vec::new(),
        parent: 0,
    };
    assert_eq!(
        write.term_in("category", 1).await.unwrap(),
        Some(uncategorized.clone())
    );
    assert_eq!(write.term_in("post_tag", 1).await.unwrap(), None);
    let news = write.insert_term(b"News", b"news", 3).await.unwrap();
    let news_tt = write
        .insert_term_taxonomy((news, "category"), b"What", 1)
        .await
        .unwrap();
    let alpha = write.insert_term(b"Alpha", b"alpha", 0).await.unwrap();
    let alpha_tt = write
        .insert_term_taxonomy((alpha, "category"), b"", 1)
        .await
        .unwrap();
    let id = |term: Option<TermRow>| term.map(|term| term.term_id);

    // Names and slugs compare under the column's collation.
    let found = write.term_by_slug("category", Some("NEWS")).await.unwrap();
    let found = found.unwrap();
    assert_eq!(
        (found.term_id, found.term_taxonomy_id, found.term_group),
        (news, news_tt, 3)
    );
    assert_eq!((found.parent, &found.description[..]), (1, &b"What"[..]));
    let first = write.term_by_slug("category", None).await.unwrap();
    assert_eq!(first, Some(uncategorized.clone()));
    assert_eq!(write.term_by_slug("post_tag", None).await.unwrap(), None);
    let named = write.term_by_name("category", "news").await.unwrap();
    assert_eq!(id(named), Some(news));
    assert_eq!(write.term_by_name("category", "").await.unwrap(), None);
    let under = write.terms_under("category", 1, None).await.unwrap();
    let under: Vec<u64> = under.into_iter().map(|term| term.term_id).collect();
    assert_eq!(under, [alpha, news]);
    let named = write
        .terms_under("category", 1, Some("NEWS"))
        .await
        .unwrap();
    assert_eq!(named.len(), 1);
    let top = write.terms_under("category", 0, None).await.unwrap();
    assert_eq!(top, [uncategorized]);

    let newer = write.insert_term(b"News", b"news", 0).await.unwrap();
    let newer_tt = write
        .insert_term_taxonomy((newer, "category"), b"", 1)
        .await
        .unwrap();
    let older = write
        .older_duplicate(("category", 1), "news", (newer, newer_tt))
        .await
        .unwrap();
    assert_eq!(id(older), Some(news));
    for (place, slug, ids) in [
        (("category", 1), "news", (news, news_tt)),
        (("category", 0), "news", (newer, newer_tt)),
        (("post_tag", 1), "news", (newer, newer_tt)),
        (("category", 1), "missing", (newer, newer_tt)),
        (("category", 1), "news", (newer, news_tt)),
    ] {
        let older = write.older_duplicate(place, slug, ids).await.unwrap();
        assert_eq!(older, None);
    }

    assert!(write.term_exists(1).await.unwrap());
    assert!(!write.term_exists(999).await.unwrap());
    for (slug, name, matches) in [
        (Some("uncategorized"), "x", true),
        (Some("x"), "Uncategorized", true),
        (Some("x"), "y", false),
        (None, "y", true),
    ] {
        assert_eq!(write.term_matches(slug, name).await.unwrap(), matches);
    }
    assert!(write.slug_taken("alpha", 0).await.unwrap());
    assert!(!write.slug_taken("alpha", alpha).await.unwrap());
    assert!(!write.slug_taken("nope", 0).await.unwrap());

    assert_eq!(write.object_terms(1, "category").await.unwrap(), [(1, 1)]);
    write.add_relationship(1, alpha_tt).await.unwrap();
    let terms = write.object_terms(1, "category").await.unwrap();
    assert_eq!(terms, [(1, 1), (alpha, alpha_tt)]);
    assert!(write.object_terms(1, "post_tag").await.unwrap().is_empty());
    assert!(write.has_relationship(1, alpha_tt).await.unwrap());
    assert!(!write.has_relationship(2, alpha_tt).await.unwrap());
    write.add_relationship(2, alpha_tt).await.unwrap();
    assert_eq!(write.term_object_ids(alpha_tt).await.unwrap(), [1, 2]);
    assert_eq!(write.max_term_group().await.unwrap(), 3);
    let parents = write.term_parents("category").await.unwrap();
    assert_eq!(parents, [(1, 0), (news, 1), (alpha, 1), (newer, 1)]);
    assert!(write.term_parents("post_tag").await.unwrap().is_empty());

    // Menu items linking to a term; a post, or a custom link, does not.
    let menu_item = PostFields {
        post_type: "nav_menu_item".into(),
        ..hello()
    };
    let news_text = news.to_string();
    for (post, kind, count) in [
        (&hello(), &b"taxonomy"[..], 0),
        (&menu_item, b"custom", 0),
        (&menu_item, b"taxonomy", 1),
    ] {
        let item = write.insert_post(post).await.unwrap();
        for (key, value) in [
            ("_menu_item_object_id", news_text.as_bytes()),
            ("_menu_item_type", kind),
            ("_menu_item_object", b"category"),
        ] {
            write
                .add_meta(Meta::Post, item, key, Some(value))
                .await
                .unwrap();
        }
        assert_eq!(
            write.term_menu_items(news, "category").await.unwrap(),
            count
        );
    }
    assert_eq!(write.term_menu_items(news, "post_tag").await.unwrap(), 0);
    assert_eq!(write.term_menu_items(alpha, "category").await.unwrap(), 0);

    let default = write.option_value("default_category").await.unwrap();
    assert_eq!(default.as_deref(), Some(&b"1"[..]));
    assert_eq!(write.option_value("hyscribe_missing").await.unwrap(), None);
    let post = write.post_type_status(2).await.unwrap();
    assert_eq!(post, Some(("page".into(), "publish".into())));
    assert_eq!(write.post_type_status(999).await.unwrap(), None);
}

#[tokio::test]
async fn writes_meta() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    // The rows PHP wrote for the same meta.
    let template = write
        .add_meta(Meta::Post, 2, "_wp_page_template", Some(b"default"))
        .await
        .unwrap();
    let post_row =
        "SELECT JSON_ARRAY(post_id, meta_key, meta_value) FROM wp_postmeta WHERE meta_id = ?";
    assert_eq!(
        json(&mut write, post_row, template).await,
        json(&mut write, post_row, 1).await
    );
    let nickname = write
        .add_meta(Meta::User, 1, "nickname", Some(b"admin"))
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
        let one = write.add_meta(meta, 1, key, Some(b"one")).await.unwrap();
        let two = write
            .add_meta(meta, 1, key, Some("two ✓".as_bytes()))
            .await
            .unwrap();
        let other = write.add_meta(meta, 2, key, Some(b"one")).await.unwrap();
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
            .update_meta_from(meta, (1, key), b"one", Some(b"uno"))
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
        assert_eq!(
            write
                .update_meta(meta, 1, key, Some(b"both"))
                .await
                .unwrap(),
            2
        );
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
        w.trashed_post_ids_named(b"n", 1).await.err(),
        w.revision_ids(1).await.err(),
        w.object_meta(meta, 1).await.err(),
        w.user(1).await.err(),
        w.user_id_by_login(b"u").await.err(),
        w.user_id_by_email(b"e").await.err(),
        w.nicename_owner(b"n", b"u").await.err(),
        w.author_post_ids_of_types(1, &["post"]).await.err(),
        w.owner_link_ids(1).await.err(),
    ];
    let post_reads = [
        w.post(1).await.err(),
        w.post_comments(1).await.err(),
        w.post_name_used(b"n", "post", 1).await.err(),
        w.sibling_name_used(b"n", ("page", 0), 1).await.err(),
        w.published_post_exists().await.err(),
        w.insert_post_with_id(990, &post).await.err(),
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
        w.add_meta(meta, 1, "k", Some(b"v")).await.err(),
        w.update_meta(meta, 1, "k", Some(b"v")).await.err(),
        w.update_meta_from(meta, (1, "k"), b"p", Some(b"v"))
            .await
            .err(),
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
    let since = "2026-01-01 00:00:00";
    let reads = [
        w.comment(1).await.err(),
        w.duplicate_comment((1, 0), b"a", Some(b"e"), b"c")
            .await
            .err(),
        w.duplicate_comment((1, 0), b"a", None, b"c").await.err(),
        w.user_has_approved_comment(1).await.err(),
        w.author_has_approved_comment(b"a", b"e").await.err(),
        w.last_comment_date(since, (0, b"ip"), b"e").await.err(),
        w.last_comment_date(since, (1, b""), b"e").await.err(),
        w.child_note_ids(1).await.err(),
        w.post_author(1).await.err(),
        w.user_id_by_email(b"e").await.err(),
        w.meta_value(meta, 1, "k").await.err(),
        w.term_in("category", 1).await.err(),
        w.term_by_slug("category", None).await.err(),
        w.term_by_name("category", "n").await.err(),
        w.terms_under("category", 0, None).await.err(),
        w.older_duplicate(("category", 0), "s", (2, 2)).await.err(),
        w.term_exists(1).await.err(),
        w.term_matches(None, "n").await.err(),
        w.slug_taken("s", 0).await.err(),
        w.object_terms(1, "category").await.err(),
        w.has_relationship(1, 1).await.err(),
        w.term_object_ids(1).await.err(),
        w.max_term_group().await.err(),
        w.term_parents("category").await.err(),
        w.term_menu_items(1, "category").await.err(),
        w.option_value("o").await.err(),
        w.post_type_status(1).await.err(),
    ];
    for error in ids
        .into_iter()
        .chain(counts)
        .chain(post_reads)
        .chain(writes)
        .chain(reads)
    {
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
        .add_meta(Meta::Post, 1, "hyscribe_test", Some(b"v"))
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
