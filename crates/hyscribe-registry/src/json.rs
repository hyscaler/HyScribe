// SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry's JSON documents through `hyscribe-phpser`'s bounded
//! `json_decode()`, each object's members taken one by one; a member left
//! over, or of another type, refuses the document.

use hyscribe_phpser::{Key, Value};

/// An object's members not yet taken.
pub(crate) struct Object(Vec<(Key, Value)>);

impl Object {
    /// The document `bytes`, if it is a JSON object.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Object> {
        Object::of(hyscribe_phpser::json_decode(bytes)?)
    }

    pub(crate) fn of(value: Value) -> Option<Object> {
        match value {
            Value::Array(members) => Some(Object(members)),
            _ => None,
        }
    }

    fn take(&mut self, name: &str) -> Option<Value> {
        let at = self.0.iter().position(|(key, _)| match key {
            Key::String(key) => key == name.as_bytes(),
            Key::Int(_) => false,
        })?;
        Some(self.0.remove(at).1)
    }

    /// A member that is an integer of at least 0.
    pub(crate) fn int(&mut self, name: &str) -> Option<u64> {
        match self.take(name)? {
            Value::Int(n) => u64::try_from(n).ok(),
            _ => None,
        }
    }

    /// A member that is a string.
    pub(crate) fn text(&mut self, name: &str) -> Option<String> {
        text(self.take(name)?)
    }

    /// A member that is an object, its members in their order.
    pub(crate) fn object(&mut self, name: &str) -> Option<Object> {
        Object::of(self.take(name)?)
    }

    /// A member that is a list: an array whose keys count up from 0.
    pub(crate) fn list(&mut self, name: &str) -> Option<Vec<Value>> {
        let members = self.object(name)?.0;
        let counted = members
            .iter()
            .zip(0..)
            .all(|((key, _), n)| *key == Key::Int(n));
        counted.then(|| members.into_iter().map(|(_, value)| value).collect())
    }

    /// The members left, as names and values.
    pub(crate) fn rest(self) -> Vec<(Key, Value)> {
        self.0
    }

    /// Nothing is left over.
    pub(crate) fn done(self) -> Option<()> {
        self.0.is_empty().then_some(())
    }
}

/// A string value.
pub(crate) fn text(value: Value) -> Option<String> {
    match value {
        Value::String(text) => String::from_utf8(text).ok(),
        _ => None,
    }
}
