// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's `json_decode($json, true)`: JSON objects become PHP arrays, with
//! the same conversions PHP makes, so the result serializes exactly as
//! PHP's would.
//!
//! - Object keys that look like integers (`"7"`, `"-3"`, not `"07"` or
//!   `"-0"`) become integer keys; a repeated key keeps its first position
//!   and takes the last value
//! - Integers outside the `i64` range become floats
//! - Nesting past [`JSON_MAX_DEPTH`] is an error, as is anything
//!   PHP rejects: invalid UTF-8, unescaped control characters, lone
//!   surrogates, upper case literals, leading zeros

use std::collections::HashMap;

use crate::{Float, Key, Value};

/// PHP's default `json_decode` depth. It counts the innermost value as a
/// level too, so at most 511 arrays and objects can be nested.
pub const JSON_MAX_DEPTH: usize = 512;

/// Decodes `json` as `json_decode($json, true)` does. `None` where PHP
/// returns `null` because of an error; JSON `null` is `Some(Value::Null)`.
pub fn json_decode(json: &[u8]) -> Option<Value> {
    let mut parser = Parser { src: json, pos: 0 };
    parser.whitespace();
    let value = parser.value(0)?;
    parser.whitespace();
    (parser.pos == json.len()).then_some(value)
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn rest(&self) -> &[u8] {
        self.src.get(self.pos..).unwrap_or_default()
    }

    fn whitespace(&mut self) {
        let skip = self
            .rest()
            .iter()
            .take_while(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
            .count();
        self.pos += skip;
    }

    fn eat(&mut self, byte: u8) -> bool {
        let found = self.peek() == Some(byte);
        self.pos += usize::from(found);
        found
    }

    fn literal(&mut self, word: &[u8], value: Value) -> Option<Value> {
        self.rest().starts_with(word).then(|| {
            self.pos += word.len();
            value
        })
    }

    /// One value; `depth` counts the arrays and objects around it.
    fn value(&mut self, depth: usize) -> Option<Value> {
        match self.peek()? {
            b'{' | b'[' if depth + 1 >= JSON_MAX_DEPTH => None,
            b'{' => self.object(depth + 1),
            b'[' => self.array(depth + 1),
            b'"' => self.string().map(Value::String),
            b't' => self.literal(b"true", Value::Bool(true)),
            b'f' => self.literal(b"false", Value::Bool(false)),
            b'n' => self.literal(b"null", Value::Null),
            _ => self.number(),
        }
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        self.pos += 1;
        self.whitespace();
        let mut items = Vec::new();
        let mut closed = self.eat(b']');
        // Every item consumes input; the bound only rules out a hang.
        for _ in 0..self.src.len() {
            if closed {
                break;
            }
            self.whitespace();
            let index = i64::try_from(items.len()).unwrap_or(i64::MAX);
            items.push((Key::Int(index), self.value(depth)?));
            self.whitespace();
            closed = self.eat(b']');
            if !closed && !self.eat(b',') {
                return None;
            }
        }
        closed.then_some(Value::Array(items))
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        self.pos += 1;
        self.whitespace();
        let mut entries: Vec<(Key, Value)> = Vec::new();
        // Where each key sits, so repeated keys stay linear.
        let mut index: HashMap<Key, usize> = HashMap::new();
        let mut closed = self.eat(b'}');
        // Every entry consumes input; the bound only rules out a hang.
        for _ in 0..self.src.len() {
            if closed {
                break;
            }
            self.whitespace();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = Key::from_php_string(self.string()?);
            self.whitespace();
            if !self.eat(b':') {
                return None;
            }
            self.whitespace();
            let value = self.value(depth)?;
            match index.get(&key).and_then(|&at| entries.get_mut(at)) {
                Some(entry) => entry.1 = value,
                None => {
                    index.insert(key.clone(), entries.len());
                    entries.push((key, value));
                }
            }
            self.whitespace();
            closed = self.eat(b'}');
            if !closed && !self.eat(b',') {
                return None;
            }
        }
        closed.then_some(Value::Array(entries))
    }

    /// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`
    fn number(&mut self) -> Option<Value> {
        let start = self.pos;
        let digits = |p: &Parser<'_>| p.rest().iter().take_while(|b| b.is_ascii_digit()).count();
        self.eat(b'-');
        let int_len = digits(self);
        if int_len == 0 || (int_len > 1 && self.peek() == Some(b'0')) {
            return None;
        }
        self.pos += int_len;
        let mut float = false;
        if self.eat(b'.') {
            let frac = digits(self);
            if frac == 0 {
                return None;
            }
            self.pos += frac;
            float = true;
        }
        if self.eat(b'e') || self.eat(b'E') {
            let _ = self.eat(b'+') || self.eat(b'-');
            let exp = digits(self);
            if exp == 0 {
                return None;
            }
            self.pos += exp;
            float = true;
        }
        // Only ASCII digits, signs, dots and exponents got this far.
        let text: String = self
            .src
            .get(start..self.pos)
            .unwrap_or_default()
            .iter()
            .map(|&b| char::from(b))
            .collect();
        if !float {
            // Integers that do not fit become floats, as in PHP.
            if let Ok(n) = text.parse::<i64>() {
                return Some(Value::Int(n));
            }
        }
        text.parse::<f64>()
            .ok()
            .map(|f| Value::Float(Float::from_f64(f)))
    }

    /// A string after its opening quote is next, with escapes decoded and
    /// UTF-8 checked. The scan is driven by an iterator over the input, so
    /// it always ends.
    fn string(&mut self) -> Option<Vec<u8>> {
        let rest = self.src.get(self.pos + 1..).unwrap_or_default();
        let mut out = Vec::new();
        let mut bytes = rest.iter().enumerate();
        while let Some((i, &b)) = bytes.next() {
            let after = rest.get(i + 1..).unwrap_or_default();
            let skip = match b {
                b'"' => {
                    self.pos += i + 2;
                    return Some(out);
                }
                b'\\' => escape(after, &mut out)?,
                0x20..=0x7f => {
                    out.push(b);
                    0
                }
                // Control characters and invalid UTF-8 fail here too.
                _ => {
                    let sequence = utf8_sequence(rest.get(i..).unwrap_or_default())?;
                    out.extend_from_slice(sequence);
                    sequence.len() - 1
                }
            };
            for _ in 0..skip {
                bytes.next();
            }
        }
        None
    }
}

/// Decodes the escape after a backslash (`after` starts right after it)
/// into `out`, and returns how many bytes of `after` it used.
fn escape(after: &[u8], out: &mut Vec<u8>) -> Option<usize> {
    let simple = match after.first()? {
        b'"' => b'"',
        b'\\' => b'\\',
        b'/' => b'/',
        b'b' => 0x08,
        b'f' => 0x0c,
        b'n' => b'\n',
        b'r' => b'\r',
        b't' => b'\t',
        b'u' => {
            let unit = hex4(after.get(1..).unwrap_or_default())?;
            let (code, used) = match unit {
                0xd800..=0xdbff => {
                    // A high surrogate needs a low one right after.
                    let low = after
                        .get(5..)
                        .and_then(|next| next.strip_prefix(b"\\u"))
                        .and_then(hex4)?;
                    if !(0xdc00..=0xdfff).contains(&low) {
                        return None;
                    }
                    (0x10000 + ((unit - 0xd800) << 10) + (low - 0xdc00), 11)
                }
                0xdc00..=0xdfff => return None,
                unit => (unit, 5),
            };
            // Surrogates were combined above, so every code is a char.
            let c = char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER);
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            return Some(used);
        }
        _ => return None,
    };
    out.push(simple);
    Some(1)
}

/// Four hex digits at the start of `text`.
fn hex4(text: &[u8]) -> Option<u32> {
    text.get(..4)?
        .iter()
        .try_fold(0, |n, &b| char::from(b).to_digit(16).map(|d| n * 16 + d))
}

/// The valid multi-byte UTF-8 sequence starting `bytes`, or `None`.
fn utf8_sequence(bytes: &[u8]) -> Option<&[u8]> {
    let len = match bytes {
        [0xc2..=0xdf, ..] => 2,
        [0xe0..=0xef, ..] => 3,
        [0xf0..=0xf4, ..] => 4,
        _ => return None,
    };
    // from_utf8 rejects overlong forms, surrogates and values past U+10FFFF.
    let sequence = bytes.get(..len)?;
    std::str::from_utf8(sequence).ok().map(|_| sequence)
}

/// `json_encode( $value )` with no flags, as `wp_json_encode()` calls it:
/// lists as arrays and other arrays as objects, `/` and every non-ASCII
/// character escaped. `None` where PHP fails: a string that is not UTF-8,
/// an infinite or NaN float, or a value other than null, a scalar or an
/// array.
pub fn json_encode(value: &Value) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    encode_value(value, false, &mut out)?;
    Some(out)
}

/// `json_encode( $value, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE )`,
/// as `serialize_block_attributes()` calls it. U+2028 and U+2029 stay
/// escaped, as PHP keeps them without `JSON_UNESCAPED_LINE_TERMINATORS`.
pub fn json_encode_unescaped(value: &Value) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    encode_value(value, true, &mut out)?;
    Some(out)
}

fn encode_value(value: &Value, unescaped: bool, out: &mut Vec<u8>) -> Option<()> {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Int(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::Float(f) => {
            let number = f.to_f64();
            if !number.is_finite() {
                return None;
            }
            out.extend_from_slice(
                crate::float::php_format(number)
                    .replace('E', "e")
                    .as_bytes(),
            );
        }
        Value::String(text) => encode_string(text, unescaped, out)?,
        Value::Array(entries) => {
            let list = entries
                .iter()
                .zip(0..)
                .all(|((key, _), i)| *key == Key::Int(i));
            out.push(if list { b'[' } else { b'{' });
            for (i, (key, item)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                if !list {
                    let name = match key {
                        Key::Int(n) => n.to_string().into_bytes(),
                        Key::String(name) => name.clone(),
                    };
                    encode_string(&name, unescaped, out)?;
                    out.push(b':');
                }
                encode_value(item, unescaped, out)?;
            }
            out.push(if list { b']' } else { b'}' });
        }
        _ => return None,
    }
    Some(())
}

fn encode_string(text: &[u8], unescaped: bool, out: &mut Vec<u8>) -> Option<()> {
    let text = std::str::from_utf8(text).ok()?;
    out.push(b'"');
    for c in text.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '/' if !unescaped => out.extend_from_slice(b"\\/"),
            '\u{8}' => out.extend_from_slice(b"\\b"),
            '\u{c}' => out.extend_from_slice(b"\\f"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            ' '..='~' | '\u{7f}' => out.push(c as u8),
            '\u{80}'.. if unescaped && !matches!(c, '\u{2028}' | '\u{2029}') => {
                out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            }
            _ => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.extend_from_slice(format!("\\u{unit:04x}").as_bytes());
                }
            }
        }
    }
    out.push(b'"');
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(json: &str) -> Option<Value> {
        json_decode(json.as_bytes())
    }

    fn php(json: &str) -> String {
        decode(json).map_or("error".to_string(), |v| {
            String::from_utf8(crate::encode(&v)).unwrap()
        })
    }

    #[test]
    fn scalars() {
        assert_eq!(php("true"), "b:1;");
        assert_eq!(php("false"), "b:0;");
        assert_eq!(php("null"), "N;");
        assert_eq!(php(" \t\r\n7 \n"), "i:7;");
        for bad in ["", "TRUE", "nul", "nulll", "x", "1 2", " "] {
            assert_eq!(php(bad), "error", "{bad:?}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(php("0"), "i:0;");
        assert_eq!(php("-0"), "i:0;");
        assert_eq!(php("-12"), "i:-12;");
        assert_eq!(php("9223372036854775807"), "i:9223372036854775807;");
        assert_eq!(php("-9223372036854775808"), "i:-9223372036854775808;");
        assert_eq!(php("9223372036854775808"), "d:9.223372036854776E+18;");
        assert_eq!(php("1.5"), "d:1.5;");
        assert_eq!(php("-0.0"), "d:-0;");
        assert_eq!(php("1e2"), "d:100;");
        assert_eq!(php("1E+2"), "d:100;");
        assert_eq!(php("1e-2"), "d:0.01;");
        assert_eq!(php("1e400"), "d:INF;");
        for bad in [
            "01", "-", "--1", "+1", ".5", "5.", "1e", "1e+", "1.e1", "-a",
        ] {
            assert_eq!(php(bad), "error", "{bad:?}");
        }
    }

    #[test]
    fn strings() {
        assert_eq!(
            php(r#""a\"\\\/\b\f\n\r\t""#),
            "s:9:\"a\"\\/\u{8}\u{c}\n\r\t\";"
        );
        assert_eq!(php(r#""\u00e9\u00E9""#), "s:4:\"\u{e9}\u{e9}\";");
        assert_eq!(php(r#""\ud83d\ude00""#), "s:4:\"\u{1F600}\";");
        assert_eq!(php(r#""\u0000""#), "s:1:\"\u{0}\";");
        assert_eq!(
            php("\"\u{e9}\u{20ac}\u{1F600}\u{7f}\""),
            "s:10:\"\u{e9}\u{20ac}\u{1F600}\u{7f}\";"
        );
        for bad in [
            r#""\ud83d""#,
            r#""\ude00""#,
            r#""\ud83d\u0041""#,
            r#""\ud83dx""#,
            r#""\ud83d\uzzzz""#,
            r#""\u12""#,
            r#""\uzzzz""#,
            r#""\x""#,
            "\"\\",
            "\"a",
            "\"\u{1}\"",
        ] {
            assert_eq!(php(bad), "error", "{bad:?}");
        }
        for bytes in [
            &b"\"\xff\""[..],
            b"\"\xc0\x80\"",
            b"\"\xed\xa0\x80\"",
            b"\"\xf4\x90\x80\x80\"",
            b"\"\xe2\x82\"",
            b"\"\xc3",
            b"\"\x80\"",
        ] {
            assert_eq!(json_decode(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn arrays() {
        assert_eq!(php("[]"), "a:0:{}");
        assert_eq!(php(" [ 1 , \"b\" ] "), "a:2:{i:0;i:1;i:1;s:1:\"b\";}");
        assert_eq!(php("[[1]]"), "a:1:{i:0;a:1:{i:0;i:1;}}");
        for bad in ["[1,]", "[,1]", "[1 2]", "[1", "[", "]"] {
            assert_eq!(php(bad), "error", "{bad:?}");
        }
    }

    #[test]
    fn objects() {
        assert_eq!(php("{}"), "a:0:{}");
        assert_eq!(
            php(r#"{ "a" : 1 , "b" : [ ] }"#),
            "a:2:{s:1:\"a\";i:1;s:1:\"b\";a:0:{}}"
        );
        // A repeated key keeps its first place and takes the last value.
        assert_eq!(
            php(r#"{"a":1,"b":2,"a":3}"#),
            "a:2:{s:1:\"a\";i:3;s:1:\"b\";i:2;}"
        );
        assert_eq!(php(r#"{"1":"x","1":"y"}"#), "a:1:{i:1;s:1:\"y\";}");
        for bad in [
            "{\"a\":1,}",
            "{\"a\" 1}",
            "{1:2}",
            "{\"a\":1",
            "{\"a\":}",
            "{\"a",
            "{",
            "}",
            "{\"a\":1 \"b\":2}",
        ] {
            assert_eq!(php(bad), "error", "{bad:?}");
        }
    }

    #[test]
    fn integer_like_keys_become_integers() {
        let key = |text: &str| Key::from_php_string(text.as_bytes().to_vec());
        let string = |text: &str| Key::String(text.as_bytes().to_vec());
        assert_eq!(key("0"), Key::Int(0));
        assert_eq!(key("7"), Key::Int(7));
        assert_eq!(key("-5"), Key::Int(-5));
        assert_eq!(key("9223372036854775807"), Key::Int(i64::MAX));
        assert_eq!(key("-9223372036854775808"), Key::Int(i64::MIN));
        for text in [
            "",
            "-",
            "-0",
            "07",
            "-07",
            "00",
            "1a",
            " 1",
            "+1",
            "9223372036854775808",
            "-9223372036854775809",
            "\u{e9}",
        ] {
            assert_eq!(key(text), string(text), "{text:?}");
        }
    }

    #[test]
    fn nesting_stops_where_php_stops() {
        let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        assert!(decode(&nested(JSON_MAX_DEPTH - 1)).is_some());
        assert_eq!(decode(&nested(JSON_MAX_DEPTH)), None);
        let objects = |depth: usize| {
            format!(
                "{}{{}}{}",
                "{\"a\":".repeat(depth - 1),
                "}".repeat(depth - 1)
            )
        };
        assert!(decode(&objects(JSON_MAX_DEPTH - 1)).is_some());
        assert_eq!(decode(&objects(JSON_MAX_DEPTH)), None);
    }

    /// Every case of `reference/phpser/json-golden.php`.
    #[test]
    fn json_encode_matches_php() {
        let golden = include_str!("../tests/golden/json-encode.tsv");
        for line in golden.lines() {
            let mut fields = line.split('\t').map(hyscribe_testkit::unhex);
            let mut field = || fields.next().unwrap();
            let (value, php, unescaped) = (field(), field(), field());
            let value = crate::decode(&value).unwrap();
            assert_eq!(json_encode(&value), Some(php), "{value:?}");
            assert_eq!(json_encode_unescaped(&value), Some(unescaped), "{value:?}");
        }
        assert!(golden.lines().count() > 30);
    }

    #[test]
    fn json_encode_fails_where_php_does() {
        assert_eq!(json_encode(&Value::String(vec![0xff])), None);
        assert_eq!(
            json_encode(&Value::Float(crate::Float::from_f64(f64::NAN))),
            None
        );
        assert_eq!(json_encode(&Value::Enum(b"Suit:Hearts".to_vec())), None);
        let nested = Value::Array(vec![(Key::String(vec![0xff]), Value::Null)]);
        assert_eq!(json_encode(&nested), None);
        assert_eq!(
            json_encode(&Value::Array(vec![(Key::Int(0), Value::Ref(1))])),
            None
        );
        // Form feeds and backspaces have short escapes.
        assert_eq!(
            json_encode(&Value::String(b"\x08\x0c".to_vec())),
            Some(br#""\b\f""#.to_vec())
        );
    }
}
