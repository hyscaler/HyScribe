// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Theme_JSON_Resolver`: the core, blocks, theme and user layers, each
//! a `WP_Theme_JSON`, merged in that order.

use hyscribe_phpser::{Value, json_decode};

use crate::blocks::Blocks;
use crate::kebab::kebab_case;
use crate::php::{self, string};
use crate::schema::latest;
use crate::theme_json::{self, Origin};
use crate::{BlockType, Sources};

/// A decoded JSON file, or an empty array where PHP gets anything else.
fn decode(json: &[u8]) -> Value {
    json_decode(json)
        .filter(|v| matches!(v, Value::Array(_)))
        .unwrap_or_else(php::array)
}

/// `get_core_data()`.
pub(crate) fn core_data(core: &[u8], blocks: &Blocks) -> Value {
    theme_json::new(decode(core), Origin::Default, blocks)
}

/// `remove_json_comments()`: `//` keys go, at every level.
fn remove_comments(value: &Value) -> Value {
    match value {
        Value::Array(entries) => Value::Array(
            entries
                .iter()
                .filter(|(k, _)| php::key_str(k) != "//")
                .map(|(k, v)| (k.clone(), remove_comments(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// `get_block_data()`: the styles blocks declare in `supports`.
pub(crate) fn block_data(types: &[BlockType], blocks: &Blocks) -> Value {
    let mut config = latest();
    for block in types {
        let supports = php::get(&block.metadata, &["supports"]).unwrap_or(&Value::Null);
        let path = ["styles", "blocks", block.name.as_str()];
        if let Some(style) =
            php::get(supports, &["__experimentalStyle"]).filter(|s| php::isset(Some(s)))
        {
            php::set(&mut config, &path, remove_comments(style));
        }
        let default_gap = php::get(supports, &["spacing", "blockGap", "__experimentalDefault"]);
        let gap = [
            "styles",
            "blocks",
            block.name.as_str(),
            "spacing",
            "blockGap",
        ];
        if php::isset(default_gap) && !php::isset(php::get(&config, &gap)) {
            php::set(&mut config, &gap, Value::Null);
        }
    }
    theme_json::new(config, Origin::Blocks, blocks)
}

/// The block style variation partials of a theme (`get_style_variations(
/// 'block' )`), as raw data, in path order.
fn partials(files: &[(String, Vec<u8>)], blocks: &Blocks) -> Vec<Value> {
    let mut files: Vec<&(String, Vec<u8>)> = files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
        .into_iter()
        .filter_map(|(path, json)| {
            let decoded = json_decode(json).filter(|d| php::isset(php::get(d, &["blockTypes"])))?;
            let mut variation = theme_json::new(decoded, Origin::Theme, blocks);
            if php::empty(php::get(&variation, &["title"])) {
                let file = path.rsplit('/').next().unwrap_or_default();
                php::set(
                    &mut variation,
                    &["title"],
                    string(file.strip_suffix(".json").unwrap_or(file)),
                );
            }
            Some(variation)
        })
        .collect()
}

/// A partial's variation name: its slug, else its title in kebab case.
fn variation_name(variation: &Value) -> String {
    match php::get(variation, &["slug"]).filter(|s| php::isset(Some(s))) {
        Some(slug) => php::php_string(slug),
        None => kebab_case(&php::php_string(
            php::get(variation, &["title"]).unwrap_or(&Value::Null),
        )),
    }
}

/// The partials that apply, with their block types.
fn applicable(variations: &[Value]) -> impl Iterator<Item = (String, &Value, Vec<String>)> {
    variations.iter().filter_map(|variation| {
        let styles = php::get(variation, &["styles"]).filter(|s| !php::empty(Some(s)))?;
        let types = php::get(variation, &["blockTypes"]).filter(|t| !php::empty(Some(t)))?;
        let types = php::entries(types)
            .iter()
            .map(|(_, t)| php::php_string(t))
            .collect();
        Some((variation_name(variation), styles, types))
    })
}

/// `inject_variations_from_block_style_variation_files()`.
fn inject(mut data: Value, variations: &[Value]) -> Value {
    for (name, styles, types) in applicable(variations) {
        let mut styles = styles.clone();
        for block in types {
            for path in [
                &["styles", "variations", &name][..],
                &["styles", "blocks", &block, "variations", &name],
            ] {
                if let Some(over) = php::get(&data, path).filter(|v| !php::empty(Some(v))) {
                    php::replace_recursive(&mut styles, over);
                }
            }
            php::set(
                &mut data,
                &["styles", "blocks", &block, "variations", &name],
                styles.clone(),
            );
        }
    }
    data
}

/// `get_classic_theme_supports_block_editor_settings()` for a theme that
/// declares no theme supports, through `get_from_editor_settings()`.
fn theme_supports(has_theme_json: bool) -> Value {
    let mut config = latest();
    let settings: &[(&[&str], bool)] = &[
        (&["color", "custom"], true),
        (&["color", "customGradient"], true),
        (&["typography", "customFontSize"], true),
        (&["typography", "lineHeight"], false),
        (&["spacing", "units"], false),
        (&["spacing", "padding"], false),
    ];
    let classic: &[(&[&str], bool)] = &[
        (&["color", "defaultPalette"], true),
        (&["color", "defaultGradients"], true),
        (&["typography", "defaultFontSizes"], true),
        (&["spacing", "defaultSpacingSizes"], true),
        (&["shadow", "defaultPresets"], false),
    ];
    let extra = if has_theme_json { &[][..] } else { classic };
    for (path, value) in settings.iter().chain(extra) {
        let full: Vec<&str> = std::iter::once("settings")
            .chain(path.iter().copied())
            .collect();
        php::set(&mut config, &full, Value::Bool(*value));
    }
    config
}

/// `get_theme_data()`. Registers the theme's partial variations in
/// `blocks` first, as PHP does.
pub(crate) fn theme_data(sources: &Sources<'_>, blocks: &mut Blocks) -> Value {
    let variations = partials(sources.theme_styles, blocks);
    let styles: Vec<(String, String)> = applicable(&variations)
        .flat_map(|(name, _, types)| types.into_iter().map(move |t| (t, name.clone())))
        .collect();
    blocks.register_styles(&styles);
    let data = sources.theme.map_or_else(latest, decode);
    let theme = theme_json::new(inject(data, &variations), Origin::Theme, blocks);
    let mut with_supports = theme_json::new(
        theme_supports(sources.theme.is_some()),
        Origin::Theme,
        blocks,
    );
    theme_json::merge(&mut with_supports, &theme);
    with_supports
}

/// `get_user_data()`: the `wp_global_styles` post, when it is a user
/// theme.json.
pub(crate) fn user_data(content: Option<&[u8]>, blocks: &Blocks) -> Value {
    let decoded = content.and_then(json_decode);
    let config = match decoded {
        Some(mut data) if !php::empty(php::get(&data, &["isGlobalStylesUserThemeJSON"])) => {
            php::unset(&mut data, &["isGlobalStylesUserThemeJSON"]);
            data
        }
        _ => php::array(),
    };
    theme_json::new(config, Origin::Custom, blocks)
}

/// `get_merged_data()`, and the block metadata it ends with.
pub(crate) fn merged(sources: &Sources<'_>) -> (Value, Blocks) {
    let mut blocks = Blocks::new(sources.blocks, sources.block_styles);
    let mut result = theme_json::new(latest(), Origin::Theme, &blocks);
    theme_json::merge(&mut result, &core_data(sources.core, &blocks));
    theme_json::merge(&mut result, &block_data(sources.blocks, &blocks));
    let theme = theme_data(sources, &mut blocks);
    theme_json::merge(&mut result, &theme);
    theme_json::merge(&mut result, &user_data(sources.user, &blocks));
    (result, blocks)
}

#[cfg(test)]
mod tests;
