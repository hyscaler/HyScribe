// SPDX-License-Identifier: AGPL-3.0-or-later

//! Block supports: `get_block_wrapper_attributes()` and the `apply`
//! callbacks `WP_Block_Supports::apply_block_supports()` runs, in the order
//! WordPress registers them. The supports that work through `render_block`
//! filters (layout, position, duotone, background, block style variations,
//! custom CSS and elements) are not part of the wrapper attributes.

use crate::blocks::is_pcre_space;
use hyscribe_kses::{esc_attr, safecss_filter_attr};
use hyscribe_phpser::{Key, Value, is_empty_string, isset, php_trim};
use hyscribe_theme_json::GlobalStyles;

use crate::registry::{BlockType, block_type};
use crate::style::{get_styles, preset_value_slug};

/// The block being rendered, as the supports see it.
struct Block<'a> {
    name: &'a str,
    block_type: &'a BlockType,
    /// The prepared attributes, as a PHP array.
    attributes: Value,
    global: &'a GlobalStyles,
}

/// `wp_should_skip_block_supports_serialization()`, as a condition.
pub(crate) fn skips_serialization(
    block_type: &BlockType,
    feature_set: &str,
    feature: Option<&str>,
) -> bool {
    match block_type.supports.get_path(&[feature_set, SKIP]) {
        Some(Value::Array(features)) => {
            let wanted = feature.map_or(Value::Null, string);
            features.iter().any(|(_, f)| *f == wanted)
        }
        skip => truthy(skip),
    }
}

/// What one support adds: attribute names and values.
type Attributes = Vec<(&'static str, Value)>;

/// The `apply` callbacks, in registration order.
const SUPPORTS: [fn(&Block<'_>) -> Attributes; 11] = [
    align,
    custom_classname,
    generated_classname,
    colors,
    typography,
    border,
    spacing,
    dimensions,
    shadow,
    aria_label,
    anchor,
];

const SKIP: &str = "__experimentalSkipSerialization";

/// The attributes the wrapper merges with extra ones, in output order.
const MERGED: [&str; 4] = ["style", "class", "id", "aria-label"];

/// `get_block_wrapper_attributes( $extra_attributes )` while `name` renders
/// with `attributes` (as parsed): the attributes the block supports give,
/// merged with `extra`, escaped and joined. `global` gives
/// `wp_get_global_settings()`.
pub fn get_block_wrapper_attributes(
    name: &str,
    attributes: &[(Key, Value)],
    extra: &[(&str, Value)],
    global: &GlobalStyles,
) -> String {
    wrapper_attributes(name, block_type(name), attributes, extra, global)
}

/// [`get_block_wrapper_attributes`] for a given block type.
fn wrapper_attributes(
    name: &str,
    block_type: Option<&BlockType>,
    attributes: &[(Key, Value)],
    extra: &[(&str, Value)],
    global: &GlobalStyles,
) -> String {
    let new = block_type
        .map(|block_type| apply_block_supports(name, block_type, attributes, global))
        .unwrap_or_default();
    let extra_value = |name: &str| {
        extra
            .iter()
            .rev()
            .find(|(n, _)| *n == name)
            .and_then(|(_, v)| scalar(v))
            .unwrap_or_default()
    };
    let mut output: Vec<(&str, Vec<u8>)> = Vec::new();
    for name in MERGED {
        let new = new
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_default();
        let extra = extra_value(name);
        if new.is_empty() && extra.is_empty() {
            continue;
        }
        let merged = match name {
            "style" => merge_styles(&new, &extra),
            "class" => merge_classes(&new, &extra),
            _ if extra.is_empty() => new,
            _ => extra,
        };
        output.push((name, merged));
    }
    for (name, value) in extra {
        if let (false, Some(value)) = (MERGED.contains(name), scalar(value)) {
            output.push((name, value));
        }
    }
    output
        .iter()
        .map(|(name, value)| format!("{name}=\"{}\"", esc_attr(value)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// `is_scalar( $value ) && ! is_bool( $value )`, as `(string) $value`.
fn scalar(value: &Value) -> Option<Vec<u8>> {
    matches!(value, Value::Int(_) | Value::Float(_) | Value::String(_))
        .then(|| value.to_php_string())
}

/// The `style` merge: both values trimmed of whitespace, then of trailing
/// `;`, the non-empty ones joined by `;` and filtered again.
fn merge_styles(new: &[u8], extra: &[u8]) -> Vec<u8> {
    let parts: Vec<&[u8]> = [new, extra]
        .into_iter()
        .map(|css| {
            let css = php_trim(css);
            let end = css.iter().rposition(|&b| b != b';').map_or(0, |i| i + 1);
            css.get(..end).unwrap_or_default()
        })
        .filter(|css| !is_empty_string(css))
        .collect();
    safecss_filter_attr(&parts.join(&b';'))
}

/// The `class` merge: the extra classes, then the new ones, each once.
fn merge_classes(new: &[u8], extra: &[u8]) -> Vec<u8> {
    let mut classes: Vec<&[u8]> = Vec::new();
    // `preg_split( '/\s+/' )`: PCRE's `\s` without UTF mode.
    let split = |text| <[u8]>::split(text, |&b| is_pcre_space(b)).filter(|class| !class.is_empty());
    for class in split(extra).chain(split(new)) {
        if !classes.contains(&class) {
            classes.push(class);
        }
    }
    classes.join(&b' ')
}

/// `WP_Block_Supports::apply_block_supports()`: the supports' attributes
/// for the prepared attributes, strings and numbers only, the values of one
/// attribute joined by spaces.
fn apply_block_supports(
    name: &str,
    block_type: &BlockType,
    attributes: &[(Key, Value)],
    global: &GlobalStyles,
) -> Vec<(&'static str, Vec<u8>)> {
    let block = Block {
        name,
        block_type,
        attributes: Value::Array(block_type.prepare(attributes)),
        global,
    };
    let mut output: Vec<(&str, Vec<u8>)> = Vec::new();
    for (name, value) in SUPPORTS.iter().flat_map(|apply| apply(&block)) {
        let Some(value) = scalar(&value) else {
            continue;
        };
        // PHP replaces an empty value instead of appending to it. Only a
        // class can be empty, and the wrapper splits classes again.
        match output.iter_mut().find(|(n, _)| *n == name) {
            Some((_, existing)) => {
                existing.push(b' ');
                existing.extend_from_slice(&value);
            }
            None => output.push((name, value)),
        }
    }
    output
}

fn string(bytes: impl Into<Vec<u8>>) -> Value {
    Value::String(bytes.into())
}

/// A PHP array with string keys.
fn array<'k>(entries: impl IntoIterator<Item = (&'k str, Value)>) -> Value {
    Value::Array(
        entries
            .into_iter()
            .map(|(key, value)| (Key::String(key.as_bytes().to_vec()), value))
            .collect(),
    )
}

/// PHP's truthiness of a value that may be missing.
fn truthy(value: Option<&Value>) -> bool {
    value.is_some_and(|v| !v.is_empty())
}

impl Block<'_> {
    /// `$block_type->supports[$name] ?? false`; `null` stands for `false`.
    fn support(&self, name: &str) -> &Value {
        self.block_type
            .supports
            .get_str(name)
            .unwrap_or(&Value::Null)
    }

    /// `block_has_support( $block_type, $feature, $default )`: the support
    /// is `true` or an array.
    fn has_support(&self, feature: &[&str], default: bool) -> bool {
        let support = match feature {
            [name] => Some(self.support(name)).filter(|s| **s != Value::Null),
            path => self.block_type.supports.get_path(path),
        };
        support.map_or(default, |s| {
            matches!(s, Value::Bool(true) | Value::Array(_))
        })
    }

    fn skips(&self, feature_set: &str, feature: Option<&str>) -> bool {
        skips_serialization(self.block_type, feature_set, feature)
    }

    /// `$block_attributes[...] ?? null`.
    fn attribute(&self, path: &[&str]) -> Value {
        self.attributes
            .get_path(path)
            .cloned()
            .unwrap_or(Value::Null)
    }

    /// `isset( $block_attributes[...] )`.
    fn isset(&self, path: &[&str]) -> bool {
        isset(self.attributes.get_path(path))
    }

    /// `"var:preset|$kind|{$block_attributes[$name]}"`, when the attribute
    /// exists, else the value at `custom`: a preset attribute wins over a
    /// custom style.
    fn preset_or(&self, name: &str, kind: &str, custom: Value) -> Value {
        match self.attributes.get_str(name) {
            Some(slug) => string(
                [
                    format!("var:preset|{kind}|").as_bytes(),
                    &slug.to_php_string(),
                ]
                .concat(),
            ),
            None => custom,
        }
    }

    /// `wp_has_border_feature_support()`.
    fn has_border_support(&self, feature: &str) -> bool {
        *self.support("__experimentalBorder") == Value::Bool(true)
            || self.has_support(&["__experimentalBorder", feature], false)
    }
}

/// The `class` and `style` attributes of compiled styles.
fn styled(styles: &Value, convert_vars_to_classnames: bool) -> Attributes {
    let styles = get_styles(styles, convert_vars_to_classnames);
    let mut attributes = Attributes::new();
    if !styles.classnames.is_empty() {
        attributes.push(("class", string(styles.classnames)));
    }
    if !styles.css.is_empty() {
        attributes.push(("style", string(styles.css)));
    }
    attributes
}

/// `wp_apply_alignment_support()`.
fn align(block: &Block<'_>) -> Attributes {
    match block.attributes.get_str("align") {
        Some(align) if block.has_support(&["align"], false) => {
            vec![(
                "class",
                string([b"align", &align.to_php_string()[..]].concat()),
            )]
        }
        _ => Vec::new(),
    }
}

/// `wp_apply_custom_classname_support()`.
fn custom_classname(block: &Block<'_>) -> Attributes {
    match block.attributes.get_str("className") {
        Some(class) if block.has_support(&["customClassName"], true) => {
            vec![("class", class.clone())]
        }
        _ => Vec::new(),
    }
}

/// `wp_apply_generated_classname_support()`, with
/// `wp_get_block_default_classname()`.
fn generated_classname(block: &Block<'_>) -> Attributes {
    if !block.has_support(&["className"], true) {
        return Vec::new();
    }
    let name = block.name.replace('/', "-");
    let name = name.strip_prefix("core-").unwrap_or(&name);
    vec![("class", string(format!("wp-block-{name}")))]
}

/// `wp_apply_colors_support()`.
fn colors(block: &Block<'_>) -> Attributes {
    let support = block.support("color");
    if matches!(support, Value::Array(_)) && block.skips("color", None) {
        return Vec::new();
    }
    // Text and background colors are on unless switched off.
    let has = |name: &str| {
        *support == Value::Bool(true)
            || truthy(support.get_str(name))
            || (matches!(support, Value::Array(_))
                && support.get_str(name).is_none_or(|v| *v == Value::Null))
    };
    let mut styles = Vec::new();
    if has("text") && !block.skips("color", Some("text")) {
        let custom = block.attribute(&["style", "color", "text"]);
        styles.push(("text", block.preset_or("textColor", "color", custom)));
    }
    if has("background") && !block.skips("color", Some("background")) {
        let custom = block.attribute(&["style", "color", "background"]);
        styles.push((
            "background",
            block.preset_or("backgroundColor", "color", custom),
        ));
    }
    // A background gradient takes over from the color one.
    let background_gradient = block.has_support(&["background", "gradient"], false)
        && truthy(
            block
                .attributes
                .get_path(&["style", "background", "gradient"]),
        );
    if truthy(support.get_str("gradients"))
        && !block.skips("color", Some("gradients"))
        && !background_gradient
    {
        let custom = block.attribute(&["style", "color", "gradient"]);
        styles.push(("gradient", block.preset_or("gradient", "gradient", custom)));
    }
    styled(&array([("color", array(styles))]), true)
}

/// The typography features `wp_apply_typography_support()` copies from
/// `style.typography` when set: the support flag, the feature and, for
/// those that may hold a preset, the CSS property of its variable.
/// `textAlign` is not here: it gives a class, not a style.
const TYPOGRAPHY: [(&str, &str, Option<&str>); 9] = [
    ("__experimentalFontStyle", "fontStyle", Some("font-style")),
    (
        "__experimentalFontWeight",
        "fontWeight",
        Some("font-weight"),
    ),
    ("lineHeight", "lineHeight", None),
    ("textColumns", "textColumns", None),
    (
        "__experimentalTextDecoration",
        "textDecoration",
        Some("text-decoration"),
    ),
    (
        "__experimentalTextTransform",
        "textTransform",
        Some("text-transform"),
    ),
    (
        "__experimentalLetterSpacing",
        "letterSpacing",
        Some("letter-spacing"),
    ),
    ("__experimentalWritingMode", "writingMode", None),
    ("textIndent", "textIndent", None),
];

/// `wp_typography_get_preset_inline_style_value()`: a
/// `var:preset|<property>|<slug>` value as its CSS variable.
fn inline_style_value(value: Value, property: &str) -> Value {
    match preset_value_slug(&value, property) {
        Some(slug) => string(format!("var(--wp--preset--{property}--{slug});")),
        None => value,
    }
}

/// `wp_apply_typography_support()`.
fn typography(block: &Block<'_>) -> Attributes {
    // Without the support, every feature flag reads as off.
    let support = block.support("typography");
    if block.skips("typography", None) {
        return Vec::new();
    }
    let on = |flag: &str, feature: &str| {
        truthy(support.get_str(flag)) && !block.skips("typography", Some(feature))
    };
    let path = |feature| ["style", "typography", feature];
    let mut styles = Vec::new();
    if on("fontSize", "fontSize") {
        let custom = array([("size", block.attribute(&path("fontSize")))]);
        let size = block.global.font_size_value(&custom).unwrap_or(Value::Null);
        styles.push(("fontSize", block.preset_or("fontSize", "font-size", size)));
    }
    if on("__experimentalFontFamily", "fontFamily") {
        let custom = inline_style_value(block.attribute(&path("fontFamily")), "font-family");
        styles.push((
            "fontFamily",
            block.preset_or("fontFamily", "font-family", custom),
        ));
    }
    for (flag, feature, preset) in TYPOGRAPHY {
        if on(flag, feature) && block.isset(&path(feature)) {
            let value = block.attribute(&path(feature));
            styles.push((
                feature,
                preset.map_or_else(|| value.clone(), |p| inline_style_value(value.clone(), p)),
            ));
        }
    }
    let compiled = get_styles(&array([("typography", array(styles))]), true);
    let mut classnames = Vec::new();
    if !compiled.classnames.is_empty() {
        classnames.push(compiled.classnames.into_bytes());
    }
    if on("textAlign", "textAlign") && block.isset(&path("textAlign")) {
        let align = block.attribute(&path("textAlign")).to_php_string();
        classnames.push([b"has-text-align-", &align[..]].concat());
    }
    let mut attributes = Attributes::new();
    if !classnames.is_empty() {
        attributes.push(("class", string(classnames.join(&b' '))));
    }
    if !compiled.css.is_empty() {
        attributes.push(("style", string(compiled.css)));
    }
    attributes
}

/// `wp_apply_border_support()`.
fn border(block: &Block<'_>) -> Attributes {
    if block.skips("border", None) {
        return Vec::new();
    }
    let has = |feature| block.has_border_support(feature);
    let keep = |feature| !block.skips("__experimentalBorder", Some(feature));
    let mut styles = Vec::new();
    // Numbers are pixels, for radius and width.
    for (feature, pixels) in [("radius", true), ("style", false), ("width", true)] {
        let path = ["style", "border", feature];
        if has(feature) && block.isset(&path) && keep(feature) {
            let value = block.attribute(&path);
            let value = match value.is_numeric() && pixels {
                true => string([value.to_php_string(), b"px".to_vec()].concat()),
                false => value,
            };
            styles.push((feature, value));
        }
    }
    if has("color") && keep("color") {
        let custom = block.attribute(&["style", "border", "color"]);
        styles.push(("color", block.preset_or("borderColor", "color", custom)));
    }
    if has("color") || has("width") {
        for side in ["top", "right", "bottom", "left"] {
            let value = |feature| {
                let path = ["style", "border", side, feature];
                match block.isset(&path) && keep(feature) {
                    true => block.attribute(&path),
                    false => Value::Null,
                }
            };
            styles.push((
                side,
                array([
                    ("width", value("width")),
                    ("color", value("color")),
                    ("style", value("style")),
                ]),
            ));
        }
    }
    styled(&array([("border", array(styles))]), false)
}

/// `wp_apply_spacing_support()`.
fn spacing(block: &Block<'_>) -> Attributes {
    if block.skips("spacing", None) {
        return Vec::new();
    }
    let side = |feature| match block.has_support(&["spacing", feature], false)
        && !block.skips("spacing", Some(feature))
    {
        true => block.attribute(&["style", "spacing", feature]),
        false => Value::Null,
    };
    let styles = array([("padding", side("padding")), ("margin", side("margin"))]);
    styled(&array([("spacing", styles)]), false)
}

/// `wp_apply_dimensions_support()`. The aspect ratio is applied when the
/// block renders.
fn dimensions(block: &Block<'_>) -> Attributes {
    if block.skips("dimensions", None) {
        return Vec::new();
    }
    let styles = ["minHeight", "minWidth", "height", "width"].map(|feature| {
        let value = match block.has_support(&["dimensions", feature], false)
            && !block.skips("dimensions", Some(feature))
        {
            true => block.attribute(&["style", "dimensions", feature]),
            false => Value::Null,
        };
        (feature, value)
    });
    styled(&array([("dimensions", array(styles))]), false)
}

/// `wp_apply_shadow_support()`.
fn shadow(block: &Block<'_>) -> Attributes {
    if !block.has_support(&["shadow"], false) || block.skips("shadow", None) {
        return Vec::new();
    }
    styled(
        &array([("shadow", block.attribute(&["style", "shadow"]))]),
        false,
    )
}

/// `wp_apply_aria_label_support()`.
fn aria_label(block: &Block<'_>) -> Attributes {
    match block.attributes.get_str("ariaLabel") {
        Some(label)
            if block.has_support(&["ariaLabel"], false) && !block.skips("ariaLabel", None) =>
        {
            vec![("aria-label", label.clone())]
        }
        _ => Vec::new(),
    }
}

/// `wp_apply_anchor_support()`: a non-empty string anchor is the `id`.
fn anchor(block: &Block<'_>) -> Attributes {
    match block.attributes.get_str("anchor") {
        Some(Value::String(id)) if !id.is_empty() && block.has_support(&["anchor"], false) => {
            vec![("id", string(id.clone()))]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests;
