// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP 8's `empty()`, `isset()`, `is_numeric()`, `(float)`, `(int)` and
//! `(string)` over [`Value`], for code ported from WordPress that leans on
//! them.

use crate::Value;

impl Value {
    /// PHP's `empty()`.
    pub fn is_empty(&self) -> bool {
        match self {
            Value::Null | Value::Bool(false) | Value::Int(0) => true,
            Value::Float(f) => f.to_f64() == 0.0,
            Value::String(text) => is_empty_string(text),
            Value::Array(entries) => entries.is_empty(),
            _ => false,
        }
    }

    /// PHP 8's `is_numeric()`: numbers, and strings holding one with
    /// optional whitespace around it.
    pub fn is_numeric(&self) -> bool {
        match self {
            Value::Int(_) | Value::Float(_) => true,
            Value::String(text) => numeric_prefix(text).is_some_and(|(_, len)| {
                text.get(len..)
                    .is_some_and(|rest| rest.iter().all(|&b| is_space(b)))
            }),
            _ => false,
        }
    }

    /// `(float) $value`. Objects give `1`, as PHP does with a warning.
    pub fn to_float(&self) -> f64 {
        match self {
            Value::Null | Value::Bool(false) => 0.0,
            Value::Int(n) => *n as f64,
            Value::Float(f) => f.to_f64(),
            Value::String(text) => string_to_float(text),
            Value::Array(entries) => f64::from(u8::from(!entries.is_empty())),
            _ => 1.0,
        }
    }

    /// `(int) $value`. Objects give `1`, as PHP does with a warning.
    pub fn to_int(&self) -> i64 {
        match self {
            Value::Null | Value::Bool(false) => 0,
            Value::Int(n) => *n,
            Value::Float(f) => float_to_int(f.to_f64()),
            Value::String(text) => string_to_int(text),
            Value::Array(entries) => i64::from(!entries.is_empty()),
            _ => 1,
        }
    }

    /// `(string) $value`. Arrays give `"Array"`, as PHP does with a
    /// warning; objects, which PHP refuses, give nothing.
    pub fn to_php_string(&self) -> Vec<u8> {
        match self {
            Value::Bool(true) => b"1".to_vec(),
            Value::Int(n) => n.to_string().into_bytes(),
            Value::Float(f) => float_to_string(f.to_f64()).into_bytes(),
            Value::String(text) => text.clone(),
            Value::Array(_) => b"Array".to_vec(),
            _ => Vec::new(),
        }
    }
}

/// `empty( $text )` for a string: `''` and `'0'`.
pub fn is_empty_string(text: &[u8]) -> bool {
    text.is_empty() || text == b"0"
}

/// `isset( $value )` of a value that may be missing.
pub fn isset(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
}

/// WordPress's `absint( $value )`.
pub fn absint(value: &Value) -> u64 {
    value.to_int().unsigned_abs()
}

/// `(int) $float`: truncated, wrapping modulo 2^64 when out of range, and
/// 0 for infinities and NaN.
fn float_to_int(value: f64) -> i64 {
    const TWO_64: f64 = 18_446_744_073_709_551_616.0;
    if !value.is_finite() {
        return 0;
    }
    let truncated = value.trunc();
    if truncated.abs() < TWO_64 / 2.0 {
        return truncated as i64;
    }
    let wrapped = truncated.rem_euclid(TWO_64);
    if wrapped >= TWO_64 / 2.0 {
        (wrapped - TWO_64) as i64
    } else {
        wrapped as i64
    }
}

/// `(int) $text`: the number at the start of `text`, saturating at the
/// integer limits; 0 without one, or for an infinite float.
fn string_to_int(text: &[u8]) -> i64 {
    let Some((number, len)) = numeric_prefix(text) else {
        return 0;
    };
    let prefix = text.get(..len).unwrap_or_default();
    if prefix.iter().any(|b| matches!(b, b'.' | b'e' | b'E')) {
        // Float syntax: capped, as `zend_dval_to_lval_cap()` does.
        return if number.is_finite() { number as i64 } else { 0 };
    }
    let lead = prefix.iter().take_while(|&&b| is_space(b)).count();
    let digits = prefix.get(lead..).unwrap_or_default();
    // Only an overflow fails to parse: it saturates toward its sign.
    std::str::from_utf8(digits)
        .ok()
        .and_then(|digits| digits.parse().ok())
        .unwrap_or(if digits.first() == Some(&b'-') {
            i64::MIN
        } else {
            i64::MAX
        })
}

/// `(float) $text`: the number at the start of `text`, `0` without one.
pub fn string_to_float(text: &[u8]) -> f64 {
    numeric_prefix(text).map_or(0.0, |(number, _)| number)
}

/// The number PHP 8's arithmetic takes of `text`: the one at its start,
/// `None` for the `TypeError` of a string without one.
pub fn string_to_number(text: &[u8]) -> Option<f64> {
    numeric_prefix(text).map(|(number, _)| number)
}

/// `(string) $float`, with PHP's default `precision` of 14.
pub fn float_to_string(value: f64) -> String {
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
    let sign = if mantissa.starts_with('-') { "-" } else { "" };
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
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
    match digits.split_at_checked(point) {
        Some((int, frac)) if !frac.is_empty() => format!("{sign}{int}.{frac}"),
        _ => format!(
            "{sign}{digits}{}",
            "0".repeat(point - digits.len().min(point))
        ),
    }
}

/// The whitespace PHP allows around a numeric string.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b'\x0b' | b'\x0c')
}

/// The number at the start of `text` (after whitespace) and how many bytes
/// it takes, as PHP's numeric string rules read it.
fn numeric_prefix(text: &[u8]) -> Option<(f64, usize)> {
    let lead = text.iter().take_while(|&&b| is_space(b)).count();
    let rest = text.get(lead..).unwrap_or_default();
    let digits = |from: usize| {
        rest.get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut len = usize::from(matches!(rest.first(), Some(b'+' | b'-')));
    let whole = digits(len);
    len += whole;
    let fraction = if rest.get(len) == Some(&b'.') {
        digits(len + 1)
    } else {
        0
    };
    if whole + fraction == 0 {
        return None;
    }
    if rest.get(len) == Some(&b'.') {
        len += 1 + fraction;
    }
    if matches!(rest.get(len), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(rest.get(len + 1), Some(b'+' | b'-')));
        let exponent = digits(len + 1 + sign);
        if exponent > 0 {
            len += 1 + sign + exponent;
        }
    }
    // The prefix is PHP's float grammar, which Rust parses the same way.
    let number = std::str::from_utf8(rest.get(..len).unwrap_or_default())
        .ok()
        .and_then(|prefix| prefix.parse().ok())
        .unwrap_or(0.0);
    Some((number, lead + len))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode;
    use hyscribe_testkit::unhex;

    /// Every case of `reference/phpser/cast-golden.php`.
    #[test]
    fn matches_php() {
        let golden = include_str!("../tests/golden/cast.tsv");
        for line in golden.lines() {
            let mut fields = line.split('\t');
            let mut field = || fields.next().unwrap();
            let (kind, value, expected) = (field(), field(), field());
            let value = decode(&unhex(value)).unwrap();
            let expected = decode(&unhex(expected)).unwrap();
            let ours = match kind {
                "empty" => Value::Bool(value.is_empty()),
                "is_numeric" => Value::Bool(value.is_numeric()),
                "float" => Value::Float(crate::Float::from_f64(value.to_float())),
                "int" => Value::Int(value.to_int()),
                "plus" => {
                    let number = string_to_number(value.as_bytes().unwrap());
                    let php = (!matches!(expected, Value::Null)).then(|| expected.to_float());
                    assert_eq!(number, php, "{value:?}");
                    continue;
                }
                _ => Value::String(value.to_php_string()),
            };
            assert_eq!(ours, expected, "{kind} {value:?}");
        }
        assert!(golden.lines().count() > 300);
    }

    #[test]
    fn objects_and_references() {
        let object = Value::Object {
            class: b"C".to_vec(),
            properties: Vec::new(),
        };
        assert!(!object.is_empty());
        assert!(!object.is_numeric());
        assert_eq!(object.to_float(), 1.0);
        assert_eq!(object.to_int(), 1);
        assert_eq!(absint(&Value::String(b"-7".to_vec())), 7);
        assert!(isset(Some(&Value::Bool(false))));
        assert!(!isset(Some(&Value::Null)));
        assert!(!isset(None));
        assert_eq!(object.to_php_string(), b"");
        assert_eq!(Value::Null.to_php_string(), b"");
        assert_eq!(Value::Bool(false).to_php_string(), b"");
    }
}
