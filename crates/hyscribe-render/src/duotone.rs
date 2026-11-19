// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Duotone`: the duotone support of blocks. A block with a duotone
//! filter gets a `wp-duotone-<slug>` class on its first tag; its filter's
//! rule goes to the block supports store, and its SVG to the footer, and a
//! preset's custom property to the global styles.

mod colord;

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_attr, sanitize_key};
use hyscribe_phpser::{Value, float_to_string, lookup};
use hyscribe_theme_json::{GlobalStyles, duotone_slug, kebab_case};

use crate::blocks::Block;
use crate::layout::{Rule, has_support};
use crate::registry::block_type;
use crate::template::Renderer;

/// A color's red, green and blue (0 to 255) and alpha (0 to 1), as
/// `WP_Duotone::colord_parse()` reads a hex, `rgb()` or `hsl()` string: the
/// entry point for fuzzing.
pub fn parse_color(input: &[u8]) -> Option<[f64; 4]> {
    colord::parse(input).map(|c| [c.r, c.g, c.b, c.a])
}

/// The colors of a filter: a CSS value such as `unset` for a preset, or the
/// colors of its table.
#[derive(Clone, Debug, PartialEq)]
enum Colors {
    Css(String),
    List(Vec<Vec<u8>>),
}

/// A filter's source: its slug and colors.
#[derive(Clone, Debug, PartialEq)]
struct Filter {
    slug: String,
    colors: Colors,
}

/// What the duotone support collected while a page rendered.
#[derive(Debug, Default)]
pub(crate) struct Duotone {
    /// `$block_css_declarations`: each rule's selector and `filter` value.
    css: Vec<(String, String)>,
    /// `$used_svg_filter_data`, by filter ID.
    svgs: Vec<(String, Filter)>,
    /// `$used_global_styles_presets`, by filter ID.
    presets: Vec<(String, Filter)>,
}

/// `$array[ $key ] = $value`: a key keeps the place it first had.
fn put(entries: &mut Vec<(String, Filter)>, key: String, value: Filter) {
    match entries.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => entries.push((key, value)),
    }
}

/// `get_filter_id( $slug )`.
fn filter_id(slug: &str) -> String {
    format!("wp-duotone-{slug}")
}

/// A global duotone preset as `self::$global_styles_presets` holds it.
fn presets_of(global: &GlobalStyles) -> Vec<(String, Filter)> {
    let mut found = Vec::new();
    for (_, presets) in global.presets(&["color", "duotone"]) {
        for (_, preset) in presets {
            let slug = preset
                .get_str("slug")
                .map_or(Vec::new(), Value::to_php_string);
            let slug = String::from_utf8_lossy(&slug).into_owned();
            let colors = match preset.get_str("colors") {
                Some(Value::Array(list)) => {
                    Colors::List(list.iter().map(|(_, c)| c.to_php_string()).collect())
                }
                other => Colors::Css(
                    String::from_utf8_lossy(&other.map_or(Vec::new(), Value::to_php_string))
                        .into_owned(),
                ),
            };
            let id = filter_id(&kebab_case(&slug));
            put(&mut found, id, Filter { slug, colors });
        }
    }
    found
}

impl Duotone {
    /// `WP_Duotone::render_duotone_support()`, the `render_block` filter:
    /// the class of the block's filter on its first tag.
    pub(crate) fn render(
        &mut self,
        global: &GlobalStyles,
        block: &Block,
        html: Vec<u8>,
        unique_id: &mut dyn FnMut(&str) -> String,
    ) -> Vec<u8> {
        let Some(name) = block.name.as_deref() else {
            return html;
        };
        let Some(selector) = block_type(name)
            .filter(|block_type| has_support(block_type, &["filter", "duotone"]))
            .and_then(|_| global.duotone_selector(name))
        else {
            return html;
        };
        let attribute = block
            .attrs
            .as_deref()
            .and_then(|attrs| lookup(attrs, "style"))
            .and_then(|style| style.get_path(&["color", "duotone"]))
            .filter(|value| **value != Value::Null);
        let by_global = global.duotone_blocks();
        let from_global = by_global.iter().find(|(block, _)| block == name);
        let id = match (attribute, from_global) {
            (Some(attribute), _) => self.with_attribute(global, selector, attribute, unique_id),
            (None, Some((_, slug))) => self.preset(global, selector, slug),
            (None, None) => return html,
        };
        let mut tags = TagProcessor::new(&html);
        if tags.next_tag(None, None) {
            tags.add_class(id.as_bytes());
        }
        tags.get_updated_html().to_vec()
    }

    /// The branch of `render_duotone_support()` for a block's own
    /// attribute: a preset's, a CSS value's or custom colors'. The filter
    /// ID, empty for a value of another type.
    fn with_attribute(
        &mut self,
        global: &GlobalStyles,
        selector: &str,
        attribute: &Value,
        unique_id: &mut dyn FnMut(&str) -> String,
    ) -> String {
        match attribute {
            Value::String(text) => {
                let slug = duotone_slug(text);
                let known = presets_of(global);
                if known.iter().any(|(id, _)| *id == filter_id(&slug)) {
                    return self.preset(global, selector, &slug);
                }
                let slug = unique_id(&format!("{}-", sanitize_key(text)));
                let id = filter_id(&slug);
                self.block_css(&id, selector, &String::from_utf8_lossy(text));
                id
            }
            Value::Array(colors) => {
                let colors: Vec<Vec<u8>> = colors.iter().map(|(_, c)| c.to_php_string()).collect();
                let joined = colors.join(&b'-');
                let slug = unique_id(&format!("{}-", sanitize_key(&joined)));
                let id = filter_id(&slug);
                let value = format!("url(#{id})");
                self.svgs.push((
                    id.clone(),
                    Filter {
                        slug,
                        colors: Colors::List(colors),
                    },
                ));
                self.block_css(&id, selector, &value);
                id
            }
            // `$filter_id` is not set: the class is empty.
            _ => String::new(),
        }
    }

    /// `enqueue_global_styles_preset()` for the preset `slug`, which may
    /// not exist (then no filter is enqueued): the filter ID.
    fn preset(&mut self, global: &GlobalStyles, selector: &str, slug: &str) -> String {
        let id = filter_id(slug);
        let known = presets_of(global);
        if let Some((_, preset)) = known.into_iter().find(|(known, _)| *known == id) {
            put(&mut self.presets, id.clone(), preset.clone());
            put(&mut self.svgs, id.clone(), preset);
            self.block_css(
                &id,
                selector,
                &format!("var(--wp--preset--duotone--{slug})"),
            );
        }
        id
    }

    /// `enqueue_block_css()`: the filter on each part of the selector under
    /// the filter's class.
    fn block_css(&mut self, id: &str, selector: &str, value: &str) {
        let scoped: Vec<String> = selector
            .split(',')
            .map(|part| format!(".{id}{}", part.trim_matches(is_trim)))
            .collect();
        self.css.push((scoped.join(", "), value.to_owned()));
    }

    /// The rules `output_block_styles()` adds to the block supports store.
    pub(crate) fn rules(&self) -> Vec<Rule> {
        self.css
            .iter()
            .map(|(selector, value)| Rule {
                selector: selector.clone(),
                declarations: vec![(
                    "filter".to_owned(),
                    Value::String(value.as_bytes().to_vec()),
                )],
                group: None,
            })
            .collect()
    }

    /// `output_global_styles()`: the custom properties of the presets used,
    /// for the global styles; empty without one.
    pub(crate) fn global_styles(&self) -> String {
        if self.presets.is_empty() {
            return String::new();
        }
        let mut css = String::from(":root{");
        for (id, preset) in &self.presets {
            let value = match &preset.colors {
                Colors::Css(text) => text.clone(),
                Colors::List(_) => format!("url(#{id})"),
            };
            css.push_str(&format!("--wp--preset--duotone--{}:{value};", preset.slug));
        }
        css.push('}');
        css
    }

    /// `output_footer_assets()`: the SVG filters of the filters used.
    pub(crate) fn footer(&self) -> Vec<u8> {
        self.svgs
            .iter()
            .flat_map(|(id, filter)| svg(id, &filter.colors))
            .collect()
    }
}

/// `trim()`'s default characters.
fn is_trim(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\0' | '\x0b')
}

/// `get_filter_svg( $filter_id, $colors )`: the SVG, its whitespace
/// collapsed. A color that cannot be parsed is left out.
fn svg(id: &str, colors: &Colors) -> Vec<u8> {
    let mut channels: [Vec<String>; 4] = Default::default();
    if let Colors::List(list) = colors {
        for color in list.iter().filter_map(|color| colord::parse(color)) {
            channels[0].push(float_to_string(color.r / 255.0));
            channels[1].push(float_to_string(color.g / 255.0));
            channels[2].push(float_to_string(color.b / 255.0));
            channels[3].push(float_to_string(color.a));
        }
    }
    let table = |name: &str, values: &[String]| {
        format!(
            "<feFunc{name} type=\"table\" tableValues=\"{}\" />",
            esc_attr(values.join(" ").as_bytes())
        )
    };
    let [red, green, blue, alpha] = &channels;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 0 0\" width=\"0\" height=\"0\" focusable=\"false\" role=\"none\" style=\"visibility: hidden; position: absolute; left: -9999px; overflow: hidden;\" ><defs><filter id=\"{}\"><feColorMatrix color-interpolation-filters=\"sRGB\" type=\"matrix\" values=\" .299 .587 .114 0 0 .299 .587 .114 0 0 .299 .587 .114 0 0 .299 .587 .114 0 0 \" /><feComponentTransfer color-interpolation-filters=\"sRGB\" >{}{}{}{}</feComponentTransfer><feComposite in2=\"SourceGraphic\" operator=\"in\" /></filter></defs></svg>",
        esc_attr(id.as_bytes()),
        table("R", red),
        table("G", green),
        table("B", blue),
        table("A", alpha),
    )
    .into_bytes()
}

impl Renderer<'_> {
    /// The duotone support's `render_block` filter.
    pub(crate) fn duotone(&mut self, block: &Block, html: Vec<u8>) -> Vec<u8> {
        let global = &self.context.theme.global;
        let counter = &mut self.unique_id;
        self.duotone
            .render(global, block, html, &mut |prefix: &str| {
                *counter += 1;
                format!("{prefix}{counter}")
            })
    }
}

#[cfg(test)]
mod tests;
