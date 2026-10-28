// SPDX-License-Identifier: AGPL-3.0-or-later

//! `parse_blocks()`: a port of WordPress's `WP_Block_Parser`, including its
//! handling of malformed input, so that every document splits into exactly
//! the blocks PHP WordPress sees.
//!
//! The block comment delimiter regex is matched by hand, byte for byte with
//! PCRE's semantics, in linear time: a scan for attributes that finds no
//! terminator is remembered, so hostile input cannot make it quadratic.

use hyscribe_phpser::{Key, Value, is_empty_string, json_decode, json_encode_unescaped};

use crate::formatting::find;

/// One block, as `parse_blocks()` returns it.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// `namespace/name`, or `None` for HTML between blocks.
    pub name: Option<String>,
    /// The decoded attributes; `None` where their JSON is invalid (PHP's
    /// `null`). Empty when the delimiter has none.
    pub attrs: Option<Vec<(Key, Value)>>,
    /// Nested blocks.
    pub inner_blocks: Vec<Block>,
    /// The block's own HTML, without its inner blocks.
    pub inner_html: Vec<u8>,
    /// HTML chunks, with `None` where an inner block goes.
    pub inner_content: Vec<Option<Vec<u8>>>,
}

impl Block {
    fn freeform(html: &[u8]) -> Block {
        Block {
            name: None,
            attrs: Some(Vec::new()),
            inner_blocks: Vec::new(),
            inner_html: html.to_vec(),
            inner_content: vec![Some(html.to_vec())],
        }
    }

    fn named(name: String, attrs: Option<Vec<(Key, Value)>>) -> Block {
        Block {
            name: Some(name),
            attrs,
            inner_blocks: Vec::new(),
            inner_html: Vec::new(),
            inner_content: Vec::new(),
        }
    }

    /// The block as PHP's `(array) $block`, for comparing with PHP.
    pub fn to_php(&self) -> Value {
        let key = |name: &str| Key::String(name.as_bytes().to_vec());
        let string = |bytes: &[u8]| Value::String(bytes.to_vec());
        Value::Array(vec![
            (
                key("blockName"),
                self.name
                    .as_ref()
                    .map_or(Value::Null, |n| string(n.as_bytes())),
            ),
            (
                key("attrs"),
                self.attrs.clone().map_or(Value::Null, Value::Array),
            ),
            (key("innerBlocks"), list(&self.inner_blocks)),
            (key("innerHTML"), string(&self.inner_html)),
            (
                key("innerContent"),
                Value::Array(
                    self.inner_content
                        .iter()
                        .zip(0..)
                        .map(|(chunk, i)| {
                            (Key::Int(i), chunk.as_deref().map_or(Value::Null, string))
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// A list of blocks as the PHP array `parse_blocks()` returns.
pub fn list(blocks: &[Block]) -> Value {
    Value::Array(
        blocks
            .iter()
            .zip(0..)
            .map(|(block, i)| (Key::Int(i), block.to_php()))
            .collect(),
    )
}

/// The HTML of `blocks` with every inner block put back in its place: what
/// `do_blocks()` outputs for static blocks. Dynamic blocks come later.
pub fn render(blocks: &[Block]) -> Vec<u8> {
    let mut out = Vec::new();
    for block in blocks {
        render_into(block, &mut out);
    }
    out
}

fn render_into(block: &Block, out: &mut Vec<u8>) {
    if block.name.is_none() {
        out.extend_from_slice(&block.inner_html);
        return;
    }
    let mut inner = block.inner_blocks.iter();
    for chunk in &block.inner_content {
        match chunk {
            Some(html) => out.extend_from_slice(html),
            None => {
                if let Some(child) = inner.next() {
                    render_into(child, out);
                }
            }
        }
    }
}

/// `serialize_blocks()`: the block markup `parse_blocks()` reads back as
/// `blocks`.
pub(crate) fn serialize_blocks(blocks: &[Block]) -> Vec<u8> {
    let mut out = Vec::new();
    for block in blocks {
        serialize_into(block, &mut out);
    }
    out
}

/// `serialize_block()` and `get_comment_delimited_block_content()`.
fn serialize_into(block: &Block, out: &mut Vec<u8>) {
    let mut content = Vec::new();
    let mut inner = block.inner_blocks.iter();
    for chunk in &block.inner_content {
        match chunk {
            Some(html) => content.extend_from_slice(html),
            None => inner
                .next()
                .into_iter()
                .for_each(|child| serialize_into(child, &mut content)),
        }
    }
    let Some(name) = &block.name else {
        out.extend_from_slice(&content);
        return;
    };
    let name = name.strip_prefix("core/").unwrap_or(name).as_bytes();
    out.extend_from_slice(b"<!-- wp:");
    out.extend_from_slice(name);
    out.push(b' ');
    if let Some(attrs) = block.attrs.as_ref().filter(|a| !a.is_empty()) {
        out.extend_from_slice(&serialize_block_attributes(attrs));
        out.push(b' ');
    }
    if is_empty_string(&content) {
        out.extend_from_slice(b"/-->");
        return;
    }
    out.extend_from_slice(b"-->");
    out.extend_from_slice(&content);
    out.extend_from_slice(b"<!-- /wp:");
    out.extend_from_slice(name);
    out.extend_from_slice(b" -->");
}

/// `serialize_block_attributes()`: JSON that cannot close the comment or
/// open a tag. Attributes JSON cannot encode (an infinite float) give `''`.
fn serialize_block_attributes(attrs: &[(Key, Value)]) -> Vec<u8> {
    let json = json_encode_unescaped(&Value::Array(attrs.to_vec())).unwrap_or_default();
    let mut out = Vec::with_capacity(json.len());
    let mut rest = json.as_slice();
    while let Some(&byte) = rest.first() {
        let (with, used): (&[u8], usize) = match (byte, rest.get(..2).unwrap_or_default()) {
            (_, b"\\\\") => (b"\\u005c", 2),
            (_, b"--") => (b"\\u002d\\u002d", 2),
            (_, b"\\\"") => (b"\\u0022", 2),
            (b'<', _) => (b"\\u003c", 1),
            (b'>', _) => (b"\\u003e", 1),
            (b'&', _) => (b"\\u0026", 1),
            _ => (rest.get(..1).unwrap_or_default(), 1),
        };
        out.extend_from_slice(with);
        rest = rest.get(used..).unwrap_or_default();
    }
    out
}

/// Blocks nested deeper give no blocks at all: rendering recurses once per
/// level, and no real post comes near this.
pub(crate) const MAX_BLOCK_DEPTH: usize = 128;

/// Each unclosed block takes the rest of the document, a quadratic copy;
/// past this many bytes PHP has run out of memory, and the parse gives up.
const MAX_UNCLOSED_BYTES: usize = 64 << 20;

/// Splits `document` into blocks, as WordPress's `parse_blocks()` does,
/// within [`MAX_BLOCK_DEPTH`] and PHP's memory limit.
pub fn parse_blocks(document: &[u8]) -> Vec<Block> {
    let mut parser = Parser {
        doc: document,
        offset: 0,
        output: Vec::new(),
        stack: Vec::new(),
        no_terminator_from: None,
    };
    // Every round but the last consumes a delimiter; the bound only rules
    // out a hang.
    for _ in 0..=document.len() {
        if !parser.proceed() {
            break;
        }
    }
    parser.output
}

enum Token {
    NoMore,
    Void(String, Option<Vec<(Key, Value)>>, usize, usize),
    Opener(String, Option<Vec<(Key, Value)>>, usize, usize),
    Closer(usize, usize),
}

struct Frame {
    block: Block,
    token_start: usize,
    prev_offset: usize,
    leading_html_start: Option<usize>,
}

struct Parser<'a> {
    doc: &'a [u8],
    offset: usize,
    output: Vec<Block>,
    stack: Vec<Frame>,
    /// No attribute terminator exists at or after this offset.
    no_terminator_from: Option<usize>,
}

impl Parser<'_> {
    fn slice(&self, from: usize, to: usize) -> &[u8] {
        self.doc.get(from..to).unwrap_or_default()
    }

    fn rest(&self, from: usize) -> &[u8] {
        self.doc.get(from..).unwrap_or_default()
    }

    /// One step of `WP_Block_Parser::proceed()`; `false` when done.
    fn proceed(&mut self) -> bool {
        let token = self.next_token();
        let depth = self.stack.len();
        let leading = |start: usize, offset: usize| (start > offset).then_some(offset);
        match token {
            Token::NoMore => {
                let copied: usize = self
                    .stack
                    .iter()
                    .map(|frame| self.doc.len() - frame.prev_offset)
                    .sum();
                if copied > MAX_UNCLOSED_BYTES {
                    return self.give_up();
                }
                if depth == 0 {
                    self.add_freeform();
                }
                // Unclosed blocks are closed implicitly, each straight
                // into the output, as WordPress does.
                while let Some(top) = self.stack.pop() {
                    self.finish(top, None);
                }
                false
            }
            Token::Void(name, attrs, start, len) => {
                let block = Block::named(name, attrs);
                let doc = self.doc;
                match self.stack.last_mut() {
                    None => {
                        if let Some(from) = leading(start, self.offset) {
                            let html = doc.get(from..start).unwrap_or_default();
                            self.output.push(Block::freeform(html));
                        }
                        self.output.push(block);
                    }
                    Some(parent) => add_inner_block(doc, parent, block, start, start + len),
                }
                self.offset = start + len;
                true
            }
            Token::Opener(..) if depth == MAX_BLOCK_DEPTH => self.give_up(),
            Token::Opener(name, attrs, start, len) => {
                self.stack.push(Frame {
                    block: Block::named(name, attrs),
                    token_start: start,
                    prev_offset: start + len,
                    leading_html_start: leading(start, self.offset),
                });
                self.offset = start + len;
                true
            }
            Token::Closer(start, len) => self.close(start, start + len),
        }
    }

    fn close(&mut self, start: usize, end: usize) -> bool {
        let Some(mut top) = self.stack.pop() else {
            // A closer with nothing open: the rest is one HTML chunk.
            self.add_freeform();
            return false;
        };
        let doc = self.doc;
        match self.stack.last_mut() {
            None => self.finish(top, Some(start)),
            Some(parent) => {
                // Unlike the other cases, no empty() check here.
                let html = doc.get(top.prev_offset..start).unwrap_or_default();
                top.block.inner_html.extend_from_slice(html);
                top.block.inner_content.push(Some(html.to_vec()));
                add_inner_block(doc, parent, top.block, top.token_start, end);
            }
        }
        self.offset = end;
        true
    }

    fn give_up(&mut self) -> bool {
        self.output.clear();
        false
    }

    fn add_freeform(&mut self) {
        let html = self.rest(self.offset);
        if !html.is_empty() {
            let block = Block::freeform(html);
            self.output.push(block);
        }
    }

    /// Closes `top` at `end` (or the end of the document) into the output.
    fn finish(&mut self, mut top: Frame, end: Option<usize>) {
        let html = match end {
            Some(end) => self.slice(top.prev_offset, end),
            None => self.rest(top.prev_offset),
        }
        .to_vec();
        if !is_empty_string(&html) {
            top.block.inner_html.extend_from_slice(&html);
            top.block.inner_content.push(Some(html));
        }
        if let Some(from) = top.leading_html_start {
            let leading = self.slice(from, top.token_start).to_vec();
            self.output.push(Block::freeform(&leading));
        }
        self.output.push(top.block);
    }

    /// The next block delimiter at or after `offset`.
    fn next_token(&mut self) -> Token {
        let mut from = self.offset;
        for _ in 0..=self.doc.len() {
            let Some(start) = find(self.doc, from, b"<!--") else {
                break;
            };
            if let Some(token) = self.delimiter(start) {
                return token;
            }
            from = start + 1;
        }
        Token::NoMore
    }

    /// Matches the delimiter regex at `start`, which holds `<!--`:
    /// `<!--\s+(/)?wp:([a-z][a-z0-9_-]*/)?([a-z][a-z0-9_-]*)\s+(ATTRS)?(/)?-->`
    fn delimiter(&mut self, start: usize) -> Option<Token> {
        let doc = self.doc;
        let at = |i: usize| doc.get(i).copied();
        let mut i = start + 4;
        i = after_space(doc, i)?;
        let closer = at(i) == Some(b'/');
        i += usize::from(closer);
        if !self.rest(i).starts_with(b"wp:") {
            return None;
        }
        i += 3;
        // An optional `namespace/`, then the name.
        let word = |from: usize| {
            at(from)
                .filter(u8::is_ascii_lowercase)
                .map(|_| from + 1 + run(doc, from + 1, is_name_byte))
        };
        let mut namespace = None;
        if let Some(end) = word(i).filter(|&end| at(end) == Some(b'/')) {
            namespace = Some((i, end + 1));
            i = end + 1;
        }
        let name_start = i;
        i = word(i)?;
        let name = self.slice(name_start, i);
        let full = match namespace {
            Some((from, to)) => [self.slice(from, to), name].concat(),
            None => [b"core/".as_slice(), name].concat(),
        };
        let full = String::from_utf8_lossy(&full).into_owned();
        i = after_space(doc, i)?;

        let mut attrs = Some(Vec::new());
        if at(i) == Some(b'{')
            && let Some(end) = self.attributes(i)
        {
            // The capture includes the whitespace after `}`, which JSON
            // only accepts if it is a space, tab or line break.
            attrs = match json_decode(self.slice(i, end)) {
                Some(Value::Array(entries)) => Some(entries),
                _ => None,
            };
            i = end;
        }
        let void = at(i) == Some(b'/');
        i += usize::from(void);
        if !self.rest(i).starts_with(b"-->") {
            return None;
        }
        let len = i + 3 - start;
        Some(if void {
            Token::Void(full, attrs, start, len)
        } else if closer {
            Token::Closer(start, len)
        } else {
            Token::Opener(full, attrs, start, len)
        })
    }

    /// The attributes group from `open`, which holds `{`, through the `}`
    /// and the whitespace after it: the possessive
    /// `{(?:[^}]+|}+(?=})|(?!}\s+/?-->).)*+}\s+`. Returns where it ends.
    fn attributes(&mut self, open: usize) -> Option<usize> {
        if self.no_terminator_from.is_some_and(|from| open >= from) {
            return None;
        }
        let doc = self.doc;
        // The `{` itself goes with the first run of other bytes. A `}` only
        // ends the group when `\s+/?-->` follows, so stepping over every
        // other `}` one at a time matches `}+(?=})` and `.` alike.
        let mut i = open;
        for _ in 0..=doc.len() {
            match doc.get(i) {
                None => break,
                Some(b'}') => {
                    if let Some(end) = terminator(doc, i) {
                        return Some(end);
                    }
                    i += 1;
                }
                Some(_) => i += run(doc, i, |b| b != b'}'),
            }
        }
        // No terminator anywhere after `open`, so none after any later `{`.
        self.no_terminator_from = Some(open);
        None
    }
}

/// Adds `block` to `parent`, with the HTML between them, as
/// `WP_Block_Parser::add_inner_block()` does.
fn add_inner_block(
    doc: &[u8],
    parent: &mut Frame,
    block: Block,
    token_start: usize,
    last_offset: usize,
) {
    let html = doc.get(parent.prev_offset..token_start).unwrap_or_default();
    parent.block.inner_blocks.push(block);
    if !is_empty_string(html) {
        parent.block.inner_html.extend_from_slice(html);
        parent.block.inner_content.push(Some(html.to_vec()));
    }
    parent.block.inner_content.push(None);
    parent.prev_offset = last_offset;
}

/// `}\s+` followed by `/?-->` at `i`: the end of the `}\s+`, or `None`.
fn terminator(doc: &[u8], i: usize) -> Option<usize> {
    let end = after_space(doc, i + 1)?;
    let tail = doc.get(end..).unwrap_or_default();
    (tail.starts_with(b"-->") || tail.starts_with(b"/-->")).then_some(end)
}

/// After one or more PCRE `\s` characters at `i`, or `None` if there is none.
fn after_space(doc: &[u8], i: usize) -> Option<usize> {
    let spaces = run(doc, i, is_pcre_space);
    (spaces > 0).then_some(i + spaces)
}

/// PCRE's `\s` without Unicode: space, tab, line feed, vertical tab, form
/// feed and carriage return.
pub(crate) fn is_pcre_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-'
}

/// How many bytes from `i` satisfy `keep`.
pub(crate) fn run(doc: &[u8], i: usize, keep: impl Fn(u8) -> bool) -> usize {
    doc.get(i..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| keep(b))
        .count()
}

#[cfg(test)]
mod tests;
