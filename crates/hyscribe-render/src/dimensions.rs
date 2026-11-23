// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_render_dimensions_support()`, the `render_block` filter that keeps
//! an aspect ratio and a minimum height from overriding each other.

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_phpser::{Key, Value, php_trim};

use crate::blocks::Block;
use crate::registry::block_type;
use crate::style::get_styles;
use crate::supports::skips_serialization;

/// `wp_is_explicit_aspect_ratio_value()`.
fn explicit(ratio: Option<&Value>) -> bool {
    let ratio = ratio.filter(|ratio| **ratio != Value::Null);
    ratio.is_some_and(|ratio| {
        let ratio = php_trim(&ratio.to_php_string()).to_ascii_lowercase();
        !ratio.is_empty() && ratio != b"auto"
    })
}

fn key(name: &str) -> Key {
    Key::String(name.as_bytes().to_vec())
}

/// The dimensions styles on the block's first tag: an explicit aspect
/// ratio unsets the heights, a minimum height unsets the aspect ratio.
pub(crate) fn dimensions(html: Vec<u8>, block: &Block) -> Vec<u8> {
    let Some(block_type) = block.name.as_deref().and_then(block_type) else {
        return html;
    };
    let supported = block_type
        .supports
        .get_path(&["dimensions", "aspectRatio"])
        .is_some_and(|support| !support.is_empty());
    if !supported || skips_serialization(block_type, "dimensions", Some("aspectRatio")) {
        return html;
    }
    let attrs = Value::Array(block.attrs.clone().unwrap_or_default());
    let ratio = attrs.get_path(&["style", "dimensions", "aspectRatio"]);
    let has = |path: &[&str]| attrs.get_path(path).is_some_and(|v| *v != Value::Null);
    let unset = || Value::String(b"unset".to_vec());
    let styles: Vec<(Key, Value)> = if explicit(ratio) {
        vec![
            (key("aspectRatio"), ratio.cloned().unwrap_or(Value::Null)),
            (key("minHeight"), unset()),
            (key("height"), unset()),
        ]
    } else if has(&["style", "dimensions", "minHeight"]) || has(&["minHeight"]) {
        vec![(key("aspectRatio"), unset())]
    } else {
        return html;
    };
    let styles = get_styles(
        &Value::Array(vec![(key("dimensions"), Value::Array(styles))]),
        false,
    );
    let mut tags = TagProcessor::new(&html);
    if styles.css.is_empty() || !tags.next_tag(None, None) {
        return html;
    }
    let mut style = match tags.get_attribute(b"style") {
        Some(Attribute::Value(style)) => style,
        _ => Vec::new(),
    };
    if !style.is_empty() && !style.ends_with(b";") {
        style.push(b';');
    }
    style.extend_from_slice(&styles.css);
    tags.set_attribute(b"style", &style);
    if explicit(ratio) {
        for class in styles
            .classnames
            .split(' ')
            .filter(|class| !class.is_empty())
        {
            tags.add_class(class.as_bytes());
        }
    }
    tags.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::parse_blocks;

    fn rendered(markup: &str, html: &str) -> String {
        let blocks = parse_blocks(markup.as_bytes());
        String::from_utf8(dimensions(html.as_bytes().to_vec(), &blocks[0])).unwrap()
    }

    /// As `wp_render_dimensions_support()` gives them.
    #[test]
    fn heights_and_ratios_unset_each_other() {
        let cover = |attrs: &str| format!("<!-- wp:cover {attrs} /-->");
        let html = "<div class=\"c\" style=\"color:red\">x</div>";
        assert_eq!(
            rendered(&cover(r#"{"minHeight":274}"#), html),
            "<div class=\"c\" style=\"color:red;aspect-ratio:unset;\">x</div>"
        );
        assert_eq!(
            rendered(
                &cover(r#"{"style":{"dimensions":{"aspectRatio":"16/9"}}}"#),
                "<div>x</div>"
            ),
            "<div class=\"has-aspect-ratio\" style=\"aspect-ratio:16/9;height:unset;min-height:unset;\">x</div>"
        );
        assert_eq!(
            rendered(
                &cover(r#"{"style":{"dimensions":{"aspectRatio":"auto"}}}"#),
                html
            ),
            html
        );
        assert_eq!(
            rendered("<!-- wp:paragraph {\"minHeight\":1} /-->", html),
            html
        );
        assert_eq!(rendered("<p>x</p>", html), html);
        assert_eq!(rendered(&cover(r#"{"minHeight":1}"#), "text"), "text");
    }
}
