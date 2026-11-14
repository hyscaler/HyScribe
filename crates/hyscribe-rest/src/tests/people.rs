// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comment and user queries over the oracle's content, as MySQL answers
//! them: text compared and ordered with case ignored.

use std::cmp::Ordering;

use hyscribe_model::{Comment, CommentOrder, CommentQuery, CommentRow, User, UserOrder, UserQuery};

use hyscribe_testkit::{RestCase, rest_cases};

use super::content::{Content, collate};

fn lower(text: &[u8]) -> String {
    String::from_utf8_lossy(text).to_lowercase()
}

/// `LIKE '%text%'`.
fn like(text: &[u8], pattern: &str) -> bool {
    lower(text).contains(&pattern.to_lowercase())
}

/// `IN` of a list: any when empty, unless it must match.
fn among<T: PartialEq>(list: &[T], value: &T) -> bool {
    list.is_empty() || list.contains(value)
}

fn excluded<T: PartialEq>(list: &[T], value: &T) -> bool {
    list.contains(value)
}

impl Content {
    fn comment_selected(&self, query: &CommentQuery) -> Vec<&CommentRow> {
        let ty = |c: &Comment| String::from_utf8_lossy(&c.comment_type).into_owned();
        let mut found: Vec<&CommentRow> = self
            .comments
            .iter()
            .filter(|row| {
                let c = &row.comment;
                query
                    .approved
                    .as_ref()
                    .is_none_or(|approved| approved.contains(&row.approved))
                    && among(&query.include, &c.id)
                    && !excluded(&query.exclude, &c.id)
                    && among(&query.parents, &c.parent)
                    && !excluded(&query.excluded_parents, &c.parent)
                    && among(&query.posts, &c.post_id)
                    && among(&query.authors, &c.user_id)
                    && !excluded(&query.excluded_authors, &c.user_id)
                    && query
                        .author_email
                        .as_ref()
                        .is_none_or(|email| lower(email) == lower(&c.author_email))
                    && among(&query.types, &ty(c))
                    && !excluded(&query.excluded_types, &ty(c))
                    && query.search.as_ref().is_none_or(|search| {
                        [
                            &c.author,
                            &c.author_email,
                            &c.author_url,
                            &c.author_ip,
                            &c.content,
                        ]
                        .iter()
                        .any(|column| like(column, search))
                    })
                    && query.after.as_ref().is_none_or(|after| c.date > *after)
                    && query.before.as_ref().is_none_or(|before| c.date < *before)
            })
            .collect();
        found.sort_by(|a, b| {
            let (x, y) = (&a.comment, &b.comment);
            query
                .order
                .iter()
                .fold(Ordering::Equal, |ordering, (order, descending)| {
                    let next = match order {
                        CommentOrder::Date => x.date.cmp(&y.date),
                        CommentOrder::DateGmt => x.date_gmt.cmp(&y.date_gmt),
                        CommentOrder::Post => x.post_id.cmp(&y.post_id),
                        CommentOrder::Parent => x.parent.cmp(&y.parent),
                        CommentOrder::Type => collate(&x.comment_type, &y.comment_type),
                        CommentOrder::Include => {
                            let at = |id| query.include.iter().position(|i| *i == id);
                            at(x.id).cmp(&at(y.id))
                        }
                        CommentOrder::Id => x.id.cmp(&y.id),
                    };
                    ordering.then(if *descending { next.reverse() } else { next })
                })
        });
        found
    }

    pub(super) fn comment_query(&self, query: &CommentQuery) -> Vec<CommentRow> {
        let skip = usize::try_from(query.offset).unwrap();
        let take = query
            .limit
            .map_or(usize::MAX, |n| usize::try_from(n).unwrap());
        let found = self
            .comment_selected(query)
            .into_iter()
            .skip(skip)
            .take(take);
        found.cloned().collect()
    }

    pub(super) fn count_comment_query(&self, query: &CommentQuery) -> u64 {
        self.comment_selected(query).len() as u64
    }

    fn meta(&self, user: u64, key: &str) -> Option<&[u8]> {
        let found = self
            .usermeta
            .iter()
            .find(|(id, name, _)| *id == user && name == key);
        found.map(|(_, _, value)| value.as_slice())
    }

    fn user_selected(&self, query: &UserQuery) -> Vec<&(User, String)> {
        let published = |user: u64, types: &[String]| {
            self.posts.iter().any(|post| {
                post.author == user && post.status == "publish" && types.contains(&post.post_type)
            })
        };
        let column = |user: &(User, String), name: &str| -> Vec<u8> {
            let (user, _) = user;
            match name {
                "ID" => user.id.to_string().into_bytes(),
                "user_login" => user.login.clone(),
                "user_email" => user.email.clone(),
                "user_url" => user.url.clone(),
                "user_nicename" => user.nicename.clone(),
                _ => user.display_name.clone(),
            }
        };
        let mut found: Vec<&(User, String)> = self
            .users
            .iter()
            .filter(|entry| {
                let user = &entry.0;
                let caps = self.meta(user.id, &query.caps_key).unwrap_or_default();
                query
                    .published
                    .as_ref()
                    .is_none_or(|types| published(user.id, types))
                    && (query.nicenames.is_empty()
                        || query
                            .nicenames
                            .iter()
                            .any(|name| lower(name) == lower(&user.nicename)))
                    && query.level_key.as_ref().is_none_or(|key| {
                        self.meta(user.id, key).is_some_and(|level| level != b"0")
                    })
                    && (query.roles.is_empty()
                        || query
                            .roles
                            .iter()
                            .any(|role| like(caps, &format!("\"{role}\""))))
                    && query.search.as_ref().is_none_or(|(columns, text)| {
                        columns.iter().any(|name| {
                            let value = column(entry, name);
                            if name == "ID" {
                                value == text.as_bytes()
                            } else {
                                like(&value, text)
                            }
                        })
                    })
                    && among(&query.include, &user.id)
                    && !excluded(&query.exclude, &user.id)
            })
            .collect();
        let (order, descending) = query.order;
        found.sort_by(|a, b| {
            let ordering = match order {
                UserOrder::Login => collate(&a.0.login, &b.0.login),
                UserOrder::Nicename => collate(&a.0.nicename, &b.0.nicename),
                UserOrder::Email => collate(&a.0.email, &b.0.email),
                UserOrder::Url => collate(&a.0.url, &b.0.url),
                UserOrder::Registered => a.1.cmp(&b.1),
                UserOrder::DisplayName => collate(&a.0.display_name, &b.0.display_name),
                UserOrder::Id => a.0.id.cmp(&b.0.id),
                UserOrder::Include => {
                    let at = |id| query.include.iter().position(|i| *i == id);
                    at(a.0.id).cmp(&at(b.0.id))
                }
                UserOrder::Nicenames => {
                    let at =
                        |name: &[u8]| query.nicenames.iter().position(|n| lower(n) == lower(name));
                    at(&a.0.nicename).cmp(&at(&b.0.nicename))
                }
            };
            let ordering = if descending {
                ordering.reverse()
            } else {
                ordering
            };
            ordering.then(a.0.id.cmp(&b.0.id))
        });
        found
    }

    pub(super) fn user_query(&self, query: &UserQuery) -> Vec<User> {
        let skip = usize::try_from(query.offset).unwrap();
        let take = query
            .limit
            .map_or(usize::MAX, |n| usize::try_from(n).unwrap());
        let found = self.user_selected(query).into_iter().skip(skip).take(take);
        found.map(|(user, _)| user.clone()).collect()
    }

    pub(super) fn count_user_query(&self, query: &UserQuery) -> u64 {
        self.user_selected(query).len() as u64
    }

    pub(super) fn has_posts(&self, author: u64, types: &[(String, bool)]) -> bool {
        self.posts.iter().any(|post| {
            post.author == author
                && types.iter().any(|(ty, private)| {
                    *ty == post.post_type
                        && (post.status == "publish" || (*private && post.status == "private"))
                })
        })
    }
}

/// What the comments and users routes leave unanswered: a visitor's users
/// request but `/users/me`, and embedding a comment's links (a
/// user has none to embed).
fn unported(case: &RestCase) -> bool {
    let users = case.target.contains("/wp/v2/users");
    let me = case.target.contains("/users/me");
    (case.who == "visitor" && users && !me) || (!users && case.target.contains("_embed"))
}

/// The comments and users routes answer as PHP does for a visitor and each
/// role, over the default install, with `reference/rest/people-more.sql`
/// and, by hash, over the Theme Test Data fixture.
#[test]
fn people_answer_as_php() {
    let goldens = [
        (
            super::content::default_install(),
            include_str!("../../tests/golden/people.tsv"),
            false,
        ),
        (
            super::content::default_install_more(),
            include_str!("../../tests/golden/people-more.tsv"),
            false,
        ),
        (
            super::content::theme_test_data(),
            include_str!("../../tests/golden/people-ttd.tsv"),
            true,
        ),
    ];
    let (failures, not_served) = super::mismatches(&goldens, &unported);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    let expected = goldens.iter().flat_map(|(_, golden, _)| rest_cases(golden));
    assert_eq!(not_served, expected.filter(unported).count());
}

/// The default install with `reference/rest/people-more.sql`, changed.
fn variant(change: impl FnOnce(&mut Content)) -> &'static Content {
    let more = super::content::default_install_more();
    let mut content = Content {
        options: more.options.clone(),
        posts: more.posts.clone(),
        usermeta: more.usermeta.clone(),
        comments: more.comments.clone(),
        users: more.users.clone(),
        ..Content::default()
    };
    change(&mut content);
    Box::leak(Box::new(content))
}

fn allow(content: &'static Content, who: &str, target: &str) -> String {
    let served = super::get(content, who, target);
    let allow = served.headers.iter().find(|(name, _)| name == "Allow");
    allow.map(|(_, value)| value.clone()).unwrap_or_default()
}

/// `create_item_permissions_check()` reads `(int) $request['post']`, the
/// collection's list of posts as 1, so only post 1 decides whether a
/// moderator may comment: not on a draft, in the trash, with comments
/// closed, or with the clock read for old posts' comments.
#[test]
fn comments_may_be_created_on_an_open_first_post() {
    let target = "/?rest_route=/wp/v2/comments&post=9001";
    let open = variant(|_| {});
    assert_eq!(allow(open, "administrator", target), "GET, POST");
    let first = |change: fn(&mut hyscribe_model::Post)| {
        variant(move |content| {
            content
                .posts
                .iter_mut()
                .filter(|post| post.id == 1)
                .for_each(change);
        })
    };
    for closed in [
        first(|post| post.status = "draft".into()),
        first(|post| post.status = "trash".into()),
        first(|post| post.comment_status = "closed".into()),
        variant(|content| {
            for name in ["close_comments_for_old_posts", "close_comments_days_old"] {
                content.options.insert(name.into(), b"14".to_vec());
            }
        }),
    ] {
        assert_eq!(allow(closed, "administrator", target), "GET");
    }
    let mine = "/?rest_route=/wp/v2/comments&post=1&author=1";
    assert_eq!(allow(open, "administrator", mine), "GET, POST");
    assert_eq!(allow(open, "author", mine), "GET");
}

/// A user's locale is their own, else the site's, else `en_US`; a user
/// without capabilities has `{}` of them, as `(object)` casts an empty
/// array.
#[test]
fn users_are_prepared_as_php_prepares_them() {
    let target = "/?rest_route=/wp/v2/users/702&context=edit";
    let field = |content, name: &str| {
        let served = super::get(content, "administrator", target);
        let data = hyscribe_phpser::json_decode(&served.body).unwrap();
        hyscribe_phpser::json_encode(data.get_str(name).unwrap()).unwrap()
    };
    let more = super::content::default_install_more();
    assert_eq!(field(more, "locale"), br#""en_US""#);
    let site = variant(|content| {
        content.options.insert("WPLANG".into(), b"de_DE".to_vec());
    });
    assert_eq!(field(site, "locale"), br#""de_DE""#);
    let own = variant(|content| {
        content
            .usermeta
            .push((702, "locale".into(), b"fr_FR".to_vec()));
    });
    assert_eq!(field(own, "locale"), br#""fr_FR""#);
    let none = variant(|content| {
        content
            .usermeta
            .retain(|(id, key, _)| *id != 702 || key != "wp_capabilities");
    });
    let body = super::get(none, "administrator", target).body;
    let body = String::from_utf8(body).unwrap();
    assert!(
        body.contains(r#""capabilities":{},"extra_capabilities":{}"#),
        "{body}"
    );
}

/// Notes on a post type without them: refused to who may edit the post,
/// else as the parameters not permitted; moving a comment needs the target
/// post's `edit_post` without `moderate_comments`; a moderator may comment
/// only on a post the REST API reads.
#[test]
fn notes_and_moves_are_checked_as_php_checks_them() {
    let more = super::content::default_install_more();
    let unsupported = "/?rest_route=/wp/v2/comments&type=note&post=9007";
    let refused = super::get(more, "subscriber", unsupported);
    let message =
        r#""code":"rest_forbidden_param","message":"Query parameter not permitted: type""#;
    assert!(String::from_utf8(refused.body).unwrap().contains(message));
    let own = "/?rest_route=/wp/v2/comments/9001&post=9003";
    assert_eq!(allow(more, "author", own), "GET, POST, PUT, PATCH, DELETE");
    let elsewhere = "/?rest_route=/wp/v2/comments/9001&post=2";
    assert_eq!(allow(more, "author", elsewhere), "GET, DELETE");
    let unread = variant(|content| {
        let first = content.posts.iter_mut().find(|post| post.id == 1).unwrap();
        first.post_type = "customize_changeset".into();
    });
    let target = "/?rest_route=/wp/v2/comments&post=9001";
    assert_eq!(allow(unread, "administrator", target), "GET");
}

/// Comments on a pattern or a global styles post are read as their own
/// controllers read the post: `read_post`, and for a pattern the posts
/// controller's check too; with post 1 gone no one may comment.
#[test]
fn comments_follow_their_posts_controllers() {
    let typed = |ty: &'static str| {
        variant(move |content| {
            let first = content.posts.iter_mut().find(|post| post.id == 1).unwrap();
            first.post_type = ty.into();
        })
    };
    let target = "/?rest_route=/wp/v2/comments&post=1&_fields=id";
    for ty in ["wp_block", "wp_global_styles"] {
        let content = typed(ty);
        assert_eq!(super::get(content, "visitor", target).status, 401, "{ty}");
        assert_eq!(
            super::get(content, "administrator", target).status,
            200,
            "{ty}"
        );
    }
    let gone = variant(|content| content.posts.retain(|post| post.id != 1));
    let collection = "/?rest_route=/wp/v2/comments&post=9001";
    assert_eq!(allow(gone, "administrator", collection), "GET");
}

/// The users route's search columns by their REST names, and roles named
/// twice, by themselves and by a capability, listed once.
#[test]
fn users_are_searched_and_filtered_as_php_filters_them() {
    let more = super::content::default_install_more();
    let ids = |target: &str| {
        let served = super::get(more, "administrator", target);
        let data = hyscribe_phpser::json_decode(&served.body).unwrap();
        let users = data.as_array().unwrap_or_default().to_vec();
        users
            .iter()
            .map(|(_, user)| user.get_str("id").unwrap().to_int())
            .collect::<Vec<_>>()
    };
    for columns in ["username", "slug"] {
        let target = format!("/?rest_route=/wp/v2/users&search=edit&search_columns={columns}");
        assert_eq!(ids(&target), [701], "{columns}");
    }
    let twice = "/?rest_route=/wp/v2/users&roles=administrator&capabilities=manage_options";
    assert_eq!(ids(twice), [1]);
}

fn body(content: &'static Content, who: &str, target: &str) -> String {
    String::from_utf8(super::get(content, who, target).body).unwrap()
}

/// The IDs of a collection's answer.
fn listed(content: &'static Content, who: &str, target: &str) -> Vec<i64> {
    let data = hyscribe_phpser::json_decode(body(content, who, target).as_bytes()).unwrap();
    let items = data.as_array().unwrap_or_default().to_vec();
    items
        .iter()
        .map(|(_, item)| item.get_str("id").unwrap().to_int())
        .collect()
}

/// Avatars are left out of both schemas while the site shows none.
#[test]
fn avatars_follow_show_avatars() {
    let hidden = variant(|content| {
        content.options.insert("show_avatars".into(), Vec::new());
    });
    let comment = "/?rest_route=/wp/v2/comments/1";
    assert!(!body(hidden, "visitor", comment).contains("author_avatar_urls"));
    assert!(!body(hidden, "administrator", "/?rest_route=/wp/v2/users/1").contains("avatar_urls"));
    let more = super::content::default_install_more();
    assert!(body(more, "visitor", comment).contains("author_avatar_urls"));
}

/// How a comment's post is read: an attachment through its parent only,
/// a draft not through its parent; a protected post in the edit context by
/// who may edit it as the REST API reads its type.
#[test]
fn comments_posts_are_read_as_php_reads_them() {
    let draft = variant(|content| {
        let post = content
            .posts
            .iter_mut()
            .find(|post| post.id == 9002)
            .unwrap();
        post.parent = 1;
        let row = content
            .comments
            .iter_mut()
            .find(|row| row.comment.id == 9002)
            .unwrap();
        row.approved = "1".into();
    });
    let target = "/?rest_route=/wp/v2/comments/9002";
    assert_eq!(super::get(draft, "visitor", target).status, 401);
    let cached = variant(|content| {
        let post = content
            .posts
            .iter_mut()
            .find(|post| post.id == 9003)
            .unwrap();
        post.post_type = "oembed_cache".into();
    });
    let target = "/?rest_route=/wp/v2/comments/9003&context=edit";
    assert_eq!(super::get(cached, "administrator", target).status, 403);
    let unregistered = variant(|content| {
        let post = content
            .posts
            .iter_mut()
            .find(|post| post.id == 9003)
            .unwrap();
        post.post_type = "bogus".into();
    });
    let target = "/?rest_route=/wp/v2/comments/9003";
    assert_eq!(
        super::get(unregistered, "administrator", target).status,
        403
    );
}

/// Moving a comment: to its own post needs nothing more, to a post a
/// moderator may not edit `moderate_comments`; post 1 decides a new
/// comment, whose type need not support notes.
#[test]
fn comments_moves_are_allowed_as_php_allows_them() {
    let more = super::content::default_install_more();
    let all = "GET, POST, PUT, PATCH, DELETE";
    assert_eq!(
        allow(
            more,
            "administrator",
            "/?rest_route=/wp/v2/comments/9008&post=0"
        ),
        all
    );
    assert_eq!(
        allow(more, "editor", "/?rest_route=/wp/v2/comments/1&post=9004"),
        all
    );
    let attachment = variant(|content| {
        let first = content.posts.iter_mut().find(|post| post.id == 1).unwrap();
        first.post_type = "attachment".into();
        first.status = "inherit".into();
    });
    let target = "/?rest_route=/wp/v2/comments&post=9001";
    assert_eq!(allow(attachment, "administrator", target), "GET, POST");
}

/// Comments are ordered by their local or GMT dates, ties by ID in the
/// order's direction; `author_exclude` and an `offset` of 0, which pages,
/// count; only approved and held replies make a comment's children.
#[test]
fn comments_are_queried_as_php_queries_them() {
    let ttd = super::content::theme_test_data();
    let dates = "/?rest_route=/wp/v2/comments&post=155&order=asc&orderby=";
    assert_eq!(listed(ttd, "visitor", &format!("{dates}date")), [5, 4, 3]);
    assert_eq!(
        listed(ttd, "visitor", &format!("{dates}date_gmt")),
        [4, 3, 5]
    );
    let tied = variant(|content| {
        for row in &mut content.comments {
            row.comment.date_gmt = "2026-10-28 10:00:00".into();
        }
    });
    let target = "/?rest_route=/wp/v2/comments&orderby=date_gmt&order=asc&include=9014,1";
    assert_eq!(listed(tied, "visitor", target), [1, 9014]);
    let more = super::content::default_install_more();
    let target = "/?rest_route=/wp/v2/comments&author_exclude=702&status=all&per_page=100";
    let ids = listed(more, "administrator", target);
    assert!(!ids.contains(&9001) && ids.contains(&9002));
    let target = "/?rest_route=/wp/v2/comments&post=155&offset=0&page=2&per_page=1";
    assert_eq!(listed(ttd, "visitor", target), [3]);
    let spam_child = variant(|content| {
        let row = content
            .comments
            .iter_mut()
            .find(|row| row.comment.id == 9005)
            .unwrap();
        row.comment.parent = 9014;
    });
    assert!(!body(spam_child, "visitor", "/?rest_route=/wp/v2/comments/9014").contains("children"));
}

/// Users ordered by login without IDs to order by, by slug; searched by
/// login or slug, and a URL in the URL alone; paged by an `offset` of 0;
/// a user whose only posts are private listed to who may read them; a
/// nested field computes none of its field; `/users/me` roles to set.
#[test]
fn users_are_queried_as_php_queries_them() {
    let renamed = variant(|content| {
        for (user, _) in &mut content.users {
            user.display_name = match user.id {
                1 => b"zz".to_vec(),
                701 => b"Ed".to_vec(),
                702 => b"https://a".to_vec(),
                _ => user.display_name.clone(),
            };
        }
        let protected = content
            .posts
            .iter_mut()
            .find(|post| post.id == 9003)
            .unwrap();
        protected.author = 1;
    });
    let users = "/?rest_route=/wp/v2/users&per_page=1&orderby=";
    assert_eq!(
        listed(renamed, "administrator", &format!("{users}include")),
        [1]
    );
    assert_eq!(
        listed(renamed, "administrator", &format!("{users}slug")),
        [1]
    );
    let search = "/?rest_route=/wp/v2/users&search=edit&search_columns=";
    for columns in ["username", "slug"] {
        assert_eq!(
            listed(renamed, "administrator", &format!("{search}{columns}")),
            [701]
        );
    }
    let url = "/?rest_route=/wp/v2/users&search=https://a";
    assert_eq!(listed(renamed, "administrator", url), Vec::<i64>::new());
    let paged = "/?rest_route=/wp/v2/users&offset=0&page=2&per_page=1&orderby=id";
    assert_eq!(listed(renamed, "administrator", paged), [701]);
    let private = "/?rest_route=/wp/v2/users/702";
    assert_eq!(super::get(renamed, "editor", private).status, 200);
    assert_eq!(super::get(renamed, "subscriber", private).status, 403);
    let nested = "/?rest_route=/wp/v2/users/1&_fields=avatar_urls.24";
    assert_eq!(body(renamed, "administrator", nested), "[]");
    assert_eq!(
        allow(
            renamed,
            "author",
            "/?rest_route=/wp/v2/users/me&roles=editor"
        ),
        "GET"
    );
}
