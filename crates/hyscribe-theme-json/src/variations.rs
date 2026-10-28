// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block style variation support
//! (`wp_render_block_style_variation_support_styles()`): a block with an
//! `is-style-<name>` class whose variation has styles gets an instance class
//! and a stylesheet scoped to it.

use hyscribe_phpser::Value;

use crate::php::{self, as_str};
use crate::stylesheet::{Options, Tree, Types};
use crate::theme_json::{self, Origin};
use crate::{GlobalStyles, schema};

impl GlobalStyles {
    /// The variation the support picks for a block with `class_name`: the
    /// first `is-style-<name>` class, other than `is-style-default`, whose
    /// variation has styles.
    pub fn style_variation(&self, block: &str, class_name: &[u8]) -> Option<String> {
        class_variations(class_name).into_iter().find(|variation| {
            !php::empty(php::get(
                &self.data,
                &["styles", "blocks", block, "variations", variation],
            ))
        })
    }

    /// The CSS of `instance` (`<variation>--<n>`), a copy of a block's
    /// `variation`; empty when the variation prints nothing.
    pub fn style_variation_css(&self, block: &str, variation: &str, instance: &str) -> String {
        let mut data = php::get(
            &self.data,
            &["styles", "blocks", block, "variations", variation],
        )
        .cloned()
        .unwrap_or_else(php::array);
        resolve_refs(&mut data, &self.data);
        let elements = php::get(&data, &["elements"])
            .cloned()
            .unwrap_or_else(php::array);
        let mut blocks = php::get(&data, &["blocks"])
            .cloned()
            .unwrap_or_else(php::array);
        php::unset(&mut data, &["elements"]);
        php::unset(&mut data, &["blocks"]);
        php::set(&mut blocks, &[block, "variations", instance], data);

        let mut config = schema::latest();
        php::set(
            &mut config,
            &["settings", "spacing", "blockGap"],
            Value::Bool(true),
        );
        php::set(&mut config, &["styles", "elements"], elements);
        php::set(&mut config, &["styles", "blocks"], blocks);
        if let Some(viewport) = self.setting(&["viewport"]).filter(|v| php::isset(Some(v))) {
            php::set(&mut config, &["settings", "viewport"], viewport.clone());
        }
        let mut registry = self.blocks.clone();
        registry.register_styles(&[(block.to_owned(), instance.to_owned())]);
        let data = theme_json::new(config, Origin::Blocks, &registry);
        let tree = Tree {
            data: &data,
            blocks: &registry,
            global: self.settings(),
        };
        let styles = Types {
            variables: false,
            styles: true,
            presets: false,
            custom_css: false,
        };
        tree.stylesheet(
            styles,
            &["custom"],
            Options {
                blocks: true,
                scope: Some(&format!(".is-style-{instance}")),
            },
        )
    }
}

const PREFIX: &[u8] = b"is-style-";

/// PCRE's `\w` and `\s`, without Unicode.
fn word(b: Option<&u8>) -> bool {
    b.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn space(b: &u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// `\bis-style-` at `at`: the rest of the class name and the length of
/// its first run of non-space bytes.
fn after_prefix(class: &[u8], at: usize) -> Option<(&[u8], usize)> {
    let tail = class.get(at + PREFIX.len()..)?;
    let starts = class.get(at..).unwrap_or_default().starts_with(PREFIX)
        && !word(at.checked_sub(1).and_then(|i| class.get(i)));
    starts.then(|| (tail, tail.iter().position(space).unwrap_or(tail.len())))
}

/// `wp_get_block_style_variation_name_from_class()`: the matches of
/// `/\bis-style-(?!default)(\S+)\b/`.
fn class_variations(class_name: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    let mut at = 0;
    while class_name.get(at..).is_some_and(|rest| !rest.is_empty()) {
        // `\S+\b` backtracks to the last word boundary in the run.
        let found = after_prefix(class_name, at)
            .filter(|(tail, _)| !tail.starts_with(b"default"))
            .and_then(|(tail, run)| {
                let n = (1..=run)
                    .rev()
                    .find(|&n| word(tail.get(n - 1)) != word(tail.get(n)))?;
                Some((tail.get(..n).unwrap_or_default(), n))
            });
        match found {
            Some((name, n)) => {
                names.push(String::from_utf8_lossy(name).into_owned());
                at += PREFIX.len() + n;
            }
            None => at += 1,
        }
    }
    names
}

/// The instance class `wp_render_block_style_variation_class_name()` puts
/// on a block's first tag: the first match of
/// `/\bis-style-(\S+?--\d+)\b/` in its class name.
pub fn instance_class(class_name: &[u8]) -> Option<&[u8]> {
    (0..class_name.len()).find_map(|at| {
        let (tail, run) = after_prefix(class_name, at)?;
        // The lazy `\S+?` takes the fewest characters before `--` and
        // digits that end at a word boundary.
        let end = (1..run).find_map(|n| {
            let digits = tail.get(n..).unwrap_or_default().strip_prefix(b"--")?;
            let count = digits.iter().take_while(|b| b.is_ascii_digit()).count();
            (count > 0 && !word(digits.get(count))).then_some(n + 2 + count)
        })?;
        class_name.get(at..at + PREFIX.len() + end)
    })
}

/// `wp_resolve_block_style_variation_ref_values()`: `{ "ref": "a.b" }`
/// values take the value at that path of the merged data, or go.
fn resolve_refs(data: &mut Value, tree: &Value) {
    let Value::Array(entries) = data else {
        return;
    };
    entries.retain_mut(|(_, value)| {
        let Some(reference) = php::get(value, &["ref"]) else {
            resolve_refs(value, tree);
            return true;
        };
        let target = as_str(reference)
            .and_then(|r| php::get(tree, &r.split('.').collect::<Vec<_>>()))
            .filter(|t| !matches!(t, Value::Null))
            .cloned();
        match target {
            Some(target) => {
                *value = target;
                true
            }
            None => false,
        }
    });
}

#[cfg(test)]
mod tests;
