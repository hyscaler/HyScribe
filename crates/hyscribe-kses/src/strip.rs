// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_strip_all_tags()`: `<script>` and `<style>` elements removed with
//! WordPress's regex, then PHP's `strip_tags()`.

use crate::php::{first_from, is_space, positions, rest, scan, splice, starts_with_ci, trim};

/// How far `.*?` in `@<(script|style)[^>]*?>.*?</\1>@si` may advance in
/// one attempt before PCRE's JIT reaches `pcre.backtrack_limit`
/// (1000000) and `preg_replace()` returns `null`, which WordPress turns
/// into an empty string. Measured on PHP 8.4.
const LAZY_LIMIT: usize = 999_997;

/// `preg_replace('@<(script|style)[^>]*?>.*?</\1>@si', '', $text)`, or
/// `None` where PCRE gives up. Linear: every search is a lookup in the
/// precomputed positions of `>` and of each closing tag.
fn without_scripts(text: &[u8]) -> Option<Vec<u8>> {
    const NAMES: [&[u8]; 2] = [b"script", b"style"];
    let closes = |name: &'static [u8]| {
        positions(text, move |at| {
            at.starts_with(b"</")
                && starts_with_ci(rest(at, 2), name)
                && rest(at, 2 + name.len()).first() == Some(&b'>')
        })
    };
    let ends = positions(text, |at| at.first() == Some(&b'>'));
    let closers = NAMES.map(closes);
    let mut limited = false;
    let found = scan(text.len(), |i| {
        let tag = rest(text, i).strip_prefix(b"<")?;
        let (name, closers) = NAMES
            .iter()
            .zip(&closers)
            .find(|(name, _)| starts_with_ci(tag, name))?;
        let content = first_from(&ends, i)? + 1;
        let closer = first_from(closers, content);
        limited |= closer.unwrap_or(text.len()) - content > LAZY_LIMIT;
        closer.map(|at| (at + name.len() + 3 - i, Vec::new()))
    });
    (!limited).then(|| splice(text, &found))
}

/// The states of `php_strip_tags_ex()`.
#[derive(Clone, Copy)]
enum State {
    Text,
    Tag,
    Php,
    Bang,
    Comment,
}

/// PHP's quote tracking: a quote opens, and the same quote closes.
fn toggle(in_quote: &mut u8, quote: u8) {
    if *in_quote == 0 {
        *in_quote = quote;
    } else if *in_quote == quote {
        *in_quote = 0;
    }
}

/// PHP's `strip_tags()` without allowed tags: `php_strip_tags_ex()` from
/// PHP 8.4's `ext/standard/string.c`, state for state.
pub fn strip_tags(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut state = State::Text;
    let mut depth = 0usize;
    let mut in_quote = 0u8;
    let mut last = 0u8;
    let mut brackets = 0i64;
    let mut is_xml = false;
    for (i, &c) in text.iter().enumerate() {
        let back = |n: usize| {
            i.checked_sub(n)
                .and_then(|j| text.get(j))
                .copied()
                .unwrap_or_default()
        };
        let space_next = text.get(i + 1).is_some_and(|&b| is_space(b));
        let quoted = last == b'"' || last == b'\'';
        match state {
            State::Text => match c {
                0 => {}
                b'<' if space_next => out.push(c),
                b'<' => {
                    last = b'<';
                    state = State::Tag;
                }
                b'>' if depth > 0 => depth -= 1,
                _ => out.push(c),
            },
            State::Tag => match c {
                b'<' if in_quote == 0 && !space_next => depth += 1,
                b'>' if depth > 0 => depth -= 1,
                b'>' if in_quote == 0 => {
                    last = b'>';
                    if !is_xml || back(1) != b'-' {
                        state = State::Text;
                        is_xml = false;
                    }
                }
                b'"' | b'\'' => toggle(&mut in_quote, c),
                b'!' if back(1) == b'<' => {
                    last = c;
                    state = State::Bang;
                }
                b'?' if back(1) == b'<' => {
                    brackets = 0;
                    state = State::Php;
                }
                _ => {}
            },
            State::Php => match c {
                b'(' if !quoted => {
                    last = c;
                    brackets += 1;
                }
                b')' if !quoted => {
                    last = c;
                    brackets -= 1;
                }
                b'>' if depth > 0 => depth -= 1,
                b'>' if in_quote == 0 && brackets == 0 && last != b'"' && back(1) == b'?' => {
                    state = State::Text;
                }
                b'"' | b'\'' if back(1) != b'\\' => {
                    last = if last == c { 0 } else { c };
                    toggle(&mut in_quote, c);
                }
                // `<?xml` is markup, not PHP; not at the very start, as
                // PHP checks `p > buf + 4`.
                b'l' | b'L'
                    if i > 4
                        && back(1).eq_ignore_ascii_case(&b'm')
                        && back(2).eq_ignore_ascii_case(&b'x')
                        && back(3) == b'?'
                        && back(4) == b'<' =>
                {
                    state = State::Tag;
                    is_xml = true;
                }
                _ => {}
            },
            State::Bang => match c {
                b'>' if depth > 0 => depth -= 1,
                b'>' if in_quote == 0 => state = State::Text,
                b'"' | b'\'' if back(1) != b'\\' => toggle(&mut in_quote, c),
                b'-' if back(1) == b'-' && back(2) == b'!' => state = State::Comment,
                // `<!DOCTYPE` is a tag.
                b'e' | b'E'
                    if b"pytcod"
                        .iter()
                        .zip(1..)
                        .all(|(&letter, n)| back(n).eq_ignore_ascii_case(&letter)) =>
                {
                    state = State::Tag;
                }
                _ => {}
            },
            State::Comment => {
                if c == b'>' && in_quote == 0 && back(1) == b'-' && back(2) == b'-' {
                    state = State::Text;
                }
            }
        }
    }
    out
}

/// `preg_replace('/[\r\n\t ]+/', ' ', $text)`.
pub fn collapse_breaks(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &byte in text {
        if !matches!(byte, b'\r' | b'\n' | b'\t' | b' ') {
            out.push(byte);
        } else if out.last() != Some(&b' ') {
            out.push(b' ');
        }
    }
    out
}

/// `wp_strip_all_tags()`: the text without `<script>` and `<style>`
/// elements or any tags, trimmed; with `remove_breaks`, every run of line
/// breaks, tabs and spaces becomes one space.
///
/// Where PCRE gives up (an element whose content runs on for about a
/// megabyte) the result is empty, as in PHP.
pub fn wp_strip_all_tags(text: &[u8], remove_breaks: bool) -> Vec<u8> {
    let Some(text) = without_scripts(text) else {
        return Vec::new();
    };
    let mut text = strip_tags(&text);
    if remove_breaks {
        text = collapse_breaks(&text);
    }
    trim(&text).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(text: &[u8]) -> String {
        String::from_utf8(wp_strip_all_tags(text, false)).unwrap()
    }

    #[test]
    fn removes_scripts_and_styles_with_content() {
        assert_eq!(strip(b"a<script>x</script>b<STYLE x>y</style>c"), "abc");
        assert_eq!(strip(b"<script></script><scripts>x</SCRIPT>d"), "d");
        assert_eq!(strip(b"<style a=\"</style>\">b"), "b");
        assert_eq!(strip(b"<script>a</style>b"), "ab");
        assert_eq!(strip(b"<script a"), "");
        assert_eq!(strip(b"<scrip>a</scrip>"), "a");
    }

    #[test]
    fn gives_up_on_long_script_content() {
        let content = |n: usize| [b"<script>".as_slice(), &b"a".repeat(n), b"</script>x"].concat();
        assert_eq!(strip(&content(LAZY_LIMIT)), "x");
        assert_eq!(strip(&content(LAZY_LIMIT + 1)), "");
        let open = |n: usize| [b"<style>".as_slice(), &b"a".repeat(n)].concat();
        assert_eq!(strip(&open(LAZY_LIMIT)).len(), LAZY_LIMIT);
        assert_eq!(strip(&open(LAZY_LIMIT + 1)), "");
    }

    #[test]
    fn strips_tags_but_not_lone_brackets() {
        assert_eq!(strip(b"<p>Hello <b>World</b></p>"), "Hello World");
        assert_eq!(strip(b"a < b > c"), "a < b > c");
        assert_eq!(strip(b"a <\tb>"), "a <\tb>");
        assert_eq!(strip(b"a<"), "a");
        assert_eq!(strip(b"a>b"), "a>b");
        assert_eq!(strip(b"\0a\0<b\0>c"), "ac");
    }

    #[test]
    fn quotes_hide_brackets_in_tags() {
        assert_eq!(strip(b"<a href=\"x>y\">z</a>"), "z");
        assert_eq!(strip(b"<a b=\"c'd\">e"), "e");
        assert_eq!(strip(b"<a b='c\"d'>e"), "e");
        assert_eq!(strip(b"<a\"<b>c"), "");
        assert_eq!(strip(b"<a\"<\">b"), "b");
    }

    #[test]
    fn nested_brackets_count_depth() {
        assert_eq!(strip(b"<a<b>>c"), "c");
        assert_eq!(strip(b"<a <b>c"), "");
        assert_eq!(strip(b"<a < b>c>d"), "c>d");
        assert_eq!(strip(b"<a<!-- -->>x"), "x");
        assert_eq!(strip(b"<a<!-- -->>x>y"), "x>y");
        assert_eq!(strip(b"<a<b<!-- -->>>x>y"), "x>y");
        assert_eq!(strip(b"<a<!x>>y"), "y");
        assert_eq!(strip(b"<a<!x>q>y"), "y");
        assert_eq!(strip(b"<a<?x>?>>z"), ">z");
    }

    #[test]
    fn comments_and_bangs() {
        assert_eq!(strip(b"<!-- a > b -->c"), "c");
        assert_eq!(strip(b"<!-->x"), "x");
        assert_eq!(strip(b"<!- ->x"), "x");
        assert_eq!(strip(b"<!x->z"), "z");
        assert_eq!(strip(b"<!x--a>b"), "b");
        assert_eq!(strip(b"<!-- x"), "");
        assert_eq!(strip(b"<a\"<!-- --> \">x"), "");
        assert_eq!(strip(b"<!x\">y\">z"), "z");
        assert_eq!(strip(b"<!x\\\">y"), "y");
        assert_eq!(strip(b"<!DOCTYPE html>x"), "x");
        assert_eq!(strip(b"<!doctype \"a>b\">c"), "c");
        assert_eq!(strip(b"<!x doctype 'a>b'>c"), "c");
        assert_eq!(strip(b"<!DOCTYPE<a>>b"), "b");
        assert_eq!(strip(b"<!OCTYPE<a>>b"), ">b");
    }

    #[test]
    fn php_blocks() {
        assert_eq!(strip(b"<?php echo 1; ?>x"), "x");
        assert_eq!(strip(b"<?php echo \"?>\"; ?>y"), "y");
        assert_eq!(strip(b"<?php echo '?>'; ?>y"), "y");
        assert_eq!(strip(b"<? (a ?> b) ?>c"), "c");
        assert_eq!(strip(b"<?)?>a"), "");
        assert_eq!(strip(b"<?\")\"?>a"), "a");
        assert_eq!(strip(b"<? \\\" ?>a"), "a");
        assert_eq!(strip(b"<?'\"'?>a<?(?>b"), "a");
        assert_eq!(strip(b"<?\"(\"?>a"), "a");
        assert_eq!(strip(b"<?'a'?>b>c"), "b>c");
        assert_eq!(strip(b"<?a>?>d"), "d");
        assert_eq!(strip(b"<a<?b?>>c"), "");
    }

    #[test]
    fn xml_declarations_are_tags() {
        assert_eq!(strip(b"a<?xml a>b"), "ab");
        assert_eq!(strip(b"a<?XmL a>b"), "ab");
        assert_eq!(strip(b"<?xml a>b"), "");
        assert_eq!(strip(b"a<?xml a->b>c"), "ac");
        assert_eq!(strip(b"a<?yml a>b"), "a");
        assert_eq!(strip(b"a<?xnl a>b"), "a");
        assert_eq!(strip(b"a<!xml a>b"), "ab");
        assert_eq!(strip(b"a<?Xxml a>b"), "a");
    }

    #[test]
    fn trims_and_collapses_breaks() {
        assert_eq!(strip(b" \n<b>a</b> \t"), "a");
        assert_eq!(wp_strip_all_tags(b"a\r\n \tb  c\nd", true), b"a b c d");
        assert_eq!(wp_strip_all_tags(b" a ", true), b"a");
        assert_eq!(wp_strip_all_tags(b"a\r\nb", false), b"a\r\nb");
    }
}
