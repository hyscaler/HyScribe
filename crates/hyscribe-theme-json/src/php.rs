// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP array and scalar semantics over [`Value`]: `_wp_array_get`,
//! `_wp_array_set`, `array_replace_recursive`, `empty()`, `is_numeric()` and
//! the string conversions the theme.json code relies on.

use std::collections::HashMap;

use hyscribe_phpser::{Key, Value};

/// An array key as PHP stores it: canonical decimal integers become integer
/// keys, everything else stays a string.
pub(crate) fn key(name: &str) -> Key {
    let digits = name.strip_prefix('-').unwrap_or(name);
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && name != "-0";
    match name.parse::<i64>() {
        Ok(n) if canonical => Key::Int(n),
        _ => Key::String(name.as_bytes().to_vec()),
    }
}

/// A key as the string PHP would print for it.
pub(crate) fn key_str(key: &Key) -> String {
    match key {
        Key::Int(n) => n.to_string(),
        Key::String(bytes) => String::from_utf8_lossy(bytes).into_owned(),
    }
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
    path.iter()
        .try_fold(value, |node, name| node.get(&key(name)))
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

/// `_wp_array_set( $value, $path, $new )`: missing or scalar steps become
/// arrays; does nothing unless `value` is an array.
pub(crate) fn set(value: &mut Value, path: &[&str], new: Value) {
    let (Value::Array(entries), Some((first, rest))) = (value, path.split_first()) else {
        return;
    };
    let wanted = key(first);
    let place = |child: &mut Value, new: Value| {
        if rest.is_empty() {
            *child = new;
        } else {
            if !matches!(child, Value::Array(_)) {
                *child = array();
            }
            set(child, rest, new);
        }
    };
    match entries.iter_mut().find(|(k, _)| *k == wanted) {
        Some((_, child)) => place(child, new),
        None => {
            let mut child = array();
            place(&mut child, new);
            entries.push((wanted, child));
        }
    }
}

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
    match value {
        None | Some(Value::Null | Value::Bool(false) | Value::Int(0)) => true,
        Some(Value::Float(f)) => f.to_f64() == 0.0,
        Some(Value::String(s)) => s.is_empty() || s == b"0",
        Some(Value::Array(entries)) => entries.is_empty(),
        Some(_) => false,
    }
}

/// PHP's `isset()` of a value that may be missing.
pub(crate) fn isset(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
}

/// PHP 8's `is_numeric()`.
pub(crate) fn is_numeric(value: &Value) -> bool {
    match value {
        Value::Int(_) | Value::Float(_) => true,
        Value::String(_) => as_str(value).is_some_and(numeric_str),
        _ => false,
    }
}

/// A numeric string: optional surrounding whitespace, sign, digits with an
/// optional fraction, optional exponent.
fn numeric_str(text: &str) -> bool {
    let ws = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c');
    let body = text.trim_matches(ws);
    let body = body.strip_prefix(['+', '-']).unwrap_or(body);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (body.get(..at).unwrap_or_default(), body.get(at + 1..)),
        None => (body, None),
    };
    let (int, frac) = match mantissa.split_once('.') {
        Some((int, frac)) => (int, Some(frac)),
        None => (mantissa, None),
    };
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = digits(int)
        && frac.is_none_or(digits)
        && !(int.is_empty() && frac.is_none_or(str::is_empty));
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

/// `(float) $text`: the leading numeric part, `0` without one.
pub(crate) fn to_float(text: &str) -> f64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let bytes = text.as_bytes();
    let digits = |from: usize| {
        bytes
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
    let int = digits(end);
    end += int;
    let frac = if bytes.get(end) == Some(&b'.') {
        digits(end + 1)
    } else {
        0
    };
    if int + frac == 0 {
        return 0.0;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1 + frac;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let at = end + 1 + usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exp = digits(at);
        if exp > 0 {
            end = at + exp;
        }
    }
    // The prefix is PHP's float grammar, which Rust parses the same way.
    text.get(..end)
        .and_then(|prefix| prefix.parse().ok())
        .unwrap_or(0.0)
}

/// The numeric value of a number or numeric string.
pub(crate) fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(f.to_f64()),
        Value::String(_) if is_numeric(value) => as_str(value).map(to_float),
        _ => None,
    }
}

/// `(string) $value` for the scalars CSS is built from; `None` for arrays,
/// booleans and null, which `to_ruleset` skips.
pub(crate) fn to_string(value: &Value) -> Option<String> {
    match value {
        Value::Int(n) => Some(n.to_string()),
        Value::Float(f) => Some(float_string(f.to_f64())),
        Value::String(_) => as_str(value).map(str::to_owned),
        _ => None,
    }
}

/// `(string) $value` for any value: `""` for null and false, `"1"` for
/// true, `"Array"` for arrays.
pub(crate) fn php_string(value: &Value) -> String {
    match value {
        Value::Bool(true) => "1".to_owned(),
        Value::Array(_) => "Array".to_owned(),
        other => to_string(other).unwrap_or_default(),
    }
}

/// `(string) $float`, with PHP's default `precision` of 14.
pub(crate) fn float_string(value: f64) -> String {
    if value.is_nan() {
        return "NAN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_positive() {
            "INF"
        } else {
            "-INF"
        }
        .into();
    }
    let sci = format!("{value:.13e}");
    let (mantissa, exponent) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let sign = if negative { "-" } else { "" };
    if !(-4..14).contains(&exponent) {
        let (first, rest) = digits.split_at_checked(1).unwrap_or((digits, ""));
        let rest = if rest.is_empty() { "0" } else { rest };
        return format!("{sign}{first}.{rest}E{exponent:+}");
    }
    if exponent < 0 {
        let zeros = "0".repeat(exponent.unsigned_abs() as usize - 1);
        return format!("{sign}0.{zeros}{digits}");
    }
    let point = exponent as usize + 1;
    if digits.len() <= point {
        format!("{sign}{digits}{}", "0".repeat(point - digits.len()))
    } else {
        let (int, frac) = digits.split_at_checked(point).unwrap_or((digits, ""));
        format!("{sign}{int}.{frac}")
    }
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
