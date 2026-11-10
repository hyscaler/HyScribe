// SPDX-License-Identifier: AGPL-3.0-or-later

//! The button block, `render_block_core_button()`: its saved markup, gone
//! when the link or button is empty, with its width applied.

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_phpser::{Value, float_to_string, string_to_float};
use hyscribe_theme_json::kebab_case;

use super::{Block, Context};
use crate::formatting::find;

const PRESET: &str = "var:preset|dimension|";

/// `render_block_core_button()`.
pub(super) fn button(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let content = block.content;
    let mut tags = TagProcessor::new(content);
    // The last tag seen, a link or button if there is one; without one,
    // nothing is left after it, so the button counts as empty.
    let mut tag = None;
    let mut found = false;
    while !found && tags.next_tag(None, None) {
        let name = tags.get_tag().unwrap_or_default();
        found = name == b"A" || name == b"BUTTON";
        tag = Some(name);
    }
    let Some(tag) = tag else {
        return content.to_vec();
    };
    if !found || is_empty(content.get(tags.parsed_up_to()..).unwrap_or_default(), &tag) {
        return Vec::new();
    }
    let width = block
        .attributes
        .get_path(&["style", "dimensions", "width"])
        .filter(|w| !w.is_empty())
        .map(|w| String::from_utf8_lossy(&w.to_php_string()).into_owned());
    let Some(width) = width else {
        return content.to_vec();
    };
    let slug = width.strip_prefix(PRESET);
    let preset = slug.and_then(|slug| preset_size(context, slug));
    let resolved = preset.as_deref().unwrap_or(&width);
    let mut tags = TagProcessor::new(content);
    if !tags.next_tag(None, Some(b"wp-block-button")) {
        return content.to_vec();
    }
    tags.add_class(b"has-custom-width");
    let existing = match tags.get_attribute(b"style") {
        Some(Attribute::Value(style)) => style,
        _ => Vec::new(),
    };
    let declaration = if resolved.ends_with('%') {
        tags.add_class(b"wp-block-button__width");
        if ["25%", "50%", "75%", "100%"].contains(&resolved) {
            let legacy = format!("wp-block-button__width-{}", resolved.trim_end_matches('%'));
            tags.add_class(legacy.as_bytes());
        }
        let number = float_to_string(string_to_float(resolved.as_bytes()));
        format!("--wp--block-button--width: {number};")
    } else {
        let value = slug.map_or_else(
            || width.clone(),
            |slug| format!("var(--wp--preset--dimension--{})", kebab_case(slug)),
        );
        format!("width: {value};")
    };
    let style = if existing.is_empty() {
        declaration.into_bytes()
    } else {
        [declaration.as_bytes(), b" ", &existing].concat()
    };
    tags.set_attribute(b"style", &style);
    tags.get_updated_html().to_vec()
}

/// Whether the tokens after the opening `tag` reach its closer (or another
/// tag of its name, or the end) with nothing but comments between.
fn is_empty(mut rest: &[u8], tag: &[u8]) -> bool {
    loop {
        if rest.is_empty() {
            return true;
        }
        if let Some(comment) = rest.strip_prefix(b"<!--") {
            let Some(end) = find(comment, 0, b"-->") else {
                return true;
            };
            rest = comment.get(end + 3..).unwrap_or_default();
            continue;
        }
        let name = rest
            .strip_prefix(b"</")
            .or_else(|| rest.strip_prefix(b"<"))
            .and_then(|after| {
                after
                    .get(..tag.len())
                    .filter(|n| n.eq_ignore_ascii_case(tag))
                    .map(|_| after)
            })
            .is_some_and(|after| {
                after
                    .get(tag.len())
                    .is_none_or(|b| matches!(b, b' ' | b'\t' | b'\n' | 0x0c | b'\r' | b'/' | b'>'))
            });
        return name;
    }
}

/// The size of the button's dimension preset `slug`, from the first of
/// its custom, theme or default presets with that slug.
fn preset_size(context: &Context<'_>, slug: &str) -> Option<String> {
    let settings = context.theme.global.settings();
    let presets = |origin: &&str| {
        settings
            .get_path(&[
                "blocks",
                "core/button",
                "dimensions",
                "dimensionSizes",
                origin,
            ])
            .and_then(Value::as_array)
    };
    ["custom", "theme", "default"]
        .iter()
        .filter_map(presets)
        .flatten()
        .find(|(_, preset)| {
            preset
                .get_str("slug")
                .is_some_and(|s| s.to_php_string() == slug.as_bytes())
        })
        .and_then(|(_, preset)| preset.get_str("size"))
        .map(|size| String::from_utf8_lossy(&size.to_php_string()).into_owned())
}

#[cfg(test)]
mod tests;
