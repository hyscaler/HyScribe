// SPDX-License-Identifier: AGPL-3.0-or-later

//! The icon block, `render_block_core_icon()` over `wp_get_icon()`: a core
//! icon's SVG with the block's colors, border, padding and width as its
//! style, flipped or rotated.

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_phpser::{Key, Value, is_empty_string, json_decode};

use super::{Block, Context};
use crate::blocks::is_pcre_space;
use crate::style::get_styles;

/// The registered icons' SVG as `WP_Icons_Registry` sanitizes it: each
/// one an `svg` element, without a `style` attribute.
const ICONS: &[u8] = include_bytes!("../../../../wordpress/wp-includes/icons.json");

/// `render_block_core_icon()`.
pub(super) fn icon(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(name) = block.attribute("icon").filter(|name| !name.is_empty()) else {
        return Vec::new();
    };
    let styles = get_styles(&block_styles(&block.attributes), false);
    let label = block
        .attribute("ariaLabel")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let svg = wp_get_icon(&name.to_php_string(), styles.classnames.as_bytes(), &label);
    if svg.is_empty() {
        return Vec::new();
    }
    let mut tags = TagProcessor::new(&svg);
    tags.next_tag(Some(b"SVG"), None);
    if !styles.css.is_empty() {
        tags.set_attribute(b"style", &styles.css);
    }
    let flag = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    if flag("flipHorizontal") {
        tags.add_class(b"is-flip-horizontal");
    }
    if flag("flipVertical") {
        tags.add_class(b"is-flip-vertical");
    }
    let rotation = block.attribute("rotation").map_or(0, Value::to_int);
    if rotation != 0 {
        let rotate = format!("rotate: {rotation}deg;");
        let style = match tags.get_attribute(b"style") {
            Some(Attribute::Value(style)) => [&style[..], b" ", rotate.as_bytes()].concat(),
            _ => rotate.into_bytes(),
        };
        tags.set_attribute(b"style", &style);
    }
    [
        format!("<div {}>", block.wrapper(context, b"")).as_bytes(),
        tags.get_updated_html(),
        b"</div>",
    ]
    .concat()
}

/// The styles the block hands the style engine: its colors and border
/// from presets or custom values, each side's border, its padding and
/// width.
fn block_styles(attributes: &Value) -> Value {
    let path = |path: &[&str]| attributes.get_path(path).cloned().unwrap_or(Value::Null);
    let preset = |name: &str, custom: &[&str]| match attributes.get_str(name) {
        Some(slug) => Value::String([b"var:preset|color|", &slug.to_php_string()[..]].concat()),
        None => path(custom),
    };
    let entry = |key: &str, value: Value| (Key::String(key.as_bytes().to_vec()), value);
    let mut border = Vec::new();
    for key in ["radius", "style", "width"] {
        if let Some(value) = attributes
            .get_path(&["style", "border", key])
            .filter(|v| **v != Value::Null)
        {
            border.push(entry(key, value.clone()));
        }
    }
    border.push(entry(
        "color",
        preset("borderColor", &["style", "border", "color"]),
    ));
    for side in ["top", "right", "bottom", "left"] {
        let part = |key| path(&["style", "border", side, key]);
        let values = ["color", "style", "width"].map(|key| entry(key, part(key)));
        border.push(entry(side, Value::Array(values.to_vec())));
    }
    let only = |key: &str, at: &[&str]| {
        Value::Array(
            attributes
                .get_path(at)
                .filter(|v| **v != Value::Null)
                .map(|v| entry(key, v.clone()))
                .into_iter()
                .collect(),
        )
    };
    Value::Array(vec![
        entry(
            "color",
            Value::Array(vec![
                entry("text", preset("textColor", &["style", "color", "text"])),
                entry(
                    "background",
                    preset("backgroundColor", &["style", "color", "background"]),
                ),
            ]),
        ),
        entry("border", Value::Array(border)),
        entry("spacing", only("padding", &["style", "spacing", "padding"])),
        entry(
            "dimensions",
            only("width", &["style", "dimensions", "width"]),
        ),
    ])
}

/// `wp_get_icon( $name, array( 'size' => null, ... ) )`: the icon with the
/// classes, and labelled or hidden from assistive technology.
fn wp_get_icon(name: &[u8], class: &[u8], label: &[u8]) -> Vec<u8> {
    static TABLE: OnceLock<HashMap<Vec<u8>, Vec<u8>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let icons = json_decode(ICONS).unwrap_or_default();
        let icons = icons.as_array().unwrap_or_default();
        icons
            .iter()
            .map(|(key, svg)| (key.to_php_string(), svg.to_php_string()))
            .collect()
    });
    let Some(svg) = table.get(name) else {
        return Vec::new();
    };
    let mut tags = TagProcessor::new(svg);
    tags.next_tag(Some(b"SVG"), None);
    // `preg_split( '/\s+/' )`.
    for class in class
        .split(|&b| is_pcre_space(b))
        .filter(|class| !class.is_empty())
    {
        tags.add_class(class);
    }
    if is_empty_string(label) {
        tags.set_attribute(b"aria-hidden", b"true");
        tags.set_attribute(b"focusable", b"false");
        tags.remove_attribute(b"role");
        tags.remove_attribute(b"aria-label");
    } else {
        tags.set_attribute(b"role", b"img");
        tags.set_attribute(b"aria-label", label);
        tags.remove_attribute(b"aria-hidden");
        tags.remove_attribute(b"focusable");
    }
    tags.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests;
