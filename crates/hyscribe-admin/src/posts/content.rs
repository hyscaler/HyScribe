// SPDX-License-Identifier: AGPL-3.0-or-later

//! `reference/posts-list/content.sql` over the default install, as the
//! posts list's test site reads it.

use hyscribe_model::{OrderBy, Post, PostQuery, Term, User};

/// A post of the content: ID, author, date, title, excerpt, status,
/// comment and ping status, password, slug and approved comments.
#[allow(clippy::too_many_arguments)]
fn post(
    id: u64,
    author: u64,
    date: &str,
    title: &str,
    excerpt: &str,
    status: &str,
    (comments, pings): (&str, &str),
    password: &str,
    name: &str,
    comment_count: i64,
) -> Post {
    Post {
        id,
        author,
        date: date.into(),
        modified: date.into(),
        date_gmt: date.into(),
        title: title.as_bytes().to_vec(),
        excerpt: excerpt.as_bytes().to_vec(),
        status: status.into(),
        password: password.as_bytes().to_vec(),
        name: name.as_bytes().to_vec(),
        post_type: "post".into(),
        comment_count,
        comment_status: comments.into(),
        ping_status: pings.into(),
        ..Post::default()
    }
}

/// Every post of type `post`, the fixture's first.
pub(crate) fn posts() -> Vec<Post> {
    let open = ("open", "open");
    let fifteen = "One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen";
    let posts = vec![
        super::tests::hello(),
        post(
            301,
            1,
            "2026-11-02 10:00:00",
            "A draft",
            "",
            "draft",
            open,
            "",
            "",
            0,
        ),
        post(
            302,
            2,
            "2026-11-03 10:00:00",
            "Pending review",
            "",
            "pending",
            ("open", "closed"),
            "",
            "",
            0,
        ),
        post(
            303,
            1,
            "2030-01-01 10:00:00",
            "Later",
            "",
            "future",
            ("closed", "open"),
            "",
            "later",
            0,
        ),
        post(
            304,
            1,
            "2026-11-04 10:00:00",
            "Private thoughts",
            "",
            "private",
            open,
            "",
            "private-thoughts",
            0,
        ),
        post(
            305,
            1,
            "2026-11-05 10:00:00",
            "Locked",
            "",
            "publish",
            open,
            "secret",
            "locked",
            0,
        ),
        post(
            306,
            1,
            "2026-11-06 10:00:00",
            "Pinned",
            "",
            "publish",
            open,
            "",
            "pinned",
            0,
        ),
        post(
            307,
            1,
            "2026-11-07 10:00:00",
            "Thrown away",
            "",
            "trash",
            open,
            "",
            "thrown-away__trashed",
            1,
        ),
        post(
            308,
            1,
            "2026-11-08 10:00:00",
            "",
            fifteen,
            "publish",
            open,
            "",
            "308-2",
            0,
        ),
        post(
            309,
            1,
            "2026-11-09 10:00:00",
            "Tagged &amp; \"filed\"",
            "",
            "publish",
            open,
            "",
            "tagged-filed",
            0,
        ),
        post(
            310,
            1,
            "2026-01-01 10:00:00",
            "Missed",
            "",
            "future",
            open,
            "",
            "missed",
            0,
        ),
        post(
            311,
            2,
            "2026-11-10 10:00:00",
            "By someone else",
            "",
            "publish",
            open,
            "",
            "by-someone-else",
            2,
        ),
        post(
            312,
            99,
            "0000-00-00 00:00:00",
            "Orphan",
            "",
            "draft",
            open,
            "",
            "",
            0,
        ),
        post(
            313,
            2,
            "2026-11-11 10:00:00",
            "Their secret",
            "",
            "private",
            open,
            "",
            "their-secret",
            0,
        ),
        post(
            314,
            1,
            "2026-11-12 09:00:00",
            "Edited long ago",
            "",
            "draft",
            open,
            "",
            "",
            0,
        ),
        post(
            315,
            1,
            "2026-11-12 11:00:00",
            "Edited lately",
            "",
            "pending",
            open,
            "",
            "",
            0,
        ),
    ];
    // Three were last modified on other days than their dates.
    posts
        .into_iter()
        .map(|mut post| {
            match post.id {
                303 | 312 => post.modified = "2026-11-01 10:00:00".into(),
                314 => post.modified = "2026-10-30 10:00:00".into(),
                _ => {}
            }
            post
        })
        .collect()
}

fn term(id: u64, name: &str, slug: &str, parent: u64, count: u64) -> Term {
    Term {
        id,
        name: name.as_bytes().to_vec(),
        slug: slug.as_bytes().to_vec(),
        parent,
        count,
        ..Term::default()
    }
}

/// Every term of `taxonomy`, by name.
pub(crate) fn terms(taxonomy: &str) -> Vec<Term> {
    match taxonomy {
        "category" => vec![
            term(306, "Empty", "empty", 0, 0),
            term(302, "Local", "local", 301, 1),
            term(301, "News", "news", 0, 1),
            term(1, "Uncategorized", "uncategorized", 0, 5),
        ],
        "post_tag" => vec![
            term(304, "PHP", "php", 0, 1),
            term(303, "Rust &amp; Go", "rust-go", 0, 1),
        ],
        _ => vec![
            term(305, "post-format-aside", "post-format-aside", 0, 1),
            term(307, "post-format-video", "post-format-video", 0, 0),
        ],
    }
}

/// The terms of `post` in `taxonomy`, by name.
pub(crate) fn post_terms(post: &Post, taxonomy: &str) -> Vec<Term> {
    let ids: &[u64] = match (post.id, taxonomy) {
        (309, "category") => &[302, 301],
        (309, "post_tag") => &[304, 303],
        (309, _) => &[305],
        (_, "category") => &[1],
        _ => &[],
    };
    ids.iter()
        .filter_map(|id| terms(taxonomy).into_iter().find(|term| term.id == *id))
        .collect()
}

/// The meta of the content: post 306's template, 307's status before the
/// Trash, and 307's and 312's changesets.
pub(crate) fn meta(post: &Post, key: &str) -> Option<Vec<u8>> {
    let value: &[u8] = match (post.id, key) {
        (306, "_wp_page_template") => b"wide",
        (312, "_customize_changeset_uuid") => b"0c3a1b2c-1111-4222-8333-444455556666",
        (307, "_wp_trash_meta_status") => b"publish",
        (307, "_customize_changeset_uuid") => b"1d4b2c3d-2222-4333-8444-555566667777",
        _ => return None,
    };
    Some(value.to_vec())
}

/// The posts `query` selects, newest first, and how many it matches.
pub(crate) fn list(query: &PostQuery) -> (Vec<Post>, u64) {
    let mut found: Vec<Post> = posts()
        .into_iter()
        .filter(|post| {
            query.post_types.contains(&post.post_type)
                && query.statuses.iter().any(|filter| {
                    filter.status == post.status && filter.author.is_none_or(|a| a == post.author)
                })
                && (query.authors.is_empty() || query.authors.contains(&post.author))
        })
        .collect();
    // `order_by()` always gives a column.
    let (column, ascending) = query
        .order
        .first()
        .copied()
        .unwrap_or((OrderBy::Date, false));
    found.sort_by(|a, b| {
        let order = match column {
            OrderBy::Title => a.title.cmp(&b.title),
            OrderBy::Modified => a.modified.cmp(&b.modified),
            _ => a.date.cmp(&b.date),
        };
        if ascending { order } else { order.reverse() }
    });
    let total = found.len() as u64;
    let page = found
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();
    (page, total)
}

/// The posts of each status, private ones only by `private_author`.
pub(crate) fn status_counts(private_author: Option<u64>) -> Vec<(String, u64)> {
    let mut counts: Vec<(String, u64)> = Vec::new();
    for post in posts() {
        if post.status == "private" && private_author.is_some_and(|a| a != post.author) {
            continue;
        }
        match counts.iter_mut().find(|(status, _)| *status == post.status) {
            Some((_, n)) => *n += 1,
            None => counts.push((post.status.clone(), 1)),
        }
    }
    counts
}

/// The months with posts, only trashed ones when `trash`, newest first.
pub(crate) fn months(trash: bool) -> Vec<(u32, u32)> {
    let mut dates: Vec<String> = posts()
        .into_iter()
        .filter(|post| (post.status == "trash") == trash)
        .map(|post| post.date)
        .collect();
    dates.sort_by(|a, b| b.cmp(a));
    let mut months: Vec<(u32, u32)> = Vec::new();
    for date in dates {
        let part = |range: std::ops::Range<usize>| date.get(range).unwrap().parse().unwrap();
        let month = (part(0..4), part(5..7));
        if !months.contains(&month) {
            months.push(month);
        }
    }
    months
}

/// The users who may edit posts, by display name.
pub(crate) fn authors() -> Vec<User> {
    let user = |id, name: &str, login: &str| User {
        id,
        display_name: name.as_bytes().to_vec(),
        login: login.as_bytes().to_vec(),
        ..User::default()
    };
    vec![
        user(1, "admin", "admin"),
        user(2, "Another Writer", "writer"),
    ]
}

/// `get_post_class()` in the admin for a post of the content, posts 304
/// and 306 sticky.
pub(crate) fn post_class(class: &str, post: &Post) -> String {
    let format = post_terms(post, "post_format")
        .first()
        .map_or("standard".to_string(), |term| {
            String::from_utf8_lossy(&term.slug).replacen("post-format-", "", 1)
        });
    let mut classes = format!(
        "{class} post-{} type-post status-{} format-{format}",
        post.id, post.status
    );
    if !post.password.is_empty() {
        classes.push_str(" post-password-required");
    }
    if [304, 306].contains(&post.id) {
        classes.push_str(" status-sticky");
    }
    classes.push_str(" hentry");
    for (taxonomy, prefix) in [
        ("category", "category-"),
        ("post_tag", "tag-"),
        ("post_format", "post_format-"),
    ] {
        for term in post_terms(post, taxonomy) {
            classes.push(' ');
            classes.push_str(prefix);
            classes.push_str(&String::from_utf8_lossy(&term.slug));
        }
    }
    classes
}
