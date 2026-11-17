// SPDX-License-Identifier: AGPL-3.0-or-later

//! A non-validating XML 1.0 reader for feeds and oEmbed answers, in one
//! pass over the bytes: elements with their namespaces resolved as PHP's
//! namespace-aware parser gives them, text with character and entity
//! references decoded (and HTML 4's entities when asked, as SimplePie
//! declares them), and CDATA. A document type with an internal subset is
//! refused rather than expanded, and nesting stops at libxml's 256 levels,
//! so the work stays linear in the input. Fuzz target `xml_reader`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

/// libxml's default limit on nested elements.
const MAX_DEPTH: usize = 256;

/// The namespace `xml:` names.
pub const NAMESPACE_XML: &[u8] = b"http://www.w3.org/XML/1998/namespace";

/// The entities SimplePie declares for feeds: HTML 4's.
static HTML: LazyLock<HashMap<&'static [u8], u32>> = LazyLock::new(|| {
    [
        (&b"nbsp"[..], 0x00A0),
        (&b"iexcl"[..], 0x00A1),
        (&b"cent"[..], 0x00A2),
        (&b"pound"[..], 0x00A3),
        (&b"curren"[..], 0x00A4),
        (&b"yen"[..], 0x00A5),
        (&b"brvbar"[..], 0x00A6),
        (&b"sect"[..], 0x00A7),
        (&b"uml"[..], 0x00A8),
        (&b"copy"[..], 0x00A9),
        (&b"ordf"[..], 0x00AA),
        (&b"laquo"[..], 0x00AB),
        (&b"not"[..], 0x00AC),
        (&b"shy"[..], 0x00AD),
        (&b"reg"[..], 0x00AE),
        (&b"macr"[..], 0x00AF),
        (&b"deg"[..], 0x00B0),
        (&b"plusmn"[..], 0x00B1),
        (&b"sup2"[..], 0x00B2),
        (&b"sup3"[..], 0x00B3),
        (&b"acute"[..], 0x00B4),
        (&b"micro"[..], 0x00B5),
        (&b"para"[..], 0x00B6),
        (&b"middot"[..], 0x00B7),
        (&b"cedil"[..], 0x00B8),
        (&b"sup1"[..], 0x00B9),
        (&b"ordm"[..], 0x00BA),
        (&b"raquo"[..], 0x00BB),
        (&b"frac14"[..], 0x00BC),
        (&b"frac12"[..], 0x00BD),
        (&b"frac34"[..], 0x00BE),
        (&b"iquest"[..], 0x00BF),
        (&b"Agrave"[..], 0x00C0),
        (&b"Aacute"[..], 0x00C1),
        (&b"Acirc"[..], 0x00C2),
        (&b"Atilde"[..], 0x00C3),
        (&b"Auml"[..], 0x00C4),
        (&b"Aring"[..], 0x00C5),
        (&b"AElig"[..], 0x00C6),
        (&b"Ccedil"[..], 0x00C7),
        (&b"Egrave"[..], 0x00C8),
        (&b"Eacute"[..], 0x00C9),
        (&b"Ecirc"[..], 0x00CA),
        (&b"Euml"[..], 0x00CB),
        (&b"Igrave"[..], 0x00CC),
        (&b"Iacute"[..], 0x00CD),
        (&b"Icirc"[..], 0x00CE),
        (&b"Iuml"[..], 0x00CF),
        (&b"ETH"[..], 0x00D0),
        (&b"Ntilde"[..], 0x00D1),
        (&b"Ograve"[..], 0x00D2),
        (&b"Oacute"[..], 0x00D3),
        (&b"Ocirc"[..], 0x00D4),
        (&b"Otilde"[..], 0x00D5),
        (&b"Ouml"[..], 0x00D6),
        (&b"times"[..], 0x00D7),
        (&b"Oslash"[..], 0x00D8),
        (&b"Ugrave"[..], 0x00D9),
        (&b"Uacute"[..], 0x00DA),
        (&b"Ucirc"[..], 0x00DB),
        (&b"Uuml"[..], 0x00DC),
        (&b"Yacute"[..], 0x00DD),
        (&b"THORN"[..], 0x00DE),
        (&b"szlig"[..], 0x00DF),
        (&b"agrave"[..], 0x00E0),
        (&b"aacute"[..], 0x00E1),
        (&b"acirc"[..], 0x00E2),
        (&b"atilde"[..], 0x00E3),
        (&b"auml"[..], 0x00E4),
        (&b"aring"[..], 0x00E5),
        (&b"aelig"[..], 0x00E6),
        (&b"ccedil"[..], 0x00E7),
        (&b"egrave"[..], 0x00E8),
        (&b"eacute"[..], 0x00E9),
        (&b"ecirc"[..], 0x00EA),
        (&b"euml"[..], 0x00EB),
        (&b"igrave"[..], 0x00EC),
        (&b"iacute"[..], 0x00ED),
        (&b"icirc"[..], 0x00EE),
        (&b"iuml"[..], 0x00EF),
        (&b"eth"[..], 0x00F0),
        (&b"ntilde"[..], 0x00F1),
        (&b"ograve"[..], 0x00F2),
        (&b"oacute"[..], 0x00F3),
        (&b"ocirc"[..], 0x00F4),
        (&b"otilde"[..], 0x00F5),
        (&b"ouml"[..], 0x00F6),
        (&b"divide"[..], 0x00F7),
        (&b"oslash"[..], 0x00F8),
        (&b"ugrave"[..], 0x00F9),
        (&b"uacute"[..], 0x00FA),
        (&b"ucirc"[..], 0x00FB),
        (&b"uuml"[..], 0x00FC),
        (&b"yacute"[..], 0x00FD),
        (&b"thorn"[..], 0x00FE),
        (&b"yuml"[..], 0x00FF),
        (&b"OElig"[..], 0x0152),
        (&b"oelig"[..], 0x0153),
        (&b"Scaron"[..], 0x0160),
        (&b"scaron"[..], 0x0161),
        (&b"Yuml"[..], 0x0178),
        (&b"fnof"[..], 0x0192),
        (&b"circ"[..], 0x02C6),
        (&b"tilde"[..], 0x02DC),
        (&b"Alpha"[..], 0x0391),
        (&b"Beta"[..], 0x0392),
        (&b"Gamma"[..], 0x0393),
        (&b"Epsilon"[..], 0x0395),
        (&b"Zeta"[..], 0x0396),
        (&b"Eta"[..], 0x0397),
        (&b"Theta"[..], 0x0398),
        (&b"Iota"[..], 0x0399),
        (&b"Kappa"[..], 0x039A),
        (&b"Lambda"[..], 0x039B),
        (&b"Mu"[..], 0x039C),
        (&b"Nu"[..], 0x039D),
        (&b"Xi"[..], 0x039E),
        (&b"Omicron"[..], 0x039F),
        (&b"Pi"[..], 0x03A0),
        (&b"Rho"[..], 0x03A1),
        (&b"Sigma"[..], 0x03A3),
        (&b"Tau"[..], 0x03A4),
        (&b"Upsilon"[..], 0x03A5),
        (&b"Phi"[..], 0x03A6),
        (&b"Chi"[..], 0x03A7),
        (&b"Psi"[..], 0x03A8),
        (&b"Omega"[..], 0x03A9),
        (&b"alpha"[..], 0x03B1),
        (&b"beta"[..], 0x03B2),
        (&b"gamma"[..], 0x03B3),
        (&b"delta"[..], 0x03B4),
        (&b"epsilon"[..], 0x03B5),
        (&b"zeta"[..], 0x03B6),
        (&b"eta"[..], 0x03B7),
        (&b"theta"[..], 0x03B8),
        (&b"iota"[..], 0x03B9),
        (&b"kappa"[..], 0x03BA),
        (&b"lambda"[..], 0x03BB),
        (&b"mu"[..], 0x03BC),
        (&b"nu"[..], 0x03BD),
        (&b"xi"[..], 0x03BE),
        (&b"omicron"[..], 0x03BF),
        (&b"pi"[..], 0x03C0),
        (&b"rho"[..], 0x03C1),
        (&b"sigmaf"[..], 0x03C2),
        (&b"sigma"[..], 0x03C3),
        (&b"tau"[..], 0x03C4),
        (&b"upsilon"[..], 0x03C5),
        (&b"phi"[..], 0x03C6),
        (&b"chi"[..], 0x03C7),
        (&b"psi"[..], 0x03C8),
        (&b"omega"[..], 0x03C9),
        (&b"thetasym"[..], 0x03D1),
        (&b"upsih"[..], 0x03D2),
        (&b"piv"[..], 0x03D6),
        (&b"ensp"[..], 0x2002),
        (&b"emsp"[..], 0x2003),
        (&b"thinsp"[..], 0x2009),
        (&b"zwnj"[..], 0x200C),
        (&b"zwj"[..], 0x200D),
        (&b"lrm"[..], 0x200E),
        (&b"rlm"[..], 0x200F),
        (&b"ndash"[..], 0x2013),
        (&b"mdash"[..], 0x2014),
        (&b"lsquo"[..], 0x2018),
        (&b"rsquo"[..], 0x2019),
        (&b"sbquo"[..], 0x201A),
        (&b"ldquo"[..], 0x201C),
        (&b"rdquo"[..], 0x201D),
        (&b"bdquo"[..], 0x201E),
        (&b"dagger"[..], 0x2020),
        (&b"Dagger"[..], 0x2021),
        (&b"bull"[..], 0x2022),
        (&b"hellip"[..], 0x2026),
        (&b"permil"[..], 0x2030),
        (&b"prime"[..], 0x2032),
        (&b"Prime"[..], 0x2033),
        (&b"lsaquo"[..], 0x2039),
        (&b"rsaquo"[..], 0x203A),
        (&b"oline"[..], 0x203E),
        (&b"frasl"[..], 0x2044),
        (&b"euro"[..], 0x20AC),
        (&b"image"[..], 0x2111),
        (&b"weierp"[..], 0x2118),
        (&b"real"[..], 0x211C),
        (&b"trade"[..], 0x2122),
        (&b"alefsym"[..], 0x2135),
        (&b"larr"[..], 0x2190),
        (&b"uarr"[..], 0x2191),
        (&b"rarr"[..], 0x2192),
        (&b"darr"[..], 0x2193),
        (&b"harr"[..], 0x2194),
        (&b"crarr"[..], 0x21B5),
        (&b"lArr"[..], 0x21D0),
        (&b"uArr"[..], 0x21D1),
        (&b"rArr"[..], 0x21D2),
        (&b"dArr"[..], 0x21D3),
        (&b"hArr"[..], 0x21D4),
        (&b"forall"[..], 0x2200),
        (&b"part"[..], 0x2202),
        (&b"exist"[..], 0x2203),
        (&b"empty"[..], 0x2205),
        (&b"nabla"[..], 0x2207),
        (&b"isin"[..], 0x2208),
        (&b"notin"[..], 0x2209),
        (&b"ni"[..], 0x220B),
        (&b"prod"[..], 0x220F),
        (&b"sum"[..], 0x2211),
        (&b"minus"[..], 0x2212),
        (&b"lowast"[..], 0x2217),
        (&b"radic"[..], 0x221A),
        (&b"prop"[..], 0x221D),
        (&b"infin"[..], 0x221E),
        (&b"ang"[..], 0x2220),
        (&b"and"[..], 0x2227),
        (&b"or"[..], 0x2228),
        (&b"cap"[..], 0x2229),
        (&b"cup"[..], 0x222A),
        (&b"int"[..], 0x222B),
        (&b"there4"[..], 0x2234),
        (&b"sim"[..], 0x223C),
        (&b"cong"[..], 0x2245),
        (&b"asymp"[..], 0x2248),
        (&b"ne"[..], 0x2260),
        (&b"equiv"[..], 0x2261),
        (&b"le"[..], 0x2264),
        (&b"ge"[..], 0x2265),
        (&b"sub"[..], 0x2282),
        (&b"sup"[..], 0x2283),
        (&b"nsub"[..], 0x2284),
        (&b"sube"[..], 0x2286),
        (&b"supe"[..], 0x2287),
        (&b"oplus"[..], 0x2295),
        (&b"otimes"[..], 0x2297),
        (&b"perp"[..], 0x22A5),
        (&b"sdot"[..], 0x22C5),
        (&b"lceil"[..], 0x2308),
        (&b"rceil"[..], 0x2309),
        (&b"lfloor"[..], 0x230A),
        (&b"rfloor"[..], 0x230B),
        (&b"lang"[..], 0x2329),
        (&b"rang"[..], 0x232A),
        (&b"loz"[..], 0x25CA),
        (&b"spades"[..], 0x2660),
        (&b"clubs"[..], 0x2663),
        (&b"hearts"[..], 0x2665),
        (&b"diams"[..], 0x2666),
    ]
    .into_iter()
    .collect()
});

/// A name with its namespace resolved: empty for none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    /// The namespace URI.
    pub namespace: Vec<u8>,
    /// The local part.
    pub local: Vec<u8>,
}

/// What the reader found next.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A start tag (an empty element gives `End` next).
    Start {
        /// The element's name.
        name: Name,
        /// Its attributes, `xmlns` declarations left out, in order.
        attributes: Vec<(Name, Vec<u8>)>,
    },
    /// The end of the innermost open element.
    End,
    /// Character data, references decoded.
    Text(Vec<u8>),
    /// A document type declaration without an internal subset.
    Doctype,
    /// The end of a well formed document.
    Eof,
}

/// Why a document is not read: libxml's message, and the line and column
/// (from 1) where it stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    /// The message, as PHP's XML extension words it.
    pub message: &'static str,
    /// The line.
    pub line: usize,
    /// The column.
    pub column: usize,
}

/// The reader.
pub struct Parser<'a> {
    input: &'a [u8],
    at: usize,
    html: bool,
    /// The open elements' qualified names, each with the prefixes it binds.
    open: Vec<(&'a [u8], Vec<&'a [u8]>)>,
    /// Each prefix's bindings, innermost last.
    bindings: HashMap<&'a [u8], Vec<Vec<u8>>>,
    /// An empty element's end, still to give.
    pending_end: bool,
    seen_root: bool,
}

/// XML's white space.
fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Whether `byte` may be part of a name.
fn is_name(byte: u8) -> bool {
    !is_space(byte) && !matches!(byte, b'<' | b'>' | b'/' | b'=' | b'"' | b'\'' | b'&' | b';')
}

impl<'a> Parser<'a> {
    /// A reader of `input`, with HTML 4's entities when `html`.
    pub fn new(input: &'a [u8], html: bool) -> Parser<'a> {
        let input = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
        Parser {
            input,
            at: 0,
            html,
            open: Vec::new(),
            bindings: HashMap::new(),
            pending_end: false,
            seen_root: false,
        }
    }

    /// An error with the position `at`.
    fn error(&self, message: &'static str, at: usize) -> Error {
        let before = self.input.get(..at).unwrap_or(self.input);
        let line_start = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |n| n + 1);
        Error {
            message,
            line: before.iter().filter(|&&b| b == b'\n').count() + 1,
            column: at - line_start + 1,
        }
    }

    fn rest(&self) -> &'a [u8] {
        self.input.get(self.at..).unwrap_or_default()
    }

    /// The position of `needle` from the current one.
    fn find(&self, needle: &[u8]) -> Option<usize> {
        self.rest()
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|at| self.at + at)
    }

    /// The next event.
    pub fn next(&mut self) -> Result<Event, Error> {
        if self.pending_end {
            self.pending_end = false;
            return Ok(self.close());
        }
        loop {
            let rest = self.rest();
            if rest.is_empty() {
                return match self.open.is_empty() && self.seen_root {
                    true => Ok(Event::Eof),
                    false => Err(self.error("Invalid document end", self.at)),
                };
            }
            if !rest.starts_with(b"<") {
                let end = self.find(b"<").unwrap_or(self.input.len());
                let raw = self.input.get(self.at..end).unwrap_or_default();
                let start = self.at;
                self.at = end;
                if self.open.is_empty() {
                    if raw.iter().all(|&b| is_space(b)) {
                        continue;
                    }
                    let message = match self.seen_root {
                        true => "Extra content at the end of the document",
                        false => "Start tag expected, '<' not found",
                    };
                    return Err(self.error(message, start));
                }
                return self.text(raw, start).map(Event::Text);
            }
            if rest.starts_with(b"<!--") {
                let end = self
                    .find(b"-->")
                    .ok_or_else(|| self.error("Comment not terminated", self.at))?;
                self.at = end + 3;
                continue;
            }
            if rest.starts_with(b"<![CDATA[") {
                if self.open.is_empty() {
                    return Err(self.error("Extra content at the end of the document", self.at));
                }
                let end = self
                    .find(b"]]>")
                    .ok_or_else(|| self.error("CData section not finished", self.at))?;
                let data = self.input.get(self.at + 9..end).unwrap_or_default();
                self.at = end + 3;
                return Ok(Event::Text(newlines(data)));
            }
            if rest.starts_with(b"<?") {
                let end = self
                    .find(b"?>")
                    .ok_or_else(|| self.error("ParsePI: PI xml space expected", self.at))?;
                self.at = end + 2;
                continue;
            }
            if rest.starts_with(b"<!DOCTYPE") {
                let start = self.at;
                let end = self
                    .find(b">")
                    .ok_or_else(|| self.error("DOCTYPE improperly terminated", start))?;
                if self
                    .input
                    .get(start..end)
                    .is_some_and(|d| d.contains(&b'['))
                {
                    return Err(self.error("Internal subsets are not read", start));
                }
                self.at = end + 1;
                return Ok(Event::Doctype);
            }
            if rest.starts_with(b"</") {
                return self.end_tag();
            }
            return self.start_tag();
        }
    }

    /// The end of the innermost element, its bindings dropped.
    fn close(&mut self) -> Event {
        for prefix in self.open.pop().map(|(_, bound)| bound).unwrap_or_default() {
            if let Some(uris) = self.bindings.get_mut(prefix) {
                uris.pop();
            }
        }
        Event::End
    }

    fn end_tag(&mut self) -> Result<Event, Error> {
        let start = self.at + 2;
        let length = self
            .rest()
            .get(2..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| is_name(b) || b == b'/')
            .count();
        let name = self.input.get(start..start + length).unwrap_or_default();
        let after = start + length;
        let spaces = self
            .input
            .get(after..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| is_space(b))
            .count();
        if self.input.get(after + spaces) != Some(&b'>') {
            return Err(self.error("expected '>'", after + spaces));
        }
        self.at = after + spaces + 1;
        if self.open.last().map(|(open, _)| *open) != Some(name) {
            return Err(self.error("Mismatched tag", self.at));
        }
        Ok(self.close())
    }

    fn start_tag(&mut self) -> Result<Event, Error> {
        let start = self.at;
        if self.open.is_empty() && self.seen_root {
            return Err(self.error("Extra content at the end of the document", start));
        }
        let mut at = start + 1;
        let length = self
            .input
            .get(at..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| is_name(b))
            .count();
        if length == 0 {
            return Err(self.error("StartTag: invalid element name", at));
        }
        let qname = self.input.get(at..at + length).unwrap_or_default();
        at += length;
        let mut raw = Vec::new();
        let mut names = HashSet::new();
        let empty = loop {
            let spaces = self
                .input
                .get(at..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| is_space(b))
                .count();
            at += spaces;
            match self.input.get(at) {
                None => return Err(self.error("Couldn't find end of Start Tag", at)),
                Some(b'>') => break false,
                Some(b'/') if self.input.get(at + 1) == Some(&b'>') => {
                    at += 1;
                    break true;
                }
                Some(_) if spaces == 0 => return Err(self.error("attributes construct error", at)),
                Some(_) => {}
            }
            let name_length = self
                .input
                .get(at..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| is_name(b))
                .count();
            let name = self.input.get(at..at + name_length).unwrap_or_default();
            at += name_length;
            at += self
                .input
                .get(at..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| is_space(b))
                .count();
            if name_length == 0 || self.input.get(at) != Some(&b'=') {
                return Err(self.error("Specification mandates value for attribute", at));
            }
            at += 1;
            at += self
                .input
                .get(at..)
                .unwrap_or_default()
                .iter()
                .take_while(|&&b| is_space(b))
                .count();
            let quote = *self
                .input
                .get(at)
                .filter(|q| matches!(q, b'"' | b'\''))
                .ok_or_else(|| self.error("AttValue: \" or ' expected", at))?;
            let value_start = at + 1;
            let close = self
                .input
                .get(value_start..)
                .unwrap_or_default()
                .iter()
                .position(|&b| b == quote || b == b'<');
            let close = close
                .filter(|&c| self.input.get(value_start + c) == Some(&quote))
                .ok_or_else(|| {
                    self.error(
                        "Unescaped '<' not allowed in attributes values",
                        value_start,
                    )
                })?;
            let value = self
                .input
                .get(value_start..value_start + close)
                .unwrap_or_default();
            // Literal white space in a value reads as spaces.
            let spaced: Vec<u8> = newlines(value)
                .into_iter()
                .map(|b| if matches!(b, b'\t' | b'\n') { b' ' } else { b })
                .collect();
            let value = self.text(&spaced, value_start)?;
            if !names.insert(name) {
                return Err(self.error("Attribute redefined", at));
            }
            raw.push((name, value));
            at = value_start + close + 1;
        };
        self.at = at + 1;
        if self.open.len() == MAX_DEPTH {
            return Err(self.error(
                "Excessive depth in document: 256 use XML_PARSE_HUGE option",
                start,
            ));
        }
        self.seen_root = true;
        let mut attributes = Vec::new();
        let mut bound = Vec::new();
        for (name, value) in raw {
            let prefix = match name.strip_prefix(b"xmlns") {
                Some(b"") => Some(&b""[..]),
                Some(prefixed) => prefixed.strip_prefix(b":"),
                None => None,
            };
            match prefix {
                Some(prefix) => {
                    self.bindings.entry(prefix).or_default().push(value);
                    bound.push(prefix);
                }
                None => attributes.push((name, value)),
            }
        }
        self.open.push((qname, bound));
        let name = self
            .resolve(qname, true)
            .ok_or_else(|| self.error("Namespace prefix is not defined", start))?;
        let attributes = attributes
            .into_iter()
            .map(|(name, value)| Some((self.resolve(name, false)?, value)))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| self.error("Namespace prefix is not defined", start))?;
        self.pending_end = empty;
        Ok(Event::Start { name, attributes })
    }

    /// `qname` with its prefix bound; unprefixed attributes have none.
    fn resolve(&self, qname: &[u8], element: bool) -> Option<Name> {
        let (prefix, local) = match qname.iter().position(|&b| b == b':') {
            Some(colon) => (qname.get(..colon)?, qname.get(colon + 1..)?),
            None if !element => {
                return Some(Name {
                    namespace: Vec::new(),
                    local: qname.to_vec(),
                });
            }
            None => (&b""[..], qname),
        };
        let namespace = match prefix {
            b"xml" => NAMESPACE_XML.to_vec(),
            _ => match self.bindings.get(prefix).and_then(|uris| uris.last()) {
                Some(uri) => uri.clone(),
                None if prefix.is_empty() => Vec::new(),
                None => return None,
            },
        };
        Some(Name {
            namespace,
            local: local.to_vec(),
        })
    }

    /// `raw` with its line ends normalized, then its references decoded.
    fn text(&self, raw: &[u8], start: usize) -> Result<Vec<u8>, Error> {
        let raw = &newlines(raw);
        let mut out = Vec::with_capacity(raw.len());
        let mut at = 0;
        while let Some(amp) = raw
            .get(at..)
            .and_then(|rest| rest.iter().position(|&b| b == b'&'))
        {
            out.extend(raw.get(at..at + amp).unwrap_or_default());
            let from = at + amp + 1;
            let end = raw
                .get(from..)
                .unwrap_or_default()
                .iter()
                .position(|&b| b == b';');
            let name = end
                .and_then(|end| raw.get(from..from + end))
                .ok_or_else(|| self.error("EntityRef: expecting ';'", start + from))?;
            let code = match name {
                b"lt" => Some(u32::from('<')),
                b"gt" => Some(u32::from('>')),
                b"amp" => Some(u32::from('&')),
                b"apos" => Some(u32::from('\'')),
                b"quot" => Some(u32::from('"')),
                _ if name.first() == Some(&b'#') => {
                    let digits = name.get(1..).unwrap_or_default();
                    let parsed = match digits.strip_prefix(b"x") {
                        Some(hex) => std::str::from_utf8(hex)
                            .ok()
                            .filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()))
                            .and_then(|h| u32::from_str_radix(h, 16).ok()),
                        None => std::str::from_utf8(digits)
                            .ok()
                            .filter(|d| d.bytes().all(|b| b.is_ascii_digit()))
                            .and_then(|d| d.parse().ok()),
                    };
                    let valid = parsed.filter(|&c| {
                        c == 0x9
                            || c == 0xA
                            || c == 0xD
                            || (0x20..=0xD7FF).contains(&c)
                            || (0xE000..=0xFFFD).contains(&c)
                            || (0x10000..=0x10FFFF).contains(&c)
                    });
                    Some(valid.ok_or_else(|| {
                        self.error("xmlParseCharRef: invalid xmlChar value", start + from)
                    })?)
                }
                _ if self.html => HTML.get(name).copied(),
                _ => None,
            };
            let code = code.ok_or_else(|| self.error("Undeclared entity", start + from))?;
            let mut buffer = [0; 4];
            out.extend(
                char::from_u32(code)
                    .unwrap_or('\u{FFFD}')
                    .encode_utf8(&mut buffer)
                    .as_bytes(),
            );
            at = from + name.len() + 1;
        }
        out.extend(raw.get(at..).unwrap_or_default());
        Ok(out)
    }
}

/// `\r\n` and a lone `\r` as `\n`, as XML reads line ends.
fn newlines(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.iter().peekable();
    while let Some(&byte) = bytes.next() {
        if byte == b'\r' {
            out.push(b'\n');
            if bytes.peek() == Some(&&b'\n') {
                bytes.next();
            }
        } else {
            out.push(byte);
        }
    }
    out
}

#[cfg(test)]
mod tests;
