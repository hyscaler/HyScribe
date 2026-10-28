// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP array and scalar semantics over [`Value`]: `_wp_array_get`,
//! `_wp_array_set`, `array_replace_recursive`, `empty()`, `is_numeric()` and
//! the string conversions the theme.json code relies on.

use std::collections::HashMap;

pub(crate) use hyscribe_phpser::isset;
use hyscribe_phpser::{Key, Value, float_to_string};

/// An array key as PHP stores it: canonical decimal integers become integer
/// keys, everything else stays a string.
pub(crate) fn key(name: &str) -> Key {
    Key::from_php_string(name.as_bytes().to_vec())
}

/// A key as the string PHP would print for it.
pub(crate) fn key_str(key: &Key) -> String {
    String::from_utf8_lossy(&key.to_php_string()).into_owned()
}

/// A PHP string value.
pub(crate) fn string(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// An empty PHP array.
pub(crate) fn array() -> Value {
    Value::Array(Vec::new())
}

/// The text of a string value. JSON input is always UTF-8.
pub(crate) fn as_str(value: &Value) -> Option<&str> {
    std::str::from_utf8(value.as_bytes()?).ok()
}

/// The entries of an array, empty for anything else.
pub(crate) fn entries(value: &Value) -> &[(Key, Value)] {
    value.as_array().unwrap_or_default()
}

/// `_wp_array_get( $value, $path )`: `None` where the path is missing,
/// `Some(Null)` where it holds `null`.
pub(crate) fn get<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    value.get_path(path)
}

/// Mutable access to the value at `path`, if every step is an array.
pub(crate) fn get_mut<'a>(value: &'a mut Value, path: &[&str]) -> Option<&'a mut Value> {
    path.iter().try_fold(value, |node, name| match node {
        Value::Array(entries) => {
            let wanted = key(name);
            entries
                .iter_mut()
                .find(|(k, _)| *k == wanted)
                .map(|(_, v)| v)
        }
        _ => None,
    })
}

pub(crate) use hyscribe_phpser::wp_array_set as set;

/// `unset( $value[a][b]... )`.
pub(crate) fn unset(value: &mut Value, path: &[&str]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    if let Some(Value::Array(entries)) = get_mut(value, parents) {
        let wanted = key(last);
        entries.retain(|(k, _)| *k != wanted);
    }
}

/// Writes `incoming` over the arrays `base`, key by key, in linear time:
/// `deep` merges arrays found on both sides instead of replacing them.
fn overlay(base: &mut Value, incoming: &Value, deep: bool) {
    let Value::Array(existing) = base else {
        return;
    };
    let mut index: HashMap<Key, usize> = existing
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.clone(), i))
        .collect();
    for (k, new) in entries(incoming) {
        match index.get(k).and_then(|&at| existing.get_mut(at)) {
            Some((_, old))
                if deep && matches!(old, Value::Array(_)) && matches!(new, Value::Array(_)) =>
            {
                overlay(old, new, true);
            }
            Some((_, old)) => *old = new.clone(),
            None => {
                index.insert(k.clone(), existing.len());
                existing.push((k.clone(), new.clone()));
            }
        }
    }
}

/// A PHP array being built with `$array[ $key ] = $value`: a repeated key
/// keeps its first position and takes the last value.
pub(crate) struct Ordered<K, V> {
    items: Vec<(K, V)>,
    index: HashMap<K, usize>,
}

impl<K: Clone + Eq + std::hash::Hash, V> Ordered<K, V> {
    pub(crate) fn new() -> Self {
        Ordered {
            items: Vec::new(),
            index: HashMap::new(),
        }
    }

    pub(crate) fn put(&mut self, key: K, value: V) {
        match self.index.get(&key).and_then(|&at| self.items.get_mut(at)) {
            Some(item) => item.1 = value,
            None => {
                self.index.insert(key.clone(), self.items.len());
                self.items.push((key, value));
            }
        }
    }

    pub(crate) fn into_vec(self) -> Vec<(K, V)> {
        self.items
    }
}

/// `array_replace_recursive( $base, $incoming )` for two arrays.
pub(crate) fn replace_recursive(base: &mut Value, incoming: &Value) {
    overlay(base, incoming, true);
}

/// `array_replace( $base, $other )`: top-level keys of `other` win. A
/// missing or scalar `other` changes nothing.
pub(crate) fn replace(base: &mut Value, other: Option<&Value>) {
    overlay(base, other.unwrap_or(&Value::Null), false);
}

/// PHP's `trim()` with its default characters.
pub(crate) fn trim(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
}

/// PHP's `empty()` of a value that may be missing.
pub(crate) fn empty(value: Option<&Value>) -> bool {
    value.is_none_or(Value::is_empty)
}

/// The numeric value of a number or numeric string.
pub(crate) fn number(value: &Value) -> Option<f64> {
    value.is_numeric().then(|| value.to_float())
}

/// `(string) $value` for the scalars CSS is built from; `None` for arrays,
/// booleans and null, which `to_ruleset` skips.
pub(crate) fn to_string(value: &Value) -> Option<String> {
    match value {
        Value::Int(n) => Some(n.to_string()),
        Value::Float(f) => Some(float_to_string(f.to_f64())),
        Value::String(_) => as_str(value).map(str::to_owned),
        _ => None,
    }
}

/// `(string) $value` for any value: `""` for null and false, `"1"` for
/// true, `"Array"` for arrays.
pub(crate) fn php_string(value: &Value) -> String {
    String::from_utf8_lossy(&value.to_php_string()).into_owned()
}

/// PHP 8.4's `round( $value, $places )`, half away from zero, for the
/// non-negative `places` theme.json uses.
pub(crate) fn round(value: f64, places: u32) -> f64 {
    let exponent = 10f64.powi(places as i32);
    let scaled = value * exponent;
    let mut integral = if value >= 0.0 {
        scaled.floor()
    } else {
        scaled.ceil()
    };
    // Beyond float precision rounding changes nothing; this also passes
    // NAN and INF through.
    if integral.abs().partial_cmp(&1e16) != Some(std::cmp::Ordering::Less) {
        return value;
    }
    let edge = ((integral + 0.5f64.copysign(integral)) / exponent).abs();
    if value.abs() >= edge {
        integral += 1f64.copysign(integral);
    }
    integral / exponent
}

#[cfg(test)]
mod tests;
