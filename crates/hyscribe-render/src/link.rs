// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_permalink()` with plain permalinks, the default install's setting.

use hyscribe_model::Post;
use hyscribe_phpser::{Value, absint};

/// The page on the front page: `page_on_front` when `show_on_front` is
/// `page`.
pub fn front_page(show_on_front: &[u8], page_on_front: &[u8]) -> Option<u64> {
    (show_on_front == b"page").then(|| absint(&Value::String(page_on_front.to_vec())))
}

/// `get_permalink( $post )`: `home` is `home_url()`, `front_page` as
/// [`front_page`] gives it.
pub fn permalink(home: &[u8], front_page: Option<u64>, post: &Post) -> Vec<u8> {
    let query = match post.post_type.as_str() {
        "page" if front_page == Some(post.id) => String::new(),
        "post" => format!("?p={}", post.id),
        "page" => format!("?page_id={}", post.id),
        "attachment" => format!("?attachment_id={}", post.id),
        other => format!("?post_type={other}&p={}", post.id),
    };
    [home, b"/", query.as_bytes()].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: u64, post_type: &str) -> Post {
        Post {
            id,
            post_type: post_type.to_string(),
            ..Post::default()
        }
    }

    #[test]
    fn plain_permalinks() {
        let home = b"http://example.test";
        let link = |post: &Post, front| String::from_utf8(permalink(home, front, post)).unwrap();
        assert_eq!(link(&post(1, "post"), None), "http://example.test/?p=1");
        assert_eq!(
            link(&post(2, "page"), None),
            "http://example.test/?page_id=2"
        );
        assert_eq!(link(&post(2, "page"), Some(2)), "http://example.test/");
        assert_eq!(link(&post(1, "post"), Some(1)), "http://example.test/?p=1");
        assert_eq!(
            link(&post(3, "attachment"), None),
            "http://example.test/?attachment_id=3"
        );
        assert_eq!(
            link(&post(4, "book"), None),
            "http://example.test/?post_type=book&p=4"
        );
    }
}
