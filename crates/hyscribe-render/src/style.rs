// SPDX-License-Identifier: AGPL-3.0-or-later

//! The style engine's parsing step, `WP_Style_Engine::parse_block_styles()`:
//! a block's `style` attribute to class names and CSS declarations, by the
//! same definitions table. Compiling the declarations to a filtered CSS
//! string needs kses and joins once `hyscribe-kses` lands.

use hyscribe_phpser::{Key, Value};

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
        if php_empty(styles.get_path(&[group])) {
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

/// PHP's `empty()`.
fn php_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false) | Value::Int(0)) => true,
        Some(Value::String(text)) => text.is_empty() || text == b"0",
        Some(Value::Float(f)) => f.to_f64() == 0.0,
        Some(Value::Array(entries)) => entries.is_empty(),
        Some(_) => false,
    }
}

/// `is_valid_style_value()`: `'0' === $value || ! empty( $value )`.
fn is_valid(value: &Value) -> bool {
    matches!(value, Value::String(text) if text == b"0") || !php_empty(Some(value))
}

fn text(value: &Value) -> Option<&str> {
    match value {
        Value::String(bytes) => std::str::from_utf8(bytes).ok(),
        _ => None,
    }
}

/// PHP's string cast of a scalar, as string concatenation does it.
fn php_string(value: &Value) -> Vec<u8> {
    match value {
        Value::String(text) => text.clone(),
        Value::Int(n) => n.to_string().into_bytes(),
        Value::Bool(true) => b"1".to_vec(),
        Value::Float(f) => f.as_str().as_bytes().to_vec(),
        Value::Array(_) => b"Array".to_vec(),
        _ => Vec::new(),
    }
}

/// `get_slug_from_preset_value()`: the kebab-cased slug of
/// `var:preset|<property_key>|<slug>`, or empty.
fn preset_slug(value: &Value, property_key: &str) -> String {
    let Some(value) = text(value).filter(|v| v.contains(&format!("var:preset|{property_key}|")))
    else {
        return String::new();
    };
    let slug = value.rsplit('|').next().unwrap_or_default();
    kebab_case(slug)
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
    if php_empty(Some(value)) {
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
            let url = value.get_str("url").filter(|u| !php_empty(Some(u)));
            let css = match (url, value) {
                (Some(url), _) => Some(Value::String(
                    [b"url('".as_slice(), &php_string(url), b"')"].concat(),
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
                .filter(|(_, v)| !php_empty(Some(v)))
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

/// `_wp_to_kebab_case()`: lodash's word splitting, joined by `-`, then
/// PHP's ASCII-only `strtolower()`.
pub fn kebab_case(input: &str) -> String {
    let chars: Vec<char> = input.chars().filter(|&c| c != '\'').collect();
    let mut words = Vec::new();
    let mut i = 0;
    for _ in 0..=chars.len() {
        if i >= chars.len() {
            break;
        }
        match word_at(&chars, i) {
            Some(len) => {
                words.push(
                    chars
                        .get(i..i + len)
                        .unwrap_or_default()
                        .iter()
                        .collect::<String>(),
                );
                i += len;
            }
            None => i += 1,
        }
    }
    words.join("-").to_ascii_lowercase()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Lower,
    Upper,
    Digit,
    Break,
    Misc,
}

fn class(c: char) -> Class {
    let code = u32::from(c);
    let is_space = matches!(
        code,
        0x20 | 0x09
            | 0x0b
            | 0x0c
            | 0xa0
            | 0xfeff
            | 0x0a
            | 0x0d
            | 0x2028
            | 0x2029
            | 0x1680
            | 0x180e
            | 0x2000..=0x200a | 0x202f | 0x205f | 0x3000
    );
    let is_non_char = matches!(code, 0x00..=0x2f | 0x3a..=0x40 | 0x5b..=0x60 | 0x7b..=0xbf);
    if is_space || is_non_char || (0x2000..=0x206f).contains(&code) {
        Class::Break
    } else if c.is_numeric() {
        // PHP's /u enables Unicode digits for \d.
        Class::Digit
    } else if c.is_ascii_lowercase() || matches!(code, 0xdf..=0xf6 | 0xf8..=0xff) {
        Class::Lower
    } else if c.is_ascii_uppercase() || matches!(code, 0xc0..=0xd6 | 0xd8..=0xde) {
        Class::Upper
    } else {
        Class::Misc
    }
}

/// The length of the first alternative of lodash's word regex that matches
/// at `i`, or `None`.
fn word_at(chars: &[char], i: usize) -> Option<usize> {
    let at = |j: usize| chars.get(j).copied().map(class);
    let run = |from: usize, keep: &dyn Fn(Class) -> bool| {
        (from..chars.len())
            .take_while(|&j| at(j).is_some_and(keep))
            .count()
    };
    let misc_lower = |c: Class| c == Class::Lower || c == Class::Misc;
    let misc_upper = |c: Class| c == Class::Upper || c == Class::Misc;
    let upper_at = |j: usize| at(j) == Some(Class::Upper);
    // Lodash's first alternative, Upper? Lower+ (?=Break|Upper|$), always
    // yields the same match as the third, so only the others are needed.
    // 2. MiscUpper+ (?=Break|Upper MiscLower|$), backtracking to shorter runs.
    let longest = run(i, &misc_upper);
    for len in (1..=longest).rev() {
        let next = i + len;
        let ok = matches!(at(next), None | Some(Class::Break))
            || (upper_at(next) && at(next + 1).is_some_and(misc_lower));
        if ok {
            return Some(len);
        }
    }
    // 3. Upper? MiscLower+
    for start in [i + usize::from(upper_at(i)), i] {
        let lower = run(start, &misc_lower);
        if lower > 0 {
            return Some(start - i + lower);
        }
    }
    // 4. Upper+
    let upper = run(i, &|c| c == Class::Upper);
    if upper > 0 {
        return Some(upper);
    }
    // 5, 6. Ordinals, then 7. digits.
    ordinal(chars, i).or_else(|| Some(run(i, &|c| c == Class::Digit)).filter(|&n| n > 0))
}

/// `\d*(?:1st|2nd|3rd|(?![123])\dth)(?=\b|[A-Z_])` and the upper case form.
fn ordinal(chars: &[char], i: usize) -> Option<usize> {
    let is_digit = |j: usize| chars.get(j).is_some_and(|c| class(*c) == Class::Digit);
    let digits = (i..chars.len()).take_while(|&j| is_digit(j)).count();
    // Backtrack: the last digit belongs to the ordinal.
    (1..=digits).rev().find_map(|n| {
        let last = i + n - 1;
        let Some(&[digit, first, second]) = chars.get(last..last + 3) else {
            return None;
        };
        let suffix: String = [first, second].iter().collect();
        for (upper, after_ok) in [(false, true), (true, false)] {
            let expected = match (digit, upper) {
                ('1', false) => "st",
                ('2', false) => "nd",
                ('3', false) => "rd",
                ('1', true) => "ST",
                ('2', true) => "ND",
                ('3', true) => "RD",
                (_, false) => "th",
                (_, true) => "TH",
            };
            if suffix != expected {
                continue;
            }
            let end = last + 3;
            let next = chars.get(end).copied();
            let word = |c: char| c.is_alphanumeric() || c == '_';
            let boundary = next.is_none_or(|c| !word(c));
            let follows = next.is_some_and(|c| {
                if after_ok {
                    c.is_ascii_uppercase() || c == '_'
                } else {
                    c.is_ascii_lowercase() || c == '_'
                }
            });
            if boundary || follows {
                return Some(end - i);
            }
        }
        None
    })
}

#[cfg(test)]
mod tests;
