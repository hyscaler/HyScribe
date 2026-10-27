// SPDX-License-Identifier: AGPL-3.0-or-later

//! The part of PHP's lexer a configuration file needs: open and close tags,
//! comments, strings, integers, names and variables. Everything else is a
//! single punctuation byte, which is enough to find statements and brackets.

use super::WpConfigError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Tok {
    /// `<?php`.
    OpenTag,
    /// `?>`.
    CloseTag,
    /// A name such as `define` or `true`.
    Ident(Vec<u8>),
    /// `$name`, without the `$`.
    Var(Vec<u8>),
    /// A string literal's bytes, or `None` when it interpolates variables.
    Str(Option<Vec<u8>>),
    /// An integer literal that fits in an `i64`.
    Int(i64),
    /// A float, a heredoc or an integer too large for `i64`.
    Other,
    /// Any other byte.
    Punct(u8),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub(super) tok: Tok,
    pub(super) start: usize,
    pub(super) end: usize,
}

pub(super) struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
}

fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_' || b >= 0x80
}

fn is_name(b: u8) -> bool {
    is_name_start(b) || b.is_ascii_digit()
}

impl<'a> Lexer<'a> {
    pub(super) fn new(src: &'a [u8]) -> Lexer<'a> {
        Lexer { src, pos: 0 }
    }

    fn at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    fn rest(&self) -> &'a [u8] {
        self.src.get(self.pos..).unwrap_or_default()
    }

    fn starts_with(&self, text: &[u8]) -> bool {
        self.rest().starts_with(text)
    }

    /// Advances while `keep` holds and returns the bytes passed.
    fn take_while(&mut self, keep: impl Fn(u8) -> bool) -> &'a [u8] {
        let rest = self.rest();
        let len = rest.iter().position(|&b| !keep(b)).unwrap_or(rest.len());
        self.pos += len;
        rest.get(..len).unwrap_or_default()
    }

    pub(super) fn tokens(mut self) -> Result<Vec<Token>, WpConfigError> {
        let mut tokens = Vec::new();
        let mut in_php = false;
        // Every round consumes at least one byte; the bound only makes sure
        // a lexer bug can never turn into a hang.
        for _ in 0..=self.src.len() {
            let Some(b) = self.at(0) else {
                break;
            };
            let start = self.pos;
            if !in_php {
                // Inline HTML up to the next `<?php`.
                match find_open_tag(self.rest()) {
                    Some(at) => {
                        in_php = true;
                        tokens.push(Token {
                            tok: Tok::OpenTag,
                            start: start + at,
                            end: start + at + 5,
                        });
                        self.pos = start + at + 5;
                    }
                    None => self.pos = self.src.len(),
                }
                continue;
            }
            let Some(tok) = self.token(b)? else {
                continue;
            };
            if tok == Tok::CloseTag {
                in_php = false;
            }
            tokens.push(Token {
                tok,
                start,
                end: self.pos,
            });
        }
        Ok(tokens)
    }

    /// The next token, starting with `b`, or `None` for whitespace and
    /// comments.
    fn token(&mut self, b: u8) -> Result<Option<Tok>, WpConfigError> {
        let start = self.pos;
        let unterminated = WpConfigError::Unterminated { offset: start };
        if b.is_ascii_whitespace() {
            self.take_while(|b| b.is_ascii_whitespace());
            return Ok(None);
        }
        if self.starts_with(b"?>") {
            self.pos += 2;
            return Ok(Some(Tok::CloseTag));
        }
        if self.starts_with(b"/*") {
            let at =
                find(self.src.get(start + 2..).unwrap_or_default(), b"*/").ok_or(unterminated)?;
            self.pos = start + 2 + at + 2;
            return Ok(None);
        }
        // `#[` starts an attribute in PHP 8, not a comment.
        if self.starts_with(b"//") || (b == b'#' && self.at(1) != Some(b'[')) {
            // One-line comments end at a newline or at `?>`.
            let rest = self.rest();
            let end = (0..rest.len())
                .find(|&i| {
                    rest.get(i) == Some(&b'\n')
                        || rest.get(i..).is_some_and(|r| r.starts_with(b"?>"))
                })
                .unwrap_or(rest.len());
            self.pos += end;
            return Ok(None);
        }
        if self.starts_with(b"<<<") {
            return self.heredoc().map(Some).ok_or(unterminated);
        }
        self.pos += 1;
        Ok(Some(match b {
            b'\'' => Tok::Str(Some(self.single_quoted().ok_or(unterminated)?)),
            b'"' => Tok::Str(self.double_quoted().ok_or(unterminated)?),
            b'$' if self.at(0).is_some_and(is_name_start) => {
                Tok::Var(self.take_while(is_name).to_vec())
            }
            b'0'..=b'9' => {
                self.pos -= 1;
                self.number()
            }
            b if is_name_start(b) => {
                self.pos -= 1;
                Tok::Ident(self.take_while(is_name).to_vec())
            }
            b => Tok::Punct(b),
        }))
    }

    /// After the opening `'`. `None` if it never closes.
    fn single_quoted(&mut self) -> Option<Vec<u8>> {
        let rest = self.rest();
        let mut out = Vec::new();
        let mut bytes = rest.iter().enumerate();
        while let Some((i, &b)) = bytes.next() {
            match (b, rest.get(i + 1)) {
                (b'\'', _) => {
                    self.pos += i + 1;
                    return Some(out);
                }
                (b'\\', Some(&next @ (b'\'' | b'\\'))) => {
                    out.push(next);
                    bytes.next();
                }
                (b, _) => out.push(b),
            }
        }
        None
    }

    /// After the opening `"`. `Some(None)` if it interpolates variables,
    /// `None` if it never closes.
    fn double_quoted(&mut self) -> Option<Option<Vec<u8>>> {
        let rest = self.rest();
        let mut out = Vec::new();
        let mut interpolated = false;
        let mut bytes = rest.iter().enumerate();
        while let Some((i, &b)) = bytes.next() {
            let next = rest.get(i + 1).copied();
            match b {
                b'"' => {
                    self.pos += i + 1;
                    return Some((!interpolated).then_some(out));
                }
                b'$' if next.is_some_and(|c| is_name_start(c) || c == b'{') => {
                    interpolated = true;
                }
                b'\\' => {
                    let after = rest.get(i + 1..).unwrap_or_default();
                    let used = escape(after, &mut out);
                    // Skip what the escape used.
                    for _ in 0..used {
                        bytes.next();
                    }
                }
                b => out.push(b),
            }
        }
        None
    }

    /// Integers in every PHP base; floats and overflowing values are
    /// [`Tok::Other`].
    fn number(&mut self) -> Tok {
        let mut text = self
            .take_while(|b| b.is_ascii_alphanumeric() || b == b'_')
            .to_vec();
        let float = self.at(0) == Some(b'.') && self.at(1).is_some_and(|c| c.is_ascii_digit());
        if float {
            self.pos += 1;
            self.take_while(|b| b.is_ascii_alphanumeric() || b == b'_');
            return Tok::Other;
        }
        text.retain(|&b| b != b'_');
        let text = String::from_utf8_lossy(&text).to_ascii_lowercase();
        let (digits, radix) = if let Some(hex) = text.strip_prefix("0x") {
            (hex, 16)
        } else if let Some(bin) = text.strip_prefix("0b") {
            (bin, 2)
        } else if let Some(oct) = text.strip_prefix("0o") {
            (oct, 8)
        } else if text.len() > 1 && text.starts_with('0') {
            (text.get(1..).unwrap_or_default(), 8)
        } else {
            (text.as_str(), 10)
        };
        i64::from_str_radix(digits, radix).map_or(Tok::Other, Tok::Int)
    }

    /// `<<<LABEL`, `<<<"LABEL"` or `<<<'LABEL'` up to the closing label.
    /// `None` if it never closes.
    fn heredoc(&mut self) -> Option<Tok> {
        self.pos += 3;
        self.take_while(|b| b == b' ' || b == b'\t');
        let quote = self.at(0).filter(|&q| q == b'"' || q == b'\'');
        self.pos += usize::from(quote.is_some());
        let label = self.take_while(is_name).to_vec();
        if label.is_empty() || (quote.is_some() && self.at(0) != quote) {
            return None;
        }
        let body = self.rest();
        let mut offset = 0;
        // The first piece is the rest of the opening line, which starts with
        // the closing quote or a byte that cannot start a name, so it never
        // matches the label.
        for line in body.split(|&b| b == b'\n') {
            let indent = line
                .iter()
                .take_while(|&&b| b == b' ' || b == b'\t')
                .count();
            let text = line.get(indent..).unwrap_or_default();
            if text.starts_with(&label) && !text.get(label.len()).copied().is_some_and(is_name) {
                self.pos += offset + indent + label.len();
                return Some(Tok::Other);
            }
            offset += line.len() + 1;
        }
        None
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// `<?php` in any case, as PHP accepts it.
fn find_open_tag(haystack: &[u8]) -> Option<usize> {
    haystack
        .windows(5)
        .position(|w| w.eq_ignore_ascii_case(b"<?php"))
}

/// Decodes the escape after a `\` in a double-quoted string (`after` starts
/// right after the backslash) into `out`, and returns how many bytes of
/// `after` it used. Unknown escapes keep their backslash, as in PHP.
fn escape(after: &[u8], out: &mut Vec<u8>) -> usize {
    let first = after.first().copied();
    let simple = match first {
        Some(b'n') => Some(b'\n'),
        Some(b't') => Some(b'\t'),
        Some(b'r') => Some(b'\r'),
        Some(b'v') => Some(0x0b),
        Some(b'e') => Some(0x1b),
        Some(b'f') => Some(0x0c),
        Some(c @ (b'\\' | b'$' | b'"')) => Some(c),
        _ => None,
    };
    if let Some(c) = simple {
        out.push(c);
        return 1;
    }
    let run = |skip: usize, max: usize, keep: fn(&u8) -> bool| {
        after
            .iter()
            .skip(skip)
            .take(max)
            .take_while(|c| keep(c))
            .count()
    };
    let octal = run(0, 3, |c| (b'0'..=b'7').contains(c));
    if octal > 0 {
        // `\400` and up wrap around, as in PHP.
        let value = after
            .iter()
            .take(octal)
            .fold(0u32, |n, d| n * 8 + u32::from(d - b'0'));
        out.extend(u8::try_from(value & 0xff).ok());
        return octal;
    }
    let hex = run(1, 2, u8::is_ascii_hexdigit);
    if first == Some(b'x') && hex > 0 {
        let digits = after.get(1..1 + hex).unwrap_or_default();
        out.extend(u8::from_str_radix(&String::from_utf8_lossy(digits), 16).ok());
        return 1 + hex;
    }
    if after.starts_with(b"u{") {
        let hex = run(2, 6, u8::is_ascii_hexdigit);
        let char = std::str::from_utf8(after.get(2..2 + hex).unwrap_or_default())
            .ok()
            .and_then(|digits| u32::from_str_radix(digits, 16).ok())
            .and_then(char::from_u32);
        if let (Some(c), Some(b'}')) = (char, after.get(2 + hex)) {
            let mut buf = [0; 4];
            out.extend(c.encode_utf8(&mut buf).bytes());
            return 2 + hex + 1;
        }
    }
    out.push(b'\\');
    0
}
