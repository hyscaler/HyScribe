// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block style variation support: a block with an `is-style-<name>`
//! class whose variation has styles gets an instance class
//! (`is-style-<name>--<n>`) and a stylesheet scoped to it.

use hyscribe_html::TagProcessor;
use hyscribe_phpser::{Key, Value, lookup};
use hyscribe_theme_json::instance_class;

use crate::blocks::Block;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `wp_render_block_style_variation_support_styles()`, the
    /// `render_block_data` filter: the block with the instance class added
    /// to `className`, when the instance prints CSS.
    pub(crate) fn style_variation(&mut self, block: &Block) -> Option<Block> {
        let name = block.name.as_deref()?;
        let class = class_name(block)?;
        let global = &self.context.theme.global;
        let variation = global.style_variation(name, class)?;
        let instance = self.unique_id(&format!("{variation}--"));
        let instance = String::from_utf8_lossy(&instance).into_owned();
        let global = &self.context.theme.global;
        let css = global.style_variation_css(name, &variation, &instance);
        (!css.is_empty()).then(|| {
            self.assets.variations.push(css);
            let mut class = class.to_vec();
            class.extend_from_slice(format!(" is-style-{instance}").as_bytes());
            let mut block = block.clone();
            for (key, value) in block.attrs.iter_mut().flatten() {
                if *key == Key::String(b"className".to_vec()) {
                    *value = Value::String(class.clone());
                }
            }
            block
        })
    }
}

/// The block's `className` attribute, when it is a string.
fn class_name(block: &Block) -> Option<&[u8]> {
    block
        .attrs
        .as_deref()
        .and_then(|attrs| lookup(attrs, "className"))
        .and_then(Value::as_bytes)
}

/// `wp_render_block_style_variation_class_name()`, the `render_block`
/// filter: the first instance class of `className` goes on the first tag.
pub(crate) fn variation_class(html: Vec<u8>, block: &Block) -> Vec<u8> {
    let Some(instance) = class_name(block).and_then(instance_class) else {
        return html;
    };
    let mut tags = TagProcessor::new(&html);
    if tags.next_tag(None, None) {
        tags.add_class(instance);
    }
    tags.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests;
