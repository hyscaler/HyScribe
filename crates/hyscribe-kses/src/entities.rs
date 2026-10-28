// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_kses_normalize_entities()` and `wp_kses_decode_entities()`.

use crate::php::{replace_each, rest, starts_with_ci};

/// `$allowedentitynames`: the named references WordPress keeps.
const ENTITY_NAMES: [&str; 253] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "acute", "micro", "para", "middot",
    "cedil", "ordm", "raquo", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring",
    "AElig", "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml",
    "ETH", "Ntilde", "Ograve", "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave",
    "Uacute", "Ucirc", "Uuml", "Yacute", "THORN", "szlig", "agrave", "aacute", "acirc", "atilde",
    "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml", "igrave", "iacute",
    "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml", "divide",
    "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml", "quot", "amp", "lt",
    "gt", "apos", "OElig", "oelig", "Scaron", "scaron", "Yuml", "circ", "tilde", "ensp", "emsp",
    "thinsp", "zwnj", "zwj", "lrm", "rlm", "ndash", "mdash", "lsquo", "rsquo", "sbquo", "ldquo",
    "rdquo", "bdquo", "dagger", "Dagger", "permil", "lsaquo", "rsaquo", "euro", "fnof", "Alpha",
    "Beta", "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota", "Kappa", "Lambda", "Mu",
    "Nu", "Xi", "Omicron", "Pi", "Rho", "Sigma", "Tau", "Upsilon", "Phi", "Chi", "Psi", "Omega",
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
    "lambda", "mu", "nu", "xi", "omicron", "pi", "rho", "sigmaf", "sigma", "tau", "upsilon", "phi",
    "chi", "psi", "omega", "thetasym", "upsih", "piv", "bull", "hellip", "prime", "Prime", "oline",
    "frasl", "weierp", "image", "real", "trade", "alefsym", "larr", "uarr", "rarr", "darr", "harr",
    "crarr", "lArr", "uArr", "rArr", "dArr", "hArr", "forall", "part", "exist", "empty", "nabla",
    "isin", "notin", "ni", "prod", "sum", "minus", "lowast", "radic", "prop", "infin", "ang",
    "and", "or", "cap", "cup", "int", "sim", "cong", "asymp", "ne", "equiv", "le", "ge", "sub",
    "sup", "nsub", "sube", "supe", "oplus", "otimes", "perp", "sdot", "lceil", "rceil", "lfloor",
    "rfloor", "lang", "rang", "loz", "spades", "clubs", "hearts", "diams", "sup1", "sup2", "sup3",
    "frac14", "frac12", "frac34", "there4",
];

/// `valid_unicode()`: the code points XML allows.
fn valid_unicode(code: u64) -> bool {
    matches!(code, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// The leading run of `text` that `class` accepts.
fn run(text: &[u8], class: fn(&u8) -> bool) -> &[u8] {
    let len = text.iter().take_while(|b| class(b)).count();
    text.get(..len).unwrap_or_default()
}

/// The value of hexadecimal or decimal digits in `radix`, saturating far
/// above anything the callers accept.
fn value(digits: &[u8], radix: u64) -> u64 {
    digits.iter().fold(0, |value: u64, &digit| {
        let digit = char::from(digit).to_digit(16).map_or(0, u64::from);
        value.saturating_mul(radix).saturating_add(digit)
    })
}

/// `&amp;#(0*[1-9][0-9]{0,6});` for a valid code point, padded to three
/// digits. WordPress writes invalid, zero and overlong ones back as they
/// were, so they need not match.
fn decimal(text: &[u8]) -> Option<(usize, Vec<u8>)> {
    let digits = run(text.strip_prefix(b"&amp;#")?, u8::is_ascii_digit);
    let code = value(digits, 10);
    (rest(text, 6 + digits.len()).first() == Some(&b';') && valid_unicode(code))
        .then(|| (7 + digits.len(), format!("&#{code:03};").into_bytes()))
}

/// `&amp;#[Xx](0*[1-9A-Fa-f][0-9A-Fa-f]{0,5});`, rewritten with a
/// lowercase `x` whether valid or not.
fn hexadecimal(text: &[u8]) -> Option<(usize, Vec<u8>)> {
    if !starts_with_ci(text, b"&amp;#x") {
        return None;
    }
    let tail = rest(text, 7);
    let digits = run(tail, u8::is_ascii_hexdigit);
    let significant = rest(digits, run(digits, |&b| b == b'0').len());
    if !(1..=6).contains(&significant.len()) || rest(tail, digits.len()).first() != Some(&b';') {
        return None;
    }
    let out = if valid_unicode(value(significant, 16)) {
        [b"&#x", significant, b";"].concat()
    } else {
        [b"&amp;#x", digits, b";"].concat()
    };
    Some((8 + digits.len(), out))
}

/// `&amp;([A-Za-z]{2,8}[0-9]{0,2});` with an allowed name, decoded back.
/// Every allowed name has that shape, so membership is the whole test.
fn named(text: &[u8]) -> Option<(usize, Vec<u8>)> {
    let tail = text.strip_prefix(b"&amp;")?;
    let letters = run(tail, u8::is_ascii_alphabetic).len();
    let len = letters + run(rest(tail, letters), u8::is_ascii_digit).len();
    let name = tail.get(..len).unwrap_or_default();
    (rest(tail, len).first() == Some(&b';') && ENTITY_NAMES.iter().any(|n| n.as_bytes() == name))
        .then(|| (6 + len, [b"&", name, b";"].concat()))
}

/// `wp_kses_normalize_entities()` in its default HTML context: every `&`
/// becomes `&amp;` unless it starts an allowed named reference or a
/// numeric reference to a valid code point.
pub fn wp_kses_normalize_entities(content: &[u8]) -> Vec<u8> {
    let mut amped = Vec::with_capacity(content.len());
    for &byte in content {
        match byte {
            b'&' => amped.extend_from_slice(b"&amp;"),
            _ => amped.push(byte),
        }
    }
    let content = replace_each(&amped, decimal);
    let content = replace_each(&content, hexadecimal);
    replace_each(&content, named)
}

/// The byte PHP's `chr()` makes of a reference's value: its low byte.
/// Beyond `PHP_INT_MAX` PHP passes a float, and `chr()` throws a
/// `TypeError`; that sets `thrown`.
fn chr(digits: &[u8], radix: u64, thrown: &mut bool) -> Vec<u8> {
    let value = value(digits, radix);
    *thrown |= value > i64::MAX.unsigned_abs();
    vec![value as u8]
}

/// Whether `digits`, found `offset` bytes into `text`, are a reference's:
/// at least one, then `;`.
fn terminated(text: &[u8], offset: usize, digits: &[u8]) -> bool {
    !digits.is_empty() && rest(text, offset + digits.len()).first() == Some(&b';')
}

/// `wp_kses_decode_entities()`: decodes `&#65;`, and then `&#x41;`, to
/// bytes. `None` where PHP throws a `TypeError` (a value beyond
/// `PHP_INT_MAX`).
pub fn wp_kses_decode_entities(content: &[u8]) -> Option<Vec<u8>> {
    let mut thrown = false;
    let content = replace_each(content, |text| {
        let digits = run(text.strip_prefix(b"&#")?, u8::is_ascii_digit);
        terminated(text, 2, digits).then(|| (3 + digits.len(), chr(digits, 10, &mut thrown)))
    });
    let content = replace_each(&content, |text| {
        if !starts_with_ci(text, b"&#x") {
            return None;
        }
        let digits = run(rest(text, 3), u8::is_ascii_hexdigit);
        terminated(text, 3, digits).then(|| (4 + digits.len(), chr(digits, 16, &mut thrown)))
    });
    (!thrown).then_some(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(text: &str) -> String {
        String::from_utf8(wp_kses_normalize_entities(text.as_bytes())).unwrap()
    }

    #[test]
    fn valid_code_points_are_xml_chars() {
        let valid = [
            0x9, 0xA, 0xD, 0x20, 0xD7FF, 0xE000, 0xFFFD, 0x10000, 0x10FFFF,
        ];
        let invalid = [
            0, 0x8, 0xB, 0xC, 0xE, 0x1F, 0xD800, 0xDFFF, 0xFFFE, 0xFFFF, 0x110000,
        ];
        assert!(valid.iter().all(|&c| valid_unicode(c)));
        assert!(!invalid.iter().any(|&c| valid_unicode(c)));
    }

    #[test]
    fn values_saturate() {
        assert_eq!(value(b"0129", 10), 129);
        assert_eq!(value(b"aF", 16), 0xAF);
        assert_eq!(value(b"99999999999999999999999", 10), u64::MAX);
        assert_eq!(value(b"18446744073709551616", 10), u64::MAX);
    }

    #[test]
    fn disarms_bare_ampersands() {
        assert_eq!(normalize("AT&T & &&"), "AT&amp;T &amp; &amp;&amp;");
        assert_eq!(normalize("&amp; &amp;amp;"), "&amp; &amp;amp;");
        assert_eq!(normalize("no entities"), "no entities");
    }

    #[test]
    fn keeps_allowed_names_only() {
        assert_eq!(
            normalize("&hellip;&apos;&frac14;&there4;&thetasym;"),
            "&hellip;&apos;&frac14;&there4;&thetasym;"
        );
        assert_eq!(
            normalize("&Hellip;&frac145;&nbsp&nbsp ;&a;"),
            "&amp;Hellip;&amp;frac145;&amp;nbsp&amp;nbsp ;&amp;a;"
        );
    }

    #[test]
    fn pads_valid_decimal_references() {
        assert_eq!(
            normalize("&#9;&#65;&#0065;&#1114111;"),
            "&#009;&#065;&#065;&#1114111;"
        );
        assert_eq!(
            normalize("&#0;&#8;&#55296;&#1114112;&#65&#;"),
            "&amp;#0;&amp;#8;&amp;#55296;&amp;#1114112;&amp;#65&amp;#;"
        );
        assert_eq!(
            normalize("&#99999999999999999999;"),
            "&amp;#99999999999999999999;"
        );
    }

    #[test]
    fn lowercases_hexadecimal_references() {
        assert_eq!(
            normalize("&#x41;&#X41;&#x0041;&#X10FFFF;"),
            "&#x41;&#x41;&#x41;&#x10FFFF;"
        );
        assert_eq!(normalize("&#XD800;&#xfffe;"), "&amp;#xD800;&amp;#xfffe;");
        assert_eq!(
            normalize("&#X000;&#X1234567;&#x41&#x;"),
            "&amp;#X000;&amp;#X1234567;&amp;#x41&amp;#x;"
        );
    }

    #[test]
    fn decodes_numeric_references_to_bytes() {
        let decode = |text: &str| wp_kses_decode_entities(text.as_bytes());
        assert_eq!(decode("&#65;&#x42;&#X43;&#0068;"), Some(b"ABCD".to_vec()));
        assert_eq!(decode("&#38;#x41;&#321;&#256;"), Some(b"AA\0".to_vec()));
        assert_eq!(
            decode("&#;&#x;&#65&#x41&amp;"),
            Some(b"&#;&#x;&#65&#x41&amp;".to_vec())
        );
        assert_eq!(
            decode("&#9223372036854775807;&#x7FFFFFFFFFFFFFFF;"),
            Some(vec![0xFF, 0xFF])
        );
        assert_eq!(decode("&#9223372036854775808;"), None);
        assert_eq!(decode("&#x8000000000000000;"), None);
        assert_eq!(decode("&#99999999999999999999;"), None);
        assert_eq!(
            decode("&#99999999999999999999"),
            Some(b"&#99999999999999999999".to_vec())
        );
    }
}
