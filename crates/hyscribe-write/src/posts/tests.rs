// SPDX-License-Identifier: AGPL-3.0-or-later

//! The post functions against the write oracle's goldens
//! (`reference/writes/actions.php` and `actions-posts.php`): each setup
//! writes what PHP's does, and the action must write PHP's rows.

use super::*;
use hyscribe_db::{CommentFields, Connection, Writer};
use hyscribe_testkit::{fixture_database, writes::check_writes};
use std::result::Result as StdResult;

use crate::tests::{ONE_AT_A_TIME, admin};

async fn writer() -> Writer {
    let mode = hyscribe_config::Mode::Primary;
    Writer::open(&fixture_database(), mode)
        .await
        .unwrap()
        .unwrap()
}

fn shown(err: impl std::fmt::Debug) -> String {
    format!("{err:?}")
}

/// Runs `f` as one action on the oracle's connection, as `ctx`.
async fn act<T>(
    writer: &Writer,
    conn: &mut Connection,
    ctx: &Context,
    f: impl AsyncFnOnce(&mut Write<'_>, &Context) -> Result<T>,
) -> StdResult<T, String> {
    let mut write = writer.begin_on(conn).await.map_err(shown)?;
    let value = f(&mut write, ctx).await.map_err(shown)?;
    write.commit().await.map_err(shown)?;
    Ok(value)
}

/// `$posts_insert( $args )` of actions-posts.php, and actions.php's
/// `$new_post( $status )` with `status`.
fn oracle_post(status: &str) -> PostArgs {
    PostArgs {
        title: b"A write oracle post".to_vec(),
        content: b"Its content.".to_vec(),
        status: status.into(),
        author: Some(1),
        ..PostArgs::default()
    }
}

async fn insert_post(
    writer: &Writer,
    conn: &mut Connection,
    args: PostArgs,
) -> StdResult<u64, String> {
    act(writer, conn, &admin(), async |w, ctx| {
        wp_insert_post(w, ctx, args).await
    })
    .await
}

/// `$posts_comment()`: a comment and its meta, raw.
async fn comment(
    conn: &mut Connection,
    (post, id, parent): (u64, u64, u64),
    approved: &str,
) -> StdResult<(), String> {
    sqlx::query(
        "INSERT INTO wp_comments (comment_ID, comment_post_ID, comment_parent, comment_author, \
         comment_date, comment_date_gmt, comment_content, comment_approved) VALUES (?, ?, ?, \
         'A reader', '2026-01-01 00:00:00', '2026-01-01 00:00:00', 'A comment.', ?)",
    )
    .bind(id)
    .bind(post)
    .bind(parent)
    .bind(approved)
    .execute(&mut *conn)
    .await
    .map_err(shown)?;
    sqlx::query(
        "INSERT INTO wp_commentmeta (comment_id, meta_key, meta_value) VALUES (?, 'oracle', ?)",
    )
    .bind(id)
    .bind(approved)
    .execute(&mut *conn)
    .await
    .map_err(shown)?;
    Ok(())
}

/// `$posts_with_comments()`.
async fn with_comments(writer: &Writer, conn: &mut Connection) -> StdResult<u64, String> {
    let id = insert_post(writer, conn, oracle_post("publish")).await?;
    comment(conn, (id, 901, 0), "1").await?;
    comment(conn, (id, 902, 0), "0").await?;
    comment(conn, (id, 903, 0), "1").await?;
    Ok(id)
}

/// `$posts_future()`.
fn future_post() -> PostArgs {
    PostArgs {
        import_id: 900,
        date: "2030-01-01 10:00:00".into(),
        ..oracle_post("publish")
    }
}

async fn check<T>(
    golden: &str,
    setup: impl AsyncFnOnce(&mut Connection) -> StdResult<T, String>,
    action: impl AsyncFnOnce(&mut Connection, T) -> StdResult<(), String>,
) {
    let result = check_writes(&fixture_database(), golden, setup, action).await;
    if let Err(differences) = result {
        panic!("{differences}");
    }
}

#[tokio::test]
async fn inserts_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    check(
        "insert-draft-post",
        async |_| Ok(()),
        async |conn, ()| {
            insert_post(writer, conn, oracle_post("draft"))
                .await
                .map(drop)
        },
    )
    .await;
    check(
        "insert-colliding-slugs",
        async |_| Ok(()),
        async |conn, ()| {
            let titled = |title: &str, post_type: &str| PostArgs {
                title: title.as_bytes().to_vec(),
                post_type: post_type.into(),
                ..oracle_post("publish")
            };
            for (title, post_type) in [
                ("Hello world!", "post"),
                ("Hello world!", "post"),
                ("Feed", "post"),
                ("", "post"),
                ("Sample Page", "page"),
                ("page2", "page"),
            ] {
                insert_post(writer, conn, titled(title, post_type)).await?;
            }
            Ok(())
        },
    )
    .await;
    check(
        "insert-future-post",
        async |_| Ok(()),
        async |conn, ()| insert_post(writer, conn, future_post()).await.map(drop),
    )
    .await;
    check(
        "insert-private-post",
        async |_| Ok(()),
        async |conn, ()| {
            let args = PostArgs {
                title: b"A private post".to_vec(),
                password: b"secret".to_vec(),
                ..oracle_post("private")
            };
            insert_post(writer, conn, args).await.map(drop)
        },
    )
    .await;
    check(
        "insert-pages-with-parent",
        async |_| Ok(()),
        async |conn, ()| {
            for parent in [2, 2, 0] {
                let args = PostArgs {
                    title: b"Child".to_vec(),
                    post_type: "page".into(),
                    parent,
                    menu_order: 3,
                    ..oracle_post("publish")
                };
                insert_post(writer, conn, args).await?;
            }
            Ok(())
        },
    )
    .await;
    check(
        "insert-pending-post-without-capability",
        async |_| Ok(()),
        async |conn, ()| {
            let ctx = Context {
                user: 0,
                can_publish: false,
                ..admin()
            };
            let args = PostArgs {
                name: Some(b"chosen".to_vec()),
                ..oracle_post("pending")
            };
            act(writer, conn, &ctx, async |w, ctx| {
                wp_insert_post(w, ctx, args).await
            })
            .await
            .map(drop)
        },
    )
    .await;
    check(
        "insert-sanitized-post",
        async |_| Ok(()),
        async |conn, ()| {
            let args = PostArgs {
                title: b"  <b>Bold</b> <script>alert(1)</script>title\t ".to_vec(),
                content: b"<p onclick=\"x()\">Para</p><script>bad()</script>\
                    <a href=\"javascript:x()\">link</a>"
                    .to_vec(),
                excerpt: b"<em>ok</em><iframe src=\"https://example.test/\"></iframe>".to_vec(),
                comment_status: "Closed".into(),
                ping_status: "CLOSED".into(),
                password: b"pw".to_vec(),
                to_ping: b"http://a.test/one ftp://b.test\nhttps://c.test/two not-a-url".to_vec(),
                menu_order: 5,
                ..oracle_post("Publish")
            };
            insert_post(writer, conn, args).await.map(drop)
        },
    )
    .await;
    check(
        "insert-reclaims-trashed-slug",
        async |conn| {
            let taken = || PostArgs {
                title: b"Taken".to_vec(),
                ..oracle_post("publish")
            };
            let id = insert_post(writer, conn, taken()).await?;
            act(writer, conn, &admin(), async |w, _| {
                Ok(w.set_post_status(id, "trash").await?)
            })
            .await?;
            Ok(taken())
        },
        async |conn, taken| insert_post(writer, conn, taken).await.map(drop),
    )
    .await;
}

#[tokio::test]
async fn updates_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let update = async |conn: &mut Connection, id, change: fn(&mut PostArgs)| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_update_post(w, ctx, id, &change).await
        })
        .await
        .map(drop)
    };
    check(
        "publish-post",
        async |conn| insert_post(writer, conn, oracle_post("draft")).await,
        async |conn, id| update(conn, id, |args| args.status = "publish".into()).await,
    )
    .await;
    check(
        "update-post-title",
        async |conn| insert_post(writer, conn, oracle_post("publish")).await,
        async |conn, id| update(conn, id, |args| args.title = b"A renamed post".to_vec()).await,
    )
    .await;
    check(
        "update-post-to-draft",
        async |conn| insert_post(writer, conn, oracle_post("publish")).await,
        async |conn, id| update(conn, id, |args| args.status = "draft".into()).await,
    )
    .await;
    check(
        "update-post-slug-and-date",
        async |conn| {
            let args = PostArgs {
                date: "2025-06-07 08:09:10".into(),
                ..oracle_post("publish")
            };
            insert_post(writer, conn, args).await
        },
        async |conn, id| {
            update(conn, id, |args| {
                args.name = Some(b"A new slug".to_vec());
                args.date = "2026-01-02 03:04:05".into();
            })
            .await
        },
    )
    .await;
    check(
        "update-post-with-footnotes",
        async |conn| {
            let id = insert_post(writer, conn, oracle_post("publish")).await?;
            let note = b"[{\"id\":\"n1\",\"content\":\"A note.\"}]";
            act(writer, conn, &admin(), async |w, _| {
                Ok(
                    w.add_meta(hyscribe_db::Meta::Post, id, "footnotes", Some(note))
                        .await?,
                )
            })
            .await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, |args| args.title = b"A renamed post".to_vec()).await,
    )
    .await;
    check(
        "publish-future-post",
        async |conn| insert_post(writer, conn, future_post()).await,
        async |conn, id| {
            act(writer, conn, &admin(), async |w, ctx| {
                wp_publish_post(w, ctx, id).await
            })
            .await
        },
    )
    .await;
}

#[tokio::test]
async fn trash_and_delete_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let trash = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_trash_post(w, ctx, id).await
        })
        .await
        .map(drop)
    };
    let untrash = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_untrash_post(w, ctx, id).await
        })
        .await
        .map(drop)
    };
    let delete = async |conn: &mut Connection, id, force| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_delete_post(w, ctx, id, force).await
        })
        .await
        .map(drop)
    };
    check(
        "trash-post",
        async |conn| insert_post(writer, conn, oracle_post("publish")).await,
        async |conn, id| trash(conn, id).await,
    )
    .await;
    check(
        "untrash-post",
        async |conn| {
            let id = insert_post(writer, conn, oracle_post("publish")).await?;
            trash(conn, id).await?;
            Ok(id)
        },
        async |conn, id| untrash(conn, id).await,
    )
    .await;
    check(
        "trash-future-post",
        async |conn| insert_post(writer, conn, future_post()).await,
        async |conn, id| trash(conn, id).await,
    )
    .await;
    check(
        "trash-post-with-comments",
        async |conn| with_comments(writer, conn).await,
        async |conn, id| trash(conn, id).await,
    )
    .await;
    check(
        "untrash-post-with-comments",
        async |conn| {
            let id = with_comments(writer, conn).await?;
            trash(conn, id).await?;
            Ok(id)
        },
        async |conn, id| untrash(conn, id).await,
    )
    .await;
    check(
        "trash-sticky-post",
        async |conn| {
            let args = PostArgs {
                import_id: 930,
                ..oracle_post("publish")
            };
            let id = insert_post(writer, conn, args).await?;
            act(writer, conn, &admin(), async |w, _| {
                set(w, "sticky_posts", b"a:1:{i:0;i:930;}").await
            })
            .await?;
            Ok(id)
        },
        async |conn, id| trash(conn, id).await,
    )
    .await;
    check(
        "delete-post-with-comments",
        async |conn| {
            let id = with_comments(writer, conn).await?;
            act(writer, conn, &admin(), async |w, ctx| {
                wp_update_post(w, ctx, id, &|args| args.title = b"A renamed post".to_vec()).await
            })
            .await?;
            Ok(id)
        },
        async |conn, id| delete(conn, id, true).await,
    )
    .await;
    check(
        "delete-post-to-trash",
        async |conn| insert_post(writer, conn, oracle_post("publish")).await,
        async |conn, id| delete(conn, id, false).await,
    )
    .await;
    check(
        "delete-front-page-with-children",
        async |conn| {
            let page = |title: &[u8], parent| PostArgs {
                title: title.to_vec(),
                post_type: "page".into(),
                parent,
                ..oracle_post("publish")
            };
            let parent = PostArgs {
                import_id: 910,
                ..page(b"Parent", 0)
            };
            let id = insert_post(writer, conn, parent).await?;
            insert_post(writer, conn, page(b"Kid", id)).await?;
            act(writer, conn, &admin(), async |w, _| {
                set(w, "show_on_front", b"page").await?;
                set(w, "page_on_front", b"910").await
            })
            .await?;
            Ok(id)
        },
        async |conn, id| delete(conn, id, true).await,
    )
    .await;
}

/// `$posts_trash_raw()`: the status changed without wp_trash_post().
async fn trash_raw(writer: &Writer, conn: &mut Connection, id: u64) -> StdResult<u64, String> {
    act(writer, conn, &admin(), async |w, _| {
        Ok(w.set_post_status(id, "trash").await?)
    })
    .await?;
    Ok(id)
}

/// `update_option()` of a value stored as `raw`, unserialized first.
async fn set(w: &mut Write<'_>, name: &str, raw: &[u8]) -> Result<()> {
    let value = hyscribe_phpser::unserialize(raw).unwrap_or_else(|| Value::String(raw.to_vec()));
    crate::options::update_option(w, name, &value, None).await?;
    Ok(())
}

/// `update_option()` as a setup makes it.
async fn set_option(
    writer: &Writer,
    conn: &mut Connection,
    name: &str,
    value: &[u8],
) -> StdResult<(), String> {
    act(writer, conn, &admin(), async |w, _| {
        set(w, name, value).await
    })
    .await
}

#[tokio::test]
async fn edge_cases_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let publish =
        async |conn: &mut Connection| insert_post(writer, conn, oracle_post("publish")).await;
    let update = async |conn: &mut Connection, id, change: &dyn Fn(&mut PostArgs)| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_update_post(w, ctx, id, &change).await
        })
        .await
        .map(drop)
    };
    let trash = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_trash_post(w, ctx, id).await
        })
        .await
        .map(drop)
    };
    let delete = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_delete_post(w, ctx, id, true).await
        })
        .await
        .map(drop)
    };
    check("insert-over-existing-post", publish, async |conn, id| {
        let args = PostArgs {
            id,
            title: b"Rewritten".to_vec(),
            content: b"New.".to_vec(),
            status: "publish".into(),
            ..PostArgs::default()
        };
        insert_post(writer, conn, args).await.map(drop)
    })
    .await;
    check(
        "update-post-category",
        async |conn| {
            for sql in [
                "INSERT INTO wp_terms (term_id, name, slug, term_group) \
                 VALUES (950, 'Oracle', 'oracle', 0)",
                "INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, \
                 parent, count) VALUES (950, 950, 'category', '', 0, 0)",
            ] {
                sqlx::raw_sql(sql)
                    .execute(&mut *conn)
                    .await
                    .map_err(shown)?;
            }
            publish(conn).await
        },
        async |conn, id| update(conn, id, &|args| args.category = Some(vec![950, 999, 950])).await,
    )
    .await;
    check(
        "update-page-parents",
        async |conn| {
            let page = |title: &[u8], parent| PostArgs {
                title: title.to_vec(),
                post_type: "page".into(),
                parent,
                ..oracle_post("publish")
            };
            let a = insert_post(writer, conn, page(b"A", 0)).await?;
            let b = insert_post(writer, conn, page(b"B", a)).await?;
            let c = insert_post(writer, conn, page(b"C", 0)).await?;
            Ok((a, b, c))
        },
        async |conn, (a, b, c)| {
            for (id, parent) in [(a, b), (c, a), (b, b)] {
                update(conn, id, &|args| args.parent = parent).await?;
            }
            Ok(())
        },
    )
    .await;
    check(
        "publish-uncategorized-draft",
        async |conn| {
            let id = insert_post(writer, conn, oracle_post("draft")).await?;
            act(writer, conn, &admin(), async |w, _| {
                Ok(w.remove_relationships(id, &[1]).await?)
            })
            .await?;
            Ok(id)
        },
        async |conn, id| {
            act(writer, conn, &admin(), async |w, ctx| {
                wp_publish_post(w, ctx, id).await
            })
            .await
        },
    )
    .await;
    check(
        "insert-dated-posts",
        async |_| Ok(()),
        async |conn, ()| {
            let dated = [
                PostArgs {
                    date: "2020-01-02 03:04:05".into(),
                    ..oracle_post("future")
                },
                PostArgs {
                    date_gmt: "2024-05-06 07:08:09".into(),
                    ..oracle_post("publish")
                },
                oracle_post(""),
                PostArgs {
                    date: "2023-01-01 00:00:00".into(),
                    ..oracle_post("draft")
                },
                PostArgs {
                    title: b"Only a title".to_vec(),
                    content: Vec::new(),
                    ..oracle_post("publish")
                },
            ];
            for args in dated {
                insert_post(writer, conn, args).await?;
            }
            Ok(())
        },
    )
    .await;
    check(
        "delete-privacy-policy-page",
        async |_| Ok(()),
        async |conn, ()| delete(conn, 3).await,
    )
    .await;
    check(
        "trash-posts-page",
        async |conn| {
            let args = PostArgs {
                import_id: 940,
                title: b"Blog".to_vec(),
                post_type: "page".into(),
                ..oracle_post("publish")
            };
            let id = insert_post(writer, conn, args).await?;
            set_option(writer, conn, "page_for_posts", b"940").await?;
            Ok(id)
        },
        trash,
    )
    .await;
    check(
        "delete-post-with-comment-replies",
        async |conn| {
            let id = publish(conn).await?;
            comment(conn, (id, 901, 902), "1").await?;
            comment(conn, (id, 902, 0), "1").await?;
            comment(conn, (1, 903, 901), "1").await?;
            Ok(id)
        },
        delete,
    )
    .await;
    check(
        "trash-post-named-trashed",
        async |conn| {
            let args = PostArgs {
                title: b"Gone__trashed".to_vec(),
                ..oracle_post("publish")
            };
            insert_post(writer, conn, args).await
        },
        trash,
    )
    .await;
    check(
        "update-numeric-slug-under-pretty-permalinks",
        async |conn| {
            let id = publish(conn).await?;
            update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await?;
            set_option(writer, conn, "permalink_structure", b"/%postname%/").await?;
            Ok(id)
        },
        async |conn, id| {
            for name in [&b"12"[..], b"plain-words", b"00"] {
                update(conn, id, &|args| args.name = Some(name.to_vec())).await?;
            }
            Ok(())
        },
    )
    .await;
    check(
        "update-post-slug-back",
        async |conn| {
            let id = publish(conn).await?;
            update(conn, id, &|args| args.name = Some(b"second".to_vec())).await?;
            Ok(id)
        },
        async |conn, id| {
            update(conn, id, &|args| {
                args.name = Some(b"a-write-oracle-post".to_vec())
            })
            .await
        },
    )
    .await;
    check(
        "insert-long-colliding-slugs",
        async |_| Ok(()),
        async |conn, ()| {
            for _ in 0..2 {
                let args = PostArgs {
                    title: b"a-".repeat(100),
                    ..oracle_post("publish")
                };
                insert_post(writer, conn, args).await?;
            }
            Ok(())
        },
    )
    .await;
    check(
        "untrash-post-without-desired-slug",
        async |conn| {
            let id = publish(conn).await?;
            trash_raw(writer, conn, id).await
        },
        async |conn, id| {
            act(writer, conn, &admin(), async |w, ctx| {
                wp_untrash_post(w, ctx, id).await
            })
            .await
            .map(drop)
        },
    )
    .await;
    check(
        "insert-post-without-pingbacks",
        async |conn| set_option(writer, conn, "default_pingback_flag", b"0").await,
        async |conn, ()| publish(conn).await.map(drop),
    )
    .await;
    check(
        "publish-post-without-guid",
        async |conn| {
            let id = insert_post(writer, conn, oracle_post("draft")).await?;
            act(writer, conn, &admin(), async |w, _| {
                Ok(w.set_post_guid(id, b"").await?)
            })
            .await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.status = "publish".into()).await,
    )
    .await;
    check(
        "update-auto-draft",
        async |conn| insert_post(writer, conn, oracle_post("auto-draft")).await,
        async |conn, id| update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await,
    )
    .await;
    check(
        "update-post-with-autosave",
        async |conn| {
            let id = publish(conn).await?;
            // _wp_put_post_revision( $post, true ).
            act(writer, conn, &admin(), async |w, ctx| {
                let post = w.post(id).await?.ok_or(Error::Refused("invalid_post"))?;
                let args = PostArgs {
                    title: post.title,
                    content: post.content,
                    excerpt: post.excerpt,
                    parent: id,
                    status: "inherit".into(),
                    post_type: "revision".into(),
                    name: Some(format!("{id}-autosave-v1").into_bytes()),
                    date: post.modified,
                    date_gmt: post.modified_gmt,
                    ..PostArgs::default()
                };
                insert(w, ctx, args).await.map(drop)
            })
            .await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await,
    )
    .await;
    check(
        "trash-hello-world",
        async |_| Ok(()),
        async |conn, ()| trash(conn, 1).await,
    )
    .await;
}

/// Runs `f` as an action on the fixture inside a transaction that is
/// rolled back.
async fn rolled_back<T>(f: impl AsyncFnOnce(&mut Write<'_>, &Context) -> Result<T>) -> Result<T> {
    let db = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose())
        .charset(&db.charset);
    let writer = writer().await;
    let mut conn = <Connection as sqlx::Connection>::connect_with(&options)
        .await
        .unwrap();
    let mut outer = sqlx::Connection::begin(&mut conn).await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    f(&mut write, &admin()).await
}

fn unported<T>(result: Result<T>) -> bool {
    matches!(result, Err(Error::Unported(_)))
}

fn invalid<T>(result: Result<T>, code: &str) -> bool {
    matches!(result, Err(Error::Refused(found)) if found == code)
}

async fn insert_rolled_back(args: PostArgs) -> Result<u64> {
    rolled_back(async |w, ctx| wp_insert_post(w, ctx, args).await).await
}

#[tokio::test]
async fn what_php_refuses_is_refused() {
    let _one = ONE_AT_A_TIME.lock().await;
    let empty = PostArgs {
        title: Vec::new(),
        content: b"0".to_vec(),
        ..oracle_post("publish")
    };
    assert!(invalid(insert_rolled_back(empty).await, "empty_content"));
    let excerpt = PostArgs {
        title: Vec::new(),
        content: Vec::new(),
        excerpt: b"An excerpt.".to_vec(),
        ..oracle_post("publish")
    };
    assert!(insert_rolled_back(excerpt).await.is_ok());
    let missing = PostArgs {
        id: 999_999,
        ..oracle_post("publish")
    };
    assert!(invalid(insert_rolled_back(missing).await, "invalid_post"));
    let update = rolled_back(async |w, ctx| wp_update_post(w, ctx, 999_999, &|_| ()).await).await;
    assert!(invalid(update, "invalid_post"));
    let day = PostArgs {
        date: "2026-02-30 00:00:00".into(),
        ..oracle_post("publish")
    };
    assert!(invalid(insert_rolled_back(day).await, "invalid_date"));
    let db = fixture_database();
    let closed = hyscribe_config::Database { port: 1, ..db };
    let Err(err) = Writer::open(&closed, hyscribe_config::Mode::Primary).await else {
        panic!("no database on port 1");
    };
    assert!(matches!(Error::from(err), Error::Db(_)));
}

#[tokio::test]
async fn what_hyscribe_does_not_port_is_refused() {
    let _one = ONE_AT_A_TIME.lock().await;
    let attachment = PostArgs {
        post_type: "attachment".into(),
        ..oracle_post("inherit")
    };
    assert!(unported(insert_rolled_back(attachment).await));
    for date in ["", "2026-01-01 00:00:00"] {
        let gmt = PostArgs {
            date: date.into(),
            date_gmt: "2026-01-01".into(),
            ..oracle_post("publish")
        };
        assert!(unported(insert_rolled_back(gmt).await));
    }
    for (content, filtered) in [
        (&b"{\"isGlobalStylesUserThemeJSON\":true}"[..], &b""[..]),
        (b"x", b"isGlobalStylesUserThemeJSON"),
    ] {
        let styles = PostArgs {
            content: content.to_vec(),
            content_filtered: filtered.to_vec(),
            ..oracle_post("publish")
        };
        assert!(unported(insert_rolled_back(styles).await));
    }
    let pretty = rolled_back(async |w, ctx| {
        set(w, "permalink_structure", b"/%postname%/").await?;
        wp_insert_post(w, ctx, oracle_post("publish")).await
    })
    .await;
    assert!(unported(pretty));
    let page = || PostArgs {
        post_type: "page".into(),
        ..oracle_post("publish")
    };
    let menus = rolled_back(async |w, ctx| {
        let options = b"a:1:{s:8:\"auto_add\";a:1:{i:0;i:5;}}";
        set(w, "nav_menu_options", options).await?;
        wp_insert_post(w, ctx, page()).await
    })
    .await;
    assert!(unported(menus));
    let menu_item = rolled_back(async |w, ctx| {
        w.add_meta(
            hyscribe_db::Meta::Post,
            2,
            "_menu_item_object_id",
            Some(b"1"),
        )
        .await?;
        wp_delete_post(w, ctx, 1, true).await
    })
    .await;
    assert!(unported(menu_item));
    let looped = rolled_back(async |w, ctx| {
        let a = wp_insert_post(w, ctx, page()).await?;
        let b = wp_insert_post(
            w,
            ctx,
            PostArgs {
                parent: a,
                ..page()
            },
        )
        .await?;
        let c = wp_insert_post(w, ctx, page()).await?;
        let post = w.post(a).await?.ok_or(Error::Refused("invalid_post"))?;
        w.update_post(a, &PostFields { parent: b, ..post }).await?;
        wp_update_post(w, ctx, c, &|args| args.parent = a).await
    })
    .await;
    assert!(unported(looped));
    let old_cron = rolled_back(async |w, ctx| {
        set(w, "cron", b"a:0:{}").await?;
        wp_insert_post(w, ctx, oracle_post("publish")).await
    })
    .await;
    assert!(unported(old_cron));
}

#[tokio::test]
async fn posts_that_are_not_there_or_already_done_write_nothing() {
    let _one = ONE_AT_A_TIME.lock().await;
    let done = rolled_back(async |w, ctx| {
        wp_publish_post(w, ctx, 999_999).await?;
        wp_publish_post(w, ctx, 1).await?;
        let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
        Ok([
            wp_trash_post(w, ctx, 999_999).await?,
            wp_untrash_post(w, ctx, 999_999).await?,
            wp_untrash_post(w, ctx, id).await?,
            wp_delete_post(w, ctx, 999_999, true).await?,
            wp_trash_post(w, ctx, id).await?,
            wp_trash_post(w, ctx, id).await?,
        ])
    })
    .await;
    assert_eq!(done.unwrap(), [false, false, false, false, true, false]);
}

#[tokio::test]
async fn odd_stored_values_are_read_as_php_reads_them() {
    let _one = ONE_AT_A_TIME.lock().await;
    // A cron option that is not an array starts a new one.
    let cron = rolled_back(async |w, ctx| {
        set(w, "cron", b"s:3:\"bad\";").await?;
        wp_insert_post(w, ctx, oracle_post("publish")).await?;
        Ok(w.option_value("cron").await?.unwrap_or_default())
    })
    .await;
    let cron = String::from_utf8(cron.unwrap()).unwrap();
    assert!(
        cron.starts_with("a:2:{i:")
            && cron.ends_with(
                ";a:2:{s:8:\"schedule\";b:0;s:4:\"args\";a:0:{}}}}s:7:\"version\";i:2;}"
            )
    );
    // So does a sticky_posts option that is not an array.
    let sticky = rolled_back(async |w, ctx| {
        set(w, "sticky_posts", b"s:1:\"x\";").await?;
        wp_trash_post(w, ctx, 1).await
    })
    .await;
    assert!(sticky.unwrap());
    // A taken import_id gives way to the next ID.
    let taken = PostArgs {
        import_id: 1,
        ..oracle_post("draft")
    };
    assert!(insert_rolled_back(taken).await.unwrap() > 3);
}

#[tokio::test]
async fn the_last_published_post_matches_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let trash = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_trash_post(w, ctx, id).await
        })
        .await
        .map(drop)
    };
    check(
        "trash-last-published-post",
        async |conn| {
            let id = insert_post(writer, conn, oracle_post("publish")).await?;
            trash(conn, 1).await?;
            Ok(id)
        },
        trash,
    )
    .await;
}

/// Runs `action` after `setup` with each statement budget in turn until
/// it runs through: each spent budget must come back as a database error.
async fn every_cut(
    setup: impl AsyncFn(&mut Write<'_>, &Context) -> Result<u64>,
    action: impl AsyncFn(&mut Write<'_>, &Context, u64) -> Result<()>,
) {
    let ctx = admin();
    let setup = async |w: &mut Write<'_>| setup(w, &ctx).await.unwrap();
    crate::tests::fails_at_each_step(setup, async |w, id| action(w, &ctx, id).await).await;
}

#[tokio::test]
async fn database_errors_end_every_function() {
    let none = async |_: &mut Write<'_>, _: &Context| Ok(0);
    let publish = async |w: &mut Write<'_>, ctx: &Context| {
        wp_insert_post(w, ctx, oracle_post("publish")).await
    };
    let page = |title: &str, parent| PostArgs {
        title: title.as_bytes().to_vec(),
        post_type: "page".into(),
        parent,
        ..oracle_post("publish")
    };
    let inserting = |args: PostArgs| {
        async move |w: &mut Write<'_>, ctx: &Context, _| {
            wp_insert_post(w, ctx, args.clone()).await.map(drop)
        }
    };
    let updating = |change: fn(&mut PostArgs)| {
        async move |w: &mut Write<'_>, ctx: &Context, id| {
            wp_update_post(w, ctx, id, &change).await.map(drop)
        }
    };
    let trashing =
        async |w: &mut Write<'_>, ctx: &Context, id| wp_trash_post(w, ctx, id).await.map(drop);
    let deleting = async |w: &mut Write<'_>, ctx: &Context, id| {
        wp_delete_post(w, ctx, id, true).await.map(drop)
    };
    let with_comments = async |w: &mut Write<'_>, ctx: &Context| {
        let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
        wp_update_post(w, ctx, id, &|args| args.title = b"A renamed post".to_vec()).await?;
        let reply = CommentFields {
            post_id: id,
            author: b"A reader".to_vec(),
            author_email: Vec::new(),
            author_url: Vec::new(),
            author_ip: Vec::new(),
            date: "2026-01-01 00:00:00".into(),
            date_gmt: "2026-01-01 00:00:00".into(),
            content: b"A comment.".to_vec(),
            karma: 0,
            approved: "1".into(),
            agent: Vec::new(),
            comment_type: "comment".into(),
            parent: 0,
            user_id: 0,
        };
        let first = w.insert_comment(&reply).await?;
        w.add_meta(hyscribe_db::Meta::Comment, first, "oracle", Some(b"1"))
            .await?;
        w.insert_comment(&CommentFields {
            parent: first,
            approved: "0".into(),
            ..reply
        })
        .await?;
        Ok(id)
    };
    every_cut(none, inserting(oracle_post("publish"))).await;
    every_cut(none, inserting(future_post())).await;
    every_cut(
        none,
        inserting(PostArgs {
            title: Vec::new(),
            ..oracle_post("publish")
        }),
    )
    .await;
    every_cut(none, inserting(page("Sample Page", 0))).await;
    every_cut(
        none,
        inserting(PostArgs {
            title: b"Hello world!".to_vec(),
            ..oracle_post("publish")
        }),
    )
    .await;
    every_cut(
        publish,
        updating(|args| args.title = b"A renamed post".to_vec()),
    )
    .await;
    every_cut(publish, updating(|args| args.name = Some(b"12".to_vec()))).await;
    every_cut(
        async |w, ctx| wp_insert_post(w, ctx, oracle_post("draft")).await,
        updating(|args| args.status = "publish".into()),
    )
    .await;
    every_cut(
        async |w, ctx| {
            let id = wp_insert_post(w, ctx, oracle_post("draft")).await?;
            w.set_post_guid(id, b"").await?;
            w.remove_relationships(id, &[1]).await?;
            Ok(id)
        },
        async |w, ctx, id| wp_publish_post(w, ctx, id).await,
    )
    .await;
    every_cut(
        async |w, ctx| {
            let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
            w.set_post_status(id, "trash").await?;
            Ok(id)
        },
        inserting(oracle_post("publish")),
    )
    .await;
    every_cut(
        async |w, ctx| {
            let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
            wp_update_post(w, ctx, id, &|args| args.name = Some(b"second".to_vec())).await
        },
        updating(|args| args.name = Some(b"a-write-oracle-post".to_vec())),
    )
    .await;
    every_cut(
        async |w, ctx| {
            let a = wp_insert_post(w, ctx, page("A", 0)).await?;
            wp_insert_post(w, ctx, page("B", a)).await
        },
        updating(|args| args.parent = 2),
    )
    .await;
    every_cut(
        async |w, ctx| {
            let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
            w.add_meta(hyscribe_db::Meta::Post, id, "footnotes", Some(b"[]"))
                .await?;
            wp_update_post(w, ctx, id, &|args| args.title = b"A renamed post".to_vec()).await?;
            Ok(id)
        },
        updating(|args| args.title = b"Renamed again".to_vec()),
    )
    .await;
    every_cut(with_comments, trashing).await;
    every_cut(
        async |w, ctx| {
            let id = with_comments(w, ctx).await?;
            wp_trash_post(w, ctx, id).await?;
            Ok(id)
        },
        async |w, ctx, id| wp_untrash_post(w, ctx, id).await.map(drop),
    )
    .await;
    every_cut(with_comments, deleting).await;
    every_cut(
        async |w, ctx| wp_insert_post(w, ctx, future_post()).await,
        trashing,
    )
    .await;
    every_cut(
        async |w, ctx| {
            let id = wp_insert_post(w, ctx, page("Front", 0)).await?;
            wp_insert_post(w, ctx, page("Kid", id)).await?;
            let front = id.to_string();
            set(w, "show_on_front", b"page").await?;
            set(w, "page_on_front", front.as_bytes()).await?;
            set(w, "page_for_posts", front.as_bytes()).await?;
            set(w, "wp_page_for_privacy_policy", front.as_bytes()).await?;
            set(w, "sticky_posts", format!("a:1:{{i:0;i:{id};}}").as_bytes()).await?;
            Ok(id)
        },
        deleting,
    )
    .await;
    every_cut(
        async |w, ctx| {
            let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
            wp_trash_post(w, ctx, 1).await?;
            Ok(id)
        },
        trashing,
    )
    .await;
}

#[tokio::test]
async fn attachments_are_not_ported() {
    let _one = ONE_AT_A_TIME.lock().await;
    let attachment = async |w: &mut Write<'_>, status: &str| -> Result<u64> {
        let post = existing(w, 1).await?;
        let fields = PostFields {
            post_type: "attachment".into(),
            status: status.into(),
            ..post
        };
        Ok(w.insert_post(&fields).await?)
    };
    let refused = [
        rolled_back(async |w, ctx| {
            let id = attachment(w, "inherit").await?;
            wp_update_post(w, ctx, id, &|_| ()).await.map(drop)
        })
        .await,
        rolled_back(async |w, ctx| {
            let id = attachment(w, "inherit").await?;
            wp_publish_post(w, ctx, id).await
        })
        .await,
        rolled_back(async |w, ctx| {
            let id = attachment(w, "inherit").await?;
            wp_trash_post(w, ctx, id).await.map(drop)
        })
        .await,
        rolled_back(async |w, ctx| {
            let id = attachment(w, "trash").await?;
            wp_untrash_post(w, ctx, id).await.map(drop)
        })
        .await,
        rolled_back(async |w, ctx| {
            let id = attachment(w, "inherit").await?;
            wp_delete_post(w, ctx, id, true).await.map(drop)
        })
        .await,
    ];
    for result in refused {
        assert!(unported(result));
    }
}

#[tokio::test]
async fn rare_values_are_kept_as_php_keeps_them() {
    let _one = ONE_AT_A_TIME.lock().await;
    // No default category: a published draft stays without one.
    let uncategorized = rolled_back(async |w, ctx| {
        set(w, "default_category", b"0").await?;
        let id = wp_insert_post(w, ctx, oracle_post("draft")).await?;
        w.remove_relationships(id, &[1]).await?;
        wp_publish_post(w, ctx, id).await?;
        Ok(w.object_terms(id, "category").await?)
    })
    .await;
    assert!(uncategorized.unwrap().is_empty());
    let mime = rolled_back(async |w, ctx| {
        let args = PostArgs {
            mime_type: "Text/pl ain<+*.-".into(),
            ..oracle_post("draft")
        };
        let id = wp_insert_post(w, ctx, args).await?;
        Ok(existing(w, id).await?.mime_type)
    })
    .await;
    assert_eq!(mime.unwrap(), "Text/plain+*.-");
    // Duplicates go when a sticky post leaves the list.
    let sticky = rolled_back(async |w, ctx| {
        let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
        let list = format!("a:4:{{i:0;i:{id};i:1;i:7;i:2;s:1:\"7\";i:3;i:{id};}}");
        set(w, "sticky_posts", list.as_bytes()).await?;
        wp_trash_post(w, ctx, id).await?;
        Ok(w.option_value("sticky_posts").await?.unwrap_or_default())
    })
    .await;
    assert_eq!(sticky.unwrap(), b"a:1:{i:0;i:7;}");
    // A comment left post-trashed comes back unapproved.
    let restored = rolled_back(async |w, ctx| {
        let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
        w.insert_comment(&CommentFields {
            post_id: id,
            author: Vec::new(),
            author_email: Vec::new(),
            author_url: Vec::new(),
            author_ip: Vec::new(),
            date: "2026-01-01 00:00:00".into(),
            date_gmt: "2026-01-01 00:00:00".into(),
            content: b"A comment.".to_vec(),
            karma: 0,
            approved: "post-trashed".into(),
            agent: Vec::new(),
            comment_type: "comment".into(),
            parent: 0,
            user_id: 0,
        })
        .await?;
        wp_trash_post(w, ctx, id).await?;
        wp_untrash_post(w, ctx, id).await?;
        Ok(w.post_comments(id).await?)
    })
    .await;
    let statuses: Vec<String> = restored.unwrap().into_iter().map(|(_, s)| s).collect();
    assert_eq!(statuses, ["0"]);
}

#[tokio::test]
async fn long_encoded_slugs_are_not_ported() {
    let _one = ONE_AT_A_TIME.lock().await;
    let encoded = |prefix: &str| PostArgs {
        name: Some([prefix.as_bytes(), &b"%c3%a9".repeat(33)].concat()),
        ..oracle_post("publish")
    };
    let collided = rolled_back(async |w, ctx| {
        wp_insert_post(w, ctx, encoded("a")).await?;
        wp_insert_post(w, ctx, encoded("a")).await
    })
    .await;
    assert!(unported(collided));
    let trashed = rolled_back(async |w, ctx| {
        let id = wp_insert_post(w, ctx, encoded("")).await?;
        wp_trash_post(w, ctx, id).await
    })
    .await;
    assert!(unported(trashed));
}

#[tokio::test]
async fn database_errors_end_the_rarer_paths() {
    let publish = async |w: &mut Write<'_>, ctx: &Context| {
        wp_insert_post(w, ctx, oracle_post("publish")).await
    };
    let page = |title: &str, parent| PostArgs {
        title: title.as_bytes().to_vec(),
        post_type: "page".into(),
        parent,
        ..oracle_post("publish")
    };
    every_cut(publish, async |w, ctx, id| {
        let args = PostArgs {
            id,
            title: b"Rewritten".to_vec(),
            content: b"New.".to_vec(),
            ..oracle_post("publish")
        };
        wp_insert_post(w, ctx, args).await.map(drop)
    })
    .await;
    every_cut(
        async |_, _| Ok(0),
        async |w, ctx, _| {
            let args = PostArgs {
                to_ping: b"http://a.test/".to_vec(),
                ..oracle_post("publish")
            };
            wp_insert_post(w, ctx, args).await.map(drop)
        },
    )
    .await;
    // A parent four steps from the top, and one that would make a loop.
    for loops in [false, true] {
        every_cut(
            async |w, ctx| {
                let a = wp_insert_post(w, ctx, page("A", 0)).await?;
                let mut parent = wp_insert_post(w, ctx, page("B", a)).await?;
                if loops {
                    return Ok(a * 1_000_000 + parent);
                }
                for title in ["C", "D"] {
                    parent = wp_insert_post(w, ctx, page(title, parent)).await?;
                }
                Ok(wp_insert_post(w, ctx, page("E", 0)).await? * 1_000_000 + parent)
            },
            async |w, ctx, ids| {
                let (id, parent) = (ids / 1_000_000, ids % 1_000_000);
                wp_update_post(w, ctx, id, &|args| args.parent = parent)
                    .await
                    .map(drop)
            },
        )
        .await;
    }
    every_cut(
        async |w, ctx| {
            let args = PostArgs {
                name: Some(b"12".to_vec()),
                date: "2025-06-07 08:09:10".into(),
                ..oracle_post("publish")
            };
            wp_insert_post(w, ctx, args).await
        },
        async |w, ctx, id| {
            wp_update_post(w, ctx, id, &|args| args.date = "2026-01-02 03:04:05".into())
                .await
                .map(drop)
        },
    )
    .await;
}

#[tokio::test]
async fn post_formats_are_not_ported() {
    let _one = ONE_AT_A_TIME.lock().await;
    let deleted = rolled_back(async |w, ctx| {
        let term = w.insert_term(b"Aside", b"post-format-aside", 0).await?;
        let format = w
            .insert_term_taxonomy((term, "post_format"), b"", 0)
            .await?;
        let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
        w.add_relationship(id, format).await?;
        wp_delete_post(w, ctx, id, true).await
    })
    .await;
    assert!(unported(deleted));
}

#[test]
fn term_errors_are_post_errors() {
    let unported = Error::from(crate::terms::TermError::Unported("x"));
    assert!(matches!(unported, Error::Unported("x")));
    let invalid = Error::from(crate::terms::TermError::InvalidTerm);
    assert!(matches!(invalid, Error::Refused("invalid_term")));
}

#[tokio::test]
async fn revisions_copy_footnotes_as_stored() {
    let _one = ONE_AT_A_TIME.lock().await;
    let copied = rolled_back(async |w, ctx| {
        let id = wp_insert_post(w, ctx, oracle_post("publish")).await?;
        let meta = hyscribe_db::Meta::Post;
        let values: [&[u8]; 3] = [b"a:1:{i:0;s:3:\"a\\b\";}", b"i:5;", b"x\\y"];
        for value in values {
            w.add_meta(meta, id, "footnotes", Some(value)).await?;
        }
        wp_update_post(w, ctx, id, &|args| args.title = b"Renamed".to_vec()).await?;
        let revision = w.revision_ids(id).await?.first().copied().unwrap_or(0);
        let rows = w.object_meta(meta, revision).await?;
        Ok(rows.into_iter().map(|(_, value)| value).collect::<Vec<_>>())
    })
    .await;
    let expected: Vec<Option<Vec<u8>>> = vec![
        Some(b"a:1:{i:0;s:3:\"a\\b\";}".to_vec()),
        Some(b"5".to_vec()),
        Some(b"x\\y".to_vec()),
    ];
    assert_eq!(copied.unwrap(), expected);
}

/// The user `wp_delete_user()` takes with its posts, and the posts.
async fn poster(w: &mut Write<'_>, ctx: &Context) -> Result<u64> {
    let login = b"oracle-poster";
    let data = crate::users::UserData {
        user_login: Some(login.to_vec()),
        user_pass: Some(b"A pass phrase".to_vec()),
        user_email: Some(b"oracle-poster@example.test".to_vec()),
        ..crate::users::UserData::default()
    };
    let id = crate::users::wp_insert_user(w, &data).await?;
    let post = PostArgs {
        author: Some(id),
        ..oracle_post("publish")
    };
    wp_insert_post(w, ctx, post.clone()).await?;
    let page = PostArgs {
        title: b"A page".to_vec(),
        post_type: "page".into(),
        ..post
    };
    wp_insert_post(w, ctx, page).await?;
    Ok(id)
}

#[tokio::test]
async fn a_users_posts_go_with_it_as_in_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    check(
        "user-delete-with-posts",
        async |conn| act(writer, conn, &admin(), async |w, ctx| poster(w, ctx).await).await,
        async |conn, id| {
            act(writer, conn, &admin(), async |w, ctx| {
                crate::users::wp_delete_user(w, id, None, ctx)
                    .await
                    .map(drop)
            })
            .await
        },
    )
    .await;
}

#[tokio::test]
async fn database_errors_end_a_users_post_deletion() {
    every_cut(poster, async |w, ctx, id| {
        crate::users::wp_delete_user(w, id, None, ctx)
            .await
            .map(drop)
    })
    .await;
}

/// Term `id` named `name` in `taxonomy`, written raw as the setups do.
async fn raw_term(
    conn: &mut Connection,
    id: u64,
    (name, taxonomy): (&str, &str),
    count: u64,
) -> StdResult<(), String> {
    sqlx::query("INSERT INTO wp_terms (term_id, name, slug, term_group) VALUES (?, ?, ?, 0)")
        .bind(id)
        .bind(name)
        .bind(name.to_lowercase())
        .execute(&mut *conn)
        .await
        .map_err(shown)?;
    sqlx::query(
        "INSERT INTO wp_term_taxonomy (term_taxonomy_id, term_id, taxonomy, description, \
         parent, count) VALUES (?, ?, ?, '', 0, ?)",
    )
    .bind(id)
    .bind(id)
    .bind(taxonomy)
    .bind(count)
    .execute(&mut *conn)
    .await
    .map_err(shown)?;
    Ok(())
}

#[tokio::test]
async fn rarer_saves_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = &writer().await;
    let update = async |conn: &mut Connection, id, change: &dyn Fn(&mut PostArgs)| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_update_post(w, ctx, id, change).await
        })
        .await
        .map(drop)
    };
    let trash = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_trash_post(w, ctx, id).await
        })
        .await
        .map(drop)
    };
    let delete = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_delete_post(w, ctx, id, true).await
        })
        .await
        .map(drop)
    };
    let publish = async |conn: &mut Connection, id| {
        act(writer, conn, &admin(), async |w, ctx| {
            wp_publish_post(w, ctx, id).await
        })
        .await
    };
    let insert = async |conn: &mut Connection, args| insert_post(writer, conn, args).await;
    check(
        "update-categorized-post",
        async |conn| {
            raw_term(conn, 950, ("Oracle", "category"), 0).await?;
            let args = PostArgs {
                category: Some(vec![950]),
                ..oracle_post("publish")
            };
            insert(conn, args).await
        },
        async |conn, id| {
            update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await?;
            let args = PostArgs {
                id,
                title: b"Rewritten".to_vec(),
                content: b"New.".to_vec(),
                status: "publish".into(),
                ..PostArgs::default()
            };
            insert(conn, args).await.map(drop)
        },
    )
    .await;
    check(
        "publish-dated-drafts",
        async |conn| {
            let draft = || PostArgs {
                date: "2025-01-01 00:00:00".into(),
                ..oracle_post("draft")
            };
            Ok((insert(conn, draft()).await?, insert(conn, draft()).await?))
        },
        async |conn, (first, second)| {
            update(conn, first, &|args| args.status = "publish".into()).await?;
            update(conn, second, &|args| {
                args.status = "publish".into();
                args.edit_date = true;
            })
            .await
        },
    )
    .await;
    check(
        "update-unpublished-post-keeps-date",
        async |conn| {
            let args = PostArgs {
                date: "2025-02-03 10:00:00".into(),
                ..oracle_post("publish")
            };
            let id = insert(conn, args).await?;
            update(conn, id, &|args| args.status = "draft".into()).await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await,
    )
    .await;
    check(
        "publish-draft-page",
        async |conn| {
            let args = PostArgs {
                post_type: "page".into(),
                ..oracle_post("draft")
            };
            insert(conn, args).await
        },
        publish,
    )
    .await;
    check(
        "insert-pending-post",
        async |_| Ok(()),
        async |conn, ()| {
            let args = PostArgs {
                name: Some(b"chosen".to_vec()),
                ..oracle_post("pending")
            };
            insert(conn, args).await.map(drop)
        },
    )
    .await;
    check(
        "update-post-with-stray-desired-slug",
        async |conn| {
            let id = insert(conn, oracle_post("publish")).await?;
            act(writer, conn, &admin(), async |w, _| {
                let meta = hyscribe_db::Meta::Post;
                Ok(
                    w.add_meta(meta, id, "_wp_desired_post_slug", Some(b"elsewhere"))
                        .await?,
                )
            })
            .await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await,
    )
    .await;
    check(
        "trash-post-beside-trashed-namesake",
        async |conn| {
            let titled = |title: &[u8]| PostArgs {
                title: title.to_vec(),
                ..oracle_post("publish")
            };
            let id = insert(conn, titled(b"Taken")).await?;
            let trashed = insert(conn, titled(b"Other")).await?;
            act(writer, conn, &admin(), async |w, _| {
                w.set_post_status(trashed, "trash").await?;
                Ok(w.set_post_name(trashed, b"taken").await?)
            })
            .await?;
            Ok(id)
        },
        trash,
    )
    .await;
    check(
        "publish-post-with-custom-guid",
        async |conn| {
            let id = insert(conn, oracle_post("draft")).await?;
            act(writer, conn, &admin(), async |w, _| {
                Ok(w.set_post_guid(id, b"urn:oracle:custom").await?)
            })
            .await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.status = "publish".into()).await,
    )
    .await;
    check(
        "update-post-name-with-autosave",
        async |conn| {
            let id = insert(conn, oracle_post("publish")).await?;
            autosave(writer, conn, id).await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.name = Some(b"renamed".to_vec())).await,
    )
    .await;
    check(
        "update-post-twice",
        async |conn| {
            let id = insert(conn, oracle_post("publish")).await?;
            update(conn, id, &|args| args.title = b"A renamed post".to_vec()).await?;
            Ok(id)
        },
        async |conn, id| update(conn, id, &|args| args.title = b"Renamed again".to_vec()).await,
    )
    .await;
    check(
        "delete-tagged-post",
        async |conn| {
            raw_term(conn, 960, ("Tagged", "post_tag"), 1).await?;
            let id = insert(conn, oracle_post("publish")).await?;
            act(writer, conn, &admin(), async |w, _| {
                Ok(w.add_relationship(id, 960).await?)
            })
            .await?;
            Ok(id)
        },
        delete,
    )
    .await;
    check(
        "insert-inherit-post",
        async |_| Ok(()),
        async |conn, ()| {
            let args = PostArgs {
                title: b"Hello world!".to_vec(),
                ..oracle_post("inherit")
            };
            insert(conn, args).await.map(drop)
        },
    )
    .await;
    check(
        "delete-draft-post",
        async |conn| insert(conn, oracle_post("draft")).await,
        delete,
    )
    .await;
}

/// `_wp_put_post_revision( get_post( $id ), true )`.
async fn autosave(writer: &Writer, conn: &mut Connection, id: u64) -> StdResult<(), String> {
    act(writer, conn, &admin(), async |w, ctx| {
        let post = existing(w, id).await?;
        let args = PostArgs {
            title: post.title,
            content: post.content,
            excerpt: post.excerpt,
            parent: id,
            status: "inherit".into(),
            post_type: "revision".into(),
            name: Some(format!("{id}-autosave-v1").into_bytes()),
            date: post.modified,
            date_gmt: post.modified_gmt,
            ..PostArgs::default()
        };
        insert(w, ctx, args).await.map(drop)
    })
    .await
}

#[tokio::test]
async fn edge_values_are_read_as_php_reads_them() {
    let _one = ONE_AT_A_TIME.lock().await;
    // A future page under pretty permalinks would take a pretty GUID.
    let page = rolled_back(async |w, ctx| {
        set(w, "permalink_structure", b"/%postname%/").await?;
        let args = PostArgs {
            post_type: "page".into(),
            date: "2030-01-01 10:00:00".into(),
            ..oracle_post("publish")
        };
        wp_insert_post(w, ctx, args).await
    })
    .await;
    assert!(unported(page));
    // A new post cannot close a loop, even under a parent in one.
    let looped = rolled_back(async |w, ctx| {
        let page = || PostArgs {
            post_type: "page".into(),
            ..oracle_post("publish")
        };
        let a = wp_insert_post(w, ctx, page()).await?;
        let b = wp_insert_post(
            w,
            ctx,
            PostArgs {
                parent: a,
                ..page()
            },
        )
        .await?;
        let post = existing(w, a).await?;
        w.update_post(a, &PostFields { parent: b, ..post }).await?;
        let c = wp_insert_post(
            w,
            ctx,
            PostArgs {
                parent: a,
                ..page()
            },
        )
        .await?;
        Ok(existing(w, c).await?.parent == a)
    })
    .await;
    assert!(looped.unwrap());
    // A do_pings event already due later is not scheduled again.
    let later = rolled_back(async |w, ctx| {
        let at = ctx.now + 3600;
        let cron = format!(
            "a:2:{{i:{at};a:1:{{s:8:\"do_pings\";a:1:{{s:32:\"40cd750bba9870f18aada2478b24840a\";\
             a:2:{{s:8:\"schedule\";b:0;s:4:\"args\";a:0:{{}}}}}}}}s:7:\"version\";i:2;}}"
        );
        set(w, "cron", cron.as_bytes()).await?;
        wp_insert_post(w, ctx, oracle_post("publish")).await?;
        Ok(w.option_value("cron").await?.unwrap_or_default() == cron.as_bytes())
    })
    .await;
    assert!(later.unwrap());
    // A cron array without its version, but with events, is still old.
    let old = rolled_back(async |w, ctx| {
        set(w, "cron", b"a:1:{i:5;a:0:{}}").await?;
        wp_insert_post(w, ctx, oracle_post("publish")).await
    })
    .await;
    assert!(unported(old));
}
