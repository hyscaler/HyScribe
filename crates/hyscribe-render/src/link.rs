// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_permalink()` and the other links WordPress builds, with plain
//! permalinks, the default install's setting.

use hyscribe_model::{Post, Term};
use hyscribe_phpser::{Value, absint};

use crate::permalinks::Permalinks;
use crate::site::{Archive, Request, Taxonomy};

/// The page on the front page: `page_on_front` when `show_on_front` is
/// `page`.
pub fn front_page(show_on_front: &[u8], page_on_front: &[u8]) -> Option<u64> {
    (show_on_front == b"page").then(|| absint(&Value::String(page_on_front.to_vec())))
}

/// `wp_get_shortlink( 0, 'query' )`: a singular request's post's, the
/// home page for the page on the front.
pub fn shortlink(home: &[u8], front_page: Option<u64>, request: &Request) -> Option<Vec<u8>> {
    let post = request
        .posts
        .first()
        .filter(|post| request.singular && post.id != 0)?;
    Some(if post.post_type == "page" && front_page == Some(post.id) {
        [home, b"/"].concat()
    } else {
        format!("{}/?p={}", String::from_utf8_lossy(home), post.id).into_bytes()
    })
}

/// `get_rest_url()` of `route`; with plain permalinks `route` goes in the
/// query as given.
pub fn rest_url(home: &[u8], links: &Permalinks, route: &str) -> Vec<u8> {
    links
        .rest_url(&String::from_utf8_lossy(home), route)
        .map_or_else(
            || [home, b"/index.php?rest_route=", route.as_bytes()].concat(),
            String::into_bytes,
        )
}

/// The REST API root and the URL of what the request found, as
/// `rest_output_link_wp_head()` and `rest_output_link_header()` link them.
pub fn rest_links(
    home: &[u8],
    links: &Permalinks,
    request: &Request,
) -> (Vec<u8>, Option<Vec<u8>>) {
    let route = queried_resource_route(request).map(|route| rest_url(home, links, &route));
    (rest_url(home, links, "/"), route)
}

/// `rest_get_queried_resource_route()`: the route of a singular request's
/// post, of a category or tag archive's term, or of an author archive's
/// user, by `get_queried_object_id()`. Post formats have none.
fn queried_resource_route(request: &Request) -> Option<String> {
    if let Some(post) = request.posts.first().filter(|_| request.singular) {
        let base = match post.post_type.as_str() {
            "post" => "posts",
            "page" => "pages",
            "attachment" => "media",
            _ => return None,
        };
        return Some(format!("/wp/v2/{base}/{}", post.id));
    }
    match &request.archive {
        Some(Archive::Term(Taxonomy::Category, Some(term))) => {
            Some(format!("/wp/v2/categories/{}", term.id))
        }
        Some(Archive::Term(Taxonomy::Tag, Some(term))) => Some(format!("/wp/v2/tags/{}", term.id)),
        Some(Archive::Author(id, _)) => Some(format!("/wp/v2/users/{id}")),
        _ => None,
    }
}

/// The post types WordPress registers itself (`_builtin`), other than pages
/// and attachments.
pub(crate) const BUILTIN_TYPES: [&str; 14] = [
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
pub fn host(url: &[u8]) -> Vec<u8> {
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
/// for a post format its slug without `post-format-`, and
/// `?taxonomy=<name>&term=<slug>` for a taxonomy without a query variable.
pub fn term_link(home: &[u8], taxonomy: &str, term: &Term) -> Vec<u8> {
    let base = [home, b"/"].concat();
    match taxonomy {
        "category" => [base, format!("?cat={}", term.id).into_bytes()].concat(),
        // _post_format_link() sets the slug again with add_query_arg(),
        // which leaves an empty one without `=`.
        "post_format" => {
            let slug = crate::template::replace(&term.slug, b"post-format-", b"");
            hyscribe_phpser::add_query_arg(&[(b"post_format", Some(&slug))], &base)
        }
        "post_tag" => [&base[..], b"?tag=", &term.slug].concat(),
        // A taxonomy without a query variable.
        _ => [
            &base[..],
            b"?taxonomy=",
            taxonomy.as_bytes(),
            b"&term=",
            &term.slug,
        ]
        .concat(),
    }
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

    /// Recorded from PHP: the `/wp/v2/` route a tag archive links, and none
    /// for a post format's or a missing term's.
    #[test]
    fn term_archives_link_their_rest_routes() {
        let term = Term {
            id: 5,
            ..Term::default()
        };
        let route = |taxonomy, term| {
            queried_resource_route(&Request {
                archive: Some(Archive::Term(taxonomy, term)),
                ..Request::default()
            })
        };
        assert_eq!(
            route(Taxonomy::Tag, Some(term.clone())).as_deref(),
            Some("/wp/v2/tags/5")
        );
        assert_eq!(route(Taxonomy::Format, Some(term)), None);
        assert_eq!(route(Taxonomy::Category, None), None);
        // `rest_get_route_for_post()`: attachments are media, and a post
        // type outside the REST API has no route.
        let single = |post_type| {
            queried_resource_route(&Request {
                singular: true,
                posts: vec![post(7, post_type)],
                ..Request::default()
            })
        };
        assert_eq!(single("attachment").as_deref(), Some("/wp/v2/media/7"));
        assert_eq!(single("revision"), None);
    }

    /// Only a singular request's post has a shortlink, the home page's for
    /// the page on the front.
    #[test]
    fn shortlinks() {
        let home = b"http://example.test";
        let request = |singular| Request {
            singular,
            posts: vec![post(2, "page")],
            ..Request::default()
        };
        let link =
            |front, singular| shortlink(home, front, &request(singular)).map(String::from_utf8);
        assert_eq!(link(Some(2), true), Some(Ok("http://example.test/".into())));
        assert_eq!(
            link(Some(3), true),
            Some(Ok("http://example.test/?p=2".into()))
        );
        assert_eq!(link(Some(2), false), None);
    }
}
