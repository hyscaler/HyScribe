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
