// SPDX-License-Identifier: AGPL-3.0-or-later

//! Rendering a block template, as `get_the_block_template_html()` and
//! `do_blocks()` do: template parts and patterns are expanded, dynamic
//! blocks run their render callbacks, and the others render their saved
//! markup with their inner blocks put back in place.

use std::collections::HashMap;

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::{esc_html, esc_url};
use hyscribe_phpser::{Value, is_empty_string, lookup, php_trim};

use crate::bindings::{computed_attributes, replace_html};
use crate::blocks::{Block, parse_blocks};
use crate::core_blocks::{self, Context, prepared};
use crate::elements::elements_class;
use crate::excerpt::visible;
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_smilies, wptexturize};
use crate::head::Assets;
use crate::interactivity::process_directives;
use crate::layout::{LayoutBlock, render_layout};
use crate::media::Loading;
use crate::registry::block_type;
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::template_types::PART_AREAS;
use crate::theme::Theme;
use crate::variations::variation_class;

/// `convert_smilies()`, which does nothing unless the site's
/// `use_smilies` option is set.
pub(crate) fn site_smilies(site: &dyn Site, text: &[u8]) -> Vec<u8> {
    smilies(&|name| site.option(name), text)
}

/// `convert_smilies()` when the site has smilies on, over its options.
pub(crate) fn smilies(option: &dyn Fn(&str) -> Option<Vec<u8>>, text: &[u8]) -> Vec<u8> {
    if option("use_smilies").is_some_and(|v| !is_empty_string(&v)) {
        let siteurl = option("siteurl").unwrap_or_default();
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
    // `get_the_block_template_html()` autoembeds before `do_blocks()`.
    let embedded = renderer.autoembed(template);
    let rendered = renderer.render(&embedded);
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

/// `block_core_details_set_img_fetchpriority_low()`: a details block shown
/// closed loads its images with low priority.
fn details_images(html: &[u8], block: &Block) -> Vec<u8> {
    let open = block
        .attrs
        .as_deref()
        .and_then(|attrs| lookup(attrs, "showContent"))
        .is_some_and(|v| !v.is_empty());
    if open {
        return html.to_vec();
    }
    low_priority_images(html)
}

/// Every `img` with `fetchpriority="low"`.
pub(crate) fn low_priority_images(html: &[u8]) -> Vec<u8> {
    let mut tags = TagProcessor::new(html);
    while tags.next_tag(Some(b"img"), None) {
        tags.set_attribute(b"fetchpriority", b"low");
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
pub fn replace(subject: &[u8], search: &[u8], with: &[u8]) -> Vec<u8> {
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

/// Blocks rendered inside this many others render nothing. Each parse
/// counts its own nesting up to [`crate::blocks::MAX_BLOCK_DEPTH`], but
/// synced patterns, post content and template parts parse again, so
/// rendering counts across them: twice the parser's depth leaves room for a
/// template around the deepest content and still fits a 2 MiB worker stack.
pub(crate) const MAX_RENDER_DEPTH: usize = 2 * crate::blocks::MAX_BLOCK_DEPTH;

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
    /// Posts the latest posts block is rendering in full, its own
    /// `$rendering_stack`.
    pub(crate) latest_posts_open: Vec<u64>,
    /// The categories blocks rendered, which number their dropdowns.
    pub(crate) categories_blocks: u64,
    /// `WP_Widget_Categories::widget()`'s `static $first_dropdown`, spent
    /// once a categories widget has printed its dropdown.
    pub(crate) categories_dropdown_used: bool,
    /// `WP_Widget_Calendar::$instance`: whether a calendar widget has
    /// printed, after which none is `#calendar_wrap`.
    pub(crate) calendar_widget_shown: bool,
    /// `WP_Widget_Recent_Comments::widget()`'s `static $first_instance`,
    /// spent once a recent comments widget has printed `#recentcomments`.
    pub(crate) recent_comments_shown: bool,
    /// `gallery_shortcode()`'s `static $instance`, counting its galleries.
    pub(crate) gallery_instances: u64,
    /// The `layout` attribute of the block being rendered, `parentLayout`
    /// to its inner blocks.
    pub(crate) parent_layout: Option<Value>,
    /// The last number `wp_unique_id()` gave out.
    pub(crate) unique_id: u64,
    /// `wp_unique_prefixed_id( 'wp-elements-' )`'s counter.
    pub(crate) elements_id: u64,
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
    /// The `comments/paginationArrow` context of comments pagination links.
    pub(crate) pagination_arrow: Option<Value>,
    /// The prepared attributes of the social links block being rendered.
    pub(crate) social_links: Option<Value>,
    /// The query pagination block's attributes, its links' context.
    pub(crate) query_pagination: Option<Value>,
    /// The tabs block's context for its tab list and panels.
    pub(crate) tabs: Option<crate::tabs::TabsContext>,
    /// The tab panels counted so far, by tabs ID.
    pub(crate) tab_counters: crate::tabs::TabCounters,
    /// The synced patterns being rendered, by `ref`.
    pub(crate) open_refs: Vec<i64>,
    /// The playlist's prepared attributes, its tracks' context.
    pub(crate) playlist: Option<Value>,
    /// The `pattern/overrides` context a synced pattern gives its blocks.
    pub(crate) pattern_overrides: Option<Value>,
    /// The blocks being rendered, up to [`MAX_RENDER_DEPTH`].
    depth: usize,
    /// The attachment that is the global post, whose content
    /// `prepend_attachment()` adds to.
    pub(crate) attachment: Option<hyscribe_model::Post>,
    /// `wp_audio_shortcode()`'s `static $instance`.
    pub(crate) audio_instances: std::cell::Cell<u64>,
    /// `_restore_wpautop_hook()` has added `wpautop()` back after content
    /// with blocks, last of its priority, after `prepend_attachment()`.
    pub(crate) autop_restored: bool,
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
                comment_depth: 0,
                terms: Default::default(),
                loading: Loading::in_the_loop(request.singular),
            },
            assets: Assets::default(),
            open_parts: Vec::new(),
            open_patterns: Vec::new(),
            open_posts: Vec::new(),
            latest_posts_open: Vec::new(),
            categories_blocks: 0,
            categories_dropdown_used: false,
            calendar_widget_shown: false,
            recent_comments_shown: false,
            gallery_instances: 0,
            parent_layout: None,
            unique_id: 0,
            elements_id: 0,
            navigation_names: HashMap::new(),
            navigation: None,
            has_submenus: false,
            in_interactive: false,
            comment_alt: 0,
            comment_thread_alt: 0,
            comment_depth: 0,
            pagination_arrow: None,
            social_links: None,
            query_pagination: None,
            tabs: None,
            tab_counters: HashMap::new(),
            open_refs: Vec::new(),
            playlist: None,
            pattern_overrides: None,
            depth: 0,
            attachment: None,
            audio_instances: std::cell::Cell::new(0),
            autop_restored: false,
        }
    }

    /// `render_block_core_widget_group()` outside a classic sidebar, which a
    /// block theme has none of: the title, then the inner blocks alone.
    fn widget_group(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let mut html = prepared("core/widget-group", parsed)
            .get_str("title")
            .filter(|title| !title.is_empty())
            .map(|title| {
                format!(
                    "<h2 class=\"widget-title\">{}</h2>",
                    esc_html(&title.to_php_string())
                )
                .into_bytes()
            })
            .unwrap_or_default();
        html.extend_from_slice(b"<div class=\"wp-widget-group__inner-blocks\">");
        for inner in &block.inner_blocks {
            html.extend(self.block(inner));
        }
        html.extend_from_slice(b"</div>");
        html
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
        if self.depth == MAX_RENDER_DEPTH {
            return Vec::new();
        }
        self.depth += 1;
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
        self.depth -= 1;
        html
    }

    fn render_block(&mut self, block: &Block) -> Vec<u8> {
        let styled = self.elements(block);
        let block = styled.as_ref().unwrap_or(block);
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
            Some("core/block") => self.synced_pattern(block),
            Some("core/query") => self.query(block),
            // These give their inner blocks no context but their own.
            Some("core/post-template") => self.without_overrides(|r| r.post_template(block)),
            Some("core/tabs") => self.tabs(block),
            Some("core/playlist") => self.playlist(block),
            Some("core/playlist-track") => self.playlist_track(block),
            Some("core/tab-list") => self.tab_list(block),
            Some("core/tab-panel") => self.tab_panel(block),
            Some("core/terms-query") => self.terms_query(block),
            Some("core/term-template") => self.without_overrides(|r| r.term_template(block)),
            Some("core/post-content") => self.post_content(block),
            Some("core/navigation") => self.navigation(block),
            Some("core/search") => self.search(block),
            Some("core/archives") => self.archives(block),
            Some("core/latest-posts") => self.latest_posts(block),
            Some("core/rss") => self.rss(block),
            Some("core/categories") => self.categories(block),
            Some("core/navigation-link") => self.navigation_link(block),
            Some("core/navigation-submenu") => self.navigation_submenu(block),
            Some("core/accordion") => self.accordion(block),
            Some("core/widget-group") => self.widget_group(block),
            Some("core/legacy-widget") => self.legacy_widget(block),
            Some("core/accordion-item") => self.accordion_item(block),
            Some("core/gallery") => self.gallery(block),
            Some("core/file") => self.file(block),
            Some("core/media-text") => self.media_text(block),
            Some("core/home-link") => self.home_link(block),
            Some("core/page-list") => self.page_list(block),
            Some("core/comment-template") => self.without_overrides(|r| r.comment_template(block)),
            Some("core/comments-pagination") => self.comments_pagination(block),
            Some("core/comments-pagination-previous") => self.comments_pagination_previous(block),
            Some("core/comments-pagination-numbers") => self.comments_pagination_numbers(block),
            Some("core/comments-pagination-next") => self.comments_pagination_next(block),
            Some("core/post-excerpt") => self.post_excerpt(block),
            Some("core/post-time-to-read") => self.post_time_to_read(block),
            Some("core/social-links") => self.social_links(block),
            Some("core/social-link") => self.social_link(block),
            Some("core/query-pagination") => self.query_pagination(block),
            Some("core/query-pagination-previous") => self.query_pagination_previous(block),
            Some("core/query-pagination-next") => self.query_pagination_next(block),
            Some("core/query-pagination-numbers") => self.query_pagination_numbers(block),
            Some(name) => {
                let parsed = block.attrs.as_deref().unwrap_or_default();
                let overrides = self.pattern_overrides.as_ref();
                let computed = computed_attributes(&self.context, block, overrides);
                let (mut content, mut inner_blocks) = self.saved_markup_at(block);
                if !content.is_empty() {
                    for (attribute, value) in &computed {
                        let replaced = replace_html(
                            &self.context,
                            name,
                            &content,
                            attribute,
                            value,
                            &inner_blocks,
                        );
                        // Inner blocks' offsets hold only while the markup is unchanged.
                        if replaced != content {
                            inner_blocks.clear();
                        }
                        content = replaced;
                    }
                }
                // The form's reply links move it with comment-reply.js.
                if name == "core/post-comments-form" {
                    self.assets.enqueue_script("comment-reply");
                }
                core_blocks::render_bound(&self.context, name, parsed, &computed, &content)
                    .unwrap_or(content)
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
        let types = block.name.as_deref().and_then(block_type);
        for id in types.map_or(&[][..], |t| &t.view_script_modules) {
            self.assets.enqueue_module(id);
        }
        let html = elements_class(html, block);
        let html = self.layout(block, html);
        let html = crate::dimensions::dimensions(html, block);
        // The block visibility support hides a block set not to show.
        let hidden = types.is_some() && !visible(block);
        let html = if hidden { Vec::new() } else { html };
        let html = variation_class(html, block);
        let html = match block.name.as_deref() {
            Some("core/paragraph") => paragraph_class(&html),
            Some("core/details") => details_images(&html, block),
            _ => html,
        };
        if root_interactive {
            self.in_interactive = false;
            return process_directives(&html);
        }
        html
    }

    /// Renders without a synced pattern's overrides in context.
    fn without_overrides(&mut self, render: impl FnOnce(&mut Self) -> Vec<u8>) -> Vec<u8> {
        let overrides = self.pattern_overrides.take();
        let html = render(self);
        self.pattern_overrides = overrides;
        html
    }

    /// `do_blocks()` on content of its own, whose blocks have no parent.
    pub(crate) fn render_top_level(&mut self, markup: &[u8]) -> Vec<u8> {
        let outer = self.parent_layout.take();
        let overrides = self.pattern_overrides.take();
        let html = self.render(markup);
        self.parent_layout = outer;
        self.pattern_overrides = overrides;
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
        self.saved_markup_at(block).0
    }

    /// The saved markup, with where each inner block's output starts.
    fn saved_markup_at(&mut self, block: &Block) -> (Vec<u8>, Vec<usize>) {
        let mut out = Vec::new();
        let mut starts = Vec::new();
        let mut inner = block.inner_blocks.iter();
        for chunk in &block.inner_content {
            match chunk {
                Some(html) => out.extend_from_slice(html),
                None => {
                    starts.push(out.len());
                    if let Some(child) = inner.next() {
                        out.extend(self.block(child));
                    }
                }
            }
        }
        (out, starts)
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
        let embedded = self.autoembed(content);
        let html = self.render_top_level(&embedded);
        self.open_patterns.pop();
        html
    }

    /// `render_block_core_block()`: a published synced pattern's blocks,
    /// once in a chain of patterns. Not ported yet: its embeds, which wait
    /// on outbound fetches, and pattern overrides, which need block
    /// bindings.
    fn synced_pattern(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let id = prepared("core/block", parsed)
            .get_str("ref")
            .map_or(0, Value::to_int);
        let pattern = u64::try_from(id)
            .ok()
            .and_then(|id| self.context.site.post(id))
            .filter(|post| post.post_type == "wp_block");
        let Some(pattern) = pattern else {
            return Vec::new();
        };
        if self.open_refs.contains(&id)
            || pattern.status != "publish"
            || !is_empty_string(&pattern.password)
        {
            return Vec::new();
        }
        self.open_refs.push(id);
        // Its blocks get its own `content`, empty by default, which its
        // normalizing of older formats never reaches.
        let content = prepared("core/block", parsed).get_str("content").cloned();
        let outer = std::mem::replace(&mut self.pattern_overrides, content);
        let embedded = self.autoembed(&pattern.content);
        let html = self.render(&embedded);
        self.pattern_overrides = outer;
        self.open_refs.pop();
        html
    }
}

/// The tag `get_allowed_block_template_part_areas()` gives an area.
fn area_tag(area: &[u8]) -> &'static [u8] {
    let known = PART_AREAS
        .iter()
        .find(|known| known.area.as_bytes() == area);
    known.map_or(b"div", |known| known.tag.as_bytes())
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
