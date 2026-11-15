// SPDX-License-Identifier: AGPL-3.0-or-later

//! The default install's posts and pages, and with them
//! `reference/posts-list/content.sql`, as the posts list's test site reads them.

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

/// The contents of posts 301 to 315.
const CONTENTS: [&str; 15] = [
    "Draft words",
    "Waiting",
    "Soon",
    "Mine only",
    "Hidden",
    "On top",
    "Gone",
    "Body",
    "Filed",
    "Late",
    "Theirs",
    "Left",
    "Theirs only",
    "Old edit",
    "New edit",
];

/// A page: ID, author, parent, menu order, date, title, status and slug.
#[allow(clippy::too_many_arguments)]
fn page(
    id: u64,
    author: u64,
    parent: u64,
    menu_order: i64,
    date: &str,
    title: &str,
    status: &str,
    name: &str,
) -> Post {
    Post {
        parent,
        menu_order,
        post_type: "page".into(),
        ..post(
            id,
            author,
            date,
            title,
            "",
            status,
            ("closed", "open"),
            "",
            name,
            0,
        )
    }
}

/// The default install's posts and pages, post 1 in the Trash if
/// `trashed`, and with the content if `content`.
pub(crate) fn posts(content: bool, trashed: bool) -> Vec<Post> {
    let installed = "2026-10-27 08:08:15";
    let mut hello = super::tests::hello();
    if trashed {
        hello.status = "trash".into();
    }
    let mut all = vec![
        hello,
        page(
            2,
            1,
            0,
            0,
            installed,
            "Sample Page",
            "publish",
            "sample-page",
        ),
        page(
            3,
            1,
            0,
            0,
            installed,
            "Privacy Policy",
            "draft",
            "privacy-policy",
        ),
    ];
    if content {
        all.extend(content_posts());
        all.extend(pages());
    }
    all
}

/// The content's pages: About over Team over Board, a draft History over
/// Archive, and Contact with comments.
fn pages() -> Vec<Post> {
    let at = |minutes: u8| format!("2026-11-01 09:{minutes:02}:00");
    let mut contact = page(321, 1, 0, 2, &at(50), "Contact", "publish", "contact");
    contact.comment_status = "open".into();
    contact.comment_count = 1;
    vec![
        page(316, 1, 0, 1, &at(0), "About", "publish", "about"),
        page(317, 1, 316, 0, &at(10), "Team", "publish", "team"),
        page(318, 2, 317, 0, &at(20), "Board", "publish", "board"),
        page(319, 1, 316, 1, &at(30), "History", "draft", "history"),
        page(320, 1, 319, 0, &at(40), "Archive", "publish", "archive"),
        contact,
    ]
}

/// Posts 301 to 315.
fn content_posts() -> Vec<Post> {
    let open = ("open", "open");
    let fifteen = "One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen";
    let posts = vec![
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
            post.content = CONTENTS
                .get(usize::try_from(post.id - 301).unwrap())
                .unwrap()
                .as_bytes()
                .to_vec();
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

/// The terms of `post` in `taxonomy`, by name: pages have none.
pub(crate) fn post_terms(post: &Post, taxonomy: &str) -> Vec<Term> {
    if post.post_type != "post" {
        return Vec::new();
    }
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

/// The posts of `posts` that `query` selects, in its order, and how many
/// it matches.
pub(crate) fn list(posts: Vec<Post>, query: &PostQuery) -> (Vec<Post>, u64) {
    let mut found: Vec<Post> = posts
        .into_iter()
        .filter(|post| {
            let terms = |taxonomy: &str| -> Vec<u64> {
                post_terms(post, taxonomy)
                    .iter()
                    .map(|term| term.id)
                    .collect()
            };
            let month = post.date.get(..7).unwrap_or_default();
            query.post_types.contains(&post.post_type)
                && query
                    .include
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&post.id))
                && !query.excluded_authors.contains(&post.author)
                && query.terms.iter().all(|filter| {
                    terms(&filter.taxonomy)
                        .iter()
                        .any(|id| filter.terms.contains(id))
                })
                && query
                    .year
                    .is_none_or(|year| month.starts_with(&year.to_string()))
                && query
                    .month
                    .is_none_or(|m| month.ends_with(&format!("-{m:02}")))
                && query.statuses.iter().any(|filter| {
                    filter.status == post.status && filter.author.is_none_or(|a| a == post.author)
                })
                && (query.authors.is_empty() || query.authors.contains(&post.author))
                && query
                    .search
                    .iter()
                    .all(|term| searched(post, term, query.exact))
        })
        .collect();
    found.sort_by(|a, b| {
        query
            .order
            .iter()
            .map(|&(column, ascending)| {
                let order = match column {
                    OrderBy::Relevance => rank(a, query).cmp(&rank(b, query)),
                    OrderBy::Title => a.title.cmp(&b.title),
                    OrderBy::Modified => a.modified.cmp(&b.modified),
                    OrderBy::MenuOrder => a.menu_order.cmp(&b.menu_order),
                    _ => a.date.cmp(&b.date),
                };
                if ascending || column == OrderBy::Relevance {
                    order
                } else {
                    order.reverse()
                }
            })
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = found.len() as u64;
    let page = found
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();
    (page, total)
}

/// The lowercase title, excerpt and content of `post`, as MySQL compares.
fn fields(post: &Post) -> [String; 3] {
    [&post.title, &post.excerpt, &post.content]
        .map(|field| String::from_utf8_lossy(field).to_lowercase())
}

/// A search `term` matches `post`: in a field, or as one when `exact`; a
/// term after `-` in none.
fn searched(post: &Post, term: &str, exact: bool) -> bool {
    let (term, wanted) = match term.strip_prefix('-') {
        Some(term) => (term.to_lowercase(), false),
        None => (term.to_lowercase(), true),
    };
    let found = fields(post).iter().any(|field| {
        if exact {
            *field == term
        } else {
            field.contains(&term)
        }
    });
    found == wanted
}

/// `parse_search_order()`'s rank of `post`, lowest first.
pub(crate) fn rank(post: &Post, query: &PostQuery) -> u8 {
    let [title, excerpt, content] = fields(post);
    let titled: Vec<String> = query
        .search
        .iter()
        .filter(|term| !term.starts_with('-'))
        .map(|term| term.to_lowercase())
        .collect();
    if query.search_count < 2 {
        return u8::from(!titled.iter().take(1).any(|term| title.contains(term)));
    }
    let phrase = query.sentence.to_lowercase();
    let whole = !query
        .sentence
        .split(char::is_whitespace)
        .any(|word| word.starts_with('-'));
    if whole && title.contains(&phrase) {
        1
    } else if titled.len() < 7 && titled.iter().all(|term| title.contains(term)) {
        2
    } else if titled.len() < 7 && titled.iter().any(|term| title.contains(term)) {
        3
    } else if whole && excerpt.contains(&phrase) {
        4
    } else if whole && content.contains(&phrase) {
        5
    } else {
        6
    }
}

/// The posts of `post_type` of each status, private ones only by
/// `private_author`.
pub(crate) fn status_counts(
    posts: Vec<Post>,
    post_type: &str,
    private_author: Option<u64>,
) -> Vec<(String, u64)> {
    let mut counts: Vec<(String, u64)> = Vec::new();
    for post in posts {
        if post.post_type != post_type
            || post.status == "private" && private_author.is_some_and(|a| a != post.author)
        {
            continue;
        }
        match counts.iter_mut().find(|(status, _)| *status == post.status) {
            Some((_, n)) => *n += 1,
            None => counts.push((post.status.clone(), 1)),
        }
    }
    counts
}

/// The months with posts of `post_type`, only trashed ones when `trash`,
/// newest first.
pub(crate) fn months(posts: Vec<Post>, post_type: &str, trash: bool) -> Vec<(u32, u32)> {
    let mut dates: Vec<String> = posts
        .into_iter()
        .filter(|post| post.post_type == post_type && (post.status == "trash") == trash)
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
        email: format!("{login}@default-install.test").into_bytes(),
        ..User::default()
    };
    vec![
        user(1, "admin", "admin"),
        user(2, "Another Writer", "writer"),
    ]
}

/// `get_post_class()` in the admin, posts 304 and 306 sticky; pages have
/// no format.
pub(crate) fn post_class(class: &str, post: &Post) -> String {
    let mut classes = format!(
        "{class} post-{} type-{} status-{}",
        post.id, post.post_type, post.status
    );
    if post.post_type == "post" {
        let format = post_terms(post, "post_format")
            .first()
            .map_or("standard".to_string(), |term| {
                String::from_utf8_lossy(&term.slug).replacen("post-format-", "", 1)
            });
        classes.push_str(&format!(" format-{format}"));
    }
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
