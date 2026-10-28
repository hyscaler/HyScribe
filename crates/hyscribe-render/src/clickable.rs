// SPDX-License-Identifier: AGPL-3.0-or-later

//! `make_clickable()`: URLs, `www.`/`ftp.` addresses and email addresses in
//! text turned into links, with its three patterns and the clean-up of
//! links inside links matched by hand. Each piece of text is at most a
//! little over 10,000 bytes, and every lookahead is a table lookup or
//! resumes where the last ended, so the work stays linear.

use hyscribe_kses::{esc_attr, esc_url, url_host, url_scheme};
use hyscribe_phpser::is_empty_string;

use crate::blocks::{is_pcre_space, run};
use crate::formatting::is_word;

/// `[\w\x80-\xff#%\~/@\[\]*(+=&$-]`: a URL character that is not
/// punctuation.
fn is_url_char(b: u8) -> bool {
    b.is_ascii_alphanumeric()
        || b >= 0x80
        || matches!(
            b,
            b'_' | b'#'
                | b'%'
                | b'~'
                | b'/'
                | b'@'
                | b'['
                | b']'
                | b'*'
                | b'('
                | b'+'
                | b'='
                | b'&'
                | b'$'
                | b'-'
        )
}

/// `['.,;:!?)]`: punctuation allowed inside a URL before more of it.
fn is_url_punctuation(b: u8) -> bool {
    matches!(b, b'\'' | b'.' | b',' | b';' | b':' | b'!' | b'?' | b')')
}

/// `make_clickable( $text )` on a site whose `home_url()` is `home`, with
/// `ugc` set as in the `comment_text` filter.
pub fn make_clickable(text: &[u8], home: &[u8], ugc: bool) -> Vec<u8> {
    let links = Links {
        internal_host: url_host(home).map(|host| host.to_ascii_lowercase()),
        ugc,
    };
    links.make_clickable(text)
}

struct Links {
    internal_host: Option<Vec<u8>>,
    ugc: bool,
}

impl Links {
    fn make_clickable(&self, text: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len());
        let mut nested: usize = 0;
        for piece in split_tags(text) {
            let opens = [&b"<code"[..], b"<pre", b"<script", b"<style"]
                .iter()
                .any(|name| {
                    piece
                        .get(..name.len())
                        .is_some_and(|start| start.eq_ignore_ascii_case(name))
                        && piece
                            .get(name.len())
                            .is_some_and(|&b| is_pcre_space(b) || b == b'>')
                });
            if opens {
                nested += 1;
            } else if nested > 0
                && [&b"</code>"[..], b"</pre>", b"</script>", b"</style>"]
                    .iter()
                    .any(|closer| piece.eq_ignore_ascii_case(closer))
            {
                nested -= 1;
            }
            if nested > 0
                || is_empty_string(piece)
                || (piece.first() == Some(&b'<') && !scheme_tag(piece))
            {
                out.extend_from_slice(piece);
                continue;
            }
            if piece.len() > 10_000 {
                for chunk in split_by_whitespace(piece, 2100) {
                    if chunk.len() > 2101 {
                        out.extend_from_slice(chunk);
                    } else {
                        out.extend(self.make_clickable(chunk));
                    }
                }
                continue;
            }
            let padded = [b" ", piece, b" "].concat();
            let linked = emails(&self.web_ftp(&self.urls(&padded)));
            out.extend_from_slice(linked.get(1..linked.len() - 1).unwrap_or_default());
        }
        links_in_links(&out)
    }

    /// The `$url_clickable` pattern with `_make_url_clickable_cb()`.
    fn urls(&self, text: &[u8]) -> Vec<u8> {
        let space_at = first_from(text, is_pcre_space);
        let run = |from: usize, class: fn(u8) -> bool| from + run(text, from, class);
        let mut out = Vec::with_capacity(text.len());
        let mut at = 0;
        while let Some(&lead) = text.get(at) {
            let found = (|| {
                if !(is_pcre_space(lead)
                    || matches!(lead, b'(' | b'<' | b'.' | b',' | b';' | b':' | b'!' | b'?'))
                {
                    return None;
                }
                // `[\w]{1,20}+://`: possessive, so a longer word never matches.
                let scheme_end = run(at + 1, is_word);
                if !(at + 2..=at + 21).contains(&scheme_end)
                    || text.get(scheme_end..scheme_end + 3) != Some(b"://")
                {
                    return None;
                }
                let body = scheme_end + 3;
                // `(?=\S{1,2000}\s)`; the padding puts a space after every run.
                let end = space_at.get(body).copied().unwrap_or(text.len());
                if end == body || end - body > 2000 {
                    return None;
                }
                let mut url_end = run(body, is_url_char);
                while text.get(url_end).is_some_and(|&b| is_url_punctuation(b))
                    && text.get(url_end + 1).is_some_and(|&b| is_url_char(b))
                {
                    url_end = run(url_end + 1, is_url_char);
                }
                let paren = text.get(url_end) == Some(&b')');
                let mut end = url_end + usize::from(paren);
                let extension = (text.get(end) == Some(&b'.'))
                    .then(|| run(end + 1, is_word) - (end + 1))
                    .filter(|&n| n >= 2);
                let extension =
                    extension.map(|n| text.get(end..end + 1 + n.min(6)).unwrap_or_default());
                end += extension.map_or(0, <[u8]>::len);
                let url = text.get(at + 1..url_end).unwrap_or_default();
                Some((
                    end,
                    self.url_link(
                        lead,
                        url,
                        paren,
                        extension,
                        text.get(at..end).unwrap_or_default(),
                    ),
                ))
            })();
            match found {
                Some((end, with)) => {
                    out.extend(with);
                    at = end;
                }
                None => {
                    out.push(lead);
                    at += 1;
                }
            }
        }
        out
    }

    /// `_make_url_clickable_cb()`.
    fn url_link(
        &self,
        lead: u8,
        url: &[u8],
        paren: bool,
        extension: Option<&[u8]>,
        whole: &[u8],
    ) -> Vec<u8> {
        let mut url = url.to_vec();
        let mut suffix = Vec::new();
        if paren && url.contains(&b'(') {
            url.push(b')');
        } else if paren {
            suffix.push(b')');
        }
        if let Some(extension) = extension {
            url.extend_from_slice(extension);
        }
        // Parentheses stay in the URL only when paired.
        let count = |part: &[u8], b: u8| part.iter().filter(|&&c| c == b).count();
        let (mut opens, mut closes) = (count(&url, b'('), count(&url, b')'));
        while opens < closes {
            let at = url.iter().rposition(|&b| b == b')').unwrap_or_default();
            let moved = url.split_off(at);
            opens -= count(&moved, b'(');
            closes -= count(&moved, b')');
            suffix = [moved, suffix].concat();
        }
        let url = esc_url(&url);
        if url.is_empty() {
            return whole.to_vec();
        }
        [
            &[lead][..],
            b"<a href=\"",
            &url,
            b"\"",
            &self.rel_attr(&url),
            b">",
            &url,
            b"</a>",
            &suffix,
        ]
        .concat()
    }

    /// `#([\s>])((www|ftp)\.[\w\x80-\xff\#$%&~/.\-;:=,?@\[\]+]+)#is` with
    /// `_make_web_ftp_clickable_cb()`.
    fn web_ftp(&self, text: &[u8]) -> Vec<u8> {
        let is_char = |b: u8| {
            is_word(b)
                || b >= 0x80
                || matches!(
                    b,
                    b'#' | b'$'
                        | b'%'
                        | b'&'
                        | b'~'
                        | b'/'
                        | b'.'
                        | b'-'
                        | b';'
                        | b':'
                        | b'='
                        | b','
                        | b'?'
                        | b'@'
                        | b'['
                        | b']'
                        | b'+'
                )
        };
        let mut out = Vec::with_capacity(text.len());
        let mut at = 0;
        while let Some(&lead) = text.get(at) {
            let prefix = text.get(at + 1..at + 5).unwrap_or_default();
            let starts = (is_pcre_space(lead) || lead == b'>')
                && (prefix.eq_ignore_ascii_case(b"www.") || prefix.eq_ignore_ascii_case(b"ftp."));
            let end = at + 5 + run(text, at + 5, is_char);
            if !starts || end == at + 5 {
                out.push(lead);
                at += 1;
                continue;
            }
            let mut dest = [&b"http://"[..], text.get(at + 1..end).unwrap_or_default()].concat();
            let trailing = dest.pop_if(|b| matches!(b, b'.' | b',' | b';' | b':'));
            // esc_url() keeps the `http://`, so it is never empty here.
            let dest = esc_url(&dest);
            out.push(lead);
            out.extend(
                [
                    &b"<a href=\""[..],
                    &dest,
                    b"\"",
                    &self.rel_attr(&dest),
                    b">",
                    &dest,
                    b"</a>",
                ]
                .concat(),
            );
            out.extend(trailing);
            at = end;
        }
        out
    }

    /// `_make_clickable_rel_attr()`: `nofollow` for external http and https
    /// links, `ugc` in comments.
    fn rel_attr(&self, url: &[u8]) -> Vec<u8> {
        let mut parts: Vec<&str> = Vec::new();
        let scheme = url_scheme(url)
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        if !self.is_internal(url) && (scheme == b"http" || scheme == b"https") {
            parts.push("nofollow");
        }
        if self.ugc {
            parts.push("ugc");
        }
        if parts.is_empty() {
            return Vec::new();
        }
        format!(" rel=\"{}\"", esc_attr(parts.join(" ").as_bytes())).into_bytes()
    }

    /// `wp_is_internal_link()`, without its protocol check: that always
    /// passes for the http and https links it is asked about.
    fn is_internal(&self, url: &[u8]) -> bool {
        url_host(&url.to_ascii_lowercase()) == self.internal_host
    }
}

/// `#([\s>])([.0-9a-z_+-]+)@(([0-9a-z-]+\.)+[0-9a-z]{2,})#i` with
/// `_make_email_clickable_cb()`. The domain gives back labels until the
/// rest makes a top level domain of two or more characters.
fn emails(text: &[u8]) -> Vec<u8> {
    let local = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-');
    let label = |b: u8| b.is_ascii_alphanumeric() || b == b'-';
    let run = |from: usize, class: &dyn Fn(u8) -> bool| from + run(text, from, class);
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&lead) = text.get(at) {
        let found = (|| {
            if !(is_pcre_space(lead) || lead == b'>') {
                return None;
            }
            let at_sign = run(at + 1, &local);
            if at_sign == at + 1 || text.get(at_sign) != Some(&b'@') {
                return None;
            }
            let mut labels = Vec::new();
            let mut position = at_sign + 1;
            loop {
                let end = run(position, &label);
                if end == position || text.get(end) != Some(&b'.') {
                    break;
                }
                position = end + 1;
                labels.push(position);
            }
            labels.iter().rev().find_map(|&tld| {
                let end = run(tld, &|b: u8| b.is_ascii_alphanumeric());
                (end - tld >= 2).then_some(end)
            })
        })();
        match found {
            Some(end) => {
                let email = text.get(at + 1..end).unwrap_or_default();
                out.extend(
                    [
                        &[lead][..],
                        b"<a href=\"mailto:",
                        email,
                        b"\">",
                        email,
                        b"</a>",
                    ]
                    .concat(),
                );
                at = end;
            }
            None => {
                out.push(lead);
                at += 1;
            }
        }
    }
    out
}

/// `|^<\s*[\w]{1,20}+://|`: a tag that is really a URL.
fn scheme_tag(piece: &[u8]) -> bool {
    let start = 1 + run(piece, 1, is_pcre_space);
    let word = run(piece, start, is_word);
    (1..=20).contains(&word) && piece.get(start + word..start + word + 3) == Some(b"://")
}

/// `preg_split( '/(<[^<>]+>)/', $text, -1, PREG_SPLIT_DELIM_CAPTURE )`.
fn split_tags(text: &[u8]) -> Vec<&[u8]> {
    let mut pieces = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while let Some(start) = text
        .get(at..)
        .unwrap_or_default()
        .iter()
        .position(|&b| b == b'<')
        .map(|p| at + p)
    {
        let inner = run(text, start + 1, |b| b != b'<' && b != b'>');
        let close = start + 1 + inner;
        if inner > 0 && text.get(close) == Some(&b'>') {
            pieces.push(text.get(from..start).unwrap_or_default());
            pieces.push(text.get(start..=close).unwrap_or_default());
            from = close + 1;
            at = from;
        } else {
            at = start + 1;
        }
    }
    pieces.push(text.get(from..).unwrap_or_default());
    pieces
}

/// `_split_str_by_whitespace( $text, $goal )`, which drops a last chunk
/// that is `0`.
fn split_by_whitespace(text: &[u8], goal: usize) -> Vec<&[u8]> {
    let space = |&b: &u8| is_pcre_space(b) || b == 0;
    let mut chunks = Vec::new();
    let mut rest = text;
    while let Some(window) = rest.get(..=goal) {
        let Some(at) = window.iter().rposition(space).or_else(|| {
            rest.get(goal + 1..)
                .unwrap_or_default()
                .iter()
                .position(space)
                .map(|p| goal + 1 + p)
        }) else {
            break;
        };
        chunks.push(rest.get(..=at).unwrap_or_default());
        rest = rest.get(at + 1..).unwrap_or_default();
    }
    if !is_empty_string(rest) {
        chunks.push(rest);
    }
    chunks
}

/// For each position, the first at or after it where `class` holds, or the
/// length, so lookaheads cost nothing.
fn first_from(text: &[u8], class: fn(u8) -> bool) -> Vec<usize> {
    let mut next = text.len();
    let mut first: Vec<usize> = (text.iter().enumerate().rev())
        .map(|(i, &b)| {
            if class(b) {
                next = i;
            }
            next
        })
        .collect();
    first.reverse();
    first.push(text.len());
    first
}

/// `#(<a([ \r\n\t]+[^>]+?>|>))<a [^>]+?>([^>]+?)</a></a>#i` replaced by
/// `$1$3</a>`.
fn links_in_links(text: &[u8]) -> Vec<u8> {
    let gt_at = first_from(text, |b| b == b'>');
    // Finding no `>` gives the length, where every match then fails.
    let gt = |from: usize| gt_at.get(from).copied();
    let ci = |at: usize, word: &[u8]| {
        text.get(at..at + word.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(word))
    };
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&byte) = text.get(at) {
        let found = (|| {
            if !ci(at, b"<a") {
                return None;
            }
            let open = match text.get(at + 2) {
                Some(b'>') => at + 2,
                Some(b' ' | b'\r' | b'\n' | b'\t') => gt(at + 2).filter(|&q| q >= at + 4)?,
                _ => return None,
            };
            if !ci(open + 1, b"<a ") {
                return None;
            }
            let inner = gt(open + 4).filter(|&q| q > open + 4)?;
            let close = gt(inner + 1)?;
            let ends = close >= inner + 5 && ci(close - 3, b"</a></a>");
            ends.then(|| {
                (
                    close + 5,
                    [
                        text.get(at..=open).unwrap_or_default(),
                        text.get(inner + 1..close - 3).unwrap_or_default(),
                        b"</a>",
                    ]
                    .concat(),
                )
            })
        })();
        match found {
            Some((end, with)) => {
                out.extend(with);
                at = end;
            }
            None => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
