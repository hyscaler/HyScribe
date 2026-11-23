// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Theme_JSON`'s data side: the constructor (migrate, sanitize, opt in,
//! key presets by origin, generate spacing sizes) and `merge()`.

use hyscribe_phpser::{Key, Value};

use crate::blocks::Blocks;
use crate::php;
use crate::sanitize::{self, is_origin};
use crate::{schema, spacing};

/// The sources of theme.json data, lowest priority first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Default,
    Blocks,
    Theme,
    Custom,
}

impl Origin {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Origin::Default => "default",
            Origin::Blocks => "blocks",
            Origin::Theme => "theme",
            Origin::Custom => "custom",
        }
    }
}

/// `VALID_ORIGINS`.
pub(crate) const ORIGINS: [&str; 4] = ["default", "blocks", "theme", "custom"];

/// How a preset's CSS value is found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PresetValue {
    /// The preset's own key.
    Key(&'static str),
    /// `wp_get_typography_font_size_value()`.
    FontSize,
}

/// One entry of `PRESETS_METADATA`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Preset {
    pub(crate) path: [&'static str; 2],
    pub(crate) prevent_override: Option<[&'static str; 2]>,
    pub(crate) use_default_names: bool,
    pub(crate) value: PresetValue,
    pub(crate) css_vars: &'static str,
    pub(crate) classes: &'static [(&'static str, &'static str)],
}

/// `PRESETS_METADATA`.
pub(crate) const PRESETS: [Preset; 10] = [
    Preset {
        path: ["dimensions", "aspectRatios"],
        prevent_override: Some(["dimensions", "defaultAspectRatios"]),
        use_default_names: false,
        value: PresetValue::Key("ratio"),
        css_vars: "--wp--preset--aspect-ratio--$slug",
        classes: &[],
    },
    Preset {
        path: ["color", "palette"],
        prevent_override: Some(["color", "defaultPalette"]),
        use_default_names: false,
        value: PresetValue::Key("color"),
        css_vars: "--wp--preset--color--$slug",
        classes: &[
            (".has-$slug-color", "color"),
            (".has-$slug-background-color", "background-color"),
            (".has-$slug-border-color", "border-color"),
        ],
    },
    Preset {
        path: ["color", "gradients"],
        prevent_override: Some(["color", "defaultGradients"]),
        use_default_names: false,
        value: PresetValue::Key("gradient"),
        css_vars: "--wp--preset--gradient--$slug",
        classes: &[(".has-$slug-gradient-background", "background")],
    },
    Preset {
        path: ["color", "duotone"],
        prevent_override: Some(["color", "defaultDuotone"]),
        use_default_names: false,
        // Unused: without CSS variables, duotone presets go to their block
        // support.
        value: PresetValue::Key("colors"),
        css_vars: "",
        classes: &[],
    },
    Preset {
        path: ["typography", "fontSizes"],
        prevent_override: Some(["typography", "defaultFontSizes"]),
        use_default_names: true,
        value: PresetValue::FontSize,
        css_vars: "--wp--preset--font-size--$slug",
        classes: &[(".has-$slug-font-size", "font-size")],
    },
    Preset {
        path: ["typography", "fontFamilies"],
        prevent_override: None,
        use_default_names: false,
        value: PresetValue::Key("fontFamily"),
        css_vars: "--wp--preset--font-family--$slug",
        classes: &[(".has-$slug-font-family", "font-family")],
    },
    Preset {
        path: ["spacing", "spacingSizes"],
        prevent_override: Some(["spacing", "defaultSpacingSizes"]),
        use_default_names: true,
        value: PresetValue::Key("size"),
        css_vars: "--wp--preset--spacing--$slug",
        classes: &[],
    },
    Preset {
        path: ["shadow", "presets"],
        prevent_override: Some(["shadow", "defaultPresets"]),
        use_default_names: false,
        value: PresetValue::Key("shadow"),
        css_vars: "--wp--preset--shadow--$slug",
        classes: &[],
    },
    Preset {
        path: ["border", "radiusSizes"],
        prevent_override: None,
        use_default_names: false,
        value: PresetValue::Key("size"),
        css_vars: "--wp--preset--border-radius--$slug",
        classes: &[],
    },
    Preset {
        path: ["dimensions", "dimensionSizes"],
        prevent_override: None,
        use_default_names: false,
        value: PresetValue::Key("size"),
        css_vars: "--wp--preset--dimension--$slug",
        classes: &[],
    },
];

/// Settings `appearanceTools` turns on.
const APPEARANCE_TOOLS: [[&str; 2]; 22] = [
    ["background", "backgroundImage"],
    ["background", "backgroundSize"],
    ["background", "gradient"],
    ["border", "color"],
    ["border", "radius"],
    ["border", "style"],
    ["border", "width"],
    ["color", "link"],
    ["color", "heading"],
    ["color", "button"],
    ["color", "caption"],
    ["dimensions", "aspectRatio"],
    ["dimensions", "height"],
    ["dimensions", "minHeight"],
    ["dimensions", "minWidth"],
    ["dimensions", "width"],
    ["position", "sticky"],
    ["spacing", "blockGap"],
    ["spacing", "margin"],
    ["spacing", "padding"],
    ["typography", "lineHeight"],
    ["typography", "textColumns"],
];

/// A path of owned keys, borrowed for lookups.
pub(crate) fn refs(path: &[String]) -> Vec<&str> {
    path.iter().map(String::as_str).collect()
}

fn joined(base: &[String], tail: &[&str]) -> Vec<String> {
    base.iter()
        .cloned()
        .chain(tail.iter().map(|s| (*s).to_owned()))
        .collect()
}

/// `get_setting_nodes()` paths: `settings`, then `settings.blocks.<name>`.
pub(crate) fn setting_paths(data: &Value) -> Vec<Vec<String>> {
    let Some(settings) = php::get(data, &["settings"]).filter(|s| php::isset(Some(s))) else {
        return Vec::new();
    };
    let mut paths = vec![vec!["settings".to_owned()]];
    for (name, _) in php::entries(php::get(settings, &["blocks"]).unwrap_or(&Value::Null)) {
        paths.push(vec![
            "settings".to_owned(),
            "blocks".to_owned(),
            php::key_str(name),
        ]);
    }
    paths
}

/// `new WP_Theme_JSON( $theme_json, $origin )`, as its raw data.
pub(crate) fn new(theme_json: Value, origin: Origin, blocks: &Blocks) -> Value {
    let theme_json = schema::migrate(theme_json, origin);
    let theme_json = unwrap_shared_variations(theme_json, blocks);
    let mut data = sanitize::sanitize(theme_json, blocks);
    opt_in(&mut data);

    for node in setting_paths(&data) {
        for preset in &PRESETS {
            let path = joined(&node, &preset.path);
            let path = refs(&path);
            let Some(value) = php::get(&data, &path).filter(|v| php::isset(Some(v))) else {
                continue;
            };
            let first_isset = match value {
                Value::Array(_) => php::isset(value.get(&Key::Int(0))),
                Value::String(s) => !s.is_empty(),
                _ => false,
            };
            if first_isset || php::empty(Some(value)) {
                let keyed = Value::Array(vec![(php::key(origin.name()), value.clone())]);
                php::set(&mut data, &path, keyed);
            }
        }
    }

    let scale_path = ["settings", "spacing", "spacingScale"];
    if let Some(scale) = php::get(&data, &scale_path).filter(|v| php::isset(Some(v)))
        && !php::entries(scale).iter().any(|(k, _)| is_origin(k))
    {
        let keyed = Value::Array(vec![(php::key(origin.name()), scale.clone())]);
        php::set(&mut data, &scale_path, keyed);
    }
    let scale_path = ["settings", "spacing", "spacingScale", origin.name()];
    if let Some(scale) = php::get(&data, &scale_path).filter(|v| php::isset(Some(v))) {
        let sizes_path = ["settings", "spacing", "spacingSizes", origin.name()];
        let sizes = php::get(&data, &sizes_path)
            .cloned()
            .unwrap_or_else(php::array);
        let merged = spacing::merge_sizes(spacing::compute_sizes(scale), &sizes);
        php::set(&mut data, &sizes_path, merged);
    }
    data
}

/// `unwrap_shared_block_style_variations()`: `styles.variations.<name>`
/// moves under every block that registers the variation.
fn unwrap_shared_variations(mut theme_json: Value, blocks: &Blocks) -> Value {
    // Without shared variations nothing moves, and sanitizing drops the key.
    let shared = php::get(&theme_json, &["styles", "variations"])
        .cloned()
        .unwrap_or(Value::Null);
    for meta in blocks.iter() {
        for (variation, _) in &meta.style_variations {
            let path = [
                "styles",
                "blocks",
                meta.name.as_str(),
                "variations",
                variation.as_str(),
            ];
            let mut merged = php::get(&shared, &[variation])
                .cloned()
                .unwrap_or_else(php::array);
            if let Some(block_level) = php::get(&theme_json, &path) {
                php::replace_recursive(&mut merged, block_level);
            }
            if !php::empty(Some(&merged)) {
                php::set(&mut theme_json, &path, merged);
            }
        }
    }
    php::unset(&mut theme_json, &["styles", "variations"]);
    theme_json
}

/// `maybe_opt_in_into_settings()`.
fn opt_in(data: &mut Value) {
    let enabled = |node: &Value| php::get(node, &["appearanceTools"]) == Some(&Value::Bool(true));
    let apply = |node: &mut Value| {
        for path in APPEARANCE_TOOLS {
            if php::get(node, &path).is_none() {
                php::set(node, &path, Value::Bool(true));
            }
        }
        php::unset(node, &["appearanceTools"]);
    };
    if let Some(settings) = php::get_mut(data, &["settings"]).filter(|s| enabled(s)) {
        apply(settings);
    }
    if let Some(Value::Array(blocks)) = php::get_mut(data, &["settings", "blocks"]) {
        for (_, block) in blocks.iter_mut().filter(|(_, b)| enabled(b)) {
            apply(block);
        }
    }
}

/// `WP_Theme_JSON::merge()`: `incoming` (raw data) over `data`.
pub(crate) fn merge(data: &mut Value, incoming: &Value) {
    php::replace_recursive(data, incoming);
    let mut incoming = incoming.clone();

    let mut flattened = php::array();
    for origin in ORIGINS {
        let scale_path = ["settings", "spacing", "spacingScale", origin];
        php::replace(&mut flattened, php::get(data, &scale_path));
        let Some(scale) = php::get(&incoming, &scale_path).filter(|v| php::isset(Some(v))) else {
            continue;
        };
        php::replace(&mut flattened, Some(scale));
        let sizes_path = ["settings", "spacing", "spacingSizes", origin];
        let sizes = php::get(&incoming, &sizes_path)
            .cloned()
            .unwrap_or_else(php::array);
        let merged = spacing::merge_sizes(spacing::compute_sizes(&flattened), &sizes);
        php::set(&mut incoming, &sizes_path, merged);
    }

    let slugs_global = default_slugs(data, &["settings".to_owned()]);
    for node in setting_paths(&incoming) {
        let units = joined(&node, &["spacing", "units"]);
        if let Some(content) = php::get(&incoming, &refs(&units)).filter(|v| php::isset(Some(v))) {
            php::set(data, &refs(&units), content.clone());
        }
        for preset in &PRESETS {
            let prevent = preset.prevent_override.is_some_and(|path| {
                let global = php::get(data, &["settings", path[0], path[1]]);
                let node_level = php::get(data, &refs(&joined(&node, &path)));
                !php::empty(node_level.or(global))
            });
            for origin in ORIGINS {
                let base = joined(&node, &preset.path);
                let path = joined(&base, &[origin]);
                let Some(content) =
                    php::get(&incoming, &refs(&path)).filter(|v| php::isset(Some(v)))
                else {
                    continue;
                };
                let mut content = content.clone();
                if origin == "theme" && preset.use_default_names {
                    name_from_defaults(data, &base, &mut content);
                }
                if origin == "theme" && prevent {
                    let node_slugs = default_slugs(data, &node);
                    let mut slugs: Vec<Value> = Vec::new();
                    for tree in [&slugs_global, &node_slugs] {
                        let list = php::get(tree, &preset.path).unwrap_or(&Value::Null);
                        slugs.extend(php::entries(list).iter().map(|(_, s)| s.clone()));
                    }
                    content = filter_slugs(content, &slugs);
                }
                php::set(data, &refs(&path), content);
            }
        }
    }

    let mut style_paths = block_node_paths(data);
    style_paths.push(vec!["styles".to_owned()]);
    for path in style_paths {
        let image = joined(&path, &["background", "backgroundImage"]);
        if let Some(content) = php::get(&incoming, &refs(&image)).filter(|v| php::isset(Some(v))) {
            php::set(data, &refs(&image), content.clone());
        }
    }
}

/// Names for theme presets without one, from the default preset of the
/// same slug.
fn name_from_defaults(data: &Value, base: &[String], content: &mut Value) {
    let defaults = php::get(data, &refs(&joined(base, &["default"]))).cloned();
    let Value::Array(items) = content else {
        return;
    };
    for (_, item) in items.iter_mut() {
        if php::isset(php::get(item, &["name"])) {
            continue;
        }
        let slug = php::get(item, &["slug"]).unwrap_or(&Value::Null);
        let name = defaults
            .iter()
            .filter(|d| !php::empty(Some(d)))
            .flat_map(php::entries)
            .find(|(_, d)| php::get(d, &["slug"]).unwrap_or(&Value::Null) == slug)
            .and_then(|(_, d)| php::get(d, &["name"]))
            .filter(|n| php::isset(Some(n)))
            .cloned();
        if let Some(name) = name {
            php::set(item, &["name"], name);
        }
    }
}

/// `get_default_slugs( $data, $node_path )`.
fn default_slugs(data: &Value, node: &[String]) -> Value {
    let mut slugs = php::array();
    for preset in &PRESETS {
        let path = joined(&joined(node, &preset.path), &["default"]);
        let Some(items) = php::get(data, &refs(&path)).filter(|v| php::isset(Some(v))) else {
            continue;
        };
        let list = php::entries(items)
            .iter()
            .filter_map(|(_, item)| php::get(item, &["slug"]).filter(|s| php::isset(Some(s))))
            .enumerate()
            .map(|(i, slug)| (Key::Int(i as i64), slug.clone()))
            .collect();
        php::set(&mut slugs, &preset.path, Value::Array(list));
    }
    slugs
}

/// `filter_slugs( $node, $slugs )`: presets whose slug is not in `slugs`.
fn filter_slugs(node: Value, slugs: &[Value]) -> Value {
    if slugs.is_empty() {
        return node;
    }
    let kept = php::entries(&node)
        .iter()
        .filter_map(|(_, item)| {
            let slug = php::get(item, &["slug"]).filter(|s| php::isset(Some(s)))?;
            (!slugs.contains(slug)).then(|| item.clone())
        })
        .enumerate()
        .map(|(i, item)| (Key::Int(i as i64), item))
        .collect();
    Value::Array(kept)
}

/// The paths `get_block_nodes()` lists with `include_node_paths_only`:
/// each block, then each of its styled elements.
fn block_node_paths(data: &Value) -> Vec<Vec<String>> {
    let breakpoints = crate::viewport::media_queries(php::get(data, &["settings", "viewport"]));
    let mut paths = Vec::new();
    for (name, node) in php::entries(php::get(data, &["styles", "blocks"]).unwrap_or(&Value::Null))
    {
        let name = php::key_str(name);
        paths.push(vec!["styles".to_owned(), "blocks".to_owned(), name.clone()]);
        let elements = crate::stylesheet::element_names(node, &breakpoints);
        for element in elements {
            if php::isset(php::get(node, &["elements", &element])) {
                paths.push(vec![
                    "styles".to_owned(),
                    "blocks".to_owned(),
                    name.clone(),
                    "elements".to_owned(),
                    element,
                ]);
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests;
