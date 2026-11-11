// SPDX-License-Identifier: AGPL-3.0-or-later

//! The term blocks on a term archive. The `termId` context comes with the
//! term template, not ported yet.

use hyscribe_kses::esc_url;
use hyscribe_model::Term;
use hyscribe_phpser::{Value, is_empty_string};

use super::{Block, Context, align_link_classes, element};
use crate::autop::wpautop;
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_chars, wptexturize};
use crate::site::{Archive, Taxonomy};

/// `render_block_core_term_description()`: the description through the
/// `term_description` display filters.
pub(super) fn term_description(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some((_, term)) = queried_term(context) else {
        return Vec::new();
    };
    let text = wptexturize(&term.description, &CORE_SHORTCODE_TAGS);
    let description = wpautop(&convert_chars(&text), true);
    if is_empty_string(&description) {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, &align_link_classes(block, Vec::new()));
    element(b"div", &wrapper, &description)
}

/// The queried term, as the term blocks take it outside a term template.
fn queried_term<'a>(context: &Context<'a>) -> Option<(Taxonomy, &'a Term)> {
    context.request.archive.as_ref().and_then(Archive::term)
}

/// `render_block_core_term_name()`: the name as stored, in a paragraph or
/// a heading of the block's level, linked to the term's archive if asked.
pub(super) fn term_name(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some((taxonomy, term)) = queried_term(context) else {
        return Vec::new();
    };
    let tag = super::tag(block.attribute("level"), b"p");
    let name = if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let link = crate::permalinks::term_link(context.site, taxonomy.name(), term);
        [
            &b"<a href=\""[..],
            &esc_url(&link),
            b"\">",
            &term.name,
            b"</a>",
        ]
        .concat()
    } else {
        term.name.clone()
    };
    let wrapper = block.wrapper(context, &align_link_classes(block, Vec::new()));
    element(&tag, &wrapper, &name)
}

/// `render_block_core_term_count()`: the term's post count in the block's
/// brackets.
pub(super) fn term_count(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some((_, term)) = queried_term(context) else {
        return Vec::new();
    };
    let count = term.count;
    let bracket = block.attribute("bracketType").map(Value::to_php_string);
    let count = match bracket.as_deref() {
        Some(b"round") => format!("({count})"),
        Some(b"square") => format!("[{count}]"),
        Some(b"curly") => format!("{{{count}}}"),
        Some(b"angle") => format!("<{count}>"),
        _ => count.to_string(),
    };
    element(b"div", &block.wrapper(context, b""), count.as_bytes())
}

#[cfg(test)]
mod tests;
