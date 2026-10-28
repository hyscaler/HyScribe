// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Theme_JSON::sanitize()`: keeps only what the theme.json schema
//! allows. PHP builds the schema as one large array per call; here it is a
//! small set of node kinds that answer "what may sit under this key".

use hyscribe_phpser::{Key, Value};

use crate::blocks::{Blocks, element_selector};
use crate::php::{self, key_str};
use crate::viewport;

/// A static part of the schema: `null` (anything), `true` (booleans only),
/// or an object.
#[derive(Clone, Copy, Debug, PartialEq)]
enum S {
    Any,
    Bool,
    Map(&'static [(&'static str, S)]),
}

use S::{Any, Bool, Map};

const FONT_FACE: &[(&str, S)] = &[
    ("ascentOverride", Any),
    ("descentOverride", Any),
    ("fontDisplay", Any),
    ("fontFamily", Any),
    ("fontFeatureSettings", Any),
    ("fontStyle", Any),
    ("fontStretch", Any),
    ("fontVariationSettings", Any),
    ("fontWeight", Any),
    ("lineGapOverride", Any),
    ("sizeAdjust", Any),
    ("src", Any),
    ("unicodeRange", Any),
];
const FONT_FAMILY: &[(&str, S)] = &[
    ("fontFamily", Any),
    ("name", Any),
    ("slug", Any),
    ("fontFace", Map(&[("0", Map(FONT_FACE))])),
];
const FONT_FAMILY_LIST: &[(&str, S)] = &[("0", Map(FONT_FAMILY))];
/// `schema_in_root_and_per_origin( FONT_FAMILY_SCHEMA )`.
const FONT_FAMILIES: &[(&str, S)] = &[
    ("0", Map(FONT_FAMILY)),
    ("default", Map(FONT_FAMILY_LIST)),
    ("blocks", Map(FONT_FAMILY_LIST)),
    ("theme", Map(FONT_FAMILY_LIST)),
    ("custom", Map(FONT_FAMILY_LIST)),
];

const TYPOGRAPHY_SETTINGS: &[(&str, S)] = &[
    ("fluid", Any),
    ("customFontSize", Any),
    ("defaultFontSizes", Any),
    ("dropCap", Any),
    ("fontFamilies", Any),
    ("fontSizes", Any),
    ("fontStyle", Any),
    ("fontWeight", Any),
    ("letterSpacing", Any),
    ("lineHeight", Any),
    ("textAlign", Any),
    ("textColumns", Any),
    ("textDecoration", Any),
    ("textIndent", Any),
    ("textTransform", Any),
    ("writingMode", Any),
];

/// `VALID_SETTINGS`.
const SETTINGS: &[(&str, S)] = &[
    ("appearanceTools", Any),
    ("useRootPaddingAwareAlignments", Any),
    (
        "background",
        Map(&[
            ("backgroundImage", Any),
            ("backgroundSize", Any),
            ("gradient", Any),
        ]),
    ),
    (
        "border",
        Map(&[
            ("color", Any),
            ("radius", Any),
            ("radiusSizes", Any),
            ("style", Any),
            ("width", Any),
        ]),
    ),
    (
        "color",
        Map(&[
            ("background", Any),
            ("custom", Any),
            ("customDuotone", Any),
            ("customGradient", Any),
            ("defaultDuotone", Any),
            ("defaultGradients", Any),
            ("defaultPalette", Any),
            ("duotone", Any),
            ("gradients", Any),
            ("link", Any),
            ("heading", Any),
            ("button", Any),
            ("caption", Any),
            ("palette", Any),
            ("text", Any),
        ]),
    ),
    ("custom", Any),
    (
        "dimensions",
        Map(&[
            ("aspectRatio", Any),
            ("aspectRatios", Any),
            ("defaultAspectRatios", Any),
            ("dimensionSizes", Any),
            ("height", Any),
            ("minHeight", Any),
            ("minWidth", Any),
            ("width", Any),
        ]),
    ),
    (
        "layout",
        Map(&[
            ("contentSize", Any),
            ("wideSize", Any),
            ("allowEditing", Any),
            ("allowCustomContentAndWideSize", Any),
        ]),
    ),
    (
        "lightbox",
        Map(&[("enabled", Bool), ("allowEditing", Bool)]),
    ),
    ("position", Map(&[("fixed", Any), ("sticky", Any)])),
    ("blockVisibility", Map(&[("allowEditing", Bool)])),
    (
        "spacing",
        Map(&[
            ("customSpacingSize", Any),
            ("defaultSpacingSizes", Any),
            ("spacingSizes", Any),
            ("spacingScale", Any),
            ("blockGap", Any),
            ("margin", Any),
            ("padding", Any),
            ("units", Any),
        ]),
    ),
    ("shadow", Map(&[("presets", Any), ("defaultPresets", Any)])),
    ("typography", Map(TYPOGRAPHY_SETTINGS)),
    ("viewport", Map(&[("mobile", Any), ("tablet", Any)])),
];

/// `VALID_STYLES`.
const STYLES: &[(&str, S)] = &[
    (
        "background",
        Map(&[
            ("backgroundImage", Any),
            ("backgroundPosition", Any),
            ("backgroundRepeat", Any),
            ("backgroundSize", Any),
            ("backgroundAttachment", Any),
            ("gradient", Any),
        ]),
    ),
    (
        "border",
        Map(&[
            ("color", Any),
            ("radius", Any),
            ("style", Any),
            ("width", Any),
            ("top", Any),
            ("right", Any),
            ("bottom", Any),
            ("left", Any),
        ]),
    ),
    (
        "color",
        Map(&[("background", Any), ("gradient", Any), ("text", Any)]),
    ),
    (
        "dimensions",
        Map(&[
            ("aspectRatio", Any),
            ("height", Any),
            ("minHeight", Any),
            ("minWidth", Any),
            ("width", Any),
        ]),
    ),
    ("filter", Map(&[("duotone", Any)])),
    (
        "outline",
        Map(&[
            ("color", Any),
            ("offset", Any),
            ("style", Any),
            ("width", Any),
        ]),
    ),
    ("shadow", Any),
    (
        "spacing",
        Map(&[("margin", Any), ("padding", Any), ("blockGap", Any)]),
    ),
    (
        "typography",
        Map(&[
            ("fontFamily", Any),
            ("fontSize", Any),
            ("fontStyle", Any),
            ("fontWeight", Any),
            ("letterSpacing", Any),
            ("lineHeight", Any),
            ("textAlign", Any),
            ("textColumns", Any),
            ("textDecoration", Any),
            ("textIndent", Any),
            ("textShadow", Any),
            ("textTransform", Any),
            ("writingMode", Any),
        ]),
    ),
    ("css", Any),
];

/// The top-level keys a theme.json keeps.
const TOP_LEVEL_KEYS: [&str; 10] = [
    "blockTypes",
    "customTemplates",
    "description",
    "patterns",
    "settings",
    "slug",
    "styles",
    "templateParts",
    "title",
    "version",
];

const LINK_PSEUDO: &[&str] = &[
    ":link",
    ":any-link",
    ":visited",
    ":hover",
    ":focus",
    ":focus-visible",
    ":active",
];
const STATE_PSEUDO: &[&str] = &[":hover", ":focus", ":focus-visible", ":active"];

/// Pseudo-classes elements may style.
pub(crate) const ELEMENT_PSEUDO: [(&str, &[&str]); 2] =
    [("link", LINK_PSEUDO), ("button", LINK_PSEUDO)];

/// Pseudo-classes blocks may style.
pub(crate) const BLOCK_PSEUDO: [(&str, &[&str]); 2] = [
    ("core/button", STATE_PSEUDO),
    ("core/navigation-link", STATE_PSEUDO),
];

/// Block states that map to classes rather than pseudo-classes.
pub(crate) const BLOCK_STATES: [(&str, &[&str]); 1] = [("core/navigation-link", &["-current"])];

/// The entries of one of the tables above for `name`.
pub(crate) fn lookup(
    table: &[(&str, &'static [&'static str])],
    name: &str,
) -> &'static [&'static str] {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map_or(&[], |(_, list)| list)
}

/// An object of the schema: what may sit under each of its keys.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Node<'a> {
    Map(&'static [(&'static str, S)]),
    Settings,
    RootTypography,
    SettingsBlocks,
    BlockSettings,
    Styles,
    Elements,
    Element(&'a str),
    Blocks(bool),
    Block(&'a str, bool),
    BlockBreakpoint(&'a str),
    CustomState(&'a str),
    Variations(&'a str),
    Variation(&'a str),
    VariationBreakpoint(&'a str),
}

/// The schema under one key.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind<'a> {
    Any,
    Bool,
    Node(Node<'a>),
}

struct Ctx<'a> {
    blocks: &'a Blocks,
    breakpoints: Vec<String>,
}

fn table(entries: &'static [(&'static str, S)], key: &str) -> Option<Kind<'static>> {
    entries
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, s)| match s {
            Any => Kind::Any,
            Bool => Kind::Bool,
            Map(entries) => Kind::Node(Node::Map(entries)),
        })
}

impl Ctx<'_> {
    fn breakpoint(&self, key: &str) -> bool {
        self.breakpoints.iter().any(|b| b == key)
    }

    fn block(&self, key: &str) -> bool {
        self.blocks.get(key).is_some()
    }

    /// The schema under `key` of `node`.
    fn child<'k>(&self, node: Node<'k>, key: &'k str) -> Option<Kind<'k>> {
        let styles = Kind::Node(Node::Map(STYLES));
        let pseudo = |block: &str| lookup(&BLOCK_PSEUDO, block).contains(&key);
        let some = |n: Node<'k>| Some(Kind::Node(n));
        match node {
            Node::Map(entries) => table(entries, key),
            Node::Settings => match key {
                "blocks" => some(Node::SettingsBlocks),
                "typography" => some(Node::RootTypography),
                _ => table(SETTINGS, key),
            },
            Node::RootTypography if key == "fontFamilies" => some(Node::Map(FONT_FAMILIES)),
            Node::RootTypography => table(TYPOGRAPHY_SETTINGS, key),
            Node::SettingsBlocks => self.block(key).then_some(Kind::Node(Node::BlockSettings)),
            Node::BlockSettings if key == "viewport" || key == "blockVisibility" => None,
            Node::BlockSettings => table(SETTINGS, key),
            Node::Styles => match key {
                "blocks" => some(Node::Blocks(true)),
                "elements" => some(Node::Elements),
                _ => table(STYLES, key),
            },
            Node::Elements => element_selector(key).map(|_| Kind::Node(Node::Element(key))),
            Node::Element(element) => table(STYLES, key).or_else(|| {
                (lookup(&ELEMENT_PSEUDO, element).contains(&key) || self.breakpoint(key))
                    .then_some(styles)
            }),
            Node::Blocks(variations) => self
                .block(key)
                .then_some(Kind::Node(Node::Block(key, variations))),
            Node::Block(block, variations) => match key {
                "elements" => some(Node::Elements),
                "variations" if variations => some(Node::Variations(block)),
                _ if self.breakpoint(key) => some(Node::BlockBreakpoint(block)),
                _ if lookup(&BLOCK_STATES, block).contains(&key) => some(Node::CustomState(block)),
                _ => table(STYLES, key).or_else(|| pseudo(block).then_some(styles)),
            },
            Node::BlockBreakpoint(block) => match key {
                "elements" => some(Node::Elements),
                _ => table(STYLES, key).or_else(|| pseudo(block).then_some(styles)),
            },
            Node::CustomState(block) => {
                table(STYLES, key).or_else(|| pseudo(block).then_some(styles))
            }
            Node::Variations(block) => self
                .blocks
                .get(block)
                .filter(|meta| meta.style_variations.iter().any(|(n, _)| n == key))
                .map(|_| Kind::Node(Node::Variation(block))),
            Node::Variation(block) => match key {
                "blocks" => some(Node::Blocks(false)),
                "elements" => some(Node::Elements),
                _ if self.breakpoint(key) => some(Node::VariationBreakpoint(block)),
                _ => table(STYLES, key).or_else(|| pseudo(block).then_some(styles)),
            },
            Node::VariationBreakpoint(block) => match key {
                "blocks" => some(Node::Blocks(false)),
                "elements" => some(Node::Elements),
                _ => table(STYLES, key).or_else(|| pseudo(block).then_some(styles)),
            },
        }
    }

    /// `remove_keys_not_in_schema( $tree, $schema )` for an object.
    fn remove(&self, entries: Vec<(Key, Value)>, node: Node<'_>) -> Value {
        let mut out = Vec::new();
        for (k, value) in entries {
            let name = key_str(&k);
            match (self.child(node, &name), value) {
                (Some(Kind::Bool), value @ Value::Bool(_)) | (Some(Kind::Any), value) => {
                    out.push((k, value))
                }
                // A list keeps its items, sanitized when the schema describes them.
                (Some(Kind::Node(child)), Value::Array(items))
                    if items.iter().all(|(i, _)| matches!(i, Key::Int(_))) =>
                {
                    let items = match self.child(child, "0") {
                        Some(Kind::Node(item)) => items
                            .into_iter()
                            .map(|(i, v)| match v {
                                Value::Array(e) => (i, self.remove(e, item)),
                                v => (i, v),
                            })
                            .collect(),
                        _ => items,
                    };
                    out.push((k, Value::Array(items)));
                }
                (Some(Kind::Node(child)), Value::Array(object)) => {
                    let value = self.remove(object, child);
                    if !php::empty(Some(&value)) {
                        out.push((k, value));
                    }
                }
                _ => {}
            }
        }
        Value::Array(out)
    }
}

/// `sanitize( $input, ... )` with the block names and variations of `blocks`.
pub(crate) fn sanitize(input: Value, blocks: &Blocks) -> Value {
    let mut output = Value::Array(
        php::entries(&input)
            .iter()
            .filter(|(k, _)| TOP_LEVEL_KEYS.contains(&key_str(k).as_str()))
            .cloned()
            .collect(),
    );
    let viewport_settings = php::get(&input, &["settings", "viewport"]);
    let ctx = Ctx {
        blocks,
        breakpoints: viewport::media_queries(viewport_settings)
            .into_iter()
            .map(|(k, _)| k)
            .collect(),
    };
    for (subtree, root) in [("styles", Node::Styles), ("settings", Node::Settings)] {
        let value = match php::get(&input, &[subtree]) {
            None | Some(Value::Null) => continue,
            Some(Value::Array(entries)) => entries.clone(),
            Some(_) => {
                php::unset(&mut output, &[subtree]);
                continue;
            }
        };
        let mut result = ctx.remove(value, root);
        if let (Some(viewport), "settings") = (viewport_settings, subtree) {
            php::set(&mut result, &["viewport"], viewport::sanitize(viewport));
        }
        if php::empty(Some(&result)) {
            php::unset(&mut output, &[subtree]);
        } else {
            php::set(&mut output, &[subtree], resolve_custom_css_format(result));
        }
    }
    output
}

/// `resolve_custom_css_format()`: `var:preset|color|x` becomes
/// `var(--wp--preset--color--x)` everywhere in the tree.
fn resolve_custom_css_format(tree: Value) -> Value {
    match tree {
        Value::Array(entries) => Value::Array(
            entries
                .into_iter()
                .map(|(k, v)| (k, resolve_custom_css_format(v)))
                .collect(),
        ),
        Value::String(ref bytes) if bytes.starts_with(b"var:") => {
            let name =
                String::from_utf8_lossy(bytes.get(4..).unwrap_or_default()).replace('|', "--");
            php::string(&format!("var(--wp--{name})"))
        }
        other => other,
    }
}

/// Whether a key is one of the origins presets are keyed by.
pub(crate) fn is_origin(key: &Key) -> bool {
    matches!(key, Key::String(k) if crate::theme_json::ORIGINS.iter().any(|o| o.as_bytes() == k.as_slice()))
}
