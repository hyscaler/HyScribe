// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Theme_JSON`'s CSS side: `get_stylesheet()` (variables, root layout
//! rules, styles, preset classes, custom CSS) and `get_styles_for_block()`.
//!
//! Not ported, as nothing in a default install reaches them: the
//! `root_selector` option and the `disable-layout-styles` theme support.
//! The `scope` option scopes style nodes only, as no caller asks it for
//! variables or presets.

use hyscribe_phpser::{Value, float_to_string, string_to_float};

use crate::blocks::{Blocks, ELEMENTS, element_selector};
use crate::css::{Declaration, is_safe, nest_custom, ruleset};
use crate::kebab::kebab_case;
use crate::php::{self, as_str, php_string, string};
use crate::sanitize::{BLOCK_PSEUDO, BLOCK_STATES, ELEMENT_PSEUDO, lookup};
use crate::selector;
use crate::theme_json::{PRESETS, Preset, PresetValue, refs};
use crate::typography::font_size_value;
use crate::viewport;

/// `PROPERTIES_METADATA`: CSS property and where its value is.
pub(crate) const PROPERTIES: &[(&str, &[&str])] = &[
    ("aspect-ratio", &["dimensions", "aspectRatio"]),
    ("background", &["color", "gradient"]),
    ("background-color", &["color", "background"]),
    ("background-image", &["background", "backgroundImage"]),
    ("background-position", &["background", "backgroundPosition"]),
    ("background-repeat", &["background", "backgroundRepeat"]),
    ("background-size", &["background", "backgroundSize"]),
    (
        "background-attachment",
        &["background", "backgroundAttachment"],
    ),
    ("border-radius", &["border", "radius"]),
    ("border-top-left-radius", &["border", "radius", "topLeft"]),
    ("border-top-right-radius", &["border", "radius", "topRight"]),
    (
        "border-bottom-left-radius",
        &["border", "radius", "bottomLeft"],
    ),
    (
        "border-bottom-right-radius",
        &["border", "radius", "bottomRight"],
    ),
    ("border-color", &["border", "color"]),
    ("border-width", &["border", "width"]),
    ("border-style", &["border", "style"]),
    ("border-top-color", &["border", "top", "color"]),
    ("border-top-width", &["border", "top", "width"]),
    ("border-top-style", &["border", "top", "style"]),
    ("border-right-color", &["border", "right", "color"]),
    ("border-right-width", &["border", "right", "width"]),
    ("border-right-style", &["border", "right", "style"]),
    ("border-bottom-color", &["border", "bottom", "color"]),
    ("border-bottom-width", &["border", "bottom", "width"]),
    ("border-bottom-style", &["border", "bottom", "style"]),
    ("border-left-color", &["border", "left", "color"]),
    ("border-left-width", &["border", "left", "width"]),
    ("border-left-style", &["border", "left", "style"]),
    ("color", &["color", "text"]),
    ("text-align", &["typography", "textAlign"]),
    ("column-count", &["typography", "textColumns"]),
    ("font-family", &["typography", "fontFamily"]),
    ("font-size", &["typography", "fontSize"]),
    ("font-style", &["typography", "fontStyle"]),
    ("font-weight", &["typography", "fontWeight"]),
    ("letter-spacing", &["typography", "letterSpacing"]),
    ("line-height", &["typography", "lineHeight"]),
    ("margin", &["spacing", "margin"]),
    ("margin-top", &["spacing", "margin", "top"]),
    ("margin-right", &["spacing", "margin", "right"]),
    ("margin-bottom", &["spacing", "margin", "bottom"]),
    ("margin-left", &["spacing", "margin", "left"]),
    ("min-height", &["dimensions", "minHeight"]),
    ("min-width", &["dimensions", "minWidth"]),
    ("outline-color", &["outline", "color"]),
    ("outline-offset", &["outline", "offset"]),
    ("outline-style", &["outline", "style"]),
    ("outline-width", &["outline", "width"]),
    ("padding", &["spacing", "padding"]),
    ("padding-top", &["spacing", "padding", "top"]),
    ("padding-right", &["spacing", "padding", "right"]),
    ("padding-bottom", &["spacing", "padding", "bottom"]),
    ("padding-left", &["spacing", "padding", "left"]),
    ("--wp--style--root--padding", &["spacing", "padding"]),
    (
        "--wp--style--root--padding-top",
        &["spacing", "padding", "top"],
    ),
    (
        "--wp--style--root--padding-right",
        &["spacing", "padding", "right"],
    ),
    (
        "--wp--style--root--padding-bottom",
        &["spacing", "padding", "bottom"],
    ),
    (
        "--wp--style--root--padding-left",
        &["spacing", "padding", "left"],
    ),
    ("text-decoration", &["typography", "textDecoration"]),
    ("text-shadow", &["typography", "textShadow"]),
    ("text-transform", &["typography", "textTransform"]),
    ("text-indent", &["typography", "textIndent"]),
    ("filter", &["filter", "duotone"]),
    ("box-shadow", &["shadow"]),
    ("height", &["dimensions", "height"]),
    ("width", &["dimensions", "width"]),
    ("writing-mode", &["typography", "writingMode"]),
];

const ROOT_STYLE_PREFIX: &str = "--wp--style--root--";

/// A layout rule: a selector suffix and its declarations.
type Rule = (
    &'static str,
    &'static [(&'static str, Option<&'static str>)],
);
type Rules = &'static [Rule];

/// One entry of `wp_get_layout_definitions()`. A `None` rule value stands
/// for the block gap.
struct Layout {
    key: &'static str,
    class: &'static str,
    display: Option<&'static str>,
    base: Rules,
    spacing: Rules,
}

const ALIGN_RULES: [Rule; 3] = [
    (
        " > .alignleft",
        &[
            ("float", Some("left")),
            ("margin-inline-start", Some("0")),
            ("margin-inline-end", Some("2em")),
        ],
    ),
    (
        " > .alignright",
        &[
            ("float", Some("right")),
            ("margin-inline-start", Some("2em")),
            ("margin-inline-end", Some("0")),
        ],
    ),
    (
        " > .aligncenter",
        &[
            ("margin-left", Some("auto !important")),
            ("margin-right", Some("auto !important")),
        ],
    ),
];

const FLOW_SPACING: Rules = &[
    (" > :first-child", &[("margin-block-start", Some("0"))]),
    (" > :last-child", &[("margin-block-end", Some("0"))]),
    (
        " > *",
        &[
            ("margin-block-start", None),
            ("margin-block-end", Some("0")),
        ],
    ),
];

const GAP_SPACING: Rules = &[("", &[("gap", None)])];

const LAYOUTS: [Layout; 4] = [
    Layout {
        key: "default",
        class: "is-layout-flow",
        display: None,
        base: &ALIGN_RULES,
        spacing: FLOW_SPACING,
    },
    Layout {
        key: "constrained",
        class: "is-layout-constrained",
        display: None,
        base: &[
            ALIGN_RULES[0],
            ALIGN_RULES[1],
            ALIGN_RULES[2],
            (
                " > :where(:not(.alignleft):not(.alignright):not(.alignfull))",
                &[
                    ("max-width", Some("var(--wp--style--global--content-size)")),
                    ("margin-left", Some("auto !important")),
                    ("margin-right", Some("auto !important")),
                ],
            ),
            (
                " > .alignwide",
                &[("max-width", Some("var(--wp--style--global--wide-size)"))],
            ),
        ],
        spacing: FLOW_SPACING,
    },
    Layout {
        key: "flex",
        class: "is-layout-flex",
        display: Some("flex"),
        base: &[
            (
                "",
                &[("flex-wrap", Some("wrap")), ("align-items", Some("center"))],
            ),
            (" > :is(*, div)", &[("margin", Some("0"))]),
        ],
        spacing: GAP_SPACING,
    },
    Layout {
        key: "grid",
        class: "is-layout-grid",
        display: Some("grid"),
        base: &[(" > :is(*, div)", &[("margin", Some("0"))])],
        spacing: GAP_SPACING,
    },
];

/// The metadata of one style node (`get_style_nodes()`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StyleNode {
    pub(crate) name: Option<String>,
    pub(crate) path: Vec<String>,
    pub(crate) selector: Option<String>,
    pub(crate) selectors: Option<Value>,
    pub(crate) duotone: Option<String>,
    pub(crate) media_query: Option<String>,
    /// The selector before scoping (`css`); only scoped nodes have it.
    pub(crate) css: Option<String>,
    pub(crate) variations: Vec<Variation>,
}

/// A block style variation of a block node.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Variation {
    name: String,
    pub(crate) path: Vec<String>,
    selector: String,
}

impl StyleNode {
    fn at(path: &[&str], selector: &str) -> StyleNode {
        StyleNode {
            path: path.iter().map(|s| (*s).to_owned()).collect(),
            selector: Some(selector.to_owned()),
            ..StyleNode::default()
        }
    }
}

/// A merged theme.json and what its CSS needs to know.
pub(crate) struct Tree<'a> {
    pub(crate) data: &'a Value,
    pub(crate) blocks: &'a Blocks,
    /// `wp_get_global_settings()`, which fluid font sizes fall back to.
    pub(crate) global: &'a Value,
}

/// Declarations grouped by selector, in first-seen order.
type Grouped = Vec<(String, Vec<Declaration>)>;

fn group(groups: &mut Grouped, selector: &str, declarations: Vec<Declaration>) {
    match groups.iter_mut().find(|(s, _)| s == selector) {
        Some((_, existing)) => existing.extend(declarations),
        None => groups.push((selector.to_owned(), declarations)),
    }
}

impl Tree<'_> {
    fn settings(&self) -> &Value {
        php::get(self.data, &["settings"]).unwrap_or(&Value::Null)
    }

    fn use_root_padding(&self) -> bool {
        php::get(self.data, &["settings", "useRootPaddingAwareAlignments"])
            == Some(&Value::Bool(true))
    }

    fn media_queries(&self) -> Vec<(String, String)> {
        viewport::media_queries(php::get(self.data, &["settings", "viewport"]))
    }

    /// `get_style_nodes()`; block nodes only with `blocks`.
    pub(crate) fn style_nodes(&self, blocks: bool, variations: bool) -> Vec<StyleNode> {
        let Some(styles) = php::get(self.data, &["styles"]).filter(|s| php::isset(Some(s))) else {
            return Vec::new();
        };
        let mut nodes = vec![StyleNode::at(&["styles"], "body")];
        if let Some(elements) = php::get(styles, &["elements"]).filter(|e| php::isset(Some(e))) {
            for (element, selector) in ELEMENTS {
                let Some(node) = php::get(elements, &[element]).filter(|e| php::isset(Some(e)))
                else {
                    continue;
                };
                nodes.push(StyleNode::at(&["styles", "elements", element], selector));
                for pseudo in lookup(&ELEMENT_PSEUDO, element) {
                    if php::isset(php::get(node, &[pseudo])) {
                        let selector = selector::append(selector, pseudo);
                        nodes.push(StyleNode::at(&["styles", "elements", element], &selector));
                    }
                }
            }
        }
        if blocks {
            nodes.extend(self.block_nodes(variations));
        }
        nodes
    }

    /// `get_block_nodes()`; style variations only with `variations`.
    pub(crate) fn block_nodes(&self, variations: bool) -> Vec<StyleNode> {
        let breakpoints = self.media_queries();
        let mut nodes = Vec::new();
        for (name, node) in
            php::entries(php::get(self.data, &["styles", "blocks"]).unwrap_or(&Value::Null))
        {
            let name = php::key_str(name);
            let meta = self.blocks.get(&name);
            let base = |tail: &[&str]| -> Vec<String> {
                ["styles", "blocks", &name]
                    .iter()
                    .chain(tail)
                    .map(|s| (*s).to_owned())
                    .collect()
            };
            let variations = php::entries(
                php::get(node, &["variations"])
                    .filter(|_| variations)
                    .unwrap_or(&Value::Null),
            )
            .iter()
            .map(|(variation, _)| {
                let name = php::key_str(variation);
                let selector = meta
                    .and_then(|m| m.style_variations.iter().find(|(n, _)| *n == name))
                    .map(|(_, selector)| selector.clone())
                    .unwrap_or_default();
                Variation {
                    path: base(&["variations", &name]),
                    name,
                    selector,
                }
            })
            .collect();
            let block = StyleNode {
                name: Some(name.clone()),
                path: base(&[]),
                selector: meta.map(|m| m.selector.clone()),
                selectors: meta.map(|m| m.selectors.clone()),
                duotone: meta.and_then(|m| m.duotone.clone()),
                media_query: None,
                variations,
                css: None,
            };
            let selector = block.selector.clone().unwrap_or_default();
            nodes.push(block.clone());
            for (state, query) in &breakpoints {
                if php::isset(php::get(node, &[state])) {
                    nodes.push(StyleNode {
                        path: base(&[state]),
                        media_query: Some(query.clone()),
                        duotone: None,
                        ..block.clone()
                    });
                }
            }
            for pseudo in lookup(&BLOCK_PSEUDO, &name) {
                let has = php::isset(php::get(node, &[pseudo]));
                let responsive: Vec<&(String, String)> = breakpoints
                    .iter()
                    .filter(|(b, _)| php::isset(php::get(node, &[b, pseudo])))
                    .collect();
                if !has && responsive.is_empty() {
                    continue;
                }
                let pseudo_selectors = block.selectors.as_ref().map(|s| append_all(s, pseudo));
                let pseudo_node = StyleNode {
                    path: base(&[pseudo]),
                    selector: Some(selector::append(&selector, pseudo)),
                    selectors: pseudo_selectors,
                    ..block.clone()
                };
                if has {
                    nodes.push(pseudo_node.clone());
                }
                for (state, query) in responsive {
                    nodes.push(StyleNode {
                        path: base(&[state, pseudo]),
                        media_query: Some(query.clone()),
                        duotone: None,
                        ..pseudo_node.clone()
                    });
                }
            }
            for state in lookup(&BLOCK_STATES, &name) {
                let state_selector = meta
                    .and_then(|m| m.states.as_ref())
                    .and_then(|s| php::get(s, &[state]))
                    .filter(|s| php::isset(Some(s)));
                let (Some(state_node), Some(state_selector)) = (
                    php::get(node, &[state]).filter(|s| php::isset(Some(s))),
                    state_selector,
                ) else {
                    continue;
                };
                let state_selector = php_string(state_selector);
                nodes.push(StyleNode {
                    path: base(&[state]),
                    selector: Some(state_selector.clone()),
                    ..block.clone()
                });
                for pseudo in lookup(&BLOCK_PSEUDO, &name) {
                    if php::isset(php::get(state_node, &[pseudo])) {
                        nodes.push(StyleNode {
                            path: base(&[state, pseudo]),
                            selector: Some(selector::append(&state_selector, pseudo)),
                            ..block.clone()
                        });
                    }
                }
            }
            nodes.extend(self.element_nodes(&name, node, &breakpoints));
        }
        nodes
    }

    /// The element nodes of one block.
    fn element_nodes(
        &self,
        name: &str,
        node: &Value,
        breakpoints: &[(String, String)],
    ) -> Vec<StyleNode> {
        let names = element_names(node, breakpoints);
        let elements = self
            .blocks
            .get(name)
            .map(|m| m.elements.as_slice())
            .unwrap_or_default();
        let mut nodes = Vec::new();
        // Sanitizing keeps only elements every registered block has.
        let known = names
            .iter()
            .filter_map(|n| elements.iter().find(|(e, _)| e == n));
        for (element, selector) in known {
            let element = (*element).to_owned();
            let path: Vec<String> = ["styles", "blocks", name, "elements", &element]
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            let responsive = |at: &[&str], selector: &str| -> Vec<StyleNode> {
                breakpoints
                    .iter()
                    .filter(|(b, _)| {
                        let mut full = vec![b.as_str(), "elements", element.as_str()];
                        full.extend_from_slice(at);
                        php::isset(php::get(node, &full))
                    })
                    .map(|(b, query)| StyleNode {
                        path: ["styles", "blocks", name, b, "elements", &element]
                            .iter()
                            .map(|s| (*s).to_owned())
                            .collect(),
                        selector: Some(selector.to_owned()),
                        media_query: Some(query.clone()),
                        ..StyleNode::default()
                    })
                    .collect()
            };
            if php::isset(php::get(node, &["elements", &element])) {
                nodes.push(StyleNode {
                    path: path.clone(),
                    selector: Some(selector.clone()),
                    ..StyleNode::default()
                });
            }
            nodes.extend(responsive(&[], selector));
            for pseudo in lookup(&ELEMENT_PSEUDO, &element) {
                let pseudo_selector = selector::append(selector, pseudo);
                if php::isset(php::get(node, &["elements", &element, pseudo])) {
                    nodes.push(StyleNode {
                        path: path.clone(),
                        selector: Some(pseudo_selector.clone()),
                        ..StyleNode::default()
                    });
                }
                nodes.extend(responsive(&[pseudo], &pseudo_selector));
            }
        }
        nodes
    }
}

/// One `get_setting_nodes()` entry.
struct SettingNode {
    path: Vec<String>,
    selector: String,
    selectors: Value,
}

/// The `get_stylesheet()` options HyScribe uses.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Options<'s> {
    /// Block nodes are kept (`wp_filter_out_block_nodes` is off).
    pub(crate) blocks: bool,
    /// A scoped stylesheet is a style variation's, so it also sets
    /// `include_block_style_variations` and `skip_root_layout_styles`.
    pub(crate) scope: Option<&'s str>,
}

/// Which parts of `get_stylesheet()` to build.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Types {
    pub(crate) variables: bool,
    pub(crate) styles: bool,
    pub(crate) presets: bool,
    pub(crate) custom_css: bool,
}

impl Tree<'_> {
    /// `get_property_value( $styles, $path, $theme_json )`; `{ "ref": "a.b" }`
    /// values are looked up in the tree only with `resolve`, as PHP only
    /// does when it is given the tree.
    fn property_value(&self, styles: &Value, path: &[&str], resolve: bool) -> Value {
        let Some(value) = php::get(styles, path).filter(|v| php::isset(Some(v))) else {
            return string("");
        };
        let reference = php::get(value, &["ref"]).filter(|r| resolve && php::isset(Some(r)));
        if let (Value::Array(_), Some(reference)) = (value, reference) {
            let reference = php_string(reference);
            let target = php::get(self.data, &reference.split('.').collect::<Vec<_>>());
            let url = target.and_then(|t| php::get(t, &["url"]));
            let usable =
                matches!(target, Some(Value::String(_))) || matches!(url, Some(Value::String(_)));
            if let Some(target) = target.filter(|t| usable && !php::empty(Some(t))) {
                return target.clone();
            }
        }
        value.clone()
    }

    /// `compute_style_properties( $styles, $settings, null, $theme_json,
    /// $selector, $use_root_padding )`.
    pub(crate) fn compute(
        &self,
        styles: &Value,
        selector: Option<&str>,
        use_root_padding: bool,
    ) -> Vec<Declaration> {
        if php::empty(Some(styles)) {
            return Vec::new();
        }
        let mut declarations = Vec::new();
        let mut duplicates = Vec::new();
        for &(property, path) in PROPERTIES {
            let root_style = property.strip_prefix(ROOT_STYLE_PREFIX);
            if root_style.is_some() && (selector != Some("body") || !use_root_padding) {
                continue;
            }
            let mut value = self.property_value(styles, path, true);
            if property == "--wp--style--root--padding" && matches!(value, Value::String(_)) {
                continue;
            }
            duplicates.extend(root_style);
            if property == "background-image" {
                let gradient = php::get(styles, &["background", "gradient"]);
                value = background_image(
                    Some(&value).filter(|v| !php::empty(Some(v))),
                    gradient.filter(|g| !php::empty(Some(g))),
                )
                .unwrap_or(value);
            }
            let image_id = php::get(styles, &["background", "backgroundImage", "id"]);
            if php::empty(Some(&value)) && selector != Some("body") && !php::empty(image_id) {
                if property == "background-size" {
                    value = string("cover");
                }
                if property == "background-position" {
                    let size = php::get(styles, &["background", "backgroundSize"]).and_then(as_str);
                    value = if size == Some("contain") {
                        string("50% 50%")
                    } else {
                        Value::Null
                    };
                }
            }
            if (php::empty(Some(&value)) && !value.is_numeric()) || matches!(value, Value::Array(_))
            {
                continue;
            }
            if property == "font-size" {
                let preset = Value::Array(vec![(php::key("size"), value)]);
                value =
                    font_size_value(&preset, self.settings(), self.global).unwrap_or(Value::Null);
            }
            if property == "aspect-ratio" {
                declarations.push(Declaration::text("min-height", "unset"));
            }
            declarations.push(Declaration::new(property, value));
        }
        for duplicate in duplicates {
            if let Some(at) = declarations.iter().position(|d| d.name == duplicate) {
                declarations.remove(at);
            }
        }
        declarations
    }

    /// `get_feature_declarations_for_node()`: declarations for features
    /// with their own selectors, removed from `node`.
    fn feature_declarations(&self, selectors: Option<&Value>, node: &mut Value) -> Grouped {
        let mut groups = Grouped::new();
        for (feature, feature_selectors) in php::entries(selectors.unwrap_or(&Value::Null)) {
            let feature = php::key_str(feature);
            if feature == "root" || feature == "css" || php::empty(php::get(node, &[&feature])) {
                continue;
            }
            for (sub, sub_selector) in php::entries(feature_selectors) {
                let sub = php::key_str(sub);
                let Some(value) =
                    php::get(node, &[&feature, &sub]).filter(|v| !php::empty(Some(v)))
                else {
                    continue;
                };
                let mut sub_node = php::array();
                php::set(&mut sub_node, &[&feature, &sub], value.clone());
                group(
                    &mut groups,
                    &php_string(sub_selector),
                    self.compute(&sub_node, None, false),
                );
                php::unset(node, &[&feature, &sub]);
            }
            let root = match feature_selectors {
                Value::String(_) => Some(feature_selectors),
                _ => php::get(feature_selectors, &["root"]).filter(|r| !php::empty(Some(r))),
            };
            if let Some(root) = root {
                let mut feature_node = php::array();
                php::set(
                    &mut feature_node,
                    &[&feature],
                    php::get(node, &[&feature]).cloned().unwrap_or(Value::Null),
                );
                group(
                    &mut groups,
                    &php_string(root),
                    self.compute(&feature_node, None, false),
                );
                php::unset(node, &[&feature]);
            }
        }
        groups
    }

    /// `update_paragraph_text_indent_selector()`.
    fn paragraph_indent(&self, mut groups: Grouped, block: Option<&str>) -> Grouped {
        if block != Some("core/paragraph") {
            return groups;
        }
        let set = |path: &[&str]| php::get(self.settings(), path).filter(|v| php::isset(Some(v)));
        let indent = set(&["blocks", "core/paragraph", "typography", "textIndent"])
            .or_else(|| set(&["typography", "textIndent"]));
        let old = ".wp-block-paragraph + .wp-block-paragraph";
        if indent.and_then(as_str) == Some("all")
            && let Some(at) = groups.iter().position(|(s, _)| s == old)
        {
            let (_, declarations) = groups.remove(at);
            match groups.iter_mut().find(|(s, _)| s == ".wp-block-paragraph") {
                Some(existing) => existing.1 = declarations,
                None => groups.push((".wp-block-paragraph".to_owned(), declarations)),
            }
        }
        groups
    }

    /// `update_button_width_declarations()`: percentage widths of buttons
    /// leave room for the block gap.
    fn button_width(&self, mut groups: Grouped) -> Grouped {
        let settings = self.settings();
        let mut sizes: Vec<(String, &Value)> = Vec::new();
        for path in [
            &["blocks", "core/button", "dimensions", "dimensionSizes"][..],
            &["dimensions", "dimensionSizes"],
        ] {
            for (origin, list) in php::entries(php::get(settings, path).unwrap_or(&Value::Null)) {
                let origin = php::key_str(origin);
                if !sizes.iter().any(|(o, _)| *o == origin) {
                    sizes.push((origin, list));
                }
            }
        }
        let Some((_, declarations)) = groups.iter_mut().find(|(s, _)| s == ".wp-block-button")
        else {
            return groups;
        };
        let prefix = "var(--wp--preset--dimension--";
        for declaration in declarations.iter_mut().filter(|d| d.name == "width") {
            let Some(value) = as_str(&declaration.value) else {
                continue;
            };
            let percentage = if value.ends_with('%') {
                Some(string_to_float(value.as_bytes()))
            } else {
                value.strip_prefix(prefix).and_then(|rest| {
                    let slug = rest.get(..rest.len().saturating_sub(1)).unwrap_or_default();
                    let size = sizes
                        .iter()
                        .flat_map(|(_, list)| php::entries(list))
                        .find(|(_, p)| {
                            php::get(p, &["slug"]).and_then(as_str) == Some(slug)
                                && php::isset(php::get(p, &["size"]))
                        })
                        .and_then(|(_, p)| php::get(p, &["size"]))
                        .and_then(as_str)?;
                    size.ends_with('%')
                        .then(|| string_to_float(size.as_bytes()))
                })
            };
            if let Some(p) = percentage {
                let p = float_to_string(p);
                declaration.value = string(&format!(
                    "calc({p} * 1% - (var(--wp--style--block-gap, 0.5em) * (1 - {p} / 100)))"
                ));
            }
        }
        groups
    }

    /// `get_styles_for_block( $block_metadata )`, without style variations.
    pub(crate) fn styles_for_block(&self, meta: &StyleNode) -> String {
        let mut node = php::get(self.data, &refs(&meta.path))
            .cloned()
            .unwrap_or_else(php::array);
        let use_root_padding = self.use_root_padding();
        let selector = meta.selector.clone().unwrap_or_default();
        let groups = self.features(meta, &mut node);
        let is_root = selector == "body";

        // The last path item names the element of element nodes; for other
        // nodes it is never an element name (block names have a slash).
        let current_element = meta.path.last().cloned();
        let allowed = lookup(
            &ELEMENT_PSEUDO,
            current_element.as_deref().unwrap_or_default(),
        );
        let pseudo = allowed.iter().find(|p| {
            selector
                .match_indices(*p)
                .any(|(at, _)| selector.as_bytes().get(at + p.len()) != Some(&b'-'))
        });
        let pseudo_node = pseudo
            .and_then(|p| php::get(&node, &[p]))
            .filter(|n| php::isset(Some(n)));
        let declarations = self.compute(
            pseudo_node.unwrap_or(&node),
            Some(&selector),
            use_root_padding,
        );

        let mut rules = String::new();
        let (duotone, mut declarations): (Vec<_>, Vec<_>) =
            declarations.into_iter().partition(|d| d.name == "filter");
        let duotone: Vec<Declaration> = duotone
            .into_iter()
            .filter(|d| as_str(&d.value) != Some("unset"))
            .collect();
        if is_root
            && declarations
                .iter()
                .any(|d| d.name == "background" || d.name == "background-image")
        {
            rules.push_str(&ruleset(
                "html",
                &[Declaration::text(
                    "min-height",
                    "calc(100% - var(--wp-admin--admin-bar--height, 0px))",
                )],
            ));
        }
        if selector == ".wp-block-separator" {
            separator(&mut declarations);
        }
        let element_only = is_root
            || current_element.as_deref().is_some_and(|e| {
                e != "button" && e != "caption" && element_selector(e) == Some(selector.as_str())
            });
        let general = if element_only {
            selector.clone()
        } else {
            format!(":root :where({selector})")
        };
        rules.push_str(&ruleset(&general, &declarations));
        if let Some(duotone_selector) = &meta.duotone {
            rules.push_str(&ruleset(duotone_selector, &duotone));
        }
        if !is_root && meta.name.as_deref().is_some_and(|n| !n.is_empty()) {
            rules.push_str(&self.layout_styles(meta, None));
        }
        for (feature_selector, declarations) in &groups {
            rules.push_str(&ruleset(
                &format!(":root :where({feature_selector})"),
                declarations,
            ));
        }
        if meta.media_query.is_none() {
            rules.push_str(&self.variation_rules(meta));
        }
        if let Some(css) = php::get(&node, &["css"]).filter(|c| php::isset(Some(c)) && !is_root) {
            let custom = meta.selectors.as_ref().and_then(|s| php::get(s, &["css"]));
            let custom = match custom {
                Some(Value::Array(_)) => custom.and_then(|c| php::get(c, &["root"])),
                other => other,
            };
            let css_selector = custom.and_then(as_str).unwrap_or(&selector);
            if !php::empty(Some(css)) {
                rules.push_str(&nest_custom(&php_string(css), css_selector));
            }
        }
        match &meta.media_query {
            Some(query) if !rules.is_empty() => format!("{query}{{{rules}}}"),
            _ => rules,
        }
    }

    /// The style variation rules of `get_styles_for_block()`.
    fn variation_rules(&self, meta: &StyleNode) -> String {
        let mut declarations = php::Ordered::new();
        let mut after = Vec::new();
        let mut pseudo_css = String::new();
        for variation in &meta.variations {
            let mut node = php::get(self.data, &refs(&variation.path))
                .cloned()
                .unwrap_or_else(php::array);
            for (feature_selector, list) in self.features(meta, &mut node) {
                declarations.put(
                    variation_feature_selector(variation, &feature_selector),
                    list,
                );
            }
            declarations.put(variation.selector.clone(), self.compute(&node, None, false));
            for (pseudo_selector, list) in self.variation_pseudo(&node, meta, variation) {
                declarations.put(pseudo_selector, list);
            }
            let mut css = self.variation_layout(meta, variation, &node);
            if let Some(custom) = php::get(&node, &["css"]).filter(|c| php::isset(Some(c))) {
                css.push_str(&nest_custom(&php_string(custom), &variation.selector));
            }
            for (state, media) in &self.media_queries() {
                let Some(state_node) = php::get(&node, &[state]).filter(|n| php::isset(Some(n)))
                else {
                    continue;
                };
                let (state_css, state_pseudo) =
                    self.variation_state(meta, variation, &mut state_node.clone());
                let wrap = |rules: Vec<String>| -> String {
                    rules.iter().map(|r| format!("{media}{{{r}}}")).collect()
                };
                css.push_str(&wrap(state_css));
                pseudo_css.push_str(&wrap(state_pseudo));
            }
            after.push((variation.selector.clone(), css));
        }
        let mut rules = String::new();
        for (selector, list) in declarations.into_vec() {
            rules.push_str(&ruleset(&format!(":root :where({selector})"), &list));
            if let Some((_, css)) = after.iter().find(|(s, _)| *s == selector) {
                rules.push_str(css);
            }
        }
        rules.push_str(&pseudo_css);
        rules
    }

    /// The rules of one responsive state of a variation, each to go in the
    /// state's media query: those in place, and those of pseudo-classes,
    /// which follow every variation.
    fn variation_state(
        &self,
        meta: &StyleNode,
        variation: &Variation,
        node: &mut Value,
    ) -> (Vec<String>, Vec<String>) {
        let mut rules = Vec::new();
        let mut pseudo_rules = Vec::new();
        let rule = |selector: &str, declarations: &[Declaration]| {
            ruleset(&format!(":root :where({selector})"), declarations)
        };
        for (feature_selector, list) in self.features(meta, node) {
            rules.push(rule(
                &variation_feature_selector(variation, &feature_selector),
                &list,
            ));
        }
        let base = self.compute(node, None, false);
        if !base.is_empty() {
            rules.push(rule(&variation.selector, &base));
        }
        for (pseudo_selector, list) in self.variation_pseudo(node, meta, variation) {
            pseudo_rules.push(rule(&pseudo_selector, &list));
        }
        if let Some(custom) = php::get(node, &["css"]).filter(|c| php::isset(Some(c))) {
            rules.push(nest_custom(&php_string(custom), &variation.selector));
        }
        let layout = self.variation_layout(meta, variation, node);
        if !layout.is_empty() {
            rules.push(layout);
        }
        let elements = meta
            .name
            .as_deref()
            .and_then(|name| self.blocks.get(name))
            .map(|m| m.elements.as_slice())
            .unwrap_or_default();
        let styled = php::entries(php::get(node, &["elements"]).unwrap_or(&Value::Null))
            .iter()
            .flat_map(|(element, element_node)| {
                let element = php::key_str(element);
                elements
                    .iter()
                    .filter(move |(e, _)| *e == element)
                    .map(move |(name, selector)| {
                        (
                            *name,
                            variation_feature_selector(variation, selector),
                            element_node,
                        )
                    })
            });
        for (element, selector, element_node) in styled {
            let declarations = self.compute(element_node, None, false);
            if !declarations.is_empty() {
                rules.push(rule(&selector, &declarations));
            }
            if let Some(custom) = php::get(element_node, &["css"]).filter(|c| php::isset(Some(c))) {
                rules.push(nest_custom(&php_string(custom), &selector));
            }
            for pseudo in lookup(&ELEMENT_PSEUDO, element) {
                let declarations = php::get(element_node, &[pseudo])
                    .filter(|n| php::isset(Some(n)))
                    .map(|n| self.compute(n, None, false))
                    .unwrap_or_default();
                if !declarations.is_empty() {
                    pseudo_rules.push(rule(&selector::append(&selector, pseudo), &declarations));
                }
            }
        }
        (rules, pseudo_rules)
    }

    /// Feature declarations of `node`, taken out of it, as every caller of
    /// `get_feature_declarations_for_node()` updates them.
    fn features(&self, meta: &StyleNode, node: &mut Value) -> Grouped {
        let groups = self.feature_declarations(meta.selectors.as_ref(), node);
        self.button_width(self.paragraph_indent(groups, meta.name.as_deref()))
    }

    /// The block gap rules of a variation node: `get_layout_styles()` on the
    /// variation selector followed by the block's.
    fn variation_layout(&self, meta: &StyleNode, variation: &Variation, node: &Value) -> String {
        if !php::isset(php::get(node, &["spacing", "blockGap"])) {
            return String::new();
        }
        let layout = StyleNode {
            name: meta.name.clone(),
            selector: Some(format!(
                "{}{}",
                variation.selector,
                meta.css.as_deref().unwrap_or_default()
            )),
            ..StyleNode::default()
        };
        self.layout_styles(&layout, Some(node))
    }

    /// `process_pseudo_selectors()` for the node of a style variation.
    fn variation_pseudo(&self, node: &Value, meta: &StyleNode, variation: &Variation) -> Grouped {
        let mut groups = Grouped::new();
        let mut add = |selector: String, declarations: Vec<Declaration>| {
            if !declarations.is_empty() {
                group(&mut groups, &selector, declarations);
            }
        };
        for pseudo in lookup(&BLOCK_PSEUDO, meta.name.as_deref().unwrap_or_default()) {
            let Some(pseudo_node) = php::get(node, &[pseudo]).filter(|n| php::isset(Some(n)))
            else {
                continue;
            };
            let mut pseudo_node = pseudo_node.clone();
            for (feature_selector, list) in self.features(meta, &mut pseudo_node) {
                let target = variation_feature_selector(variation, &feature_selector);
                add(selector::append(&target, pseudo), list);
            }
            add(
                selector::append(&variation.selector, pseudo),
                self.compute(&pseudo_node, None, false),
            );
        }
        groups
    }

    /// `get_layout_styles( $block_metadata, $options )`; `node` stands in
    /// for the node at the metadata's path.
    fn layout_styles(&self, meta: &StyleNode, node: Option<&Value>) -> String {
        let mut rules = String::new();
        let block = meta.name.as_deref().map(|name| self.blocks.get(name));
        if let Some(block) = block {
            let supported = |feature: &str| {
                block
                    .and_then(|b| php::get(&b.supports, &[feature]))
                    .is_some_and(|s| matches!(s, Value::Bool(true) | Value::Array(_)))
            };
            if !supported("layout") && !supported("__experimentalLayout") {
                return rules;
            }
        }
        let selector = meta.selector.clone().unwrap_or_default();
        let is_root = selector == "body";
        let has_gap_support = php::isset(php::get(self.data, &["settings", "spacing", "blockGap"]));
        let node = node
            .or_else(|| php::get(self.data, &refs(&meta.path)))
            .cloned()
            .unwrap_or_else(php::array);

        let gap = if has_gap_support {
            self.property_value(&node, &["spacing", "blockGap"], false)
        } else {
            // Only the root node comes here without a block.
            match block.flatten() {
                Some(block) => php::get(
                    &block.supports,
                    &["spacing", "blockGap", "__experimentalDefault"],
                )
                .cloned()
                .unwrap_or(Value::Null),
                None => string("0.5em"),
            }
        };
        let gap = match gap {
            Value::Array(_)
                if php::isset(php::get(&gap, &["top"]))
                    && php::isset(php::get(&gap, &["left"])) =>
            {
                let row = self.property_value(&node, &["spacing", "blockGap", "top"], false);
                let column = self.property_value(&node, &["spacing", "blockGap", "left"], false);
                if row == column {
                    row
                } else {
                    string(&format!("{} {}", php_string(&row), php_string(&column)))
                }
            }
            Value::Array(_) => Value::Null,
            gap => gap,
        };
        let has_gap = !matches!(&gap, Value::Null | Value::Bool(false)) && gap != string("");
        for layout in LAYOUTS
            .iter()
            .filter(|l| has_gap && (has_gap_support || l.key == "flex" || l.key == "grid"))
        {
            for (rule_selector, properties) in layout.spacing {
                let declarations: Vec<Declaration> = properties
                    .iter()
                    .map(|(property, value)| {
                        Declaration::new(property, value.map_or_else(|| gap.clone(), string))
                    })
                    .filter(|d| is_safe(&d.name, &php_string(&d.value)))
                    .collect();
                let class = layout.class;
                let layout_selector = match (has_gap_support, is_root) {
                    (false, true) => format!(":where(.{class}{rule_selector})"),
                    (false, false) => format!(":where({selector}.{class}{rule_selector})"),
                    (true, true) => format!(":root :where(.{class}){rule_selector}"),
                    (true, false) => format!(":root :where({selector}-{class}){rule_selector}"),
                };
                rules.push_str(&ruleset(&layout_selector, &declarations));
            }
        }
        if is_root {
            let sizes = ["contentSize", "wideSize"]
                .iter()
                .any(|k| php::isset(php::get(self.data, &["settings", "layout", k])));
            for layout in &LAYOUTS {
                let class = layout.class;
                if let Some(display) = layout.display {
                    rules.push_str(&ruleset(
                        &format!("{selector} .{class}"),
                        &[Declaration::text("display", display)],
                    ));
                }
                for (rule_selector, properties) in layout.base {
                    let declarations: Vec<Declaration> = properties
                        .iter()
                        .filter_map(|(property, value)| value.map(|v| (property, v)))
                        .filter(|(_, v)| {
                            sizes
                                || !(v.contains("--global--content-size")
                                    || v.contains("--global--wide-size"))
                        })
                        .filter(|(p, v)| is_safe(p, v))
                        .map(|(p, v)| Declaration::text(p, v))
                        .collect();
                    rules.push_str(&ruleset(&format!(".{class}{rule_selector}"), &declarations));
                }
            }
        }
        rules
    }

    /// `get_root_layout_rules( $selector, $block_metadata )`.
    fn root_layout_rules(&self, meta: &StyleNode) -> String {
        let mut css = String::new();
        let layout = |key: &str| {
            php::get(self.data, &["settings", "layout", key]).filter(|v| php::isset(Some(v)))
        };
        if layout("contentSize").is_some() || layout("wideSize").is_some() {
            let size = |first: &str, second: &str| {
                let size = php_string(
                    layout(first)
                        .or_else(|| layout(second))
                        .unwrap_or(&Value::Null),
                );
                if is_safe("max-width", &size) {
                    size
                } else {
                    "initial".to_owned()
                }
            };
            css.push_str(&format!(
                ":root {{ --wp--style--global--content-size: {};--wp--style--global--wide-size: {}; }}",
                size("contentSize", "wideSize"),
                size("wideSize", "contentSize"),
            ));
        }
        css.push_str(":where(body) { margin: 0; }");
        if self.use_root_padding() {
            css.push_str(".wp-site-blocks { padding-top: var(--wp--style--root--padding-top); padding-bottom: var(--wp--style--root--padding-bottom); }");
            css.push_str(".has-global-padding { padding-right: var(--wp--style--root--padding-right); padding-left: var(--wp--style--root--padding-left); }");
            css.push_str(".has-global-padding > .alignfull { margin-right: calc(var(--wp--style--root--padding-right) * -1); margin-left: calc(var(--wp--style--root--padding-left) * -1); }");
            css.push_str(".has-global-padding :where(:not(.alignfull.is-layout-flow) > .has-global-padding:not(.wp-block-block, .alignfull)) { padding-right: 0; padding-left: 0; }");
            css.push_str(".has-global-padding :where(:not(.alignfull.is-layout-flow) > .has-global-padding:not(.wp-block-block, .alignfull)) > .alignfull { margin-left: 0; margin-right: 0; }");
        }
        css.push_str(".wp-site-blocks > .alignleft { float: left; margin-right: 2em; }");
        css.push_str(".wp-site-blocks > .alignright { float: right; margin-left: 2em; }");
        css.push_str(".wp-site-blocks > .aligncenter { justify-content: center; margin-left: auto; margin-right: auto; }");
        if php::isset(php::get(self.data, &["settings", "spacing", "blockGap"])) {
            let gap = php_string(&self.property_value(
                self.data,
                &["styles", "spacing", "blockGap"],
                false,
            ));
            css.push_str(&format!(
                ":where(.wp-site-blocks) > * {{ margin-block-start: {gap}; margin-block-end: 0; }}"
            ));
            css.push_str(":where(.wp-site-blocks) > :first-child { margin-block-start: 0; }");
            css.push_str(":where(.wp-site-blocks) > :last-child { margin-block-end: 0; }");
            css.push_str(&format!(":root {{ --wp--style--block-gap: {gap}; }}"));
        }
        css.push_str(&self.layout_styles(meta, None));
        css
    }

    /// `get_setting_nodes()` with selectors.
    fn setting_nodes(&self) -> Vec<SettingNode> {
        crate::theme_json::setting_paths(self.data)
            .into_iter()
            .filter_map(|path| match path.get(2) {
                None => Some(SettingNode {
                    path,
                    selector: ":root".to_owned(),
                    selectors: php::array(),
                }),
                Some(name) => self.blocks.get(name).map(|meta| SettingNode {
                    selector: meta.selector.clone(),
                    selectors: meta.selectors.clone(),
                    path: path.clone(),
                }),
            })
            .collect()
    }

    /// `get_settings_values_by_slug()`: `(slug, value)` in origin order.
    fn values_by_slug(
        &self,
        settings: &Value,
        preset: &Preset,
        origins: &[&str],
    ) -> Vec<(String, Value)> {
        let per_origin = php::get(settings, &preset.path).unwrap_or(&Value::Null);
        let mut values = php::Ordered::new();
        for origin in origins {
            for (_, item) in php::entries(php::get(per_origin, &[origin]).unwrap_or(&Value::Null)) {
                let slug = kebab_case(&php_string(
                    php::get(item, &["slug"]).unwrap_or(&Value::Null),
                ));
                let value = match preset.value {
                    PresetValue::Key(key) => php::get(item, &[key])
                        .filter(|v| php::isset(Some(v)))
                        .cloned(),
                    PresetValue::FontSize => {
                        Some(font_size_value(item, settings, self.global).unwrap_or(Value::Null))
                    }
                };
                if let Some(value) = value {
                    values.put(slug, value);
                }
            }
        }
        values.into_vec()
    }

    /// `get_css_variables( $setting_nodes, $origins )`.
    fn css_variables(&self, origins: &[&str]) -> String {
        let mut css = String::new();
        for node in self.setting_nodes() {
            let settings = php::get(self.data, &refs(&node.path))
                .cloned()
                .unwrap_or_else(php::array);
            let mut groups: Grouped = vec![(node.selector.clone(), Vec::new())];
            for preset in PRESETS.iter().filter(|p| !p.css_vars.is_empty()) {
                let values = self.values_by_slug(&settings, preset, origins);
                if values.is_empty() {
                    continue;
                }
                let feature = php::get(&node.selectors, &[preset.path[0]]);
                let target = match feature {
                    Some(Value::String(_)) => feature,
                    _ => feature
                        .and_then(|f| php::get(f, &["root"]))
                        .filter(|r| matches!(r, Value::String(_))),
                }
                .map_or_else(|| node.selector.clone(), php_string);
                let declarations = values
                    .into_iter()
                    .map(|(slug, value)| {
                        Declaration::new(&preset.css_vars.replace("$slug", &slug), value)
                    })
                    .collect();
                group(&mut groups, &target, declarations);
            }
            let custom = flatten(php::get(&settings, &["custom"]).unwrap_or(&Value::Null), "")
                .into_iter()
                .map(|(key, value)| Declaration::new(&format!("--wp--custom--{key}"), value))
                .collect();
            group(&mut groups, &node.selector, custom);
            for (selector, declarations) in &groups {
                css.push_str(&ruleset(selector, declarations));
            }
        }
        css
    }

    /// `get_preset_classes( $setting_nodes, $origins )`.
    fn preset_classes(&self, origins: &[&str]) -> String {
        let mut css = String::new();
        for node in self.setting_nodes() {
            let settings = php::get(self.data, &refs(&node.path))
                .cloned()
                .unwrap_or_else(php::array);
            if node.selector == "body" || node.selector == ":root" {
                css.push_str(&preset_classes(&settings, "", origins));
            } else {
                css.push_str(&preset_classes(&settings, &node.selector, origins));
            }
        }
        css
    }

    /// `get_stylesheet( $types, $origins, $options )`.
    pub(crate) fn stylesheet(
        &self,
        types: Types,
        origins: &[&str],
        options: Options<'_>,
    ) -> String {
        let mut nodes = self.style_nodes(options.blocks, options.scope.is_some());
        let root = nodes
            .iter()
            .position(|n| n.selector.as_deref() == Some("body"));
        if let Some(scope) = options.scope {
            for node in &mut nodes {
                scope_node(scope, node);
            }
        }
        let mut css = String::new();
        if types.variables {
            css.push_str(&self.css_variables(origins));
        }
        if types.styles {
            let root = root
                .filter(|_| options.scope.is_none())
                .and_then(|r| nodes.get(r));
            if let Some(root) = root {
                css.push_str(&self.root_layout_rules(root));
            }
            for node in nodes.iter().filter(|n| n.selector.is_some()) {
                css.push_str(&self.styles_for_block(node));
            }
        }
        if types.presets {
            css.push_str(&self.preset_classes(origins));
        }
        if types.custom_css {
            css.push_str(&php_string(
                php::get(self.data, &["styles", "css"]).unwrap_or(&Value::Null),
            ));
        }
        css
    }
}

/// `compute_preset_classes( $settings, $selector, $origins )` for a block
/// selector, or the root when `selector` is empty.
fn preset_classes(settings: &Value, selector: &str, origins: &[&str]) -> String {
    let mut css = String::new();
    for preset in PRESETS.iter().filter(|p| !p.classes.is_empty()) {
        let per_origin = php::get(settings, &preset.path).unwrap_or(&Value::Null);
        let mut slugs = php::Ordered::new();
        for origin in origins {
            for (_, item) in php::entries(php::get(per_origin, &[origin]).unwrap_or(&Value::Null)) {
                let slug = kebab_case(&php_string(
                    php::get(item, &["slug"]).unwrap_or(&Value::Null),
                ));
                slugs.put(slug, ());
            }
        }
        let slugs = slugs.into_vec();
        for (class, property) in preset.classes {
            for (slug, ()) in &slugs {
                let class = class.replace("$slug", slug);
                let var = preset.css_vars.replace("$slug", slug);
                let selector = if selector.is_empty() {
                    class
                } else {
                    format!(":where({selector}){class}")
                };
                css.push_str(&ruleset(
                    &selector,
                    &[Declaration::text(
                        property,
                        &format!("var({var}) !important"),
                    )],
                ));
            }
        }
    }
    css
}

/// `scope_style_node_selectors( $scope, $node )`.
fn scope_node(scope: &str, node: &mut StyleNode) {
    let scoped = |selector: &Value| string(&selector::scope(scope, &php_string(selector)));
    node.css = node.selector.clone();
    node.selector = node.selector.as_deref().map(|s| selector::scope(scope, s));
    if let Some(Value::Array(features)) = &mut node.selectors {
        for (_, feature) in features.iter_mut() {
            match feature {
                Value::Array(subs) => {
                    for (_, sub) in subs.iter_mut() {
                        *sub = scoped(sub);
                    }
                }
                _ => *feature = scoped(feature),
            }
        }
    }
}

/// `get_block_style_variation_feature_selector()`: a feature selector of
/// the scoped block with the scope taken off, made a variation selector.
fn variation_feature_selector(variation: &Variation, feature_selector: &str) -> String {
    let prefix = format!(".is-style-{} ", variation.name);
    let parts: Vec<String> = selector::split(feature_selector)
        .iter()
        .map(|part| part.strip_prefix(&prefix).unwrap_or(part).to_owned())
        .collect();
    selector::variation(&variation.name, &parts.join(", "))
}

/// `flatten_tree( $tree )`: `custom` settings as kebab-case variable names.
fn flatten(tree: &Value, prefix: &str) -> Vec<(String, Value)> {
    let mut out = php::Ordered::new();
    for (key, value) in php::entries(tree) {
        let key = format!(
            "{prefix}{}",
            kebab_case(&php::key_str(key)).replace('/', "-")
        );
        let entries = match value {
            Value::Array(_) => flatten(value, &format!("{key}--")),
            _ => vec![(key, value.clone())],
        };
        for (k, v) in entries {
            out.put(k, v);
        }
    }
    out.into_vec()
}

/// `update_separator_declarations()`: a separator with only a background
/// colour gets it as its text colour too.
fn separator(declarations: &mut Vec<Declaration>) {
    let background = declarations
        .iter()
        .find(|d| d.name == "background-color" && php::isset(Some(&d.value)))
        .map(|d| d.value.clone());
    let other = declarations
        .iter()
        .any(|d| d.name == "border-color" || d.name == "color");
    if let Some(background) = background.filter(|b| !other && !php::empty(Some(b))) {
        declarations.push(Declaration::new("color", background));
    }
}

/// The style engine's `background-image` for a background image and a
/// gradient (`wp_style_engine_get_styles()`): `url('...')` for an image
/// object, a string as it is, the gradient first when both are set. `None`
/// when neither gives a value.
///
/// Like PHP, the URL is not escaped: theme.json values are trusted, and user
/// styles are sanitized when saved. A URL that would end the CSS string is
/// dropped all the same.
fn background_image(image: Option<&Value>, gradient: Option<&Value>) -> Option<Value> {
    let image = image.and_then(|image| match image {
        Value::String(_) => Some(image.clone()),
        _ => php::get(image, &["url"])
            .filter(|url| !php::empty(Some(url)))
            .map(php_string)
            .filter(|url| !url.contains(['\'', '\\', '\n', '\r']))
            .map(|url| string(&format!("url('{url}')"))),
    });
    let gradient = gradient.and_then(|gradient| match gradient {
        Value::Array(_) => None,
        Value::String(text) if text.windows(4).any(|w| w == b"var:") => {
            let text = php_string(gradient);
            let slug = text
                .contains("var:preset|gradient|")
                .then(|| kebab_case(text.rsplit('|').next().unwrap_or_default()))
                .filter(|slug| !slug.is_empty());
            slug.map(|slug| string(&format!("var(--wp--preset--gradient--{slug})")))
        }
        _ => Some(gradient.clone()),
    });
    match (gradient, image) {
        (Some(gradient), Some(image)) => Some(string(&format!(
            "{}, {}",
            php_string(&gradient),
            php_string(&image)
        ))),
        (gradient, image) => gradient.or(image),
    }
}

/// The elements a block node styles, outside any breakpoint or inside one,
/// each once, in first-seen order.
pub(crate) fn element_names(node: &Value, breakpoints: &[(String, String)]) -> Vec<String> {
    let mut names = php::Ordered::new();
    let sources = std::iter::once(php::get(node, &["elements"])).chain(
        breakpoints
            .iter()
            .map(|(b, _)| php::get(node, &[b, "elements"])),
    );
    for source in sources {
        for (element, _) in php::entries(source.unwrap_or(&Value::Null)) {
            names.put(php::key_str(element), ());
        }
    }
    names
        .into_vec()
        .into_iter()
        .map(|(name, ())| name)
        .collect()
}

/// Feature selectors with a pseudo-class appended to each.
fn append_all(selectors: &Value, pseudo: &str) -> Value {
    let appended = |v: &Value| string(&selector::append(&php_string(v), pseudo));
    Value::Array(
        php::entries(selectors)
            .iter()
            .map(|(k, v)| match v {
                Value::Array(subs) => (
                    k.clone(),
                    Value::Array(
                        subs.iter()
                            .map(|(sk, sv)| (sk.clone(), appended(sv)))
                            .collect(),
                    ),
                ),
                v => (k.clone(), appended(v)),
            })
            .collect(),
    )
}

#[cfg(test)]
pub(crate) mod tests;
