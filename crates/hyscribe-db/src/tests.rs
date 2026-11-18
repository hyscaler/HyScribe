// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up` (CI does).

use super::*;
use hyscribe_config::Secret;
use hyscribe_model::{
    CommentQuery, FormatFilter, StatusFilter, Term, TermFilter, TermOrder, TermQuery,
    TimeCondition, User, UserQuery,
};

/// The approved and pending comments.
const PENDING: CommentFilter<'static> = CommentFilter {
    approved: &["0", "1"],
    user: None,
    types: &[],
    search: None,
    post: 0,
};

/// Every user.
const EVERYONE: UserFilter<'static> = UserFilter {
    caps_key: "wp_capabilities",
    roles: &[],
    include: &[],
    search: None,
    all_private: true,
    me: 1,
};

/// The newest comments first.
const NEWEST: &[(&str, bool)] = &[("comment_date_gmt", true), ("comment_ID", true)];

pub(crate) fn fixture_config() -> Database {
    hyscribe_testkit::fixture_database()
}

async fn fixture() -> Db {
    named(&fixture_config().name).await
}

/// The fixture database `name`.
async fn named(name: &str) -> Db {
    let config = Database {
        name: name.into(),
        ..fixture_config()
    };
    Db::connect(&config)
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
    // Another post's slug, in any case, as `wp_unique_post_slug()` checks it.
    assert!(db.post_name_taken("Hello-World", 2).await.unwrap());
    assert!(!db.post_name_taken("hello-world", 1).await.unwrap());
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

    let found = |terms: Vec<(String, Term)>| -> Vec<(String, Vec<u8>)> {
        terms
            .into_iter()
            .map(|(tax, term)| (tax, term.slug))
            .collect()
    };
    let uncategorized = vec![("category".to_string(), b"uncategorized".to_vec())];
    assert_eq!(
        found(db.find_terms(&[], &[1, 999]).await.unwrap()),
        uncategorized
    );
    assert_eq!(
        found(db.find_terms(&uncategorized, &[]).await.unwrap()),
        uncategorized
    );
    let tag = [("post_tag".to_string(), b"uncategorized".to_vec())];
    assert!(db.find_terms(&tag, &[]).await.unwrap().is_empty());
    // The terms with posts, as a taxonomy's sitemap pages them.
    let listed = db.terms_with_posts("category", 10, 0).await.unwrap();
    assert_eq!(listed.iter().map(|t| t.id).collect::<Vec<_>>(), [1]);
    assert!(
        db.terms_with_posts("category", 10, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(db.count_terms_with_posts("category").await.unwrap(), 1);
    assert_eq!(db.count_terms_with_posts("post_tag").await.unwrap(), 0);
}

/// The time conditions over the fixture's post, written at 2026-10-27
/// 08:08:15, a Tuesday: week 43 counted from Sunday, 44 from Monday and 42
/// from Wednesday, as `reference/head/golden.sh` records them.
#[tokio::test]
async fn posts_match_time_conditions() {
    let db = fixture().await;
    let formatted = |format: &str, time: &str| TimeCondition::Formatted {
        format: format.into(),
        time: time.into(),
    };
    let week = |week, start_of_week| TimeCondition::Week {
        week,
        start_of_week,
    };
    for (condition, found) in [
        (TimeCondition::Hour(8), true),
        (TimeCondition::Hour(9), false),
        (TimeCondition::Minute(8), true),
        (TimeCondition::Second(15), true),
        (TimeCondition::Second(16), false),
        (formatted("%H.%i", "8.080000"), true),
        (formatted("0.%i%s", "0.081500"), true),
        (formatted("%H.%i", "8.090000"), false),
        (week(43, 0), true),
        (week(44, 1), true),
        (week(43, 1), false),
        (week(42, 3), true),
        (week(43, 3), false),
    ] {
        let query = PostQuery {
            time: vec![condition.clone()],
            ..PostQuery::default()
        };
        assert_eq!(!titles(&db, query).await.is_empty(), found, "{condition:?}");
    }
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
        order: vec![(OrderBy::Relevance, false), (OrderBy::Date, false)],
        ..both()
    };
    assert_eq!(titles(&db, search(&["hello"])).await, ["Hello world!"]);
    // An exact search matches whole titles only, case aside.
    let exact = |term: &str| PostQuery {
        exact: true,
        ..search(&[term])
    };
    assert!(titles(&db, exact("hello")).await.is_empty());
    assert_eq!(titles(&db, exact("hello WORLD!")).await, ["Hello world!"]);
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
    // A term after `-` excludes.
    assert!(titles(&db, search(&["hello", "-world"])).await.is_empty());
    assert_eq!(titles(&db, search(&["-world"])).await, ["Sample Page"]);
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
    // Every page's posts, however many a page lists.
    let one = PostQuery { limit: 1, ..both() };
    assert_eq!(db.count_posts(&one).await.unwrap(), 2);
    let none = search(&["nothingmatches"]);
    assert_eq!(db.count_posts(&none).await.unwrap(), 0);
    // No types: any type.
    let any = db
        .posts(&PostQuery {
            include: Some(vec![1, 2]),
            limit: 10,
            ..PostQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(any.len(), 2);
    // Wildcards are searched for literally.
    assert!(titles(&db, search(&["%"])).await.is_empty());
    assert!(titles(&db, search(&["_ello"])).await.is_empty());

    assert_eq!(
        titles(
            &db,
            PostQuery {
                terms: vec![TermFilter {
                    taxonomy: "category".into(),
                    terms: vec![1],
                    exclude: false,
                }],
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
                terms: vec![TermFilter {
                    taxonomy: "category".into(),
                    terms: vec![999],
                    exclude: false,
                }],
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

/// A socket `DB_HOST` connects through the socket, whatever the port: the
/// fixture's MariaDB exposes it on a bind mount.
#[tokio::test]
async fn connects_through_a_socket() {
    let config = Database {
        host: "localhost".into(),
        port: 1,
        socket: Some(hyscribe_testkit::fixture_socket()),
        ..fixture_config()
    };
    let db = Db::connect(&config).await.unwrap();
    assert_eq!(
        db.option("blogname").await.unwrap().unwrap(),
        b"HyScribe Default Install"
    );
}

#[tokio::test]
async fn terms_are_searched_paged_and_counted() {
    let db = Db::connect(&fixture_config()).await.unwrap();
    assert_eq!(db.count_terms("category", None).await.unwrap(), 1);
    assert_eq!(db.count_terms("category", Some("CAT")).await.unwrap(), 1);
    assert_eq!(db.count_terms("category", Some("%")).await.unwrap(), 0);
    let query = |search: &str, offset| TermQuery {
        taxonomy: "category".into(),
        search: Some(search.into()),
        order: Some((TermOrder::Name, false)),
        limit: Some(1),
        offset,
        ..TermQuery::default()
    };
    let found = db.term_query(&query("uncat", 0)).await.unwrap();
    assert_eq!(found.iter().map(|term| term.id).collect::<Vec<_>>(), [1]);
    assert!(db.term_query(&query("uncat", 1)).await.unwrap().is_empty());
}

#[tokio::test]
async fn any_comment_is_read() {
    let db = Db::connect(&fixture_config()).await.unwrap();
    assert_eq!(db.any_comment(1).await.unwrap().map(|c| c.id), Some(1));
    assert!(db.any_comment(999).await.unwrap().is_none());
    // Held comments are in the Theme Test Data.
    let ttd = named("wordpress_ttd").await;
    assert_eq!(
        ttd.held_comment(27).await.unwrap().map(|c| c.post_id),
        Some(1148)
    );
    assert!(ttd.held_comment(1).await.unwrap().is_none());
    assert_eq!(db.comment_approved(1).await.unwrap().as_deref(), Some("1"));
    assert_eq!(db.comment_approved(999).await.unwrap(), None);
    assert!(db.has_comment_types(&["", "comment"]).await.unwrap());
    assert!(!db.has_comment_types(&["pingback"]).await.unwrap());
}

/// The fixture's read user may only select; the main user may write.
#[tokio::test]
async fn privileges_are_those_of_the_connected_user() {
    let read = Database {
        read_user: Some("wordpress_read".into()),
        read_password: hyscribe_config::Secret::new("reference-only"),
        ..fixture_config()
    };
    let db = Db::connect(&read).await.unwrap();
    assert_eq!(db.privileges().await.unwrap(), ["SELECT", "USAGE"]);
    let privileges = fixture().await.privileges().await.unwrap();
    assert!(privileges.contains(&"INSERT".to_string()), "{privileges:?}");
    assert!(privileges.contains(&"USAGE".to_string()), "{privileges:?}");
}

#[tokio::test]
async fn a_term_s_taxonomy_is_its_only_one() {
    let db = Db::connect(&fixture_config()).await.unwrap();
    assert_eq!(
        db.term_taxonomy(1).await.unwrap().as_deref(),
        Some("category")
    );
    assert_eq!(db.term_taxonomy(999_999).await.unwrap(), None);
}

/// A call of every query, each failing against tables that do not exist,
/// with every piece the dialect writes.
async fn every_query() -> Vec<DbError> {
    let mut config = fixture_config();
    config.prefix = Prefix::new("missing_").unwrap();
    let db = Db::connect(&config).await.unwrap();
    let dated = PostQuery {
        year: Some(2026),
        month: Some(1),
        day: Some(1),
        time: vec![
            TimeCondition::Hour(1),
            TimeCondition::Minute(1),
            TimeCondition::Second(1),
            TimeCondition::Formatted {
                format: "%H.%i%s".into(),
                time: "1.0101".into(),
            },
            TimeCondition::Week {
                week: 1,
                start_of_week: 3,
            },
        ],
        order: vec![
            (OrderBy::Random, true),
            (OrderBy::Seeded(1), true),
            (OrderBy::Included, true),
            (OrderBy::Parents, true),
            (OrderBy::Names, true),
        ],
        names: vec!["x".into()],
        ..PostQuery::default()
    };
    let slugs = TermQuery {
        slugs: vec![b"x".to_vec()],
        order: Some((TermOrder::Slugs, false)),
        limit: Some(1),
        ..TermQuery::default()
    };
    let include = TermQuery {
        include: vec![1],
        order: Some((TermOrder::Include, false)),
        ..TermQuery::default()
    };
    let comments = CommentQuery {
        order: vec![(hyscribe_model::CommentOrder::Include, false)],
        limit: Some(1),
        ..CommentQuery::default()
    };
    let users = UserQuery {
        order: (hyscribe_model::UserOrder::Include, false),
        ..UserQuery::default()
    };
    let nicenames = UserQuery {
        order: (hyscribe_model::UserOrder::Nicenames, false),
        limit: Some(1),
        ..UserQuery::default()
    };
    let mut errors = vec![
        db.posts(&dated).await.unwrap_err(),
        db.guess_post("x", &["post"], (Some(1), Some(1), Some(1)))
            .await
            .unwrap_err(),
        db.date_counts(Period::Month).await.unwrap_err(),
        db.date_counts(Period::Day).await.unwrap_err(),
        db.date_counts(Period::Week(0)).await.unwrap_err(),
        db.date_counts(Period::Week(1)).await.unwrap_err(),
        db.date_counts(Period::Week(3)).await.unwrap_err(),
        db.term_query(&slugs).await.unwrap_err(),
        db.term_query(&include).await.unwrap_err(),
        db.comment_query(&comments).await.unwrap_err(),
        db.user_query(&users).await.unwrap_err(),
        db.user_query(&nicenames).await.unwrap_err(),
        db.has_posts(1, &[("post".into(), true)]).await.unwrap_err(),
        db.post_months("post", false).await.unwrap_err(),
    ];
    errors.extend([
        db.autoload_options().await.unwrap_err(),
        db.post(1).await.unwrap_err(),
        db.posts(&PostQuery {
            post_types: vec!["post".into()],
            limit: 1,
            ..PostQuery::default()
        })
        .await
        .unwrap_err(),
        db.find_terms(&[], &[1]).await.unwrap_err(),
        db.terms_with_posts("category", 1, 0).await.unwrap_err(),
        db.count_terms_with_posts("category").await.unwrap_err(),
        db.option("blogname").await.unwrap_err(),
        db.post_meta(1, "x").await.unwrap_err(),
        db.post_meta_all(1, "x").await.unwrap_err(),
        db.post_terms(1, "category").await.unwrap_err(),
        db.term(1, "category").await.unwrap_err(),
        db.term_taxonomy(1).await.unwrap_err(),
        db.any_comment(1).await.unwrap_err(),
        db.held_comment(1).await.unwrap_err(),
        db.comment_approved(1).await.unwrap_err(),
        db.has_comment_types(&["comment"]).await.unwrap_err(),
        db.count_terms("category", None).await.unwrap_err(),
        db.user(1).await.unwrap_err(),
        db.credentials(UserField::Login, b"admin")
            .await
            .unwrap_err(),
        db.user_meta(1, "use_ssl").await.unwrap_err(),
        db.list_users(&EVERYONE, &[], 1, 0).await.unwrap_err(),
        db.count_users(&EVERYONE).await.unwrap_err(),
        db.users_without_role("wp_capabilities", "administrator")
            .await
            .unwrap_err(),
        db.users_meta("wp_capabilities").await.unwrap_err(),
        db.post_authors("post").await.unwrap_err(),
        db.owns_content(1).await.unwrap_err(),
        db.posts_of_type("page").await.unwrap_err(),
        db.theme_posts("wp_template", "twentytwentyfive")
            .await
            .unwrap_err(),
        db.revisions(1).await.unwrap_err(),
        db.user_post_counts(&[1], true, 1).await.unwrap_err(),
        db.comments(1).await.unwrap_err(),
        db.older_comments(1, "").await.unwrap_err(),
        db.held_comments(1, b"e", None).await.unwrap_err(),
        db.comment(1).await.unwrap_err(),
        db.adjacent_post(&Post::default(), true, None)
            .await
            .unwrap_err(),
        db.feed_comments(None, 1).await.unwrap_err(),
        db.count_posts(&PostQuery {
            post_types: vec!["post".into()],
            ..PostQuery::default()
        })
        .await
        .unwrap_err(),
        db.last_comment_modified().await.unwrap_err(),
        db.comment_counts(1, 0).await.unwrap_err(),
        db.list_comments(&PENDING, NEWEST, 1, 0).await.unwrap_err(),
        db.count_comments(&PENDING).await.unwrap_err(),
        db.comment_meta(1, "_wp_trash_meta_status")
            .await
            .unwrap_err(),
        db.posts_by_status(&StatusQuery {
            status: "draft",
            author: Some(1),
            order: StatusOrder::DateAsc,
            limit: 1,
        })
        .await
        .unwrap_err(),
        db.last_post_modified().await.unwrap_err(),
        db.date_counts(Period::Year).await.unwrap_err(),
        db.recent_comments(1).await.unwrap_err(),
        db.cloud_terms("post_tag", 1).await.unwrap_err(),
        db.taxonomy_terms("category").await.unwrap_err(),
        db.term_query(&TermQuery::default()).await.unwrap_err(),
        db.has_published_posts("post").await.unwrap_err(),
        db.calendar_month(2026, 10, 31).await.unwrap_err(),
        db.term_posts("category").await.unwrap_err(),
        db.archive_feed_comments(
            &PostQuery {
                post_types: vec!["post".into()],
                ..PostQuery::default()
            },
            1,
        )
        .await
        .unwrap_err(),
        db.status_counts("post", None).await.unwrap_err(),
        db.post_name_taken("hello-world", 2).await.unwrap_err(),
        db.own_posts("post", 1).await.unwrap_err(),
        db.sticky_count("post", &[1]).await.unwrap_err(),
        db.post_months("post", true).await.unwrap_err(),
        db.mime_types("attachment").await.unwrap_err(),
        db.pending_comments(1).await.unwrap_err(),
        db.users_with("wp_capabilities", &["administrator".into()])
            .await
            .unwrap_err(),
        db.comment_query(&CommentQuery::default())
            .await
            .unwrap_err(),
        db.count_comment_query(&CommentQuery::default())
            .await
            .unwrap_err(),
        db.user_query(&UserQuery::default()).await.unwrap_err(),
        db.count_user_query(&UserQuery::default())
            .await
            .unwrap_err(),
        db.user_registered(1).await.unwrap_err(),
    ]);
    errors
}

#[tokio::test]
async fn errors_are_reported() {
    for err in every_query().await {
        assert!(
            [
                "missing_options",
                "missing_posts",
                "missing_terms",
                "missing_term_taxonomy",
                "missing_term_relationships",
                "missing_postmeta",
                "missing_users",
                "missing_usermeta",
                "missing_comments",
                "missing_commentmeta"
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

/// The server's version and the connection's collation; an error once the
/// connections are closed.
#[tokio::test]
async fn the_server_names_itself() {
    let db = fixture().await;
    let (version, collation) = db.server().await.unwrap();
    assert!(version.contains("MariaDB"), "{version}");
    assert!(collation.starts_with("utf8mb4_"), "{collation}");
    db.pool.close().await;
    assert!(db.server().await.is_err());
}

#[tokio::test]
async fn a_row_without_the_columns_fails_to_decode() {
    let db = fixture().await;
    let row = sqlx::query("SELECT 1 AS other")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert!(post(&row).is_err());
    assert!(date_count(&row).is_err());
    assert!(term_id(&row).is_err());
    assert!(people::comment_row(&row).is_err());
    let mut c = Columns::new(&row);
    let _ = (c.text("option_name"), c.bytes("option_value"));
    assert!(c.finish(()).is_err());
}

#[test]
fn order_follows_the_query() {
    let text = |order| {
        posts_sql(&PostQuery {
            order,
            search: vec!["x".into()],
            ..PostQuery::default()
        })
    };
    assert!(
        text(vec![])
            .text
            .ends_with("ORDER BY post_date DESC LIMIT ? OFFSET ?")
    );
    assert!(text(vec![]).text.contains(" AND post_password = ''"));
    let logged_in = PostQuery {
        search: vec!["x".into()],
        logged_in: true,
        ..PostQuery::default()
    };
    assert!(!posts_sql(&logged_in).text.contains("post_password = ''"));
    assert!(
        !posts_sql(&PostQuery::default())
            .text
            .contains("post_password = ''")
    );
    let pages = text(vec![(OrderBy::MenuOrder, true), (OrderBy::Title, true)]);
    assert!(
        pages
            .text
            .ends_with("ORDER BY menu_order ASC, post_title ASC LIMIT ? OFFSET ?")
    );
    // The three search columns, then the limit and offset.
    assert_eq!(pages.binds.len(), 5);
    let relevance = text(vec![(OrderBy::Relevance, false), (OrderBy::Date, true)]);
    assert!(
        relevance
            .text
            .ends_with("ORDER BY post_title LIKE ? DESC, post_date ASC LIMIT ? OFFSET ?")
    );
    assert_eq!(relevance.binds.len(), 6);
    let columns = [
        (OrderBy::Modified, "post_modified"),
        (OrderBy::Name, "post_name"),
        (OrderBy::Author, "post_author"),
        (OrderBy::Parent, "post_parent"),
        (OrderBy::Type, "post_type"),
        (OrderBy::Id, "ID"),
        (OrderBy::CommentCount, "comment_count"),
    ];
    for (column, name) in columns {
        assert_eq!(order_column(column), Some(name));
    }
    let lists = posts_sql(&PostQuery {
        include: Some(vec![5, 3]),
        parents: vec![2],
        order: vec![
            (OrderBy::Included, true),
            (OrderBy::Parents, false),
            (OrderBy::Random, true),
            (OrderBy::Seeded(7), false),
        ],
        ..PostQuery::default()
    });
    assert!(lists.text.ends_with(
        "ORDER BY FIELD(ID, ?, ?) ASC, FIELD(post_parent, ?) DESC, RAND() ASC, RAND(?) DESC LIMIT ? OFFSET ?"
    ));
    let ints: Vec<u64> = lists
        .binds
        .iter()
        .filter_map(|b| match b {
            Bind::Int(n) => Some(*n),
            _ => None,
        })
        .collect();
    // The WHERE parents and IDs (0 first), then the FIELD lists.
    assert_eq!(ints, [2, 0, 5, 3, 5, 3, 2, 7, 0, 0]);
    let empty = posts_sql(&PostQuery {
        order: vec![(OrderBy::Included, true), (OrderBy::Parents, true)],
        ..PostQuery::default()
    });
    assert!(empty.text.contains("ORDER BY post_date ASC LIMIT"));
}

/// `WP_Query`'s `RAND(n)` order of the published posts and pages, as
/// `reference/query-order/golden.sh` records it.
#[tokio::test]
async fn seeded_orders_match_php() {
    let db = fixture().await;
    let golden = include_str!("../tests/golden/rand.tsv");
    for line in golden.lines() {
        let (seed, ids) = line.split_once('\t').unwrap();
        let posts = db
            .posts(&PostQuery {
                post_types: vec!["post".into(), "page".into()],
                order: vec![(OrderBy::Seeded(seed.parse().unwrap()), false)],
                limit: 10,
                ..PostQuery::default()
            })
            .await
            .unwrap();
        let ours: Vec<String> = posts.iter().map(|p| p.id.to_string()).collect();
        assert_eq!(ours.join(","), ids, "seed {seed}");
    }
    assert_eq!(golden.lines().count(), 4);
}

#[tokio::test]
async fn author_parent_and_term_filters() {
    let db = fixture().await;
    let query = |query: PostQuery| PostQuery {
        post_types: vec!["post".into(), "page".into()],
        order: vec![(OrderBy::Title, true)],
        limit: 10,
        ..query
    };
    let filter = |taxonomy: &str, terms: &[u64], exclude| TermFilter {
        taxonomy: taxonomy.into(),
        terms: terms.to_vec(),
        exclude,
    };
    let cases = [
        (PostQuery::default(), &["Hello world!", "Sample Page"][..]),
        (
            PostQuery {
                authors: vec![1],
                ..PostQuery::default()
            },
            &["Hello world!", "Sample Page"],
        ),
        (
            PostQuery {
                authors: vec![2, 3],
                ..PostQuery::default()
            },
            &[],
        ),
        (
            PostQuery {
                parents: vec![0],
                ..PostQuery::default()
            },
            &["Hello world!", "Sample Page"],
        ),
        (
            PostQuery {
                parents: vec![1],
                ..PostQuery::default()
            },
            &[],
        ),
        (
            PostQuery {
                terms: vec![filter("category", &[1], false)],
                ..PostQuery::default()
            },
            &["Hello world!"],
        ),
        (
            PostQuery {
                terms: vec![filter("category", &[1], true)],
                ..PostQuery::default()
            },
            &["Sample Page"],
        ),
        // Term 1 is no tag: IN finds nothing, NOT IN leaves everything.
        (
            PostQuery {
                terms: vec![filter("post_tag", &[1], false)],
                ..PostQuery::default()
            },
            &[],
        ),
        (
            PostQuery {
                terms: vec![filter("post_tag", &[], true)],
                ..PostQuery::default()
            },
            &["Hello world!", "Sample Page"],
        ),
    ];
    for (case, expected) in cases {
        let posts = db.posts(&query(case.clone())).await.unwrap();
        let titles: Vec<_> = posts
            .iter()
            .map(|p| String::from_utf8_lossy(&p.title).into_owned())
            .collect();
        assert_eq!(titles, expected, "{case:?}");
    }
    let descending = db
        .posts(&PostQuery {
            order: vec![(OrderBy::Title, false)],
            ..query(PostQuery::default())
        })
        .await
        .unwrap();
    assert_eq!(descending[0].title, b"Sample Page");
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
    // Logins and emails match as the collation compares, case aside.
    let login = db
        .credentials(UserField::Login, b"ADMIN")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((login.id, &login.login[..]), (1, &b"admin"[..]));
    assert!(login.pass.starts_with(b"$wp$2y$"));
    let email = db
        .credentials(UserField::Email, b"Admin@Default-Install.test")
        .await
        .unwrap();
    assert_eq!(email.as_ref(), Some(&login));
    assert_eq!(
        db.credentials(UserField::Login, b"nobody").await.unwrap(),
        None
    );
    assert_eq!(
        db.credentials(UserField::Email, b"admin").await.unwrap(),
        None
    );
    let caps = db
        .user_meta(1, &format!("{}capabilities", db.prefix()))
        .await
        .unwrap();
    assert_eq!(
        caps.as_deref(),
        Some(&b"a:1:{s:13:\"administrator\";b:1;}"[..])
    );
    assert_eq!(
        db.user_meta(1, "use_ssl").await.unwrap().as_deref(),
        Some(&b"0"[..])
    );
    assert_eq!(db.user_meta(1, "no_such_meta").await.unwrap(), None);
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
    // The fixture's comment is from 2026-10-27 08:08:15.
    assert_eq!(
        db.older_comments(1, "2026-10-27 08:08:16").await.unwrap(),
        1
    );
    assert_eq!(
        db.older_comments(1, "2026-10-27 08:08:15").await.unwrap(),
        0
    );
    assert_eq!(
        db.older_comments(2, "2027-01-01 00:00:00").await.unwrap(),
        0
    );
    // Nothing is held in the default install.
    assert!(
        db.held_comments(1, b"wapuu@wordpress.example", None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.held_comments(1, b"x", Some(-1))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.post_meta(2, "_wp_page_template").await.unwrap(),
        Some(b"default".to_vec())
    );
    assert_eq!(db.post_meta(1, "_thumbnail_id").await.unwrap(), None);
    assert_eq!(
        db.post_meta_all(2, "_wp_page_template").await.unwrap(),
        [b"default".to_vec()]
    );
    assert!(
        db.post_meta_all(1, "_thumbnail_id")
            .await
            .unwrap()
            .is_empty()
    );
    let terms = db.post_terms(1, "category").await.unwrap();
    assert_eq!(
        terms,
        [Term {
            id: 1,
            name: b"Uncategorized".to_vec(),
            slug: b"uncategorized".to_vec(),
            description: Vec::new(),
            parent: 0,
            count: 1,
        }]
    );
    assert!(db.post_terms(1, "post_tag").await.unwrap().is_empty());
    assert_eq!(
        db.term(1, "category").await.unwrap(),
        terms.into_iter().next()
    );
    assert_eq!(db.term(1, "post_tag").await.unwrap(), None);
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

/// The terms fixture adds a post a day after the default install's.
#[tokio::test]
async fn date_counts_group_published_posts() {
    let count = |year, month, day, posts| DateCount {
        year,
        month,
        day,
        posts,
        week: 0,
    };
    let week = |week, day, posts| DateCount {
        week,
        ..count(2026, 10, day, posts)
    };
    let db = fixture().await;
    assert_eq!(
        db.date_counts(Period::Month).await.unwrap(),
        [count(2026, 10, 0, 1)]
    );
    let db = Db::connect(&Database {
        name: "wordpress_terms".into(),
        ..fixture_config()
    })
    .await
    .unwrap();
    assert_eq!(
        db.date_counts(Period::Year).await.unwrap(),
        [count(2026, 0, 0, 2)]
    );
    // October has the two posts; November follows it, before nothing.
    assert!(db.has_published_posts("post").await.unwrap());
    assert!(!db.has_published_posts("attachment").await.unwrap());
    let october = db.calendar_month(2026, 10, 31).await.unwrap();
    assert_eq!(
        (october.previous, october.next, &october.days[..]),
        (None, None, &[27, 28][..])
    );
    let november = db.calendar_month(2026, 11, 30).await.unwrap();
    assert_eq!((november.previous, november.next), (Some((2026, 10)), None));
    // A month past 12 makes MariaDB's dates the zero date, as get_calendar()'s
    // literals do in PHP: nothing before, the first post after.
    let thirteenth = db.calendar_month(2026, 13, 31).await.unwrap();
    assert_eq!(
        (thirteenth.previous, thirteenth.next, thirteenth.days.len()),
        (None, Some((2026, 10)), 0)
    );
    let september = db.calendar_month(2026, 9, 30).await.unwrap();
    assert_eq!(
        (september.next, september.days.is_empty()),
        (Some((2026, 10)), true)
    );
    // The tag `foo` has a post, `empty` none.
    let tags = db.cloud_terms("post_tag", 45).await.unwrap();
    assert_eq!(
        tags.iter()
            .map(|t| (&t.slug[..], t.count))
            .collect::<Vec<_>>(),
        [(&b"foo"[..], 1)]
    );
    assert!(db.cloud_terms("post_tag", 0).await.unwrap().is_empty());
    // Every tag by name, the empty one too, and the posts in them.
    let tags = db.taxonomy_terms("post_tag").await.unwrap();
    assert_eq!(
        tags.iter()
            .map(|t| (&t.slug[..], t.count))
            .collect::<Vec<_>>(),
        [(&b"empty"[..], 0), (&b"foo"[..], 1)]
    );
    assert_eq!(db.term_posts("post_tag").await.unwrap(), [(2, 1)]);
    assert_eq!(
        db.date_counts(Period::Day).await.unwrap(),
        [count(2026, 10, 28, 1), count(2026, 10, 27, 1)]
    );
    // Tuesday the 27th and Wednesday the 28th: weeks from Sunday (and
    // from 7, which MySQL numbers as Sunday's), from Monday, and from
    // Wednesday, which splits them.
    for start in [0, 7] {
        assert_eq!(
            db.date_counts(Period::Week(start)).await.unwrap(),
            [week(43, 28, 2)]
        );
    }
    assert_eq!(
        db.date_counts(Period::Week(1)).await.unwrap(),
        [week(44, 28, 2)]
    );
    assert_eq!(
        db.date_counts(Period::Week(3)).await.unwrap(),
        [week(43, 28, 1), week(42, 27, 1)]
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

#[tokio::test]
async fn feed_comments() {
    let db = fixture().await;
    let ids =
        |comments: Vec<hyscribe_model::Comment>| comments.iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(db.feed_comments(None, 10).await.unwrap()), [1]);
    assert_eq!(ids(db.feed_comments(Some(1), 10).await.unwrap()), [1]);
    assert!(db.feed_comments(Some(2), 10).await.unwrap().is_empty());
    assert!(db.feed_comments(None, 0).await.unwrap().is_empty());
    assert_eq!(ids(db.recent_comments(5).await.unwrap()), [1]);
    assert!(db.recent_comments(0).await.unwrap().is_empty());
    assert_eq!(
        db.last_comment_modified().await.unwrap().as_deref(),
        Some("2026-10-27 08:08:15")
    );
}

#[tokio::test]
async fn archive_feed_comments() {
    let db = fixture().await;
    let ids =
        |comments: Vec<hyscribe_model::Comment>| comments.iter().map(|c| c.id).collect::<Vec<_>>();
    let query = |search: &[&str], category: Option<u64>| PostQuery {
        post_types: vec!["post".into(), "page".into()],
        search: search.iter().map(|s| (*s).to_owned()).collect(),
        terms: category
            .map(|id| TermFilter {
                taxonomy: "category".into(),
                terms: vec![id],
                exclude: false,
            })
            .into_iter()
            .collect(),
        ..PostQuery::default()
    };
    assert_eq!(
        ids(db
            .archive_feed_comments(&query(&["hello"], None), 10)
            .await
            .unwrap()),
        [1]
    );
    assert_eq!(
        ids(db
            .archive_feed_comments(&query(&[], Some(1)), 10)
            .await
            .unwrap()),
        [1]
    );
    assert!(
        db.archive_feed_comments(&query(&["nothingmatches"], None), 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        db.archive_feed_comments(&query(&[], Some(1)), 0)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn sticky_posts_come_first_and_formats_filter() {
    let db = fixture().await;
    let titles = |posts: Vec<Post>| {
        posts
            .iter()
            .map(|p| String::from_utf8_lossy(&p.title).into_owned())
            .collect::<Vec<_>>()
    };
    let query = |query: PostQuery| PostQuery {
        post_types: vec!["post".into(), "page".into()],
        order: vec![(OrderBy::Title, true)],
        ..query
    };
    let cases = [
        (
            PostQuery {
                limit: 10,
                sticky: vec![2],
                ..PostQuery::default()
            },
            &["Sample Page", "Hello world!"][..],
        ),
        // Missing from the page, so fetched.
        (
            PostQuery {
                limit: 1,
                sticky: vec![2],
                ..PostQuery::default()
            },
            &["Sample Page", "Hello world!"],
        ),
        (
            PostQuery {
                limit: 1,
                sticky: vec![2],
                exclude: vec![2],
                ..PostQuery::default()
            },
            &["Hello world!"],
        ),
        (
            PostQuery {
                limit: 10,
                sticky: vec![999],
                ..PostQuery::default()
            },
            &["Hello world!", "Sample Page"],
        ),
        (
            PostQuery {
                limit: 10,
                formats: Some(FormatFilter {
                    standard: true,
                    slugs: vec![],
                }),
                ..PostQuery::default()
            },
            &["Hello world!", "Sample Page"],
        ),
        (
            PostQuery {
                limit: 10,
                formats: Some(FormatFilter {
                    standard: false,
                    slugs: vec!["post-format-aside".into()],
                }),
                ..PostQuery::default()
            },
            &[],
        ),
        (
            PostQuery {
                limit: 10,
                formats: Some(FormatFilter {
                    standard: true,
                    slugs: vec!["post-format-aside".into()],
                }),
                ..PostQuery::default()
            },
            &["Hello world!", "Sample Page"],
        ),
    ];
    for (case, expected) in cases {
        assert_eq!(
            titles(db.posts(&query(case.clone())).await.unwrap()),
            expected,
            "{case:?}"
        );
    }
    // A sticky page is not fetched for a list of posts.
    let posts_only = PostQuery {
        post_types: vec!["post".into()],
        limit: 10,
        sticky: vec![2],
        ..PostQuery::default()
    };
    assert_eq!(
        titles(db.posts(&posts_only).await.unwrap()),
        ["Hello world!"]
    );
    // Sticky posts do not count toward `found_posts`.
    let one = query(PostQuery {
        limit: 1,
        sticky: vec![2],
        ..PostQuery::default()
    });
    assert_eq!(db.count_posts(&one).await.unwrap(), 2);
}

#[tokio::test]
async fn posts_by_name_path_and_guess() {
    let db = fixture().await;
    let id = |post: Option<Post>| post.map(|p| p.id);
    assert_eq!(id(db.post_by_name("hello-world").await.unwrap()), Some(1));
    // A page is no post, and a name must match whole.
    assert_eq!(id(db.post_by_name("sample-page").await.unwrap()), None);
    assert_eq!(id(db.post_by_name("hello").await.unwrap()), None);
    // Pages of any status, the draft privacy policy among them.
    let names = [
        b"sample-page".to_vec(),
        b"privacy-policy".to_vec(),
        b"hello-world".to_vec(),
    ];
    let mut pages: Vec<_> = db
        .named_pages(&names)
        .await
        .unwrap()
        .iter()
        .map(|p| (p.id, p.status.clone()))
        .collect();
    pages.sort();
    assert_eq!(
        pages,
        [(2, "publish".to_string()), (3, "draft".to_string())]
    );
    assert!(db.named_pages(&[]).await.unwrap().is_empty());
    let guess = |name, types: &'static [&'static str], year| {
        let db = db.clone();
        async move {
            id(db
                .guess_post(name, types, (year, None, None))
                .await
                .unwrap())
        }
    };
    assert_eq!(guess("hel", &["post", "page"], None).await, Some(1));
    assert_eq!(guess("sam", &["post", "page"], None).await, Some(2));
    assert_eq!(guess("sam", &["post"], None).await, None);
    assert_eq!(guess("hel", &["post"], Some(2026)).await, Some(1));
    assert_eq!(guess("hel", &["post"], Some(1999)).await, None);
    // Drafts are not guessed, and `%` is no wildcard.
    assert_eq!(guess("privacy", &["page"], None).await, None);
    assert_eq!(guess("%", &["post", "page"], None).await, None);
    let date = db
        .guess_post("hel", &["post"], (Some(2026), Some(10), Some(27)))
        .await
        .unwrap();
    assert_eq!(id(date), Some(1));
    let date = db
        .guess_post("hel", &["post"], (None, Some(10), Some(28)))
        .await
        .unwrap();
    assert_eq!(id(date), None);
}

/// `author__not_in`: the default install's posts are all by user 1.
#[tokio::test]
async fn excluded_authors_leave_their_posts_out() {
    let db = fixture().await;
    let query = |excluded_authors: Vec<u64>| PostQuery {
        post_types: vec!["post".into(), "page".into()],
        excluded_authors,
        limit: 10,
        ..PostQuery::default()
    };
    assert!(db.posts(&query(vec![1])).await.unwrap().is_empty());
    assert_eq!(db.posts(&query(vec![2, 3])).await.unwrap().len(), 2);
}

/// Every order and clause of a term query is valid SQL; the one category
/// comes back.
#[tokio::test]
async fn term_queries_order_by_any_column() {
    let db = fixture().await;
    for order in [
        TermOrder::TermId,
        TermOrder::Name,
        TermOrder::Slug,
        TermOrder::TermGroup,
        TermOrder::Count,
        TermOrder::Parent,
        TermOrder::Taxonomy,
        TermOrder::TermTaxonomyId,
        TermOrder::Description,
        TermOrder::Include,
    ] {
        let query = TermQuery {
            taxonomy: "category".into(),
            include: vec![1, 2],
            parent: Some(0),
            nonempty: true,
            order: Some((order, order == TermOrder::Name)),
            limit: Some(5),
            ..TermQuery::default()
        };
        let terms = db.term_query(&query).await.unwrap();
        assert_eq!(
            terms.iter().map(|t| t.id).collect::<Vec<_>>(),
            [1],
            "{order:?}"
        );
    }
}

/// The REST API's term queries: excluded terms, slugs in their order, the
/// terms of a post, and their count.
#[tokio::test]
async fn term_queries_take_the_rest_arguments() {
    let db = fixture().await;
    let ids = |terms: Vec<Term>| terms.iter().map(|t| t.id).collect::<Vec<_>>();
    let query = TermQuery {
        taxonomy: "category".into(),
        slugs: vec![b"x".to_vec(), b"uncategorized".to_vec()],
        objects: vec![1],
        order: Some((TermOrder::Slugs, true)),
        ..TermQuery::default()
    };
    assert_eq!(ids(db.term_query(&query).await.unwrap()), [1]);
    assert_eq!(db.term_query_count(&query).await.unwrap(), 1);
    let excluded = TermQuery {
        exclude: vec![1],
        ..query.clone()
    };
    assert_eq!(
        ids(db.term_query(&excluded).await.unwrap()),
        Vec::<u64>::new()
    );
    assert_eq!(db.term_query_count(&excluded).await.unwrap(), 0);
    let elsewhere = TermQuery {
        objects: vec![2],
        ..query
    };
    assert_eq!(db.term_query_count(&elsewhere).await.unwrap(), 0);
}

/// A term query over several taxonomies finds the terms of each.
#[tokio::test]
async fn term_queries_take_several_taxonomies() {
    let db = named("wordpress_ttd").await;
    let count = |taxonomy: &str, taxonomies: Vec<String>| TermQuery {
        taxonomy: taxonomy.into(),
        taxonomies,
        order: Some((TermOrder::Name, false)),
        ..TermQuery::default()
    };
    let categories = db.term_query_count(&count("category", Vec::new())).await;
    let tags = db.term_query_count(&count("post_tag", Vec::new())).await;
    let both = count("category", vec!["post_tag".into()]);
    let total = categories.unwrap() + tags.unwrap();
    assert_eq!(db.term_query_count(&both).await.unwrap(), total);
    let found = db.term_query(&both).await.unwrap();
    assert_eq!(found.len() as u64, total);
}

/// The order of an empty list, for which PHP writes no `FIELD()`: terms
/// sort by name, and a post order leaves the key out, the date alone.
#[tokio::test]
async fn empty_list_orders_sort_as_php() {
    let db = named("wordpress_ttd").await;
    let ids = |terms: Vec<Term>| terms.iter().map(|t| t.id).collect::<Vec<_>>();
    let terms = |order| TermQuery {
        taxonomy: "category".into(),
        order: Some((order, true)),
        limit: Some(5),
        ..TermQuery::default()
    };
    let named = db.term_query(&terms(TermOrder::Name)).await.unwrap();
    let by_name = ids(named.clone());
    for order in [TermOrder::Include, TermOrder::Slugs] {
        let listed = db.term_query(&terms(order)).await.unwrap();
        assert_eq!(ids(listed), by_name, "{order:?}");
    }
    // Lists that are not empty keep their own order, here neither name order.
    let mut reversed: Vec<u64> = by_name.iter().rev().copied().collect();
    reversed.swap(0, 1);
    let included = TermQuery {
        include: reversed.clone(),
        order: Some((TermOrder::Include, false)),
        ..terms(TermOrder::Include)
    };
    assert_eq!(ids(db.term_query(&included).await.unwrap()), reversed);
    let slugged = TermQuery {
        slugs: reversed
            .iter()
            .map(|id| named.iter().find(|t| t.id == *id).unwrap().slug.clone())
            .collect(),
        order: Some((TermOrder::Slugs, false)),
        ..terms(TermOrder::Slugs)
    };
    assert_eq!(ids(db.term_query(&slugged).await.unwrap()), reversed);
    let posts = |order| PostQuery {
        post_types: vec!["post".into()],
        order,
        limit: 5,
        ..PostQuery::default()
    };
    let by_date = db.posts(&posts(vec![(OrderBy::Date, true)])).await.unwrap();
    for list in [OrderBy::Included, OrderBy::Parents, OrderBy::Names] {
        let listed = db.posts(&posts(vec![(list, true)])).await.unwrap();
        assert_eq!(listed, by_date, "{list:?}");
        let after = db
            .posts(&posts(vec![(list, false), (OrderBy::Date, true)]))
            .await
            .unwrap();
        assert_eq!(after, by_date, "{list:?}");
    }
}

/// The dashboard's comment counts, latest comments and posts by status,
/// over the three fixtures.
#[tokio::test]
async fn dashboard_reads() {
    let db = fixture().await;
    assert_eq!(db.comment_counts(1, 0).await.unwrap(), [1, 0, 0, 0, 0]);
    let by_login = [("user_login", false), ("x", true)];
    let users = db.list_users(&EVERYONE, &by_login, 5, 0).await.unwrap();
    assert_eq!(users.iter().map(|u| u.id).collect::<Vec<_>>(), [1]);
    assert!(
        db.list_users(&EVERYONE, &by_login, 5, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(db.count_users(&EVERYONE).await.unwrap(), 1);
    let filtered = |roles: &'static [String], include: &'static [u64], search| UserFilter {
        roles,
        include,
        search,
        ..EVERYONE
    };
    let editors: &'static [String] = Box::leak(vec!["editor".to_string()].into_boxed_slice());
    assert_eq!(
        db.count_users(&filtered(editors, &[], None)).await.unwrap(),
        0
    );
    assert_eq!(db.count_users(&filtered(&[], &[2], None)).await.unwrap(), 0);
    let by_id: &'static [&'static str] = &["ID", "user_login", "bogus"];
    assert_eq!(
        db.count_users(&filtered(&[], &[], Some((by_id, "1"))))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.count_users(&filtered(&[], &[], Some((&["bogus"], "1"))))
            .await
            .unwrap(),
        1
    );
    let by_posts = [("post_count", true)];
    assert_eq!(
        db.list_users(&EVERYONE, &by_posts, 5, 0)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(db.list_users(&EVERYONE, &[], 5, 0).await.unwrap().len() == 1);
    assert!(
        db.users_without_role("wp_capabilities", "administrator")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.users_without_role("wp_capabilities", "editor")
            .await
            .unwrap(),
        [1]
    );
    let caps = db.users_meta("wp_capabilities").await.unwrap();
    assert_eq!(caps, [(1, b"a:1:{s:13:\"administrator\";b:1;}".to_vec())]);
    assert_eq!(db.post_authors("page").await.unwrap(), [1]);
    assert!(db.post_authors("attachment").await.unwrap().is_empty());
    assert!(db.owns_content(1).await.unwrap() && !db.owns_content(2).await.unwrap());
    assert_eq!(db.user_post_counts(&[1], false, 0).await.unwrap(), [(1, 1)]);
    let latest = db.list_comments(&PENDING, NEWEST, 5, 0).await.unwrap();
    let ids: Vec<(u64, String)> = latest.into_iter().map(|(c, a)| (c.id, a)).collect();
    assert_eq!(ids, [(1, "1".to_string())]);
    assert!(
        db.list_comments(&PENDING, NEWEST, 5, 1)
            .await
            .unwrap()
            .is_empty()
    );
    let user = |user| CommentFilter {
        approved: &["1"],
        user: Some(user),
        ..PENDING
    };
    assert_eq!(db.count_comments(&user(0)).await.unwrap(), 1);
    assert_eq!(db.count_comments(&user(-1)).await.unwrap(), 0);
    assert!(
        db.list_comments(&user(-1), NEWEST, 5, 0)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        db.comment_meta(1, "_wp_trash_meta_status").await.unwrap(),
        None
    );
    let ttd = named("wordpress_ttd").await;
    let ids = |users: Vec<User>| users.iter().map(|u| u.id).collect::<Vec<_>>();
    let by_email = [("user_email", false), ("ID", true)];
    let sorted = ttd.list_users(&EVERYONE, &by_email, 5, 0).await.unwrap();
    assert_eq!(ids(sorted), [1, 3, 2]);
    let searched = |columns: &'static [&'static str], text: &'static str| UserFilter {
        search: Some((columns, text)),
        ..EVERYONE
    };
    assert_eq!(
        ttd.count_users(&searched(&["user_email"], "gmail"))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        ttd.count_users(&searched(&["user_login"], "theme"))
            .await
            .unwrap(),
        2
    );
    assert_eq!(ttd.count_users(&searched(&["ID"], "2")).await.unwrap(), 1);
    let [approved, moderated, spam, trash, _] = ttd.comment_counts(1, 0).await.unwrap();
    assert_eq!((approved, moderated, spam, trash), (31, 3, 0, 0));
    let [approved, moderated, ..] = ttd.comment_counts(1, 1148).await.unwrap();
    assert_eq!((approved, moderated), (19, 1));
    let on_post = CommentFilter {
        post: 155,
        ..PENDING
    };
    assert_eq!(ttd.count_comments(&on_post).await.unwrap(), 4);
    let approved_only = CommentFilter {
        approved: &["1"],
        ..PENDING
    };
    let listed = ttd
        .list_comments(&approved_only, NEWEST, 50, 0)
        .await
        .unwrap();
    assert!(listed.iter().all(|(_, a)| a == "1"));
    assert_eq!(ttd.count_comments(&approved_only).await.unwrap(), 31);
    assert_eq!(
        ttd.list_comments(&PENDING, NEWEST, 50, 0)
            .await
            .unwrap()
            .len(),
        34
    );
    let pings = CommentFilter {
        types: &["pingback", "trackback"],
        ..PENDING
    };
    assert_eq!(ttd.count_comments(&pings).await.unwrap(), 4);
    let searched = CommentFilter {
        search: Some("anon"),
        ..PENDING
    };
    assert_eq!(ttd.count_comments(&searched).await.unwrap(), 2);
    // Unknown columns are left out of the order, which may be empty.
    let by_author = [("comment_author", false), ("x", true), ("comment_ID", true)];
    let sorted = ttd.list_comments(&PENDING, &by_author, 2, 0).await.unwrap();
    let ids: Vec<u64> = sorted.iter().map(|(c, _)| c.id).collect();
    assert_eq!(ids, [1, 3]);
    assert_eq!(
        ttd.list_comments(&PENDING, &[], 50, 0).await.unwrap().len(),
        34
    );
    let terms = named("wordpress_terms").await;
    let ids = |posts: Vec<Post>| posts.iter().map(|p| p.id).collect::<Vec<_>>();
    // Every status, newest first: the auto-draft 103 is as new as 101.
    let posts = terms.posts_of_type("post").await.unwrap();
    assert!(posts.windows(2).all(|pair| pair[0].date >= pair[1].date));
    let mut found = ids(posts);
    found.sort_unstable();
    assert_eq!(found, [1, 101, 103]);
    assert_eq!(ids(terms.posts_of_type("wp_block").await.unwrap()), [102]);
    // Newest first, the later saved first of those saved together.
    assert_eq!(terms.revisions(1).await.unwrap(), [105, 104]);
    assert!(terms.revisions(2).await.unwrap().is_empty());
    let query = |order, author| StatusQuery {
        status: "publish",
        author,
        order,
        limit: 5,
    };
    assert_eq!(
        ids(terms
            .posts_by_status(&query(StatusOrder::DateDesc, None))
            .await
            .unwrap()),
        [101, 1]
    );
    assert_eq!(
        ids(terms
            .posts_by_status(&query(StatusOrder::DateAsc, Some(1)))
            .await
            .unwrap()),
        [1, 101]
    );
    assert_eq!(
        ids(terms
            .posts_by_status(&query(StatusOrder::ModifiedDesc, None))
            .await
            .unwrap()),
        [101, 1]
    );
    assert!(
        terms
            .posts_by_status(&query(StatusOrder::DateAsc, Some(2)))
            .await
            .unwrap()
            .is_empty()
    );
}

/// The posts list's counts and lists, over the Theme Test Data.
#[tokio::test]
async fn posts_list_reads() {
    let ttd = named("wordpress_ttd").await;
    let mut counts = ttd.status_counts("post", None).await.unwrap();
    counts.sort();
    let counts: Vec<(&str, u64)> = counts.iter().map(|(s, n)| (s.as_str(), *n)).collect();
    assert_eq!(counts, [("draft", 1), ("future", 1), ("publish", 57)]);
    assert_eq!(ttd.status_counts("page", Some(2)).await.unwrap().len(), 2);
    let named = ttd
        .named_status_counts("post", "hello-world")
        .await
        .unwrap();
    assert_eq!(named, [("publish".to_string(), 1)]);
    assert!(
        ttd.named_status_counts("page", "hello-world")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(ttd.own_posts("post", 2).await.unwrap(), 39);
    assert_eq!(ttd.sticky_count("post", &[1241, 155]).await.unwrap(), 1);
    let months = ttd.post_months("post", false).await.unwrap();
    assert_eq!(months.get(..2), Some(&[(2030, 1), (2026, 11)][..]));
    assert_eq!(ttd.pending_comments(155).await.unwrap(), 1);
    assert_eq!(ttd.pending_comments(1241).await.unwrap(), 0);
    let ids = |users: Vec<User>| users.iter().map(|u| u.id).collect::<Vec<_>>();
    let editors = ["edit_posts".to_string(), "administrator".to_string()];
    let admins = ttd.users_with("wp_capabilities", &editors).await.unwrap();
    assert_eq!(ids(admins), [1]);
    let readers = ["subscriber".to_string()];
    let subscribers = ttd.users_with("wp_capabilities", &readers).await.unwrap();
    assert_eq!(ids(subscribers), [2, 3]);
    let all = PostQuery {
        post_types: vec!["post".into()],
        statuses: ["publish", "future", "draft", "pending", "private"]
            .iter()
            .map(|status| StatusFilter {
                status: (*status).into(),
                author: None,
            })
            .collect(),
        limit: 100,
        ..PostQuery::default()
    };
    assert_eq!(ttd.count_posts(&all).await.unwrap(), 59);
    let drafts_by = |author| PostQuery {
        post_types: vec!["page".into()],
        statuses: vec![StatusFilter {
            status: "draft".into(),
            author,
        }],
        limit: 5,
        ..PostQuery::default()
    };
    let page_ids = |posts: Vec<Post>| posts.iter().map(|p| p.id).collect::<Vec<_>>();
    assert_eq!(page_ids(ttd.posts(&drafts_by(Some(1))).await.unwrap()), [3]);
    assert!(ttd.posts(&drafts_by(Some(2))).await.unwrap().is_empty());
}

/// A search's relevance order is `parse_search_order()`'s: WordPress's own
/// SQL for it, run here, sorts the Theme Test Data's posts the same.
#[tokio::test]
async fn searches_sort_by_relevance() {
    let ttd = named("wordpress_ttd").await;
    for (terms, sentence, case) in [
        (
            vec!["image", "alignment"],
            "image alignment",
            "CASE WHEN post_title LIKE '%image alignment%' THEN 1 \
             WHEN post_title LIKE '%image%' AND post_title LIKE '%alignment%' THEN 2 \
             WHEN post_title LIKE '%image%' OR post_title LIKE '%alignment%' THEN 3 \
             WHEN post_excerpt LIKE '%image alignment%' THEN 4 \
             WHEN post_content LIKE '%image alignment%' THEN 5 ELSE 6 END",
        ),
        // Seven terms rank only as a phrase, though one title has a term.
        (
            vec!["wp:", "class", "figure", "size", "src", "alt", "image"],
            "wp: class figure size src alt image",
            "CASE WHEN post_title LIKE '%wp: class figure size src alt image%' THEN 1 \
             WHEN post_excerpt LIKE '%wp: class figure size src alt image%' THEN 4 \
             WHEN post_content LIKE '%wp: class figure size src alt image%' THEN 5 ELSE 6 END",
        ),
        (
            vec!["image", "-gallery"],
            "image -gallery",
            "CASE WHEN post_title LIKE '%image%' THEN 2 ELSE 6 END",
        ),
        // A stopword dropped still counts as a word.
        (
            vec!["image"],
            "the image",
            "CASE WHEN post_title LIKE '%the image%' THEN 1 \
             WHEN post_title LIKE '%image%' THEN 2 \
             WHEN post_excerpt LIKE '%the image%' THEN 4 \
             WHEN post_content LIKE '%the image%' THEN 5 ELSE 6 END",
        ),
        // Seven terms and one left out leave nothing to rank by.
        (
            vec![
                "image", "block", "wp:", "class", "figure", "size", "src", "-xyzzy",
            ],
            "image block wp: class figure size src -xyzzy",
            "NULL",
        ),
    ] {
        let query = PostQuery {
            post_types: vec!["post".into()],
            search: terms.iter().map(|term| (*term).to_string()).collect(),
            sentence: sentence.into(),
            search_count: sentence.split(' ').count(),
            order: vec![(OrderBy::Relevance, false), (OrderBy::Date, false)],
            limit: 50,
            ..PostQuery::default()
        };
        let ours: Vec<u64> = ttd
            .posts(&query)
            .await
            .unwrap()
            .iter()
            .map(|p| p.id)
            .collect();
        let found = |term: &str| match term.strip_prefix('-') {
            Some(term) => format!(
                "post_title NOT LIKE '%{term}%' AND post_excerpt NOT LIKE '%{term}%' AND post_content NOT LIKE '%{term}%'"
            ),
            None => format!(
                "(post_title LIKE '%{term}%' OR post_excerpt LIKE '%{term}%' OR post_content LIKE '%{term}%')"
            ),
        };
        let filters: Vec<String> = terms.iter().map(|term| found(term)).collect();
        let sql = format!(
            "SELECT ID FROM wp_posts WHERE post_status = 'publish' AND post_type = 'post' AND {} \
             ORDER BY ({case}), post_date DESC LIMIT 50",
            filters.join(" AND ")
        );
        // Test SQL built from the constants above.
        let theirs: Vec<u64> = sqlx::query_scalar::<_, u64>(sqlx::AssertSqlSafe(sql))
            .fetch_all(&ttd.pool)
            .await
            .unwrap();
        assert!(theirs.len() > 1, "{sentence}");
        assert_eq!(ours, theirs, "{sentence}");
    }
}

/// The Theme Test Data's attachments by type and by file name, as the
/// media library asks for them.
#[tokio::test]
async fn attachments_by_type_and_file() {
    let ttd = named("wordpress_ttd").await;
    let mut types = ttd.mime_types("attachment").await.unwrap();
    types.sort();
    assert_eq!(
        types,
        ["audio/mpeg", "image/gif", "image/jpeg", "video/quicktime"]
    );
    let count = |mime_types: &[&str], search: &[&str], filenames: bool| {
        let query = PostQuery {
            post_types: vec!["attachment".into()],
            statuses: vec![StatusFilter {
                status: "inherit".into(),
                author: None,
            }],
            mime_types: mime_types.iter().map(|t| t.to_string()).collect(),
            search: search.iter().map(|t| t.to_string()).collect(),
            filenames,
            ..PostQuery::default()
        };
        let ttd = &ttd;
        async move { ttd.count_posts(&query).await.unwrap() }
    };
    assert_eq!(count(&[], &[], false).await, 37);
    assert_eq!(count(&["image/%"], &[], false).await, 35);
    assert_eq!(count(&["audio/%", "video/quicktime"], &[], false).await, 2);
    assert_eq!(count(&["video/mp4"], &[], false).await, 0);
    // The files' names hold `jpg` more often than the text.
    assert_eq!(count(&[], &["jpg"], false).await, 15);
    assert_eq!(count(&[], &["jpg"], true).await, 34);
    assert_eq!(count(&[], &["-jpg"], true).await, 3);
}

/// The REST API's filters: slugs and their order, parents left out, the
/// menu order, and the columns a search looks in.
#[tokio::test]
async fn rest_post_filters() {
    let db = fixture().await;
    let status = |name: &str| StatusFilter {
        status: name.into(),
        author: None,
    };
    let pages = || PostQuery {
        post_types: vec!["page".into()],
        statuses: vec![status("publish"), status("draft")],
        order: vec![(OrderBy::Id, true)],
        ..PostQuery::default()
    };
    let named = PostQuery {
        names: vec!["sample-page".into(), "privacy-policy".into()],
        order: vec![(OrderBy::Names, true)],
        ..pages()
    };
    assert_eq!(titles(&db, named).await, ["Sample Page", "Privacy Policy"]);
    let orphans = PostQuery {
        excluded_parents: vec![0],
        ..pages()
    };
    assert!(titles(&db, orphans).await.is_empty());
    for (order, found) in [(0, 2), (1, 0)] {
        let query = PostQuery {
            menu_order: Some(order),
            ..pages()
        };
        assert_eq!(titles(&db, query).await.len(), found);
    }
    let search = |term: &str, column: &str| PostQuery {
        search: vec![term.into()],
        search_columns: vec![column.into()],
        ..pages()
    };
    assert!(
        titles(&db, search("example", "post_title"))
            .await
            .is_empty()
    );
    assert!(
        titles(&db, search("example", "post_excerpt"))
            .await
            .is_empty()
    );
    assert_eq!(
        titles(&db, search("example", "post_content")).await,
        ["Sample Page"]
    );
    assert_eq!(
        titles(&db, search("-sample", "post_title")).await,
        ["Privacy Policy"]
    );
}

/// `count_user_posts()` as the users REST route asks it: published posts
/// of the types given, private ones too where a type's flag is set; and a
/// user's `user_registered`, empty for none.
#[tokio::test]
async fn authors_posts_are_found() {
    let db = fixture().await;
    let types = |types: &[(&str, bool)]| -> Vec<(String, bool)> {
        types
            .iter()
            .map(|(ty, private)| ((*ty).into(), *private))
            .collect()
    };
    assert!(
        db.has_posts(1, &types(&[("wp_block", false), ("post", false)]))
            .await
            .unwrap()
    );
    assert!(
        !db.has_posts(1, &types(&[("wp_block", true)]))
            .await
            .unwrap()
    );
    assert!(!db.has_posts(2, &types(&[("post", true)])).await.unwrap());
    assert_eq!(db.user_registered(1).await.unwrap(), "2026-10-27 08:08:15");
    assert_eq!(db.user_registered(999).await.unwrap(), "");
    // An ID is searched whole, and `include` orders.
    let query = |search: &str| hyscribe_model::UserQuery {
        search: Some((vec!["user_login".into(), "ID".into()], search.into())),
        include: vec![2, 1],
        order: (hyscribe_model::UserOrder::Include, false),
        ..hyscribe_model::UserQuery::default()
    };
    let ids = |users: Vec<User>| users.iter().map(|user| user.id).collect::<Vec<_>>();
    assert_eq!(ids(db.user_query(&query("1")).await.unwrap()), [1]);
    assert_eq!(
        ids(db.user_query(&query("11")).await.unwrap()),
        Vec::<u64>::new()
    );
}

/// The REST API's comment and user queries, every clause at once.
#[tokio::test]
async fn rest_queries_read_every_clause() {
    use hyscribe_model::{CommentOrder, UserOrder};
    let db = fixture().await;
    let comments = CommentQuery {
        approved: Some(vec!["1".into()]),
        types: vec![String::new(), "comment".into()],
        excluded_types: vec!["note".into()],
        include: vec![1, 2],
        exclude: vec![2],
        parents: vec![0],
        excluded_parents: vec![5],
        posts: vec![1],
        authors: vec![0],
        excluded_authors: vec![1],
        author_email: Some(b"wapuu@wordpress.example".to_vec()),
        search: Some("COMMENT".into()),
        after: Some("2026-10-27 00:00:00".into()),
        before: Some("2026-10-28 00:00:00".into()),
        order: vec![(CommentOrder::Include, false), (CommentOrder::Id, true)],
        limit: Some(1),
        offset: 0,
    };
    let found = db.comment_query(&comments).await.unwrap();
    assert_eq!(
        found.iter().map(|row| row.comment.id).collect::<Vec<_>>(),
        [1]
    );
    assert_eq!(
        (found[0].approved.as_str(), found[0].agent.as_slice()),
        ("1", &b""[..])
    );
    assert_eq!(db.count_comment_query(&comments).await.unwrap(), 1);
    let orders = [
        CommentOrder::Date,
        CommentOrder::DateGmt,
        CommentOrder::Post,
        CommentOrder::Parent,
        CommentOrder::Type,
    ];
    for order in orders {
        let query = CommentQuery {
            order: vec![(order, true)],
            ..CommentQuery::default()
        };
        assert_eq!(
            db.comment_query(&query).await.unwrap().len(),
            1,
            "{order:?}"
        );
    }
    let users = UserQuery {
        caps_key: "wp_capabilities".into(),
        roles: vec!["administrator".into(), "editor".into()],
        level_key: Some("wp_user_level".into()),
        published: Some(vec!["post".into()]),
        nicenames: vec![b"admin".to_vec()],
        search: Some((vec!["user_login".into(), "ID".into()], "dmi".into())),
        include: vec![1],
        exclude: vec![2],
        order: (UserOrder::Nicenames, false),
        limit: Some(1),
        offset: 0,
    };
    let ids = |users: Vec<User>| users.iter().map(|user| user.id).collect::<Vec<_>>();
    assert_eq!(ids(db.user_query(&users).await.unwrap()), [1]);
    assert_eq!(db.count_user_query(&users).await.unwrap(), 1);
    let orders = [
        UserOrder::Login,
        UserOrder::Nicename,
        UserOrder::Email,
        UserOrder::Url,
        UserOrder::Registered,
        UserOrder::DisplayName,
        UserOrder::Id,
        UserOrder::Include,
    ];
    for order in orders {
        let query = UserQuery {
            include: vec![1],
            order: (order, true),
            ..UserQuery::default()
        };
        assert_eq!(ids(db.user_query(&query).await.unwrap()), [1], "{order:?}");
    }
    let slugs = UserQuery {
        order: (UserOrder::Nicenames, false),
        ..UserQuery::default()
    };
    assert_eq!(ids(db.user_query(&slugs).await.unwrap()), [1]);
}

/// Each clause of the REST API's comment and user queries narrows the
/// Theme Test Data's comments and users.
#[tokio::test]
async fn rest_queries_narrow_by_each_clause() {
    use hyscribe_model::CommentOrder;
    let ttd = named("wordpress_ttd").await;
    let all = CommentQuery::default();
    assert_eq!(ttd.count_comment_query(&all).await.unwrap(), 34);
    let counted = [
        (
            CommentQuery {
                approved: Some(vec!["0".into()]),
                ..all.clone()
            },
            3,
        ),
        (
            CommentQuery {
                types: vec!["pingback".into()],
                ..all.clone()
            },
            2,
        ),
        (
            CommentQuery {
                excluded_types: vec!["comment".into()],
                ..all.clone()
            },
            4,
        ),
        (
            CommentQuery {
                include: vec![3, 4],
                ..all.clone()
            },
            2,
        ),
        (
            CommentQuery {
                exclude: vec![3],
                ..all.clone()
            },
            33,
        ),
        (
            CommentQuery {
                parents: vec![13],
                ..all.clone()
            },
            1,
        ),
        (
            CommentQuery {
                excluded_parents: vec![0],
                ..all.clone()
            },
            10,
        ),
        (
            CommentQuery {
                posts: vec![155],
                ..all.clone()
            },
            4,
        ),
        (
            CommentQuery {
                excluded_authors: vec![0],
                ..all.clone()
            },
            0,
        ),
        (
            CommentQuery {
                authors: vec![1],
                ..all.clone()
            },
            0,
        ),
        (
            CommentQuery {
                author_email: Some(b"anon@example.com".to_vec()),
                ..all.clone()
            },
            1,
        ),
        (
            CommentQuery {
                search: Some("\u{393}\u{3b9}\u{3ac}\u{3bd}\u{3bd}\u{3b7}\u{3c2}".into()),
                ..all.clone()
            },
            1,
        ),
        (
            CommentQuery {
                after: Some("2014-01-01 00:00:00".into()),
                ..all.clone()
            },
            5,
        ),
        (
            CommentQuery {
                before: Some("2007-09-05 00:00:00".into()),
                ..all.clone()
            },
            3,
        ),
    ];
    for (query, expected) in counted {
        assert_eq!(
            ttd.count_comment_query(&query).await.unwrap(),
            expected,
            "{query:?}"
        );
        let found = ttd.comment_query(&query).await.unwrap();
        assert_eq!(found.len() as u64, expected, "{query:?}");
    }
    let first = |order: Vec<(CommentOrder, bool)>, include: Vec<u64>| CommentQuery {
        order,
        include,
        limit: Some(3),
        offset: 0,
        ..CommentQuery::default()
    };
    let ordered = [
        (
            first(vec![(CommentOrder::Include, false)], vec![5, 3, 4]),
            [5, 3, 4],
        ),
        (
            first(
                vec![(CommentOrder::Post, false), (CommentOrder::Id, true)],
                vec![],
            ),
            [1, 2, 6],
        ),
        (
            first(
                vec![(CommentOrder::Parent, true), (CommentOrder::Id, true)],
                vec![],
            ),
            [22, 21, 20],
        ),
        (
            first(
                vec![(CommentOrder::Type, true), (CommentOrder::Id, true)],
                vec![],
            ),
            [29, 28, 31],
        ),
    ];
    for (query, expected) in ordered {
        let found = ttd.comment_query(&query).await.unwrap();
        let ids: Vec<u64> = found.iter().map(|row| row.comment.id).collect();
        assert_eq!(ids, expected, "{query:?}");
    }
    let paged = CommentQuery {
        order: vec![(CommentOrder::Id, false)],
        limit: Some(2),
        offset: 1,
        ..CommentQuery::default()
    };
    let found = ttd.comment_query(&paged).await.unwrap();
    assert_eq!(
        found.iter().map(|row| row.comment.id).collect::<Vec<_>>(),
        [2, 3]
    );
    let users = |query: UserQuery| async move {
        let found = named("wordpress_ttd")
            .await
            .user_query(&query)
            .await
            .unwrap();
        found.iter().map(|user| user.id).collect::<Vec<_>>()
    };
    let roles = UserQuery {
        caps_key: "wp_capabilities".into(),
        roles: vec!["editor".into(), "administrator".into()],
        ..UserQuery::default()
    };
    assert_eq!(users(roles).await, [1]);
    let search = UserQuery {
        search: Some((vec!["user_login".into(), "ID".into()], "admi".into())),
        ..UserQuery::default()
    };
    assert_eq!(users(search).await, [1]);
    let login = UserQuery {
        search: Some((vec!["user_login".into()], "admi".into())),
        ..UserQuery::default()
    };
    assert_eq!(users(login).await, [1]);
}

/// The columns each order sorts by, `FIELD()` of a list for none.
#[test]
fn orders_sort_by_their_columns() {
    use hyscribe_model::{CommentOrder, UserOrder};
    let comments = [
        (CommentOrder::Date, Some("comment_date")),
        (CommentOrder::DateGmt, Some("comment_date_gmt")),
        (CommentOrder::Id, Some("comment_ID")),
        (CommentOrder::Post, Some("comment_post_ID")),
        (CommentOrder::Parent, Some("comment_parent")),
        (CommentOrder::Type, Some("comment_type")),
        (CommentOrder::Include, None),
    ];
    for (order, column) in comments {
        assert_eq!(people::comment_column(order), column);
    }
    let users = [
        (UserOrder::Login, Some("user_login")),
        (UserOrder::Nicename, Some("user_nicename")),
        (UserOrder::Email, Some("user_email")),
        (UserOrder::Url, Some("user_url")),
        (UserOrder::Registered, Some("user_registered")),
        (UserOrder::DisplayName, Some("display_name")),
        (UserOrder::Id, Some("u.ID")),
        (UserOrder::Include, None),
        (UserOrder::Nicenames, None),
    ];
    for (order, column) in users {
        assert_eq!(people::user_column(order), column);
    }
}

/// The SQL every query emitted in this test binary, each [`Dialect`]
/// fragment replaced by a marker.
static EMITTED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub(super) fn emitted(sql: &str) {
    EMITTED.lock().unwrap().push(sql.to_string());
}

/// The first MySQL or MariaDB only construct in `sql` (ADR 0014).
fn mysql_only(sql: &str) -> Option<&'static str> {
    const ONLY: &[&str] = &[
        "`",
        "LIMIT ?,",
        " AS SIGNED",
        " AS UNSIGNED",
        " AS CHAR",
        " AS DECIMAL",
        " REGEXP ",
        " RLIKE ",
        "BINARY ",
        " COLLATE ",
        "SQL_CALC_FOUND_ROWS",
        " INTERVAL ",
        "ON DUPLICATE KEY",
        "INSERT IGNORE",
        "REPLACE INTO",
        "STRAIGHT_JOIN",
        "@@",
        " ENGINE",
    ];
    const FUNCTIONS: &[&str] = &[
        "FIELD",
        "RAND",
        "IF",
        "IFNULL",
        "YEAR",
        "MONTH",
        "DAYOFMONTH",
        "HOUR",
        "MINUTE",
        "SECOND",
        "WEEK",
        "DATE_SUB",
        "DATE_ADD",
        "DATE_FORMAT",
        "NOW",
        "UNIX_TIMESTAMP",
        "GREATEST",
        "LEAST",
        "CONCAT",
        "GROUP_CONCAT",
        "FOUND_ROWS",
        "DATABASE",
        "LAST_INSERT_ID",
    ];
    let mut upper = sql.to_ascii_uppercase();
    while upper.contains(" (") {
        upper = upper.replace(" (", "(");
    }
    if let Some(found) = ONLY.iter().find(|only| upper.contains(*only)) {
        return Some(found);
    }
    // A function name, as a whole word, before its parenthesis.
    let words = upper.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '('));
    let call = |word: &str| {
        FUNCTIONS
            .iter()
            .copied()
            .find(|f| word.strip_prefix(f) == Some("("))
    };
    words
        .flat_map(|word| word.split_inclusive('('))
        .find_map(call)
}

/// Each piece of the MySQL dialect is MySQL only, and the guard sees none
/// of it in the portable text.
#[test]
fn the_guard_finds_only_the_dialect() {
    let pieces: [fn(&mut Sql); 15] = [
        Engine::random,
        |sql| Engine::seeded_random(sql, 1),
        |sql| Engine::list_order(sql, "ID", std::iter::once(Bind::Int(1))),
        |sql| Engine::date_part(sql, DatePart::Year, "post_date"),
        |sql| Engine::date_part(sql, DatePart::Day, "post_date"),
        |sql| Engine::date_part(sql, DatePart::Second, "post_date"),
        |sql| Engine::week(sql, "post_date", 0),
        |sql| Engine::week(sql, "post_date", 4),
        |sql| Engine::formatted_time(sql, "post_date", "%H".into(), "1".into()),
        |sql| Engine::date_text(sql, "post_date"),
        |sql| Engine::integer(sql, |sql| sql.push("menu_order")),
        |sql| Engine::not_matching(sql, "meta_value", "^a$".into()),
        |sql| Engine::rollback_less(sql, std::iter::once(Bind::Text("wp_posts".into()))),
        |sql| Engine::insert_or_update(sql, &["option_value"]),
        Engine::privileges,
    ];
    for piece in pieces {
        let mut sql = Sql::new("SELECT ");
        piece(&mut sql);
        assert!(mysql_only(&sql.text).is_some(), "{}", sql.text);
        assert_eq!(mysql_only(&sql.portable), None, "{}", sql.portable);
    }
    assert_eq!(
        mysql_only("SELECT a FROM t WHERE f LIKE ? LIMIT ? OFFSET ?"),
        None
    );
    assert_eq!(mysql_only("SELECT ifnull (a, b)"), Some("IFNULL"));
    assert_eq!(mysql_only("SELECT x.ifnull(a, b)"), Some("IFNULL"));
    assert_eq!(mysql_only("SELECT `a`"), Some("`"));
    assert_eq!(mysql_only("SELECT NOTIF(a)"), None);
}

/// ADR 0014: the SQL the layer emits is portable outside the dialect.
#[tokio::test]
async fn emitted_sql_is_portable() {
    let mut errors = every_query().await;
    errors.extend(crate::write::tests::every_write().await);
    let emitted = EMITTED.lock().unwrap();
    assert!(emitted.len() >= errors.len());
    for sql in emitted.iter() {
        assert_eq!(mysql_only(sql), None, "{sql}");
    }
    assert!(emitted.iter().any(|sql| sql.contains("/* dialect */")));
}
