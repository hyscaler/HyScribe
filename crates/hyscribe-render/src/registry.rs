// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block type registry, as WordPress registers core blocks (exported to
//! `wordpress/wp-includes/blocks/registry.json`), and
//! `WP_Block_Type::prepare_attributes_for_render()` with the part of
//! `rest_validate_value_from_schema()` that block attributes use: types,
//! `enum`, `items`, `minimum` and `maximum`.

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_phpser::{Float, Key, Value, json_decode};

const REGISTRY: &[u8] = include_bytes!("../../../wordpress/wp-includes/blocks/registry.json");

/// A registered block type.
#[derive(Debug)]
pub struct BlockType {
    /// The attribute schemas, in registration order.
    pub attributes: Vec<(Key, Value)>,
    /// `supports`, as registered.
    pub supports: Value,
}

/// The core block types.
pub fn block_type(name: &str) -> Option<&'static BlockType> {
    static TYPES: OnceLock<HashMap<Vec<u8>, BlockType>> = OnceLock::new();
    TYPES.get_or_init(|| load(REGISTRY)).get(name.as_bytes())
}

pub(crate) fn load(json: &[u8]) -> HashMap<Vec<u8>, BlockType> {
    let Some(Value::Array(types)) = json_decode(json) else {
        return HashMap::new();
    };
    types
        .into_iter()
        .filter_map(|(key, fields)| {
            let Key::String(name) = key else { return None };
            let attributes = match fields.get_str("attributes") {
                Some(Value::Array(attributes)) => attributes.clone(),
                _ => Vec::new(),
            };
            let supports = fields.get_str("supports").cloned().unwrap_or(Value::Null);
            Some((
                name,
                BlockType {
                    attributes,
                    supports,
                },
            ))
        })
        .collect()
}

impl BlockType {
    /// `prepare_attributes_for_render()`: invalid values are dropped, and
    /// missing attributes with a default get it.
    pub fn prepare(&self, attributes: &[(Key, Value)]) -> Vec<(Key, Value)> {
        let schema = |key: &Key| {
            self.attributes
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, s)| s)
        };
        let mut prepared: Vec<(Key, Value)> = attributes
            .iter()
            .filter(|(key, value)| schema(key).is_none_or(|schema| is_valid(value, schema)))
            .cloned()
            .collect();
        for (key, schema) in &self.attributes {
            let missing = !prepared.iter().any(|(k, _)| k == key);
            if let (true, Some(default)) = (missing, schema.get_str("default")) {
                // `isset()`: a null default counts as none.
                if *default != Value::Null {
                    prepared.push((key.clone(), default.clone()));
                }
            }
        }
        prepared
    }
}

const TYPES: [&str; 7] = [
    "array", "object", "string", "number", "integer", "boolean", "null",
];

/// The schema's type: one, or the best of several for `value`, as
/// `rest_handle_multi_type_schema()` picks it. `None` when no listed type
/// fits.
fn schema_type(value: &Value, schema: &Value) -> Option<String> {
    match schema.get_str("type") {
        Some(Value::String(name)) => Some(String::from_utf8_lossy(name).into_owned()),
        Some(Value::Array(types)) => {
            // A non-string entry is an unknown type, like any other.
            let names: Vec<String> = types
                .iter()
                .map(|(_, t)| String::from_utf8_lossy(&t.to_php_string()).into_owned())
                .collect();
            if *value == Value::String(Vec::new()) && names.iter().any(|n| n == "string") {
                return Some("string".into());
            }
            names
                .iter()
                .find(|name| fits(value, name))
                .or_else(|| names.iter().find(|name| !TYPES.contains(&name.as_str())))
                .cloned()
        }
        _ => Some(String::new()),
    }
}

/// `rest_get_best_type_for_value()`'s check for one type.
fn fits(value: &Value, name: &str) -> bool {
    match name {
        "array" => is_array(value),
        "object" => is_object(value),
        "integer" => is_integer(value),
        "number" => value.is_numeric(),
        "boolean" => is_boolean(value),
        "string" => matches!(value, Value::String(_)),
        "null" => *value == Value::Null,
        _ => false,
    }
}

/// `rest_validate_value_from_schema()` for the keywords block attributes
/// use.
pub(crate) fn is_valid(value: &Value, schema: &Value) -> bool {
    let Some(kind) = schema_type(value, schema) else {
        return false;
    };
    let typed = match kind.as_str() {
        "null" => *value == Value::Null,
        "boolean" => is_boolean(value),
        "object" => is_object(value),
        "array" => {
            is_array(value)
                && schema
                    .get_str("items")
                    .is_none_or(|items| list(value).iter().all(|item| is_valid(item, items)))
        }
        "number" => value.is_numeric() && in_bounds(value, schema),
        "integer" => is_integer(value) && in_bounds(value, schema),
        "string" => matches!(value, Value::String(_)),
        // Other types are not checked, as in WordPress.
        _ => true,
    };
    typed && enum_allows(value, schema, &kind)
}

/// `rest_validate_enum()`: the value, sanitized for its type, must equal
/// one of the listed values.
fn enum_allows(value: &Value, schema: &Value, kind: &str) -> bool {
    match schema.get_str("enum") {
        Some(Value::Array(options)) if !options.is_empty() => {
            let sanitized = sanitize(value, kind);
            options.iter().any(|(_, option)| equal(&sanitized, option))
        }
        _ => true,
    }
}

/// `rest_sanitize_value_from_schema()`, for values that passed their type
/// check: null and string values stay as they are.
fn sanitize(value: &Value, kind: &str) -> Value {
    match kind {
        "integer" => Value::Int(value.to_float() as i64),
        "number" => Value::Float(Float::from_f64(value.to_float())),
        // `rest_sanitize_boolean()`, for what passed `rest_is_boolean()`.
        "boolean" => Value::Bool(match value {
            Value::String(text) => !matches!(text.to_ascii_lowercase().as_slice(), b"false" | b"0"),
            other => *other == Value::Bool(true) || *other == Value::Int(1),
        }),
        "array" => Value::Array(
            list(value)
                .into_iter()
                .zip(0..)
                .map(|(v, i)| (Key::Int(i), v))
                .collect(),
        ),
        _ => value.clone(),
    }
}

/// `rest_are_values_equal()`.
fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(key, v)| y.iter().any(|(k, w)| k == key && equal(v, w)))
        }
        (Value::Int(n), Value::Float(f)) | (Value::Float(f), Value::Int(n)) => {
            *n as f64 == f.to_f64()
        }
        (Value::Float(x), Value::Float(y)) => x.to_f64() == y.to_f64(),
        _ => a == b,
    }
}

/// `minimum` and `maximum`, inclusive (core blocks set no exclusive bounds).
fn in_bounds(value: &Value, schema: &Value) -> bool {
    let number = value.to_float();
    let bound = |name: &str| schema.get_str(name).map(Value::to_float);
    bound("minimum").is_none_or(|min| number >= min)
        && bound("maximum").is_none_or(|max| number <= max)
}

/// `rest_is_boolean()`.
fn is_boolean(value: &Value) -> bool {
    match value {
        Value::Bool(_) => true,
        Value::String(text) => matches!(
            text.to_ascii_lowercase().as_slice(),
            b"false" | b"true" | b"0" | b"1"
        ),
        Value::Int(n) => *n == 0 || *n == 1,
        _ => false,
    }
}

/// `rest_is_object()`: any array, or `''`.
fn is_object(value: &Value) -> bool {
    matches!(value, Value::Array(_)) || *value == Value::String(Vec::new())
}

/// `rest_is_array()`: scalars split into lists, so they count; arrays count
/// when every key is an integer.
fn is_array(value: &Value) -> bool {
    match value {
        Value::Array(entries) => entries.iter().all(|(key, _)| matches!(key, Key::Int(_))),
        Value::Null => false,
        _ => true,
    }
}

/// `rest_sanitize_array()`: the list a value stands for.
fn list(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(entries) => entries.iter().map(|(_, v)| v.clone()).collect(),
        // `wp_parse_list()`: split on whitespace and commas.
        other => other
            .to_php_string()
            .split(|b| b.is_ascii_whitespace() || *b == b',')
            .filter(|part| !part.is_empty())
            .map(|part| Value::String(part.to_vec()))
            .collect(),
    }
}

/// `rest_is_integer()`.
fn is_integer(value: &Value) -> bool {
    match value {
        Value::Int(_) => true,
        Value::String(text) => {
            let trimmed = String::from_utf8_lossy(text);
            let trimmed = trimmed.trim();
            let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
            (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
                || (value.is_numeric() && value.to_float().fract() == 0.0)
        }
        Value::Float(f) => f.to_f64().fract() == 0.0,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
