// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up` (CI does).

use super::*;
use hyscribe_config::Secret;
use hyscribe_model::{FormatFilter, TermFilter};

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
        order: vec![(OrderBy::Relevance, false), (OrderBy::Date, false)],
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
        db.feed_comments(None, 1).await.unwrap_err(),
        db.count_posts(&PostQuery {
            post_types: vec!["post".into()],
            ..PostQuery::default()
        })
        .await
        .unwrap_err(),
        db.last_comment_modified().await.unwrap_err(),
        db.last_post_modified().await.unwrap_err(),
        db.archive_feed_comments(
            &PostQuery {
                post_types: vec!["post".into()],
                ..PostQuery::default()
            },
            1,
        )
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
        assert_eq!(order_column(column), name);
    }
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
            slug: b"uncategorized".to_vec(),
            description: Vec::new(),
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

#[tokio::test]
async fn feed_comments() {
    let db = fixture().await;
    let ids =
        |comments: Vec<hyscribe_model::Comment>| comments.iter().map(|c| c.id).collect::<Vec<_>>();
    assert_eq!(ids(db.feed_comments(None, 10).await.unwrap()), [1]);
    assert_eq!(ids(db.feed_comments(Some(1), 10).await.unwrap()), [1]);
    assert!(db.feed_comments(Some(2), 10).await.unwrap().is_empty());
    assert!(db.feed_comments(None, 0).await.unwrap().is_empty());
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
