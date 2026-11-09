// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress content rendering: the block parser, block themes,
//! templates and the dynamic core blocks.

mod archives;
pub mod autop;
pub mod balance;
pub mod blocks;
mod categories;
pub mod clickable;
mod comment_pages;
mod comments;
mod content;
mod core_blocks;
pub mod date;
mod elements;
mod excerpt;
pub mod feed;
#[cfg(test)]
#[path = "tests/fixture.rs"]
mod fixture;
pub mod formatting;
pub mod head;
mod interactivity;
pub mod kses;
mod latest_posts;
mod layout;
pub mod link;
mod media;
mod navigation;
pub mod permalinks;
pub mod post;
mod query_loop;
mod query_pagination;
pub mod registry;
mod search;
pub mod site;
pub mod sitemap;
pub mod style;
pub mod supports;
pub mod template;
pub mod theme;
mod title;
mod variations;

pub use query_loop::{search_terms, sticky_posts};
