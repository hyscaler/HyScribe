// SPDX-License-Identifier: AGPL-3.0-or-later

//! The TOML a manifest is written in, read strictly: tables of bare keys
//! whose values are strings, integers, booleans or arrays of strings, and
//! comments. Anything else TOML allows is refused, with the line.

/// A value of a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Item {
    Text(String),
    Int(i64),
    Bool(bool),
    List(Vec<String>),
}

/// A table's name and its keys, in the order written.
pub(crate) type Table = (String, Vec<(String, Item)>);

/// The tables of `text`, in the order written; a key outside a table, a
/// table or key named twice, or anything outside the subset is refused.
pub(crate) fn parse(text: &str) -> Result<Vec<Table>, String> {
    let mut reader = Reader {
        text,
        bytes: text.as_bytes(),
        at: 0,
        line: 1,
    };
    let mut tables: Vec<Table> = Vec::new();
    loop {
        reader.blank();
        let Some(next) = reader.peek() else {
            return Ok(tables);
        };
        if next == b'[' {
            reader.at += 1;
            let name = reader.key()?;
            reader.expect(b']')?;
            if tables.iter().any(|(named, _)| *named == name) {
                return Err(reader.error(&format!("[{name}] is named twice")));
            }
            tables.push((name, Vec::new()));
        } else {
            let key = reader.key()?;
            reader.spaces();
            reader.expect(b'=')?;
            reader.spaces();
            let item = reader.item()?;
            let Some((table, keys)) = tables.last_mut() else {
                return Err(reader.error(&format!("{key} is outside a table")));
            };
            if keys.iter().any(|(named, _)| *named == key) {
                return Err(reader.error(&format!("[{table}] {key} is named twice")));
            }
            keys.push((key, item));
        }
        reader.end_of_line()?;
    }
}

struct Reader<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
    line: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn error(&self, why: &str) -> String {
        format!("line {}: {why}", self.line)
    }

    fn spaces(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.at += 1;
        }
    }

    /// Spaces, then a comment to the end of the line, if any.
    fn comment(&mut self) {
        self.spaces();
        if self.peek() == Some(b'#') {
            while !matches!(self.peek(), None | Some(b'\n' | b'\r')) {
                self.at += 1;
            }
        }
    }

    /// A line ending, `\n` or `\r\n`; false when there is none here.
    fn newline(&mut self) -> bool {
        let skip = match (self.peek(), self.bytes.get(self.at + 1)) {
            (Some(b'\n'), _) => 1,
            (Some(b'\r'), Some(b'\n')) => 2,
            _ => return false,
        };
        self.at += skip;
        self.line += 1;
        true
    }

    /// Blank and comment lines, and spaces before what comes next.
    fn blank(&mut self) {
        loop {
            self.comment();
            if !self.newline() {
                return;
            }
        }
    }

    fn end_of_line(&mut self) -> Result<(), String> {
        self.comment();
        if self.peek().is_none() || self.newline() {
            Ok(())
        } else {
            Err(self.error("unexpected text after the value"))
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(self.error(&format!("expected {}", char::from(byte))))
        }
    }

    /// A bare key: ASCII letters, digits, `_` and `-`.
    fn key(&mut self) -> Result<String, String> {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            self.at += 1;
        }
        let key = self.bytes.get(start..self.at).unwrap_or_default();
        match std::str::from_utf8(key) {
            Ok(key) if !key.is_empty() => Ok(key.to_string()),
            _ => Err(self.error("expected a bare key")),
        }
    }

    fn item(&mut self) -> Result<Item, String> {
        match self.peek() {
            Some(b'"' | b'\'') => self.text().map(Item::Text),
            Some(b'[') => self.list().map(Item::List),
            Some(b't' | b'f') => self.word(),
            _ => self.int(),
        }
    }

    fn word(&mut self) -> Result<Item, String> {
        for (word, value) in [("true", true), ("false", false)] {
            if self
                .bytes
                .get(self.at..)
                .unwrap_or_default()
                .starts_with(word.as_bytes())
            {
                self.at += word.len();
                return Ok(Item::Bool(value));
            }
        }
        Err(self.error("expected a value"))
    }

    /// A decimal integer with an optional sign, no leading zero.
    fn int(&mut self) -> Result<Item, String> {
        let start = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let digits = self.at;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        let number = self.bytes.get(start..self.at).unwrap_or_default();
        let leading = self.bytes.get(digits..self.at).unwrap_or_default();
        let parsed = std::str::from_utf8(number)
            .ok()
            .and_then(|n| n.parse().ok());
        match parsed {
            Some(n) if leading == b"0" || !leading.starts_with(b"0") => Ok(Item::Int(n)),
            _ => Err(self.error("expected a value")),
        }
    }

    /// An array of strings, over lines if need be, a comma after the last
    /// allowed.
    fn list(&mut self) -> Result<Vec<String>, String> {
        self.at += 1;
        let mut items = Vec::new();
        loop {
            self.blank();
            if self.peek() == Some(b']') {
                self.at += 1;
                return Ok(items);
            }
            items.push(self.text()?);
            self.blank();
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b']') => {}
                _ => return Err(self.error("expected , or ] in the array")),
            }
        }
    }

    /// A basic string with its escapes, or a literal string, on one line.
    fn text(&mut self) -> Result<String, String> {
        let quote = self.peek().filter(|b| matches!(b, b'"' | b'\''));
        let quote = quote.ok_or_else(|| self.error("expected a string"))?;
        self.at += 1;
        let mut text = String::new();
        loop {
            let rest = self.text.get(self.at..).unwrap_or_default();
            let Some(c) = rest.chars().next() else {
                return Err(self.error("unterminated string"));
            };
            self.at += c.len_utf8();
            match c {
                _ if c == char::from(quote) => return Ok(text),
                '\\' if quote == b'"' => text.push(self.escape()?),
                '\t' => text.push(c),
                _ if c.is_control() => return Err(self.error("a control character in a string")),
                _ => text.push(c),
            }
        }
    }

    fn escape(&mut self) -> Result<char, String> {
        let c = self
            .peek()
            .ok_or_else(|| self.error("unterminated string"))?;
        self.at += 1;
        let width = match c {
            b'b' => return Ok('\u{8}'),
            b't' => return Ok('\t'),
            b'n' => return Ok('\n'),
            b'f' => return Ok('\u{c}'),
            b'r' => return Ok('\r'),
            b'"' => return Ok('"'),
            b'\\' => return Ok('\\'),
            b'u' => 4,
            b'U' => 8,
            _ => return Err(self.error("an unknown escape")),
        };
        // Past the end the digits are none, which do not parse.
        let hex = self.bytes.get(self.at..self.at + width).unwrap_or_default();
        self.at += width;
        let code = std::str::from_utf8(hex)
            .ok()
            .filter(|hex| hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok());
        code.and_then(char::from_u32)
            .ok_or_else(|| self.error("an escape that is not a character"))
    }
}
