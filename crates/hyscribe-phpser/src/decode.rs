// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::{Error, ErrorKind, Float, Key, Value};

/// Deepest nesting of arrays and objects accepted. Bounds recursion so hostile
/// input cannot exhaust the stack, even on small thread stacks in debug
/// builds. WordPress data rarely nests deeper than ten levels.
pub const MAX_DEPTH: usize = 128;

/// Smallest possible array entry (`i:0;N;`), used to reject counts the input
/// cannot hold before allocating for them.
const MIN_ENTRY_LEN: usize = 6;

/// Decodes one serialized value that must span the whole input.
///
/// Accepts what PHP's `unserialize()` accepts for the types it produces,
/// except that trailing bytes are an error rather than a warning. Integers
/// outside the 64-bit range saturate, as in PHP.
pub fn decode(input: &[u8]) -> Result<Value, Error> {
    let mut parser = Parser { input, pos: 0 };
    let value = parser.value(0)?;
    if parser.pos == input.len() {
        Ok(value)
    } else {
        Err(parser.error(ErrorKind::TrailingData))
    }
}

/// PHP's `unserialize()`: the value at the start of the input, with any
/// trailing bytes ignored, as PHP 8 only warns about them.
pub fn unserialize(input: &[u8]) -> Option<Value> {
    Parser { input, pos: 0 }.value(0).ok()
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, kind: ErrorKind) -> Error {
        Error {
            offset: self.pos,
            kind,
        }
    }

    fn peek(&self) -> Result<u8, Error> {
        self.input
            .get(self.pos)
            .copied()
            .ok_or_else(|| self.error(ErrorKind::UnexpectedEnd))
    }

    fn expect(&mut self, token: &'static str) -> Result<(), Error> {
        let bytes = token.as_bytes();
        match self.input.get(self.pos..self.pos + bytes.len()) {
            Some(found) if found == bytes => {
                self.pos += bytes.len();
                Ok(())
            }
            Some(_) => Err(self.error(ErrorKind::Expected(token))),
            None => Err(self.error(ErrorKind::UnexpectedEnd)),
        }
    }

    fn take(&mut self, len: usize) -> Result<&[u8], Error> {
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| self.error(ErrorKind::LengthOverflow))?;
        let bytes = self
            .input
            .get(self.pos..end)
            .ok_or_else(|| self.error(ErrorKind::UnexpectedEnd))?;
        self.pos = end;
        Ok(bytes)
    }

    fn digits(&mut self) -> Result<&[u8], Error> {
        let rest = self.input.get(self.pos..).unwrap_or_default();
        let len = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if len == 0 {
            return Err(if rest.is_empty() {
                self.error(ErrorKind::UnexpectedEnd)
            } else {
                self.error(ErrorKind::Expected("digit"))
            });
        }
        self.take(len)
    }

    /// Unsigned length or count.
    fn uint(&mut self) -> Result<usize, Error> {
        let start = self.pos;
        let mut n: usize = 0;
        for &d in self.digits()? {
            n = n
                .checked_mul(10)
                .and_then(|n| n.checked_add(usize::from(d - b'0')))
                .ok_or(Error {
                    offset: start,
                    kind: ErrorKind::LengthOverflow,
                })?;
        }
        Ok(n)
    }

    /// Signed integer, saturating like PHP.
    fn int(&mut self) -> Result<i64, Error> {
        let negative = match self.peek()? {
            b'-' => true,
            b'+' => false,
            _ => return self.digits().map(|d| accumulate(d, false)),
        };
        self.pos += 1;
        self.digits().map(|d| accumulate(d, negative))
    }

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        let marker = self.peek()?;
        match marker {
            b'N' => self.expect("N;").map(|()| Value::Null),
            b'b' => {
                self.expect("b:")?;
                let value = match self.peek()? {
                    b'0' => false,
                    b'1' => true,
                    _ => return Err(self.error(ErrorKind::Expected("0 or 1"))),
                };
                self.pos += 1;
                self.expect(";")?;
                Ok(Value::Bool(value))
            }
            b'i' => self.int_body().map(Value::Int),
            b'd' => {
                self.expect("d:")?;
                let start = self.pos;
                let rest = self.input.get(start..).unwrap_or_default();
                let len = rest.iter().take_while(|&&b| b != b';').count();
                let (text, _) = rest.split_at(len);
                self.pos += len;
                let float = Float::parse(text).ok_or(Error {
                    offset: start,
                    kind: ErrorKind::InvalidFloat,
                })?;
                self.expect(";")?;
                Ok(Value::Float(float))
            }
            b's' => self.string_body().map(Value::String),
            b'a' => {
                self.expect("a:")?;
                let entries = self.entries(depth)?;
                Ok(Value::Array(entries))
            }
            b'O' => {
                self.expect("O:")?;
                let class = self.class_name()?;
                self.expect(":")?;
                let properties = self.entries(depth)?;
                Ok(Value::Object { class, properties })
            }
            b'C' => {
                self.expect("C:")?;
                let class = self.class_name()?;
                self.expect(":")?;
                let len = self.uint()?;
                self.expect(":{")?;
                let data = self.take(len)?.to_vec();
                self.expect("}")?;
                Ok(Value::Custom { class, data })
            }
            b'E' => {
                self.expect("E:")?;
                let start = self.pos;
                let name = self.quoted()?;
                let valid = name.iter().position(|&b| b == b':').is_some_and(|colon| {
                    let (class, case) = name.split_at(colon);
                    is_class_name(class) && is_label(case.get(1..).unwrap_or_default())
                });
                if !valid {
                    return Err(Error {
                        offset: start,
                        kind: ErrorKind::InvalidClassName,
                    });
                }
                self.expect(";")?;
                Ok(Value::Enum(name))
            }
            b'r' | b'R' => {
                self.pos += 1;
                self.expect(":")?;
                let n = self.uint()? as u64;
                self.expect(";")?;
                Ok(if marker == b'r' {
                    Value::ObjectRef(n)
                } else {
                    Value::Ref(n)
                })
            }
            other => Err(self.error(ErrorKind::UnsupportedType(other))),
        }
    }

    fn int_body(&mut self) -> Result<i64, Error> {
        self.expect("i:")?;
        let n = self.int()?;
        self.expect(";")?;
        Ok(n)
    }

    fn string_body(&mut self) -> Result<Vec<u8>, Error> {
        self.expect("s:")?;
        let bytes = self.quoted()?;
        self.expect(";")?;
        Ok(bytes)
    }

    /// `<len>:"<bytes>"`
    fn quoted(&mut self) -> Result<Vec<u8>, Error> {
        let len = self.uint()?;
        self.expect(":\"")?;
        let bytes = self.take(len)?.to_vec();
        self.expect("\"")?;
        Ok(bytes)
    }

    fn class_name(&mut self) -> Result<Vec<u8>, Error> {
        let start = self.pos;
        let name = self.quoted()?;
        if is_class_name(&name) {
            Ok(name)
        } else {
            Err(Error {
                offset: start,
                kind: ErrorKind::InvalidClassName,
            })
        }
    }

    /// `<n>:{<key><value>...}`
    fn entries(&mut self, depth: usize) -> Result<Vec<(Key, Value)>, Error> {
        if depth >= MAX_DEPTH {
            return Err(self.error(ErrorKind::TooDeep));
        }
        let count_at = self.pos;
        let count = self.uint()?;
        self.expect(":{")?;
        let remaining = self.input.len() - self.pos;
        if count > remaining / MIN_ENTRY_LEN {
            return Err(Error {
                offset: count_at,
                kind: ErrorKind::CountTooLarge,
            });
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let key = match self.peek()? {
                b'i' => Key::Int(self.int_body()?),
                b's' => Key::String(self.string_body()?),
                _ => return Err(self.error(ErrorKind::InvalidKey)),
            };
            let value = self.value(depth + 1)?;
            entries.push((key, value));
        }
        self.expect("}")?;
        Ok(entries)
    }
}

fn accumulate(digits: &[u8], negative: bool) -> i64 {
    let mut n: i64 = 0;
    for &d in digits {
        let d = i64::from(d - b'0');
        let next = n.checked_mul(10).and_then(|n| {
            if negative {
                n.checked_sub(d)
            } else {
                n.checked_add(d)
            }
        });
        match next {
            Some(next) => n = next,
            None => return if negative { i64::MIN } else { i64::MAX },
        }
    }
    n
}

/// Class names PHP's `unserialize()` accepts: non-empty, made of letters,
/// digits, `_`, `\\` and bytes 0x80 and up. Unknown classes are never looked
/// up here.
fn is_class_name(name: &[u8]) -> bool {
    !name.is_empty()
        && name
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b >= 0x80)
}

fn is_label(part: &[u8]) -> bool {
    match part.split_first() {
        Some((first, rest)) => {
            is_label_start(*first)
                && rest
                    .iter()
                    .all(|&b| is_label_start(b) || b.is_ascii_digit())
        }
        None => false,
    }
}

fn is_label_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(input: &str) -> Error {
        decode(input.as_bytes()).expect_err(input)
    }

    fn kind(input: &str) -> ErrorKind {
        err(input).kind
    }

    #[test]
    fn decodes_scalars() {
        assert_eq!(decode(b"N;"), Ok(Value::Null));
        assert_eq!(decode(b"b:0;"), Ok(Value::Bool(false)));
        assert_eq!(decode(b"b:1;"), Ok(Value::Bool(true)));
        assert_eq!(decode(b"i:42;"), Ok(Value::Int(42)));
        assert_eq!(decode(b"i:-42;"), Ok(Value::Int(-42)));
        assert_eq!(decode(b"i:+42;"), Ok(Value::Int(42)));
        assert_eq!(decode(b"d:0.5;"), Ok(Value::Float(Float::from_f64(0.5))));
        assert_eq!(
            decode(b"s:3:\"a\"b\";"),
            Ok(Value::String(b"a\"b".to_vec()))
        );
        assert_eq!(decode(b"s:0:\"\";"), Ok(Value::String(Vec::new())));
        assert_eq!(decode(b"r:2;"), Ok(Value::ObjectRef(2)));
    }

    /// PHP 8.4 keeps a value that trailing bytes follow.
    #[test]
    fn unserialize_ignores_trailing_bytes() {
        assert_eq!(super::unserialize(b"i:5;junk"), Some(Value::Int(5)));
        assert_eq!(super::unserialize(b"junk"), None);
        assert_eq!(decode(b"R:3;"), Ok(Value::Ref(3)));
        assert_eq!(
            decode(b"E:7:\"Suit:Up\";"),
            Ok(Value::Enum(b"Suit:Up".to_vec()))
        );
    }

    #[test]
    fn integers_saturate_like_php() {
        assert_eq!(decode(b"i:9223372036854775807;"), Ok(Value::Int(i64::MAX)));
        assert_eq!(decode(b"i:-9223372036854775808;"), Ok(Value::Int(i64::MIN)));
        assert_eq!(decode(b"i:99999999999999999999;"), Ok(Value::Int(i64::MAX)));
        assert_eq!(
            decode(b"i:-99999999999999999999;"),
            Ok(Value::Int(i64::MIN))
        );
    }

    #[test]
    fn decodes_containers() {
        let value = decode(b"a:2:{i:0;s:1:\"x\";s:1:\"k\";a:0:{}}").unwrap();
        assert_eq!(
            value,
            Value::Array(vec![
                (Key::Int(0), Value::String(b"x".to_vec())),
                (Key::String(b"k".to_vec()), Value::Array(Vec::new())),
            ])
        );
        let object = decode(b"O:8:\"stdClass\":1:{s:4:\"\0*\0p\";N;}").unwrap();
        assert_eq!(
            object,
            Value::Object {
                class: b"stdClass".to_vec(),
                properties: vec![(Key::String(b"\0*\0p".to_vec()), Value::Null)],
            }
        );
        let custom = decode(b"C:11:\"ArrayObject\":5:{x:i:0}").unwrap();
        assert_eq!(
            custom,
            Value::Custom {
                class: b"ArrayObject".to_vec(),
                data: b"x:i:0".to_vec(),
            }
        );
        // PHP accepts these names and would make an incomplete class.
        for name in ["\\A\\B_c1", "1ab", "a\\\\", "\u{e9}t\u{e9}"] {
            let input = format!("O:{}:\"{name}\":0:{{}}", name.len());
            assert!(decode(input.as_bytes()).is_ok(), "{name}");
        }
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(kind(""), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("N"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("Nx"), ErrorKind::Expected("N;"));
        assert_eq!(kind("b:2;"), ErrorKind::Expected("0 or 1"));
        assert_eq!(kind("b:"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("b:1:"), ErrorKind::Expected(";"));
        assert_eq!(kind("i:;"), ErrorKind::Expected("digit"));
        assert_eq!(kind("i:-;"), ErrorKind::Expected("digit"));
        assert_eq!(kind("i:"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("i:-"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("s:"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("d:1x;"), ErrorKind::InvalidFloat);
        assert_eq!(kind("d:1"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("s:5:\"abc\";"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("s:1:\"ab\";"), ErrorKind::Expected("\""));
        assert_eq!(kind("s:1:'a';"), ErrorKind::Expected(":\""));
        assert_eq!(
            kind("s:99999999999999999999999:\"\";"),
            ErrorKind::LengthOverflow
        );
        assert_eq!(kind("a:1:{N;N;N;}"), ErrorKind::InvalidKey);
        assert_eq!(kind("a:1:{i:0;N;"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("a:1000:{i:0;N;}"), ErrorKind::CountTooLarge);
        // One entry more than the remaining bytes can hold.
        assert_eq!(kind("a:2:{i:0;N;}"), ErrorKind::CountTooLarge);
        assert!(decode(b"a:2:{i:0;N;i:1;N;}").is_ok());
        assert_eq!(kind("O:0:\"\":0:{}"), ErrorKind::InvalidClassName);
        assert_eq!(kind("O:3:\"a b\":0:{}"), ErrorKind::InvalidClassName);
        assert_eq!(kind("O:1:\"a\"0:{}"), ErrorKind::Expected(":"));
        assert_eq!(kind("C:1:\"a\":1:{ab}"), ErrorKind::Expected("}"));
        assert_eq!(kind("C:1:\"a\"3"), ErrorKind::Expected(":"));
        assert_eq!(kind("C:1:\"!\":0:{}"), ErrorKind::InvalidClassName);
        assert_eq!(kind("E:4:\"Suit\";"), ErrorKind::InvalidClassName);
        assert_eq!(kind("E:5:\"Suit:\";"), ErrorKind::InvalidClassName);
        assert_eq!(kind("E:5:\":Case\";"), ErrorKind::InvalidClassName);
        assert_eq!(kind("E:6:\"Su:t:x\";"), ErrorKind::InvalidClassName);
        assert_eq!(kind("E:3:\"A:B\""), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("r2;"), ErrorKind::Expected(":"));
        assert_eq!(kind("R:2"), ErrorKind::UnexpectedEnd);
        assert_eq!(kind("S:1:\"a\";"), ErrorKind::UnsupportedType(b'S'));
        assert_eq!(kind("o:1:\"a\":0:{}"), ErrorKind::UnsupportedType(b'o'));
        assert_eq!(kind("N;N;"), ErrorKind::TrailingData);
        let markers = ["bx", "ix", "dx", "sx", "ax", "Ox", "Cx", "Ex", "Rx"];
        let expected = ["b:", "i:", "d:", "s:", "a:", "O:", "C:", "E:", ":"];
        assert_eq!(
            markers.map(kind),
            expected.map(ErrorKind::Expected),
            "a marker must be followed by its colon"
        );
        assert_eq!(kind("C:1:\"a\":x"), ErrorKind::Expected("digit"));
        assert_eq!(kind("C:1:\"a\":1xx}"), ErrorKind::Expected(":{"));
        assert_eq!(kind("r:x;"), ErrorKind::Expected("digit"));
    }

    #[test]
    fn nested_errors_propagate() {
        let cases = [
            ("i:5x", ErrorKind::Expected(";")),
            ("s:1:\"a\"x", ErrorKind::Expected(";")),
            ("O:x", ErrorKind::Expected("digit")),
            ("O:1:\"a\":x", ErrorKind::Expected("digit")),
            ("C:1:\"a\":5:{ab", ErrorKind::UnexpectedEnd),
            ("E:x", ErrorKind::Expected("digit")),
            ("a:x", ErrorKind::Expected("digit")),
            ("a:0x{}", ErrorKind::Expected(":{")),
            ("a:2:{i:0;s:5:\"xxxxx\";", ErrorKind::UnexpectedEnd),
            ("a:1:{i:x;N;}", ErrorKind::Expected("digit")),
            ("a:1:{s:x;N;}", ErrorKind::Expected("digit")),
            ("a:1:{i:0;X;}", ErrorKind::UnsupportedType(b'X')),
        ];
        assert_eq!(
            cases.map(|(input, _)| kind(input)),
            cases.map(|(_, expected)| expected)
        );
    }

    #[test]
    fn errors_carry_offsets() {
        assert_eq!(err("a:1:{i:0;X;}").offset, 9);
        assert_eq!(err("s:99999999999999999999999:\"\";").offset, 2);
        assert_eq!(err("a:1000:{}").offset, 2);
        assert_eq!(err("O:3:\"a b\":0:{}").offset, 2);
        assert_eq!(err("E:4:\"Suit\";").offset, 2);
        assert_eq!(err("d:1x;").offset, 2);
        assert_eq!(err("N;N;").offset, 2);
    }

    #[test]
    fn take_rejects_overflowing_lengths() {
        let mut parser = Parser {
            input: b"ab",
            pos: 1,
        };
        assert_eq!(
            parser.take(usize::MAX).unwrap_err().kind,
            ErrorKind::LengthOverflow
        );
    }

    #[test]
    fn nesting_is_bounded() {
        let open = "a:1:{i:0;".repeat(MAX_DEPTH + 1);
        let close = "}".repeat(MAX_DEPTH + 1);
        assert_eq!(kind(&format!("{open}N;{close}")), ErrorKind::TooDeep);

        let open = "a:1:{i:0;".repeat(MAX_DEPTH);
        let close = "}".repeat(MAX_DEPTH);
        assert!(decode(format!("{open}N;{close}").as_bytes()).is_ok());
    }
}
