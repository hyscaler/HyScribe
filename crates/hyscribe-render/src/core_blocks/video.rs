// SPDX-License-Identifier: AGPL-3.0-or-later

//! The video block, `render_block_core_video()`: the attachment's stored
//! width and height, and their aspect ratio, on the `<video>`.

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_phpser::Value;

use super::{Block, Context};
use crate::attachment::metadata;
use crate::formatting::contains;

/// `render_block_core_video()`.
pub(super) fn video(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let content = block.content;
    let id = match block.attribute("id") {
        Some(Value::Int(id)) if contains(content, b"<video") => u64::try_from(*id).unwrap_or(0),
        _ => 0,
    };
    let attachment = context
        .site
        .post(id)
        .filter(|post| post.post_type == "attachment");
    let meta = attachment.and_then(|_| metadata(context.site, id));
    let size = |key| match meta.as_ref().and_then(|meta| meta.get_str(key)) {
        Some(Value::Int(n)) if *n > 0 => Some(*n),
        _ => None,
    };
    let (Some(width), Some(height)) = (size("width"), size("height")) else {
        return content.to_vec();
    };
    let mut tags = TagProcessor::new(content);
    if !tags.next_tag(Some(b"VIDEO"), None) {
        return content.to_vec();
    }
    tags.set_attribute(b"width", width.to_string().as_bytes());
    tags.set_attribute(b"height", height.to_string().as_bytes());
    let style = match tags.get_attribute(b"style") {
        Some(Attribute::Value(style)) => style,
        _ => Vec::new(),
    };
    let ratio = format!("aspect-ratio: {width} / {height};");
    tags.set_attribute(b"style", &[ratio.as_bytes(), &style].concat());
    tags.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests;
