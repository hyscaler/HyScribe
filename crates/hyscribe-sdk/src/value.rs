// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A PHP value crossing a hook.

use alloc::string::String;

/// A PHP value: arrays and objects as the JSON `wp_json_encode()` writes.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Text(String),
    /// An array or object, as JSON.
    Json(String),
}

impl Value {
    /// The string, for a string.
    pub fn text(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The integer, for an integer.
    pub fn int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }
}

impl From<&str> for Value {
    fn from(text: &str) -> Value {
        Value::Text(text.into())
    }
}

impl From<String> for Value {
    fn from(text: String) -> Value {
        Value::Text(text)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Value {
        Value::Int(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

#[cfg(test)]
mod tests;
