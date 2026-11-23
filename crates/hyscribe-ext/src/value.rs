// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP values crossing into a component and back (`types.value`): scalars
//! as they are, strings as UTF-8 text, arrays and objects as the JSON
//! `wp_json_encode()` writes. What cannot cross (a string that is not
//! UTF-8, a serialized payload) does not: the callback is not called.

use std::borrow::Cow;

use hyscribe_phpser::{Float, Value, json_decode, json_encode};
use wasmtime::component::{ComponentType, Lift, Lower};

/// `types.value-kind`.
#[derive(ComponentType, Lift, Lower, Clone, Copy, Debug, PartialEq, Eq)]
#[component(enum)]
#[repr(u8)]
pub(crate) enum Kind {
    #[component(name = "null")]
    Null,
    #[component(name = "boolean")]
    Boolean,
    #[component(name = "int")]
    Int,
    #[component(name = "float")]
    Float,
    #[component(name = "text")]
    Text,
    #[component(name = "json")]
    Json,
}

/// `types.value` as the host lowers it, its text borrowed where it can be.
pub(crate) type Lowered<'a> = (Kind, i64, f64, &'a str);

/// `types.value` as the host lifts it.
pub(crate) type Lifted = (Kind, i64, f64, String);

/// A value ready to lower: a string where it lies, JSON once encoded.
pub(crate) struct Crossing<'a>(Kind, i64, f64, Cow<'a, str>);

impl Crossing<'_> {
    pub(crate) fn lowered(&self) -> Lowered<'_> {
        (self.0, self.1, self.2, &self.3)
    }
}

/// `value` ready to cross, or none when it cannot.
pub(crate) fn lower(value: &Value) -> Option<Crossing<'_>> {
    let scalar = |kind, int, float| Some(Crossing(kind, int, float, Cow::Borrowed("")));
    match value {
        Value::Null => scalar(Kind::Null, 0, 0.0),
        Value::Bool(b) => scalar(Kind::Boolean, i64::from(*b), 0.0),
        Value::Int(n) => scalar(Kind::Int, *n, 0.0),
        Value::Float(f) => scalar(Kind::Float, 0, f.to_f64()),
        Value::String(bytes) => std::str::from_utf8(bytes)
            .ok()
            .map(|text| Crossing(Kind::Text, 0, 0.0, Cow::Borrowed(text))),
        _ => json_encode(value)
            .and_then(|json| String::from_utf8(json).ok())
            .map(|json| Crossing(Kind::Json, 0, 0.0, Cow::Owned(json))),
    }
}

/// Every value in `values` ready to cross, or none when one cannot.
pub(crate) fn lower_all(values: &[Value]) -> Option<Vec<Crossing<'_>>> {
    values.iter().map(lower).collect()
}

/// A value the component answered, or none for JSON that does not decode.
pub(crate) fn lift((kind, int, float, text): Lifted) -> Option<Value> {
    match kind {
        Kind::Null => Some(Value::Null),
        Kind::Boolean => Some(Value::Bool(int != 0)),
        Kind::Int => Some(Value::Int(int)),
        Kind::Float => Some(Value::Float(Float::from_f64(float))),
        Kind::Text => Some(Value::String(text.into_bytes())),
        Kind::Json => json_decode(text.as_bytes()),
    }
}

/// Values a component handed the host, or none when one does not decode.
pub(crate) fn lift_all(values: Vec<Lifted>) -> Option<Vec<Value>> {
    values.into_iter().map(lift).collect()
}

#[cfg(test)]
mod tests;
