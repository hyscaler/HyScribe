// SPDX-License-Identifier: AGPL-3.0-or-later

//! What dynamic blocks read from the site and the request (ADR 0009).

use hyscribe_model::{Comment, Post, PostQuery, Term, User};
use hyscribe_phpser::Value;

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

    /// `found_posts` of that query: its posts on every page.
    fn count_posts(&self, query: &PostQuery) -> u64 {
        let all = PostQuery {
            limit: u32::MAX,
            offset: 0,
            ..query.clone()
        };
        self.query(&all).len() as u64
    }

    /// A post's approved comments, oldest first, then by ID. Sites without
    /// comment data have none.
    fn comments(&self, _post_id: u64) -> Vec<Comment> {
        vec![]
    }

    /// `get_comment( $id )` when that comment is approved, of any type.
    /// Sites without comment data have none.
    fn comment(&self, _id: u64) -> Option<Comment> {
        None
    }

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
    /// The `paged` query var, as `absint()` casts it.
    pub paged: u64,
    /// `$wp_query->max_num_pages`.
    pub max_pages: u64,
    /// `$wp_query->found_posts`.
    pub found_posts: u64,
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
    /// `is_date()`, which a category archive with a date can be as well.
    pub date: bool,
    /// `is_search()`: the raw search query, `get_query_var( 's' )`.
    pub search: Option<Vec<u8>>,
    /// `get_query_var( 's' )`, which a 404 keeps where `search` is gone.
    pub search_query: Vec<u8>,
    /// `is_singular()`: the main query's one post is the queried object.
    pub singular: bool,
    /// `is_404()`
    pub not_found: bool,
    /// `is_feed()`: posts show in full, as a singular post does.
    pub feed: bool,
    /// The `page` query var, 0 or 1 on the first page.
    pub page: u64,
    /// `$_SERVER['REQUEST_URI']`: the path and query as requested.
    pub uri: Vec<u8>,
    /// `$_GET`: the query as `parse_str()` reads it.
    pub get: Value,
}

impl Request {
    /// `is_paged()`
    pub fn is_paged(&self) -> bool {
        self.paged > 1
    }
}

/// The taxonomies a visitor can query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taxonomy {
    /// `category`.
    Category,
    /// `post_tag`.
    Tag,
    /// `post_format`.
    Format,
}

/// The archives the default install has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Archive {
    /// A category, tag or post format archive, and its term, which a query
    /// naming no term that exists lacks.
    Term(Taxonomy, Option<Term>),
    /// `?m=YYYY`.
    Year,
    /// `?m=YYYYMM`.
    Month,
    /// `?m=YYYYMMDD`.
    Day,
    /// A week's or a time's: no year, month or day.
    Date,
}
