// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up` (CI does).

use super::*;
use hyscribe_config::Secret;

fn fixture_config() -> Database {
    hyscribe_testkit::fixture_database()
}

async fn fixture() -> Db {
    Db::connect(&fixture_config())
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE)
}

#[test]
fn the_prefix_goes_into_sql() {
    assert_eq!(
        Prefix::default().sql("SELECT * FROM {prefix}posts").0,
        "SELECT * FROM wp_posts"
    );
}

#[tokio::test]
async fn reads_the_default_install() {
    let db = fixture().await;
    let options = db.autoload_options().await.unwrap();
    assert!(
        options
            .iter()
            .any(|(name, value)| name == "blogname" && value == b"HyScribe Default Install")
    );
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

    assert_eq!(
        db.category(1).await.unwrap().map(|term| term.slug),
        Some(b"uncategorized".to_vec())
    );
    assert_eq!(db.category(999).await.unwrap(), None);
}

/// Titles of the posts `query` (with defaults for the rest) selects.
async fn titles(db: &Db, query: PostQuery) -> Vec<String> {
    let query = PostQuery {
        post_types: if query.post_types.is_empty() {
            vec!["post".into()]
        } else {
            query.post_types
        },
        limit: if query.limit == 0 { 10 } else { query.limit },
        ..query
    };
    db.posts(&query)
        .await
        .unwrap()
        .into_iter()
        .map(|p| String::from_utf8(p.title).unwrap())
        .collect()
}

#[tokio::test]
async fn post_lists() {
    let db = fixture().await;
    let all = || PostQuery::default();
    assert_eq!(titles(&db, all()).await, ["Hello world!"]);
    // The privacy policy page is a draft.
    assert_eq!(
        titles(
            &db,
            PostQuery {
                post_types: vec!["page".into()],
                ..all()
            }
        )
        .await,
        ["Sample Page"]
    );
    assert!(
        titles(&db, PostQuery { offset: 1, ..all() })
            .await
            .is_empty()
    );

    let both = || PostQuery {
        post_types: vec!["post".into(), "page".into()],
        ..all()
    };
    let search = |terms: &[&str]| PostQuery {
        search: terms.iter().map(|t| t.to_string()).collect(),
        ..both()
    };
    assert_eq!(titles(&db, search(&["hello"])).await, ["Hello world!"]);
    // Case-insensitive, in content too, and title matches first.
    assert_eq!(
        titles(&db, search(&["WORDPRESS"])).await,
        ["Hello world!", "Sample Page"]
    );
    assert_eq!(
        titles(&db, search(&["sample", "page"])).await,
        ["Sample Page"]
    );
    assert!(titles(&db, search(&["nothingmatches"])).await.is_empty());
    assert_eq!(
        titles(
            &db,
            PostQuery {
                exclude: vec![1],
                ..both()
            }
        )
        .await,
        ["Sample Page"]
    );
    assert_eq!(
        titles(
            &db,
            PostQuery {
                include: Some(vec![2]),
                ..both()
            }
        )
        .await,
        ["Sample Page"]
    );
    assert!(
        titles(
            &db,
            PostQuery {
                include: Some(Vec::new()),
                ..both()
            }
        )
        .await
        .is_empty()
    );
    // Wildcards are searched for literally.
    assert!(titles(&db, search(&["%"])).await.is_empty());
    assert!(titles(&db, search(&["_ello"])).await.is_empty());

    assert_eq!(
        titles(
            &db,
            PostQuery {
                category: Some(1),
                ..all()
            }
        )
        .await,
        ["Hello world!"]
    );
    assert!(
        titles(
            &db,
            PostQuery {
                category: Some(999),
                ..all()
            }
        )
        .await
        .is_empty()
    );

    let hello = db.post(1).await.unwrap().unwrap();
    let part =
        |range: std::ops::Range<usize>| hello.date.get(range).unwrap().parse::<u64>().unwrap();
    let (year, month, day) = (part(0..4), part(5..7), part(8..10));
    let dated = |year, month, day| PostQuery {
        year,
        month,
        day,
        ..all()
    };
    assert_eq!(
        titles(&db, dated(Some(year), Some(month), Some(day))).await,
        ["Hello world!"]
    );
    assert_eq!(
        titles(&db, dated(Some(year), None, None)).await,
        ["Hello world!"]
    );
    assert!(titles(&db, dated(Some(1999), None, None)).await.is_empty());
    assert!(
        titles(&db, dated(Some(year), Some(month % 12 + 1), None))
            .await
            .is_empty()
    );
    assert!(
        titles(&db, dated(Some(year), Some(month), Some(day % 28 + 1)))
            .await
            .is_empty()
    );
}

#[test]
fn like_patterns_escape_wildcards() {
    assert_eq!(like("a%b_c\\d"), "%a\\%b\\_c\\\\d%");
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
    config.prefix = Prefix::new("missing_").unwrap();
    let db = Db::connect(&config).await.unwrap();
    for err in [
        db.autoload_options().await.unwrap_err(),
        db.post(1).await.unwrap_err(),
        db.posts(&PostQuery {
            post_types: vec!["post".into()],
            limit: 1,
            ..PostQuery::default()
        })
        .await
        .unwrap_err(),
        db.category(1).await.unwrap_err(),
        db.option("blogname").await.unwrap_err(),
        db.post_meta(1, "x").await.unwrap_err(),
        db.post_terms(1, "category").await.unwrap_err(),
        db.user(1).await.unwrap_err(),
        db.comments(1).await.unwrap_err(),
        db.comment(1).await.unwrap_err(),
        db.adjacent_post(&Post::default(), true, None)
            .await
            .unwrap_err(),
    ] {
        assert!(
            [
                "missing_options",
                "missing_posts",
                "missing_terms",
                "missing_postmeta",
                "missing_users",
                "missing_comments"
            ]
            .iter()
            .any(|table| err.to_string().contains(table)),
            "{err}"
        );
    }

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

#[test]
fn order_follows_the_query() {
    let text = |ascending| {
        posts_sql(&PostQuery {
            ascending,
            ..PostQuery::default()
        })
        .text
    };
    assert!(text(true).ends_with("ORDER BY post_date ASC LIMIT ? OFFSET ?"));
    assert!(text(false).ends_with("ORDER BY post_date DESC LIMIT ? OFFSET ?"));
    // get_pages() order; a search adds no title ordering of its own.
    let pages = posts_sql(&PostQuery {
        by_menu_order: true,
        search: vec!["x".into()],
        ..PostQuery::default()
    });
    assert!(
        pages
            .text
            .ends_with("ORDER BY menu_order ASC, post_title ASC LIMIT ? OFFSET ?")
    );
    assert_eq!(pages.binds.len(), 5);
}

#[tokio::test]
async fn options_meta_and_terms() {
    let db = fixture().await;
    assert_eq!(
        db.option("blogname").await.unwrap(),
        Some(b"HyScribe Default Install".to_vec())
    );
    assert_eq!(db.option("no_such_option").await.unwrap(), None);
    let admin = db.user(1).await.unwrap().unwrap();
    assert_eq!((admin.id, &admin.display_name[..]), (1, &b"admin"[..]));
    assert_eq!(
        (
            &admin.login[..],
            &admin.email[..],
            &admin.first_name[..],
            &admin.last_name[..]
        ),
        (
            &b"admin"[..],
            &b"admin@default-install.test"[..],
            &b""[..],
            &b""[..]
        )
    );
    assert_eq!(db.user(99).await.unwrap(), None);
    let comments = db.comments(1).await.unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(db.comment(1).await.unwrap().as_ref(), comments.first());
    assert_eq!(db.comment(99).await.unwrap(), None);
    assert_eq!(
        (comments[0].id, comments[0].parent, &comments[0].author[..]),
        (1, 0, &b"A WordPress Commenter"[..])
    );
    assert!(comments[0].content.starts_with(b"Hi, this is a comment."));
    assert!(db.comments(2).await.unwrap().is_empty());
    assert_eq!(
        db.post_meta(2, "_wp_page_template").await.unwrap(),
        Some(b"default".to_vec())
    );
    assert_eq!(db.post_meta(1, "_thumbnail_id").await.unwrap(), None);
    let terms = db.post_terms(1, "category").await.unwrap();
    assert_eq!(
        terms,
        [Term {
            id: 1,
            name: b"Uncategorized".to_vec(),
            slug: b"uncategorized".to_vec()
        }]
    );
    assert!(db.post_terms(1, "post_tag").await.unwrap().is_empty());
}

#[tokio::test]
async fn adjacent_posts() {
    let db = fixture().await;
    let hello = db.post(1).await.unwrap().unwrap();
    let at = |date: &str, id, post_type: &str| Post {
        id,
        date: date.into(),
        post_type: post_type.into(),
        ..Post::default()
    };
    let title = |found: Option<Post>| found.map(|p| String::from_utf8(p.title).unwrap());
    let later = at("2099-01-01 00:00:00", 999, "post");
    let earlier = at("2000-01-01 00:00:00", 999, "post");
    assert_eq!(
        title(db.adjacent_post(&later, true, None).await.unwrap()).as_deref(),
        Some("Hello world!")
    );
    assert_eq!(
        title(db.adjacent_post(&later, false, None).await.unwrap()),
        None
    );
    assert_eq!(
        title(db.adjacent_post(&earlier, false, None).await.unwrap()).as_deref(),
        Some("Hello world!")
    );
    assert_eq!(
        title(db.adjacent_post(&earlier, true, None).await.unwrap()),
        None
    );
    // The same date breaks ties by ID.
    let same_date = |id| at(&hello.date, id, "post");
    assert!(
        db.adjacent_post(&same_date(2), true, None)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        db.adjacent_post(&same_date(1), true, None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.adjacent_post(&same_date(0), false, None)
            .await
            .unwrap()
            .is_some()
    );
    // Only the post's type, published, sharing a term when asked.
    assert_eq!(
        db.adjacent_post(&at("2099-01-01 00:00:00", 999, "page"), true, None)
            .await
            .unwrap()
            .map(|p| p.id),
        Some(2)
    );
    let in_terms = |ids: &'static [u64]| db.adjacent_post(&later, true, Some(("category", ids)));
    assert!(in_terms(&[1]).await.unwrap().is_some());
    assert!(in_terms(&[99]).await.unwrap().is_none());
    assert!(
        db.adjacent_post(&later, true, Some(("post_tag", &[1])))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn last_post_modified() {
    let db = fixture().await;
    assert_eq!(
        db.last_post_modified().await.unwrap().as_deref(),
        Some("2026-10-27 08:08:15")
    );
}
