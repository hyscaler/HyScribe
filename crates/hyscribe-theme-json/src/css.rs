// SPDX-License-Identifier: AGPL-3.0-or-later

//! CSS text helpers: rulesets, the `safecss_filter_attr()` check behind
//! `is_safe_css_declaration()`, and the nesting of a block's custom CSS.

use hyscribe_kses::{esc_html, safecss_filter_attr};
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

/// `is_safe_css_declaration( $name, $value )`: whether `safecss_filter_attr()`
/// keeps anything of `name: value`.
pub(crate) fn is_safe(name: &str, value: &str) -> bool {
    let filtered = safecss_filter_attr(format!("{name}: {value}").as_bytes());
    !trim(&esc_html(&filtered)).is_empty()
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
