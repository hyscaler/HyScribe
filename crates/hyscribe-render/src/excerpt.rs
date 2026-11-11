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
use crate::core_blocks::{prepared, render};
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
const EXCERPT_LENGTH: i64 = 55;
pub(crate) const EXCERPT_MORE: &[u8] = b" [&hellip;]";

impl Renderer<'_> {
    /// `get_the_excerpt()` for `post`, the loop's post.
    pub(crate) fn get_the_excerpt(&mut self, post: &Post) -> Vec<u8> {
        self.excerpt(post, EXCERPT_LENGTH, EXCERPT_MORE)
    }

    /// `render_block_core_post_excerpt()`: the excerpt, which the block's
    /// own more link and length rule, made here for the block callback.
    pub(crate) fn post_excerpt(&mut self, block: &Block) -> Vec<u8> {
        let context = &self.context;
        let Some(post) = context.post_id.and_then(|id| context.site.post(id)) else {
            return Vec::new();
        };
        let name = "core/post-excerpt";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let has_more = prepared(name, parsed)
            .get_str("moreText")
            .is_some_and(|text| !text.is_empty());
        // The more link replaces `excerpt_more`; 101 words leave the block's
        // length to trim.
        let more = if has_more { &b""[..] } else { EXCERPT_MORE };
        let excerpt = self.excerpt(&post, 101, more);
        render(&self.context, name, parsed, &excerpt).unwrap_or_default()
    }

    /// `get_the_excerpt()` with `excerpt_length` and `excerpt_more` as given.
    pub(crate) fn excerpt(&mut self, post: &Post, length: i64, more: &[u8]) -> Vec<u8> {
        if !post.password.is_empty() {
            return b"There is no excerpt because this is a protected post.".to_vec();
        }
        // wp_trim_excerpt()
        if !php_trim(&post.excerpt).is_empty() {
            return post.excerpt.clone();
        }
        let content = get_the_content(self, post, false);
        let content = self.excerpt_remove_blocks(&content);
        let content = excerpt_remove_footnotes(&content);
        let content = replace(
            &self.content_filters(&content, true, false),
            b"]]>",
            b"]]&gt;",
        );
        wp_trim_words(&content, length, more)
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
pub(crate) fn wp_trim_words(text: &[u8], num_words: i64, more: &[u8]) -> Vec<u8> {
    let text = wp_strip_all_tags(text, false);
    let words: Vec<&[u8]> = text
        .split(|b| matches!(b, b'\n' | b'\r' | b'\t' | b' '))
        .filter(|word| !word.is_empty())
        .collect();
    // A negative count splits everything, then always drops the last word.
    let kept = match usize::try_from(num_words) {
        Ok(n) => words.get(..n).filter(|_| words.len() > n),
        Err(_) => words.get(..words.len().saturating_sub(1)),
    };
    match kept {
        Some(kept) => [&kept.join(&b' ')[..], more].concat(),
        None => words.join(&b' '),
    }
}

#[cfg(test)]
mod tests;
