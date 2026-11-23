// SPDX-License-Identifier: AGPL-3.0-or-later

//! The style engine for block supports: `WP_Style_Engine::parse_block_styles()`
//! turns a block's `style` attribute into class names and CSS declarations,
//! by the same definitions table, and `wp_style_engine_get_styles()`
//! compiles the declarations to a filtered CSS string.

use hyscribe_kses::{safecss_filter_attr, sanitize_key, wp_strip_all_tags};
use hyscribe_phpser::{Key, Value, php_trim};
use hyscribe_theme_json::kebab_case;

/// How a value becomes CSS.
#[derive(Clone, Copy)]
enum Kind {
    /// `property_keys`: the property, and the `%s` pattern for per-side
    /// values when there is one.
    Plain(&'static str, Option<&'static str>),
    /// `get_url_or_value_css_declaration`.
    UrlOrValue(&'static str),
    /// `get_individual_property_css_declarations` for one border side.
    BorderSide(&'static str),
}

/// One entry of `BLOCK_STYLE_DEFINITIONS_METADATA`.
struct Definition {
    path: &'static [&'static str],
    kind: Kind,
    /// `css_vars`: preset type and variable pattern (`$slug` replaced).
    css_vars: &'static [(&'static str, &'static str)],
    /// `classnames`: the class, and the preset type whose slug fills
    /// `$slug`, or `None` for a class that is always added.
    classnames: &'static [(&'static str, Option<&'static str>)],
}

/// A [`Definition`]; a macro, so the table needs no function at run time.
macro_rules! def {
    ($path:expr, $kind:expr, $css_vars:expr, $classnames:expr $(,)?) => {
        Definition {
            path: $path,
            kind: $kind,
            css_vars: $css_vars,
            classnames: $classnames,
        }
    };
}

use Kind::{BorderSide, Plain, UrlOrValue};

const COLOR_VAR: &[(&str, &str)] = &[("color", "--wp--preset--color--$slug")];
const GRADIENT_VAR: &[(&str, &str)] = &[("gradient", "--wp--preset--gradient--$slug")];
const DIMENSION_VAR: &[(&str, &str)] = &[("dimension", "--wp--preset--dimension--$slug")];
const SPACING_VAR: &[(&str, &str)] = &[("spacing", "--wp--preset--spacing--$slug")];

/// The definitions, in WordPress's order: groups, then entries.
const DEFINITIONS: &[(&str, &[Definition])] = &[
    (
        "background",
        &[
            def!(
                &["background", "backgroundImage"],
                UrlOrValue("background-image"),
                &[],
                &[],
            ),
            def!(
                &["background", "backgroundPosition"],
                Plain("background-position", None),
                &[],
                &[],
            ),
            def!(
                &["background", "backgroundRepeat"],
                Plain("background-repeat", None),
                &[],
                &[],
            ),
            def!(
                &["background", "backgroundSize"],
                Plain("background-size", None),
                &[],
                &[],
            ),
            def!(
                &["background", "backgroundAttachment"],
                Plain("background-attachment", None),
                &[],
                &[],
            ),
            def!(
                &["background", "gradient"],
                Plain("background-image", None),
                GRADIENT_VAR,
                &[("has-background", None)],
            ),
        ],
    ),
    (
        "color",
        &[
            def!(
                &["color", "text"],
                Plain("color", None),
                COLOR_VAR,
                &[("has-text-color", None), ("has-$slug-color", Some("color"))],
            ),
            def!(
                &["color", "background"],
                Plain("background-color", None),
                COLOR_VAR,
                &[
                    ("has-background", None),
                    ("has-$slug-background-color", Some("color")),
                ],
            ),
            def!(
                &["color", "gradient"],
                Plain("background", None),
                GRADIENT_VAR,
                &[
                    ("has-background", None),
                    ("has-$slug-gradient-background", Some("gradient")),
                ],
            ),
        ],
    ),
    (
        "border",
        &[
            def!(
                &["border", "color"],
                Plain("border-color", Some("border-%s-color")),
                &[],
                &[
                    ("has-border-color", None),
                    ("has-$slug-border-color", Some("color")),
                ],
            ),
            def!(
                &["border", "radius"],
                Plain("border-radius", Some("border-%s-radius")),
                &[("border-radius", "--wp--preset--border-radius--$slug")],
                &[],
            ),
            def!(
                &["border", "style"],
                Plain("border-style", Some("border-%s-style")),
                &[],
                &[],
            ),
            def!(
                &["border", "width"],
                Plain("border-width", Some("border-%s-width")),
                &[],
                &[],
            ),
            def!(&["border", "top"], BorderSide("top"), COLOR_VAR, &[]),
            def!(&["border", "right"], BorderSide("right"), COLOR_VAR, &[]),
            def!(&["border", "bottom"], BorderSide("bottom"), COLOR_VAR, &[]),
            def!(&["border", "left"], BorderSide("left"), COLOR_VAR, &[]),
        ],
    ),
    (
        "shadow",
        &[def!(
            &["shadow"],
            Plain("box-shadow", None),
            &[("shadow", "--wp--preset--shadow--$slug")],
            &[],
        )],
    ),
    (
        "dimensions",
        &[
            def!(
                &["dimensions", "aspectRatio"],
                Plain("aspect-ratio", None),
                &[],
                &[("has-aspect-ratio", None)],
            ),
            def!(
                &["dimensions", "height"],
                Plain("height", None),
                DIMENSION_VAR,
                &[],
            ),
            def!(
                &["dimensions", "minHeight"],
                Plain("min-height", None),
                DIMENSION_VAR,
                &[],
            ),
            def!(
                &["dimensions", "minWidth"],
                Plain("min-width", None),
                DIMENSION_VAR,
                &[],
            ),
            def!(
                &["dimensions", "objectFit"],
                Plain("object-fit", None),
                &[],
                &[],
            ),
            def!(
                &["dimensions", "width"],
                Plain("width", None),
                DIMENSION_VAR,
                &[],
            ),
        ],
    ),
    (
        "spacing",
        &[
            def!(
                &["spacing", "padding"],
                Plain("padding", Some("padding-%s")),
                SPACING_VAR,
                &[],
            ),
            def!(
                &["spacing", "margin"],
                Plain("margin", Some("margin-%s")),
                SPACING_VAR,
                &[],
            ),
        ],
    ),
    (
        "typography",
        &[
            def!(
                &["typography", "fontSize"],
                Plain("font-size", None),
                &[("font-size", "--wp--preset--font-size--$slug")],
                &[("has-$slug-font-size", Some("font-size"))],
            ),
            def!(
                &["typography", "fontFamily"],
                Plain("font-family", None),
                &[("font-family", "--wp--preset--font-family--$slug")],
                &[("has-$slug-font-family", Some("font-family"))],
            ),
            def!(
                &["typography", "fontStyle"],
                Plain("font-style", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "fontWeight"],
                Plain("font-weight", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "lineHeight"],
                Plain("line-height", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "textColumns"],
                Plain("column-count", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "textDecoration"],
                Plain("text-decoration", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "textIndent"],
                Plain("text-indent", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "textTransform"],
                Plain("text-transform", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "letterSpacing"],
                Plain("letter-spacing", None),
                &[],
                &[],
            ),
            def!(
                &["typography", "writingMode"],
                Plain("writing-mode", None),
                &[],
                &[],
            ),
        ],
    ),
];

/// What `parse_block_styles()` returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Parsed {
    /// Class names, in order, duplicates included as PHP collects them.
    pub classnames: Vec<String>,
    /// CSS properties and their values, in insertion order.
    pub declarations: Vec<(String, Value)>,
}

impl Parsed {
    /// `$declarations[$property] = $value`: an existing property keeps its
    /// place.
    fn set(&mut self, property: String, value: Value) {
        match self.declarations.iter_mut().find(|(p, _)| *p == property) {
            Some(entry) => entry.1 = value,
            None => self.declarations.push((property, value)),
        }
    }
}

/// `WP_Style_Engine::parse_block_styles()`. With
/// `convert_vars_to_classnames`, preset values give classes instead of CSS
/// variables, as colors and typography supports ask.
pub fn parse_block_styles(styles: &Value, convert_vars_to_classnames: bool) -> Parsed {
    let mut parsed = Parsed::default();
    for (group, definitions) in DEFINITIONS {
        if styles.get_path(&[group]).is_none_or(Value::is_empty) {
            continue;
        }
        for definition in *definitions {
            let Some(value) = styles.get_path(definition.path).filter(|v| is_valid(v)) else {
                continue;
            };
            parsed.classnames.extend(classnames(value, definition));
            for (property, css) in declarations(value, definition, convert_vars_to_classnames) {
                // Two background images stack, the new one first.
                let css = match (&css, existing(&parsed, &property)) {
                    (Value::String(new), Some(Value::String(old)))
                        if property == "background-image" =>
                    {
                        Value::String([new.as_slice(), b", ", old].concat())
                    }
                    _ => css,
                };
                parsed.set(property, css);
            }
        }
    }
    parsed
}

fn existing<'p>(parsed: &'p Parsed, property: &str) -> Option<&'p Value> {
    parsed
        .declarations
        .iter()
        .find(|(p, _)| p == property)
        .map(|(_, v)| v)
}

/// `is_valid_style_value()`: `'0' === $value || ! empty( $value )`.
fn is_valid(value: &Value) -> bool {
    matches!(value, Value::String(text) if text == b"0") || !value.is_empty()
}

fn text(value: &Value) -> Option<&str> {
    match value {
        Value::String(bytes) => std::str::from_utf8(bytes).ok(),
        _ => None,
    }
}

/// The kebab-cased slug of a `var:preset|<property_key>|<slug>` value.
pub(crate) fn preset_value_slug(value: &Value, property_key: &str) -> Option<String> {
    let value = text(value).filter(|v| v.contains(&format!("var:preset|{property_key}|")))?;
    Some(kebab_case(value.rsplit('|').next().unwrap_or_default()))
}

/// `get_slug_from_preset_value()`: the slug, or empty.
fn preset_slug(value: &Value, property_key: &str) -> String {
    preset_value_slug(value, property_key).unwrap_or_default()
}

/// `get_css_var_value()`.
fn css_var(value: &Value, css_vars: &[(&str, &str)]) -> Option<String> {
    css_vars.iter().find_map(|(property_key, pattern)| {
        let slug = preset_slug(value, property_key);
        (slug == "0" || !slug.is_empty())
            .then(|| format!("var({})", pattern.replace("$slug", &slug)))
    })
}

fn classnames(value: &Value, definition: &Definition) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    definition
        .classnames
        .iter()
        .filter_map(|(class, preset)| match preset {
            None => Some((*class).to_string()),
            Some(property_key) => {
                let slug = preset_slug(value, property_key);
                (!slug.is_empty()).then(|| class.replace("$slug", &slug))
            }
        })
        .collect()
}

/// `get_css_declarations()`.
fn declarations(value: &Value, definition: &Definition, skip_vars: bool) -> Vec<(String, Value)> {
    let preset = |v: &Value| -> Option<Value> {
        match text(v) {
            Some(s) if s.contains("var:") && !skip_vars && !definition.css_vars.is_empty() => Some(
                css_var(v, definition.css_vars).map_or(Value::String(Vec::new()), |var| {
                    Value::String(var.into_bytes())
                }),
            ),
            _ => None,
        }
    };
    match definition.kind {
        UrlOrValue(property) => {
            let url = value.get_str("url").filter(|u| !u.is_empty());
            let css = match (url, value) {
                (Some(url), _) => Some(Value::String(
                    [b"url('".as_slice(), &url.to_php_string(), b"')"].concat(),
                )),
                (None, Value::String(s)) => Some(Value::String(s.clone())),
                (None, _) => None,
            };
            css.map(|css| vec![(property.to_string(), css)])
                .unwrap_or_default()
        }
        BorderSide(side) => {
            let Value::Array(entries) = value else {
                return Vec::new();
            };
            entries
                .iter()
                .filter(|(_, v)| !v.is_empty())
                .filter_map(|(key, v)| {
                    // Only named entries can match a border definition.
                    let Key::String(name) = key else { return None };
                    let name = String::from_utf8_lossy(name);
                    let pattern = DEFINITIONS
                        .iter()
                        .find(|(group, _)| *group == "border")
                        .and_then(|(_, defs)| {
                            defs.iter().find(|d| d.path.get(1) == Some(&name.as_ref()))
                        })
                        .and_then(|d| match d.kind {
                            Plain(_, individual) => individual,
                            _ => None,
                        })?;
                    let css = preset(v).unwrap_or_else(|| v.clone());
                    Some((pattern.replace("%s", side), css))
                })
                .collect()
        }
        Plain(property, individual) => {
            if text(value).is_some_and(|s| s.contains("var:")) {
                return match preset(value) {
                    Some(css) if is_valid(&css) => vec![(property.to_string(), css)],
                    _ => Vec::new(),
                };
            }
            match value {
                Value::Array(entries) => {
                    let Some(pattern) = individual else {
                        return Vec::new();
                    };
                    entries
                        .iter()
                        .filter_map(|(key, v)| {
                            let name = match key {
                                Key::String(name) => String::from_utf8_lossy(name).into_owned(),
                                Key::Int(n) => n.to_string(),
                            };
                            let css = preset(v).unwrap_or_else(|| v.clone());
                            is_valid(&css).then(|| (pattern.replace("%s", &kebab_case(&name)), css))
                        })
                        .collect()
                }
                _ => vec![(property.to_string(), value.clone())],
            }
        }
    }
}

/// What `wp_style_engine_get_styles()` returns without a selector or
/// context; each part is empty where PHP leaves its key out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Styles {
    /// The declarations as one `style` attribute value.
    pub css: Vec<u8>,
    /// The class names, without duplicates, joined by spaces.
    pub classnames: String,
}

/// `wp_style_engine_get_styles()`.
pub fn get_styles(styles: &Value, convert_vars_to_classnames: bool) -> Styles {
    let parsed = parse_block_styles(styles, convert_vars_to_classnames);
    let mut classnames: Vec<&str> = Vec::new();
    for class in &parsed.classnames {
        if !classnames.contains(&class.as_str()) {
            classnames.push(class);
        }
    }
    Styles {
        css: declarations_string(&parsed.declarations),
        classnames: classnames.join(" "),
    }
}

/// `WP_Style_Engine_CSS_Declarations`: `add_declarations()` keeps string
/// values that are not blank, under their `sanitize_key()`ed property, and
/// `get_declarations_string()` joins them, each through
/// `wp_strip_all_tags()` and `safecss_filter_attr()`.
pub(crate) fn declarations_string(declarations: &[(String, Value)]) -> Vec<u8> {
    let mut kept: Vec<(String, &[u8])> = Vec::new();
    for (property, value) in declarations {
        let property = sanitize_key(property.as_bytes());
        let Value::String(value) = value else {
            continue;
        };
        let value = php_trim(value);
        // Properties come from the definitions, so none sanitizes to empty.
        if value.is_empty() {
            continue;
        }
        match kept.iter_mut().find(|(p, _)| *p == property) {
            Some(entry) => entry.1 = value,
            None => kept.push((property, value)),
        }
    }
    let mut css = Vec::new();
    for (property, value) in kept {
        let value = wp_strip_all_tags(value, true);
        if value.is_empty() {
            continue;
        }
        let declaration = safecss_filter_attr(&[property.as_bytes(), b":", &value].concat());
        if !declaration.is_empty() {
            css.extend_from_slice(&declaration);
            css.push(b';');
        }
    }
    css
}

#[cfg(test)]
mod tests;
