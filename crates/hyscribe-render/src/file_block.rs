// SPDX-License-Identifier: AGPL-3.0-or-later

//! The file block, `render_block_core_file()`: a PDF preview made into an
//! interactive region that shows only where the browser can preview PDFs.

use hyscribe_html::{Attribute, TagProcessor};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `render_block_core_file()`.
    pub(crate) fn file(&mut self, block: &Block) -> Vec<u8> {
        let content = self.saved_markup(block);
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let preview = prepared("core/file", parsed)
            .get_str("displayPreview")
            .is_some_and(|v| !v.is_empty());
        if !preview {
            return content;
        }
        self.assets
            .enqueue_module("@wordpress/block-library/file/view");
        let mut tags = TagProcessor::new(&content);
        // With no tag at all this sets nothing, and no `<object>` follows.
        tags.next_tag(None, None);
        tags.set_attribute(b"data-wp-interactive", b"core/file");
        if !tags.next_tag(Some(b"OBJECT"), None) {
            return content;
        }
        tags.set_attribute(b"data-wp-bind--hidden", b"!state.hasPdfPreview");
        tags.set_boolean_attribute(b"hidden");
        let label = match tags.get_attribute(b"aria-label") {
            Some(Attribute::Value(name))
                if !hyscribe_phpser::is_empty_string(&name) && name != b"PDF embed" =>
            {
                [&b"Embed of "[..], &name, b"."].concat()
            }
            _ => b"PDF embed".to_vec(),
        };
        tags.set_attribute(b"aria-label", &label);
        tags.get_updated_html().to_vec()
    }
}

#[cfg(test)]
mod tests;
