// SPDX-License-Identifier: AGPL-3.0-or-later

//! What dynamic blocks read from the site and the request (ADR 0009).

use hyscribe_model::{Post, PostQuery, Term, User};

/// The site's data, as blocks read it while rendering.
pub trait Site {
    /// `get_option( $name )` as stored, `None` when the option is missing.
    fn option(&self, name: &str) -> Option<Vec<u8>>;

    /// `home_url()`: the `home` option, or what `WP_HOME` sets.
    fn home_url(&self) -> Vec<u8>;

    /// `get_post( $id )`, whatever its type and status.
    fn post(&self, id: u64) -> Option<Post>;

    /// `get_the_terms( $post_id, $taxonomy )`, in WordPress's order.
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term>;

    /// `get_post_meta( $post_id, $key, true )`, `None` when unset.
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>>;

    /// The published posts a `WP_Query` with `query`'s arguments finds.
    fn query(&self, query: &PostQuery) -> Vec<Post>;

    /// `get_userdata( $id )`. Sites without user data have none.
    fn user(&self, _id: u64) -> Option<User> {
        None
    }

    /// `get_adjacent_post()` for a visitor: the published post of `post`'s
    /// type just before (`previous`) or after it by date, then ID, sharing
    /// one of `terms` in their taxonomy when given. Sites without other
    /// posts have none.
    fn adjacent_post(
        &self,
        _post: &Post,
        _previous: bool,
        _terms: Option<(&str, &[u64])>,
    ) -> Option<Post> {
        None
    }
}

/// The main query's conditionals that blocks check.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    /// `is_front_page()`
    pub front_page: bool,
    /// `is_home()`
    pub home: bool,
    /// `is_paged()`
    pub paged: bool,
    /// `get_queried_object_id()`
    pub queried_object_id: u64,
    /// The global `$post`: the main query's first post, which top-level
    /// blocks get as their `postId` context.
    pub post: Option<u64>,
    /// `time()`, which relative dates are measured from.
    pub now: i64,
    /// The main query's posts, which query blocks that inherit it list.
    pub posts: Vec<Post>,
    /// `is_archive()`: which archive, if any.
    pub archive: Option<Archive>,
    /// `is_search()`: the raw search query, `get_query_var( 's' )`.
    pub search: Option<Vec<u8>>,
    /// `is_singular()`: the main query's one post is the queried object.
    pub singular: bool,
    /// `is_404()`
    pub not_found: bool,
    /// The `page` or `paged` query var, 0 or 1 on the first page.
    pub page: u64,
}

/// The archives the default install has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Archive {
    /// A category.
    Category(Term),
    /// `?m=YYYY`.
    Year,
    /// `?m=YYYYMM`.
    Month,
    /// `?m=YYYYMMDD`.
    Day,
}
