// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block registry as `WP_Theme_JSON` sees it: each registered block's
//! selectors (`wp_get_block_css_selector()`, `get_block_selectors()`), the
//! selectors of its elements, its duotone selector and its style
//! variations (`get_blocks_metadata()`).

use hyscribe_phpser::Value;

use crate::php::{self, as_str, string};
use crate::selector;

/// A registered block type: its name and its `block.json` metadata (of which
/// `supports`, `selectors` and `styles` are read).
#[derive(Clone, Debug, PartialEq)]
pub struct BlockType {
    /// `namespace/name`.
    pub name: String,
    /// The decoded `block.json`.
    pub metadata: Value,
}

/// The elements `styles.elements` may style, with their selectors.
pub(crate) const ELEMENTS: [(&str, &str); 13] = [
    ("link", "a:where(:not(.wp-element-button))"),
    ("heading", "h1, h2, h3, h4, h5, h6"),
    ("h1", "h1"),
    ("h2", "h2"),
    ("h3", "h3"),
    ("h4", "h4"),
    ("h5", "h5"),
    ("h6", "h6"),
    ("button", ".wp-element-button, .wp-block-button__link"),
    (
        "caption",
        ".wp-element-caption, .wp-block-audio figcaption, .wp-block-embed figcaption, .wp-block-gallery figcaption, .wp-block-image figcaption, .wp-block-table figcaption, .wp-block-video figcaption",
    ),
    ("cite", "cite"),
    (
        "textInput",
        "textarea, input:where([type=email],[type=number],[type=password],[type=search],[type=text],[type=tel],[type=url])",
    ),
    ("select", "select"),
];

/// The selector of an element, if it is one.
pub(crate) fn element_selector(name: &str) -> Option<&'static str> {
    ELEMENTS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// Block supports whose styles may go under their own selector.
const FEATURE_LEVEL_SELECTORS: [(&str, &str); 5] = [
    ("__experimentalBorder", "border"),
    ("color", "color"),
    ("dimensions", "dimensions"),
    ("spacing", "spacing"),
    ("typography", "typography"),
];

/// What `get_blocks_metadata()` knows about one block.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BlockMeta {
    pub(crate) name: String,
    pub(crate) supports: Value,
    pub(crate) selector: String,
    pub(crate) selectors: Value,
    pub(crate) elements: Vec<(&'static str, String)>,
    pub(crate) duotone: Option<String>,
    pub(crate) style_variations: Vec<(String, String)>,
    pub(crate) states: Option<Value>,
}

/// The metadata of every registered block, in registration order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Blocks(Vec<BlockMeta>);

impl Blocks {
    /// Metadata for `types`, with `styles` (block name, style name) registered
    /// through `register_block_style()` after their `block.json` styles.
    pub(crate) fn new(types: &[BlockType], styles: &[(String, String)]) -> Blocks {
        let mut blocks = Blocks(types.iter().map(meta).collect());
        blocks.register_styles(styles);
        blocks
    }

    /// Adds style variations that are not registered yet.
    pub(crate) fn register_styles(&mut self, styles: &[(String, String)]) {
        for (block, style) in styles {
            if let Some(meta) = self.0.iter_mut().find(|m| m.name == *block)
                && !meta.style_variations.iter().any(|(n, _)| n == style)
            {
                let selector = selector::variation(style, &meta.selector);
                meta.style_variations.push((style.clone(), selector));
            }
        }
    }

    pub(crate) fn get(&self, name: &str) -> Option<&BlockMeta> {
        self.0.iter().find(|m| m.name == name)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &BlockMeta> {
        self.0.iter()
    }
}

fn selectors(block: &BlockType) -> Option<&Value> {
    php::get(&block.metadata, &["selectors"]).filter(|s| !php::empty(Some(s)))
}

fn supports(block: &BlockType) -> &Value {
    php::get(&block.metadata, &["supports"]).unwrap_or(&Value::Null)
}

/// `wp_get_block_css_selector( $block_type )`.
fn root_selector(block: &BlockType) -> String {
    let from_selectors = selectors(block)
        .and_then(|s| php::get(s, &["root"]))
        .and_then(as_str);
    let experimental = php::get(supports(block), &["__experimentalSelector"]).and_then(as_str);
    match from_selectors.or(experimental) {
        Some(selector) => selector.to_owned(),
        None => format!(
            ".wp-block-{}",
            block.name.replace("core/", "").replace('/', "-")
        ),
    }
}

/// `wp_get_block_css_selector( $block_type, $feature )` for a block without
/// `selectors`: the feature's `__experimentalSelector`, scoped to the block.
fn experimental_selector(block: &BlockType, feature: &str) -> Option<String> {
    let experimental = php::get(supports(block), &[feature, "__experimentalSelector"])
        .filter(|v| php::isset(Some(v)))?;
    Some(selector::scope(
        &root_selector(block),
        &php::php_string(experimental),
    ))
}

/// `wp_get_block_css_selector( $block_type, 'filter.duotone' )`.
fn duotone_selector(block: &BlockType) -> Option<String> {
    selectors(block)
        .and_then(|s| php::get(s, &["filter", "duotone"]))
        .filter(|v| !php::empty(Some(v)))
        .and_then(as_str)
        .map(str::to_owned)
}

/// `get_block_selectors( $block_type, $root_selector )`.
fn block_selectors(block: &BlockType, root: &str) -> Value {
    if let Some(selectors) = selectors(block) {
        return selectors.clone();
    }
    let mut out = php::array();
    php::set(&mut out, &["root"], string(root));
    for (key, feature) in FEATURE_LEVEL_SELECTORS {
        if let Some(selector) = experimental_selector(block, key) {
            php::set(&mut out, &[feature, "root"], string(&selector));
        }
    }
    out
}

/// `get_block_element_selectors( $root_selector )`.
fn element_selectors(root: &str) -> Vec<(&'static str, String)> {
    let parts: Vec<&str> = root.split(',').collect();
    ELEMENTS
        .iter()
        .map(|&(name, element)| {
            let selector = if parts.contains(&element) {
                element.to_owned()
            } else {
                parts
                    .iter()
                    .map(|part| selector::prepend(element, &format!("{part} ")))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            (name, selector)
        })
        .collect()
}

fn meta(block: &BlockType) -> BlockMeta {
    let selector = root_selector(block);
    let duotone = duotone_selector(block).or_else(|| {
        let support = php::get(supports(block), &["color", "__experimentalDuotone"])?;
        if php::empty(Some(support)) {
            return None;
        }
        Some(selector::scope(&selector, &php::php_string(support)))
    });
    let mut style_variations = php::Ordered::new();
    for style in php::entries(php::get(&block.metadata, &["styles"]).unwrap_or(&Value::Null)) {
        let name = php::php_string(php::get(&style.1, &["name"]).unwrap_or(&Value::Null));
        let variation = selector::variation(&name, &selector);
        style_variations.put(name, variation);
    }
    let style_variations = style_variations.into_vec();
    let states = selectors(block)
        .and_then(|s| php::get(s, &["states"]))
        .filter(|s| !php::empty(Some(s)) && matches!(s, Value::Array(_)))
        .cloned();
    BlockMeta {
        name: block.name.clone(),
        supports: supports(block).clone(),
        selectors: block_selectors(block, &selector),
        elements: element_selectors(&selector),
        duotone,
        style_variations,
        states,
        selector,
    }
}

#[cfg(test)]
mod tests;
