// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// A reader for user 5 of a private post by user 2, the user holding
/// `read_private_posts` through a role.
fn reader(exists: bool) -> Reader {
    let roles = b"a:1:{s:6:\"reader\";a:2:{s:4:\"name\";s:6:\"Reader\";s:12:\"capabilities\";\
                  a:1:{s:18:\"read_private_posts\";b:1;}}}";
    Reader {
        prefix: "wp_".into(),
        user: 5,
        exists,
        roles: Some(roles.to_vec()),
        caps: Some(b"a:1:{s:6:\"reader\";b:1;}".to_vec()),
        post: CapPost {
            id: 7,
            author: 2,
            status: "private".into(),
            post_type: "post".into(),
            parent: 0,
        },
    }
}

/// `read_post` reads only the roles, the user's capabilities and the post,
/// and a user who does not exist has none.
#[test]
fn read_post_reads_what_it_needs() {
    assert!(reader(true).can_read());
    assert!(!reader(false).can_read());
    let reader = reader(true);
    assert_eq!(reader.post_meta(7, "x"), None);
    assert_eq!(reader.comment_post(7), None);
    assert_eq!(reader.term_taxonomy(7), None);
}
