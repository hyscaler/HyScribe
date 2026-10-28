// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dynamic core blocks: the `render_block_core_*()` callbacks from
//! `wp-includes/blocks/`, run on the attributes as
//! `prepare_attributes_for_render()` leaves them.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string, isset, php_trim};

use crate::date::{self, Zone};
use crate::link;
use crate::post::the_title;
use crate::query_loop::query_posts;
use crate::registry::block_type;
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::theme::Theme;
use crate::title::archive_title;

/// What a render callback reads besides its block.
#[derive(Clone)]
pub(crate) struct Context<'a> {
    pub(crate) theme: &'a Theme,
    pub(crate) site: &'a dyn Site,
    pub(crate) request: &'a Request,
    /// The `postId` block context.
    pub(crate) post_id: Option<u64>,
    /// The context `core/query` provides.
    pub(crate) query: Option<QueryContext>,
}

/// The block context of a query block's descendants: `query`,
/// `displayLayout` and `enhancedPagination`.
#[derive(Clone, Debug, Default)]
pub(crate) struct QueryContext {
    pub(crate) query: Value,
    pub(crate) display_layout: Option<Value>,
    pub(crate) enhanced_pagination: bool,
}

/// One block being rendered: its name, parsed and prepared attributes, and
/// its inner blocks rendered, as `WP_Block::render()` passes `$content`.
struct Block<'a> {
    name: &'a str,
    parsed: &'a [(Key, Value)],
    attributes: Value,
    content: &'a [u8],
}

/// The attributes of `name` as `prepare_attributes_for_render()` leaves
/// them.
pub(crate) fn prepared(name: &str, parsed: &[(Key, Value)]) -> Value {
    Value::Array(
        block_type(name)
            .map(|t| t.prepare(parsed))
            .unwrap_or_default(),
    )
}

/// The output of `name`'s render callback, `None` for a block that renders
/// its saved markup.
pub(crate) fn render(
    context: &Context<'_>,
    name: &str,
    parsed: &[(Key, Value)],
    content: &[u8],
) -> Option<Vec<u8>> {
    let callback = match name {
        "core/post-date" => post_date,
        "core/post-title" => post_title,
        "core/query-no-results" => query_no_results,
        "core/query-pagination" => query_pagination,
        "core/query-title" => query_title,
        "core/site-tagline" => site_tagline,
        "core/site-title" => site_title,
        _ => return None,
    };
    let block = Block {
        name,
        parsed,
        attributes: prepared(name, parsed),
        content,
    };
    Some(callback(context, &block))
}

impl Block<'_> {
    fn attribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get_str(name).filter(|v| **v != Value::Null)
    }

    /// `get_block_wrapper_attributes( array( 'class' => $class ) )`.
    fn wrapper(&self, context: &Context<'_>, class: &[u8]) -> String {
        let extra = [("class", Value::String(class.to_vec()))];
        get_block_wrapper_attributes(self.name, self.parsed, &extra, &context.theme.global)
    }

    /// `isset( $attributes['style']['elements']['link']['color']['text'] )`.
    fn has_link_color(&self) -> bool {
        isset(
            self.attributes
                .get_path(&["style", "elements", "link", "color", "text"]),
        )
    }

    /// `has-text-align-<textAlign>`, or empty without an alignment.
    fn text_align_class(&self) -> Vec<u8> {
        match self.attribute("textAlign").filter(|v| !v.is_empty()) {
            Some(align) => [b"has-text-align-", &align.to_php_string()[..]].concat(),
            None => Vec::new(),
        }
    }
}

impl Context<'_> {
    /// The post in the `postId` context.
    fn post(&self) -> Option<Post> {
        self.post_id.and_then(|id| self.site.post(id))
    }

    /// `get_permalink( $post )`.
    fn permalink(&self, post: &Post) -> Vec<u8> {
        let option = |name| self.site.option(name).unwrap_or_default();
        let front_page = link::front_page(&option("show_on_front"), &option("page_on_front"));
        link::permalink(&self.site.home_url(), front_page, post)
    }

    /// `wp_timezone()`.
    fn zone(&self) -> Zone {
        let option = |name| self.site.option(name).unwrap_or_default();
        date::site_zone(&option("timezone_string"), &option("gmt_offset"))
    }
}

/// The classes post blocks share: `has-text-align-*` when `textAlign` is
/// set (`isset()`, where the site blocks test `empty()`), and
/// `has-link-color` when the link has a color.
fn post_classes(block: &Block<'_>, mut classes: Vec<Vec<u8>>) -> Vec<u8> {
    if let Some(align) = block.attribute("textAlign") {
        classes.push([b"has-text-align-", &align.to_php_string()[..]].concat());
    }
    if block.has_link_color() {
        classes.push(b"has-link-color".to_vec());
    }
    classes.join(&b' ')
}

/// `render_block_core_post_title()`.
fn post_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = context.post() else {
        return Vec::new();
    };
    let mut title = the_title(&post);
    if is_empty_string(&title) {
        return Vec::new();
    }
    let tag = tag(block.attribute("level"), b"h2");
    if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let rel = match block.attribute("rel").filter(|v| !v.is_empty()) {
            Some(rel) => format!("rel=\"{}\"", esc_attr(&rel.to_php_string())),
            None => String::new(),
        };
        let target = block
            .attribute("linkTarget")
            .map(Value::to_php_string)
            .unwrap_or_default();
        title = [
            b"<a href=\"".as_slice(),
            &esc_url(&context.permalink(&post)),
            b"\" target=\"",
            esc_attr(&target).as_bytes(),
            b"\" ",
            rel.as_bytes(),
            b">",
            &title,
            b"</a>",
        ]
        .concat();
    }
    element(
        &tag,
        &block.wrapper(context, &post_classes(block, Vec::new())),
        &title,
    )
}

/// `_block_bindings_post_data_get_value()`: `None` unless the post is
/// public and has no password.
fn post_data(context: &Context<'_>, field: &[u8]) -> Option<Vec<u8>> {
    let post = context
        .post()
        .filter(|post| post.status == "publish" && post.password.is_empty())?;
    let zone = context.zone();
    let iso = |stored: &str| esc_attr(date::format("c", stored, zone).as_bytes()).into_bytes();
    match field {
        b"date" => Some(iso(&post.date)),
        // Only a modified date later than the publish date shows.
        b"modified" => Some(
            if date::stored_unix(&post.modified, zone) > date::stored_unix(&post.date, zone) {
                iso(&post.modified)
            } else {
                Vec::new()
            },
        ),
        b"link" => Some(esc_url(&context.permalink(&post))),
        _ => None,
    }
}

/// `render_block_core_post_date()`, with the post data binding resolved as
/// `WP_Block` resolves it before rendering.
fn post_date(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let mut classes = Vec::new();
    let binding = block
        .attributes
        .get_path(&["metadata", "bindings", "datetime"])
        .filter(|b| isset(b.get_str("source")) && isset(b.get_str("args")));
    let datetime = match (binding, block.attribute("datetime")) {
        (Some(binding), _) => match binding
            .get_str("source")
            .map(Value::to_php_string)
            .as_deref()
        {
            Some(b"core/post-data") => post_data(
                context,
                &binding
                    .get_path(&["args", "field"])
                    .map(Value::to_php_string)
                    .unwrap_or_default(),
            ),
            _ => None,
        },
        (None, Some(datetime)) => Some(datetime.to_php_string()),
        (None, None) => {
            let modified = block
                .attribute("displayType")
                .map(Value::to_php_string)
                .as_deref()
                == Some(b"modified");
            if modified {
                classes.push(b"wp-block-post-date__modified-date".to_vec());
            }
            post_data(context, if modified { b"modified" } else { b"date" })
        }
    };
    let Some(datetime) = datetime.filter(|d| !is_empty_string(d)) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&datetime).into_owned();
    let timestamp = date::strtotime(&text);
    let format = block
        .attribute("format")
        .filter(|v| !v.is_empty())
        .map(Value::to_php_string);
    let formatted = match format.as_deref() {
        Some(b"human-diff") => {
            // A time `strtotime()` cannot read counts as 0, as PHP's false.
            let timestamp = timestamp.unwrap_or(0);
            let diff = date::human_time_diff(timestamp, context.request.now);
            if timestamp > context.request.now {
                format!("{diff} from now")
            } else {
                format!("{diff} ago")
            }
        }
        _ => {
            let format =
                format.unwrap_or_else(|| context.site.option("date_format").unwrap_or_default());
            timestamp
                .map(|t| date::format_unix(&String::from_utf8_lossy(&format), t, context.zone()))
                .unwrap_or_default()
        }
    };
    let mut time = [
        b"<time datetime=\"".as_slice(),
        esc_attr(&datetime).as_bytes(),
        b"\">",
        esc_html(formatted.as_bytes()).as_bytes(),
        b"</time>",
    ]
    .concat();
    if block.attribute("isLink").is_some_and(|v| !v.is_empty()) && context.post_id.is_some() {
        let link = context
            .post()
            .map(|post| context.permalink(&post))
            .unwrap_or_default();
        time = [
            b"<a href=\"".as_slice(),
            &esc_url(&link),
            b"\">",
            &time,
            b"</a>",
        ]
        .concat();
    }
    let wrapper = block.wrapper(context, &post_classes(block, classes));
    [
        b"<div ".as_slice(),
        wrapper.as_bytes(),
        b">",
        &time,
        b"</div>",
    ]
    .concat()
}

/// `has-link-color` when the block's link has a color, else empty.
fn link_color_class(block: &Block<'_>) -> &'static [u8] {
    if block.has_link_color() {
        b"has-link-color"
    } else {
        b""
    }
}

/// `render_block_core_query_no_results()`: the content, when the query
/// finds nothing.
fn query_no_results(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    if php_trim(block.content).is_empty() || !query_posts(context).is_empty() {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, link_color_class(block));
    [
        b"<div ".as_slice(),
        wrapper.as_bytes(),
        b">",
        block.content,
        b"</div>",
    ]
    .concat()
}

/// `render_block_core_query_pagination()`.
fn query_pagination(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    if php_trim(block.content).is_empty() {
        return Vec::new();
    }
    let extra = [
        ("aria-label", Value::String(b"Pagination".to_vec())),
        ("class", Value::String(link_color_class(block).to_vec())),
    ];
    let wrapper =
        get_block_wrapper_attributes(block.name, block.parsed, &extra, &context.theme.global);
    [
        b"<nav ".as_slice(),
        wrapper.as_bytes(),
        b">",
        block.content,
        b"</nav>",
    ]
    .concat()
}

/// `get_post_type_object( $type )->labels->singular_name` for the core
/// post types.
const POST_TYPE_NAMES: [(&[u8], &[u8]); 16] = [
    (b"post", b"Post"),
    (b"page", b"Page"),
    (b"attachment", b"Media"),
    (b"revision", b"Revision"),
    (b"nav_menu_item", b"Navigation Menu Item"),
    (b"custom_css", b"Custom CSS"),
    (b"customize_changeset", b"Changeset"),
    (b"oembed_cache", b"oEmbed Response"),
    (b"user_request", b"User Request"),
    (b"wp_block", b"Pattern"),
    (b"wp_template", b"Template"),
    (b"wp_template_part", b"Template Part"),
    (b"wp_global_styles", b"Global Styles"),
    (b"wp_navigation", b"Navigation Menu"),
    (b"wp_font_family", b"Font Family"),
    (b"wp_font_face", b"Font Face"),
];

/// `render_block_core_query_title()`.
fn query_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let request = context.request;
    let kind = block.attribute("type").map(Value::to_php_string);
    let post_type = context
        .query
        .as_ref()
        .and_then(|q| q.query.get_str("postType"))
        .filter(|v| **v != Value::Null)
        .map(Value::to_php_string)
        .or_else(|| context.post().map(|post| post.post_type.into_bytes()));
    let shown = match kind.as_deref() {
        None => false,
        Some(b"archive") => request.archive.is_some(),
        Some(b"search") => request.search.is_some(),
        Some(b"post-type") => post_type.as_deref().is_some_and(|t| !is_empty_string(t)),
        Some(_) => true,
    };
    if !shown {
        return Vec::new();
    }
    let show_prefix = block.attribute("showPrefix").is_none_or(|v| !v.is_empty());
    let mut title = Vec::new();
    if let Some(archive) = &request.archive {
        let date_format = context.site.option("date_format").unwrap_or_default();
        let posted = request.posts.first().map_or("", |post| post.date.as_str());
        title = archive_title(archive, posted, context.zone(), &date_format, show_prefix);
    }
    if let Some(search) = &request.search {
        title = b"Search results".to_vec();
        if block
            .attribute("showSearchTerm")
            .is_some_and(|v| !v.is_empty())
        {
            title = [
                b"Search results for: &#8220;".as_slice(),
                esc_attr(search).as_bytes(),
                b"&#8221;",
            ]
            .concat();
        }
    }
    if kind.as_deref() == Some(b"post-type") {
        let Some((_, name)) = POST_TYPE_NAMES
            .iter()
            .find(|(slug, _)| Some(*slug) == post_type.as_deref())
        else {
            return Vec::new();
        };
        let name = *name;
        title = if show_prefix {
            [b"Post Type: &#8220;", name, b"&#8221;"].concat()
        } else {
            name.to_vec()
        };
    }
    let tag = tag(block.attribute("level"), b"h1");
    element(
        &tag,
        &block.wrapper(context, &block.text_align_class()),
        &title,
    )
}

/// The tag for a `level` attribute: `p` for 0, else `'h' . (int) $level`,
/// and `default` without one.
fn tag(level: Option<&Value>, default: &[u8]) -> Vec<u8> {
    match level {
        Some(Value::Int(0)) => b"p".to_vec(),
        // Every caller registers a default level, so `default` is a backstop.
        _ => level.map_or(default.to_vec(), |level| {
            format!("h{}", level.to_int()).into_bytes()
        }),
    }
}

/// `sprintf( '<%1$s %2$s>%3$s</%1$s>', $tag, $wrapper, $content )`.
fn element(tag: &[u8], wrapper: &str, content: &[u8]) -> Vec<u8> {
    [
        b"<",
        tag,
        b" ",
        wrapper.as_bytes(),
        b">",
        content,
        b"</",
        tag,
        b">",
    ]
    .concat()
}

/// `render_block_core_site_title()`.
fn site_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let title = context.site.option("blogname").unwrap_or_default();
    if is_empty_string(php_trim(&title)) {
        return Vec::new();
    }
    let mut classes = block.text_align_class();
    if block.has_link_color() {
        classes.extend_from_slice(b" has-link-color");
    }
    let tag = tag(block.attribute("level"), b"h1");
    let content = if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let request = context.request;
        let page_for_posts = context
            .site
            .option("page_for_posts")
            .map_or(0, |v| Value::String(v).to_int());
        let current = !request.paged
            && (request.front_page
                || request.home
                    && u64::try_from(page_for_posts).ok() != Some(request.queried_object_id));
        let target = block
            .attribute("linkTarget")
            .filter(|v| !v.is_empty())
            .map_or(b"_self".to_vec(), Value::to_php_string);
        [
            b"<a href=\"".as_slice(),
            &esc_url(&context.site.home_url()),
            b"\" target=\"",
            esc_attr(&target).as_bytes(),
            b"\" rel=\"home\"",
            if current {
                b" aria-current=\"page\""
            } else {
                b""
            },
            b">",
            esc_html(&title).as_bytes(),
            b"</a>",
        ]
        .concat()
    } else {
        esc_html(&title).into_bytes()
    };
    element(&tag, &block.wrapper(context, php_trim(&classes)), &content)
}

/// `render_block_core_site_tagline()`. The tagline prints as stored.
fn site_tagline(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let tagline = context.site.option("blogdescription").unwrap_or_default();
    if is_empty_string(&tagline) {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, &block.text_align_class());
    let tag = tag(block.attribute("level"), b"p");
    element(&tag, &wrapper, &tagline)
}

#[cfg(test)]
mod tests;
