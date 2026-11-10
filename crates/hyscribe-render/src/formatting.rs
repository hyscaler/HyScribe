// SPDX-License-Identifier: AGPL-3.0-or-later

//! Ports of WordPress's `wptexturize()`, `convert_smilies()`,
//! `convert_chars()` and `capital_P_dangit()` from
//! `wp-includes/formatting.php`, byte for byte, for locale en_US and the
//! default filters.
//!
//! Every regex is matched by hand with PCRE's semantics (leftmost match,
//! lookarounds against the subject of that one replacement, ASCII `\d`,
//! `\s` and `\b`). Each pass is linear in the input: no scan restarts from
//! a position an earlier failed scan already covered.

/// The shortcodes WordPress core registers, in registration order.
pub const CORE_SHORTCODE_TAGS: [&[u8]; 7] = [
    b"wp_caption",
    b"caption",
    b"gallery",
    b"playlist",
    b"audio",
    b"video",
    b"embed",
];

/// A match: its length and what replaces it.
type Match = Option<(usize, Vec<u8>)>;

/// A pattern tried at one position of the subject.
type Pattern = fn(&[u8], usize) -> Match;

/// `wp_spaces_regexp()`: `[\r\n\t ]|\xC2\xA0|&nbsp;`.
const SPACES: [&[u8]; 6] = [b"\r", b"\n", b"\t", b" ", b"\xC2\xA0", b"&nbsp;"];

const NO_TEXTURIZE_TAGS: [&[u8]; 6] = [b"pre", b"code", b"kbd", b"style", b"script", b"tt"];
const NO_TEXTURIZE_SHORTCODES: [&[u8]; 1] = [b"code"];

const OPEN_Q_FLAG: &[u8] = b"<!--oq-->";
const OPEN_SQ_FLAG: &[u8] = b"<!--osq-->";
const APOS_FLAG: &[u8] = b"<!--apos-->";
const PRIME_FLAG: &[u8] = b"<!--wp-prime-or-quote-->";
const CLOSE_Q: &[u8] = b"&#8221;";
const CLOSE_SQ: &[u8] = b"&#8217;";

/// `$static_characters` and `$static_replacements`, applied in order.
const STATIC: [(&[u8], &[u8]); 15] = [
    (b"...", b"&#8230;"),
    (b"``", b"&#8220;"),
    (b"''", CLOSE_Q),
    (b" (tm)", b" &#8482;"),
    (b"'tain't", b"&#8217;tain&#8217;t"),
    (b"'twere", b"&#8217;twere"),
    (b"'twas", b"&#8217;twas"),
    (b"'tis", b"&#8217;tis"),
    (b"'twill", b"&#8217;twill"),
    (b"'til", b"&#8217;til"),
    (b"'bout", b"&#8217;bout"),
    (b"'nuff", b"&#8217;nuff"),
    (b"'round", b"&#8217;round"),
    (b"'cause", b"&#8217;cause"),
    (b"'em", b"&#8217;em"),
];

/// `$dynamic_characters['apos']`, applied in order.
const APOSTROPHES: [Pattern; 6] = [
    |s, i| year_quoted(s, i, b'\'', CLOSE_SQ),
    |s, i| year_quoted(s, i, b'"', CLOSE_Q),
    abbreviated_year,
    |s, i| quoted_number(s, i, b'\'', OPEN_SQ_FLAG, CLOSE_SQ),
    opening_single,
    apostrophe_in_word,
];

/// `$dynamic_characters['quote']`, applied in order.
const QUOTES: [Pattern; 2] = [
    |s, i| quoted_number(s, i, b'"', OPEN_Q_FLAG, CLOSE_Q),
    opening_double,
];

/// `$dynamic_characters['dash']`, applied in order.
const DASHES: [Pattern; 4] = [
    |s, i| has(s, i, b"---").then(|| (3, b"&#8212;".to_vec())),
    |s, i| {
        (has(s, i, b"--") && start_or_space(s, i) && end_or_space(s, i + 2))
            .then(|| (2, b"&#8212;".to_vec()))
    },
    |s, i| (has(s, i, b"--") && !ends(s, i, b"xn")).then(|| (2, b"&#8211;".to_vec())),
    |s, i| {
        (byte(s, i) == Some(b'-') && start_or_space(s, i) && end_or_space(s, i + 1))
            .then(|| (1, b"&#8211;".to_vec()))
    },
];

/// `$wpsmiliestrans` after `smilies_init()`: the code and the emoji, or
/// the image file under `wp-includes/images/smilies/`.
const SMILIES: [(&[u8], &[u8]); 43] = [
    (b":mrgreen:", b"mrgreen.png"),
    (b":neutral:", "\u{1f610}".as_bytes()),
    (b":twisted:", "\u{1f608}".as_bytes()),
    (b":arrow:", "\u{27a1}".as_bytes()),
    (b":shock:", "\u{1f62f}".as_bytes()),
    (b":smile:", "\u{1f642}".as_bytes()),
    (b":???:", "\u{1f615}".as_bytes()),
    (b":cool:", "\u{1f60e}".as_bytes()),
    (b":evil:", "\u{1f47f}".as_bytes()),
    (b":grin:", "\u{1f600}".as_bytes()),
    (b":idea:", "\u{1f4a1}".as_bytes()),
    (b":oops:", "\u{1f633}".as_bytes()),
    (b":razz:", "\u{1f61b}".as_bytes()),
    (b":roll:", "\u{1f644}".as_bytes()),
    (b":wink:", "\u{1f609}".as_bytes()),
    (b":cry:", "\u{1f625}".as_bytes()),
    (b":eek:", "\u{1f62e}".as_bytes()),
    (b":lol:", "\u{1f606}".as_bytes()),
    (b":mad:", "\u{1f621}".as_bytes()),
    (b":sad:", "\u{1f641}".as_bytes()),
    (b"8-)", "\u{1f60e}".as_bytes()),
    (b"8-O", "\u{1f62f}".as_bytes()),
    (b":-(", "\u{1f641}".as_bytes()),
    (b":-)", "\u{1f642}".as_bytes()),
    (b":-?", "\u{1f615}".as_bytes()),
    (b":-D", "\u{1f600}".as_bytes()),
    (b":-P", "\u{1f61b}".as_bytes()),
    (b":-o", "\u{1f62e}".as_bytes()),
    (b":-x", "\u{1f621}".as_bytes()),
    (b":-|", "\u{1f610}".as_bytes()),
    (b";-)", "\u{1f609}".as_bytes()),
    (b"8O", "\u{1f62f}".as_bytes()),
    (b":(", "\u{1f641}".as_bytes()),
    (b":)", "\u{1f642}".as_bytes()),
    (b":?", "\u{1f615}".as_bytes()),
    (b":D", "\u{1f600}".as_bytes()),
    (b":P", "\u{1f61b}".as_bytes()),
    (b":o", "\u{1f62e}".as_bytes()),
    (b":x", "\u{1f621}".as_bytes()),
    (b":|", "\u{1f610}".as_bytes()),
    (b";)", "\u{1f609}".as_bytes()),
    (b":!:", "\u{2757}".as_bytes()),
    (b":?:", "\u{2753}".as_bytes()),
];

/// `convert_smilies()` leaves the content of these elements alone.
const SMILIES_IGNORED: [&[u8]; 5] = [b"code", b"pre", b"style", b"script", b"textarea"];

/// Replaces quotes, apostrophes, dashes, ellipses, primes and `x` between
/// numbers with their typographic entities, outside HTML tags, comments,
/// shortcodes and the content of `<pre>`, `<code>`, `<kbd>`, `<style>`,
/// `<script>`, `<tt>` and `[code]`, as WordPress's `wptexturize()` does.
/// `shortcode_tags` are the registered shortcode names, in registration
/// order (`array_keys( $shortcode_tags )`).
pub fn wptexturize(text: &[u8], shortcode_tags: &[&[u8]]) -> Vec<u8> {
    let found = shortcode_names(text);
    let names: Vec<&[u8]> = shortcode_tags
        .iter()
        .copied()
        .filter(|name| found.contains(name))
        .collect();
    let mut tags = Vec::new();
    let mut shortcodes = Vec::new();
    let mut out = Vec::with_capacity(text.len());
    for (piece, kind) in split(text, &names) {
        match kind {
            Piece::Html if piece.starts_with(b"<!--") => out.extend_from_slice(piece),
            Piece::Html => {
                let element = replace_matches(piece, ampersand).0;
                pushpop(&element, &mut tags, &NO_TEXTURIZE_TAGS);
                out.extend(element);
            }
            Piece::Shortcode => {
                if !piece.starts_with(b"[[") && !piece.ends_with(b"]]") {
                    pushpop(piece, &mut shortcodes, &NO_TEXTURIZE_SHORTCODES);
                }
                out.extend_from_slice(piece);
            }
            Piece::Text if tags.is_empty() && shortcodes.is_empty() => out.extend(texturize(piece)),
            Piece::Text => out.extend_from_slice(piece),
        }
    }
    out
}

/// Replaces text smilies such as `:)` with emoji, or for `:mrgreen:` with
/// an image, outside tags and the content of `<code>`, `<pre>`, `<style>`,
/// `<script>` and `<textarea>`, as WordPress's `convert_smilies()` does
/// with `use_smilies` on. `smilies_url` is the escaped
/// `includes_url( 'images/smilies/' )`.
pub fn convert_smilies(text: &[u8], smilies_url: &[u8]) -> Vec<u8> {
    let mut ignored: Option<&[u8]> = None;
    let mut out = Vec::with_capacity(text.len());
    for piece in split_tags(text) {
        if ignored.is_none() {
            ignored = SMILIES_IGNORED.into_iter().find(|name| {
                piece
                    .strip_prefix(b"<")
                    .and_then(|rest| rest.strip_prefix(*name))
                    .is_some_and(|rest| rest.contains(&b'>'))
            });
        }
        if ignored.is_none() && piece.first().is_some_and(|&first| first != b'<') {
            out.extend(replace_matches(piece, |s, i| smiley(s, i, smilies_url)).0);
        } else {
            out.extend_from_slice(piece);
        }
        if ignored.is_some_and(|name| piece == [b"</", name, b">"].concat()) {
            ignored = None;
        }
    }
    out
}

fn byte(s: &[u8], i: usize) -> Option<u8> {
    s.get(i).copied()
}

fn before(s: &[u8], i: usize) -> Option<u8> {
    i.checked_sub(1).and_then(|j| byte(s, j))
}

fn digit(s: &[u8], i: usize) -> bool {
    byte(s, i).is_some_and(|c| c.is_ascii_digit())
}

fn digit_before(s: &[u8], i: usize) -> bool {
    before(s, i).is_some_and(|c| c.is_ascii_digit())
}

pub(crate) fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `s[i..]` starts with `literal`.
fn has(s: &[u8], i: usize, literal: &[u8]) -> bool {
    s.get(i..i + literal.len()) == Some(literal)
}

/// `s[..i]` ends with `literal`.
fn ends(s: &[u8], i: usize, literal: &[u8]) -> bool {
    s.get(..i).is_some_and(|head| head.ends_with(literal))
}

/// `(?<=spaces)`.
fn space_before(s: &[u8], i: usize) -> bool {
    SPACES.iter().any(|space| ends(s, i, space))
}

/// `(?=spaces)`.
fn space_at(s: &[u8], i: usize) -> bool {
    SPACES.iter().any(|space| has(s, i, space))
}

/// `(?<=\A|spaces)`.
fn start_or_space(s: &[u8], i: usize) -> bool {
    i == 0 || space_before(s, i)
}

/// `(?=\z|spaces)`; `\Z` and `$` only differ before a final newline, which
/// is a space too.
fn end_or_space(s: &[u8], i: usize) -> bool {
    i == s.len() || space_at(s, i)
}

/// `(?=\Z|[.,:;!?)}\-\]]|&gt;|spaces)`: what may follow a closing quote.
fn closer(s: &[u8], i: usize) -> bool {
    end_or_space(s, i)
        || matches!(
            byte(s, i),
            Some(b'.' | b',' | b':' | b';' | b'!' | b'?' | b')' | b'}' | b'-' | b']')
        )
        || has(s, i, b"&gt;")
}

/// Where the run of `[\d.,]` starting at `i` ends.
fn number(s: &[u8], i: usize) -> usize {
    i + s
        .get(i..)
        .unwrap_or_default()
        .iter()
        .take_while(|c| matches!(c, b'0'..=b'9' | b'.' | b','))
        .count()
}

/// The next `needle` at or after an offset that only grows, remembered so
/// that the searches stay linear.
pub(crate) struct Next<'a> {
    haystack: &'a [u8],
    needle: &'a [u8],
    found: Option<Option<usize>>,
}

impl<'a> Next<'a> {
    pub(crate) fn new(haystack: &'a [u8], needle: &'a [u8]) -> Next<'a> {
        Next {
            haystack,
            needle,
            found: None,
        }
    }

    pub(crate) fn from(&mut self, from: usize) -> Option<usize> {
        if let Some(found) = self.found
            && found.is_none_or(|at| at >= from)
        {
            return found;
        }
        let found = find(self.haystack, from, self.needle);
        self.found = Some(found);
        found
    }
}

/// `str_contains( $text, $needle )`.
pub(crate) fn contains(text: &[u8], needle: &[u8]) -> bool {
    find(text, 0, needle).is_some()
}

pub(crate) fn find(s: &[u8], from: usize, literal: &[u8]) -> Option<usize> {
    s.get(from..)
        .unwrap_or_default()
        .windows(literal.len())
        .position(|window| window == literal)
        .map(|at| at + from)
}

/// `preg_replace()` of one pattern, with the number of replacements.
fn replace_matches(s: &[u8], mut pattern: impl FnMut(&[u8], usize) -> Match) -> (Vec<u8>, usize) {
    let mut out = Vec::with_capacity(s.len());
    let mut count = 0;
    let mut next = 0;
    for (i, &c) in s.iter().enumerate() {
        if i < next {
            continue;
        }
        match pattern(s, i) {
            Some((length, with)) => {
                out.extend(with);
                count += 1;
                next = i + length;
            }
            None => out.push(c),
        }
    }
    (out, count)
}

/// `str_replace()` of one string.
fn replace(s: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    replace_matches(s, |s, i| has(s, i, from).then(|| (from.len(), to.to_vec()))).0
}

/// `explode()`.
fn explode<'a>(s: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    let mut parts = Vec::new();
    let mut from = 0;
    // Every round but the last consumes a separator.
    for _ in 0..=s.len() {
        let Some(at) = find(s, from, separator) else {
            break;
        };
        parts.push(s.get(from..at).unwrap_or_default());
        from = at + separator.len();
    }
    parts.push(s.get(from..).unwrap_or_default());
    parts
}

#[derive(Clone, Copy)]
enum Piece {
    Html,
    Shortcode,
    Text,
}

/// The names `[name` or `[/name` mention, as
/// `preg_match_all( '@\[/?([^<>&/\[\]\x00-\x20=]++)@' )` finds them.
fn shortcode_names(text: &[u8]) -> Vec<&[u8]> {
    let mut names = Vec::new();
    for (i, &c) in text.iter().enumerate() {
        if c == b'[' {
            let from = i + 1 + usize::from(byte(text, i + 1) == Some(b'/'));
            let rest = text.get(from..).unwrap_or_default();
            let length = rest
                .iter()
                .take_while(|&&c| c > b' ' && !b"<>&/[]=".contains(&c))
                .count();
            if length > 0 {
                names.push(rest.get(..length).unwrap_or_default());
            }
        }
    }
    names
}

/// `preg_split()` with `_get_wptexturize_split_regex()`: HTML elements,
/// comments and shortcodes of `names`, and the text between them.
fn split<'a>(text: &'a [u8], names: &[&[u8]]) -> Vec<(&'a [u8], Piece)> {
    let mut pieces = Vec::new();
    let mut from = 0;
    for (i, &c) in text.iter().enumerate() {
        if i < from {
            continue;
        }
        let delimiter = match c {
            b'<' => Some((html_end(text, i), Piece::Html)),
            b'[' => shortcode_end(text, i, names).map(|end| (end, Piece::Shortcode)),
            _ => None,
        };
        if let Some((end, kind)) = delimiter {
            pieces.push((text.get(from..i).unwrap_or_default(), Piece::Text));
            pieces.push((text.get(i..end).unwrap_or_default(), kind));
            from = end;
        }
    }
    pieces.push((text.get(from..).unwrap_or_default(), Piece::Text));
    pieces
}

/// The end of the element or comment starting at `<` at `i`: up to `-->`
/// or `>`, or the end of the text when there is none.
fn html_end(s: &[u8], i: usize) -> usize {
    if has(s, i + 1, b"!--") {
        find(s, i + 2, b"-->").map_or(s.len(), |at| at + 3)
    } else {
        find(s, i, b">").map_or(s.len(), |at| at + 1)
    }
}

/// `_get_wptexturize_shortcode_regex()` at `[` at `i`.
fn shortcode_end(s: &[u8], i: usize, names: &[&[u8]]) -> Option<usize> {
    let prefix = usize::from(matches!(byte(s, i + 1), Some(b'/' | b'[')));
    (0..=prefix).rev().find_map(|skip| {
        let start = i + 1 + skip;
        names.iter().find_map(|name| {
            let after = start + name.len();
            if !has(s, start, name)
                || !matches!(
                    byte(s, after),
                    Some(b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b']' | b'/')
                )
            {
                return None;
            }
            let end = shortcode_body_end(s, after);
            (byte(s, end) == Some(b']'))
                .then(|| end + 1 + usize::from(byte(s, end + 1) == Some(b']')))
        })
    })
}

/// `(?:[^\[\]<>]+|<[^\[\]>]*>)*+` from `from`.
fn shortcode_body_end(s: &[u8], from: usize) -> usize {
    let mut end = from;
    for (k, &c) in s.iter().enumerate().skip(from) {
        if k < end {
            continue;
        }
        match c {
            b'[' | b']' | b'>' => break,
            b'<' => {
                let stop = s
                    .get(k + 1..)
                    .unwrap_or_default()
                    .iter()
                    .position(|c| matches!(c, b'[' | b']' | b'>'))
                    .map(|at| at + k + 1);
                match stop {
                    Some(close) if byte(s, close) == Some(b'>') => end = close + 1,
                    _ => break,
                }
            }
            _ => end = k + 1,
        }
    }
    end
}

/// `_wptexturize_pushpop_element()`.
fn pushpop(text: &[u8], stack: &mut Vec<&'static [u8]>, disabled: &[&'static [u8]]) {
    let opening = byte(text, 1).is_some_and(|c| c != b'/');
    let offset = if opening { 1 } else { 2 };
    let end = text
        .iter()
        .position(|&c| c == b' ')
        .unwrap_or(text.len().saturating_sub(1));
    let tag = text.get(offset..end).unwrap_or_default();
    if let Some(&name) = disabled.iter().find(|&&name| name == tag) {
        if opening {
            stack.push(name);
        } else if stack.last() == Some(&name) {
            stack.pop();
        }
    }
}

/// The texturizing of text outside tags and shortcodes.
fn texturize(text: &[u8]) -> Vec<u8> {
    let mut text = text.to_vec();
    for (from, to) in STATIC {
        text = replace(&text, from, to);
    }
    if text.contains(&b'\'') {
        for pattern in APOSTROPHES {
            text = replace_matches(&text, pattern).0;
        }
        text = primes(&text, b'\'', b"&#8242;", OPEN_SQ_FLAG, CLOSE_SQ);
        text = replace(&text, APOS_FLAG, CLOSE_SQ);
        text = replace(&text, OPEN_SQ_FLAG, b"&#8216;");
    }
    if text.contains(&b'"') {
        for pattern in QUOTES {
            text = replace_matches(&text, pattern).0;
        }
        text = primes(&text, b'"', b"&#8243;", OPEN_Q_FLAG, CLOSE_Q);
        text = replace(&text, OPEN_Q_FLAG, b"&#8220;");
    }
    if text.contains(&b'-') {
        for pattern in DASHES {
            text = replace_matches(&text, pattern).0;
        }
    }
    if text
        .windows(3)
        .any(|w| matches!(w, [b'0'..=b'9', b'x', b'0'..=b'9']))
    {
        text = replace_matches(&text, times).0;
    }
    replace_matches(&text, ampersand).0
}

/// `'(\d\d)'` or `'(\d\d)"` before a closer: a year at the end of a quote.
fn year_quoted(s: &[u8], i: usize, quote: u8, close: &[u8]) -> Match {
    (byte(s, i) == Some(b'\'')
        && digit(s, i + 1)
        && digit(s, i + 2)
        && byte(s, i + 3) == Some(quote)
        && closer(s, i + 4))
    .then(|| {
        let year = s.get(i + 1..i + 3).unwrap_or_default();
        (4, [APOS_FLAG, year, close].concat())
    })
}

/// `'(?=\d\d(?:\Z|(?![%\d]|[.,]\d)))`: `'99`, but not `'999`, `'99%`
/// or `'99.0`.
fn abbreviated_year(s: &[u8], i: usize) -> Match {
    (byte(s, i) == Some(b'\'')
        && digit(s, i + 1)
        && digit(s, i + 2)
        && !matches!(byte(s, i + 3), Some(b'%' | b'0'..=b'9'))
        && !(matches!(byte(s, i + 3), Some(b'.' | b',')) && digit(s, i + 4)))
    .then(|| (1, APOS_FLAG.to_vec()))
}

/// `(?<=\A|spaces)Q(\d[.,\d]*)Q`: a quoted number.
fn quoted_number(s: &[u8], i: usize, quote: u8, flag: &[u8], close: &[u8]) -> Match {
    if byte(s, i) != Some(quote) || !start_or_space(s, i) || !digit(s, i + 1) {
        return None;
    }
    let end = number(s, i + 1);
    (byte(s, end) == Some(quote)).then(|| {
        let number = s.get(i + 1..end).unwrap_or_default();
        (end + 1 - i, [flag, number, close].concat())
    })
}

/// `(?<=\A|[([{"\-]|&lt;|spaces)'`.
fn opening_single(s: &[u8], i: usize) -> Match {
    (byte(s, i) == Some(b'\'')
        && (start_or_space(s, i)
            || matches!(before(s, i), Some(b'(' | b'[' | b'{' | b'"' | b'-'))
            || ends(s, i, b"&lt;")))
    .then(|| (1, OPEN_SQ_FLAG.to_vec()))
}

/// `(?<!spaces)'(?!\Z|[.,:;!?"'(){}[\]\-]|&[lg]t;|spaces)`.
fn apostrophe_in_word(s: &[u8], i: usize) -> Match {
    (byte(s, i) == Some(b'\'')
        && !space_before(s, i)
        && !end_or_space(s, i + 1)
        && !matches!(
            byte(s, i + 1),
            Some(
                b'.' | b','
                    | b':'
                    | b';'
                    | b'!'
                    | b'?'
                    | b'"'
                    | b'\''
                    | b'('
                    | b')'
                    | b'{'
                    | b'}'
                    | b'['
                    | b']'
                    | b'-'
            )
        )
        && !has(s, i + 1, b"&lt;")
        && !has(s, i + 1, b"&gt;"))
    .then(|| (1, APOS_FLAG.to_vec()))
}

/// `(?<=\A|[([{\-]|&lt;|spaces)"(?!spaces)`.
fn opening_double(s: &[u8], i: usize) -> Match {
    (byte(s, i) == Some(b'"')
        && (start_or_space(s, i)
            || matches!(before(s, i), Some(b'(' | b'[' | b'{' | b'-'))
            || ends(s, i, b"&lt;"))
        && !space_at(s, i + 1))
    .then(|| (1, OPEN_Q_FLAG.to_vec()))
}

/// `wptexturize_primes()`: closing quotes and primes after the opening
/// quotes are flagged.
fn primes(text: &[u8], needle: u8, prime: &[u8], open: &[u8], close: &[u8]) -> Vec<u8> {
    let quote = |s: &[u8], i| {
        (byte(s, i) == Some(needle) && closer(s, i + 1)).then(|| (1, PRIME_FLAG.to_vec()))
    };
    let prime_at = |s: &[u8], i| {
        (byte(s, i) == Some(needle) && digit_before(s, i)).then(|| (1, prime.to_vec()))
    };
    let flag = |after_digit: bool, with: &[u8]| {
        let with = with.to_vec();
        move |s: &[u8], i: usize| {
            (has(s, i, PRIME_FLAG) && digit_before(s, i) == after_digit)
                .then(|| (PRIME_FLAG.len(), with.clone()))
        }
    };
    let sentences: Vec<Vec<u8>> = explode(text, open)
        .into_iter()
        .enumerate()
        .map(|(key, sentence)| {
            if !sentence.contains(&needle) {
                return sentence.to_vec();
            }
            let mut sentence = if key > 0 && find(sentence, 0, close).is_none() {
                // The closing quote candidates: the first ones not after a
                // digit close, else the rightmost before a period, else the
                // rightmost. WordPress only does this for several
                // candidates, but for one it comes to the same.
                let flagged = replace_matches(sentence, quote).0;
                let (mut closed, closes) = replace_matches(&flagged, flag(false, close));
                if closes == 0 {
                    let period = [PRIME_FLAG, b"."].concat();
                    if let Some(at) = rfind(&closed, &period).or_else(|| rfind(&closed, PRIME_FLAG))
                    {
                        closed = [
                            closed.get(..at).unwrap_or_default(),
                            close,
                            closed.get(at + PRIME_FLAG.len()..).unwrap_or_default(),
                        ]
                        .concat();
                    }
                }
                let primed = replace_matches(&closed, prime_at).0;
                let primed = replace_matches(&primed, flag(true, prime)).0;
                replace(&primed, PRIME_FLAG, close)
            } else {
                let primed = replace_matches(sentence, prime_at).0;
                replace_matches(&primed, |s, i| {
                    quote(s, i).map(|(length, _)| (length, close.to_vec()))
                })
                .0
            };
            if needle == b'"' {
                sentence = replace(&sentence, b"\"", close);
            }
            sentence
        })
        .collect();
    sentences.join(open)
}

fn rfind(s: &[u8], literal: &[u8]) -> Option<usize> {
    s.windows(literal.len())
        .rposition(|window| window == literal)
}

/// `/\b(\d(?(?<=0)[\d\.,]+|[\d\.,]*))x(\d[\d\.,]*)\b/` to `$1&#215;$2`.
fn times(s: &[u8], i: usize) -> Match {
    if !digit(s, i) || before(s, i).is_some_and(is_word) {
        return None;
    }
    let x = number(s, i);
    if byte(s, i) == Some(b'0') && x == i + 1 {
        return None;
    }
    let word = |k: usize| byte(s, k).is_some_and(is_word);
    let end = if byte(s, x) == Some(b'x') && digit(s, x + 1) {
        (x + 2..=number(s, x + 1))
            .rev()
            .find(|&k| word(k - 1) != word(k))
    } else {
        None
    };
    Some(match end {
        Some(end) => {
            let left = s.get(i..x).unwrap_or_default();
            let right = s.get(x + 1..end).unwrap_or_default();
            (end - i, [left, b"&#215;", right].concat())
        }
        // Every later start in this run fails the same way: skip it.
        None => (x - i, s.get(i..x).unwrap_or_default().to_vec()),
    })
}

/// `&(?!#(?:\d+|x[a-f0-9]+);|[a-z1-4]{1,8};)` (case-insensitive) to
/// `&#038;`.
fn ampersand(s: &[u8], i: usize) -> Match {
    if byte(s, i) != Some(b'&') {
        return None;
    }
    let run = |from: usize, keep: fn(&u8) -> bool| {
        s.get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|c| keep(c))
            .count()
    };
    let terminated =
        |from: usize, length: usize| length > 0 && byte(s, from + length) == Some(b';');
    let entity = match (byte(s, i + 1), byte(s, i + 2)) {
        (Some(b'#'), Some(b'x' | b'X')) => terminated(i + 3, run(i + 3, u8::is_ascii_hexdigit)),
        (Some(b'#'), _) => terminated(i + 2, run(i + 2, u8::is_ascii_digit)),
        _ => {
            let length = run(i + 1, |c| {
                c.is_ascii_alphabetic() || (b'1'..=b'4').contains(c)
            });
            length <= 8 && terminated(i + 1, length)
        }
    };
    (!entity).then(|| (1, b"&#038;".to_vec()))
}

/// `preg_split( '/(<[^>]*>)/U' )` with the delimiters.
fn split_tags(text: &[u8]) -> Vec<&[u8]> {
    let mut pieces = Vec::new();
    let mut from = 0;
    for (i, &c) in text.iter().enumerate() {
        if i < from || c != b'<' {
            continue;
        }
        let Some(close) = find(text, i, b">") else {
            break;
        };
        pieces.push(text.get(from..i).unwrap_or_default());
        pieces.push(text.get(i..=close).unwrap_or_default());
        from = close + 1;
    }
    pieces.push(text.get(from..).unwrap_or_default());
    pieces
}

/// `$wp_smiliessearch` and `translate_smiley()` at `i`.
fn smiley(s: &[u8], i: usize, smilies_url: &[u8]) -> Match {
    if !start_or_space(s, i) {
        return None;
    }
    SMILIES
        .iter()
        .find(|(code, _)| has(s, i, code) && end_or_space(s, i + code.len()))
        .map(|&(code, file)| {
            let with = if file.ends_with(b".png") {
                [
                    b"<img src=\"",
                    smilies_url,
                    file,
                    b"\" alt=\"",
                    code,
                    b"\" class=\"wp-smiley\" style=\"height: 1em; max-height: 1em;\" />",
                ]
                .concat()
            } else {
                file.to_vec()
            };
            (code.len(), with)
        })
}

/// `convert_chars()`: `/&([^#])(?![a-z1-4]{1,8};)/i` becomes `&#038;$1`,
/// so a bare `&` is encoded and named references stay.
pub fn convert_chars(text: &[u8]) -> Vec<u8> {
    encode_ampersands(text, true)
}

/// `/&([^#])(?![a-z]{1,8};)/i` becoming `&#038;$1`, with `1-4` among the
/// letters when `digits` is set.
pub(crate) fn encode_ampersands(text: &[u8], digits: bool) -> Vec<u8> {
    // `[a-z]{1,8};` at the start of `text`.
    let reference = |text: &[u8]| {
        let name = text
            .iter()
            .take_while(|b| b.is_ascii_alphabetic() || (digits && (b'1'..=b'4').contains(b)))
            .count();
        (1..=8).contains(&name) && text.get(name) == Some(&b';')
    };
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for _ in 0..=text.len() {
        let Some((&first, after)) = text.get(at..).and_then(<[u8]>::split_first) else {
            break;
        };
        match after.split_first() {
            Some((&next, tail)) if first == b'&' && next != b'#' && !reference(tail) => {
                out.extend_from_slice(b"&#038;");
                out.push(next);
                at += 2;
            }
            _ => {
                out.push(first);
                at += 1;
            }
        }
    }
    out
}

/// `ent2ncr()`: named entities become numeric ones, and `|` becomes
/// `&#124;`, as WordPress's table has it.
pub fn ent2ncr(text: &[u8]) -> Vec<u8> {
    let named = |name: &[u8]| {
        let (_, code) = entities::NAMED.iter().find(|(n, _)| *n == name)?;
        Some(format!("&#{code};").into_bytes())
    };
    references(&crate::template::replace(text, b"|", b"&#124;"), named)
}

/// `html_entity_decode( $text, ENT_COMPAT )`: HTML 4.01's named entities
/// and numeric references become characters, a single quote's excepted.
pub fn html_entity_decode(text: &[u8]) -> Vec<u8> {
    references(text, |reference| {
        entity(reference).map(|c| c.to_string().into_bytes())
    })
}

/// `text` with each `&reference;` that `replace` knows replaced.
fn references(text: &[u8], replace: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some((&first, tail)) = rest.split_first() {
        let reference = tail.split(|&b| b == b';').next().unwrap_or_default();
        let replaced = (first == b'&' && reference.len() < tail.len())
            .then(|| replace(reference))
            .flatten();
        rest = match replaced {
            Some(replaced) => {
                out.extend_from_slice(&replaced);
                tail.get(reference.len() + 1..).unwrap_or_default()
            }
            None => {
                out.push(first);
                tail
            }
        };
    }
    out
}

/// The value of one or more digits in `radix`, if it fits.
fn reference_number(digits: &[u8], radix: u32) -> Option<u32> {
    let value = digits.iter().try_fold(0_u64, |n, &b| {
        let digit = char::from(b).to_digit(radix)?;
        Some(
            n.saturating_mul(u64::from(radix))
                .saturating_add(u64::from(digit)),
        )
    })?;
    u32::try_from(value).ok().filter(|_| !digits.is_empty())
}

/// The character `&reference;` stands for under `ENT_COMPAT`: HTML 4.01
/// has `ent2ncr()`'s names but its three older ones.
fn entity(reference: &[u8]) -> Option<char> {
    let code = match reference {
        b"brkbar" | b"die" | b"hibar" => return None,
        [b'#', b'x' | b'X', hex @ ..] => reference_number(hex, 16)?,
        [b'#', decimal @ ..] => reference_number(decimal, 10)?,
        name => entities::NAMED.iter().find(|(n, _)| *n == name)?.1,
    };
    char::from_u32(code).filter(|&c| c != '\'')
}

/// `sanitize_title()`'s replacements in the save context, in order: what
/// becomes a hyphen, what is removed, what becomes a hyphen again, and
/// `&times`.
const TITLE_REPLACEMENTS: [(&[&[u8]], &[u8]); 6] = [
    (&[b"%c2%a0", b"%e2%80%91", b"%e2%80%93", b"%e2%80%94"], b"-"),
    (
        &[
            b"&nbsp;", b"&#8209;", b"&#160;", b"&ndash;", b"&#8211;", b"&mdash;", b"&#8212;",
        ],
        b"-",
    ),
    (&[b"/"], b"-"),
    (
        &[
            b"%c2%ad",
            b"%c2%a1",
            b"%c2%bf",
            b"%c2%ab",
            b"%c2%bb",
            b"%e2%80%b9",
            b"%e2%80%ba",
            b"%e2%80%98",
            b"%e2%80%99",
            b"%e2%80%9c",
            b"%e2%80%9d",
            b"%e2%80%9a",
            b"%e2%80%9b",
            b"%e2%80%9e",
            b"%e2%80%9f",
            b"%e2%80%a2",
            b"%c2%a9",
            b"%c2%ae",
            b"%c2%b0",
            b"%e2%80%a6",
            b"%e2%84%a2",
            b"%c2%b4",
            b"%cb%8a",
            b"%cc%81",
            b"%cd%81",
            b"%cc%80",
            b"%cc%84",
            b"%cc%8c",
            b"%e2%80%8b",
            b"%e2%80%8c",
            b"%e2%80%8d",
            b"%e2%80%8e",
            b"%e2%80%8f",
            b"%e2%80%aa",
            b"%e2%80%ab",
            b"%e2%80%ac",
            b"%e2%80%ad",
            b"%e2%80%ae",
            b"%ef%bb%bf",
            b"%ef%bf%bc",
        ],
        b"",
    ),
    (
        &[
            b"%e2%80%80",
            b"%e2%80%81",
            b"%e2%80%82",
            b"%e2%80%83",
            b"%e2%80%84",
            b"%e2%80%85",
            b"%e2%80%86",
            b"%e2%80%87",
            b"%e2%80%88",
            b"%e2%80%89",
            b"%e2%80%8a",
            b"%e2%80%a8",
            b"%e2%80%a9",
            b"%e2%80%af",
        ],
        b"-",
    ),
    (&[b"%c3%97"], b"x"),
];

/// `sanitize_title( $title )`, in the save context.
pub fn sanitize_title(title: &[u8]) -> Vec<u8> {
    sanitize_title_with_dashes(&remove_accents(title), true)
}

/// `sanitize_title_for_query( $title )`, as WordPress reads a slug from a
/// request.
pub fn sanitize_title_for_query(title: &[u8]) -> Vec<u8> {
    sanitize_title_with_dashes(title, false)
}

/// `sanitize_text_field( $text )`: invalid UTF-8 is dropped, tags are
/// stripped after `wp_pre_kses_less_than()` escapes each `<` opening none,
/// whitespace runs become one space, and percent-encoded octets go.
pub fn sanitize_text_field(text: &[u8]) -> Vec<u8> {
    if std::str::from_utf8(text).is_err() {
        return Vec::new();
    }
    let mut text = text.to_vec();
    if text.contains(&b'<') {
        text = hyscribe_kses::wp_strip_all_tags(&pre_kses_less_than(&text), false);
        text = replace(&text, b"<\n", b"&lt;\n");
    }
    let mut text = hyscribe_phpser::php_trim(&hyscribe_kses::collapse_breaks(&text)).to_vec();
    let octet = |t: &[u8]| {
        t.windows(3)
            .find(|w| matches!(w, [b'%', a, b] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit()))
            .map(<[u8]>::to_vec)
    };
    let mut found = false;
    while let Some(octet) = octet(&text) {
        text = replace(&text, &octet, b"");
        found = true;
    }
    // Its spaces collapse as `/ +/` would: no other whitespace is left.
    if found {
        text = hyscribe_phpser::php_trim(&hyscribe_kses::collapse_breaks(&text)).to_vec();
    }
    text
}

/// `wp_pre_kses_less_than()`: a `<` whose run up to the next `<` or the
/// end holds no `>` is HTML-escaped with that run.
fn pre_kses_less_than(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.iter().position(|&b| b == b'<') {
        let (before, run) = rest.split_at(at);
        out.extend_from_slice(before);
        let end = run
            .iter()
            .skip(1)
            .position(|&b| b == b'<' || b == b'>')
            .map_or(run.len(), |i| i + 1);
        let (tag, after) = run.split_at(end);
        match after.split_first() {
            Some((b'>', after)) => {
                out.extend_from_slice(tag);
                out.push(b'>');
                rest = after;
            }
            _ => {
                out.extend_from_slice(hyscribe_kses::esc_html(tag).as_bytes());
                rest = after;
            }
        }
    }
    out.extend_from_slice(rest);
    out
}

/// `sanitize_title_with_dashes( $title, '', $context )`, in the save
/// context when `save` is set, else in the query context.
fn sanitize_title_with_dashes(title: &[u8], save: bool) -> Vec<u8> {
    let stripped = hyscribe_kses::strip_tags(title);
    // A `%` that does not start an escaped octet goes.
    let mut text = Vec::with_capacity(stripped.len());
    for (i, &b) in stripped.iter().enumerate() {
        let octet = stripped
            .get(i + 1..i + 3)
            .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit));
        if b != b'%' || octet {
            text.push(b);
        }
    }
    // The octets come back from `---XX---`, those the title had too.
    let mut text = restore_octets(&text);
    if let Ok(utf8) = std::str::from_utf8(&text) {
        text = utf8_uri_encode(&utf8.to_lowercase(), 200);
    }
    text.make_ascii_lowercase();
    if save {
        for (searches, with) in TITLE_REPLACEMENTS {
            for search in searches {
                text = replace_all(&text, search, with);
            }
        }
    }
    let mut kept = Vec::with_capacity(text.len());
    let mut rest = &text[..];
    for _ in 0..=text.len() {
        let Some((&first, after)) = rest.split_first() else {
            break;
        };
        // `/&.+?;/`: an entity goes, from its `&` to the next `;` on the line.
        let entity = match after.split_first() {
            Some((&next, tail)) if first == b'&' && next != b'\n' => tail
                .iter()
                .take_while(|&&b| b != b'\n')
                .position(|&b| b == b';')
                .map(|at| at + 3),
            _ => None,
        };
        match entity {
            Some(length) => rest = rest.get(length..).unwrap_or_default(),
            None => {
                match first {
                    b'.' | b' ' => kept.push(b'-'),
                    b'%' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' => kept.push(first),
                    _ => {}
                }
                rest = after;
            }
        }
    }
    let mut collapsed: Vec<u8> = Vec::with_capacity(kept.len());
    for b in kept {
        if !(b == b'-' && collapsed.last() == Some(&b'-')) {
            collapsed.push(b);
        }
    }
    let start = collapsed
        .iter()
        .position(|&b| b != b'-')
        .unwrap_or(collapsed.len());
    let end = collapsed
        .iter()
        .rposition(|&b| b != b'-')
        .map_or(start, |at| at + 1);
    collapsed.get(start..end).unwrap_or_default().to_vec()
}

/// `preg_replace( '|---([a-fA-F0-9][a-fA-F0-9])---|', '%$1', $text )`.
fn restore_octets(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some((&first, after)) = rest.split_first() {
        match rest.get(..8) {
            Some([b'-', b'-', b'-', a, b, b'-', b'-', b'-'])
                if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() =>
            {
                out.extend([b'%', *a, *b]);
                rest = rest.get(8..).unwrap_or_default();
            }
            _ => {
                out.push(first);
                rest = after;
            }
        }
    }
    out
}

/// `utf8_uri_encode( $text, $length )`: each byte of a multibyte
/// character as `%xx`, stopping before the character that would take the
/// result past `length` bytes.
fn utf8_uri_encode(text: &str, length: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let mut bytes = [0; 4];
        let bytes = c.encode_utf8(&mut bytes).as_bytes();
        let encoded = if c.is_ascii() {
            bytes.len()
        } else {
            bytes.len() * 3
        };
        if out.len() + encoded > length {
            break;
        }
        if c.is_ascii() {
            out.extend_from_slice(bytes);
        } else {
            for b in bytes {
                out.extend(format!("%{b:x}").into_bytes());
            }
        }
    }
    out
}

/// `str_replace( $search, $with, $subject )` for one search string.
fn replace_all(subject: &[u8], search: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(subject.len());
    let mut rest = subject;
    for _ in 0..=subject.len() {
        match rest.windows(search.len()).position(|w| w == search) {
            Some(at) => {
                out.extend_from_slice(rest.get(..at).unwrap_or_default());
                out.extend_from_slice(with);
                rest = rest.get(at + search.len()..).unwrap_or_default();
            }
            None => break,
        }
    }
    out.extend_from_slice(rest);
    out
}

/// `capital_P_dangit()` as the `the_content` filter runs it: only after a
/// space, an opening quote, `>` or `(`.
pub fn capital_p_dangit(text: &[u8]) -> Vec<u8> {
    const AFTER: [&[u8]; 5] = [b" ", b"&#8216;", b"&#8220;", b">", b"("];
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    for _ in 0..=text.len() {
        let Some((first, after)) = rest.split_first() else {
            break;
        };
        let lead = AFTER.iter().find(|lead| {
            rest.starts_with(lead)
                && rest
                    .get(lead.len()..)
                    .is_some_and(|r| r.starts_with(b"Wordpress"))
        });
        match lead {
            Some(lead) => {
                out.extend_from_slice(lead);
                out.extend_from_slice(b"WordPress");
                rest = rest.get(lead.len() + 9..).unwrap_or_default();
            }
            None => {
                out.push(*first);
                rest = after;
            }
        }
    }
    out
}

/// `capital_P_dangit()` as the `the_title` and `wp_title` filters run it.
pub fn capital_p_dangit_title(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    for _ in 0..=text.len() {
        match rest.strip_prefix(b"Wordpress") {
            Some(after) => {
                out.extend_from_slice(b"WordPress");
                rest = after;
            }
            None => match rest.split_first() {
                Some((first, after)) => {
                    out.push(*first);
                    rest = after;
                }
                None => break,
            },
        }
    }
    out
}

mod accents;
mod entities;

pub use accents::remove_accents;

/// `is_email()`: whether `email` has the shape WordPress accepts, a local
/// part of the allowed characters and a domain of two or more labels.
pub(crate) fn is_email(email: &[u8]) -> bool {
    if email.len() < 6 {
        return false;
    }
    let Some(at) = email.iter().skip(1).position(|&b| b == b'@') else {
        return false;
    };
    let (local, domain) = (
        email.get(..=at).unwrap_or_default(),
        email.get(at + 2..).unwrap_or_default(),
    );
    // PCRE's `$` also matches before a final new line.
    let local = local.strip_suffix(b"\n").unwrap_or(local);
    let local_ok = !local.is_empty()
        && local
            .iter()
            .all(|&b| b.is_ascii_alphanumeric() || b"!#$%&'*+/=?^_`{|}~.-".contains(&b));
    if !local_ok || contains(domain, b"..") || !untrimmed(domain, b" \t\n\r\0\x0B.") {
        return false;
    }
    let subs: Vec<&[u8]> = domain.split(|&b| b == b'.').collect();
    subs.len() >= 2
        && subs.iter().all(|sub| {
            untrimmed(sub, b" \t\n\r\0\x0B-")
                && sub.iter().all(|&b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

/// Whether PHP's `trim( $text, $list )` would leave `text` as it is.
fn untrimmed(text: &[u8], list: &[u8]) -> bool {
    text.first().is_none_or(|b| !list.contains(b)) && text.last().is_none_or(|b| !list.contains(b))
}

/// `antispambot()` on an address `is_email()` accepts: each character as
/// itself or a decimal reference, drawn at random, and `@` always as
/// `&#64;`.
pub(crate) fn antispambot(email: &[u8]) -> Vec<u8> {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    let mut out = Vec::new();
    for (i, &b) in email.iter().enumerate() {
        match state.hash_one(i) % 2 {
            _ if b == b'@' => out.extend_from_slice(b"&#64;"),
            0 => out.extend_from_slice(format!("&#{b};").as_bytes()),
            _ => out.push(b),
        }
    }
    out
}

#[cfg(test)]
mod tests;
