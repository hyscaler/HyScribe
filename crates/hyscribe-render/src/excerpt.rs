// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_the_excerpt()`: a post's hand-written excerpt, or the one
//! `wp_trim_excerpt()` makes from its content.
//!
//! Not ported yet, as with the content: shortcodes (`strip_shortcodes()`,
//! `shortcode_unautop()`, `do_shortcode()`).

use hyscribe_kses::wp_strip_all_tags;
use hyscribe_model::Post;
use hyscribe_phpser::{Value, php_trim};

use crate::blocks::{Block, parse_blocks};
use crate::content::get_the_content;
use crate::formatting::contains;
use crate::template::{Renderer, replace};

/// The blocks whose markup an excerpt keeps (`null` is HTML between
/// blocks), and those it looks inside.
const INNER_BLOCKS: [Option<&str>; 12] = [
    None,
    Some("core/freeform"),
    Some("core/heading"),
    Some("core/html"),
    Some("core/list"),
    Some("core/media-text"),
    Some("core/paragraph"),
    Some("core/preformatted"),
    Some("core/pullquote"),
    Some("core/quote"),
    Some("core/table"),
    Some("core/verse"),
];
const WRAPPER_BLOCKS: [&str; 3] = ["core/columns", "core/column", "core/group"];

/// `excerpt_length` and `excerpt_more`.
const EXCERPT_LENGTH: usize = 55;
const EXCERPT_MORE: &[u8] = b" [&hellip;]";

impl Renderer<'_> {
    /// `get_the_excerpt()` for `post`, the loop's post.
    pub(crate) fn get_the_excerpt(&mut self, post: &Post) -> Vec<u8> {
        if !post.password.is_empty() {
            return b"There is no excerpt because this is a protected post.".to_vec();
        }
        // wp_trim_excerpt()
        if !php_trim(&post.excerpt).is_empty() {
            return post.excerpt.clone();
        }
        let content = get_the_content(self, post);
        let content = self.excerpt_remove_blocks(&content);
        let content = excerpt_remove_footnotes(&content);
        let content = replace(&self.content_filters(&content, true), b"]]>", b"]]&gt;");
        wp_trim_words(&content, EXCERPT_LENGTH, EXCERPT_MORE)
    }

    /// `excerpt_remove_blocks()`: the rendered blocks an excerpt can show.
    fn excerpt_remove_blocks(&mut self, content: &[u8]) -> Vec<u8> {
        if !contains(content, b"<!-- wp:") {
            return content.to_vec();
        }
        let mut out = Vec::new();
        for block in parse_blocks(content).iter().filter(|b| visible(b)) {
            let name = block.name.as_deref();
            if name.is_some_and(|n| WRAPPER_BLOCKS.contains(&n)) && !block.inner_blocks.is_empty() {
                out.extend(self.excerpt_inner_blocks(block));
                continue;
            }
            let plain = block.inner_blocks.iter().all(|inner| {
                INNER_BLOCKS.contains(&inner.name.as_deref()) && inner.inner_blocks.is_empty()
            });
            if allowed(name) && plain {
                out.extend(self.block(block));
            }
        }
        out
    }

    /// `_excerpt_render_inner_blocks()`.
    fn excerpt_inner_blocks(&mut self, parent: &Block) -> Vec<u8> {
        let mut out = Vec::new();
        for block in parent.inner_blocks.iter().filter(|b| visible(b)) {
            let name = block.name.as_deref();
            if !allowed(name) {
                continue;
            }
            if block.inner_blocks.is_empty() {
                out.extend(self.block(block));
            } else {
                out.extend(self.excerpt_inner_blocks(block));
            }
        }
        out
    }
}

/// `excerpt_allowed_blocks`: inner and wrapper blocks.
fn allowed(name: Option<&str>) -> bool {
    INNER_BLOCKS.contains(&name) || name.is_some_and(|n| WRAPPER_BLOCKS.contains(&n))
}

/// Not hidden with `metadata.blockVisibility` set to `false`.
pub(crate) fn visible(block: &Block) -> bool {
    let visibility = block
        .attrs
        .as_deref()
        .and_then(|attrs| hyscribe_phpser::lookup(attrs, "metadata"))
        .and_then(|metadata| metadata.get_str("blockVisibility"));
    visibility != Some(&Value::Bool(false))
}

/// `excerpt_remove_footnotes()`: footnote markers go, as
/// `_<sup data-fn="[^"]+" class="[^"]+">\s*<a href="[^"]+" id="[^"]+">\d+</a>\s*</sup>_`
/// matches them.
fn excerpt_remove_footnotes(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut at = 0;
    while let Some(rest) = content.get(at..).filter(|r| !r.is_empty()) {
        match footnote(rest) {
            Some(len) => at += len,
            None => {
                out.extend(rest.get(..1).unwrap_or_default());
                at += 1;
            }
        }
    }
    out
}

/// One part of the footnote pattern.
enum Part {
    Text(&'static [u8]),
    /// `[^"]+`
    Value,
    /// `\s*`
    Space,
    /// `\d*`
    Digits,
}

const FOOTNOTE: [Part; 15] = [
    Part::Text(b"<sup data-fn=\""),
    Part::Value,
    Part::Text(b"\" class=\""),
    Part::Value,
    Part::Text(b"\">"),
    Part::Space,
    Part::Text(b"<a href=\""),
    Part::Value,
    Part::Text(b"\" id=\""),
    Part::Value,
    Part::Text(b"\">"),
    Part::Digits,
    Part::Text(b"</a>"),
    Part::Space,
    Part::Text(b"</sup>"),
];

/// The length of a footnote marker at the start of `text`.
fn footnote(text: &[u8]) -> Option<usize> {
    FOOTNOTE.iter().try_fold(0, |at, part| {
        let rest = text.get(at..).unwrap_or_default();
        let run = |matches: fn(&u8) -> bool| rest.iter().take_while(|b| matches(b)).count();
        let len = match part {
            Part::Text(literal) => rest.starts_with(literal).then_some(literal.len()),
            Part::Value => Some(run(|b| *b != b'"')).filter(|&n| n > 0),
            Part::Space => Some(run(|b| b.is_ascii_whitespace() || *b == b'\x0b')),
            // `\d+`, though an empty marker strips to nothing all the same.
            Part::Digits => Some(run(u8::is_ascii_digit)),
        }?;
        Some(at + len)
    })
}

/// `wp_trim_words( $text, $num_words, $more )` for a language that counts
/// words.
pub(crate) fn wp_trim_words(text: &[u8], num_words: usize, more: &[u8]) -> Vec<u8> {
    let text = wp_strip_all_tags(text, false);
    let words: Vec<&[u8]> = text
        .split(|b| matches!(b, b'\n' | b'\r' | b'\t' | b' '))
        .filter(|word| !word.is_empty())
        .collect();
    match words.get(..num_words).filter(|_| words.len() > num_words) {
        Some(kept) => [&kept.join(&b' ')[..], more].concat(),
        None => words.join(&b' '),
    }
}

#[cfg(test)]
mod tests;
