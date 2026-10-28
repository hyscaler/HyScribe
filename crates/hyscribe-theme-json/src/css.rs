// SPDX-License-Identifier: AGPL-3.0-or-later

//! CSS text helpers: rulesets, the `safecss_filter_attr()` check behind
//! `is_safe_css_declaration()`, and the nesting of a block's custom CSS.

use hyscribe_phpser::Value;

use crate::php::{self, trim};
use crate::selector;

/// One `name: value` declaration. The value is a PHP value: rulesets print
/// strings and numbers and skip the rest.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Declaration {
    pub(crate) name: String,
    pub(crate) value: Value,
}

impl Declaration {
    pub(crate) fn new(name: &str, value: Value) -> Declaration {
        Declaration {
            name: name.to_owned(),
            value,
        }
    }

    /// A declaration with a string value.
    pub(crate) fn text(name: &str, value: &str) -> Declaration {
        Declaration::new(name, php::string(value))
    }
}

/// `to_ruleset( $selector, $declarations )`.
pub(crate) fn ruleset(selector: &str, declarations: &[Declaration]) -> String {
    if declarations.is_empty() {
        return String::new();
    }
    let body: String = declarations
        .iter()
        .filter_map(|d| php::to_string(&d.value).map(|v| format!("{}: {v};", d.name)))
        .collect();
    format!("{selector}{{{body}}}")
}

/// The properties `safecss_filter_attr()` allows by default.
const SAFE_PROPERTIES: &[&str] = &[
    "background",
    "background-color",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "background-attachment",
    "background-blend-mode",
    "border",
    "border-radius",
    "border-width",
    "border-color",
    "border-style",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-top",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "border-spacing",
    "border-collapse",
    "caption-side",
    "columns",
    "column-count",
    "column-fill",
    "column-gap",
    "column-rule",
    "column-span",
    "column-width",
    "display",
    "color",
    "filter",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "letter-spacing",
    "line-height",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "white-space",
    "height",
    "min-height",
    "max-height",
    "width",
    "min-width",
    "max-width",
    "margin",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-top",
    "margin-block-start",
    "margin-block-end",
    "margin-inline-start",
    "margin-inline-end",
    "padding",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-top",
    "padding-block-start",
    "padding-block-end",
    "padding-inline-start",
    "padding-inline-end",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "gap",
    "row-gap",
    "grid-template-columns",
    "grid-auto-columns",
    "grid-column-start",
    "grid-column-end",
    "grid-column",
    "grid-column-gap",
    "grid-template-rows",
    "grid-auto-rows",
    "grid-row-start",
    "grid-row-end",
    "grid-row",
    "grid-row-gap",
    "grid-gap",
    "justify-content",
    "justify-items",
    "justify-self",
    "align-content",
    "align-items",
    "align-self",
    "clear",
    "cursor",
    "direction",
    "float",
    "list-style-type",
    "object-fit",
    "object-position",
    "opacity",
    "overflow",
    "vertical-align",
    "writing-mode",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "z-index",
    "box-shadow",
    "aspect-ratio",
    "container-type",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "color-interpolation",
    "color-interpolation-filters",
    "paint-order",
    "stop-color",
    "stop-opacity",
    "flood-color",
    "flood-opacity",
    "lighting-color",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "clip-path",
    "clip-rule",
    "mask",
    "mask-type",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x",
    "y",
    "d",
    "alignment-baseline",
    "baseline-shift",
    "dominant-baseline",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "text-anchor",
    "unicode-bidi",
    "word-spacing",
    "font-size-adjust",
    "font-stretch",
    "color-rendering",
    "image-rendering",
    "shape-rendering",
    "text-rendering",
    "vector-effect",
    "transform",
    "transform-origin",
    "pointer-events",
    "visibility",
];

/// Properties whose values may hold `url()` or gradients. Their checks need
/// the URL protocol filter, which is not ported, so they are refused.
const URL_PROPERTIES: &[&str] = &[
    "background",
    "background-image",
    "cursor",
    "filter",
    "list-style",
    "list-style-image",
    "clip-path",
    "fill",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "stroke",
];

/// Functions whose calls `safecss_filter_attr()` strips before looking for
/// unsafe characters.
const VALUE_FUNCTIONS: &[&str] = &[
    "var",
    "calc",
    "min",
    "max",
    "minmax",
    "clamp",
    "repeat",
    "matrix",
    "matrix3d",
    "perspective",
    "rotate",
    "rotate3d",
    "rotateX",
    "rotateY",
    "rotateZ",
    "scale",
    "scale3d",
    "scaleX",
    "scaleY",
    "scaleZ",
    "skew",
    "skewX",
    "skewY",
    "translate",
    "translate3d",
    "translateX",
    "translateY",
    "translateZ",
    "circle",
    "ellipse",
    "inset",
    "path",
    "polygon",
    "rect",
    "shape",
    "xywh",
];

/// `is_safe_css_declaration( $name, $value )`: whether `safecss_filter_attr()`
/// keeps anything of `name: value`.
pub(crate) fn is_safe(name: &str, value: &str) -> bool {
    let css: String = format!("{name}: {value}")
        .chars()
        .filter(|c| !matches!(c, '\0'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}'))
        .collect();
    let css = strip_escaped_nulls(&css).replace(['\n', '\r', '\t'], "");
    trim(&css).split(';').any(|item| {
        let item = trim(item);
        !item.is_empty() && safe_item(item)
    })
}

/// `wp_kses_no_null()`'s second pass: `\\+0+` goes.
fn strip_escaped_nulls(css: &str) -> String {
    let mut out = String::new();
    let mut rest = css;
    while let Some(at) = rest.find('\\') {
        let (head, tail) = rest.split_at_checked(at).unwrap_or((rest, ""));
        out.push_str(head);
        let slashes = tail.len() - tail.trim_start_matches('\\').len();
        let after = tail.get(slashes..).unwrap_or_default();
        let zeros = after.len() - after.trim_start_matches('0').len();
        if zeros == 0 {
            out.push_str(tail.get(..slashes).unwrap_or_default());
        }
        rest = after.get(zeros..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

fn safe_item(item: &str) -> bool {
    if let Some((property, value)) = item.split_once(':') {
        let property = trim(property);
        let custom = property.len() > 2
            && property.starts_with("--")
            && property
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if !(custom || SAFE_PROPERTIES.contains(&property)) {
            return false;
        }
        // URLs would need the protocol filter, which is not ported: refuse.
        let url = if custom {
            trim(value).starts_with("url(")
        } else {
            URL_PROPERTIES.contains(&property)
        };
        if url && value.contains("url(") {
            return false;
        }
    }
    let stripped = strip_functions(item);
    !(stripped.contains(['\\', '(', '&', '=', '}']) || stripped.contains("/*"))
}

/// Removes every call to a [`VALUE_FUNCTIONS`] function that starts at a
/// word boundary and has balanced parentheses.
fn strip_functions(text: &str) -> String {
    let bytes = text.as_bytes();
    let word = |i: usize| {
        bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    };
    let mut out = String::new();
    let mut copied = 0;
    let mut at = 0;
    // Every pass moves `at` forward; the bound only rules out a hang.
    for _ in 0..=bytes.len() {
        let call = (at == 0 || !word(at - 1))
            .then(|| {
                let rest = bytes.get(at..).unwrap_or_default();
                VALUE_FUNCTIONS
                    .iter()
                    .filter(|f| rest.starts_with(f.as_bytes()) && rest.get(f.len()) == Some(&b'('))
                    .find_map(|f| balanced(bytes, at + f.len()))
            })
            .flatten();
        match call {
            Some(end) => {
                out.push_str(text.get(copied..at).unwrap_or_default());
                copied = end;
                at = end;
            }
            None => at += 1,
        }
    }
    out.push_str(text.get(copied..).unwrap_or_default());
    out
}

/// The end of the balanced parenthesized group that opens at `open`.
fn balanced(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, b) in bytes.iter().enumerate().skip(open) {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// `process_blocks_custom_css( $css, $selector )`: nests `&` rules under the
/// block selector, every rule at 0-1-0 specificity.
pub(crate) fn nest_custom(css: &str, selector: &str) -> String {
    let mut processed = String::new();
    for part in css.split('&').filter(|p| !p.is_empty() && *p != "0") {
        if !part.contains('{') {
            processed.push_str(&format!(
                ":root :where({}){{{}}}",
                trim(selector),
                trim(part)
            ));
            continue;
        }
        let unbraced = part.replace('}', "");
        let Some((nested, value)) = unbraced.split_once('{').filter(|(_, v)| !v.contains('{'))
        else {
            continue;
        };
        let (nested, pseudo) = match pseudo_element(nested) {
            Some(pseudo) => (nested.replace(pseudo, ""), pseudo),
            None => (nested.to_owned(), ""),
        };
        let part_selector = if nested.starts_with(' ') {
            selector::scope(selector, &nested)
        } else {
            selector::append(selector, &nested)
        };
        processed.push_str(&format!(
            ":root :where({part_selector}){pseudo}{{{}}}",
            trim(value)
        ));
    }
    processed
}

/// The first match of `/([>+~\s]*::[a-zA-Z-]+)/`.
fn pseudo_element(selector: &str) -> Option<&str> {
    let bytes = selector.as_bytes();
    let lead = |b: u8| {
        matches!(
            b,
            b'>' | b'+' | b'~' | b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'
        )
    };
    let name = |b: &u8| b.is_ascii_alphabetic() || *b == b'-';
    (0..bytes.len()).find_map(|start| {
        let colons = start
            + bytes
                .get(start..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| lead(b))
                .count();
        let rest = bytes
            .get(colons..)
            .unwrap_or_default()
            .strip_prefix(b"::")?;
        let len = rest.iter().take_while(|b| name(b)).count();
        (len > 0)
            .then(|| selector.get(start..colons + 2 + len))
            .flatten()
    })
}

#[cfg(test)]
mod tests;
