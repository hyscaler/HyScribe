// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress content rendering: the block parser, block themes,
//! templates and the dynamic core blocks.

mod accordion;
mod archives;
mod attachment;
pub mod autop;
pub mod balance;
mod bindings;
pub mod blocks;
mod categories;
pub mod clickable;
mod comment_pages;
mod comments;
mod content;
pub mod content_tags;
mod core_blocks;
pub use core_blocks::{comment_avatar, number_format, user_avatar};
pub mod date;
mod elements;
mod excerpt;
pub use excerpt::{the_excerpt, wp_trim_words};
pub mod feed;
mod file_block;
#[cfg(test)]
#[path = "tests/fixture.rs"]
mod fixture;
pub mod formatting;
mod gallery;
pub mod head;
mod interactivity;
pub mod kses;
mod latest_posts;
mod layout;
pub mod link;
mod media;
mod media_text;
mod navigation;
pub mod permalinks;
mod playlist;
pub mod post;
mod query_loop;
mod query_pagination;
pub mod registry;
mod search;
pub mod site;
pub mod sitemap;
mod social_link;
pub mod style;
pub mod supports;
mod tabs;
pub mod template;
pub mod term_template;
pub mod theme;
mod title;
mod variations;
mod widgets;

pub use categories::{Walk, walk};
pub use query_loop::{order_by, search_terms, sticky_posts};
