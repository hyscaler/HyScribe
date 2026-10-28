// SPDX-License-Identifier: AGPL-3.0-or-later

//! The layout block support: `wp_render_layout_support_flag()`, its
//! container and child styles, and the style engine's stylesheet for them.
//!
//! Not ported yet: viewport overrides (`style.<breakpoint>`, which
//! WordPress 7 reads from `settings.viewport`) and block gaps set by block
//! style variations.

use hyscribe_html::TagProcessor;
use hyscribe_phpser::{Key, Value, is_empty_string, json_encode, md5};
use hyscribe_theme_json::{GlobalStyles, kebab_case};

use crate::formatting::sanitize_title;
use crate::registry::BlockType;
use crate::style::{declarations_string, parse_block_styles};

/// A CSS rule as the style engine takes it.
pub(crate) struct Rule {
    selector: String,
    declarations: Vec<(String, Value)>,
    /// An at-rule the rule sits in, such as a container query.
    group: Option<String>,
}

fn rule(selector: String, declarations: &[(&str, Value)]) -> Rule {
    Rule {
        selector,
        declarations: declarations
            .iter()
            .map(|(p, v)| (p.to_string(), v.clone()))
            .collect(),
        group: None,
    }
}

/// `explode( $delimiter, $text )[1]`: the text between the first two
/// occurrences of `delimiter`, or after the first; `None` without one.
fn second_part<'a>(text: &'a [u8], delimiter: &[u8]) -> Option<&'a [u8]> {
    let find = |text: &[u8]| text.windows(delimiter.len()).position(|w| w == delimiter);
    let after = text
        .get(find(text)? + delimiter.len()..)
        .unwrap_or_default();
    Some(
        after
            .get(..find(after).unwrap_or(after.len()))
            .unwrap_or_default(),
    )
}

fn string(text: impl Into<Vec<u8>>) -> Value {
    Value::String(text.into())
}

/// A rule's selector and group, with the declarations merged into it.
type Merged<'a> = (&'a str, Option<&'a str>, Vec<(String, Value)>);

/// `wp_style_engine_get_stylesheet_from_css_rules( $rules )` without
/// prettifying: rules with the same selector and group merge, each
/// declaration goes through kses, and grouped rules follow inside their
/// at-rule.
pub(crate) fn stylesheet(rules: &[Rule]) -> String {
    let mut merged: Vec<Merged<'_>> = Vec::new();
    for rule in rules {
        let key = (rule.selector.as_str(), rule.group.as_deref());
        match merged.iter_mut().find(|(s, g, _)| (*s, *g) == key) {
            Some((_, _, declarations)) => {
                for (property, value) in &rule.declarations {
                    match declarations.iter_mut().find(|(p, _)| p == property) {
                        Some(existing) => existing.1 = value.clone(),
                        None => declarations.push((property.clone(), value.clone())),
                    }
                }
            }
            None => merged.push((key.0, key.1, rule.declarations.clone())),
        }
    }
    let mut css = String::new();
    for (selector, group, declarations) in merged {
        let declarations =
            String::from_utf8_lossy(&declarations_string(&declarations)).into_owned();
        if declarations.is_empty() {
            continue;
        }
        match group {
            Some(group) => css.push_str(&format!("{group}{{{selector}{{{declarations}}}}}")),
            None => css.push_str(&format!("{selector}{{{declarations}}}")),
        }
    }
    css
}

/// `wp_unique_id_from_values( $data, $prefix )`.
fn unique_id(data: &Value, prefix: &str) -> String {
    let hash = md5(&json_encode(data).unwrap_or_default());
    format!("{prefix}{}", hash.get(..8).unwrap_or_default())
}

/// `block_has_support( $block_type, $feature, false )`.
fn has_support(block_type: &BlockType, feature: &[&str]) -> bool {
    matches!(
        block_type.supports.get_path(feature),
        Some(Value::Bool(true) | Value::Array(_))
    )
}

const CHILD_KEYS: [&str; 6] = [
    "selfStretch",
    "flexSize",
    "columnStart",
    "columnSpan",
    "rowStart",
    "rowSpan",
];

/// `wp_get_layout_child_values( $layout )`.
fn child_values(layout: Option<&Value>) -> Vec<(Key, Value)> {
    layout
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|(key, _)| matches!(key, Key::String(name) if CHILD_KEYS.iter().any(|k| k.as_bytes() == name)))
        .cloned()
        .collect()
}

/// `wp_sanitize_block_gap_value( $gap )`: values that could break out of a
/// declaration (`\`, `(`, `&`, `=`, `}` or a comment) become null.
fn sanitize_gap(gap: Option<&Value>) -> Value {
    let unsafe_value = |value: &Value| {
        let text = value.to_php_string();
        !value.is_empty()
            && (text.iter().any(|b| b"\\(&=}".contains(b)) || text.windows(2).any(|w| w == b"/*"))
    };
    match gap {
        None | Some(Value::Null) => Value::Null,
        Some(Value::Array(sides)) => Value::Array(
            sides
                .iter()
                .map(|(key, value)| {
                    let scalar = matches!(
                        value,
                        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
                    );
                    (
                        key.clone(),
                        if !scalar || unsafe_value(value) {
                            Value::Null
                        } else {
                            value.clone()
                        },
                    )
                })
                .collect(),
        ),
        Some(value) if unsafe_value(value) => Value::Null,
        Some(value) => value.clone(),
    }
}

/// A `var:preset|spacing|<slug>` gap as its CSS variable.
fn gap_css(value: &Value) -> Value {
    let text = value.to_php_string();
    match (
        value,
        text.windows(19).position(|w| w == b"var:preset|spacing|"),
    ) {
        (Value::String(_), Some(_)) => {
            let slug = text.rsplit(|&b| b == b'|').next().unwrap_or_default();
            string(format!(
                "var(--wp--preset--spacing--{})",
                kebab_case(&String::from_utf8_lossy(slug))
            ))
        }
        _ => value.clone(),
    }
}

/// The gap of a flex or grid layout: one value, or `top left` from an
/// array, sides missing from it taking the fallback's.
fn combined_gap(gap: &Value, fallback: &Value) -> String {
    let sides: &[&str] = if matches!(gap, Value::Array(_)) {
        &["top", "left"]
    } else {
        &["top"]
    };
    let mut combined = String::new();
    for side in sides {
        let value = match gap {
            Value::Array(_) => gap
                .get_str(side)
                .filter(|v| **v != Value::Null)
                .cloned()
                .unwrap_or_else(|| match fallback {
                    Value::Array(entries) => fallback.get_str(side).cloned().unwrap_or_else(|| {
                        entries.first().map(|(_, v)| v.clone()).unwrap_or_default()
                    }),
                    other => other.clone(),
                }),
            other => other.clone(),
        };
        combined.push_str(&String::from_utf8_lossy(&gap_css(&value).to_php_string()));
        combined.push(' ');
    }
    combined.trim().to_string()
}

/// What `wp_get_layout_style()` reads besides the selector and layout.
struct Gap<'a> {
    supported: bool,
    value: &'a Value,
    skip: bool,
    fallback: &'a Value,
    spacing: Option<&'a Value>,
}

/// `wp_get_layout_style()` without viewport overrides: the container's
/// rules for its layout type.
fn layout_style(selector: &str, layout: &Value, gap: &Gap<'_>) -> Vec<Rule> {
    let field = |name: &str| layout.get_str(name).filter(|v| **v != Value::Null);
    let text = |name: &str| field(name).map(Value::to_php_string).unwrap_or_default();
    let filled = |name: &str| field(name).is_some_and(|v| !v.is_empty());
    let mut rules = Vec::new();
    let kind = field("type").map_or(b"default".to_vec(), Value::to_php_string);
    let flow_gap = |rules: &mut Vec<Rule>| {
        let value = match gap.value {
            Value::Array(_) => gap.value.get_str("top").cloned().unwrap_or_default(),
            other => other.clone(),
        };
        if gap.supported && value != Value::Null && !gap.skip {
            let value = gap_css(&value);
            rules.push(rule(
                format!("{selector} > *"),
                &[
                    ("margin-block-start", string("0")),
                    ("margin-block-end", string("0")),
                ],
            ));
            rules.push(rule(
                format!("{selector} > * + *"),
                &[
                    ("margin-block-start", value),
                    ("margin-block-end", string("0")),
                ],
            ));
        }
    };
    match kind.as_slice() {
        b"default" => flow_gap(&mut rules),
        b"constrained" => {
            let (content, wide) = (text("contentSize"), text("wideSize"));
            let justify = field("justifyContent").map_or(b"center".to_vec(), Value::to_php_string);
            let width = |own: &[u8], other: &[u8], global: &str| -> Value {
                let value = if !is_empty_string(own) {
                    own.to_vec()
                } else if !is_empty_string(other) {
                    other.to_vec()
                } else {
                    format!("var(--wp--style--global--{global}-size, none)").into_bytes()
                };
                let first = value.split(|&b| b == b';').next().unwrap_or_default();
                string(hyscribe_kses::safecss_filter_attr(first))
            };
            let all_width = width(&content, &wide, "content");
            let wide_width = width(&wide, &content, "wide");
            let left = if justify == b"left" {
                "0 !important"
            } else {
                "auto !important"
            };
            let right = if justify == b"right" {
                "0 !important"
            } else {
                "auto !important"
            };
            let main =
                format!("{selector} > :where(:not(.alignleft):not(.alignright):not(.alignfull))");
            if !is_empty_string(&content) || !is_empty_string(&wide) {
                rules.push(rule(
                    main.clone(),
                    &[
                        ("max-width", all_width),
                        ("margin-left", string(left)),
                        ("margin-right", string(right)),
                    ],
                ));
                rules.push(rule(
                    format!("{selector} > .alignwide"),
                    &[("max-width", wide_width)],
                ));
                rules.push(rule(
                    format!("{selector} .alignfull"),
                    &[("max-width", string("none"))],
                ));
            }
            if let Some(spacing) = gap.spacing {
                let declarations = parse_block_styles(
                    &Value::Array(vec![(Key::String(b"spacing".to_vec()), spacing.clone())]),
                    false,
                )
                .declarations;
                for (side, property) in [("right", "margin-right"), ("left", "margin-left")] {
                    let padding = declarations
                        .iter()
                        .find(|(p, _)| *p == format!("padding-{side}"))
                        .map(|(_, v)| v.to_php_string());
                    if let Some(padding) = padding {
                        let padding = if padding == b"0" {
                            b"0px".to_vec()
                        } else {
                            padding
                        };
                        let value =
                            string(format!("calc({} * -1)", String::from_utf8_lossy(&padding)));
                        rules.push(rule(
                            format!("{selector} > .alignfull"),
                            &[(property, value)],
                        ));
                    }
                }
            }
            match justify.as_slice() {
                b"left" => rules.push(rule(main, &[("margin-left", string("0 !important"))])),
                b"right" => rules.push(rule(main, &[("margin-right", string("0 !important"))])),
                _ => {}
            }
            flow_gap(&mut rules);
        }
        b"flex" => {
            let vertical = field("orientation").is_some_and(|v| v.to_php_string() != b"horizontal");
            let justify = text("justifyContent");
            let align = text("verticalAlignment");
            if text("flexWrap") == b"nowrap" {
                rules.push(rule(selector.into(), &[("flex-wrap", string("nowrap"))]));
            }
            if gap.supported && *gap.value != Value::Null && !gap.skip {
                rules.push(rule(
                    selector.into(),
                    &[("gap", string(combined_gap(gap.value, gap.fallback)))],
                ));
            }
            let justify_value: Option<&str> = match (justify.as_slice(), vertical) {
                (b"left", _) => Some("flex-start"),
                (b"right", _) => Some("flex-end"),
                (b"center", _) => Some("center"),
                (b"space-between", false) | (b"stretch", true) => {
                    Some(if vertical { "stretch" } else { "space-between" })
                }
                _ => None,
            };
            let align_value: Option<&str> = match (align.as_slice(), vertical) {
                (b"top", _) => Some("flex-start"),
                (b"center", _) => Some("center"),
                (b"bottom", _) => Some("flex-end"),
                (b"stretch", false) | (b"space-between", true) => {
                    Some(if vertical { "space-between" } else { "stretch" })
                }
                _ => None,
            };
            if vertical {
                rules.push(rule(
                    selector.into(),
                    &[("flex-direction", string("column"))],
                ));
                rules.push(rule(
                    selector.into(),
                    &[("align-items", string(justify_value.unwrap_or("flex-start")))],
                ));
                if let Some(value) = align_value {
                    rules.push(rule(selector.into(), &[("justify-content", string(value))]));
                }
            } else {
                if let Some(value) = justify_value {
                    rules.push(rule(selector.into(), &[("justify-content", string(value))]));
                }
                if let Some(value) = align_value {
                    rules.push(rule(selector.into(), &[("align-items", string(value))]));
                }
            }
        }
        b"grid" => {
            let mut responsive_gap = match gap.fallback {
                Value::Array(entries) => {
                    gap.fallback.get_str("left").cloned().unwrap_or_else(|| {
                        entries.first().map(|(_, v)| v.clone()).unwrap_or_default()
                    })
                }
                other => other.clone(),
            };
            let mut gap_value = gap.value.clone();
            if gap.supported && *gap.value != Value::Null {
                gap_value = string(combined_gap(gap.value, gap.fallback));
                responsive_gap = gap_value.clone();
            }
            if matches!(&responsive_gap, Value::String(t) if t == b"0")
                || responsive_gap == Value::Int(0)
            {
                responsive_gap = string("0px");
            }
            let responsive_gap =
                String::from_utf8_lossy(&responsive_gap.to_php_string()).into_owned();
            let auto = if filled("autoFit") {
                "auto-fit"
            } else {
                "auto-fill"
            };
            let count = String::from_utf8_lossy(&text("columnCount")).into_owned();
            let minimum = String::from_utf8_lossy(&text("minimumColumnWidth")).into_owned();
            let mut declarations: Vec<(&str, Value)> = Vec::new();
            let columns = match (filled("columnCount"), filled("minimumColumnWidth")) {
                (true, true) => format!(
                    "repeat({auto}, minmax(max(min({minimum}, 100%), (100% - ({responsive_gap} * ({count} - 1))) /{count}), 1fr))"
                ),
                (true, false) => format!("repeat({count}, minmax(0, 1fr))"),
                (false, minimum_set) => {
                    let minimum = if minimum_set {
                        minimum.as_str()
                    } else {
                        "12rem"
                    };
                    format!("repeat({auto}, minmax(min({minimum}, 100%), 1fr))")
                }
            };
            declarations.push(("grid-template-columns", string(columns)));
            if !filled("columnCount") || filled("minimumColumnWidth") {
                declarations.push(("container-type", string("inline-size")));
            }
            rules.push(rule(selector.into(), &declarations));
            if filled("columnCount") && filled("rowCount") {
                let rows = format!(
                    "repeat({}, minmax(1rem, auto))",
                    String::from_utf8_lossy(&text("rowCount"))
                );
                rules.push(rule(
                    selector.into(),
                    &[("grid-template-rows", string(rows))],
                ));
            }
            if gap.supported && gap_value != Value::Null && !gap.skip {
                rules.push(rule(selector.into(), &[("gap", gap_value)]));
            }
        }
        _ => {}
    }
    rules
}

/// `wp_get_child_layout_style_rules()` without viewport overrides.
fn child_layout_style(selector: &str, child: &[(Key, Value)], parent: Option<&Value>) -> Vec<Rule> {
    let child = Value::Array(child.to_vec());
    let field = |name: &str| {
        child
            .get_str(name)
            .filter(|v| **v != Value::Null && !v.is_empty())
    };
    let text =
        |name: &str| field(name).map(|v| String::from_utf8_lossy(&v.to_php_string()).into_owned());
    let mut declarations: Vec<(&str, Value)> = Vec::new();
    let stretch = child.get_str("selfStretch").map(Value::to_php_string);
    let flex_size = child.get_str("flexSize").filter(|v| **v != Value::Null);
    match (stretch.as_deref(), flex_size) {
        (Some(b"fixed" | b"fixedNoShrink"), Some(size)) => {
            declarations.push(("flex-basis", size.clone()));
            if stretch.as_deref() == Some(b"fixedNoShrink") {
                declarations.push(("flex-shrink", string("0")));
            }
            declarations.push(("box-sizing", string("border-box")));
        }
        (Some(b"fill"), _) => declarations.push(("flex-grow", string("1"))),
        _ => {}
    }
    let place = |start: Option<String>, span: Option<String>| match (start, span) {
        (Some(start), Some(span)) => Some(format!("{start} / span {span}")),
        (Some(start), None) => Some(start),
        (None, Some(span)) => Some(format!("span {span}")),
        (None, None) => None,
    };
    if let Some(column) = place(text("columnStart"), text("columnSpan")) {
        declarations.push(("grid-column", string(column)));
    }
    if let Some(row) = place(text("rowStart"), text("rowSpan")) {
        declarations.push(("grid-row", string(row)));
    }
    let mut rules = Vec::new();
    if !declarations.is_empty() {
        rules.push(rule(selector.into(), &declarations));
    }
    let parent = parent.cloned().unwrap_or_default();
    let minimum = parent
        .get_str("minimumColumnWidth")
        .filter(|v| !v.is_empty());
    let count = parent.get_str("columnCount").filter(|v| !v.is_empty());
    let (span, start) = (field("columnSpan"), field("columnStart"));
    if (span.is_some() || start.is_some()) && (minimum.is_some() || count.is_none()) {
        let number = |v: Option<&Value>| v.map_or(0.0, Value::to_float);
        let (span_number, start_number) = (number(span), number(start));
        let width = minimum.map_or(b"12rem".to_vec(), Value::to_php_string);
        let mut value = Value::String(width.clone()).to_float();
        // `explode( $value, $width )[1]`: what follows the number as PHP
        // prints it, up to its next occurrence.
        let printed = hyscribe_phpser::float_to_string(value);
        let unit = match second_part(&width, printed.as_bytes()) {
            Some(unit) => {
                let unit = String::from_utf8_lossy(unit).into_owned();
                if matches!(unit.as_str(), "px" | "rem" | "em") {
                    unit
                } else {
                    "rem".into()
                }
            }
            None => {
                value = 12.0;
                "rem".into()
            }
        };
        let columns = match (span_number != 0.0, start_number != 0.0) {
            (true, true) => start_number + span_number - 1.0,
            (true, false) => span_number,
            _ => start_number,
        };
        let default_gap = if unit == "px" { 24.0 } else { 1.5 };
        let query =
            (columns * value + (columns - 1.0) * default_gap).max(value * 2.0 + default_gap - 1.0);
        let grid_column = if span.is_some_and(|s| s.to_float() > 1.0) {
            "1/-1"
        } else {
            "auto"
        };
        rules.push(Rule {
            group: Some(format!(
                "@container (max-width: {}{unit} )",
                hyscribe_phpser::float_to_string(query)
            )),
            ..rule(
                selector.into(),
                &[
                    ("grid-column", string(grid_column)),
                    ("grid-row", string("auto")),
                ],
            )
        });
    }
    rules
}

/// The `className` of a layout type in `wp_get_layout_definitions()`.
fn layout_class(kind: Option<&Value>) -> Vec<u8> {
    match kind.map(Value::to_php_string).as_deref() {
        None | Some(b"default") => b"is-layout-flow".to_vec(),
        Some(b"constrained") => b"is-layout-constrained".to_vec(),
        Some(b"flex") => b"is-layout-flex".to_vec(),
        Some(b"grid") => b"is-layout-grid".to_vec(),
        Some(_) => Vec::new(),
    }
}

/// `WP_HTML_Processor::is_void()`.
pub(crate) fn is_void(tag: &[u8]) -> bool {
    const VOID: [&[u8]; 18] = [
        b"AREA",
        b"BASE",
        b"BASEFONT",
        b"BGSOUND",
        b"BR",
        b"COL",
        b"EMBED",
        b"FRAME",
        b"HR",
        b"IMG",
        b"INPUT",
        b"KEYGEN",
        b"LINK",
        b"META",
        b"PARAM",
        b"SOURCE",
        b"TRACK",
        b"WBR",
    ];
    VOID.contains(&tag)
}

/// The class of the element the first chunk of a block's own markup leaves
/// open last: the wrapper its inner blocks go in.
fn inner_wrapper_class(first_chunk: &[u8]) -> Option<Vec<u8>> {
    let mut tags = TagProcessor::new(first_chunk);
    let mut stack: Vec<Option<Vec<u8>>> = Vec::new();
    while tags.next_tag_visiting_closers() {
        if tags.is_tag_closer() {
            stack.pop();
        } else if !is_void(&tags.get_tag().unwrap_or_default()) {
            stack.push(match tags.get_attribute(b"class") {
                Some(hyscribe_html::Attribute::Value(class)) => Some(class),
                _ => None,
            });
        }
    }
    stack
        .into_iter()
        .rev()
        .flatten()
        .find(|class| !class.is_empty())
}

/// One block as the layout support sees it.
pub(crate) struct LayoutBlock<'a> {
    pub(crate) name: &'a str,
    pub(crate) block_type: &'a BlockType,
    /// The attributes as parsed.
    pub(crate) attributes: &'a Value,
    /// The parent block's `layout` attribute, `parentLayout`.
    pub(crate) parent_layout: Option<&'a Value>,
    /// The first chunk of the block's markup, when it has inner blocks.
    pub(crate) first_chunk: Option<&'a [u8]>,
}

/// `wp_render_layout_support_flag()`: the layout classes on the block's
/// wrapper, and the CSS they need for the block supports stylesheet.
pub(crate) fn render_layout(
    block: &LayoutBlock<'_>,
    global: &GlobalStyles,
    content: Vec<u8>,
) -> (Vec<u8>, String) {
    let block_type = block.block_type;
    let supports_layout =
        has_support(block_type, &["layout"]) || has_support(block_type, &["__experimentalLayout"]);
    let style = block
        .attributes
        .get_str("style")
        .cloned()
        .unwrap_or_default();
    let child_layout = style.get_str("layout").filter(|v| !v.is_empty());
    if !supports_layout && child_layout.is_none() {
        return (content, String::new());
    }
    let mut css = String::new();
    let mut outer_classes = Vec::new();
    if let Some(child) = child_layout {
        let base = child_values(Some(child));
        let parent = block
            .parent_layout
            .and_then(Value::as_array)
            .unwrap_or_default();
        let parent_grid: Vec<(Key, Value)> = parent
            .iter()
            .filter(|(key, _)| matches!(key, Key::String(n) if n == b"minimumColumnWidth" || n == b"columnCount"))
            .cloned()
            .collect();
        let input = Value::Array(vec![
            (Key::String(b"layout".to_vec()), Value::Array(base.clone())),
            (
                Key::String(b"parentLayout".to_vec()),
                Value::Array(parent_grid),
            ),
        ]);
        let class = unique_id(&input, "wp-container-content-");
        let child_css = stylesheet(&child_layout_style(
            &format!(".{class}"),
            &base,
            block.parent_layout,
        ));
        if !child_css.is_empty() {
            outer_classes.push(class.into_bytes());
            css.push_str(&child_css);
        }
    }
    let mut tags = TagProcessor::new(&content);
    if !tags.next_tag(None, None) {
        return (content, css);
    }
    if !supports_layout {
        for class in &outer_classes {
            tags.add_class(class);
        }
        return (tags.get_updated_html().to_vec(), css);
    }
    let fallback = block_type
        .supports
        .get_path(&["layout", "default"])
        .or_else(|| {
            block_type
                .supports
                .get_path(&["__experimentalLayout", "default"])
        })
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let mut used = block
        .attributes
        .get_str("layout")
        .filter(|v| **v != Value::Null)
        .cloned()
        .unwrap_or(fallback);
    let layout = block
        .attributes
        .get_str("layout")
        .cloned()
        .unwrap_or_default();
    let truthy = |name: &str| used.get_str(name).is_some_and(|v| !v.is_empty());
    if (truthy("inherit") || truthy("contentSize"))
        && let Value::Array(entries) = &mut used
    {
        match entries
            .iter_mut()
            .find(|(k, _)| *k == Key::String(b"type".to_vec()))
        {
            Some((_, kind)) => *kind = string("constrained"),
            None => entries.push((Key::String(b"type".to_vec()), string("constrained"))),
        }
    }
    let kind = used
        .get_str("type")
        .filter(|v| **v != Value::Null)
        .map(Value::to_php_string);
    let mut classes: Vec<Vec<u8>> = Vec::new();
    if global.use_root_padding_aware_alignments() && kind.as_deref() == Some(b"constrained") {
        classes.push(b"has-global-padding".to_vec());
    }
    let filled = |name: &str| {
        layout
            .get_str(name)
            .filter(|v| !v.is_empty())
            .map(Value::to_php_string)
    };
    if let Some(orientation) = filled("orientation") {
        classes.push([b"is-", &sanitize_title(&orientation)[..]].concat());
    }
    if let Some(justify) = filled("justifyContent") {
        classes.push([b"is-content-justification-", &sanitize_title(&justify)[..]].concat());
    }
    if filled("flexWrap").as_deref() == Some(b"nowrap") {
        classes.push(b"is-nowrap".to_vec());
    }
    let layout_classname = layout_class(used.get_str("type").filter(|v| **v != Value::Null));
    if !layout_classname.is_empty() {
        classes.push(sanitize_title(&layout_classname));
    }
    let gap_value = sanitize_gap(style.get_path(&["spacing", "blockGap"]));
    let fallback_gap = block_type
        .supports
        .get_path(&["spacing", "blockGap", "__experimentalDefault"])
        .cloned()
        .unwrap_or_else(|| string("0.5em"));
    let spacing = style.get_str("spacing").filter(|v| **v != Value::Null);
    let skip = match block_type
        .supports
        .get_path(&["spacing", "__experimentalSkipSerialization"])
    {
        Some(Value::Array(features)) => features.iter().any(|(_, f)| *f == string("blockGap")),
        skip => skip.is_some_and(|v| !v.is_empty()),
    };
    let has_gap_support = global.has_block_gap_support();
    // `??` passes over nulls as well as missing keys.
    let global_gap = global
        .block_style(block.name, &["spacing", "blockGap"])
        .filter(|v| **v != Value::Null)
        .or_else(|| {
            global
                .style(&["spacing", "blockGap"])
                .filter(|v| **v != Value::Null)
        });
    let fallback_gap = global_gap.cloned().unwrap_or(fallback_gap);
    let input = Value::Array(
        [
            used.clone(),
            Value::Bool(has_gap_support),
            gap_value.clone(),
            Value::Bool(skip),
            fallback_gap.clone(),
            spacing.cloned().unwrap_or_default(),
        ]
        .into_iter()
        .zip(0..)
        .map(|(value, i)| (Key::Int(i), value))
        .collect(),
    );
    let container = unique_id(
        &input,
        &format!(
            "wp-container-{}-is-layout-",
            String::from_utf8_lossy(&sanitize_title(block.name.as_bytes()))
        ),
    );
    let gap = Gap {
        supported: has_gap_support,
        value: &gap_value,
        skip,
        fallback: &fallback_gap,
        spacing,
    };
    let container_css = stylesheet(&layout_style(&format!(".{container}"), &used, &gap));
    if !container_css.is_empty() {
        classes.push(container.into_bytes());
        css.push_str(&container_css);
    }
    let short_name = match block.name.split_once('/') {
        Some(("core", name)) => name.to_string(),
        _ => block.name.replace('/', "-"),
    };
    classes.push(
        [
            format!("wp-block-{short_name}-").as_bytes(),
            &layout_classname,
        ]
        .concat(),
    );
    for class in &outer_classes {
        tags.add_class(class);
    }
    if let Some(wrapper) = block.first_chunk.and_then(inner_wrapper_class) {
        // The tag whose class holds the wrapper's classes, else the first.
        for _ in 0..=content.len() {
            let class = match tags.get_attribute(b"class") {
                Some(hyscribe_html::Attribute::Value(class)) => class,
                _ => Vec::new(),
            };
            if class
                .windows(wrapper.len())
                .any(|w| w == wrapper.as_slice())
                || !tags.next_tag(None, None)
            {
                break;
            }
        }
    }
    for class in &classes {
        tags.add_class(class);
    }
    (tags.get_updated_html().to_vec(), css)
}

#[cfg(test)]
mod tests;
