// SPDX-License-Identifier: AGPL-3.0-or-later

//! The elements block support: a block whose `style.elements` colors its
//! buttons, links or headings gets a `wp-elements-<n>` class and rules
//! scoped to it in the block supports store.

use hyscribe_html::TagProcessor;
use hyscribe_phpser::{Key, Value, isset, lookup};

use crate::blocks::Block;
use crate::layout::Rule;
use crate::registry::block_type;
use crate::style::parse_block_styles;
use crate::supports::skips_serialization;
use crate::template::Renderer;

const HEADINGS: [&str; 6] = ["h1", "h2", "h3", "h4", "h5", "h6"];

/// An element type: its name, its selector under the class and, for
/// links, the hover selector.
struct Element {
    name: &'static str,
    selector: fn(&str) -> String,
    hover: Option<fn(&str) -> String>,
}

/// The element types, in the order their rules are stored.
const ELEMENTS: [Element; 3] = [
    Element {
        name: "button",
        selector: |c| format!(".{c} .wp-element-button, .{c} .wp-block-button__link"),
        hover: None,
    },
    Element {
        name: "link",
        selector: |c| format!(".{c} a:where(:not(.wp-element-button))"),
        hover: Some(|c| format!(".{c} a:where(:not(.wp-element-button)):hover")),
    },
    Element {
        name: "heading",
        selector: |c| HEADINGS.map(|h| format!(".{c} {h}")).join(", "),
        hover: None,
    },
];

/// `wp_should_add_elements_class_name()`'s color paths of an element type.
fn colored(name: &str, elements: &Value) -> bool {
    let set = |path: &[&str]| isset(elements.get_path(path));
    let colors = |element| {
        ["text", "background", "gradient"]
            .iter()
            .any(|c| set(&[element, "color", c]))
    };
    match name {
        "button" => colors("button"),
        "link" => set(&["link", "color", "text"]) || set(&["link", ":hover", "color", "text"]),
        _ => colors("heading") || HEADINGS.iter().any(|h| colors(h)),
    }
}

impl Renderer<'_> {
    /// `wp_render_elements_support_styles()`, the `render_block_data`
    /// filter: the block with the class added to `className`, when an
    /// element it serializes has a color.
    pub(crate) fn elements(&mut self, block: &Block) -> Option<Block> {
        let block_type = block.name.as_deref().and_then(block_type)?;
        let elements = block
            .attrs
            .as_deref()
            .and_then(|attrs| lookup(attrs, "style"))
            .and_then(|style| style.get_str("elements"))
            .filter(|elements| !elements.is_empty())?;
        let shown: Vec<&Element> = ELEMENTS
            .iter()
            .filter(|e| !skips_serialization(block_type, "color", Some(e.name)))
            .collect();
        if !shown.iter().any(|e| colored(e.name, elements)) {
            return None;
        }
        self.elements_id += 1;
        let class = format!("wp-elements-{}", self.elements_id);
        let mut store = |styles: Option<&Value>, selector: String| {
            let styles = styles.filter(|s| !s.is_empty())?;
            let declarations = parse_block_styles(styles, false).declarations;
            (!declarations.is_empty()).then(|| {
                self.assets.rules.push(Rule {
                    selector,
                    declarations,
                    group: None,
                })
            })
        };
        for element in shown {
            let styles = elements.get_str(element.name);
            store(styles, (element.selector)(&class));
            if let Some(hover) = element.hover {
                store(styles.and_then(|s| s.get_str(":hover")), hover(&class));
            }
            if element.name == "heading" {
                for heading in HEADINGS {
                    store(elements.get_str(heading), format!(".{class} {heading}"));
                }
            }
        }
        let mut block = block.clone();
        let entries = block.attrs.get_or_insert_default();
        let key = Key::String(b"className".to_vec());
        match entries.iter_mut().rev().find(|(k, _)| *k == key) {
            Some((_, Value::Null)) | None => entries.push((key, Value::String(class.into_bytes()))),
            Some((_, existing)) => {
                *existing =
                    Value::String([&existing.to_php_string()[..], b" ", class.as_bytes()].concat())
            }
        }
        Some(block)
    }
}

/// `wp_render_elements_class_name()`, the `render_block` filter: the first
/// `wp-elements-` class of `className` goes on the first tag.
pub(crate) fn elements_class(html: Vec<u8>, block: &Block) -> Vec<u8> {
    let class = block
        .attrs
        .as_deref()
        .and_then(|attrs| lookup(attrs, "className"))
        .and_then(Value::as_bytes)
        .and_then(|names| {
            names
                .split(|b| b" \t\x0c\r\n".contains(b))
                .find(|token| token.starts_with(b"wp-elements-"))
        });
    let Some(class) = class else {
        return html;
    };
    let mut tags = TagProcessor::new(&html);
    if tags.next_tag(None, None) {
        tags.add_class(class);
    }
    tags.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests;
