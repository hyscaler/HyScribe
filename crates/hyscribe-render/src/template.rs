// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rendering a block template, as `get_the_block_template_html()` and
//! `do_blocks()` do: template parts and patterns are expanded, dynamic
//! blocks run their render callbacks, and the others render their saved
//! markup with their inner blocks put back in place.

use std::collections::HashMap;

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::esc_url;
use hyscribe_phpser::{Value, is_empty_string, lookup, php_trim};

use crate::blocks::{Block, parse_blocks};
use crate::core_blocks::{self, Context};
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_smilies, wptexturize};
use crate::head::Assets;
use crate::interactivity::process_directives;
use crate::layout::{LayoutBlock, render_layout};
use crate::media::Loading;
use crate::registry::block_type;
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::theme::Theme;
use crate::variations::variation_class;

/// `convert_smilies()`, which does nothing unless the site's
/// `use_smilies` option is set.
pub(crate) fn site_smilies(site: &dyn Site, text: &[u8]) -> Vec<u8> {
    if site
        .option("use_smilies")
        .is_some_and(|v| !is_empty_string(&v))
    {
        let siteurl = site.option("siteurl").unwrap_or_default();
        return convert_smilies(
            text,
            &esc_url(&[&siteurl[..], b"/wp-includes/images/smilies/"].concat()),
        );
    }
    text.to_vec()
}

/// `get_the_block_template_html()`: the template's blocks rendered, the
/// template filters run, inside `.wp-site-blocks`, with the skip link.
/// Images are not yet given `wp_filter_content_tags()`' attributes.
pub fn render_template(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    template: &[u8],
) -> (Vec<u8>, Assets) {
    let mut renderer = Renderer::new(theme, site, request);
    let rendered = renderer.render(template);
    let content = site_smilies(site, &wptexturize(&rendered, &CORE_SHORTCODE_TAGS));
    let html = [
        b"<div class=\"wp-site-blocks\">".as_slice(),
        &replace(&content, b"]]>", b"]]&gt;"),
        b"</div>",
    ]
    .concat();
    (add_skip_link(html), renderer.assets)
}

/// `block_core_paragraph_add_class()`: the first `<p>` gets the class
/// `wp-block-paragraph`.
fn paragraph_class(html: &[u8]) -> Vec<u8> {
    let mut tags = TagProcessor::new(html);
    if tags.next_tag(Some(b"p"), None) {
        tags.add_class(b"wp-block-paragraph");
    }
    tags.get_updated_html().to_vec()
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
pub(crate) fn replace(subject: &[u8], search: &[u8], with: &[u8]) -> Vec<u8> {
    if search.is_empty() {
        return subject.to_vec();
    }
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
    Renderer::new(theme, site, request).render(content)
}

pub(crate) struct Renderer<'a> {
    pub(crate) context: Context<'a>,
    /// The styles, block supports CSS and script modules rendering
    /// enqueued.
    pub(crate) assets: Assets,
    /// Template parts and patterns being rendered, so one that includes
    /// itself stops instead of recursing forever.
    pub(crate) open_parts: Vec<Vec<u8>>,
    pub(crate) open_patterns: Vec<Vec<u8>>,
    /// Posts whose content is being rendered, as `$seen_ids` tracks them.
    pub(crate) open_posts: Vec<u64>,
    /// The `layout` attribute of the block being rendered, `parentLayout`
    /// to its inner blocks.
    pub(crate) parent_layout: Option<Value>,
    /// The last number `wp_unique_id()` gave out.
    pub(crate) unique_id: u64,
    /// How often each navigation name has been seen, for unique labels.
    pub(crate) navigation_names: HashMap<Vec<u8>, u64>,
    /// The attributes of the navigation block being rendered, its inner
    /// blocks' context.
    pub(crate) navigation: Option<Value>,
    /// Whether a navigation block has had submenus; once set, it stays for
    /// the request, as PHP's static does.
    pub(crate) has_submenus: bool,
    /// An interactive block is rendering; its directives are processed
    /// when it ends, with every interactive block inside it.
    in_interactive: bool,
    /// `comment_class()`'s globals: comments and top-level comments seen,
    /// and the depth being rendered.
    pub(crate) comment_alt: u64,
    pub(crate) comment_thread_alt: u64,
    pub(crate) comment_depth: u64,
}

/// A string attribute.
fn attr<'b>(block: &'b Block, name: &str) -> Option<&'b [u8]> {
    lookup(block.attrs.as_deref()?, name).and_then(Value::as_bytes)
}

impl<'a> Renderer<'a> {
    /// A renderer for one request, with the main query's post as context.
    pub(crate) fn new(theme: &'a Theme, site: &'a dyn Site, request: &'a Request) -> Renderer<'a> {
        Renderer {
            context: Context {
                theme,
                site,
                request,
                post_id: request.post,
                query: None,
                comment: None,
                loading: Loading::in_the_loop(request.singular),
            },
            assets: Assets::default(),
            open_parts: Vec::new(),
            open_patterns: Vec::new(),
            open_posts: Vec::new(),
            parent_layout: None,
            unique_id: 0,
            navigation_names: HashMap::new(),
            navigation: None,
            has_submenus: false,
            in_interactive: false,
            comment_alt: 0,
            comment_thread_alt: 0,
            comment_depth: 0,
        }
    }

    /// `wp_unique_id( $prefix )`.
    pub(crate) fn unique_id(&mut self, prefix: &str) -> Vec<u8> {
        self.unique_id += 1;
        format!("{prefix}{}", self.unique_id).into_bytes()
    }
}

impl Renderer<'_> {
    /// `do_blocks()`.
    pub(crate) fn render(&mut self, markup: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for block in parse_blocks(markup) {
            out.extend(self.block(&block));
        }
        out
    }

    /// `render_block()` for one block.
    pub(crate) fn block(&mut self, block: &Block) -> Vec<u8> {
        let styles = self.assets.styles.len();
        let scripts = self.assets.scripts.len();
        let modules = self.assets.modules.len();
        let html = self.render_block(block);
        // A block that renders nothing dequeues what it and its inner
        // blocks enqueued.
        if php_trim(&html).is_empty() {
            self.assets.styles.truncate(styles);
            self.assets.scripts.truncate(scripts);
            self.assets.modules.truncate(modules);
        }
        html
    }

    fn render_block(&mut self, block: &Block) -> Vec<u8> {
        let varied = self.style_variation(block);
        let block = varied.as_ref().unwrap_or(block);
        let root_interactive = !self.in_interactive
            && block.name.as_deref().and_then(block_type).is_some_and(|t| {
                t.supports.get_str("interactivity") == Some(&Value::Bool(true))
                    || t.supports
                        .get_path(&["interactivity", "interactive"])
                        .map(Value::is_empty)
                        == Some(false)
            });
        self.in_interactive |= root_interactive;
        // Inner blocks get this block's layout as `parentLayout`.
        let own = block
            .attrs
            .as_deref()
            .and_then(|attrs| lookup(attrs, "layout"))
            .cloned();
        let parent = std::mem::replace(&mut self.parent_layout, own);
        let html = match block.name.as_deref() {
            Some("core/template-part") => self.template_part(block),
            Some("core/pattern") => self.pattern(block),
            Some("core/query") => self.query(block),
            Some("core/post-template") => self.post_template(block),
            Some("core/post-content") => self.post_content(block),
            Some("core/navigation") => self.navigation(block),
            Some("core/search") => self.search(block),
            Some("core/navigation-link") => self.navigation_link(block),
            Some("core/page-list") => self.page_list(block),
            Some("core/comment-template") => self.comment_template(block),
            Some(name) => {
                let content = self.saved_markup(block);
                // The form's reply links move it with comment-reply.js.
                if name == "core/post-comments-form" {
                    self.assets.enqueue_script("comment-reply");
                }
                let parsed = block.attrs.as_deref().unwrap_or_default();
                core_blocks::render(&self.context, name, parsed, &content).unwrap_or(content)
            }
            None => self.saved_markup(block),
        };
        self.parent_layout = parent;
        let theme = self.context.theme;
        for handle in block
            .name
            .as_deref()
            .and_then(|name| theme.style_handles.get(name))
            .into_iter()
            .flatten()
        {
            self.assets.enqueue_style(handle);
        }
        let html = self.layout(block, html);
        let html = variation_class(html, block);
        let html = match block.name.as_deref() {
            Some("core/paragraph") => paragraph_class(&html),
            _ => html,
        };
        if root_interactive {
            self.in_interactive = false;
            return process_directives(&html);
        }
        html
    }

    /// `do_blocks()` on content of its own, whose blocks have no parent.
    pub(crate) fn render_top_level(&mut self, markup: &[u8]) -> Vec<u8> {
        let outer = self.parent_layout.take();
        let html = self.render(markup);
        self.parent_layout = outer;
        html
    }

    /// The layout support's `render_block` filter, whose rules go to the
    /// block supports store.
    fn layout(&mut self, block: &Block, html: Vec<u8>) -> Vec<u8> {
        let Some((name, block_type)) = block
            .name
            .as_deref()
            .and_then(|name| Some((name, block_type(name)?)))
        else {
            return html;
        };
        let attributes = Value::Array(block.attrs.clone().unwrap_or_default());
        let first_chunk = match block.inner_content.as_slice() {
            [Some(first), _, ..] => Some(first.as_slice()),
            _ => None,
        };
        let layout_block = LayoutBlock {
            name,
            block_type,
            attributes: &attributes,
            parent_layout: self.parent_layout.as_ref(),
            first_chunk,
        };
        let (html, rules) = render_layout(&layout_block, &self.context.theme.global, html);
        self.assets.rules.extend(rules);
        html
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
        let html = self.render_top_level(content);
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
        let html = self.render_top_level(content);
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
