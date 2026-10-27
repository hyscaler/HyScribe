// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

/// A PHP float as it appears after `d:`.
///
/// The text is kept so that values written with a different
/// `serialize_precision` (older PHP wrote `0.10000000000000001`) re-encode
/// byte for byte. New values made with [`Float::from_f64`] use PHP 7.1+
/// default formatting (`serialize_precision = -1`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Float(String);

impl Float {
    /// Accepts the text between `d:` and `;` if PHP's `unserialize()` would.
    pub fn parse(text: &[u8]) -> Option<Float> {
        if valid(text) {
            // `valid` admits ASCII only.
            String::from_utf8(text.to_vec()).ok().map(Float)
        } else {
            None
        }
    }

    /// Formats a float the way PHP's `serialize()` does by default.
    pub fn from_f64(value: f64) -> Float {
        Float(php_format(value))
    }

    /// The serialized text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The numeric value.
    pub fn to_f64(&self) -> f64 {
        match self.0.as_str() {
            "NAN" => f64::NAN,
            "INF" => f64::INFINITY,
            "-INF" => f64::NEG_INFINITY,
            // Every other text passed `valid`, which is a subset of what Rust
            // parses.
            text => text.parse().unwrap_or(f64::NAN),
        }
    }
}

impl fmt::Display for Float {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// PHP's grammar: `[+-]? (digits | digits? "." digits | digits "." digits?)
/// ([eE] [+-]? digits)?`, or `NAN`, `INF`, `-INF`.
fn valid(text: &[u8]) -> bool {
    if matches!(text, b"NAN" | b"INF" | b"-INF") {
        return true;
    }
    let mut rest = text;
    if let [b'+' | b'-', tail @ ..] = rest {
        rest = tail;
    }
    let int_digits = leading_digits(rest);
    rest = rest.get(int_digits..).unwrap_or_default();
    let mut frac_digits = 0;
    if let [b'.', tail @ ..] = rest {
        frac_digits = leading_digits(tail);
        rest = tail.get(frac_digits..).unwrap_or_default();
    }
    if int_digits + frac_digits == 0 {
        return false;
    }
    if let [b'e' | b'E', tail @ ..] = rest {
        rest = tail;
        if let [b'+' | b'-', tail @ ..] = rest {
            rest = tail;
        }
        let exp_digits = leading_digits(rest);
        if exp_digits == 0 {
            return false;
        }
        rest = rest.get(exp_digits..).unwrap_or_default();
    }
    rest.is_empty()
}

fn leading_digits(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|b| b.is_ascii_digit()).count()
}

/// PHP's `serialize_precision = -1` output: the shortest digits that round
/// trip, in fixed notation when the decimal exponent is in `-4..17`,
/// otherwise as `d.dddE+x` with at least one fractional digit.
fn php_format(value: f64) -> String {
    if value.is_nan() {
        return "NAN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-INF"
        } else {
            "INF"
        }
        .into();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    let (digits, exp) = shortest_digits(value.abs());

    if !(-4..17).contains(&exp) {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { "0" } else { rest };
        let exp_sign = if exp.is_negative() { '-' } else { '+' };
        return format!("{sign}{first}.{rest}E{exp_sign}{}", exp.unsigned_abs());
    }
    if exp < 0 {
        let zeros = "0".repeat(exp.unsigned_abs() as usize - 1);
        return format!("{sign}0.{zeros}{digits}");
    }
    let point = exp as usize + 1;
    if digits.len() <= point {
        let zeros = "0".repeat(point - digits.len());
        format!("{sign}{digits}{zeros}")
    } else {
        let (int, frac) = digits.split_at(point);
        format!("{sign}{int}.{frac}")
    }
}

/// Shortest digits that round trip, and the decimal exponent of the first.
///
/// Rust and PHP agree except when two candidates are equally close to the
/// exact binary value: Rust rounds the last digit up, PHP's `zend_dtoa` picks
/// the even digit.
fn shortest_digits(value: f64) -> (String, i32) {
    let (digits, exp) = split_sci(&format!("{value:e}"));
    // Every double has an exact decimal expansion of at most 767 digits, so
    // this is the exact value, with the same leading digit and exponent.
    let (exact, _) = split_sci(&format!("{value:.800e}"));
    let (truncated, tail) = exact.split_at(digits.len());
    // In a tie both candidates are equally close, so both round trip.
    let tie = tail
        .strip_prefix('5')
        .is_some_and(|rest| rest.bytes().all(|b| b == b'0'));
    let even = truncated
        .bytes()
        .last()
        .is_some_and(|b| matches!(b, b'0' | b'2' | b'4' | b'6' | b'8'));
    if tie && even {
        (truncated.to_string(), exp)
    } else {
        (digits, exp)
    }
}

/// "1.25e-7" to ("125", -7).
fn split_sci(sci: &str) -> (String, i32) {
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci, "0"));
    let digits = mantissa.chars().filter(char::is_ascii_digit).collect();
    (digits, exp.parse().unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_like_php() {
        // Expected values from PHP 8.4 `serialize()`.
        let cases: [(f64, &str); 19] = [
            (1.0, "1"),
            (0.1, "0.1"),
            (-0.0, "-0"),
            (0.0, "0"),
            (1e25, "1.0E+25"),
            (1e17, "1.0E+17"),
            (1e16, "10000000000000000"),
            (123456789012345678.0, "1.2345678901234568E+17"),
            (0.0001, "0.0001"),
            (0.00001, "1.0E-5"),
            (1.5e-7, "1.5E-7"),
            (-2.5, "-2.5"),
            (100.0, "100"),
            (1.0 / 3.0, "0.3333333333333333"),
            (f64::MAX, "1.7976931348623157E+308"),
            (f64::MIN_POSITIVE, "2.2250738585072014E-308"),
            (5e-324, "5.0E-324"),
            (f64::INFINITY, "INF"),
            (f64::NEG_INFINITY, "-INF"),
        ];
        for (value, text) in cases {
            assert_eq!(Float::from_f64(value).as_str(), text, "{value:e}");
        }
        assert_eq!(Float::from_f64(f64::NAN).as_str(), "NAN");
    }

    #[test]
    fn ties_round_to_even_like_php() {
        // The nearest double is exactly 676144192310117.25; PHP writes .2 where Rust's shortest
        // formatting gives .3.
        assert_eq!(
            Float::from_f64(676_144_192_310_117.2).as_str(),
            "676144192310117.2"
        );
        // A tie whose even candidate is the one Rust already chose.
        assert_eq!(Float::from_f64(0.125).as_str(), "0.125");
        assert_eq!(Float::from_f64(2.5).as_str(), "2.5");
    }

    #[test]
    fn accepts_php_grammar() {
        for ok in [
            "1",
            "-1",
            "+1",
            "1.5",
            ".5",
            "5.",
            "1e5",
            "1E+5",
            "1.0E-5",
            "-.5e-3",
            "NAN",
            "INF",
            "-INF",
            "0.10000000000000001",
        ] {
            assert!(Float::parse(ok.as_bytes()).is_some(), "{ok}");
        }
        for bad in [
            "", "+", "-", ".", "e5", "1e", "1e+", "1.5.5", "1x", "+INF", "-NAN", "nan", "1 ", " 1",
            "\u{e9}",
        ] {
            assert!(Float::parse(bad.as_bytes()).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn to_f64_reads_every_form() {
        let read = |t: &str| Float::parse(t.as_bytes()).map(|f| f.to_f64());
        assert_eq!(read("1.0E-5"), Some(0.00001));
        assert_eq!(read(".5"), Some(0.5));
        assert_eq!(read("5."), Some(5.0));
        assert_eq!(read("+2"), Some(2.0));
        assert_eq!(read("INF"), Some(f64::INFINITY));
        assert_eq!(read("-INF"), Some(f64::NEG_INFINITY));
        assert!(read("NAN").is_some_and(f64::is_nan));
    }

    #[test]
    fn displays_its_text() {
        assert_eq!(Float::from_f64(2.5).to_string(), "2.5");
    }
}
