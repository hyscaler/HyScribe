// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress content rendering: the block parser, block themes,
//! templates and the dynamic core blocks.

pub mod autop;
pub mod balance;
pub mod blocks;
pub mod clickable;
mod comments;
mod content;
mod core_blocks;
pub mod date;
#[cfg(test)]
#[path = "tests/fixture.rs"]
mod fixture;
pub mod formatting;
mod interactivity;
pub mod kses;
mod layout;
pub mod link;
mod media;
mod navigation;
pub mod post;
mod query_loop;
pub mod registry;
pub mod site;
pub mod style;
pub mod supports;
pub mod template;
pub mod theme;
mod title;
