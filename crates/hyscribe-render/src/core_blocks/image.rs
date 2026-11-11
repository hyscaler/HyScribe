// SPDX-License-Identifier: AGPL-3.0-or-later

//! The image block, `render_block_core_image()`: its saved markup, gone
//! without an image source, with a gallery's `data-id`, a bound `id` in the
//! image's class and an empty caption left out. The lightbox, which needs
//! `uniqid()` and the footer's interactivity state, is not ported.

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_phpser::{Value, is_empty_string, isset};

use super::{Block, Context};
use crate::formatting::{contains, find};

/// `render_block_core_image()`.
pub(super) fn image(_: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let content = block.content;
    if find(&content.to_ascii_lowercase(), 0, b"<img").is_none() {
        return Vec::new();
    }
    let mut tags = TagProcessor::new(content);
    let has_src = tags.next_tag(Some(b"img"), None)
        && match tags.get_attribute(b"src") {
            Some(Attribute::Value(src)) => !is_empty_string(&src),
            Some(Attribute::True) => true,
            None => false,
        };
    if !has_src {
        return Vec::new();
    }
    let id = block.attribute("id");
    let bound = id.filter(|_| isset(block.attributes.get_path(&["metadata", "bindings", "id"])));
    if let Some(id) = bound {
        let wanted = [&b"wp-image-"[..], &id.to_php_string()].concat();
        if let Some(Attribute::Value(classes)) = tags.get_attribute(b"class")
            && !contains(&classes, &wanted)
        {
            tags.set_attribute(b"class", &rename_image_classes(&classes, &wanted));
        }
    }
    if let Some(data_id) = block.attribute("data-id") {
        match bound.unwrap_or(data_id) {
            Value::Bool(true) => tags.set_boolean_attribute(b"data-id"),
            Value::Bool(false) => tags.remove_attribute(b"data-id"),
            value => tags.set_attribute(b"data-id", &value.to_php_string()),
        };
    }
    // Where the empty caption was, in the markup as given.
    let caption = block.attribute("caption").is_none_or(Value::is_empty);
    let span = if caption && tags.next_tag(Some(b"FIGCAPTION"), None) {
        empty_caption(&mut tags)
    } else {
        None
    };
    let output = tags.get_updated_html().to_vec();
    match span {
        Some((start, end)) => [
            output.get(..start).unwrap_or_default(),
            output.get(end..).unwrap_or_default(),
        ]
        .concat(),
        None => output,
    }
}

/// The span from the current `<figcaption>` to its closer, when only
/// comments are between.
fn empty_caption(tags: &mut TagProcessor) -> Option<(usize, usize)> {
    let start = tags.token_start().unwrap_or_default();
    let mut more = tags.next_token();
    while more && tags.is_comment() {
        more = tags.next_token();
    }
    let closer = tags.get_tag().as_deref() == Some(b"FIGCAPTION") && tags.is_tag_closer();
    closer.then(|| (start, tags.parsed_up_to()))
}

/// `preg_replace( '/wp-image-(\d+)/', $wanted, $classes )`.
fn rename_image_classes(classes: &[u8], wanted: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = find(classes, at, b"wp-image-") {
        let digits = classes
            .iter()
            .skip(start + 9)
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            out.extend_from_slice(classes.get(at..start + 9).unwrap_or_default());
            at = start + 9;
            continue;
        }
        out.extend_from_slice(classes.get(at..start).unwrap_or_default());
        out.extend_from_slice(wanted);
        at = start + 9 + digits;
    }
    out.extend_from_slice(classes.get(at..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests;
