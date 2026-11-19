// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's shortcode engine from `wp-includes/shortcodes.php`:
//! `get_shortcode_regex()` matched by hand, `shortcode_parse_atts()`,
//! `shortcode_atts()`, `do_shortcode()`'s replacing, `strip_shortcodes()`
//! and `shortcode_unautop()`.
//!
//! Every pattern is matched with PCRE's semantics (leftmost match, the
//! possessive and lazy quantifiers of core's regex) in linear time: the
//! positions of every `]` and of every closing tag are found once, and
//! each opening bracket looks its partner up in them.
//!
//! Not ported: a shortcode inside an HTML attribute (`do_shortcodes_in_html_tags()`
//! parses attributes with `wp_kses_hair_parse()`): it is left as written.

use hyscribe_phpser::{Key, Value};

use crate::autop::html_elements;
use crate::formatting::find;

/// The deepest nesting of shortcodes a handler runs: a post of nested
/// `[caption]`s must not exhaust the stack. PHP has no limit.
pub const MAX_DEPTH: usize = 64;

/// One match of `get_shortcode_regex()`.
pub struct Shortcode<'a> {
    /// The offset of the match in the text.
    pub start: usize,
    /// The offset after the match.
    pub end: usize,
    /// `$m[1] === '['`: a second opening bracket, which escapes the tag.
    pub open: bool,
    /// `$m[2]`: the tag name.
    pub tag: &'a [u8],
    /// `$m[3]`: what the opening tag holds after its name.
    pub atts: &'a [u8],
    /// `$m[5]`: what lies between the opening and closing tags, if the
    /// closing tag is there.
    pub content: Option<&'a [u8]>,
    /// `$m[6] === ']'`: a second closing bracket.
    pub close: bool,
    /// The match as written, `$m[0]`.
    pub whole: &'a [u8],
}

impl Shortcode<'_> {
    /// `[[tag]]`: the escape `do_shortcode_tag()` and `strip_shortcode_tag()`
    /// unwrap, `substr( $m[0], 1, -1 )`.
    pub fn escaped(&self) -> bool {
        self.open && self.close
    }

    /// `substr( $m[0], 1, -1 )`.
    pub fn unwrapped(&self) -> &[u8] {
        self.whole
            .get(1..self.whole.len().saturating_sub(1))
            .unwrap_or_default()
    }
}

/// The positions the matching looks up.
struct Scanner<'a> {
    text: &'a [u8],
    tags: &'a [&'a [u8]],
    /// Every `]`.
    closers: Vec<usize>,
    /// Every `[/tag]` of each tag, by tag.
    ends: Vec<Vec<usize>>,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a [u8], tags: &'a [&'a [u8]]) -> Scanner<'a> {
        let closers = positions(text, b"]");
        let ends = tags
            .iter()
            .map(|tag| positions(text, &[&b"[/"[..], tag, b"]"].concat()))
            .collect();
        Scanner {
            text,
            tags,
            closers,
            ends,
        }
    }

    /// The first position of `list` at or after `from`.
    fn after(list: &[usize], from: usize) -> Option<usize> {
        list.get(list.partition_point(|&at| at < from)).copied()
    }

    /// The tags `(?:tag1|tag2)(?![\w-])` can take at `at`, in order, each
    /// with the offset after its name.
    fn tags_at(&self, at: usize) -> impl Iterator<Item = (usize, usize)> + '_ {
        let rest = self.text.get(at..).unwrap_or_default();
        self.tags.iter().enumerate().filter_map(move |(i, tag)| {
            let next = rest.get(tag.len()).copied();
            let whole = rest.starts_with(tag) && !next.is_some_and(|b| is_word(b) || b == b'-');
            whole.then_some((i, at + tag.len()))
        })
    }

    /// The opening tag of tag `tag` at `at` (just after its name): where
    /// its attributes end, and whether it closes itself, `/]`.
    fn opening(&self, at: usize) -> Option<(usize, bool)> {
        let close = Self::after(&self.closers, at)?;
        // A `/` before a tag's name ends cannot belong to the name.
        let slash = self.text.get(close - 1) == Some(&b'/');
        Some((if slash { close - 1 } else { close }, slash))
    }

    /// `get_shortcode_regex()` at the `[` at `i`.
    fn matches(&self, i: usize) -> Option<Shortcode<'a>> {
        let open = self.text.get(i + 1) == Some(&b'[');
        let name = i + 1 + usize::from(open);
        for (tag_index, after) in self.tags_at(name) {
            let tag = self.tags.get(tag_index).copied().unwrap_or_default();
            let Some((attributes_end, self_closing)) = self.opening(after) else {
                continue;
            };
            let atts = self.text.get(after..attributes_end).unwrap_or_default();
            let (content, end) = if self_closing {
                (None, attributes_end + 2)
            } else {
                let body = attributes_end + 1;
                let closing = self
                    .ends
                    .get(tag_index)
                    .and_then(|ends| Self::after(ends, body));
                match closing {
                    Some(closing) => (self.text.get(body..closing), closing + tag.len() + 3),
                    None => (None, body),
                }
            };
            let close = self.text.get(end) == Some(&b']');
            let end = end + usize::from(close);
            return Some(Shortcode {
                start: i,
                end,
                open,
                tag,
                atts,
                content,
                close,
                whole: self.text.get(i..end).unwrap_or_default(),
            });
        }
        None
    }
}

/// Every offset of `needle` in `text`, overlapping or not.
pub(crate) fn positions(text: &[u8], needle: &[u8]) -> Vec<usize> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(next) = find(text, at, needle) {
        found.push(next);
        at = next + 1;
    }
    found
}

/// `strtr( $text, $pairs )` for keys that are not prefixes of one another.
fn strtr(text: &[u8], pairs: &[(&[u8], &[u8])]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    'scan: while let Some(rest) = text.get(i..).filter(|rest| !rest.is_empty()) {
        for (from, to) in pairs {
            if rest.starts_with(from) {
                out.extend_from_slice(to);
                i += from.len();
                continue 'scan;
            }
        }
        out.extend(rest.first());
        i += 1;
    }
    out
}

/// `\w`, as PCRE reads it without `/u`.
fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The names `do_shortcode()` finds after an opening bracket with
/// `\[([^<>&/\[\]\x00-\x20=]++)`, in order of appearance.
fn names_in(content: &[u8]) -> Vec<&[u8]> {
    let mut names = Vec::new();
    let mut at = 0;
    while let Some(open) = find(content, at, b"[") {
        let rest = content.get(open + 1..).unwrap_or_default();
        let len = rest
            .iter()
            .take_while(|&&b| {
                !matches!(b, b'<' | b'>' | b'&' | b'/' | b'[' | b']' | b'=' | 0..=0x20)
            })
            .count();
        names.extend(rest.get(..len).filter(|name| !name.is_empty()));
        at = open + 1;
    }
    names
}

/// `array_intersect( array_keys( $shortcode_tags ), $matches[1] )`: the
/// registered tags that appear after a bracket, in registration order.
pub fn tags_in<'t>(registered: &[&'t [u8]], content: &[u8]) -> Vec<&'t [u8]> {
    if !content.contains(&b'[') {
        return Vec::new();
    }
    let names = names_in(content);
    registered
        .iter()
        .filter(|tag| names.contains(tag))
        .copied()
        .collect()
}

/// `preg_replace_callback( "/$pattern/", $callback, $text )` over
/// `get_shortcode_regex( $tags )`.
pub fn replace_shortcodes(
    text: &[u8],
    tags: &[&[u8]],
    callback: &mut dyn FnMut(&Shortcode<'_>) -> Vec<u8>,
) -> Vec<u8> {
    if tags.is_empty() {
        return text.to_vec();
    }
    let scanner = Scanner::new(text, tags);
    let mut out = Vec::with_capacity(text.len());
    let (mut at, mut from) = (0, 0);
    while let Some(open) = find(text, from, b"[") {
        match scanner.matches(open) {
            Some(shortcode) => {
                out.extend_from_slice(text.get(at..open).unwrap_or_default());
                out.extend(callback(&shortcode));
                at = shortcode.end;
                from = shortcode.end;
            }
            None => from = open + 1,
        }
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

/// `do_shortcodes_in_html_tags( $content, $ignore_html, $tagnames )`: the
/// brackets inside HTML elements are escaped, so `replace_shortcodes()`
/// leaves them as they are, and shortcodes in attributes stay as written.
pub fn escape_brackets_in_elements(content: &[u8]) -> Vec<u8> {
    let content = strtr(
        content,
        &[
            (&b"&#91;"[..], &b"&#091;"[..]),
            (&b"&#93;"[..], &b"&#093;"[..]),
        ],
    );
    let mut out = Vec::with_capacity(content.len());
    let mut at = 0;
    for (start, end) in html_elements(&content) {
        out.extend_from_slice(content.get(at..start).unwrap_or_default());
        let element = content.get(start..end).unwrap_or_default();
        out.extend(strtr(
            element,
            &[(&b"["[..], &b"&#91;"[..]), (&b"]"[..], &b"&#93;"[..])],
        ));
        at = end;
    }
    out.extend_from_slice(content.get(at..).unwrap_or_default());
    out
}

/// `unescape_invalid_shortcodes( $content )`.
pub fn unescape_invalid_shortcodes(content: &[u8]) -> Vec<u8> {
    strtr(
        content,
        &[(&b"&#91;"[..], &b"["[..]), (&b"&#93;"[..], &b"]"[..])],
    )
}

/// `strip_shortcodes( $content )` over the `registered` tags.
pub fn strip_shortcodes(content: &[u8], registered: &[&[u8]]) -> Vec<u8> {
    let tags = tags_in(registered, content);
    if tags.is_empty() {
        return content.to_vec();
    }
    let content = escape_brackets_in_elements(content);
    let stripped = replace_shortcodes(&content, &tags, &mut |shortcode| {
        if shortcode.escaped() {
            shortcode.unwrapped().to_vec()
        } else {
            let pair = [
                if shortcode.open { &b"["[..] } else { b"" },
                if shortcode.close { b"]" } else { b"" },
            ];
            pair.concat()
        }
    });
    unescape_invalid_shortcodes(&stripped)
}

/// The spaces `shortcode_unautop()` skips, `wp_spaces_regexp()`: one of
/// `\r\n\t `, a no-break space, or `&nbsp;`.
fn skip_spaces(text: &[u8], mut at: usize) -> usize {
    loop {
        let rest = text.get(at..).unwrap_or_default();
        at += if rest.starts_with(b"&nbsp;") {
            6
        } else if rest.starts_with(b"\xC2\xA0") {
            2
        } else if matches!(rest.first(), Some(b'\r' | b'\n' | b'\t' | b' ')) {
            1
        } else {
            return at;
        };
    }
}

/// `shortcode_unautop( $text )` over the `registered` tags: a paragraph
/// holding only a shortcode, with the spaces around it, is the shortcode.
pub fn shortcode_unautop(text: &[u8], registered: &[&[u8]]) -> Vec<u8> {
    if registered.is_empty() {
        return text.to_vec();
    }
    let scanner = Scanner::new(text, registered);
    let mut out = Vec::with_capacity(text.len());
    let (mut at, mut from) = (0, 0);
    while let Some(open) = find(text, from, b"<p>") {
        match unwrapped_in_paragraph(&scanner, open) {
            Some((shortcode, end)) => {
                out.extend_from_slice(text.get(at..open).unwrap_or_default());
                out.extend_from_slice(shortcode);
                at = end;
                from = end;
            }
            None => from = open + 1,
        }
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

/// The shortcode and the end of the paragraph `shortcode_unautop()`'s
/// pattern matches at the `<p>` at `open`.
fn unwrapped_in_paragraph<'a>(scanner: &Scanner<'a>, open: usize) -> Option<(&'a [u8], usize)> {
    let text = scanner.text;
    let bracket = skip_spaces(text, open + 3);
    if text.get(bracket) != Some(&b'[') {
        return None;
    }
    let paragraph_end = |end: usize| {
        let end = skip_spaces(text, end);
        text.get(end..)
            .is_some_and(|rest| rest.starts_with(b"</p>"))
            .then_some(end + 4)
    };
    for (tag_index, after) in scanner.tags_at(bracket + 1) {
        let tag = scanner.tags.get(tag_index).copied().unwrap_or_default();
        let Some((attributes_end, self_closing)) = scanner.opening(after) else {
            continue;
        };
        if self_closing {
            let shortcode_end = attributes_end + 2;
            if let Some(end) = paragraph_end(shortcode_end) {
                return Some((text.get(bracket..shortcode_end).unwrap_or_default(), end));
            }
            continue;
        }
        let body = attributes_end + 1;
        // With the content and closing tag, then, as the group is
        // optional, without them.
        let closing = scanner
            .ends
            .get(tag_index)
            .and_then(|ends| Scanner::after(ends, body))
            .map(|closing| closing + tag.len() + 3);
        for shortcode_end in closing.into_iter().chain([body]) {
            if let Some(end) = paragraph_end(shortcode_end) {
                return Some((text.get(bracket..shortcode_end).unwrap_or_default(), end));
            }
        }
    }
    None
}

/// `shortcode_parse_atts( $text )`: the attributes as a PHP array, named
/// ones by their lower case name and the others by position. Text that is
/// not UTF-8 gives none, as `preg_replace()` with `/u` fails on it.
pub fn shortcode_parse_atts(text: &[u8]) -> Vec<(Key, Value)> {
    let mut atts: Vec<(Key, Value)> = Vec::new();
    let Ok(text) = std::str::from_utf8(text) else {
        return atts;
    };
    let text = replace_spaces(text);
    let mut at = 0;
    while text.get(at).is_some() {
        let Some((token, end)) = attribute_at(&text, at) else {
            at += 1;
            continue;
        };
        at = end;
        match token {
            Token::Named(name, value) => {
                // `! empty( $m[1] )`: the name `0` is empty too.
                if !(name.is_empty() || name == b"0") {
                    let key = Key::from_php_string(name.to_ascii_lowercase());
                    set(&mut atts, key, stripcslashes(value));
                }
            }
            Token::Positional(value) => {
                if !value.is_empty() {
                    push(&mut atts, stripcslashes(value));
                }
            }
        }
    }
    for (_, value) in &mut atts {
        if let Value::String(text) = value
            && text.contains(&b'<')
            && !balanced_angle_brackets(text)
        {
            text.clear();
        }
    }
    atts
}

/// `preg_replace( "/[\x{00a0}\x{200b}]+/u", ' ', $text )`.
fn replace_spaces(text: &str) -> Vec<u8> {
    let mut out = String::with_capacity(text.len());
    let mut in_run = false;
    for c in text.chars() {
        if c == '\u{a0}' || c == '\u{200b}' {
            if !in_run {
                out.push(' ');
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out.into_bytes()
}

/// An attribute `get_shortcode_atts_regex()` finds.
enum Token<'a> {
    /// `name=value`, quoted or not.
    Named(&'a [u8], &'a [u8]),
    /// A quoted or bare word.
    Positional(&'a [u8]),
}

/// PCRE's `\s` without `/u`.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The attribute `get_shortcode_atts_regex()` matches at `at`, and the
/// offset after it and its one space.
fn attribute_at(text: &[u8], at: usize) -> Option<(Token<'_>, usize)> {
    let byte = |i: usize| text.get(i).copied();
    // `(?:\s|$)`: a space, or the end; the space itself is skipped by the
    // caller's next step.
    let boundary = |i: usize| byte(i).is_none_or(is_space);
    let skip = |mut i: usize| {
        while byte(i).is_some_and(is_space) {
            i += 1;
        }
        i
    };
    let name_len = text
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| is_word(b) || b == b'-')
        .count();
    if name_len > 0 {
        let name = text.get(at..at + name_len).unwrap_or_default();
        let equals = skip(at + name_len);
        if byte(equals) == Some(b'=') {
            let start = skip(equals + 1);
            for quote in *b"\"'" {
                if byte(start) == Some(quote)
                    && let Some(close) = find(text, start + 1, &[quote])
                    && boundary(close + 1)
                {
                    let value = text.get(start + 1..close).unwrap_or_default();
                    return Some((Token::Named(name, value), close + 1));
                }
            }
            let len = text
                .get(start..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| !is_space(b) && b != b'\'' && b != b'"')
                .count();
            if len > 0 && boundary(start + len) {
                let value = text.get(start..start + len).unwrap_or_default();
                return Some((Token::Named(name, value), start + len));
            }
        }
    }
    for quote in *b"\"'" {
        if byte(at) == Some(quote)
            && let Some(close) = find(text, at + 1, &[quote])
            && boundary(close + 1)
        {
            let value = text.get(at + 1..close).unwrap_or_default();
            return Some((Token::Positional(value), close + 1));
        }
    }
    let len = text
        .get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| !is_space(b))
        .count();
    let word = text.get(at..at + len).filter(|word| !word.is_empty())?;
    Some((Token::Positional(word), at + len))
}

/// `$atts[ $key ] = $value`: in place if the key is there.
fn set(atts: &mut Vec<(Key, Value)>, key: Key, value: Vec<u8>) {
    match atts.iter_mut().find(|(known, _)| *known == key) {
        Some(found) => found.1 = Value::String(value),
        None => atts.push((key, Value::String(value))),
    }
}

/// `$atts[] = $value`: under the next free integer key.
fn push(atts: &mut Vec<(Key, Value)>, value: Vec<u8>) {
    let next = atts
        .iter()
        .filter_map(|(key, _)| match key {
            Key::Int(n) => Some(*n),
            Key::String(_) => None,
        })
        .max()
        .map_or(0, |n| n.saturating_add(1));
    atts.push((Key::Int(next), Value::String(value)));
}

/// `1 === preg_match( '/^[^<]*+(?:<[^>]*+>[^<]*+)*+$/', $value )`: every
/// `<` closed by a `>` before the next `<`.
fn balanced_angle_brackets(text: &[u8]) -> bool {
    let mut at = 0;
    while let Some(open) = find(text, at, b"<") {
        let Some(close) = find(text, open + 1, b">") else {
            return false;
        };
        at = close + 1;
    }
    true
}

/// `stripcslashes( $text )`.
pub fn stripcslashes(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while let Some(&byte) = text.get(i) {
        let Some(&next) = text.get(i + 1).filter(|_| byte == b'\\') else {
            out.push(byte);
            i += 1;
            continue;
        };
        i += 2;
        match next {
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'a' => out.push(7),
            b'v' => out.push(0x0b),
            b'b' => out.push(8),
            b'f' => out.push(0x0c),
            b'\\' => out.push(b'\\'),
            b'x' if text.get(i).is_some_and(|b| b.is_ascii_hexdigit()) => {
                let digits = text
                    .get(i..)
                    .unwrap_or_default()
                    .iter()
                    .take(2)
                    .take_while(|b| b.is_ascii_hexdigit())
                    .count();
                let hex = std::str::from_utf8(text.get(i..i + digits).unwrap_or_default());
                out.push(u8::from_str_radix(hex.unwrap_or("0"), 16).unwrap_or(0));
                i += digits;
            }
            _ => {
                // Up to three octal digits starting at the character after
                // the backslash, else that character itself.
                let from = i - 1;
                let digits = text
                    .get(from..)
                    .unwrap_or_default()
                    .iter()
                    .take(3)
                    .take_while(|b| (b'0'..=b'7').contains(b))
                    .count();
                if digits == 0 {
                    out.push(next);
                } else {
                    let octal =
                        std::str::from_utf8(text.get(from..from + digits).unwrap_or_default());
                    // `(char) strtol()`: modulo 256.
                    out.push(
                        (u32::from_str_radix(octal.unwrap_or("0"), 8).unwrap_or(0) % 256) as u8,
                    );
                    i = from + digits;
                }
            }
        }
    }
    out
}

/// `shortcode_atts( $pairs, $atts )`, before the `shortcode_atts_{$shortcode}`
/// filter: each pair's value from the attributes when they have its name.
pub fn shortcode_atts(pairs: &[(&str, Value)], atts: &[(Key, Value)]) -> Vec<(String, Value)> {
    pairs
        .iter()
        .map(|(name, default)| {
            let key = Key::from_php_string(name.as_bytes().to_vec());
            let value = atts
                .iter()
                .find(|(known, _)| *known == key)
                .map_or_else(|| default.clone(), |(_, value)| value.clone());
            ((*name).to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests;
