// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_specialchars_decode()`.

/// `wp_specialchars_decode( $text, $quote_style )`: `&amp;`, `&lt;` and
/// `&gt;` decoded, with `&quot;` and `&#039;` too when `quotes`
/// (`ENT_QUOTES`, else `ENT_NOQUOTES`); numeric forms with any zero
/// padding, other entities kept.
pub fn wp_specialchars_decode(text: &[u8], quotes: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some((&byte, after)) = rest.split_first() {
        match entity(rest, quotes) {
            Some((length, decoded)) => {
                out.push(decoded);
                rest = rest.get(length..).unwrap_or_default();
            }
            None => {
                out.push(byte);
                rest = after;
            }
        }
    }
    out
}

/// The length and character of the entity `text` starts with, if decoded.
fn entity(text: &[u8], quotes: bool) -> Option<(usize, u8)> {
    for (name, decoded) in [
        (&b"&amp;"[..], b'&'),
        (b"&lt;", b'<'),
        (b"&gt;", b'>'),
        (b"&quot;", b'"'),
    ] {
        if text.starts_with(name) && (quotes || decoded != b'"') {
            return Some((name.len(), decoded));
        }
    }
    let number = text.strip_prefix(b"&#")?;
    let (number, hex) = match number.strip_prefix(b"x").or(number.strip_prefix(b"X")) {
        Some(digits) => (digits, true),
        None => (number, false),
    };
    let zeros = number.iter().take_while(|&&b| b == b'0').count();
    let decoded = match (hex, number.get(zeros..zeros + 3)?) {
        (false, b"38;") | (true, b"26;") => b'&',
        (false, b"60;") => b'<',
        (false, b"62;") => b'>',
        (false, b"34;") | (true, b"22;") => b'"',
        (false, b"39;") | (true, b"27;") => b'\'',
        _ => return None,
    };
    let quote = matches!(decoded, b'"' | b'\'');
    (quotes || !quote).then_some((text.len() - number.len() + zeros + 3, decoded))
}
