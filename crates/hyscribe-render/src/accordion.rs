// SPDX-License-Identifier: AGPL-3.0-or-later

//! The accordion blocks: `render_block_core_accordion()` and
//! `block_core_accordion_item_render()`, which add the Interactivity API
//! directives the accordion's root block then processes.

use hyscribe_html::TagProcessor;
use hyscribe_phpser::Value;

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `render_block_core_accordion()`: the store and its `autoclose`.
    pub(crate) fn accordion(&mut self, block: &Block) -> Vec<u8> {
        let content = self.saved_markup(block);
        if content.is_empty() {
            return content;
        }
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let autoclose = flag(&prepared("core/accordion", parsed), "autoclose");
        let mut tags = TagProcessor::new(&content);
        if !tags.next_tag(None, Some(b"wp-block-accordion")) {
            return content;
        }
        tags.set_attribute(b"data-wp-interactive", b"core/accordion");
        let context = format!("{{ \"autoclose\": {autoclose}, \"accordionItems\": [] }}");
        tags.set_attribute(b"data-wp-context", context.as_bytes());
        tags.get_updated_html().to_vec()
    }

    /// `block_core_accordion_item_render()`: an ID for the item, its toggle
    /// and its panel, with their directives, set only when all three are
    /// found; images of an item shown closed load with low priority.
    pub(crate) fn accordion_item(&mut self, block: &Block) -> Vec<u8> {
        let mut content = self.saved_markup(block);
        if content.is_empty() {
            return content;
        }
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let open = flag(&prepared("core/accordion-item", parsed), "openByDefault");
        let id = String::from_utf8_lossy(&self.unique_id("accordion-item-")).into_owned();
        let mut tags = TagProcessor::new(&content);
        if tags.next_tag(None, Some(b"wp-block-accordion-item")) {
            let context = format!("{{ \"id\": \"{id}\", \"openByDefault\": {open} }}");
            tags.set_attribute(b"data-wp-context", context.as_bytes());
            tags.set_attribute(b"data-wp-class--is-open", b"state.isOpen");
            tags.set_attribute(b"data-wp-init", b"callbacks.initAccordionItems");
            tags.set_attribute(b"data-wp-on-window--hashchange", b"callbacks.hashChange");
            if tags.next_tag(None, Some(b"wp-block-accordion-heading__toggle")) {
                tags.set_attribute(b"data-wp-on--click", b"actions.toggle");
                tags.set_attribute(b"id", id.as_bytes());
                tags.set_attribute(b"aria-controls", format!("{id}-panel").as_bytes());
                tags.set_attribute(b"data-wp-bind--aria-expanded", b"state.isOpen");
                if tags.next_tag(None, Some(b"wp-block-accordion-panel")) {
                    tags.set_attribute(b"id", format!("{id}-panel").as_bytes());
                    tags.set_attribute(b"aria-labelledby", id.as_bytes());
                    tags.set_attribute(b"data-wp-bind--hidden", b"state.isHidden");
                    tags.set_attribute(b"data-wp-on--beforematch", b"actions.handleBeforeMatch");
                    content = tags.get_updated_html().to_vec();
                }
            }
        }
        if !open {
            let mut images = TagProcessor::new(&content);
            while images.next_tag(Some(b"img"), None) {
                images.set_attribute(b"fetchpriority", b"low");
            }
            content = images.get_updated_html().to_vec();
        }
        content
    }
}

/// A boolean attribute as `true` or `false`.
fn flag(attributes: &Value, name: &str) -> bool {
    attributes.get_str(name).is_some_and(|v| !v.is_empty())
}

#[cfg(test)]
mod tests;
