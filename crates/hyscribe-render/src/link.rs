// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_permalink()` with plain permalinks, the default install's setting.

use hyscribe_model::{Post, Term};
use hyscribe_phpser::{Value, absint};

/// The page on the front page: `page_on_front` when `show_on_front` is
/// `page`.
pub fn front_page(show_on_front: &[u8], page_on_front: &[u8]) -> Option<u64> {
    (show_on_front == b"page").then(|| absint(&Value::String(page_on_front.to_vec())))
}

/// The post types WordPress registers itself (`_builtin`), other than pages
/// and attachments.
const BUILTIN_TYPES: [&str; 14] = [
    "post",
    "revision",
    "nav_menu_item",
    "custom_css",
    "customize_changeset",
    "oembed_cache",
    "user_request",
    "wp_block",
    "wp_template",
    "wp_template_part",
    "wp_global_styles",
    "wp_navigation",
    "wp_font_family",
    "wp_font_face",
];

/// The host of `url`, with `:port`: `parse_url()`'s `host` and `port`.
pub(crate) fn host(url: &[u8]) -> Vec<u8> {
    let after_scheme = crate::formatting::find(url, 0, b"://")
        .and_then(|at| url.get(at + 3..))
        .unwrap_or_default();
    let authority = after_scheme
        .split(|&b| matches!(b, b'/' | b'?' | b'#'))
        .next()
        .unwrap_or_default();
    authority
        .rsplit(|&b| b == b'@')
        .next()
        .unwrap_or_default()
        .to_vec()
}

/// `get_permalink( $post )`: `home` is `home_url()`, `front_page` as
/// [`front_page`] gives it.
pub fn permalink(home: &[u8], front_page: Option<u64>, post: &Post) -> Vec<u8> {
    let query = match post.post_type.as_str() {
        "page" if front_page == Some(post.id) => String::new(),
        "page" => format!("?page_id={}", post.id),
        "attachment" => format!("?attachment_id={}", post.id),
        // Core's other types take the post branch of get_permalink().
        builtin if BUILTIN_TYPES.contains(&builtin) => format!("?p={}", post.id),
        other => format!("?post_type={other}&p={}", post.id),
    };
    [home, b"/", query.as_bytes()].concat()
}

/// `get_author_posts_url( $id )` with plain permalinks.
pub fn author_posts_url(home: &[u8], id: impl std::fmt::Display) -> Vec<u8> {
    [home, format!("/?author={id}").as_bytes()].concat()
}

/// `get_term_link( $term, $taxonomy )` for the taxonomies the default
/// install shows: `?cat=<ID>` for a category, `?tag=<slug>` for a tag,
/// and for a post format its slug without `post-format-`.
pub fn term_link(home: &[u8], taxonomy: &str, term: &Term) -> Vec<u8> {
    let query = match taxonomy {
        "category" => format!("?cat={}", term.id).into_bytes(),
        "post_format" => {
            let slug = crate::template::replace(&term.slug, b"post-format-", b"");
            [&b"?post_format="[..], &slug].concat()
        }
        _ => [&b"?tag="[..], &term.slug].concat(),
    };
    [home, b"/", &query].concat()
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
        assert_eq!(
            link(&post(5, "wp_navigation"), None),
            "http://example.test/?p=5"
        );
    }
}
