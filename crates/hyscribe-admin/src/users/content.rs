// SPDX-License-Identifier: AGPL-3.0-or-later

//! `reference/users-list/content.sql` over the default install, as the
//! users list's test site reads it.

use hyscribe_model::User;

use super::ListedUser;

/// A user: ID, login, email, display name, first and last names,
/// capabilities and posts.
fn user(
    (id, login, email, display): (u64, &str, &str, &str),
    (first, last): (&str, &str),
    capabilities: &str,
    posts: u64,
) -> ListedUser {
    ListedUser {
        user: User {
            id,
            login: login.into(),
            nicename: login.into(),
            email: email.into(),
            display_name: display.into(),
            first_name: first.into(),
            last_name: last.into(),
            ..User::default()
        },
        capabilities: capabilities.into(),
        posts,
    }
}

/// The administrator, or with the content every user, by login.
pub(crate) fn users(content: bool) -> Vec<ListedUser> {
    let admin = user(
        (1, "admin", "admin@default-install.test", "admin"),
        ("", ""),
        "a:1:{s:13:\"administrator\";b:1;}",
        1,
    );
    if !content {
        return vec![admin];
    }
    vec![
        admin,
        ListedUser {
            user: User {
                url: b"http://ann.example.test".to_vec(),
                ..user(
                    (602, "author1", "ann@example.test", "Ann"),
                    ("Ann", ""),
                    "a:1:{s:6:\"author\";b:1;}",
                    0,
                )
                .user
            },
            capabilities: b"a:1:{s:6:\"author\";b:1;}".to_vec(),
            posts: 0,
        },
        user(
            (605, "both", "both@example.test", "Both Roles"),
            ("Both", "Roles"),
            "a:2:{s:6:\"editor\";b:1;s:6:\"author\";b:1;}",
            0,
        ),
        user(
            (601, "editor1", "ed@example.test", "Ed Itor"),
            ("Ed", "Itor"),
            "a:1:{s:6:\"editor\";b:1;}",
            2,
        ),
        user(
            (606, "elsewhere", "elsewhere@example.test", "elsewhere"),
            ("", ""),
            "",
            0,
        ),
        user(
            (604, "nobody", "nobody@example.test", "nobody"),
            ("", ""),
            "a:0:{}",
            0,
        ),
        user(
            (607, "rnd", "r&amp;d@example.test", "R&amp;D &lt;b&gt;"),
            ("R&amp;D", "0"),
            "a:1:{s:10:\"subscriber\";b:1;}",
            0,
        ),
        user(
            (603, "zed", "zed@example.test", "Zimmer"),
            ("", "Zimmer"),
            "a:1:{s:10:\"subscriber\";b:1;}",
            0,
        ),
    ]
}
