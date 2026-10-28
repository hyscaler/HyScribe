// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress rows as plain data: what `hyscribe-db` reads and
//! `hyscribe-render` renders (ADR 0009).

/// A row of `{prefix}posts`, with the columns rendering needs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Post {
    /// `ID`.
    pub id: u64,
    /// `post_author`.
    pub author: u64,
    /// `post_date`, as stored.
    pub date: String,
    /// `post_date_gmt`, as stored.
    pub date_gmt: String,
    /// `post_content`.
    pub content: Vec<u8>,
    /// `post_title`.
    pub title: Vec<u8>,
    /// `post_excerpt`.
    pub excerpt: Vec<u8>,
    /// `post_status`.
    pub status: String,
    /// `post_password`.
    pub password: Vec<u8>,
    /// `post_name`.
    pub name: Vec<u8>,
    /// `post_modified`, as stored.
    pub modified: String,
    /// `post_parent`.
    pub parent: u64,
    /// `guid`.
    pub guid: Vec<u8>,
    /// `menu_order`.
    pub menu_order: i64,
    /// `post_type`.
    pub post_type: String,
    /// `comment_count`.
    pub comment_count: i64,
}

/// A term of a taxonomy, as `get_the_terms()` returns it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Term {
    /// `term_id`.
    pub id: u64,
    /// `name`.
    pub name: Vec<u8>,
    /// `slug`.
    pub slug: Vec<u8>,
}

/// What a list of posts is filtered by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostQuery {
    /// Post types to include, such as `post`, or `post` and `page` for a
    /// search.
    pub post_types: Vec<String>,
    /// Search terms; every term must appear in the title, excerpt or content.
    pub search: Vec<String>,
    /// A category's `term_id`.
    pub category: Option<u64>,
    /// `YEAR(post_date)`.
    pub year: Option<u64>,
    /// `MONTH(post_date)`.
    pub month: Option<u64>,
    /// `DAYOFMONTH(post_date)`.
    pub day: Option<u64>,
    /// `post__in`: only these posts, when set.
    pub include: Option<Vec<u64>>,
    /// `post__not_in`.
    pub exclude: Vec<u64>,
    /// Oldest first, where WordPress defaults to newest first.
    pub ascending: bool,
    /// How many posts.
    pub limit: u32,
    /// How many to skip.
    pub offset: u32,
}
