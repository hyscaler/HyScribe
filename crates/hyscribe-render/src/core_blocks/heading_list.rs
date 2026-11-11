// SPDX-License-Identifier: AGPL-3.0-or-later

//! The heading and list blocks, `block_core_heading_render()` and
//! `block_core_list_render()`: their class on the first heading or list
//! tag, which markup saved before the class existed lacks.

use hyscribe_html::TagProcessor;

use super::{Block, Context};

/// `block_core_heading_render()`.
pub(super) fn heading(_: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    first_tag_class(
        block.content,
        &[b"H1", b"H2", b"H3", b"H4", b"H5", b"H6"],
        b"wp-block-heading",
    )
}

/// `block_core_list_render()`.
pub(super) fn list(_: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    first_tag_class(block.content, &[b"OL", b"UL"], b"wp-block-list")
}

/// `class` added to the first tag named in `tags`; `"0"` and empty content
/// are falsy and stay as they are.
fn first_tag_class(content: &[u8], tags: &[&[u8]], class: &[u8]) -> Vec<u8> {
    if hyscribe_phpser::is_empty_string(content) {
        return content.to_vec();
    }
    let mut processor = TagProcessor::new(content);
    while processor.next_tag(None, None) {
        if processor
            .get_tag()
            .is_some_and(|tag| tags.contains(&tag.as_slice()))
        {
            processor.add_class(class);
            break;
        }
    }
    processor.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests;
