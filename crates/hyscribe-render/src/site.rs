// SPDX-License-Identifier: AGPL-3.0-or-later

//! What dynamic blocks read from the site and the request (ADR 0009).

use hyscribe_model::Post;

/// The site's data, as blocks read it while rendering.
pub trait Site {
    /// `get_option( $name )` as stored, `None` when the option is missing.
    fn option(&self, name: &str) -> Option<Vec<u8>>;

    /// `home_url()`: the `home` option, or what `WP_HOME` sets.
    fn home_url(&self) -> Vec<u8>;

    /// `get_post( $id )`, whatever its type and status.
    fn post(&self, id: u64) -> Option<Post>;
}

/// The main query's conditionals that blocks check.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
}
