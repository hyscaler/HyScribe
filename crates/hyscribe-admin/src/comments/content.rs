// SPDX-License-Identifier: AGPL-3.0-or-later

//! `reference/comments-list/content.sql` over the default install, as the
//! comments list's test site reads it.

use hyscribe_model::{Comment, Post, User};

use crate::dashboard::RecentComment;
use crate::tests::Role;

/// A comment of the content: ID, post, author, email, URL, IP, day of
/// November, content, type, parent and user.
#[allow(clippy::too_many_arguments)]
fn comment(
    id: u64,
    post_id: u64,
    (author, email, url, ip): (&str, &str, &str, &str),
    day: u32,
    content: &str,
    comment_type: &str,
    parent: u64,
    user_id: u64,
) -> Comment {
    Comment {
        id,
        post_id,
        author: author.into(),
        author_email: email.into(),
        author_url: url.into(),
        author_ip: ip.into(),
        date: format!("2026-11-{day:02} 10:00:00"),
        content: content.into(),
        comment_type: comment_type.into(),
        parent,
        user_id,
        date_gmt: format!("2026-11-{day:02} 10:00:00"),
        ..Comment::default()
    }
}

/// The content's posts: post 1 with its three approved comments, the
/// Sample Page with one, and a protected post.
pub(crate) fn post(id: u64) -> Post {
    let attachment = crate::media::attachment::content::posts()
        .into_iter()
        .find(|post| post.id == id);
    if let Some(attachment) = attachment {
        return attachment;
    }
    match id {
        1 => Post {
            comment_count: 3,
            ..crate::posts::tests::hello()
        },
        2 => Post {
            id: 2,
            author: 1,
            title: b"Sample Page".to_vec(),
            status: "publish".into(),
            post_type: "page".into(),
            comment_count: 1,
            ..Post::default()
        },
        _ => Post {
            id: 501,
            author: 1,
            title: b"Locked away".to_vec(),
            status: "publish".into(),
            post_type: "post".into(),
            password: b"secret".to_vec(),
            comment_count: 1,
            ..Post::default()
        },
    }
}

/// The comments of the content, newest first, as `role` reads them.
pub(crate) fn comments(role: &Role) -> Vec<RecentComment> {
    let mut hello = crate::dashboard::tests::hello_comment(false);
    let admin = User {
        id: 1,
        nicename: b"admin".to_vec(),
        display_name: b"admin".to_vec(),
        email: b"admin@default-install.test".to_vec(),
        ..User::default()
    };
    let lost = comment(
        508,
        999,
        ("Lost", "lost@example.test", "", ""),
        7,
        "On a post since deleted",
        "comment",
        0,
        0,
    );
    let mut found = vec![RecentComment {
        link: "#comment-508".into(),
        permalink: String::new(),
        status: "approved",
        user: None,
        can_edit: crate::menu::MenuUser::can(role, "edit_posts"),
        post: None,
        comment: lost,
    }];
    let rows = [
        (
            comment(
                507,
                501,
                ("Visitor", "v@example.test", "", ""),
                6,
                "Locked words",
                "comment",
                0,
                0,
            ),
            "approved",
        ),
        (
            comment(
                506,
                2,
                ("", "", "http://", ""),
                5,
                "Who am I",
                "comment",
                0,
                0,
            ),
            "approved",
        ),
        (
            Comment {
                date: "2026-11-04 11:00:00".into(),
                date_gmt: "2026-11-04 11:00:00".into(),
                ..comment(
                    505,
                    1,
                    ("Trashed", "gone@example.test", "", ""),
                    4,
                    "Gone",
                    "comment",
                    0,
                    0,
                )
            },
            "trash",
        ),
        (
            comment(
                504,
                1,
                (
                    "Spammer",
                    "spam@example.test",
                    "http://spam.example.test",
                    "",
                ),
                4,
                "Buy now",
                "comment",
                0,
                0,
            ),
            "spam",
        ),
        (
            comment(
                503,
                1,
                ("admin", "admin@default-install.test", "", "127.0.0.1"),
                3,
                "Thanks!",
                "comment",
                1,
                1,
            ),
            "approved",
        ),
        (
            comment(
                502,
                1,
                (
                    "Some Blog",
                    "",
                    "https://blog.example.test/a/very/long/path/that/goes/on/and/on/beyond-fifty/",
                    "",
                ),
                2,
                "Linked to you",
                "pingback",
                0,
                0,
            ),
            "approved",
        ),
        (
            comment(
                501,
                1,
                (
                    "Pending Pat",
                    "pat@example.test",
                    "http://www.example.test/",
                    "192.0.2.1",
                ),
                1,
                "Pending <b>bold</b> and <script>x</script> see http://example.test/link",
                "comment",
                0,
                0,
            ),
            "unapproved",
        ),
    ];
    // The g9attach variant's comments on the attachments of
    // `reference/attachment-edit/content.sql`.
    let attached = [
        (
            Comment {
                date: "2026-03-05 10:00:00".into(),
                date_gmt: "2026-03-05 10:00:00".into(),
                ..comment(
                    801,
                    801,
                    ("Visitor", "visitor@example.test", "", ""),
                    5,
                    "Nice",
                    "comment",
                    0,
                    0,
                )
            },
            "approved",
        ),
        (
            comment(
                802,
                802,
                ("Reader", "", "", ""),
                8,
                "On the report",
                "comment",
                0,
                0,
            ),
            "approved",
        ),
        (
            comment(803, 805, ("Reader", "", "", ""), 9, "Tiny", "comment", 0, 0),
            "unapproved",
        ),
    ];
    let attached = attached
        .into_iter()
        .filter(|_| role.variant() == "g9attach");
    found.extend(rows.into_iter().chain(attached).map(|(comment, status)| {
        let post = post(comment.post_id);
        let permalink = crate::posts::PostsSite::permalink(role, &post);
        RecentComment {
            link: format!("{permalink}#comment-{}", comment.id),
            permalink,
            status,
            user: (comment.user_id == 1).then(|| admin.clone()),
            can_edit: crate::posts::PostsSite::can_post(role, "edit_post", &post),
            post: Some(post),
            comment,
        }
    }));
    hello.post = Some(post(1));
    hello.can_edit = crate::posts::PostsSite::can_post(role, "edit_post", &post(1));
    found.push(hello);
    found
}
