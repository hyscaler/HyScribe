// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::Float;

/// An array or object key. PHP only allows integers and strings.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    /// `i:<n>;`
    Int(i64),
    /// `s:<len>:"<bytes>";`
    String(Vec<u8>),
}

/// A PHP serialized value, kept as data.
///
/// Strings are bytes because WordPress databases hold any encoding. Arrays and
/// objects keep their entries in order, including duplicate keys, so encoding
/// reproduces the input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// `N;`
    Null,
    /// `b:0;` or `b:1;`
    Bool(bool),
    /// `i:<n>;`
    Int(i64),
    /// `d:<text>;`
    Float(Float),
    /// `s:<len>:"<bytes>";`
    String(Vec<u8>),
    /// `a:<n>:{...}`
    Array(Vec<(Key, Value)>),
    /// `O:<len>:"<class>":<n>:{...}`, never instantiated. Property names keep
    /// PHP's visibility markers (`\0*\0name`, `\0Class\0name`).
    Object {
        /// Class name as written.
        class: Vec<u8>,
        /// Properties in order.
        properties: Vec<(Key, Value)>,
    },
    /// `C:<len>:"<class>":<len>:{<data>}`, a `Serializable` payload kept as
    /// raw bytes.
    Custom {
        /// Class name as written.
        class: Vec<u8>,
        /// Payload between the braces.
        data: Vec<u8>,
    },
    /// `E:<len>:"<Enum>:<Case>";`
    Enum(Vec<u8>),
    /// `r:<n>;`, an object handle reference. Not resolved.
    ObjectRef(u64),
    /// `R:<n>;`, a PHP reference (`&`). Not resolved.
    Ref(u64),
}

impl Value {
    /// Entries of an array, or `None` for any other value.
    pub fn as_array(&self) -> Option<&[(Key, Value)]> {
        match self {
            Value::Array(entries) => Some(entries),
            _ => None,
        }
    }

    /// Bytes of a string, or `None` for any other value.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::String(bytes) => Some(bytes),
            _ => None,
        }
    }

    /// The value for a key in an array. PHP keeps the last of duplicate keys,
    /// and so does this.
    pub fn get(&self, key: &Key) -> Option<&Value> {
        self.as_array()?
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Shorthand for [`Value::get`] with a string key.
    pub fn get_str(&self, key: &str) -> Option<&Value> {
        self.get(&Key::String(key.as_bytes().to_vec()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array() -> Value {
        Value::Array(vec![
            (Key::String(b"a".to_vec()), Value::Int(1)),
            (Key::Int(0), Value::String(b"zero".to_vec())),
            (Key::String(b"a".to_vec()), Value::Int(2)),
        ])
    }

    #[test]
    fn get_returns_last_duplicate() {
        assert_eq!(array().get_str("a"), Some(&Value::Int(2)));
        assert_eq!(
            array().get(&Key::Int(0)),
            Some(&Value::String(b"zero".to_vec()))
        );
        assert_eq!(array().get_str("missing"), None);
    }

    #[test]
    fn accessors_reject_other_types() {
        assert_eq!(Value::Null.as_array(), None);
        assert_eq!(Value::Null.get_str("a"), None);
        assert_eq!(Value::Int(1).as_bytes(), None);
        assert_eq!(Value::String(b"x".to_vec()).as_bytes(), Some(&b"x"[..]));
        assert_eq!(array().as_array().map(<[_]>::len), Some(3));
    }
}
