// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Theme_JSON::remove_insecure_properties( $theme_json, 'custom' )` for
//! a user without `edit_css`, which no HyScribe user has: every value that
//! `safecss_filter_attr()` refuses goes, and with it all custom CSS.

use hyscribe_kses::{esc_attr, esc_html, sanitize_html_class};
use hyscribe_phpser::Value;

use crate::blocks::{Blocks, ELEMENTS};
use crate::css::is_safe;
use crate::php::{self, php_string};
use crate::sanitize::{BLOCK_PSEUDO, ELEMENT_PSEUDO, lookup, sanitize};
use crate::schema::migrate;
use crate::stylesheet::{PROPERTIES, Tree};
use crate::theme_json::{Origin, PRESETS, PresetValue, refs, setting_paths};
use crate::typography::font_size_value;
use crate::viewport;

/// `INDIRECT_PROPERTIES_METADATA`.
const INDIRECT: &[(&str, &[&[&str]])] = &[
    ("gap", &[&["spacing", "blockGap"]]),
    ("column-gap", &[&["spacing", "blockGap", "left"]]),
    ("row-gap", &[&["spacing", "blockGap", "top"]]),
    (
        "max-width",
        &[&["layout", "contentSize"], &["layout", "wideSize"]],
    ),
    (
        "background-image",
        &[
            &["background", "backgroundImage", "url"],
            &["background", "gradient"],
        ],
    ),
];

/// The CSS properties each preset of `PRESETS_METADATA` is checked as.
const PRESET_PROPERTIES: [&[&str]; 10] = [
    &["aspect-ratio"],
    &["color", "background-color", "border-color"],
    &["background"],
    &["filter"],
    &["font-size"],
    &["font-family"],
    &["padding", "margin"],
    &["box-shadow"],
    &["border-radius"],
    &["width", "height", "min-height"],
];

/// The boolean settings `VALID_SETTINGS` marks, which
/// `preserve_valid_typed_settings()` keeps.
const TYPED_SETTINGS: [&[&str]; 3] = [
    &["lightbox", "enabled"],
    &["lightbox", "allowEditing"],
    &["blockVisibility", "allowEditing"],
];

/// `VALID_ORIGINS`.
const ORIGINS: [&str; 4] = ["default", "blocks", "theme", "custom"];

/// The value at `path`, or an empty array as `_wp_array_get()`'s default.
fn at(value: &Value, path: &[&str]) -> Value {
    php::get(value, path).cloned().unwrap_or_else(php::array)
}

/// What removes insecure values from the nodes of one theme.json.
struct Remover<'a> {
    /// Declarations are computed without a tree, as PHP passes none.
    bare: Tree<'a>,
    /// The viewport states styles may hold.
    states: Vec<String>,
}

impl Remover<'_> {
    /// `remove_insecure_styles( $input )`.
    fn styles(&self, input: &Value) -> Value {
        let mut output = php::array();
        for declaration in self.bare.compute(input, None, false) {
            let safe = is_safe(&declaration.name, &php_string(&declaration.value));
            let found = PROPERTIES.iter().find(|(p, _)| *p == declaration.name);
            if let Some((_, path)) = found.filter(|_| safe) {
                let value = at(input, path);
                if !matches!(value, Value::Array(_)) {
                    php::set(&mut output, path, value);
                }
            }
        }
        indirect(input, &mut output);
        output
    }

    /// The pseudo-class styles of `names` in `input`, into `output`.
    fn pseudo(&self, input: &Value, output: &mut Value, names: &[&str]) {
        for pseudo in names {
            if php::isset(php::get(input, &[pseudo])) {
                php::set(output, &[pseudo], self.styles(&at(input, &[pseudo])));
            }
        }
    }

    /// `remove_insecure_element_styles( $elements, $queries )`.
    fn elements(&self, elements: &Value) -> Value {
        let mut sanitized = php::array();
        for (name, _) in ELEMENTS {
            let input = php::get(elements, &[name]).cloned().unwrap_or(Value::Null);
            if input.is_empty() {
                continue;
            }
            let mut output = self.styles(&input);
            self.pseudo(&input, &mut output, lookup(&ELEMENT_PSEUDO, name));
            for state in &self.states {
                if php::isset(php::get(&input, &[state])) {
                    let state_input = at(&input, &[state]);
                    let mut state_output = self.styles(&state_input);
                    self.pseudo(
                        &state_input,
                        &mut state_output,
                        lookup(&ELEMENT_PSEUDO, name),
                    );
                    php::set(&mut output, &[state], state_output);
                }
            }
            php::set(&mut sanitized, &[name], output);
        }
        sanitized
    }

    /// `remove_insecure_inner_block_styles( $blocks, $queries )`.
    fn inner_blocks(&self, blocks: &Value) -> Value {
        let mut sanitized = php::array();
        for (key, input) in php::entries(blocks) {
            let name = php::key_str(key);
            let mut output = self.styles(input);
            if php::isset(php::get(input, &["elements"])) {
                php::set(
                    &mut output,
                    &["elements"],
                    self.elements(&at(input, &["elements"])),
                );
            }
            for state in &self.states {
                if php::isset(php::get(input, &[state])) {
                    let state_input = at(input, &[state]);
                    let mut state_output = self.styles(&state_input);
                    self.pseudo(
                        &state_input,
                        &mut state_output,
                        lookup(&BLOCK_PSEUDO, &name),
                    );
                    php::set(&mut output, &[state], state_output);
                }
            }
            php::set(&mut sanitized, &[&name], output);
        }
        sanitized
    }

    /// The viewport states of a node or variation, re-added from `input`
    /// into `output`, with the pseudo-classes of `block`.
    fn states(&self, input: &Value, output: &mut Value, block: Option<&str>) {
        for state in &self.states {
            if !php::isset(php::get(input, &[state])) {
                continue;
            }
            let state_input = at(input, &[state]);
            let mut state_output = self.styles(&state_input);
            if php::isset(php::get(&state_input, &["elements"])) {
                let elements = self.elements(&at(&state_input, &["elements"]));
                php::set(&mut state_output, &["elements"], elements);
            }
            if php::isset(php::get(&state_input, &["blocks"])) {
                let blocks = self.inner_blocks(&at(&state_input, &["blocks"]));
                php::set(&mut state_output, &["blocks"], blocks);
            }
            let pseudo = block.map_or(&[][..], |block| lookup(&BLOCK_PSEUDO, block));
            self.pseudo(&state_input, &mut state_output, pseudo);
            php::set(output, &[state], state_output);
        }
    }
}

/// `remove_indirect_properties( $input, $output )`.
fn indirect(input: &Value, output: &mut Value) {
    for (property, paths) in INDIRECT {
        for path in *paths {
            if let Some(Value::String(_)) = php::get(input, path)
                && is_safe(property, &php_string(&at(input, path)))
            {
                php::set(output, path, at(input, path));
            }
        }
    }
}

/// `remove_insecure_settings( $input, $is_root )`, `global` the site's
/// settings, which font sizes are computed with.
fn settings(input: &Value, is_root: bool, global: &Value) -> Value {
    let mut output = php::array();
    for (preset, properties) in PRESETS.iter().zip(PRESET_PROPERTIES) {
        for origin in ORIGINS {
            let path = [preset.path[0], preset.path[1], origin];
            let Some(presets) = php::get(input, &path).filter(|p| **p != Value::Null) else {
                continue;
            };
            let kept: Vec<Value> = php::entries(presets)
                .iter()
                .map(|(_, item)| item)
                .filter(|item| {
                    let name = php::get(item, &["name"]);
                    let slug = php::get(item, &["slug"]);
                    let (Some(Value::String(name)), Some(Value::String(slug))) = (name, slug)
                    else {
                        return false;
                    };
                    if esc_attr(esc_html(name).as_bytes()).as_bytes() != name.as_slice()
                        || sanitize_html_class(slug, b"") != *slug
                    {
                        return false;
                    }
                    let value = match preset.value {
                        // Duotone presets have no value, which no check passes.
                        PresetValue::Key("colors") => Value::Null,
                        PresetValue::Key(key) => {
                            php::get(item, &[key]).cloned().unwrap_or(Value::Null)
                        }
                        PresetValue::FontSize => {
                            font_size_value(item, &php::array(), global).unwrap_or(Value::Null)
                        }
                    };
                    properties
                        .iter()
                        .all(|property| is_safe(property, &php_string(&value)))
                })
                .cloned()
                .collect();
            if !kept.is_empty() {
                let list = kept.into_iter().enumerate();
                let list = list.map(|(at, item)| (hyscribe_phpser::Key::Int(at as i64), item));
                php::set(&mut output, &path, Value::Array(list.collect()));
            }
        }
    }
    indirect(input, &mut output);
    for path in TYPED_SETTINGS {
        if let Some(Value::Bool(value)) = php::get(input, path) {
            php::set(&mut output, path, Value::Bool(*value));
        }
    }
    if let (true, Some(viewport)) = (is_root, php::get(input, &["viewport"])) {
        php::set(&mut output, &["viewport"], viewport::sanitize(viewport));
    }
    output
}

/// `WP_Theme_JSON::remove_insecure_properties( $theme_json, 'custom' )`.
pub(crate) fn remove_insecure_properties(
    theme_json: Value,
    blocks: &Blocks,
    global: &Value,
) -> Value {
    let mut theme_json = sanitize(migrate(theme_json, Origin::Custom), blocks);
    let empty = php::array();
    let remover = Remover {
        bare: Tree {
            data: &empty,
            blocks,
            global,
        },
        states: viewport::media_queries(php::get(&theme_json, &["settings", "viewport"]))
            .into_iter()
            .map(|(state, _)| state)
            .collect(),
    };
    let tree = Tree {
        data: &theme_json,
        blocks,
        global,
    };
    let mut sanitized = php::array();
    for node in tree.style_nodes(true, true) {
        let path = refs(&node.path);
        let input = at(&theme_json, &path);
        if input.is_empty() {
            continue;
        }
        let block = path
            .contains(&"blocks")
            .then(|| path.get(2).copied())
            .flatten();
        let mut output = remover.styles(&input);
        let element = path.last().copied().unwrap_or_default();
        remover.pseudo(&input, &mut output, lookup(&ELEMENT_PSEUDO, element));
        remover.states(&input, &mut output, block);
        if !output.is_empty() {
            php::set(&mut sanitized, &path, output);
        }
        for variation in &node.variations {
            let path = refs(&variation.path);
            let input = at(&theme_json, &path);
            if input.is_empty() {
                continue;
            }
            let mut output = remover.styles(&input);
            if php::isset(php::get(&input, &["blocks"])) {
                php::set(
                    &mut output,
                    &["blocks"],
                    remover.inner_blocks(&at(&input, &["blocks"])),
                );
            }
            if php::isset(php::get(&input, &["elements"])) {
                php::set(
                    &mut output,
                    &["elements"],
                    remover.elements(&at(&input, &["elements"])),
                );
            }
            remover.states(&input, &mut output, block);
            if !output.is_empty() {
                php::set(&mut sanitized, &path, output);
            }
        }
    }
    for path in setting_paths(&theme_json) {
        let path = refs(&path);
        let input = at(&theme_json, &path);
        if input.is_empty() {
            continue;
        }
        let output = settings(&input, path == ["settings"], global);
        if !output.is_empty() {
            php::set(&mut sanitized, &path, output);
        }
    }
    for part in ["styles", "settings"] {
        match php::get(&sanitized, &[part]).filter(|v| !v.is_empty()) {
            Some(value) => php::set(&mut theme_json, &[part], value.clone()),
            None => php::unset(&mut theme_json, &[part]),
        }
    }
    theme_json
}

#[cfg(test)]
mod tests;
