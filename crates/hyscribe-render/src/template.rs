// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rendering a block template, as `get_the_block_template_html()` and
//! `do_blocks()` do: template parts and patterns are expanded, dynamic
//! blocks run their render callbacks, and the others render their saved
//! markup with their inner blocks put back in place.

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::esc_url;
use hyscribe_phpser::{Value, lookup};

use crate::blocks::{Block, parse_blocks};
use crate::core_blocks::{self, Context};
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_smilies, wptexturize};
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::theme::Theme;

/// `get_the_block_template_html()`: the template's blocks rendered, the
/// template filters run, inside `.wp-site-blocks`, with the skip link.
/// Images are not yet given `wp_filter_content_tags()`' attributes.
pub fn render_template(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    template: &[u8],
) -> Vec<u8> {
    let mut content = wptexturize(
        &render_content(theme, site, request, template),
        &CORE_SHORTCODE_TAGS,
    );
    if site
        .option("use_smilies")
        .is_some_and(|v| !Value::String(v).is_empty())
    {
        let siteurl = site.option("siteurl").unwrap_or_default();
        let smilies = esc_url(&[&siteurl[..], b"/wp-includes/images/smilies/"].concat());
        content = convert_smilies(&content, &smilies);
    }
    let html = [
        b"<div class=\"wp-site-blocks\">".as_slice(),
        &replace(&content, b"]]>", b"]]&gt;"),
        b"</div>",
    ]
    .concat();
    add_skip_link(html)
}

/// `_block_template_add_skip_link()`: the first `<main>` gets an id if it
/// has none, and a link to it goes before `.wp-site-blocks`.
fn add_skip_link(html: Vec<u8>) -> Vec<u8> {
    let mut tags = TagProcessor::new(&html);
    if !tags.next_tag(Some(b"main"), None) {
        return html;
    }
    let id = match tags.get_attribute(b"id") {
        Some(Attribute::Value(id)) if !id.is_empty() => id,
        _ => {
            tags.set_attribute(b"id", b"wp--skip-link--target");
            b"wp--skip-link--target".to_vec()
        }
    };
    [
        b"<a class=\"skip-link screen-reader-text\" id=\"wp-skip-link\" href=\"".as_slice(),
        &esc_url(&[b"#", &id[..]].concat()),
        b"\">Skip to content</a>",
        tags.get_updated_html(),
    ]
    .concat()
}

/// `str_replace( $search, $with, $subject )`.
fn replace(subject: &[u8], search: &[u8], with: &[u8]) -> Vec<u8> {
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

/// Renders post content: `do_blocks()` on `post_content`.
pub fn render_content(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    content: &[u8],
) -> Vec<u8> {
    Renderer {
        context: Context {
            theme,
            site,
            request,
            post_id: request.post,
            query: None,
        },
        open_parts: Vec::new(),
        open_patterns: Vec::new(),
    }
    .render(content)
}

pub(crate) struct Renderer<'a> {
    pub(crate) context: Context<'a>,
    /// Template parts and patterns being rendered, so one that includes
    /// itself stops instead of recursing forever.
    open_parts: Vec<Vec<u8>>,
    open_patterns: Vec<Vec<u8>>,
}

/// A string attribute.
fn attr<'b>(block: &'b Block, name: &str) -> Option<&'b [u8]> {
    lookup(block.attrs.as_deref()?, name).and_then(Value::as_bytes)
}

impl Renderer<'_> {
    /// `do_blocks()`.
    fn render(&mut self, markup: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for block in parse_blocks(markup) {
            out.extend(self.block(&block));
        }
        out
    }

    /// `render_block()` for one block.
    fn block(&mut self, block: &Block) -> Vec<u8> {
        match block.name.as_deref() {
            Some("core/template-part") => self.template_part(block),
            Some("core/pattern") => self.pattern(block),
            Some("core/query") => self.query(block),
            Some("core/post-template") => self.post_template(block),
            Some(name) => {
                let content = self.saved_markup(block);
                let parsed = block.attrs.as_deref().unwrap_or_default();
                core_blocks::render(&self.context, name, parsed, &content).unwrap_or(content)
            }
            None => self.saved_markup(block),
        }
    }

    /// The block's own markup with its inner blocks rendered in place.
    pub(crate) fn saved_markup(&mut self, block: &Block) -> Vec<u8> {
        let mut out = Vec::new();
        let mut inner = block.inner_blocks.iter();
        for chunk in &block.inner_content {
            match chunk {
                Some(html) => out.extend_from_slice(html),
                None => {
                    if let Some(child) = inner.next() {
                        out.extend(self.block(child));
                    }
                }
            }
        }
        out
    }

    /// `render_block_core_template_part()`.
    fn template_part(&mut self, block: &Block) -> Vec<u8> {
        let theme = self.context.theme;
        let other_theme = attr(block, "theme").is_some_and(|t| t != theme.slug().as_bytes());
        let found = attr(block, "slug")
            .filter(|_| !other_theme)
            .and_then(|slug| Some((slug, theme.part(slug)?)));
        let Some((slug, (content, area))) = found else {
            return Vec::new();
        };
        if self.open_parts.iter().any(|open| open == slug) {
            return Vec::new();
        }
        self.open_parts.push(slug.to_vec());
        let html = self.render(content);
        self.open_parts.pop();
        // A valid `tagName` wins; otherwise the area's tag.
        let tag = match attr(block, "tagName").filter(|t| is_tag_name(t)) {
            Some(tag) => tag,
            None => area_tag(area),
        };
        let wrapper = get_block_wrapper_attributes(
            "core/template-part",
            block.attrs.as_deref().unwrap_or_default(),
            &[],
            &theme.global,
        );
        [
            b"<",
            tag,
            b" ",
            wrapper.as_bytes(),
            b">",
            &html,
            b"</",
            tag,
            b">",
        ]
        .concat()
    }

    /// `render_block_core_pattern()`.
    fn pattern(&mut self, block: &Block) -> Vec<u8> {
        let theme = self.context.theme;
        let Some((slug, content)) =
            attr(block, "slug").and_then(|slug| Some((slug, theme.pattern(slug)?)))
        else {
            return Vec::new();
        };
        if self.open_patterns.iter().any(|open| open == slug) {
            return Vec::new();
        }
        self.open_patterns.push(slug.to_vec());
        let html = self.render(content);
        self.open_patterns.pop();
        html
    }
}

/// The tag `get_allowed_block_template_part_areas()` gives an area.
fn area_tag(area: &[u8]) -> &'static [u8] {
    match area {
        b"header" => b"header",
        b"footer" => b"footer",
        _ => b"div",
    }
}

/// What survives `tag_escape()` unchanged: lower case letters, digits,
/// `_`, `:` and `-`.
fn is_tag_name(tag: &[u8]) -> bool {
    !tag.is_empty()
        && tag.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b':' | b'-')
        })
}

#[cfg(test)]
mod tests;
