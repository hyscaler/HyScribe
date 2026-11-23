// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reads an existing site's `wp-config.php` without running it
//! (docs/SETUP_WIZARD.md, step 3).
//!
//! The file is tokenized as PHP, but nothing is evaluated: only `define()`
//! calls and `$table_prefix` assignments whose value is a single literal are
//! read. Anything computed (concatenation, `getenv()`, constants) is
//! reported as [`Value::Computed`] with its source text, and the wizard asks
//! for it instead.

mod lex;

use std::fmt;

use lex::{Lexer, Tok, Token};

/// Larger files are refused; a real `wp-config.php` is a few kilobytes.
pub const MAX_SIZE: usize = 1 << 20;

/// Computed source text is cut to this many bytes for display.
const MAX_SOURCE: usize = 200;

/// The value of a constant or of `$table_prefix`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A string literal.
    String(String),
    /// An integer literal.
    Int(i64),
    /// `true` or `false`.
    Bool(bool),
    /// `null`.
    Null,
    /// Anything that needs PHP to evaluate, with its source text.
    Computed(String),
}

impl Value {
    /// The string, if this is a string literal.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(text) => Some(text),
            _ => None,
        }
    }
}

/// One `define()` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Constant {
    /// The constant's name.
    pub name: String,
    /// Its value.
    pub value: Value,
    /// Whether the call only runs under a condition (inside a block, or as
    /// in `defined('X') || define('X', 1)`), so PHP may skip it.
    pub conditional: bool,
}

/// What could be read from a `wp-config.php`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WpConfig {
    /// Every `define()` with a literal name, in file order.
    pub constants: Vec<Constant>,
    /// The last assignment to `$table_prefix`.
    pub table_prefix: Option<Value>,
}

impl WpConfig {
    /// The first definition of `name`. PHP keeps the first `define()` that
    /// runs and ignores later ones.
    pub fn get(&self, name: &str) -> Option<&Constant> {
        self.constants.iter().find(|c| c.name == name)
    }

    /// The value of `name` if it is a string literal.
    pub fn string(&self, name: &str) -> Option<&str> {
        self.get(name).and_then(|c| c.value.as_str())
    }
}

/// Why a file could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WpConfigError {
    /// The file is larger than [`MAX_SIZE`].
    TooLarge(usize),
    /// A string, comment or heredoc runs to the end of the file.
    Unterminated {
        /// Where it starts.
        offset: usize,
    },
}

impl fmt::Display for WpConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WpConfigError::TooLarge(size) => write!(
                f,
                "wp-config.php is {size} bytes; files over {MAX_SIZE} bytes are refused"
            ),
            WpConfigError::Unterminated { offset } => write!(
                f,
                "wp-config.php has an unterminated string or comment at byte {offset}"
            ),
        }
    }
}

impl std::error::Error for WpConfigError {}

/// Reads the constants and table prefix of a `wp-config.php`.
pub fn parse(source: &[u8]) -> Result<WpConfig, WpConfigError> {
    if source.len() > MAX_SIZE {
        return Err(WpConfigError::TooLarge(source.len()));
    }
    let tokens = Lexer::new(source).tokens()?;
    Ok(Parser {
        source,
        tokens: &tokens,
        pos: 0,
    }
    .run())
}

struct Parser<'a> {
    source: &'a [u8],
    tokens: &'a [Token],
    pos: usize,
}

impl Parser<'_> {
    fn run(mut self) -> WpConfig {
        let mut config = WpConfig::default();
        let mut braces = 0usize;
        // Whether the next token starts a statement.
        let mut statement_start = true;
        // Every round consumes at least one token; the bound only makes sure
        // a parser bug can never turn into a hang.
        for _ in 0..self.tokens.len() {
            let Some(token) = self.tokens.get(self.pos) else {
                break;
            };
            self.pos += 1;
            let starts = statement_start;
            statement_start = matches!(
                token.tok,
                Tok::Punct(b';' | b'{' | b'}') | Tok::OpenTag | Tok::CloseTag
            );
            match &token.tok {
                Tok::Punct(b'{') => braces += 1,
                Tok::Punct(b'}') => braces = braces.saturating_sub(1),
                Tok::Ident(name) if name.eq_ignore_ascii_case(b"define") => {
                    if let Some(constant) = self.define(!starts || braces > 0) {
                        config.constants.push(constant);
                    }
                }
                Tok::Var(name) if name == b"table_prefix" => {
                    if let Some(value) = self.assignment() {
                        config.table_prefix = Some(value);
                    }
                }
                _ => {}
            }
        }
        config
    }

    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.pos).map(|t| &t.tok)
    }

    fn eat(&mut self, punct: u8) -> bool {
        let found = self.peek() == Some(&Tok::Punct(punct));
        if found {
            self.pos += 1;
        }
        found
    }

    /// After `define`: `( 'NAME' , value [, ...] )`.
    fn define(&mut self, conditional: bool) -> Option<Constant> {
        if !self.eat(b'(') {
            return None;
        }
        let name = match self.peek() {
            Some(Tok::Str(Some(bytes))) => String::from_utf8(bytes.clone()).ok(),
            _ => None,
        }?;
        self.pos += 1;
        if !self.eat(b',') {
            return None;
        }
        let value = self.value_until(b",)");
        Some(Constant {
            name,
            value,
            conditional,
        })
    }

    /// After `$table_prefix`: `= value ;`, or a compound assignment.
    fn assignment(&mut self) -> Option<Value> {
        let next = self.tokens.get(self.pos + 1).map(|t| &t.tok);
        match (self.peek(), next) {
            // `==` is a comparison, not an assignment.
            (Some(Tok::Punct(b'=')), Some(Tok::Punct(b'='))) => None,
            (Some(Tok::Punct(b'=')), _) => {
                self.pos += 1;
                Some(self.value_until(b";"))
            }
            // `.=` and friends change it in a way only PHP can work out.
            (Some(Tok::Punct(b'.' | b'+')), Some(Tok::Punct(b'='))) => {
                Some(Value::Computed("compound assignment".to_string()))
            }
            _ => None,
        }
    }

    /// Reads tokens up to one of `ends` outside any brackets, and returns
    /// the value they spell. The end token itself is left in place.
    fn value_until(&mut self, ends: &[u8]) -> Value {
        let first = self.pos;
        let mut depth = 0usize;
        for _ in 0..self.tokens.len() {
            let Some(tok) = self.peek() else {
                break;
            };
            match tok {
                Tok::Punct(b'(' | b'[' | b'{') => depth += 1,
                Tok::Punct(b')' | b']' | b'}') if depth > 0 => depth -= 1,
                Tok::Punct(p) if depth == 0 && ends.contains(p) => break,
                Tok::CloseTag => break,
                _ => {}
            }
            self.pos += 1;
        }
        let tokens = self.tokens.get(first..self.pos).unwrap_or_default();
        literal(tokens).unwrap_or_else(|| Value::Computed(self.text(tokens)))
    }

    fn text(&self, tokens: &[Token]) -> String {
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return String::new();
        };
        let bytes = self.source.get(first.start..last.end).unwrap_or_default();
        let shown = bytes.get(..MAX_SOURCE).unwrap_or(bytes);
        String::from_utf8_lossy(shown).into_owned()
    }
}

/// The value of a single literal, with an optional sign on integers.
fn literal(tokens: &[Token]) -> Option<Value> {
    let toks: Vec<&Tok> = tokens.iter().map(|t| &t.tok).collect();
    match toks.as_slice() {
        [Tok::Str(Some(bytes))] => String::from_utf8(bytes.clone()).ok().map(Value::String),
        [Tok::Int(n)] | [Tok::Punct(b'+'), Tok::Int(n)] => Some(Value::Int(*n)),
        [Tok::Punct(b'-'), Tok::Int(n)] => n.checked_neg().map(Value::Int),
        [Tok::Ident(word)] => match word.to_ascii_lowercase().as_slice() {
            b"true" => Some(Value::Bool(true)),
            b"false" => Some(Value::Bool(false)),
            b"null" => Some(Value::Null),
            _ => None,
        },
        _ => None,
    }
}

/// `DB_HOST` split the way WordPress splits it (`wpdb::parse_db_host`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbHost {
    /// Host name or address without brackets; empty means the default.
    pub host: String,
    /// The port, if one is given. Not range checked, as in WordPress.
    pub port: Option<u64>,
    /// The socket path, if one is given.
    pub socket: Option<String>,
    /// Whether the host is an IPv6 address.
    pub ipv6: bool,
}

/// Splits `DB_HOST` into host, port and socket, as WordPress does:
/// `localhost`, `db.example:3307`, `localhost:/run/mysqld/mysqld.sock`,
/// `[::1]:3306`. `None` where WordPress fails too.
pub fn parse_db_host(value: &str) -> Option<DbHost> {
    // The socket is peeled off first, from the first ":/".
    let (host, socket) = match value.split_once(":/") {
        Some((host, path)) => (host, Some(format!("/{path}"))),
        None => (value, None),
    };
    let ipv6 = host.matches(':').count() > 1;
    let (host, port) = if ipv6 {
        // #^(?:\[)?(?P<host>[0-9a-fA-F:]+)(?:\]:(?P<port>[\d]+))?#
        let rest = host.strip_prefix('[').unwrap_or(host);
        let (name, rest) = split_while(rest, |c| c.is_ascii_hexdigit() || c == ':');
        if name.is_empty() {
            return None;
        }
        let port = rest
            .strip_prefix("]:")
            .map(|rest| split_while(rest, |c| c.is_ascii_digit()).0);
        (name, port)
    } else {
        // #^(?P<host>[^:/]*)(?::(?P<port>[\d]+))?#
        let (name, rest) = split_while(host, |c| c != ':' && c != '/');
        let port = rest
            .strip_prefix(':')
            .map(|rest| split_while(rest, |c| c.is_ascii_digit()).0);
        (name, port)
    };
    Some(DbHost {
        host: if php_empty(host) { "" } else { host }.to_string(),
        // `abs((int) $port)`: PHP saturates digits that overflow.
        port: port
            .filter(|p| !php_empty(p))
            .map(|p| p.parse().unwrap_or(i64::MAX.unsigned_abs())),
        socket,
        ipv6,
    })
}

/// PHP's `empty()` for a string.
fn php_empty(text: &str) -> bool {
    text.is_empty() || text == "0"
}

/// Splits `text` after its longest prefix whose characters match `keep`.
fn split_while(text: &str, keep: impl Fn(char) -> bool) -> (&str, &str) {
    let end = text.find(|c| !keep(c)).unwrap_or(text.len());
    text.split_at(end)
}

#[cfg(test)]
mod tests;
