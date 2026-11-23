// SPDX-License-Identifier: AGPL-3.0-or-later

//! The gallery block, `block_core_gallery_render()` for saved images: a
//! unique class whose column gap goes to the block supports stylesheet,
//! and the `data-id` its images get from `render_block_data`. Not ported:
//! images from `dynamicContent`, `randomOrder`'s shuffle, the lightbox and
//! breakpoints a theme sets in `viewport` (the defaults are used).

use hyscribe_html::TagProcessor;
use hyscribe_kses::esc_attr;
use hyscribe_phpser::{Key, Value, isset, lookup};
use hyscribe_theme_json::kebab_case;

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::contains;
use crate::layout::Rule;
use crate::template::Renderer;

/// `WP_Theme_JSON::get_viewport_media_queries()` with the default
/// breakpoints.
const VIEWPORTS: [(&str, &str); 2] = [
    ("@mobile", "@media (width <= 480px)"),
    ("@tablet", "@media (480px < width <= 782px)"),
];

const FALLBACK_GAP: &str = "var( --wp--style--gallery-gap-default, var( --gallery-block--gutter-size, var( --wp--style--block-gap, 0.5em ) ) )";

impl Renderer<'_> {
    /// `block_core_gallery_render()` over the saved markup.
    pub(crate) fn gallery(&mut self, block: &Block) -> Vec<u8> {
        let mut block = block.clone();
        block.inner_blocks.iter_mut().for_each(data_id);
        let content = self.saved_markup(&block);
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/gallery", parsed);
        let class = String::from_utf8_lossy(&self.unique_id("wp-block-gallery-")).into_owned();
        let mut tags = TagProcessor::new(&content);
        tags.next_tag(None, None);
        tags.add_class(class.as_bytes());
        let style = attributes
            .get_str("style")
            .filter(|s| s.as_array().is_some());
        let global = self
            .context
            .theme
            .global
            .styles()
            .get_path(&["blocks", "core/gallery"]);
        let own = block_gap(style);
        let gap = own
            .or_else(|| block_gap(global))
            .cloned()
            .unwrap_or(Value::Null);
        let selector = format!(".wp-block-gallery.{class}");
        let rule = |gap: &Value, group: Option<&str>| Rule {
            selector: selector.clone(),
            declarations: vec![(
                "--wp--style--unstable-gallery-gap".into(),
                Value::String(column_gap(gap)),
            )],
            group: group.map(str::to_string),
        };
        self.assets.rules.push(rule(&gap, None));
        // The default breakpoints, as no theme here sets `viewport`.
        for (breakpoint, query) in VIEWPORTS {
            let viewport = block_gap(style.and_then(|style| style.get_str(breakpoint)));
            let global_viewport = block_gap(global.and_then(|g| g.get_str(breakpoint)));
            let gap = match (viewport, own, global_viewport) {
                (Some(gap), _, _) | (None, None, Some(gap)) => gap,
                _ => continue,
            };
            if *gap != Value::Null {
                self.assets.rules.push(rule(gap, Some(query)));
            }
        }
        tags.get_updated_html().to_vec()
    }
}

/// `$style['spacing']['blockGap']` when `spacing` is an array holding the
/// key, a null value included (`array_key_exists()`).
fn block_gap(style: Option<&Value>) -> Option<&Value> {
    style
        .and_then(|style| style.get_str("spacing"))
        .and_then(Value::as_array)
        .and_then(|spacing| lookup(spacing, "blockGap"))
}

/// `block_core_gallery_data_id_backcompatibility()` for one inner block.
fn data_id(inner: &mut Block) {
    if inner.name.as_deref() != Some("core/image") {
        return;
    }
    let Some(attrs) = inner.attrs.as_mut() else {
        return;
    };
    let given = |name| isset(lookup(attrs, name));
    if !given("data-id") && given("id") {
        let id = lookup(attrs, "id")
            .map(Value::to_php_string)
            .unwrap_or_default();
        attrs.push((
            Key::String(b"data-id".to_vec()),
            Value::String(esc_attr(&id).into_bytes()),
        ));
    }
}

/// `block_core_gallery_get_column_gap_value()`.
fn column_gap(gap: &Value) -> Vec<u8> {
    let gap = match gap {
        // `$gap['left'] ?? $fallback_gap`
        Value::Array(sides) => match lookup(sides, "left") {
            Some(Value::String(left)) => left.clone(),
            Some(Value::Null) | None => FALLBACK_GAP.as_bytes().to_vec(),
            Some(_) => Vec::new(),
        },
        Value::String(gap) => gap.clone(),
        _ => Vec::new(),
    };
    // Unsafe CSS is dropped for the fallback.
    let unsafe_css = gap.iter().any(|b| b"\\(&=}".contains(b)) || contains(&gap, b"/*");
    let gap = if unsafe_css && !hyscribe_phpser::is_empty_string(&gap) {
        Vec::new()
    } else if contains(&gap, b"var:preset|spacing|") {
        let slug = gap.rsplit(|b| *b == b'|').next().unwrap_or_default();
        format!(
            "var(--wp--preset--spacing--{})",
            kebab_case(&String::from_utf8_lossy(slug))
        )
        .into_bytes()
    } else {
        gap
    };
    match gap.as_slice() {
        b"" => FALLBACK_GAP.as_bytes().to_vec(),
        b"0" => b"0px".to_vec(),
        _ => gap,
    }
}

#[cfg(test)]
mod tests;
