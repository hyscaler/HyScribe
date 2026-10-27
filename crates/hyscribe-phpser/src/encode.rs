// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{Key, Value};

/// Encodes a value exactly as PHP's `serialize()` would.
pub fn encode(value: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    encode_into(value, &mut out);
    out
}

/// Appends the encoding of `value` to `out`.
fn encode_into(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.extend_from_slice(b"N;"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"b:1;" } else { b"b:0;" }),
        Value::Int(n) => int(*n, out),
        Value::Float(f) => {
            out.extend_from_slice(b"d:");
            out.extend_from_slice(f.as_str().as_bytes());
            out.push(b';');
        }
        Value::String(bytes) => string(bytes, out),
        Value::Array(entries) => {
            out.extend_from_slice(b"a:");
            entries_into(entries, out);
        }
        Value::Object { class, properties } => {
            out.extend_from_slice(b"O:");
            quoted(class, out);
            out.push(b':');
            entries_into(properties, out);
        }
        Value::Custom { class, data } => {
            out.extend_from_slice(b"C:");
            quoted(class, out);
            out.extend_from_slice(format!(":{}:{{", data.len()).as_bytes());
            out.extend_from_slice(data);
            out.push(b'}');
        }
        Value::Enum(name) => {
            out.extend_from_slice(b"E:");
            quoted(name, out);
            out.push(b';');
        }
        Value::ObjectRef(n) => out.extend_from_slice(format!("r:{n};").as_bytes()),
        Value::Ref(n) => out.extend_from_slice(format!("R:{n};").as_bytes()),
    }
}

fn int(n: i64, out: &mut Vec<u8>) {
    out.extend_from_slice(format!("i:{n};").as_bytes());
}

fn string(bytes: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(b"s:");
    quoted(bytes, out);
    out.push(b';');
}

fn quoted(bytes: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(format!("{}:\"", bytes.len()).as_bytes());
    out.extend_from_slice(bytes);
    out.push(b'"');
}

fn entries_into(entries: &[(Key, Value)], out: &mut Vec<u8>) {
    out.extend_from_slice(format!("{}:{{", entries.len()).as_bytes());
    for (key, value) in entries {
        match key {
            Key::Int(n) => int(*n, out),
            Key::String(bytes) => string(bytes, out),
        }
        encode_into(value, out);
    }
    out.push(b'}');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Float;

    #[test]
    fn encodes_every_type() {
        let value = Value::Array(vec![
            (Key::Int(0), Value::Null),
            (Key::Int(1), Value::Bool(true)),
            (Key::Int(2), Value::Bool(false)),
            (Key::Int(-3), Value::Int(-7)),
            (
                Key::String(b"f".to_vec()),
                Value::Float(Float::from_f64(0.1)),
            ),
            (
                Key::String(b"o".to_vec()),
                Value::Object {
                    class: b"stdClass".to_vec(),
                    properties: vec![(Key::String(b"p".to_vec()), Value::String(b"\xff".to_vec()))],
                },
            ),
            (
                Key::String(b"c".to_vec()),
                Value::Custom {
                    class: b"ArrayObject".to_vec(),
                    data: b"x:i:0".to_vec(),
                },
            ),
            (Key::String(b"e".to_vec()), Value::Enum(b"Suit:Up".to_vec())),
            (Key::String(b"r".to_vec()), Value::ObjectRef(1)),
            (Key::String(b"R".to_vec()), Value::Ref(2)),
        ]);
        let expected: &[u8] = b"a:10:{i:0;N;i:1;b:1;i:2;b:0;i:-3;i:-7;s:1:\"f\";d:0.1;\
s:1:\"o\";O:8:\"stdClass\":1:{s:1:\"p\";s:1:\"\xff\";}\
s:1:\"c\";C:11:\"ArrayObject\":5:{x:i:0}\
s:1:\"e\";E:7:\"Suit:Up\";s:1:\"r\";r:1;s:1:\"R\";R:2;}";
        assert_eq!(encode(&value), expected);
    }

    #[test]
    fn encode_into_appends() {
        let mut out = b"x".to_vec();
        encode_into(&Value::Null, &mut out);
        assert_eq!(out, b"xN;");
    }
}
