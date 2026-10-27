// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_HTML_Tag_Processor`: finds tags in HTML and edits their attributes
//! and classes in place, leaving every other byte alone.
//!
//! The port follows the PHP closely, down to its quirks: the first of
//! duplicate attributes wins and the others go only when it is removed;
//! new attributes are inserted after the tag name sorted by their text;
//! class edits are applied as one rewrite of the `class` attribute; and
//! the pending edits behave like the PHP array they are kept in, numeric
//! attribute names included.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::mem;

use crate::decoder::{REPLACEMENT, decode_attribute};
use crate::url::esc_url;

/// HTML whitespace.
const WHITESPACE: &[u8] = b" \t\x0c\r\n";

/// The attributes `set_attribute()` passes through `esc_url()`:
/// `wp_kses_uri_attributes()`.
const URI_ATTRIBUTES: [&[u8]; 17] = [
    b"action",
    b"archive",
    b"background",
    b"cite",
    b"classid",
    b"codebase",
    b"data",
    b"formaction",
    b"href",
    b"icon",
    b"longdesc",
    b"manifest",
    b"poster",
    b"profile",
    b"src",
    b"usemap",
    b"xmlns",
];

/// More pending edits than this are applied before the next token, as
/// WordPress does to bound its bookkeeping.
const MAX_PENDING_UPDATES: usize = 1000;

/// An attribute's value as `get_attribute()` returns it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Attribute {
    /// An attribute without a value, PHP's `true`.
    True,
    /// The value, with its character references decoded.
    Value(Vec<u8>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Ready,
    Complete,
    Incomplete,
    MatchedTag,
    /// Text, a comment, a doctype or another token that is not a tag.
    OtherToken,
}

#[derive(Clone, Copy, Debug)]
struct Span {
    start: usize,
    length: usize,
}

#[derive(Clone, Copy, Debug)]
struct AttributeToken {
    value_starts_at: usize,
    value_length: usize,
    span: Span,
    is_true: bool,
}

#[derive(Clone, Debug)]
struct Replacement {
    start: usize,
    length: usize,
    text: Vec<u8>,
}

/// A PHP array key: a string of a canonical integer becomes the integer.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Int(i64),
    Str(Vec<u8>),
}

impl Key {
    fn of(name: &[u8]) -> Key {
        std::str::from_utf8(name)
            .ok()
            .and_then(|text| text.parse::<i64>().ok())
            .filter(|n| n.to_string().as_bytes() == name)
            .map_or_else(|| Key::Str(name.to_vec()), Key::Int)
    }
}

/// The PHP array of pending edits: keyed by attribute name for the current
/// tag, by integer once the tag is left behind.
#[derive(Debug, Default)]
struct Updates {
    entries: Vec<(Key, Replacement)>,
    /// PHP's next free integer key; `None` until an integer key is used.
    next_free: Option<i64>,
}

impl Updates {
    fn get(&self, key: &Key) -> Option<&Replacement> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, r)| r)
    }

    fn insert(&mut self, key: Key, replacement: Replacement) {
        if let Key::Int(n) = key
            && self.next_free.is_none_or(|next| n >= next)
        {
            self.next_free = Some(n.saturating_add(1));
        }
        match self.entries.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = replacement,
            None => self.entries.push((key, replacement)),
        }
    }

    fn append(&mut self, replacement: Replacement) {
        let key = Key::Int(self.next_free.unwrap_or(0));
        self.insert(key, replacement);
    }

    fn remove(&mut self, key: &Key) {
        self.entries.retain(|(k, _)| k != key);
    }

    /// Moves the entries named after attributes to integer keys.
    fn rekey(&mut self) {
        let (named, kept): (Vec<_>, Vec<_>) = mem::take(&mut self.entries)
            .into_iter()
            .partition(|(key, _)| matches!(key, Key::Str(_)));
        self.entries = kept;
        for (_, replacement) in named {
            self.append(replacement);
        }
    }
}

/// `strspn()`: how many bytes from `at` are in `set`.
fn span_of(html: &[u8], at: usize, set: &[u8]) -> usize {
    html.get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| set.contains(b))
        .count()
}

/// `strcspn()`: how many bytes from `at` are not in `set`.
fn span_until(html: &[u8], at: usize, set: &[u8]) -> usize {
    html.get(at..)
        .unwrap_or_default()
        .iter()
        .take_while(|b| !set.contains(b))
        .count()
}

/// `strpos()`: where `needle` next occurs at or after `at`.
fn find(html: &[u8], at: usize, needle: &[u8]) -> Option<usize> {
    html.get(at..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| at + p)
}

/// PHP's `substr( $text, $start, $length )` for `$start` within the text,
/// including a negative `$length`.
fn php_substr(text: &[u8], start: usize, length: isize) -> &[u8] {
    let end = if length < 0 {
        text.len().saturating_sub(length.unsigned_abs())
    } else {
        start.saturating_add(length.unsigned_abs()).min(text.len())
    };
    text.get(start..end).unwrap_or_default()
}

/// `str_replace( "\0", "\u{FFFD}", $name )`.
fn replace_nulls(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    for &b in name {
        match b {
            0 => out.extend_from_slice(REPLACEMENT),
            _ => out.push(b),
        }
    }
    out
}

/// `wp_has_noncharacters()`, on bytes.
fn has_noncharacters(text: &[u8]) -> bool {
    let three = text
        .windows(3)
        .any(|w| matches!(w, [0xEF, 0xB7, 0x90..=0xAF] | [0xEF, 0xBF, 0xBE..=0xBF]));
    let four = text.windows(4).any(|w| {
        matches!(
            w,
            [0xF0, 0x9F | 0xAF | 0xBF, 0xBF, 0xBE..=0xBF]
                | [0xF1..=0xF3, 0x8F | 0x9F | 0xAF | 0xBF, 0xBF, 0xBE..=0xBF]
                | [0xF4, 0x8F, 0xBF, 0xBE..=0xBF]
        )
    });
    three || four
}

/// `set_attribute()`'s escaping of values outside URL attributes.
fn escape(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    for &b in value {
        match b {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'&' => out.extend_from_slice(b"&amp;"),
            b'"' => out.extend_from_slice(b"&quot;"),
            b'\'' => out.extend_from_slice(b"&apos;"),
            _ => out.push(b),
        }
    }
    out
}

/// A port of WordPress's `WP_HTML_Tag_Processor`.
#[derive(Debug)]
pub struct TagProcessor {
    html: Vec<u8>,
    state: State,
    bytes_already_parsed: usize,
    token_starts_at: Option<usize>,
    tag_name_starts_at: usize,
    tag_name_length: usize,
    is_closing_tag: bool,
    attributes: HashMap<Vec<u8>, AttributeToken>,
    duplicate_attributes: HashMap<Vec<u8>, Vec<Span>>,
    /// Class name to add (`true`) or remove, in the order first named.
    classname_updates: Vec<(Key, bool)>,
    lexical_updates: Updates,
}

impl TagProcessor {
    /// A processor for `html`, before its first tag.
    pub fn new(html: &[u8]) -> TagProcessor {
        TagProcessor {
            html: html.to_vec(),
            state: State::Ready,
            bytes_already_parsed: 0,
            token_starts_at: None,
            tag_name_starts_at: 0,
            tag_name_length: 0,
            is_closing_tag: false,
            attributes: HashMap::new(),
            duplicate_attributes: HashMap::new(),
            classname_updates: Vec::new(),
            lexical_updates: Updates::default(),
        }
    }

    /// Moves to the next opening tag, optionally one with the given tag name
    /// (any case) and class: `next_tag( array( 'tag_name' => ...,
    /// 'class_name' => ... ) )`. `false` when there is none.
    pub fn next_tag(&mut self, tag_name: Option<&[u8]>, class_name: Option<&[u8]>) -> bool {
        let mut found = false;
        // Every token takes at least one byte.
        for _ in 0..=self.html.len() {
            if !self.next_token() {
                break;
            }
            if self.state == State::MatchedTag && self.matches(tag_name, class_name) {
                found = true;
                break;
            }
        }
        found
    }

    fn matches(&mut self, tag_name: Option<&[u8]>, class_name: Option<&[u8]>) -> bool {
        if self.is_closing_tag {
            return false;
        }
        if let Some(wanted) = tag_name
            && !self
                .get_tag()
                .unwrap_or_default()
                .eq_ignore_ascii_case(wanted)
        {
            return false;
        }
        class_name.is_none_or(|wanted| self.has_class(wanted) == Some(true))
    }

    /// The current tag's name in upper case, `None` when not at a tag.
    pub fn get_tag(&self) -> Option<Vec<u8>> {
        if self.state != State::MatchedTag {
            return None;
        }
        let name = self
            .html
            .get(self.tag_name_starts_at..self.tag_name_starts_at + self.tag_name_length)
            .unwrap_or_default();
        let mut name = replace_nulls(name);
        name.make_ascii_uppercase();
        Some(name)
    }

    fn byte(&self, at: usize) -> Option<u8> {
        self.html.get(at).copied()
    }

    /// Stops at an incomplete token.
    fn incomplete(&mut self) -> bool {
        self.state = State::Incomplete;
        false
    }

    /// `base_class_next_token()`.
    fn next_token(&mut self) -> bool {
        let was_at = self.bytes_already_parsed;
        self.after_tag();
        if matches!(self.state, State::Complete | State::Incomplete) {
            return false;
        }
        self.state = State::Ready;
        let length = self.html.len();
        if self.bytes_already_parsed >= length {
            self.state = State::Complete;
            return false;
        }
        if !self.parse_next_tag() {
            self.bytes_already_parsed = was_at;
            return false;
        }
        if self.state != State::MatchedTag {
            return true;
        }
        for _ in 0..=length {
            if !self.parse_next_attribute() {
                break;
            }
        }
        // An incomplete attribute leaves the offset at the end of the input.
        let Some(tag_ends_at) = find(&self.html, self.bytes_already_parsed, b">") else {
            self.state = State::Incomplete;
            self.bytes_already_parsed = was_at;
            return false;
        };
        self.state = State::MatchedTag;
        self.bytes_already_parsed = tag_ends_at + 1;
        if self.is_closing_tag {
            return true;
        }
        let tag = self.get_tag().unwrap_or_default();
        // The contents of these elements are text up to their closing tag.
        let script = match tag.as_slice() {
            b"SCRIPT" => true,
            b"TEXTAREA" | b"TITLE" | b"IFRAME" | b"NOEMBED" | b"NOFRAMES" | b"STYLE" | b"XMP" => {
                false
            }
            _ => return true,
        };
        // Skipping parses the closing tag's attributes; the opener keeps its
        // own.
        let attributes = mem::take(&mut self.attributes);
        let duplicates = mem::take(&mut self.duplicate_attributes);
        let found_closer = if script {
            self.skip_script_data()
        } else {
            self.skip_rcdata(&tag)
        };
        self.attributes = attributes;
        self.duplicate_attributes = duplicates;
        if !found_closer {
            self.state = State::Incomplete;
            self.bytes_already_parsed = was_at;
            return false;
        }
        self.state = State::MatchedTag;
        self.token_starts_at = Some(was_at);
        true
    }

    /// The token at `was_at`, a text node up to `end`.
    fn text_token(&mut self, was_at: usize, end: usize) -> bool {
        self.state = State::OtherToken;
        self.token_starts_at = Some(was_at);
        self.bytes_already_parsed = end;
        true
    }

    /// A token that ends at the next `>` after `from`, which is not one.
    fn token_until_gt(&mut self, from: usize) -> bool {
        let Some(closer_at) = find(&self.html, from, b">") else {
            return self.incomplete();
        };
        self.state = State::OtherToken;
        self.bytes_already_parsed = closer_at + 1;
        true
    }

    /// `parse_next_tag()`: the next token, attributes not yet parsed.
    fn parse_next_tag(&mut self) -> bool {
        let length = self.html.len();
        let was_at = self.bytes_already_parsed;
        let mut at = was_at;
        for _ in 0..=length {
            let Some(found) = find(&self.html, at, b"<") else {
                break;
            };
            at = found;
            if at > was_at {
                // Text, unless this `<` cannot start a tag.
                let starts_token = self
                    .byte(at + 1)
                    .is_some_and(|c| matches!(c, b'!' | b'/' | b'?') || c.is_ascii_alphabetic());
                if !starts_token {
                    at += 1;
                    continue;
                }
                return self.text_token(was_at, at);
            }
            self.token_starts_at = Some(at);
            self.is_closing_tag = self.byte(at + 1) == Some(b'/');
            at += usize::from(self.is_closing_tag);
            let prefix = self
                .html
                .get(at + 1..)
                .unwrap_or_default()
                .iter()
                .take_while(|b| b.is_ascii_alphabetic())
                .count();
            if prefix > 0 {
                at += 1;
                self.state = State::MatchedTag;
                self.tag_name_starts_at = at;
                self.tag_name_length =
                    prefix + span_until(&self.html, at + prefix, b" \t\x0c\r\n/>");
                self.bytes_already_parsed = at + self.tag_name_length;
                return true;
            }
            let Some(next) = self.byte(at + 1) else {
                return self.incomplete();
            };
            if !self.is_closing_tag && next == b'!' {
                if self.html.get(at + 2..at + 4) == Some(b"--") {
                    return self.comment(at + 4);
                }
                // A doctype, or a bogus comment such as a CDATA lookalike.
                return self.token_until_gt(at);
            }
            if next == b'>' {
                if !self.is_closing_tag {
                    at += 1;
                    continue;
                }
                // `</>` is ignored.
                self.state = State::OtherToken;
                self.bytes_already_parsed = at + 2;
                return true;
            }
            if self.is_closing_tag || next == b'?' {
                // A processing instruction or its lookalike comment, or a
                // closer that cannot be a tag: WordPress's "funky comment".
                return self.token_until_gt(at);
            }
            at += 1;
        }
        self.text_token(was_at, length)
    }

    /// An HTML comment whose text starts at `text_at`, after `<!--`.
    fn comment(&mut self, text_at: usize) -> bool {
        let length = self.html.len();
        // `<!-->` and `<!--->` close at once.
        let gt = text_at + span_of(&self.html, text_at, b"-");
        if self.byte(gt) == Some(b'>') {
            self.state = State::OtherToken;
            self.bytes_already_parsed = gt + 1;
            return true;
        }
        let mut from = text_at;
        for _ in 0..=length {
            let Some(dashes_at) = find(&self.html, from, b"--") else {
                break;
            };
            let end = match (self.byte(dashes_at + 2), self.byte(dashes_at + 3)) {
                (Some(b'>'), _) => Some(dashes_at + 3),
                (Some(b'!'), Some(b'>')) => Some(dashes_at + 4),
                _ => None,
            };
            if let Some(end) = end {
                self.state = State::OtherToken;
                self.bytes_already_parsed = end;
                return true;
            }
            from = dashes_at + 1;
        }
        self.incomplete()
    }

    /// `parse_next_attribute()`; `false` at the end of the tag, or of the
    /// input, where the offset is left at or past the end.
    fn parse_next_attribute(&mut self) -> bool {
        let length = self.html.len();
        self.bytes_already_parsed +=
            span_of(&self.html, self.bytes_already_parsed, b" \t\x0c\r\n/");
        let Some(first) = self.byte(self.bytes_already_parsed) else {
            return false;
        };
        if first == b'>' {
            return false;
        }
        // A leading `=` is part of the name.
        let leading = usize::from(first == b'=');
        let name_length = leading
            + span_until(
                &self.html,
                self.bytes_already_parsed + leading,
                b"=/> \t\x0c\r\n",
            );
        let attribute_start = self.bytes_already_parsed;
        if attribute_start + name_length >= length {
            return false;
        }
        self.bytes_already_parsed += name_length;
        self.bytes_already_parsed += span_of(&self.html, self.bytes_already_parsed, WHITESPACE);
        let Some(after_name) = self.byte(self.bytes_already_parsed) else {
            return false;
        };
        let has_value = after_name == b'=';
        let (value_start, value_length, attribute_end) = if has_value {
            self.bytes_already_parsed += 1;
            self.bytes_already_parsed += span_of(&self.html, self.bytes_already_parsed, WHITESPACE);
            let Some(quote) = self.byte(self.bytes_already_parsed) else {
                return false;
            };
            let (value_start, value_length, attribute_end) = if quote == b'"' || quote == b'\'' {
                let value_start = self.bytes_already_parsed + 1;
                let end_quote_at = find(&self.html, value_start, &[quote]).unwrap_or(length);
                (value_start, end_quote_at - value_start, end_quote_at + 1)
            } else {
                let value_start = self.bytes_already_parsed;
                let value_length = span_until(&self.html, value_start, b"> \t\x0c\r\n");
                (value_start, value_length, value_start + value_length)
            };
            self.bytes_already_parsed = attribute_end;
            (value_start, value_length, attribute_end)
        } else {
            (self.bytes_already_parsed, 0, attribute_start + name_length)
        };
        if attribute_end >= length {
            return false;
        }
        if self.is_closing_tag {
            return true;
        }
        let name = self
            .html
            .get(attribute_start..attribute_start + name_length)
            .unwrap_or_default();
        let mut comparable = replace_nulls(name);
        comparable.make_ascii_lowercase();
        let span = Span {
            start: attribute_start,
            length: attribute_end - attribute_start,
        };
        match self.attributes.entry(comparable) {
            Entry::Occupied(first) => self
                .duplicate_attributes
                .entry(first.key().clone())
                .or_default()
                .push(span),
            Entry::Vacant(slot) => {
                slot.insert(AttributeToken {
                    value_starts_at: value_start,
                    value_length,
                    span,
                    is_true: !has_value,
                });
            }
        }
        true
    }

    /// Skips the text of a `<textarea>`, `<title>` or raw text element to
    /// the end of its closing tag, named `tag` in upper case.
    fn skip_rcdata(&mut self, tag: &[u8]) -> bool {
        let length = self.html.len();
        let mut at = self.bytes_already_parsed;
        for _ in 0..=length {
            let Some(closer_at) = find(&self.html, at, b"</") else {
                break;
            };
            at = closer_at + 2;
            let Some(name) = self.html.get(at..at + tag.len()) else {
                break;
            };
            if let Some(i) = name
                .iter()
                .zip(tag)
                .position(|(h, t)| h.to_ascii_uppercase() != *t)
            {
                at += i;
                continue;
            }
            at += tag.len();
            if !matches!(
                self.byte(at),
                Some(b' ' | b'\t' | b'\x0c' | b'\r' | b'\n' | b'/' | b'>')
            ) {
                continue;
            }
            self.bytes_already_parsed = at;
            for _ in 0..=length {
                if !self.parse_next_attribute() {
                    break;
                }
            }
            at = self.bytes_already_parsed;
            match self.byte(at) {
                None => break,
                Some(b'>') => {
                    self.bytes_already_parsed = at + 1;
                    return true;
                }
                Some(_) => {}
            }
        }
        false
    }

    /// Skips the text of a `<script>` to the end of its closing tag,
    /// following the HTML5 script data states.
    fn skip_script_data(&mut self) -> bool {
        #[derive(PartialEq)]
        enum Escape {
            Unescaped,
            Escaped,
            DoubleEscaped,
        }
        let length = self.html.len();
        let mut state = Escape::Unescaped;
        let mut at = self.bytes_already_parsed;
        for _ in 0..=length {
            let html = &self.html;
            // The next `<`, or `-->`, which ends an escape. A `<` in the last
            // two bytes cannot start a closing tag.
            let Some(next) = html
                .get(at..)
                .unwrap_or_default()
                .windows(3)
                .position(|w| matches!(w, [b'<', ..] | b"-->"))
            else {
                break;
            };
            at += next;
            let byte = |i: usize| html.get(i).copied();
            if html.get(at..at + 3) == Some(b"-->") {
                at += 3;
                state = Escape::Unescaped;
                continue;
            }
            // Past the `<`.
            at += 1;
            if state == Escape::Unescaped && html.get(at..at + 3) == Some(b"!--") {
                // PHP skips the dashes and stays unescaped when `>` follows
                // them; going on from the `!` finds that `-->` instead.
                state = Escape::Escaped;
                continue;
            }
            let is_closing = byte(at) == Some(b'/');
            at += usize::from(is_closing);
            let named_script = html
                .get(at..at + 6)
                .is_some_and(|name| name.eq_ignore_ascii_case(b"script"));
            if !named_script {
                continue;
            }
            at += 6;
            if !matches!(
                byte(at),
                Some(b'>' | b' ' | b'\n' | b'/' | b'\t' | b'\x0c' | b'\r')
            ) {
                continue;
            }
            if state == Escape::Escaped && !is_closing {
                state = Escape::DoubleEscaped;
                continue;
            }
            if state == Escape::DoubleEscaped && is_closing {
                state = Escape::Escaped;
                continue;
            }
            if is_closing {
                // PHP parses from the `<`; the name `script` adds nothing.
                self.bytes_already_parsed = at;
                for _ in 0..=length {
                    if !self.parse_next_attribute() {
                        break;
                    }
                }
                match self.byte(self.bytes_already_parsed) {
                    None => break,
                    Some(b'>') => {
                        self.bytes_already_parsed += 1;
                        return true;
                    }
                    Some(_) => {}
                }
            }
        }
        false
    }

    /// `after_tag()`: turns the class edits into an attribute edit and
    /// forgets the token.
    fn after_tag(&mut self) {
        self.class_name_updates_to_attributes_updates();
        if self.lexical_updates.entries.len() > MAX_PENDING_UPDATES {
            self.get_updated_html();
        }
        self.lexical_updates.rekey();
        self.token_starts_at = None;
        self.is_closing_tag = false;
        self.attributes.clear();
        self.duplicate_attributes.clear();
    }

    /// The pending edit of the attribute `comparable`: `Some(None)` when it
    /// is removed.
    fn enqueued_attribute_value(&self, comparable: &[u8]) -> Option<Option<Attribute>> {
        let text = &self.lexical_updates.get(&Key::of(comparable))?.text;
        if text.is_empty() {
            return Some(None);
        }
        let Some(equals_at) = text.iter().position(|&b| b == b'=') else {
            return Some(Some(Attribute::True));
        };
        // The value between the quotes `set_attribute()` wrote.
        let value = text.get(equals_at + 2..text.len() - 1).unwrap_or_default();
        Some(Some(Attribute::Value(decode_attribute(value))))
    }

    fn decoded_attribute_value(&self, attribute: &AttributeToken) -> Vec<u8> {
        let start = attribute.value_starts_at;
        let raw = self
            .html
            .get(start..start + attribute.value_length)
            .unwrap_or_default();
        let mut normalized = Vec::with_capacity(raw.len());
        let mut bytes = raw.iter().copied().peekable();
        while let Some(b) = bytes.next() {
            match b {
                b'\r' => {
                    bytes.next_if_eq(&b'\n');
                    normalized.push(b'\n');
                }
                0 => normalized.extend_from_slice(REPLACEMENT),
                _ => normalized.push(b),
            }
        }
        decode_attribute(&normalized)
    }

    fn class_name_updates_to_attributes_updates(&mut self) {
        if self.classname_updates.is_empty() {
            return;
        }
        let existing = match self.enqueued_attribute_value(b"class") {
            Some(Some(Attribute::Value(value))) => value,
            Some(_) => Vec::new(),
            None => self
                .attributes
                .get(b"class".as_slice())
                .map(|attribute| self.decoded_attribute_value(attribute))
                .unwrap_or_default(),
        };
        let updates = mem::take(&mut self.classname_updates);
        // PHP compares strictly, so an integer key never matches a class.
        let to_remove: HashSet<&[u8]> = updates
            .iter()
            .filter_map(|(key, add)| match key {
                Key::Str(name) if !add => Some(name.as_slice()),
                _ => None,
            })
            .collect();
        let mut class = Vec::new();
        let mut modified = false;
        let mut seen: HashSet<&[u8]> = HashSet::new();
        let mut at = 0;
        for _ in 0..=existing.len() {
            let ws_at = at;
            let ws_length = span_of(&existing, at, WHITESPACE);
            at += ws_length;
            let name_length = span_until(&existing, at, WHITESPACE);
            if name_length == 0 {
                break;
            }
            let name = existing.get(at..at + name_length).unwrap_or_default();
            at += name_length;
            if to_remove.contains(name) {
                modified = true;
                continue;
            }
            if !seen.insert(name) {
                continue;
            }
            // Whitespace between kept classes is kept as it was.
            if !class.is_empty() {
                class.extend_from_slice(existing.get(ws_at..ws_at + ws_length).unwrap_or_default());
            }
            class.extend_from_slice(name);
        }
        for (key, add) in &updates {
            let name = match key {
                Key::Str(name) if seen.contains(name.as_slice()) => continue,
                Key::Str(name) => name.clone(),
                Key::Int(n) => n.to_string().into_bytes(),
            };
            if *add {
                modified = true;
                if !class.is_empty() {
                    class.push(b' ');
                }
                class.extend_from_slice(&name);
            }
        }
        if !modified {
            return;
        }
        if class.is_empty() {
            self.remove_attribute(b"class");
        } else {
            self.set(b"class", Some(&class));
        }
    }

    /// The value of the attribute `name` (any case) on the current tag,
    /// pending edits included; `None` when it is missing or not at a tag.
    pub fn get_attribute(&mut self, name: &[u8]) -> Option<Attribute> {
        if self.state != State::MatchedTag {
            return None;
        }
        let comparable = name.to_ascii_lowercase();
        if comparable == b"class" {
            self.class_name_updates_to_attributes_updates();
        }
        if let Some(enqueued) = self.enqueued_attribute_value(&comparable) {
            return enqueued;
        }
        let attribute = self.attributes.get(&comparable)?;
        Some(if attribute.is_true {
            Attribute::True
        } else {
            Attribute::Value(self.decoded_attribute_value(attribute))
        })
    }

    /// Sets the attribute `name` to `value`, escaped as WordPress does
    /// (`esc_url()` for URL attributes). `false` when not at an opening tag,
    /// for an invalid name, or for a URL `esc_url()` rejects.
    pub fn set_attribute(&mut self, name: &[u8], value: &[u8]) -> bool {
        self.set(name, Some(value))
    }

    /// Sets the attribute `name` without a value: `set_attribute( $name,
    /// true )`.
    pub fn set_boolean_attribute(&mut self, name: &[u8]) -> bool {
        self.set(name, None)
    }

    fn set(&mut self, name: &[u8], value: Option<&[u8]>) -> bool {
        if self.state != State::MatchedTag || self.is_closing_tag {
            return false;
        }
        let invalid = name.is_empty()
            || name
                .iter()
                .any(|&b| matches!(b, 0x00..=0x1f) || b"\"'>&</ =".contains(&b))
            || has_noncharacters(name);
        if invalid {
            return false;
        }
        let comparable = name.to_ascii_lowercase();
        let updated = match value {
            None => name.to_vec(),
            Some(value) => {
                let escaped = if URI_ATTRIBUTES.contains(&comparable.as_slice()) {
                    let url = esc_url(value);
                    if url.is_empty() && !value.is_empty() {
                        return false;
                    }
                    url
                } else {
                    escape(value)
                };
                [name, b"=\"", &escaped, b"\""].concat()
            }
        };
        let replacement = match self.attributes.get(&comparable) {
            Some(existing) => Replacement {
                start: existing.span.start,
                length: existing.span.length,
                text: updated,
            },
            None => Replacement {
                start: self.tag_name_starts_at + self.tag_name_length,
                length: 0,
                text: [b" ", updated.as_slice()].concat(),
            },
        };
        self.lexical_updates
            .insert(Key::of(&comparable), replacement);
        if comparable == b"class" {
            self.classname_updates.clear();
        }
        true
    }

    /// Removes the attribute `name` (any case) and its duplicates. `false`
    /// when the tag does not have it, or when not at an opening tag.
    pub fn remove_attribute(&mut self, name: &[u8]) -> bool {
        if self.state != State::MatchedTag || self.is_closing_tag {
            return false;
        }
        let name = name.to_ascii_lowercase();
        if name == b"class" {
            self.classname_updates.clear();
        }
        let key = Key::of(&name);
        let Some(attribute) = self.attributes.get(&name) else {
            // A pending edit of an attribute the tag lacks is dropped.
            self.lexical_updates.remove(&key);
            return false;
        };
        let removal = |span: &Span| Replacement {
            start: span.start,
            length: span.length,
            text: Vec::new(),
        };
        self.lexical_updates.insert(key, removal(&attribute.span));
        for span in self.duplicate_attributes.get(&name).into_iter().flatten() {
            self.lexical_updates.append(removal(span));
        }
        true
    }

    fn update_class(&mut self, class_name: &[u8], add: bool) -> bool {
        if self.state != State::MatchedTag || self.is_closing_tag {
            return false;
        }
        let key = Key::of(class_name);
        match self.classname_updates.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = add,
            None => self.classname_updates.push((key, add)),
        }
        true
    }

    /// Adds a class to the current opening tag.
    pub fn add_class(&mut self, class_name: &[u8]) -> bool {
        self.update_class(class_name, true)
    }

    /// Removes a class from the current opening tag.
    pub fn remove_class(&mut self, class_name: &[u8]) -> bool {
        self.update_class(class_name, false)
    }

    /// The current tag's classes, decoded, without duplicates, pending
    /// edits included. Empty when not at a tag.
    pub fn class_list(&mut self) -> Vec<Vec<u8>> {
        let Some(Attribute::Value(class)) = self.get_attribute(b"class") else {
            return Vec::new();
        };
        let mut seen = HashSet::new();
        class
            .split(|b| WHITESPACE.contains(b))
            .filter(|name| !name.is_empty() && seen.insert(*name))
            .map(<[u8]>::to_vec)
            .collect()
    }

    /// Whether the current tag has the class; `None` when not at a tag.
    pub fn has_class(&mut self, wanted: &[u8]) -> Option<bool> {
        if self.state != State::MatchedTag {
            return None;
        }
        Some(self.class_list().iter().any(|name| name == wanted))
    }

    /// The HTML with every edit so far applied. The processor stays on the
    /// current tag.
    pub fn get_updated_html(&mut self) -> &[u8] {
        if self.classname_updates.is_empty() && self.lexical_updates.entries.is_empty() {
            return &self.html;
        }
        let before_current_tag = self.token_starts_at.unwrap_or(0);
        self.class_name_updates_to_attributes_updates();
        let shift = self.apply_attributes_updates(before_current_tag);
        // PHP would go on from a negative offset, which only repeated
        // removals of a duplicated attribute can produce.
        self.bytes_already_parsed = before_current_tag.saturating_add_signed(shift);
        self.next_token();
        &self.html
    }

    /// Applies the pending edits; returns how far they move the offset
    /// `shift_this_point`.
    fn apply_attributes_updates(&mut self, shift_this_point: usize) -> isize {
        let mut updates: Vec<Replacement> = mem::take(&mut self.lexical_updates)
            .entries
            .into_iter()
            .map(|(_, replacement)| replacement)
            .collect();
        updates.sort_by(|a, b| {
            a.start
                .cmp(&b.start)
                .then_with(|| a.text.cmp(&b.text))
                .then_with(|| a.length.cmp(&b.length))
        });
        let mut shift: isize = 0;
        let mut copied = 0;
        let mut output = Vec::with_capacity(self.html.len());
        for diff in &updates {
            if diff.start < shift_this_point {
                shift += diff.text.len() as isize - diff.length as isize;
            }
            // Repeated removals of a duplicate overlap: PHP's `substr()`
            // then gets a negative length and copies from the far end.
            let gap = diff.start as isize - copied as isize;
            output.extend_from_slice(php_substr(&self.html, copied, gap));
            output.extend_from_slice(&diff.text);
            copied = diff.start + diff.length;
        }
        output.extend_from_slice(self.html.get(copied..).unwrap_or_default());
        self.html = output;
        shift
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// The opening tags `next_tag()` visits.
    fn tags(html: &str) -> Vec<String> {
        let mut p = TagProcessor::new(html.as_bytes());
        let mut found = Vec::new();
        for _ in 0..=html.len() {
            if !p.next_tag(None, None) {
                break;
            }
            found.push(text(&p.get_tag().unwrap()));
        }
        assert_eq!(p.get_tag(), None);
        found
    }

    /// The HTML after `edit` on the first tag.
    fn edit(html: &str, edit: impl FnOnce(&mut TagProcessor)) -> String {
        let mut p = TagProcessor::new(html.as_bytes());
        assert!(p.next_tag(None, None));
        edit(&mut p);
        text(p.get_updated_html())
    }

    fn value(value: &str) -> Option<Attribute> {
        Some(Attribute::Value(value.as_bytes().to_vec()))
    }

    fn get(html: &str, name: &str) -> Option<Attribute> {
        let mut p = TagProcessor::new(html.as_bytes());
        assert!(p.next_tag(None, None));
        p.get_attribute(name.as_bytes())
    }

    #[test]
    fn visits_opening_tags() {
        assert_eq!(tags("<div><P>x</p><br/></div>"), ["DIV", "P", "BR"]);
        assert_eq!(tags("<a-b.c d><x\0y>"), ["A-B.C", "X\u{FFFD}Y"]);
        assert_eq!(tags("plain text"), Vec::<String>::new());
        assert_eq!(tags(""), Vec::<String>::new());
        assert_eq!(tags("<b>tail"), ["B"]);
    }

    #[test]
    fn skips_what_is_not_a_tag() {
        for html in [
            "a<3<b>",
            "<3<b>",
            "<><b>",
            "a<><b>",
            "</><b>",
            "</ x><b>",
            "</3><b>",
            "<!-- <a> --><b>",
            "<!--><b>",
            "<!---><b>",
            "<!-- a --!><b>",
            "<!-- a -- b --><b>",
            "<!-- a ---><b>",
            "<!DOCTYPE html><b>",
            "<!x><b>",
            "<![CDATA[<a>]]><b>",
            "<?xml version=1?><b>",
            "<?><b>",
            "</?x><b>",
            "</!x><b>",
        ] {
            assert_eq!(tags(html), ["B"], "{html:?}");
        }
    }

    #[test]
    fn stops_at_incomplete_tokens() {
        for html in [
            "<", "x<", "</", "<!", "<!-", "<!--", "<!-- a", "<!--a--", "<!--a--!", "<!x", "<?x",
            "</ x", "<b", "<b ", "<b/", "<b x", "<b x ", "<b x=", "<b x= ", "<b x=1", "<b x=\"1",
            "<b x='1'", "<b =", "<b ==",
        ] {
            assert_eq!(tags(html), Vec::<String>::new(), "{html:?}");
            assert_eq!(tags(&format!("<i>{html}")), ["I"], "{html:?}");
        }
    }

    #[test]
    fn skips_script_contents() {
        for html in [
            "<script>a</script><b>",
            "<script><b></script><b>",
            "<script>a</scriptx></script><b>",
            "<script>a</SCRIPT ><b>",
            "<script>a</script/><b>",
            "<script>a</script x=\">\"><b>",
            "<script><!--a--></script><b>",
            "<script><!-- <script> </script> --></script><b>",
            "<script><!-- <SCRIPT>a</script>b</script><b>",
            "<script><!-- <scriptx> </script><b>",
            "<script><!-- </script><b>",
            "<script><!--></script><b>",
            "<script><!---></script><b>",
            "<script>--></script><b>",
            "<script> - </script><b>",
            "<script>a<</script><b>",
            "<script><script></script><b>",
            "<script>a</scrip</script><b>",
            "<script>12345678</script abc><b>",
            "<script>12345678</script abc</script><b>",
        ] {
            assert_eq!(tags(html), ["SCRIPT", "B"], "{html:?}");
        }
        for html in [
            "<script>",
            "<script>a</script",
            "<script>12345678</script",
            "<script>12345678</script abc",
            "<script>12345678</script x=1",
            "<script><!-- <script> </script>",
        ] {
            assert_eq!(tags(html), Vec::<String>::new(), "{html:?}");
        }
        assert_eq!(tags("<script>12345678</script>"), ["SCRIPT"]);
        assert_eq!(tags("<script>1234567</script>"), ["SCRIPT"]);
    }

    #[test]
    fn skips_raw_text_contents() {
        for name in [
            "style", "TEXTAREA", "title", "iframe", "noembed", "noframes", "xmp",
        ] {
            let html = format!("<{name}><b></{name}><b>");
            assert_eq!(tags(&html).len(), 2, "{html:?}");
        }
        for html in [
            "<style>a</stylex></style><b>",
            "<style>a</sty</style><b>",
            "<style>a</STYLE\t><b>",
            "<style>a</style/><b>",
            "<style>a</style x=\"</style>\"><b>",
            "<style>a</style/ x><b>",
            "<style>a</style =x><b>",
            "<style>a</style abc</style><b>",
        ] {
            assert_eq!(tags(html), ["STYLE", "B"], "{html:?}");
        }
        for html in [
            "<style>a</style",
            "<style>a</style ",
            "<style>a</style x",
            "<style>a</style x=",
            "<style>a</st",
            "<style>",
            "<style>a</style abc",
        ] {
            assert_eq!(tags(html), Vec::<String>::new(), "{html:?}");
        }
        assert_eq!(
            tags("<listing>a</listing><pre>b<p>"),
            ["LISTING", "PRE", "P"]
        );
    }

    #[test]
    fn reads_attributes() {
        let html = "<div id=a ID=\"b\" class=\"x  y x\" data-v='&lt;q&gt;' =eq hidden e=\"\" \
                    t=\"a\r\nb\rc\" n=\"a\0b\">";
        assert_eq!(get(html, "id"), value("a"));
        assert_eq!(get(html, "ID"), value("a"));
        assert_eq!(get(html, "data-v"), value("<q>"));
        assert_eq!(get(html, "hidden"), Some(Attribute::True));
        assert_eq!(get(html, "=eq"), Some(Attribute::True));
        assert_eq!(get(html, "e"), value(""));
        assert_eq!(get(html, "t"), value("a\nb\nc"));
        assert_eq!(get(html, "n"), value("a\u{FFFD}b"));
        assert_eq!(get(html, "missing"), None);
        assert_eq!(get("<a x = 1 y\t=\n'2'>", "x"), value("1"));
        assert_eq!(get("<a x = 1 y\t=\n'2'>", "y"), value("2"));
        assert_eq!(get("<a x=a\"b>", "x"), value("a\"b"));
        assert_eq!(get("<a/x/y=1>", "y"), value("1"));
        assert_eq!(get("<a \0=1>", "\u{FFFD}"), value("1"));
        assert_eq!(get("<a \0=1>", "\0"), None);
        let mut p = TagProcessor::new(b"<a x=1>");
        assert_eq!(p.get_attribute(b"x"), None);
        assert_eq!(p.has_class(b"x"), None);
        assert!(p.class_list().is_empty());
    }

    #[test]
    fn closers_have_no_attributes() {
        let mut p = TagProcessor::new(b"</a x=1><b>");
        assert!(p.next_token());
        assert_eq!(p.get_tag(), Some(b"A".to_vec()));
        assert_eq!(p.get_attribute(b"x"), None);
        assert!(!p.set_attribute(b"x", b"2"));
        assert!(!p.remove_attribute(b"x"));
        assert!(!p.add_class(b"x"));
        assert!(!p.remove_class(b"x"));
        assert_eq!(p.has_class(b"x"), Some(false));
        assert!(!p.next_tag(None, Some(b"x")));
    }

    #[test]
    fn finds_tags_by_name_and_class() {
        let html = b"<div class=a><P class=\"b c\"></p><p class=c><br><SPAN class=c>";
        let found = |tag: Option<&[u8]>, class: Option<&[u8]>| {
            let mut p = TagProcessor::new(html);
            let mut found = Vec::new();
            for _ in 0..html.len() {
                if !p.next_tag(tag, class) {
                    break;
                }
                found.push(text(&p.get_tag().unwrap()));
            }
            found
        };
        assert_eq!(found(Some(b"p"), None), ["P", "P"]);
        assert_eq!(found(None, Some(b"c")), ["P", "P", "SPAN"]);
        assert_eq!(found(Some(b"Span"), Some(b"c")), ["SPAN"]);
        assert_eq!(found(Some(b"div"), Some(b"b")), Vec::<String>::new());
        assert_eq!(found(None, Some(b"C")), Vec::<String>::new());
        assert_eq!(found(Some(b"sp"), None), Vec::<String>::new());
    }

    #[test]
    fn sets_attributes() {
        assert_eq!(
            edit("<div>", |p| assert!(p.set_attribute(b"title", b"<\"'&>"))),
            "<div title=\"&lt;&quot;&apos;&amp;&gt;\">"
        );
        assert_eq!(
            edit("<div title=x>", |p| assert!(
                p.set_attribute(b"TITLE", b"y")
            )),
            "<div TITLE=\"y\">"
        );
        assert_eq!(
            edit("<div x=1 b x=2>", |p| assert!(p.set_attribute(b"x", b"z"))),
            "<div x=\"z\" b x=2>"
        );
        assert_eq!(
            edit("<div>", |p| {
                assert!(p.set_attribute(b"zz", b"z"));
                assert!(p.set_attribute(b"b", b"2"));
                assert!(p.set_boolean_attribute(b"a"));
                assert!(p.set_attribute(b"b", b"1"));
            }),
            "<div a b=\"1\" zz=\"z\">"
        );
        assert_eq!(
            edit("<div x>", |p| {
                assert!(p.set_boolean_attribute(b"x"));
                assert_eq!(p.get_attribute(b"x"), Some(Attribute::True));
                assert!(p.set_attribute(b"y", b"&lt;"));
                assert_eq!(p.get_attribute(b"y"), value("&lt;"));
            }),
            "<div y=\"&amp;lt;\" x>"
        );
        assert_eq!(
            edit("<a>", |p| {
                assert!(p.set_attribute(b"href", b"http://x/?a[1]&b"));
                assert!(!p.set_attribute(b"src", b"javascript:x"));
                assert!(p.set_attribute(b"data", b""));
            }),
            "<a data=\"\" href=\"http://x/?a%5B1%5D&#038;b\">"
        );
    }

    #[test]
    fn rejects_invalid_attribute_names() {
        for name in [
            "",
            "a b",
            "a\"",
            "a'",
            "a>",
            "a&",
            "a<",
            "a/",
            "a=",
            "a\x01",
            "a\x1f",
            "a\u{FDD0}",
            "a\u{FFFE}",
            "\u{1FFFF}",
            "a\u{FDEF}",
            "\u{10FFFE}",
            "\u{FFFFF}",
        ] {
            assert_eq!(
                edit("<p>", |p| {
                    assert!(!p.set_attribute(name.as_bytes(), b"v"), "{name:?}");
                    assert!(!p.set_boolean_attribute(name.as_bytes()), "{name:?}");
                }),
                "<p>"
            );
        }
        assert_eq!(
            edit("<p>", |p| assert!(
                p.set_attribute("a\x7fé\u{FDCF}".as_bytes(), b"v")
            )),
            "<p a\x7fé\u{FDCF}=\"v\">"
        );
    }

    #[test]
    fn removes_attributes_and_duplicates() {
        assert_eq!(
            edit("<div a=1 b a=2 A=3>", |p| {
                assert!(p.remove_attribute(b"A"));
                assert_eq!(p.get_attribute(b"a"), None);
                assert!(!p.remove_attribute(b"c"));
            }),
            "<div  b  >"
        );
        assert_eq!(
            edit("<div a=1 a=2>", |p| {
                assert!(p.remove_attribute(b"a"));
                assert!(p.set_attribute(b"a", b"z"));
            }),
            "<div a=\"z\" >"
        );
        assert_eq!(
            edit("<div>", |p| {
                assert!(p.set_attribute(b"c", b"1"));
                assert!(!p.remove_attribute(b"c"));
                assert_eq!(p.get_attribute(b"c"), None);
            }),
            "<div>"
        );
        // Removing twice queues the duplicates twice, which PHP's substr()
        // turns into a copy of the tail.
        assert_eq!(
            edit("<div a=1 b a=2 A=3>x</div><p>", |p| {
                assert!(p.remove_attribute(b"a"));
                assert!(p.remove_attribute(b"a"));
            }),
            "<div  b  A=3>x</div> >x</div>>x</div><p>"
        );
    }

    #[test]
    fn edits_classes() {
        let classes = |html: &str, edit: &dyn Fn(&mut TagProcessor)| {
            let mut p = TagProcessor::new(html.as_bytes());
            assert!(p.next_tag(None, None));
            edit(&mut p);
            let list: Vec<String> = p.class_list().iter().map(|c| text(c)).collect();
            (text(p.get_updated_html()), list.join(","))
        };
        let add = |names: &'static [&'static str]| {
            move |p: &mut TagProcessor| {
                for name in names {
                    assert!(p.add_class(name.as_bytes()));
                }
            }
        };
        let remove = |names: &'static [&'static str]| {
            move |p: &mut TagProcessor| {
                for name in names {
                    assert!(p.remove_class(name.as_bytes()));
                }
            }
        };
        assert_eq!(
            classes("<p>", &add(&["a", "b", "a"])),
            ("<p class=\"a b\">".into(), "a,b".into())
        );
        assert_eq!(
            classes("<p class=\"a b\">", &remove(&["a", "b"])),
            ("<p >".into(), "".into())
        );
        assert_eq!(
            classes("<p class=\"a b\">", &remove(&["z"])),
            ("<p class=\"a b\">".into(), "a,b".into())
        );
        assert_eq!(
            classes("<p class=\"a b\">", &add(&["a"])),
            ("<p class=\"a b\">".into(), "a,b".into())
        );
        assert_eq!(
            classes("<p class=\" a  b\tc a \">", &remove(&["b"])),
            ("<p class=\"a\tc\">".into(), "a,c".into())
        );
        assert_eq!(
            classes("<p class=\" a  b\tc a \">", &add(&["d"])),
            ("<p class=\"a  b\tc d\">".into(), "a,b,c,d".into())
        );
        assert_eq!(
            classes("<p class>", &add(&["x"])),
            ("<p class=\"x\">".into(), "x".into())
        );
        assert_eq!(
            classes("<p class=x class=y>", &remove(&["x"])),
            ("<p  >".into(), "".into())
        );
        assert_eq!(
            classes("<p class=\"&lt;a\">", &add(&["b\""])),
            ("<p class=\"&lt;a b&quot;\">".into(), "<a,b\"".into())
        );
        // An integer class name is a PHP integer key, never equal to a
        // class string.
        assert_eq!(
            classes("<p class=\"1 2\">", &|p: &mut TagProcessor| {
                assert!(p.add_class(b"1"));
                assert!(p.remove_class(b"2"));
            }),
            ("<p class=\"1 2 1\">".into(), "1,2".into())
        );
        assert_eq!(
            classes("<p class=A>", &|p: &mut TagProcessor| {
                assert!(p.remove_class(b"a"));
                assert!(p.add_class(b"a"));
                assert!(p.remove_class(b"a"));
            }),
            ("<p class=A>".into(), "A".into())
        );
        assert_eq!(
            classes("<p class=a>", &|p: &mut TagProcessor| {
                assert!(p.add_class(b"b"));
                assert!(p.remove_attribute(b"class"));
                assert!(p.add_class(b"c"));
            }),
            ("<p class=\"c\">".into(), "c".into())
        );
        assert_eq!(
            classes("<p class=a>", &|p: &mut TagProcessor| {
                assert!(p.add_class(b"b"));
                assert!(p.set_attribute(b"class", b"c"));
            }),
            ("<p class=\"c\">".into(), "c".into())
        );
        assert_eq!(
            classes("<p class=a>", &|p: &mut TagProcessor| {
                assert!(p.set_attribute(b"class", b"c&amp;"));
                assert!(p.add_class(b"b"));
            }),
            ("<p class=\"c&amp;amp; b\">".into(), "c&amp;,b".into())
        );
        assert_eq!(
            classes("<p>", &|p: &mut TagProcessor| {
                assert!(p.set_boolean_attribute(b"class"));
                assert!(p.add_class(b"b"));
            }),
            ("<p class=\"b\">".into(), "b".into())
        );
        assert_eq!(
            classes("<p class=a>", &|p: &mut TagProcessor| {
                assert!(p.remove_attribute(b"class"));
                assert!(p.add_class(b"b"));
                assert_eq!(p.has_class(b"b"), Some(true));
                assert_eq!(p.has_class(b"a"), Some(false));
            }),
            ("<p class=\"b\">".into(), "b".into())
        );
    }

    #[test]
    fn integer_attribute_names_are_php_integer_keys() {
        assert_eq!(Key::of(b"0"), Key::Int(0));
        assert_eq!(Key::of(b"-5"), Key::Int(-5));
        assert_eq!(Key::of(b"05"), Key::Str(b"05".to_vec()));
        assert_eq!(Key::of(b"-0"), Key::Str(b"-0".to_vec()));
        assert_eq!(Key::of(b"+1"), Key::Str(b"+1".to_vec()));
        assert_eq!(
            Key::of(b"9223372036854775808"),
            Key::Str(b"9223372036854775808".to_vec())
        );
        assert_eq!(Key::of(b"\xff"), Key::Str(b"\xff".to_vec()));
        // The removal of `x=2` is pending under key 0 when `<b>` sets "0".
        let mut p = TagProcessor::new(b"<a x=1 x=2><b y>");
        assert!(p.next_tag(None, None));
        assert!(p.remove_attribute(b"x"));
        assert!(p.next_tag(None, None));
        assert_eq!(p.get_attribute(b"0"), None);
        assert_eq!(p.get_attribute(b"1"), None);
        assert!(p.set_attribute(b"0", b"q"));
        assert!(!p.remove_attribute(b"1"));
        assert_eq!(text(p.get_updated_html()), "<a x=1 x=2><b 0=\"q\" y>");
        // Negative keys count up from themselves.
        let mut p = TagProcessor::new(b"<a><b x=1 x=2>");
        assert!(p.next_tag(None, None));
        assert!(p.set_attribute(b"-5", b"q"));
        assert!(p.next_tag(None, None));
        assert!(p.remove_attribute(b"x"));
        assert_eq!(p.get_attribute(b"-4"), None);
        assert_eq!(text(p.get_updated_html()), "<a -5=\"q\"><b  >");
    }

    #[test]
    fn applies_pending_edits_after_a_thousand() {
        // Removing an attribute with n duplicates queues n + 1 edits. Past a
        // thousand they are applied before the next token, and the integer
        // keys start again from zero: "3" no longer replaces an edit.
        for (count, left) in [(1000, 1), (1001, 0)] {
            let html = format!("<a{}><b>", " x".repeat(count));
            let mut p = TagProcessor::new(html.as_bytes());
            assert!(p.next_tag(None, None));
            assert!(p.remove_attribute(b"x"));
            assert!(p.next_tag(None, None));
            assert!(p.set_attribute(b"3", b"q"));
            let updated = text(p.get_updated_html());
            assert_eq!(updated.matches('x').count(), left, "{count}");
            assert!(updated.ends_with("><b 3=\"q\">"), "{count}");
        }
    }

    #[test]
    fn updated_html_keeps_the_position() {
        let mut p = TagProcessor::new(b"<p><script>a</script><i>");
        assert!(p.next_tag(None, None));
        assert!(p.set_attribute(b"a", b"1"));
        assert!(p.next_tag(None, None));
        assert!(p.set_attribute(b"b", b"1"));
        assert_eq!(
            text(p.get_updated_html()),
            "<p a=\"1\"><script b=\"1\">a</script><i>"
        );
        assert_eq!(p.get_tag(), Some(b"SCRIPT".to_vec()));
        assert_eq!(p.get_attribute(b"b"), value("1"));
        assert!(p.next_tag(None, None));
        assert_eq!(p.get_tag(), Some(b"I".to_vec()));
        // Edits survive the end of the document, and nothing more is found.
        let mut p = TagProcessor::new(b"<p><i");
        assert!(p.next_tag(None, None));
        assert!(p.add_class(b"x"));
        assert!(!p.next_tag(None, None));
        assert!(!p.add_class(b"y"));
        assert_eq!(text(p.get_updated_html()), "<p class=\"x\"><i");
        assert!(!p.next_tag(None, None));
        // A class edit that changes nothing leaves the HTML alone.
        let mut p = TagProcessor::new(b"<p class=a><b>");
        assert!(p.next_tag(None, None));
        assert!(p.add_class(b"a"));
        assert_eq!(text(p.get_updated_html()), "<p class=a><b>");
        assert!(p.next_tag(None, None));
        assert_eq!(p.get_tag(), Some(b"B".to_vec()));
    }

    /// The state and offset after each token, as PHP's `next_token()`
    /// leaves `parser_state` and `bytes_already_parsed`: `M` a tag, `C`
    /// complete, `I` incomplete, `O` any other token.
    fn boundaries(html: &str) -> String {
        let mut p = TagProcessor::new(html.as_bytes());
        let mut found = Vec::new();
        for _ in 0..20 {
            let more = p.next_token();
            let code = match p.state {
                State::MatchedTag => 'M',
                State::Complete => 'C',
                State::Incomplete => 'I',
                _ => 'O',
            };
            found.push(format!("{code}{}", p.bytes_already_parsed));
            if !more {
                break;
            }
        }
        found.join(" ")
    }

    #[test]
    fn tokens_end_where_php_ends_them() {
        let cases = [
            ("a<3<b>", "O3 M6 C6"),
            ("<>x", "O3 C3"),
            ("x<>y<b>", "O4 M7 C7"),
            ("</>x", "O3 O4 C4"),
            ("</ x>y", "O5 O6 C6"),
            ("</3>", "O4 C4"),
            ("<!---->x", "O7 O8 C8"),
            ("<!-->x", "O5 O6 C6"),
            ("<!--->x", "O6 O7 C7"),
            ("<!-- a -->x", "O10 O11 C11"),
            ("<!-- a --!>x", "O11 O12 C12"),
            ("<!-- a -- b -->x", "O15 O16 C16"),
            ("<!-- a ---><b>", "O11 M14 C14"),
            ("<!DOCTYPE html>x", "O15 O16 C16"),
            ("<!x>y", "O4 O5 C5"),
            ("<?x y?>z", "O7 O8 C8"),
            ("<?>", "O3 C3"),
            ("</?x>", "O5 C5"),
            ("</!x>", "O5 C5"),
            ("<b x=1>t", "M7 O8 C8"),
            ("<b/>", "M4 C4"),
            ("</b x=1>", "M8 C8"),
            ("<script>a</script>b", "M18 O19 C19"),
            ("<script>a</script x=1>b", "M22 O23 C23"),
            ("<script>a</script x=1 y=2>b", "M26 O27 C27"),
            ("<style>a</style x=1 y=2>b", "M24 O25 C25"),
            ("<style>a</style>b", "M16 O17 C17"),
            ("<title>a</title >b", "M17 O18 C18"),
            ("<textarea>a</textarea/>b", "M23 O24 C24"),
            ("<!--", "I0"),
            ("<b x", "I0"),
            ("<!-- > <b> -->x", "O14 O15 C15"),
            ("x<!--<b>-->y", "O1 O11 O12 C12"),
            ("</a x=\"><b>\"><i>", "M13 M16 C16"),
            ("<script><!--><script></script><b>", "M30 M33 C33"),
            ("<script>x/script><b>", "I0"),
            ("<p><b", "M3 I3"),
            ("<!--a--!x-->", "O12 C12"),
            ("<!--a--", "I0"),
            ("<!---", "I0"),
            ("<b x=\"1\">", "M9 C9"),
            ("<b x='1' y = 2>", "M15 C15"),
            ("<b x=1 y>", "M9 C9"),
            (
                "<script><!-- <script> </script> --></script>b",
                "M44 O45 C45",
            ),
            ("<script>--></script>b", "M20 O21 C21"),
            ("<p>text", "M3 O7 C7"),
            ("text", "O4 C4"),
            ("<style>a</sty</style>b", "M21 O22 C22"),
            ("<!--!>x", "I0"),
            ("<script><!---><script></script><b>", "M31 M34 C34"),
            ("<script><!--x--><script></script><b>", "M33 M36 C36"),
            ("<script>a</script", "I0"),
            ("<script>a</scriptx></script>b", "M28 O29 C29"),
            ("<title>a</title", "I0"),
            ("<title>a</titl", "I0"),
            ("<title>a</title x", "I0"),
            ("<!x", "I0"),
            ("<?x", "I0"),
            ("</ x", "I0"),
            ("<p x=\"1", "I0"),
            ("<p x=", "I0"),
            ("<b =x>", "M6 C6"),
            ("</b>", "M4 C4"),
            ("<script><!--></script>b", "M22 O23 C23"),
            ("<!--a--!>", "O9 C9"),
            ("<!--a-- -->", "O11 C11"),
            ("<script>-- --></script>b", "M23 O24 C24"),
        ];
        for (html, expected) in cases {
            assert_eq!(boundaries(html), expected, "{html:?}");
        }
    }

    #[test]
    fn edits_before_a_point_shift_it() {
        let mut p = TagProcessor::new(b"abcdef");
        let edit = |start, length, text: &[u8]| Replacement {
            start,
            length,
            text: text.to_vec(),
        };
        p.lexical_updates.append(edit(1, 1, b"XYZ"));
        p.lexical_updates.append(edit(3, 0, b"Q"));
        p.lexical_updates.append(edit(4, 2, b""));
        assert_eq!(p.apply_attributes_updates(3), 2);
        assert_eq!(text(&p.html), "aXYZcQd");
        assert!(p.lexical_updates.entries.is_empty());
    }

    #[test]
    fn substr_follows_php() {
        assert_eq!(php_substr(b"abcdef", 1, 2), b"bc");
        assert_eq!(php_substr(b"abcdef", 4, 9), b"ef");
        assert_eq!(php_substr(b"abcdef", 1, -2), b"bcd");
        assert_eq!(php_substr(b"abcdef", 4, -3), b"");
        assert_eq!(php_substr(b"abcdef", 1, -9), b"");
    }
}
