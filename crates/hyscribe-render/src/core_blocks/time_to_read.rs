// SPDX-License-Identifier: AGPL-3.0-or-later

//! The time to read block, `render_block_core_post_time_to_read()`, which
//! counts the words of the post's raw content.

use hyscribe_phpser::{Value, php_trim};

use super::{Block, Context, number_format, render};
use crate::template::Renderer;

impl Renderer<'_> {
    /// The block over `get_the_content()` of the `postId` context's post.
    pub(crate) fn post_time_to_read(&self, block: &crate::blocks::Block) -> Vec<u8> {
        let name = "core/post-time-to-read";
        let Some(id) = self.context.post_id else {
            return Vec::new();
        };
        let content = self
            .context
            .site
            .post(id)
            .map(|post| crate::content::get_the_content(self, &post, true))
            .unwrap_or_default();
        let parsed = block.attrs.as_deref().unwrap_or_default();
        render(&self.context, name, parsed, &content).unwrap_or_default()
    }
}

/// `render_block_core_post_time_to_read()` over `block.content`, the raw
/// content; English counts words.
pub(super) fn post_time_to_read(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let words = word_count(block.content);
    let rate = block
        .attribute("averageReadingSpeed")
        .map_or(189.0, Value::to_float);
    // (int) round(), never under a minute.
    let minutes = |factor: f64| ((words as f64 / rate * factor).round() as i64).max(1);
    let text = match block
        .attribute("displayMode")
        .map(Value::to_php_string)
        .as_deref()
    {
        Some(b"time")
            if block
                .attribute("displayAsRange")
                .is_some_and(|v| !v.is_empty()) =>
        {
            let (low, high) = (minutes(0.8), minutes(1.2));
            let high = if low == high { low + 1 } else { high };
            format!("{low}–{high} minutes")
        }
        Some(b"time") => match minutes(1.0) {
            1 => "1 minute".to_owned(),
            n => format!("{n} minutes"),
        },
        Some(b"words") => match words {
            1 => "1 word".to_owned(),
            n => format!("{} words", number_format(n)),
        },
        _ => String::new(),
    };
    let align = block
        .attribute("textAlign")
        .filter(|v| !v.is_empty())
        .map(|v| [b"has-text-align-".as_slice(), &v.to_php_string()].concat())
        .unwrap_or_default();
    format!("<div {}>{text}</div>", block.wrapper(context, &align)).into_bytes()
}

/// `block_core_post_time_to_read_word_count()` for `words`: the words left
/// once tags, comments, entities and punctuation are gone, joined words
/// split; content that is not UTF-8 counts none, as PHP's `/u` patterns
/// then fail.
fn word_count(text: &[u8]) -> u64 {
    if php_trim(text).is_empty() {
        return 0;
    }
    let text = [text, b"\n"].concat();
    let text = strip_tags(&text);
    let text = strip_comments(&text);
    let text = non_breaking_spaces(&text);
    let text = strip_entities(&text);
    let Ok(text) = String::from_utf8(text) else {
        return 0;
    };
    let text: String = text
        .replace("--", " ")
        .replace('\u{2014}', " ")
        .chars()
        .filter(|&c| !removed(c))
        .collect();
    // `\S\s+`: each word followed by white space.
    let mut count = 0;
    let mut previous = None;
    for c in text.chars() {
        if c.is_whitespace() && previous.is_some_and(|p: char| !p.is_whitespace()) {
            count += 1;
        }
        previous = Some(c);
    }
    count
}

/// `/<\/?[a-z][^>]*?>/i` replaced by a new line.
fn strip_tags(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while let Some(&b) = text.get(i) {
        let name = i + 1 + usize::from(text.get(i + 1) == Some(&b'/'));
        let end = (b == b'<' && text.get(name).is_some_and(u8::is_ascii_alphabetic))
            .then(|| text.iter().skip(name).position(|&c| c == b'>'))
            .flatten();
        match end {
            Some(end) => {
                out.push(b'\n');
                i = name + end + 1;
            }
            None => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// `/<!--[\s\S]*?-->/` removed.
fn strip_comments(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = find(rest, b"<!--") {
        let after = rest.get(start + 4..).unwrap_or_default();
        let Some(end) = find(after, b"-->") else {
            break;
        };
        out.extend_from_slice(rest.get(..start).unwrap_or_default());
        rest = after.get(end + 3..).unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}

/// `/&nbsp;|&#160;/i` replaced by a space.
fn non_breaking_spaces(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while let Some(&b) = text.get(i) {
        let entity = [&b"&nbsp;"[..], b"&#160;"].into_iter().find(|e| {
            text.get(i..i + e.len())
                .is_some_and(|t| t.eq_ignore_ascii_case(e))
        });
        match entity {
            Some(e) => {
                out.push(b' ');
                i += e.len();
            }
            None => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// `/&\S+?;/` removed, `\S` any byte but ASCII white space.
fn strip_entities(text: &[u8]) -> Vec<u8> {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while let Some(&b) = text.get(i) {
        // At least one byte, then the first `;` before any white space.
        let end = (b == b'&')
            .then(|| text.get(i + 1..))
            .flatten()
            .filter(|body| body.first().is_some_and(|c| !space(c)))
            .and_then(|body| {
                body.iter()
                    .skip(1)
                    .take_while(|c| !space(c))
                    .position(|&c| c == b';')
            });
        match end {
            Some(end) => i += end + 3,
            None => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// A character `remove_regexp` takes out: ASCII punctuation and digits,
/// Latin-1 symbols, and the general punctuation to miscellaneous symbol
/// blocks.
fn removed(c: char) -> bool {
    matches!(c,
        '\u{21}'..='\u{40}'
        | '\u{5B}'..='\u{60}'
        | '\u{7B}'..='\u{7E}'
        | '\u{80}'..='\u{BF}'
        | '\u{D7}'
        | '\u{F7}'
        | '\u{2000}'..='\u{2BFF}'
        | '\u{2E00}'..='\u{2E7F}')
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests;
